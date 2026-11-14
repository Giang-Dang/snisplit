use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use crate::reader::Reader;

const PROTOCOL_TCP: u8 = 6;
const BYTES_PER_WORD: usize = 4;
const IPV4_MIN_HEADER_LEN: usize = 20;
const IPV6_HEADER_LEN: usize = 40;
const TCP_MIN_HEADER_LEN: usize = 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TcpPacket<'a> {
    pub src: IpAddr,
    pub dst: IpAddr,
    pub src_port: u16,
    pub dst_port: u16,
    pub payload: &'a [u8],
}

struct IpHeader {
    src: IpAddr,
    dst: IpAddr,
    header_len: usize,
    total_len: usize,
}

struct TcpHeader {
    src_port: u16,
    dst_port: u16,
    header_len: usize,
}

pub fn parse_tcp(packet: &[u8]) -> Option<TcpPacket<'_>> {
    let ip = match packet.first()? >> 4 {
        4 => parse_ipv4_header(packet)?,
        6 => parse_ipv6_header(packet)?,
        _ => return None,
    };
    let segment = packet.get(ip.header_len..ip.total_len)?;
    let tcp = parse_tcp_header(segment)?;
    Some(TcpPacket {
        src: ip.src,
        dst: ip.dst,
        src_port: tcp.src_port,
        dst_port: tcp.dst_port,
        payload: segment.get(tcp.header_len..)?,
    })
}

fn parse_ipv4_header(packet: &[u8]) -> Option<IpHeader> {
    let mut reader = Reader::new(packet);
    let header_len = usize::from(reader.u8()? & 0x0f) * BYTES_PER_WORD;
    let _type_of_service = reader.u8()?;
    let total_len = usize::from(reader.u16()?);
    let _identification = reader.u16()?;
    let _flags_and_fragment_offset = reader.u16()?;
    let _time_to_live = reader.u8()?;
    let protocol = reader.u8()?;
    let _checksum = reader.u16()?;
    let src = Ipv4Addr::from(reader.array::<4>()?);
    let dst = Ipv4Addr::from(reader.array::<4>()?);

    let is_valid = header_len >= IPV4_MIN_HEADER_LEN
        && protocol == PROTOCOL_TCP
        && (header_len..=packet.len()).contains(&total_len);
    is_valid.then_some(IpHeader {
        src: src.into(),
        dst: dst.into(),
        header_len,
        total_len,
    })
}

fn parse_ipv6_header(packet: &[u8]) -> Option<IpHeader> {
    let mut reader = Reader::new(packet);
    let _version_traffic_class_flow_label = reader.u32()?;
    let payload_len = usize::from(reader.u16()?);
    let next_header = reader.u8()?;
    let _hop_limit = reader.u8()?;
    let src = Ipv6Addr::from(reader.array::<16>()?);
    let dst = Ipv6Addr::from(reader.array::<16>()?);

    let total_len = IPV6_HEADER_LEN + payload_len;
    let is_valid = next_header == PROTOCOL_TCP && total_len <= packet.len();
    is_valid.then_some(IpHeader {
        src: src.into(),
        dst: dst.into(),
        header_len: IPV6_HEADER_LEN,
        total_len,
    })
}

fn parse_tcp_header(segment: &[u8]) -> Option<TcpHeader> {
    let mut reader = Reader::new(segment);
    let src_port = reader.u16()?;
    let dst_port = reader.u16()?;
    let _sequence_number = reader.u32()?;
    let _acknowledgment_number = reader.u32()?;
    let header_len = usize::from(reader.u8()? >> 4) * BYTES_PER_WORD;
    (header_len >= TCP_MIN_HEADER_LEN).then_some(TcpHeader {
        src_port,
        dst_port,
        header_len,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const TCP_FLAGS_PSH_ACK: u8 = 0x18;
    const IPV4_VERSION_AND_MIN_HEADER: u8 = 0x45;
    const IPV6_VERSION: u8 = 0x60;

    fn tcp_segment(src_port: u16, dst_port: u16, payload: &[u8]) -> Vec<u8> {
        let header_words = (TCP_MIN_HEADER_LEN / BYTES_PER_WORD) as u8;
        let sequence_number = 0u32;
        let acknowledgment_number = 0u32;
        let window = u16::MAX;
        let checksum = 0u16;
        let urgent_pointer = 0u16;
        [
            src_port.to_be_bytes().as_slice(),
            &dst_port.to_be_bytes(),
            &sequence_number.to_be_bytes(),
            &acknowledgment_number.to_be_bytes(),
            &[header_words << 4, TCP_FLAGS_PSH_ACK],
            &window.to_be_bytes(),
            &checksum.to_be_bytes(),
            &urgent_pointer.to_be_bytes(),
            payload,
        ]
        .concat()
    }

    fn ipv4_packet(src: Ipv4Addr, dst: Ipv4Addr, segment: &[u8]) -> Vec<u8> {
        let total_len = (IPV4_MIN_HEADER_LEN + segment.len()) as u16;
        let identification = 0u16;
        let flags_and_fragment_offset = 0u16;
        let time_to_live = 64u8;
        let checksum = 0u16;
        [
            &[IPV4_VERSION_AND_MIN_HEADER, 0],
            total_len.to_be_bytes().as_slice(),
            &identification.to_be_bytes(),
            &flags_and_fragment_offset.to_be_bytes(),
            &[time_to_live, PROTOCOL_TCP],
            &checksum.to_be_bytes(),
            &src.octets(),
            &dst.octets(),
            segment,
        ]
        .concat()
    }

    fn ipv6_packet(src: Ipv6Addr, dst: Ipv6Addr, segment: &[u8]) -> Vec<u8> {
        let payload_len = segment.len() as u16;
        let hop_limit = 64u8;
        [
            &[IPV6_VERSION, 0, 0, 0],
            payload_len.to_be_bytes().as_slice(),
            &[PROTOCOL_TCP, hop_limit],
            &src.octets(),
            &dst.octets(),
            segment,
        ]
        .concat()
    }

    #[test]
    fn ipv4() {
        let dst = Ipv4Addr::new(1, 2, 3, 4);
        let packet = ipv4_packet(
            Ipv4Addr::new(10, 0, 0, 2),
            dst,
            &tcp_segment(50000, 443, b"hello"),
        );
        let tcp = parse_tcp(&packet).unwrap();
        assert_eq!(tcp.dst, IpAddr::from(dst));
        assert_eq!((tcp.src_port, tcp.dst_port), (50000, 443));
        assert_eq!(tcp.payload, b"hello");
    }

    #[test]
    fn ipv6() {
        let dst: Ipv6Addr = "2001:db8::1".parse().unwrap();
        let packet = ipv6_packet(Ipv6Addr::LOCALHOST, dst, &tcp_segment(50001, 80, b"hi"));
        let tcp = parse_tcp(&packet).unwrap();
        assert_eq!(tcp.dst, IpAddr::from(dst));
        assert_eq!(tcp.dst_port, 80);
        assert_eq!(tcp.payload, b"hi");
    }

    #[test]
    fn short_or_garbage_input_is_rejected() {
        for len in 0..60 {
            assert_eq!(parse_tcp(&vec![IPV4_VERSION_AND_MIN_HEADER; len]), None);
            assert_eq!(parse_tcp(&vec![IPV6_VERSION; len]), None);
        }
        assert_eq!(parse_tcp(&[0x10; 40]), None);
    }
}
