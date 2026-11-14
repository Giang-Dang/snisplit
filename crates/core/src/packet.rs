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
    pub sequence_number: u32,
    pub ip_header_len: usize,
    pub tcp_header_len: usize,
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
    sequence_number: u32,
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
        sequence_number: tcp.sequence_number,
        ip_header_len: ip.header_len,
        tcp_header_len: tcp.header_len,
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

    let is_valid = header_len >= IPV4_MIN_HEADER_LEN && protocol == PROTOCOL_TCP;
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

    (next_header == PROTOCOL_TCP).then_some(IpHeader {
        src: src.into(),
        dst: dst.into(),
        header_len: IPV6_HEADER_LEN,
        total_len: IPV6_HEADER_LEN + payload_len,
    })
}

fn parse_tcp_header(segment: &[u8]) -> Option<TcpHeader> {
    let mut reader = Reader::new(segment);
    let src_port = reader.u16()?;
    let dst_port = reader.u16()?;
    let sequence_number = reader.u32()?;
    let _acknowledgment_number = reader.u32()?;
    let header_len = usize::from(reader.u8()? >> 4) * BYTES_PER_WORD;
    (header_len >= TCP_MIN_HEADER_LEN).then_some(TcpHeader {
        src_port,
        dst_port,
        sequence_number,
        header_len,
    })
}

#[cfg(test)]
mod tests;
