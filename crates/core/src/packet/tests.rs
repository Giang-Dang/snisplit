use super::*;

const TCP_FLAGS_PSH_ACK: u8 = 0x18;
const IPV4_VERSION: u8 = 0x40;
const IPV4_VERSION_AND_MIN_HEADER: u8 = 0x45;
const IPV6_VERSION: u8 = 0x60;
const IPV4_OPTION_ROUTER_ALERT: [u8; 4] = [0x94, 0x04, 0x00, 0x00];
const TCP_OPTION_NO_OPERATION: u8 = 0x01;

fn tcp_segment(src_port: u16, dst_port: u16, payload: &[u8]) -> Vec<u8> {
    let sequence_number = 0u32;
    let options = [];
    tcp_segment_with_options(src_port, dst_port, sequence_number, &options, payload)
}

fn tcp_segment_with_options(
    src_port: u16,
    dst_port: u16,
    sequence_number: u32,
    options: &[u8],
    payload: &[u8],
) -> Vec<u8> {
    let header_words = ((TCP_MIN_HEADER_LEN + options.len()) / BYTES_PER_WORD) as u8;
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
        options,
        payload,
    ]
    .concat()
}

fn ipv4_packet(src: Ipv4Addr, dst: Ipv4Addr, segment: &[u8]) -> Vec<u8> {
    let options = [];
    ipv4_packet_with_options(src, dst, &options, segment)
}

fn ipv4_packet_with_options(
    src: Ipv4Addr,
    dst: Ipv4Addr,
    options: &[u8],
    segment: &[u8],
) -> Vec<u8> {
    let header_len = IPV4_MIN_HEADER_LEN + options.len();
    let header_words = (header_len / BYTES_PER_WORD) as u8;
    let total_len = (header_len + segment.len()) as u16;
    let identification = 0u16;
    let flags_and_fragment_offset = 0u16;
    let time_to_live = 64u8;
    let checksum = 0u16;
    [
        &[IPV4_VERSION | header_words, 0],
        total_len.to_be_bytes().as_slice(),
        &identification.to_be_bytes(),
        &flags_and_fragment_offset.to_be_bytes(),
        &[time_to_live, PROTOCOL_TCP],
        &checksum.to_be_bytes(),
        &src.octets(),
        &dst.octets(),
        options,
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

#[test]
fn minimal_headers_report_their_lengths() {
    let segment = tcp_segment(50000, 443, b"hello");
    let ipv4 = ipv4_packet(Ipv4Addr::LOCALHOST, Ipv4Addr::LOCALHOST, &segment);
    let ipv6 = ipv6_packet(Ipv6Addr::LOCALHOST, Ipv6Addr::LOCALHOST, &segment);
    let ipv4 = parse_tcp(&ipv4).unwrap();
    let ipv6 = parse_tcp(&ipv6).unwrap();
    assert_eq!(ipv4.ip_header_len, IPV4_MIN_HEADER_LEN);
    assert_eq!(ipv6.ip_header_len, IPV6_HEADER_LEN);
    assert_eq!(ipv4.tcp_header_len, TCP_MIN_HEADER_LEN);
    assert_eq!(ipv6.tcp_header_len, TCP_MIN_HEADER_LEN);
}

#[test]
fn options_extend_header_lengths_and_sequence_number_is_exposed() {
    let sequence_number = 0xfeed_beef;
    let tcp_options = [TCP_OPTION_NO_OPERATION; 8];
    let segment = tcp_segment_with_options(50000, 443, sequence_number, &tcp_options, b"hello");
    let packet = ipv4_packet_with_options(
        Ipv4Addr::LOCALHOST,
        Ipv4Addr::LOCALHOST,
        &IPV4_OPTION_ROUTER_ALERT,
        &segment,
    );
    let tcp = parse_tcp(&packet).unwrap();
    assert_eq!(tcp.sequence_number, sequence_number);
    assert_eq!(
        tcp.ip_header_len,
        IPV4_MIN_HEADER_LEN + IPV4_OPTION_ROUTER_ALERT.len()
    );
    assert_eq!(tcp.tcp_header_len, TCP_MIN_HEADER_LEN + tcp_options.len());
    assert_eq!(tcp.payload, b"hello");
}
