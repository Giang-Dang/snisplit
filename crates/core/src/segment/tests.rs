use core::ops::Range;
use std::collections::BTreeSet;
use std::net::{Ipv4Addr, Ipv6Addr};

use super::{Order, split};

const PROTOCOL_TCP: u8 = 6;
const PROTOCOL_UDP: u8 = 17;
const NEXT_HEADER_HOP_BY_HOP: u8 = 0;
const BYTES_PER_WORD: usize = 4;
const IPV4_VERSION: u8 = 0x40;
const IPV6_VERSION: u8 = 0x60;
const IPV4_MIN_HEADER_LEN: usize = 20;
const IPV6_HEADER_LEN: usize = 40;
const TCP_MIN_HEADER_LEN: usize = 20;

const IPV4_HEADER_LEN_MASK: u8 = 0x0f;
const IPV4_ADDRESSES: Range<usize> = 12..20;
const IPV6_ADDRESSES: Range<usize> = 8..40;
const IPV4_DONT_FRAGMENT: u16 = 0x4000;
const IPV4_RESERVED_FLAG: u16 = 0x8000;
const IPV4_MORE_FRAGMENTS: u16 = 0x2000;
const IPV4_FRAGMENT_OFFSET_MASK: u16 = 0x1fff;
const IPV4_FRAGMENTATION_BITS: u16 = IPV4_MORE_FRAGMENTS | IPV4_FRAGMENT_OFFSET_MASK;
const IPV4_OPTION_ROUTER_ALERT: [u8; 4] = [0x94, 0x04, 0x00, 0x00];
const IPV4_TOTAL_LEN_FIELD: Range<usize> = 2..4;
const IPV4_IDENTIFICATION_FIELD: Range<usize> = 4..6;
const IPV4_FRAGMENT_FIELD: Range<usize> = 6..8;
const IPV4_CHECKSUM_FIELD: Range<usize> = 10..12;
const IPV6_PAYLOAD_LEN_FIELD: Range<usize> = 4..6;
const IPV4_REWRITTEN_FIELDS: [Range<usize>; 3] = [
    IPV4_TOTAL_LEN_FIELD,
    IPV4_IDENTIFICATION_FIELD,
    IPV4_CHECKSUM_FIELD,
];
const IPV6_REWRITTEN_FIELDS: [Range<usize>; 1] = [IPV6_PAYLOAD_LEN_FIELD];

const TCP_ACKNOWLEDGMENT_NUMBER_OFFSET: usize = 8;
const TCP_DATA_OFFSET_OFFSET: usize = 12;
const TCP_WINDOW_OFFSET: usize = 14;
const TCP_FLAG_FIN: u8 = 0x01;
const TCP_FLAG_SYN: u8 = 0x02;
const TCP_FLAG_RST: u8 = 0x04;
const TCP_FLAG_PSH: u8 = 0x08;
const TCP_FLAG_ACK: u8 = 0x10;
const TCP_FLAG_URG: u8 = 0x20;
const TCP_FLAG_ECE: u8 = 0x40;
const TCP_FLAG_CWR: u8 = 0x80;
const TCP_FLAGS_THAT_PREVENT_SPLITTING: u8 = TCP_FLAG_SYN | TCP_FLAG_URG | TCP_FLAG_RST;
const TCP_OPTIONS_NOP_NOP_TIMESTAMP: [u8; 12] = [
    0x01, 0x01, 0x08, 0x0a, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88,
];
const TCP_SEQUENCE_NUMBER_FIELD: Range<usize> = 4..8;
const TCP_FLAGS_FIELD: Range<usize> = 13..14;
const TCP_CHECKSUM_FIELD: Range<usize> = 16..18;
const TCP_REWRITTEN_FIELDS: [Range<usize>; 3] = [
    TCP_SEQUENCE_NUMBER_FIELD,
    TCP_FLAGS_FIELD,
    TCP_CHECKSUM_FIELD,
];

const STALE_CHECKSUM: u16 = 0xdead;
const VALID_ONES_COMPLEMENT_SUM: u16 = 0xffff;

const CLIENT_V4: Ipv4Addr = Ipv4Addr::new(192, 168, 1, 23);
const SERVER_V4: Ipv4Addr = Ipv4Addr::new(23, 54, 187, 9);
const CLIENT_V6: Ipv6Addr = Ipv6Addr::new(0xfd00, 0x1234, 0, 0, 0x8a2e, 0x0370, 0x7334, 0x0001);
const SERVER_V6: Ipv6Addr = Ipv6Addr::new(0x2a02, 0x26f0, 0x00e1, 0, 0, 0, 0x1737, 0xbb09);
const CLIENT_PORT: u16 = 50123;
const SERVER_PORT: u16 = 443;

const FUZZ_ITERATIONS: usize = 30_000;
const FUZZ_MIN_SUCCESSFUL_SPLITS: usize = 2_000;
const FUZZ_MAX_PAYLOAD_LEN: u32 = 48;
const FUZZ_MAX_OFFSET: u32 = 56;
const HEADER_OFFSETS_WORTH_CORRUPTING: [usize; 16] =
    [0, 2, 3, 4, 5, 6, 7, 9, 32, 33, 36, 37, 40, 44, 52, 53];

#[derive(Clone)]
struct Tcp {
    sequence_number: u32,
    acknowledgment_number: u32,
    flags: u8,
    window: u16,
    urgent_pointer: u16,
    options: Vec<u8>,
    payload: Vec<u8>,
}

impl Tcp {
    fn carrying(payload: &[u8]) -> Self {
        Self {
            sequence_number: 0x1000_0000,
            acknowledgment_number: 0x2000_0000,
            flags: TCP_FLAG_ACK | TCP_FLAG_PSH,
            window: 0xfaf0,
            urgent_pointer: 0,
            options: Vec::new(),
            payload: payload.to_vec(),
        }
    }
}

#[derive(Clone)]
struct Ipv4 {
    identification: u16,
    flags_and_fragment_offset: u16,
    protocol: u8,
    options: Vec<u8>,
}

impl Ipv4 {
    fn plain() -> Self {
        Self {
            identification: 0x4321,
            flags_and_fragment_offset: IPV4_DONT_FRAGMENT,
            protocol: PROTOCOL_TCP,
            options: Vec::new(),
        }
    }
}

fn tcp_segment(tcp: &Tcp) -> Vec<u8> {
    let header_words = ((TCP_MIN_HEADER_LEN + tcp.options.len()) / BYTES_PER_WORD) as u8;
    [
        CLIENT_PORT.to_be_bytes().as_slice(),
        &SERVER_PORT.to_be_bytes(),
        &tcp.sequence_number.to_be_bytes(),
        &tcp.acknowledgment_number.to_be_bytes(),
        &[header_words << 4, tcp.flags],
        &tcp.window.to_be_bytes(),
        &STALE_CHECKSUM.to_be_bytes(),
        &tcp.urgent_pointer.to_be_bytes(),
        &tcp.options,
        &tcp.payload,
    ]
    .concat()
}

fn ipv4_packet(ip: &Ipv4, segment: &[u8]) -> Vec<u8> {
    let header_len = IPV4_MIN_HEADER_LEN + ip.options.len();
    let header_words = (header_len / BYTES_PER_WORD) as u8;
    let type_of_service = 0u8;
    let total_len = (header_len + segment.len()) as u16;
    let time_to_live = 128u8;
    [
        &[IPV4_VERSION | header_words, type_of_service],
        total_len.to_be_bytes().as_slice(),
        &ip.identification.to_be_bytes(),
        &ip.flags_and_fragment_offset.to_be_bytes(),
        &[time_to_live, ip.protocol],
        &STALE_CHECKSUM.to_be_bytes(),
        &CLIENT_V4.octets(),
        &SERVER_V4.octets(),
        &ip.options,
        segment,
    ]
    .concat()
}

fn ipv6_packet(next_header: u8, upper_layer: &[u8]) -> Vec<u8> {
    let traffic_class_and_flow_label = [0x0a, 0xbc, 0xde];
    let payload_len = upper_layer.len() as u16;
    let hop_limit = 64u8;
    [
        &[IPV6_VERSION],
        traffic_class_and_flow_label.as_slice(),
        &payload_len.to_be_bytes(),
        &[next_header, hop_limit],
        &CLIENT_V6.octets(),
        &SERVER_V6.octets(),
        upper_layer,
    ]
    .concat()
}

fn plain_ipv4(tcp: &Tcp) -> Vec<u8> {
    ipv4_packet(&Ipv4::plain(), &tcp_segment(tcp))
}

fn plain_ipv6(tcp: &Tcp) -> Vec<u8> {
    ipv6_packet(PROTOCOL_TCP, &tcp_segment(tcp))
}

fn be16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_be_bytes([bytes[offset], bytes[offset + 1]])
}

fn be32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

fn reference_sum(bytes: &[u8]) -> u64 {
    let mut sum = 0u64;
    let mut index = 0;
    while index < bytes.len() {
        let high = u64::from(bytes[index]);
        let low = if index + 1 < bytes.len() {
            u64::from(bytes[index + 1])
        } else {
            0
        };
        sum += high * 256 + low;
        index += 2;
    }
    sum
}

fn reference_fold(mut sum: u64) -> u16 {
    while sum >> 16 != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    sum as u16
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Dissected {
    ip_header: Vec<u8>,
    tcp_header: Vec<u8>,
    payload: Vec<u8>,
}

impl Dissected {
    fn is_ipv4(&self) -> bool {
        self.ip_header[0] >> 4 == 4
    }

    fn identification(&self) -> u16 {
        be16(&self.ip_header, IPV4_IDENTIFICATION_FIELD.start)
    }

    fn fragment_field(&self) -> u16 {
        be16(&self.ip_header, IPV4_FRAGMENT_FIELD.start)
    }

    fn sequence_number(&self) -> u32 {
        be32(&self.tcp_header, TCP_SEQUENCE_NUMBER_FIELD.start)
    }

    fn flags(&self) -> u8 {
        self.tcp_header[TCP_FLAGS_FIELD.start]
    }

    fn ip_header_without_rewritten_fields(&self) -> Vec<u8> {
        if self.is_ipv4() {
            zeroed(&self.ip_header, &IPV4_REWRITTEN_FIELDS)
        } else {
            zeroed(&self.ip_header, &IPV6_REWRITTEN_FIELDS)
        }
    }

    fn tcp_header_without_rewritten_fields(&self) -> Vec<u8> {
        zeroed(&self.tcp_header, &TCP_REWRITTEN_FIELDS)
    }
}

fn zeroed(bytes: &[u8], fields: &[Range<usize>]) -> Vec<u8> {
    let mut bytes = bytes.to_vec();
    for field in fields {
        bytes[field.clone()].fill(0);
    }
    bytes
}

fn declared_lengths(packet: &[u8]) -> (usize, usize) {
    match packet[0] >> 4 {
        4 => (
            usize::from(packet[0] & IPV4_HEADER_LEN_MASK) * BYTES_PER_WORD,
            usize::from(be16(packet, IPV4_TOTAL_LEN_FIELD.start)),
        ),
        6 => (
            IPV6_HEADER_LEN,
            IPV6_HEADER_LEN + usize::from(be16(packet, IPV6_PAYLOAD_LEN_FIELD.start)),
        ),
        version => panic!("unexpected IP version {version}"),
    }
}

fn dissect(packet: &[u8]) -> Dissected {
    let (ip_header_len, total_len) = declared_lengths(packet);
    let data_offset = packet[ip_header_len + TCP_DATA_OFFSET_OFFSET] >> 4;
    let payload_start = ip_header_len + usize::from(data_offset) * BYTES_PER_WORD;
    Dissected {
        ip_header: packet[..ip_header_len].to_vec(),
        tcp_header: packet[ip_header_len..payload_start].to_vec(),
        payload: packet[payload_start..total_len].to_vec(),
    }
}

fn reference_pseudo_header(segment: &[u8], tcp_len: usize) -> Vec<u8> {
    match segment[0] >> 4 {
        4 => [
            &segment[IPV4_ADDRESSES],
            &[0, PROTOCOL_TCP],
            (tcp_len as u16).to_be_bytes().as_slice(),
        ]
        .concat(),
        _ => [
            &segment[IPV6_ADDRESSES],
            (tcp_len as u32).to_be_bytes().as_slice(),
            &[0, 0, 0, PROTOCOL_TCP],
        ]
        .concat(),
    }
}

fn reference_tcp_sum(segment: &[u8]) -> u16 {
    let (ip_header_len, _) = declared_lengths(segment);
    let tcp = &segment[ip_header_len..];
    let pseudo_header = reference_pseudo_header(segment, tcp.len());
    reference_fold(reference_sum(&pseudo_header) + reference_sum(tcp))
}

fn assert_valid_on_the_wire(segment: &[u8]) {
    let (ip_header_len, total_len) = declared_lengths(segment);
    assert_eq!(total_len, segment.len());
    if segment[0] >> 4 == 4 {
        let header_sum = reference_fold(reference_sum(&segment[..ip_header_len]));
        assert_eq!(header_sum, VALID_ONES_COMPLEMENT_SUM);
    }
    assert_eq!(reference_tcp_sum(segment), VALID_ONES_COMPLEMENT_SUM);
}

fn expected_cuts(payload_offsets: &[usize], payload_len: usize) -> Vec<usize> {
    let inside: BTreeSet<usize> = payload_offsets
        .iter()
        .copied()
        .filter(|&offset| offset != 0 && offset < payload_len)
        .collect();
    inside.into_iter().collect()
}

fn assert_splits(original: &[u8], payload_offsets: &[usize], in_sequence: &[Vec<u8>]) {
    let source = dissect(original);
    let cuts = expected_cuts(payload_offsets, source.payload.len());
    assert_eq!(in_sequence.len(), cuts.len() + 1);
    assert_eq!(source.flags() & TCP_FLAGS_THAT_PREVENT_SPLITTING, 0);

    let mut payload_seen = Vec::new();
    for (index, segment) in in_sequence.iter().enumerate() {
        assert_valid_on_the_wire(segment);
        let piece = dissect(segment);
        let is_last = index + 1 == in_sequence.len();
        let piece_start = payload_seen.len();
        let piece_end = if is_last {
            source.payload.len()
        } else {
            cuts[index]
        };
        let expected_flags = if is_last {
            source.flags()
        } else {
            source.flags() & !(TCP_FLAG_PSH | TCP_FLAG_FIN)
        };

        assert_eq!(piece.payload, source.payload[piece_start..piece_end]);
        assert_eq!(
            piece.sequence_number(),
            source.sequence_number().wrapping_add(piece_start as u32)
        );
        assert_eq!(piece.flags(), expected_flags);
        assert_eq!(
            piece.tcp_header_without_rewritten_fields(),
            source.tcp_header_without_rewritten_fields()
        );
        assert_eq!(
            piece.ip_header_without_rewritten_fields(),
            source.ip_header_without_rewritten_fields()
        );
        if source.is_ipv4() {
            assert_eq!(source.fragment_field() & IPV4_FRAGMENTATION_BITS, 0);
            assert_eq!(
                piece.identification(),
                source.identification().wrapping_add(index as u16)
            );
        }
        payload_seen.extend_from_slice(&piece.payload);
    }
    assert_eq!(payload_seen, source.payload);
}

fn split_in_both_orders(packet: &[u8], payload_offsets: &[usize]) -> Vec<Vec<u8>> {
    let in_sequence = split(packet, payload_offsets, Order::InSequence).unwrap();
    let mut reversed = split(packet, payload_offsets, Order::Reversed).unwrap();
    reversed.reverse();
    assert_eq!(reversed, in_sequence);
    assert_splits(packet, payload_offsets, &in_sequence);
    in_sequence
}

fn flags_of(segments: &[Vec<u8>]) -> Vec<u8> {
    segments.iter().map(|s| dissect(s).flags()).collect()
}

fn sequence_numbers_of(segments: &[Vec<u8>]) -> Vec<u32> {
    segments
        .iter()
        .map(|s| dissect(s).sequence_number())
        .collect()
}

fn identifications_of(segments: &[Vec<u8>]) -> Vec<u16> {
    segments
        .iter()
        .map(|s| dissect(s).identification())
        .collect()
}

fn payloads_of(segments: &[Vec<u8>]) -> Vec<Vec<u8>> {
    segments.iter().map(|s| dissect(s).payload).collect()
}

fn with_all_options(tcp: &Tcp) -> (Ipv4, Tcp) {
    let ip = Ipv4 {
        options: IPV4_OPTION_ROUTER_ALERT.to_vec(),
        ..Ipv4::plain()
    };
    let tcp = Tcp {
        options: TCP_OPTIONS_NOP_NOP_TIMESTAMP.to_vec(),
        ..tcp.clone()
    };
    (ip, tcp)
}

fn packets_of_every_shape(tcp: &Tcp) -> Vec<Vec<u8>> {
    let (ip_with_options, tcp_with_options) = with_all_options(tcp);
    vec![
        plain_ipv4(tcp),
        plain_ipv6(tcp),
        ipv4_packet(&ip_with_options, &tcp_segment(&tcp_with_options)),
        plain_ipv6(&tcp_with_options),
    ]
}

struct XorShift(u32);

impl XorShift {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0
    }

    fn below(&mut self, bound: u32) -> u32 {
        self.next() % bound.max(1)
    }

    fn one_in(&mut self, chances: u32) -> bool {
        self.below(chances) == 0
    }

    fn bytes(&mut self, len: u32) -> Vec<u8> {
        (0..len).map(|_| self.next() as u8).collect()
    }
}

#[test]
fn ipv4_segments_carry_the_payload_pieces() {
    let packet = plain_ipv4(&Tcp::carrying(
        b"GET / HTTP/1.1\r\nHost: example.com\r\n\r\n",
    ));
    let segments = split_in_both_orders(&packet, &[4, 22]);
    assert_eq!(
        payloads_of(&segments),
        [
            b"GET ".to_vec(),
            b"/ HTTP/1.1\r\nHost: ".to_vec(),
            b"example.com\r\n\r\n".to_vec()
        ]
    );
    assert_eq!(
        sequence_numbers_of(&segments),
        [0x1000_0000, 0x1000_0004, 0x1000_0016]
    );
}

#[test]
fn ipv6_segments_carry_the_payload_pieces() {
    let packet = plain_ipv6(&Tcp::carrying(
        b"GET / HTTP/1.1\r\nHost: example.com\r\n\r\n",
    ));
    let segments = split_in_both_orders(&packet, &[4, 22]);
    assert_eq!(
        payloads_of(&segments),
        [
            b"GET ".to_vec(),
            b"/ HTTP/1.1\r\nHost: ".to_vec(),
            b"example.com\r\n\r\n".to_vec()
        ]
    );
    assert_eq!(
        sequence_numbers_of(&segments),
        [0x1000_0000, 0x1000_0004, 0x1000_0016]
    );
}

#[test]
fn stale_input_checksums_are_replaced() {
    let packet = plain_ipv4(&Tcp::carrying(b"abcdef"));
    let tcp_checksum_at = IPV4_MIN_HEADER_LEN + TCP_CHECKSUM_FIELD.start;
    assert_eq!(be16(&packet, IPV4_CHECKSUM_FIELD.start), STALE_CHECKSUM);
    assert_eq!(be16(&packet, tcp_checksum_at), STALE_CHECKSUM);
    for segment in split_in_both_orders(&packet, &[3]) {
        assert_ne!(be16(&segment, IPV4_CHECKSUM_FIELD.start), STALE_CHECKSUM);
        assert_ne!(be16(&segment, tcp_checksum_at), STALE_CHECKSUM);
    }
}

#[test]
fn reference_check_rejects_a_corrupted_segment() {
    for packet in packets_of_every_shape(&Tcp::carrying(b"abcdefg")) {
        for mut segment in split_in_both_orders(&packet, &[3]) {
            let last = segment.len() - 1;
            segment[last] ^= 0x01;
            assert_ne!(reference_tcp_sum(&segment), VALID_ONES_COMPLEMENT_SUM);
        }
    }
}

#[test]
fn every_cut_of_odd_and_even_payloads_verifies() {
    for payload_len in 2..=9u8 {
        let payload: Vec<u8> = (0..payload_len).map(|byte| 0xf0 | byte).collect();
        for packet in packets_of_every_shape(&Tcp::carrying(&payload)) {
            for first in 1..payload.len() {
                split_in_both_orders(&packet, &[first]);
                for second in first + 1..payload.len() {
                    split_in_both_orders(&packet, &[first, second]);
                }
            }
        }
    }
}

#[test]
fn a_cut_at_every_byte_gives_one_byte_segments() {
    let payload: Vec<u8> = (0..1400u32).map(|index| (index % 251) as u8).collect();
    let every_offset: Vec<usize> = (0..=payload.len()).collect();
    for packet in packets_of_every_shape(&Tcp::carrying(&payload)) {
        let segments = split_in_both_orders(&packet, &every_offset);
        assert_eq!(segments.len(), payload.len());
    }
}

#[test]
fn largest_ipv4_packet_is_split() {
    let payload_len = usize::from(u16::MAX) - IPV4_MIN_HEADER_LEN - TCP_MIN_HEADER_LEN;
    let payload: Vec<u8> = (0..payload_len).map(|index| (index % 253) as u8).collect();
    let packet = plain_ipv4(&Tcp::carrying(&payload));
    assert_eq!(packet.len(), usize::from(u16::MAX));
    split_in_both_orders(&packet, &[1, payload_len - 1]);
}

#[test]
fn largest_ipv6_packet_is_split() {
    let payload_len = usize::from(u16::MAX) - TCP_MIN_HEADER_LEN;
    let payload: Vec<u8> = (0..payload_len).map(|index| (index % 253) as u8).collect();
    let packet = plain_ipv6(&Tcp::carrying(&payload));
    assert_eq!(packet.len(), IPV6_HEADER_LEN + usize::from(u16::MAX));
    split_in_both_orders(&packet, &[1, payload_len - 1]);
}

#[test]
fn offsets_are_filtered_sorted_and_deduplicated() {
    let payload = b"0123456789";
    for packet in packets_of_every_shape(&Tcp::carrying(payload)) {
        let tidy = split_in_both_orders(&packet, &[3, 7]);
        let messy_offsets = [7, 0, 3, 7, payload.len(), 3, usize::MAX, payload.len() + 1];
        assert_eq!(split_in_both_orders(&packet, &messy_offsets), tidy);
        assert_eq!(tidy.len(), 3);
    }
}

#[test]
fn sequence_numbers_wrap_around() {
    let tcp = Tcp {
        sequence_number: u32::MAX - 1,
        ..Tcp::carrying(b"0123456789")
    };
    for packet in packets_of_every_shape(&tcp) {
        let segments = split_in_both_orders(&packet, &[1, 2, 5]);
        assert_eq!(
            sequence_numbers_of(&segments),
            [u32::MAX - 1, u32::MAX, 0, 3]
        );
    }
}

#[test]
fn psh_and_fin_stay_on_the_segment_that_ends_the_payload() {
    let acknowledged = TCP_FLAG_ACK;
    let closing = TCP_FLAG_ACK | TCP_FLAG_PSH | TCP_FLAG_FIN;
    let tcp = Tcp {
        flags: closing,
        ..Tcp::carrying(b"0123456789")
    };
    for packet in packets_of_every_shape(&tcp) {
        let in_sequence = split(&packet, &[2, 5], Order::InSequence).unwrap();
        let reversed = split(&packet, &[2, 5], Order::Reversed).unwrap();
        assert_eq!(
            flags_of(&in_sequence),
            [acknowledged, acknowledged, closing]
        );
        assert_eq!(flags_of(&reversed), [closing, acknowledged, acknowledged]);
        assert_eq!(
            payloads_of(&reversed),
            [b"56789".to_vec(), b"234".to_vec(), b"01".to_vec()]
        );
    }
}

#[test]
fn absent_psh_and_fin_are_not_added() {
    let tcp = Tcp {
        flags: TCP_FLAG_ACK,
        ..Tcp::carrying(b"0123456789")
    };
    for packet in packets_of_every_shape(&tcp) {
        let segments = split_in_both_orders(&packet, &[2, 5]);
        assert_eq!(flags_of(&segments), [TCP_FLAG_ACK; 3]);
    }
}

#[test]
fn other_flags_are_copied_to_every_segment() {
    let copied = TCP_FLAG_ACK | TCP_FLAG_ECE | TCP_FLAG_CWR;
    let tcp = Tcp {
        flags: copied | TCP_FLAG_PSH,
        ..Tcp::carrying(b"0123456789")
    };
    for packet in packets_of_every_shape(&tcp) {
        let segments = split_in_both_orders(&packet, &[2, 5]);
        assert_eq!(flags_of(&segments), [copied, copied, copied | TCP_FLAG_PSH]);
    }
}

#[test]
fn syn_urg_or_rst_packets_are_not_split() {
    for refused_flag in [TCP_FLAG_SYN, TCP_FLAG_URG, TCP_FLAG_RST] {
        let tcp = Tcp {
            flags: TCP_FLAG_ACK | refused_flag,
            urgent_pointer: 4,
            ..Tcp::carrying(b"0123456789")
        };
        for packet in packets_of_every_shape(&tcp) {
            assert_eq!(split(&packet, &[5], Order::InSequence), None);
            assert_eq!(split(&packet, &[5], Order::Reversed), None);
        }
    }
}

#[test]
fn reversed_emits_the_same_segments_last_first() {
    for packet in packets_of_every_shape(&Tcp::carrying(b"0123456789")) {
        let in_sequence = split(&packet, &[1, 4, 8], Order::InSequence).unwrap();
        let reversed = split(&packet, &[1, 4, 8], Order::Reversed).unwrap();
        let last_first: Vec<Vec<u8>> = in_sequence.iter().rev().cloned().collect();
        assert_eq!(in_sequence.len(), 4);
        assert_ne!(reversed, in_sequence);
        assert_eq!(reversed, last_first);
    }
}

#[test]
fn ipv4_identifications_are_distinct_and_wrap_around() {
    let ip = Ipv4 {
        identification: u16::MAX,
        ..Ipv4::plain()
    };
    let packet = ipv4_packet(&ip, &tcp_segment(&Tcp::carrying(b"0123456789")));
    let in_sequence = split_in_both_orders(&packet, &[2, 5]);
    let reversed = split(&packet, &[2, 5], Order::Reversed).unwrap();
    assert_eq!(identifications_of(&in_sequence), [u16::MAX, 0, 1]);
    assert_eq!(identifications_of(&reversed), [1, 0, u16::MAX]);
}

#[test]
fn ipv4_identifications_never_repeat_within_one_split() {
    let payload = vec![0x5a; 300];
    let every_offset: Vec<usize> = (1..payload.len()).collect();
    let packet = plain_ipv4(&Tcp::carrying(&payload));
    let segments = split_in_both_orders(&packet, &every_offset);
    let distinct: BTreeSet<u16> = identifications_of(&segments).into_iter().collect();
    assert_eq!(distinct.len(), payload.len());
}

#[test]
fn ip_and_tcp_options_are_preserved_byte_for_byte() {
    let (ip, tcp) = with_all_options(&Tcp::carrying(b"0123456789"));
    let ip_header_len = IPV4_MIN_HEADER_LEN + IPV4_OPTION_ROUTER_ALERT.len();
    let tcp_header_len = TCP_MIN_HEADER_LEN + TCP_OPTIONS_NOP_NOP_TIMESTAMP.len();
    let ipv4 = ipv4_packet(&ip, &tcp_segment(&tcp));
    let ipv6 = plain_ipv6(&tcp);

    for segment in split_in_both_orders(&ipv4, &[3, 6]) {
        let dissected = dissect(&segment);
        assert_eq!(dissected.ip_header.len(), ip_header_len);
        assert_eq!(dissected.tcp_header.len(), tcp_header_len);
        assert_eq!(
            dissected.ip_header[IPV4_MIN_HEADER_LEN..],
            IPV4_OPTION_ROUTER_ALERT
        );
        assert_eq!(
            dissected.tcp_header[TCP_MIN_HEADER_LEN..],
            TCP_OPTIONS_NOP_NOP_TIMESTAMP
        );
    }
    for segment in split_in_both_orders(&ipv6, &[3, 6]) {
        let dissected = dissect(&segment);
        assert_eq!(dissected.ip_header.len(), IPV6_HEADER_LEN);
        assert_eq!(
            dissected.tcp_header[TCP_MIN_HEADER_LEN..],
            TCP_OPTIONS_NOP_NOP_TIMESTAMP
        );
    }
}

#[test]
fn acknowledgment_number_and_window_are_copied() {
    let tcp = Tcp::carrying(b"0123456789");
    for packet in packets_of_every_shape(&tcp) {
        for segment in split_in_both_orders(&packet, &[3, 6]) {
            let header = dissect(&segment).tcp_header;
            assert_eq!(
                be32(&header, TCP_ACKNOWLEDGMENT_NUMBER_OFFSET),
                tcp.acknowledgment_number
            );
            assert_eq!(be16(&header, TCP_WINDOW_OFFSET), tcp.window);
        }
    }
}

#[test]
fn ipv4_flags_that_are_not_fragmentation_are_kept() {
    for kept_flags in [0, IPV4_DONT_FRAGMENT, IPV4_RESERVED_FLAG] {
        let ip = Ipv4 {
            flags_and_fragment_offset: kept_flags,
            ..Ipv4::plain()
        };
        let packet = ipv4_packet(&ip, &tcp_segment(&Tcp::carrying(b"0123456789")));
        for segment in split_in_both_orders(&packet, &[3, 6]) {
            assert_eq!(be16(&segment, IPV4_FRAGMENT_FIELD.start), kept_flags);
        }
    }
}

#[test]
fn ipv4_fragments_are_rejected() {
    let first_fragment = IPV4_MORE_FRAGMENTS;
    let later_fragment = 1;
    let last_fragment = IPV4_FRAGMENT_OFFSET_MASK;
    let contradictory = IPV4_DONT_FRAGMENT | IPV4_MORE_FRAGMENTS;
    for fragment_field in [first_fragment, later_fragment, last_fragment, contradictory] {
        let ip = Ipv4 {
            flags_and_fragment_offset: fragment_field,
            ..Ipv4::plain()
        };
        let packet = ipv4_packet(&ip, &tcp_segment(&Tcp::carrying(b"0123456789")));
        assert_eq!(split(&packet, &[5], Order::InSequence), None);
        assert_eq!(split(&packet, &[5], Order::Reversed), None);
    }
}

#[test]
fn packet_without_payload_is_rejected() {
    for packet in packets_of_every_shape(&Tcp::carrying(b"")) {
        assert_eq!(split(&packet, &[0, 1, 2], Order::InSequence), None);
    }
}

#[test]
fn offsets_outside_the_payload_are_rejected() {
    let payload = b"0123456789";
    for packet in packets_of_every_shape(&Tcp::carrying(payload)) {
        let outside = [0, payload.len(), payload.len() + 1, usize::MAX];
        assert_eq!(split(&packet, &[], Order::InSequence), None);
        assert_eq!(split(&packet, &outside, Order::InSequence), None);
        assert_eq!(split(&packet, &outside, Order::Reversed), None);
    }
}

#[test]
fn one_byte_payload_cannot_be_split() {
    for packet in packets_of_every_shape(&Tcp::carrying(b"x")) {
        assert_eq!(split(&packet, &[0, 1], Order::InSequence), None);
    }
}

#[test]
fn truncated_buffers_are_rejected() {
    for packet in packets_of_every_shape(&Tcp::carrying(b"0123456789")) {
        for len in 0..packet.len() {
            assert_eq!(split(&packet[..len], &[5], Order::InSequence), None);
        }
        assert!(split(&packet, &[5], Order::InSequence).is_some());
    }
}

#[test]
fn bytes_after_the_ip_length_are_ignored() {
    for packet in packets_of_every_shape(&Tcp::carrying(b"0123456789")) {
        let padded = [packet.as_slice(), &[0xee; 7]].concat();
        let expected = split_in_both_orders(&packet, &[5]);
        assert_eq!(split_in_both_orders(&padded, &[5]), expected);
        assert_eq!(split(&padded, &[12], Order::InSequence), None);
    }
}

#[test]
fn garbage_is_rejected() {
    let unknown_version = [0x10; 60];
    assert_eq!(split(&[], &[1], Order::InSequence), None);
    assert_eq!(split(&unknown_version, &[1], Order::InSequence), None);
    assert_eq!(
        split(&[IPV4_VERSION | 5; 60], &[1], Order::InSequence),
        None
    );
    assert_eq!(split(&[IPV6_VERSION; 80], &[1], Order::InSequence), None);
}

#[test]
fn non_tcp_ipv4_is_rejected() {
    let ip = Ipv4 {
        protocol: PROTOCOL_UDP,
        ..Ipv4::plain()
    };
    let packet = ipv4_packet(&ip, &tcp_segment(&Tcp::carrying(b"0123456789")));
    assert_eq!(split(&packet, &[5], Order::InSequence), None);
}

#[test]
fn ipv6_extension_headers_are_rejected() {
    let hop_by_hop_then_tcp = [PROTOCOL_TCP, 0, 1, 4, 0, 0, 0, 0];
    let upper_layer = [
        hop_by_hop_then_tcp.as_slice(),
        &tcp_segment(&Tcp::carrying(b"0123456789")),
    ]
    .concat();
    let packet = ipv6_packet(NEXT_HEADER_HOP_BY_HOP, &upper_layer);
    assert_eq!(split(&packet, &[5], Order::InSequence), None);
}

#[test]
fn tcp_header_longer_than_the_packet_is_rejected() {
    let mut ipv4 = plain_ipv4(&Tcp::carrying(b"0123456789"));
    let mut ipv6 = plain_ipv6(&Tcp::carrying(b"0123456789"));
    ipv4[IPV4_MIN_HEADER_LEN + TCP_DATA_OFFSET_OFFSET] = 0xf0;
    ipv6[IPV6_HEADER_LEN + TCP_DATA_OFFSET_OFFSET] = 0xf0;
    assert_eq!(split(&ipv4, &[1], Order::InSequence), None);
    assert_eq!(split(&ipv6, &[1], Order::InSequence), None);
}

fn random_tcp(rng: &mut XorShift) -> Tcp {
    let payload_len = rng.below(FUZZ_MAX_PAYLOAD_LEN);
    Tcp {
        sequence_number: if rng.one_in(4) {
            u32::MAX - rng.below(FUZZ_MAX_PAYLOAD_LEN)
        } else {
            rng.next()
        },
        acknowledgment_number: rng.next(),
        flags: if rng.one_in(4) {
            rng.next() as u8
        } else {
            rng.next() as u8 & !TCP_FLAGS_THAT_PREVENT_SPLITTING
        },
        window: rng.next() as u16,
        urgent_pointer: rng.next() as u16,
        options: if rng.one_in(2) {
            TCP_OPTIONS_NOP_NOP_TIMESTAMP.to_vec()
        } else {
            Vec::new()
        },
        payload: rng.bytes(payload_len),
    }
}

fn random_ipv4(rng: &mut XorShift) -> Ipv4 {
    Ipv4 {
        identification: rng.next() as u16,
        flags_and_fragment_offset: if rng.one_in(4) {
            rng.next() as u16
        } else {
            IPV4_DONT_FRAGMENT
        },
        protocol: PROTOCOL_TCP,
        options: if rng.one_in(2) {
            IPV4_OPTION_ROUTER_ALERT.to_vec()
        } else {
            Vec::new()
        },
    }
}

fn random_packet(rng: &mut XorShift) -> Vec<u8> {
    let segment = tcp_segment(&random_tcp(rng));
    if rng.one_in(2) {
        ipv4_packet(&random_ipv4(rng), &segment)
    } else {
        ipv6_packet(PROTOCOL_TCP, &segment)
    }
}

fn overwrite_random_byte(rng: &mut XorShift, packet: &mut [u8], candidates: &[usize]) {
    let candidate = candidates[rng.below(candidates.len() as u32) as usize];
    if let Some(byte) = packet.get_mut(candidate) {
        *byte = rng.next() as u8;
    }
}

fn mutate(rng: &mut XorShift, packet: &mut Vec<u8>) {
    let anywhere: Vec<usize> = (0..packet.len()).collect();
    match rng.below(8) {
        0 => packet.truncate(rng.below(packet.len() as u32 + 1) as usize),
        1 => overwrite_random_byte(rng, packet, &HEADER_OFFSETS_WORTH_CORRUPTING),
        2 => {
            overwrite_random_byte(rng, packet, &HEADER_OFFSETS_WORTH_CORRUPTING);
            overwrite_random_byte(rng, packet, &HEADER_OFFSETS_WORTH_CORRUPTING);
        }
        3 if !anywhere.is_empty() => overwrite_random_byte(rng, packet, &anywhere),
        4 => {
            let trailing_len = rng.below(16);
            packet.extend(rng.bytes(trailing_len));
        }
        5 => {
            let len = rng.below(120);
            *packet = rng.bytes(len);
        }
        _ => {}
    }
}

fn random_offsets(rng: &mut XorShift) -> Vec<usize> {
    let count = rng.below(5);
    (0..count)
        .map(|_| match rng.below(8) {
            0 => 0,
            1 => usize::MAX,
            2 => usize::MAX / 2,
            _ => rng.below(FUZZ_MAX_OFFSET) as usize,
        })
        .collect()
}

#[test]
fn mutated_packets_never_panic_and_every_split_verifies() {
    let mut rng = XorShift(0x2545_f491);
    let mut successful_splits = 0;
    for _ in 0..FUZZ_ITERATIONS {
        let mut packet = random_packet(&mut rng);
        mutate(&mut rng, &mut packet);
        let payload_offsets = random_offsets(&mut rng);
        let order = if rng.one_in(2) {
            Order::Reversed
        } else {
            Order::InSequence
        };
        if let Some(mut segments) = split(&packet, &payload_offsets, order) {
            if order == Order::Reversed {
                segments.reverse();
            }
            assert_splits(&packet, &payload_offsets, &segments);
            successful_splits += 1;
        }
    }
    assert!(
        successful_splits >= FUZZ_MIN_SUCCESSFUL_SPLITS,
        "only {successful_splits} mutated packets were split"
    );
}

#[test]
fn random_bytes_never_panic() {
    let mut rng = XorShift(0x1234_5678);
    for _ in 0..FUZZ_ITERATIONS {
        let len = rng.below(200);
        let mut input = rng.bytes(len);
        if let Some(version_and_header_len) = input.first_mut() {
            *version_and_header_len = match rng.below(3) {
                0 => IPV4_VERSION | 5,
                1 => IPV6_VERSION,
                _ => *version_and_header_len,
            };
        }
        let payload_offsets = random_offsets(&mut rng);
        if let Some(segments) = split(&input, &payload_offsets, Order::InSequence) {
            assert_splits(&input, &payload_offsets, &segments);
        }
    }
}
