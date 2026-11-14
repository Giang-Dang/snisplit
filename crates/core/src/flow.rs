use core::ops::Range;
use std::collections::HashMap;
use std::net::IpAddr;

use crate::engine::Limits;
use crate::packet::TcpPacket;
use crate::segment::Order;

const TLS_CONTENT_TYPE_HANDSHAKE: u8 = 0x16;
const SMALLEST_DISTANCE_BEFORE_FIRST_BYTE: u32 = 1 << 31;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct FlowKey {
    src: IpAddr,
    src_port: u16,
    dst: IpAddr,
    dst_port: u16,
}

impl FlowKey {
    fn of(tcp: &TcpPacket) -> Self {
        Self {
            src: tcp.src,
            src_port: tcp.src_port,
            dst: tcp.dst,
            dst_port: tcp.dst_port,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cuts {
    pub order: Order,
    pub stream_offsets: Vec<usize>,
}

impl Cuts {
    fn without_offsets_inside(self, sent: &[Range<usize>]) -> Self {
        let stream_offsets = self
            .stream_offsets
            .into_iter()
            .filter(|&offset| !sent.iter().any(|range| strictly_inside(offset, range)))
            .collect();
        Self {
            order: self.order,
            stream_offsets,
        }
    }

    fn for_packet(&self, packet: Range<usize>) -> PacketCuts {
        let payload_offsets = self
            .stream_offsets
            .iter()
            .filter(|&&offset| strictly_inside(offset, &packet))
            .map(|&offset| offset - packet.start)
            .collect();
        PacketCuts {
            order: self.order,
            payload_offsets,
        }
    }
}

fn strictly_inside(offset: usize, range: &Range<usize>) -> bool {
    range.start < offset && offset < range.end
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PacketCuts {
    pub order: Order,
    pub payload_offsets: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Progress {
    NeedMoreData,
    Decided(Option<Cuts>),
}

#[derive(Debug)]
pub struct FlowTable {
    flows: HashMap<FlowKey, Flow>,
    limits: Limits,
}

impl FlowTable {
    pub fn new(limits: Limits) -> Self {
        Self {
            flows: HashMap::new(),
            limits,
        }
    }

    pub fn flow_for(&mut self, tcp: &TcpPacket, now_ms: u64) -> Option<&mut Flow> {
        let key = FlowKey::of(tcp);
        let idle_timeout_ms = self.limits.idle_timeout_ms;
        if self
            .flows
            .get(&key)
            .is_some_and(|flow| flow.is_idle(now_ms, idle_timeout_ms))
        {
            self.flows.remove(&key);
        }
        if !self.flows.contains_key(&key) {
            self.start_tracking(key, tcp, now_ms)?;
        }
        let flow = self.flows.get_mut(&key)?;
        flow.last_seen_ms = now_ms;
        Some(flow)
    }

    fn start_tracking(&mut self, key: FlowKey, tcp: &TcpPacket, now_ms: u64) -> Option<()> {
        if tcp.payload.first() != Some(&TLS_CONTENT_TYPE_HANDSHAKE) {
            return None;
        }
        if self.flows.len() >= self.limits.max_flows {
            self.expire_idle(now_ms);
        }
        if self.flows.len() >= self.limits.max_flows {
            return None;
        }
        let flow = Flow::new(tcp.sequence_number, now_ms, self.limits.max_prefix_bytes);
        self.flows.insert(key, flow);
        Some(())
    }

    fn expire_idle(&mut self, now_ms: u64) {
        let idle_timeout_ms = self.limits.idle_timeout_ms;
        self.flows
            .retain(|_, flow| !flow.is_idle(now_ms, idle_timeout_ms));
    }
}

#[derive(Debug)]
pub struct Flow {
    first_sequence_number: u32,
    last_seen_ms: u64,
    state: FlowState,
}

#[derive(Debug)]
enum FlowState {
    Collecting(StreamPrefix),
    Decided(Option<Cuts>),
}

impl Flow {
    fn new(first_sequence_number: u32, now_ms: u64, max_prefix_bytes: usize) -> Self {
        Self {
            first_sequence_number,
            last_seen_ms: now_ms,
            state: FlowState::Collecting(StreamPrefix::new(max_prefix_bytes)),
        }
    }

    fn is_idle(&self, now_ms: u64, idle_timeout_ms: u64) -> bool {
        now_ms.saturating_sub(self.last_seen_ms) >= idle_timeout_ms
    }

    pub fn packet_cuts(
        &mut self,
        sequence_number: u32,
        payload: &[u8],
        decide: impl FnOnce(&[u8]) -> Progress,
    ) -> Option<PacketCuts> {
        let start = self.stream_offset(sequence_number)?;
        let packet = start..start.saturating_add(payload.len());
        if let FlowState::Collecting(prefix) = &mut self.state {
            let sent_before = prefix.received.clone();
            prefix.absorb(start, payload);
            match decide(prefix.contiguous()) {
                Progress::NeedMoreData if prefix.is_full() => {
                    self.state = FlowState::Decided(None);
                }
                Progress::NeedMoreData => {}
                Progress::Decided(cuts) => {
                    let cuts = cuts.map(|cuts| cuts.without_offsets_inside(&sent_before));
                    self.state = FlowState::Decided(cuts);
                }
            }
        }
        match &self.state {
            FlowState::Decided(Some(cuts)) => Some(cuts.for_packet(packet)),
            FlowState::Decided(None) | FlowState::Collecting(_) => None,
        }
    }

    fn stream_offset(&self, sequence_number: u32) -> Option<usize> {
        let distance = sequence_number.wrapping_sub(self.first_sequence_number);
        if distance >= SMALLEST_DISTANCE_BEFORE_FIRST_BYTE {
            return None;
        }
        usize::try_from(distance).ok()
    }
}

#[derive(Debug)]
struct StreamPrefix {
    bytes: Vec<u8>,
    received: Vec<Range<usize>>,
    max_bytes: usize,
}

impl StreamPrefix {
    fn new(max_bytes: usize) -> Self {
        Self {
            bytes: Vec::new(),
            received: Vec::new(),
            max_bytes,
        }
    }

    fn absorb(&mut self, start: usize, payload: &[u8]) {
        let end = start.saturating_add(payload.len()).min(self.max_bytes);
        if start >= end {
            return;
        }
        let Some(kept) = payload.get(..end - start) else {
            return;
        };
        if self.bytes.len() < end {
            self.bytes.resize(end, 0);
        }
        if let Some(destination) = self.bytes.get_mut(start..end) {
            destination.copy_from_slice(kept);
            self.mark_received(start..end);
        }
    }

    fn mark_received(&mut self, range: Range<usize>) {
        self.received.push(range);
        self.received.sort_unstable_by_key(|range| range.start);
        let mut merged: Vec<Range<usize>> = Vec::with_capacity(self.received.len());
        for range in self.received.drain(..) {
            match merged.last_mut() {
                Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
                _ => merged.push(range),
            }
        }
        self.received = merged;
    }

    fn contiguous_len(&self) -> usize {
        match self.received.first() {
            Some(range) if range.start == 0 => range.end,
            _ => 0,
        }
    }

    fn contiguous(&self) -> &[u8] {
        self.bytes.get(..self.contiguous_len()).unwrap_or_default()
    }

    fn is_full(&self) -> bool {
        self.contiguous_len() >= self.max_bytes
    }
}

#[cfg(test)]
mod tests;
