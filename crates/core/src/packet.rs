//! Minimal IPv4/IPv6 + TCP header parsing.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use crate::reader::Reader;

const PROTO_TCP: u8 = 6;

/// The parts of a TCP/IP packet that snisplit needs to look at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TcpPacket<'a> {
    pub src: IpAddr,
    pub dst: IpAddr,
    pub src_port: u16,
    pub dst_port: u16,
    pub payload: &'a [u8],
}

/// Parses a raw IP packet (as delivered by WinDivert or NFQUEUE).
///
/// Returns `None` for anything that is not a well-formed TCP packet. IPv6
/// extension headers are not followed yet: packets with any are rejected.
pub fn parse_tcp(ip: &[u8]) -> Option<TcpPacket<'_>> {
    let version = ip.first()? >> 4;
    let (src, dst, l4_start, ip_end) = match version {
        4 => {
            let ihl = usize::from(ip[0] & 0x0f) * 4;
            let total_len = usize::from(u16::from_be_bytes([*ip.get(2)?, *ip.get(3)?]));
            if ihl < 20 || *ip.get(9)? != PROTO_TCP || total_len < ihl || total_len > ip.len() {
                return None;
            }
            let src: [u8; 4] = ip.get(12..16)?.try_into().ok()?;
            let dst: [u8; 4] = ip.get(16..20)?.try_into().ok()?;
            (
                Ipv4Addr::from(src).into(),
                Ipv4Addr::from(dst).into(),
                ihl,
                total_len,
            )
        }
        6 => {
            let payload_len = usize::from(u16::from_be_bytes([*ip.get(4)?, *ip.get(5)?]));
            let total_len = 40 + payload_len;
            if *ip.get(6)? != PROTO_TCP || total_len > ip.len() {
                return None;
            }
            let src: [u8; 16] = ip.get(8..24)?.try_into().ok()?;
            let dst: [u8; 16] = ip.get(24..40)?.try_into().ok()?;
            (
                Ipv6Addr::from(src).into(),
                Ipv6Addr::from(dst).into(),
                40,
                total_len,
            )
        }
        _ => return None,
    };

    let tcp = ip.get(l4_start..ip_end)?;
    let mut r = Reader::new(tcp);
    let src_port = r.u16()?;
    let dst_port = r.u16()?;
    r.skip(8)?; // seq, ack
    let data_offset = usize::from(r.u8()? >> 4) * 4;
    if data_offset < 20 {
        return None;
    }
    let payload = tcp.get(data_offset..)?;
    Some(TcpPacket {
        src,
        dst,
        src_port,
        dst_port,
        payload,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tcp_header(src_port: u16, dst_port: u16) -> Vec<u8> {
        let mut h = Vec::new();
        h.extend_from_slice(&src_port.to_be_bytes());
        h.extend_from_slice(&dst_port.to_be_bytes());
        h.extend_from_slice(&[0; 8]);
        h.push(5 << 4);
        h.extend_from_slice(&[0x18, 0xff, 0xff, 0, 0, 0, 0]);
        h
    }

    #[test]
    fn ipv4() {
        let payload = b"hello";
        let mut p = vec![0x45, 0, 0, 0, 0, 0, 0x40, 0, 64, PROTO_TCP, 0, 0];
        p.extend_from_slice(&[10, 0, 0, 2, 1, 2, 3, 4]);
        p.extend_from_slice(&tcp_header(50000, 443));
        p.extend_from_slice(payload);
        let len = p.len() as u16;
        p[2..4].copy_from_slice(&len.to_be_bytes());

        let t = parse_tcp(&p).unwrap();
        assert_eq!(t.dst, "1.2.3.4".parse::<IpAddr>().unwrap());
        assert_eq!((t.src_port, t.dst_port), (50000, 443));
        assert_eq!(t.payload, payload);
    }

    #[test]
    fn ipv6() {
        let payload = b"hi";
        let tcp = [tcp_header(50001, 80), payload.to_vec()].concat();
        let mut p = vec![0x60, 0, 0, 0];
        p.extend_from_slice(&(tcp.len() as u16).to_be_bytes());
        p.extend_from_slice(&[PROTO_TCP, 64]);
        p.extend_from_slice(&Ipv6Addr::LOCALHOST.octets());
        p.extend_from_slice(&"2001:db8::1".parse::<Ipv6Addr>().unwrap().octets());
        p.extend_from_slice(&tcp);

        let t = parse_tcp(&p).unwrap();
        assert_eq!(t.dst, "2001:db8::1".parse::<IpAddr>().unwrap());
        assert_eq!(t.dst_port, 80);
        assert_eq!(t.payload, payload);
    }

    #[test]
    fn short_or_garbage_input_is_rejected() {
        for len in 0..60 {
            let buf = vec![0x45; len];
            let _ = parse_tcp(&buf);
            let buf = vec![0x60; len];
            let _ = parse_tcp(&buf);
        }
        assert_eq!(parse_tcp(&[0x10; 40]), None);
    }
}
