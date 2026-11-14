use core::iter::once;
use core::ops::Range;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use crate::checksum::{checksum, ipv4_pseudo_header_sum, ipv6_pseudo_header_sum, tcp_checksum};
use crate::packet::{TcpPacket, parse_tcp};

const IPV4_TOTAL_LEN_OFFSET: usize = 2;
const IPV4_IDENTIFICATION_OFFSET: usize = 4;
const IPV4_FRAGMENT_FIELD_OFFSET: usize = 6;
const IPV4_CHECKSUM_OFFSET: usize = 10;
const IPV4_MORE_FRAGMENTS: u16 = 0x2000;
const IPV4_FRAGMENT_OFFSET_MASK: u16 = 0x1fff;
const IPV6_PAYLOAD_LEN_OFFSET: usize = 4;

const TCP_SEQUENCE_NUMBER_OFFSET: usize = 4;
const TCP_FLAGS_OFFSET: usize = 13;
const TCP_CHECKSUM_OFFSET: usize = 16;
const TCP_FLAG_FIN: u8 = 0x01;
const TCP_FLAG_SYN: u8 = 0x02;
const TCP_FLAG_RST: u8 = 0x04;
const TCP_FLAG_PSH: u8 = 0x08;
const TCP_FLAG_URG: u8 = 0x20;
const TCP_FLAGS_ONLY_ON_FINAL_SEGMENT: u8 = TCP_FLAG_PSH | TCP_FLAG_FIN;
const TCP_FLAGS_THAT_PREVENT_SPLITTING: u8 = TCP_FLAG_SYN | TCP_FLAG_URG | TCP_FLAG_RST;

const UNFILLED_CHECKSUM: [u8; 2] = [0, 0];
const FIRST_OFFSET_INSIDE_PAYLOAD: usize = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Order {
    InSequence,
    Reversed,
}

pub fn split(packet: &[u8], payload_offsets: &[usize], order: Order) -> Option<Vec<Vec<u8>>> {
    let original = Original::parse(packet)?;
    let bounds = piece_bounds(payload_offsets, original.payload.len())?;
    let pieces = bounds.iter().zip(bounds.iter().skip(1));
    let mut segments = pieces
        .enumerate()
        .map(|(segment_index, (&start, &end))| original.segment(segment_index, start..end))
        .collect::<Option<Vec<_>>>()?;
    if order == Order::Reversed {
        segments.reverse();
    }
    Some(segments)
}

fn piece_bounds(payload_offsets: &[usize], payload_len: usize) -> Option<Vec<usize>> {
    let mut cuts: Vec<usize> = payload_offsets
        .iter()
        .copied()
        .filter(|offset| (FIRST_OFFSET_INSIDE_PAYLOAD..payload_len).contains(offset))
        .collect();
    if cuts.is_empty() {
        return None;
    }
    cuts.sort_unstable();
    cuts.dedup();
    Some(once(0).chain(cuts).chain(once(payload_len)).collect())
}

struct Original<'a> {
    ip: Ip,
    headers: &'a [u8],
    tcp_start: usize,
    sequence_number: u32,
    flags: u8,
    payload: &'a [u8],
}

impl<'a> Original<'a> {
    fn parse(packet: &'a [u8]) -> Option<Self> {
        let tcp = parse_tcp(packet)?;
        let tcp_start = tcp.ip_header_len;
        let headers = packet.get(..tcp_start.checked_add(tcp.tcp_header_len)?)?;
        let flags = *headers.get(tcp_start.checked_add(TCP_FLAGS_OFFSET)?)?;
        if prevents_splitting(flags) {
            return None;
        }
        Some(Self {
            ip: Ip::parse(headers, &tcp)?,
            headers,
            tcp_start,
            sequence_number: tcp.sequence_number,
            flags,
            payload: tcp.payload,
        })
    }

    fn segment(&self, segment_index: usize, piece: Range<usize>) -> Option<Vec<u8>> {
        let sequence_number = self
            .sequence_number
            .wrapping_add(u32::try_from(piece.start).ok()?);
        let flags = self.flags_for_piece_ending_at(piece.end);
        let sequence_number_at = self.tcp_start.checked_add(TCP_SEQUENCE_NUMBER_OFFSET)?;
        let flags_at = self.tcp_start.checked_add(TCP_FLAGS_OFFSET)?;
        let checksum_at = self.tcp_start.checked_add(TCP_CHECKSUM_OFFSET)?;

        let mut segment = [self.headers, self.payload.get(piece)?].concat();
        write(
            &mut segment,
            sequence_number_at,
            sequence_number.to_be_bytes(),
        )?;
        write(&mut segment, flags_at, [flags])?;
        write(&mut segment, checksum_at, UNFILLED_CHECKSUM)?;
        self.ip
            .rewrite_header(&mut segment, self.tcp_start, segment_index)?;

        let tcp_segment = segment.get(self.tcp_start..)?;
        let pseudo_header_sum = self.ip.pseudo_header_sum(tcp_segment.len())?;
        let tcp_checksum = tcp_checksum(pseudo_header_sum, tcp_segment);
        write(&mut segment, checksum_at, tcp_checksum.to_be_bytes())?;
        Some(segment)
    }

    fn flags_for_piece_ending_at(&self, piece_end: usize) -> u8 {
        if piece_end == self.payload.len() {
            self.flags
        } else {
            self.flags & !TCP_FLAGS_ONLY_ON_FINAL_SEGMENT
        }
    }
}

fn prevents_splitting(flags: u8) -> bool {
    flags & TCP_FLAGS_THAT_PREVENT_SPLITTING != 0
}

enum Ip {
    V4 {
        src: Ipv4Addr,
        dst: Ipv4Addr,
        identification: u16,
    },
    V6 {
        src: Ipv6Addr,
        dst: Ipv6Addr,
    },
}

impl Ip {
    fn parse(headers: &[u8], tcp: &TcpPacket) -> Option<Self> {
        match (tcp.src, tcp.dst) {
            (IpAddr::V4(src), IpAddr::V4(dst)) => {
                let identification = read_u16(headers, IPV4_IDENTIFICATION_OFFSET)?;
                let fragment_field = read_u16(headers, IPV4_FRAGMENT_FIELD_OFFSET)?;
                (!is_ipv4_fragment(fragment_field)).then_some(Ip::V4 {
                    src,
                    dst,
                    identification,
                })
            }
            (IpAddr::V6(src), IpAddr::V6(dst)) => Some(Ip::V6 { src, dst }),
            _ => None,
        }
    }

    fn rewrite_header(
        &self,
        segment: &mut [u8],
        header_len: usize,
        segment_index: usize,
    ) -> Option<()> {
        match *self {
            Ip::V4 { identification, .. } => {
                let total_len = u16::try_from(segment.len()).ok()?;
                let identification =
                    identification.wrapping_add(u16::try_from(segment_index).ok()?);
                write(segment, IPV4_TOTAL_LEN_OFFSET, total_len.to_be_bytes())?;
                write(
                    segment,
                    IPV4_IDENTIFICATION_OFFSET,
                    identification.to_be_bytes(),
                )?;
                write(segment, IPV4_CHECKSUM_OFFSET, UNFILLED_CHECKSUM)?;
                let header_checksum = checksum(segment.get(..header_len)?);
                write(segment, IPV4_CHECKSUM_OFFSET, header_checksum.to_be_bytes())
            }
            Ip::V6 { .. } => {
                let payload_len = u16::try_from(segment.len().checked_sub(header_len)?).ok()?;
                write(segment, IPV6_PAYLOAD_LEN_OFFSET, payload_len.to_be_bytes())
            }
        }
    }

    fn pseudo_header_sum(&self, tcp_len: usize) -> Option<u64> {
        match *self {
            Ip::V4 { src, dst, .. } => {
                let tcp_len = u16::try_from(tcp_len).ok()?;
                Some(ipv4_pseudo_header_sum(src, dst, tcp_len))
            }
            Ip::V6 { src, dst } => {
                let tcp_len = u32::try_from(tcp_len).ok()?;
                Some(ipv6_pseudo_header_sum(src, dst, tcp_len))
            }
        }
    }
}

fn is_ipv4_fragment(fragment_field: u16) -> bool {
    fragment_field & (IPV4_MORE_FRAGMENTS | IPV4_FRAGMENT_OFFSET_MASK) != 0
}

fn read_u16(buf: &[u8], offset: usize) -> Option<u16> {
    let bytes = buf.get(offset..)?.first_chunk()?;
    Some(u16::from_be_bytes(*bytes))
}

fn write<const N: usize>(buf: &mut [u8], offset: usize, bytes: [u8; N]) -> Option<()> {
    let field = buf.get_mut(offset..offset.checked_add(N)?)?;
    field.copy_from_slice(&bytes);
    Some(())
}

#[cfg(test)]
mod tests;
