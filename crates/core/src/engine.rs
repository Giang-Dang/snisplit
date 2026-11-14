use core::ops::Range;

use crate::domains::DomainList;
use crate::flow::{Cuts, FlowTable, PacketCuts, Progress};
use crate::http;
use crate::inspect::Inspection;
use crate::packet::parse_tcp;
use crate::segment;
use crate::strategy::Strategy;
use crate::tls;

const TLS_RECORD_HEADER_LEN: usize = 5;
const TLS_MAX_RECORD_LEN: usize = 1 << 14;
const DEFAULT_MAX_FLOWS: usize = 1024;
const DEFAULT_MAX_PREFIX_BYTES: usize = TLS_RECORD_HEADER_LEN + TLS_MAX_RECORD_LEN;
const DEFAULT_IDLE_TIMEOUT_MS: u64 = 30_000;

#[derive(Debug, Clone)]
pub struct Profile {
    pub domains: DomainList,
    pub tls: Option<Strategy>,
    pub http: Option<Strategy>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub max_flows: usize,
    pub max_prefix_bytes: usize,
    pub idle_timeout_ms: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_flows: DEFAULT_MAX_FLOWS,
            max_prefix_bytes: DEFAULT_MAX_PREFIX_BYTES,
            idle_timeout_ms: DEFAULT_IDLE_TIMEOUT_MS,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Pass,
    Replace(Vec<Vec<u8>>),
}

#[derive(Debug)]
pub struct Engine {
    profiles: Vec<Profile>,
    flows: FlowTable,
}

impl Engine {
    pub fn new(profiles: Vec<Profile>, limits: Limits) -> Self {
        Self {
            profiles,
            flows: FlowTable::new(limits),
        }
    }

    pub fn process(&mut self, packet: &[u8], now_ms: u64) -> Verdict {
        let Some(tcp) = parse_tcp(packet) else {
            return Verdict::Pass;
        };
        if tcp.payload.is_empty() {
            return Verdict::Pass;
        }
        let Self { profiles, flows } = self;
        let cuts = match flows.flow_for(&tcp, now_ms) {
            Some(flow) => flow.packet_cuts(tcp.sequence_number, tcp.payload, |prefix| {
                tls_progress(profiles, prefix)
            }),
            None => http_cuts(profiles, tcp.payload),
        };
        cuts.and_then(|cuts| segment::split(packet, &cuts.payload_offsets, cuts.order))
            .map_or(Verdict::Pass, Verdict::Replace)
    }
}

fn tls_progress(profiles: &[Profile], stream_prefix: &[u8]) -> Progress {
    match tls::inspect_client_hello(stream_prefix) {
        Inspection::NeedMoreData => Progress::NeedMoreData,
        Inspection::NoHost => Progress::Decided(None),
        Inspection::Host(host) => {
            Progress::Decided(cuts_for_host(profiles, stream_prefix, host, |profile| {
                profile.tls.as_ref()
            }))
        }
    }
}

fn http_cuts(profiles: &[Profile], payload: &[u8]) -> Option<PacketCuts> {
    let Inspection::Host(host) = http::inspect_request(payload) else {
        return None;
    };
    let cuts = cuts_for_host(profiles, payload, host, |profile| profile.http.as_ref())?;
    Some(PacketCuts {
        order: cuts.order,
        payload_offsets: cuts.stream_offsets,
    })
}

fn cuts_for_host(
    profiles: &[Profile],
    stream: &[u8],
    host_in_stream: Range<usize>,
    strategy_of: impl Fn(&Profile) -> Option<&Strategy>,
) -> Option<Cuts> {
    let host = stream.get(host_in_stream.clone())?;
    let (profile, matched_domain) = profiles.iter().find_map(|profile| {
        let matched_domain = profile.domains.matched_domain(host)?;
        Some((profile, matched_domain))
    })?;
    let strategy = strategy_of(profile)?;
    Some(Cuts {
        order: strategy.order,
        stream_offsets: strategy.stream_offsets(host_in_stream, host, matched_domain),
    })
}

#[cfg(test)]
mod tests;
