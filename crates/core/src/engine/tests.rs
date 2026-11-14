use core::iter::once;
use std::net::Ipv4Addr;

use super::*;
use crate::segment::Order;
use crate::strategy::SplitPosition;

const CLIENT: Ipv4Addr = Ipv4Addr::new(192, 168, 1, 23);
const SERVER: Ipv4Addr = Ipv4Addr::new(23, 54, 187, 9);
const CLIENT_PORT: u16 = 50123;
const HTTPS_PORT: u16 = 443;
const HTTP_PORT: u16 = 80;
const PROTOCOL_TCP: u8 = 6;
const PROTOCOL_UDP: u8 = 17;
const IPV4_VERSION_AND_MIN_HEADER: u8 = 0x45;
const IPV4_HEADER_LEN: usize = 20;
const IPV4_PROTOCOL_OFFSET: usize = 9;
const IPV4_DONT_FRAGMENT: u16 = 0x4000;
const IPV4_IDENTIFICATION: u16 = 0x4321;
const TIME_TO_LIVE: u8 = 128;
const TCP_DATA_OFFSET_MIN_HEADER: u8 = 0x50;
const TCP_HEADER_LEN: usize = 20;
const TCP_FLAGS_PSH_ACK: u8 = 0x18;
const ACKNOWLEDGMENT_NUMBER: u32 = 0x2000_0000;
const WINDOW: u16 = 0xfaf0;
const UNFILLED_CHECKSUM: [u8; 2] = [0, 0];
const URGENT_POINTER: [u8; 2] = [0, 0];

const CONTENT_TYPE_HANDSHAKE: u8 = 0x16;
const RECORD_VERSION_TLS_1_0: [u8; 2] = [0x03, 0x01];
const HANDSHAKE_TYPE_CLIENT_HELLO: u8 = 0x01;
const CLIENT_VERSION_TLS_1_2: [u8; 2] = [0x03, 0x03];
const RANDOM_LEN: usize = 32;
const SESSION_ID_LEN: usize = 32;
const CIPHER_TLS_AES_128_GCM_SHA256: u16 = 0x1301;
const COMPRESSION_NULL: u8 = 0x00;
const EXTENSION_SERVER_NAME: u16 = 0x0000;
const EXTENSION_PADDING: u16 = 0x0015;
const EXTENSION_KEY_SHARE: u16 = 0x0033;
const SERVER_NAME_TYPE_HOST: u8 = 0x00;
const FILLER_BYTE: u8 = 0x5a;
const TRAILING_PADDING_LEN: usize = 1000;

const PACKET_BOUNDARY: usize = 1400;
const HOST_START_IN_FIRST_PACKET: usize = 600;
const HOST_START_IN_SECOND_PACKET: usize = 1600;
const HOST_START_STRADDLING: usize = PACKET_BOUNDARY - 10;

const LISTED: &str = "steampowered.com";
const HOST: &str = "a.store.steampowered.com";
const HOST_BEFORE_DOMAIN_MID: &str = "a.store.steamp";
const HOST_AFTER_DOMAIN_MID: &str = "owered.com";
const UNLISTED_HOST: &str = "example.org";

const HOST_START_WITHOUT_FILLER: usize = 97;
const CUTS_HOST_IN_FIRST: [usize; 2] = [600, 614];
const CUTS_HOST_IN_SECOND: [usize; 2] = [200, 214];
const CUTS_HOST_STRADDLING: [usize; 1] = [4];

const FIRST_SEQUENCE_NUMBER: u32 = 0x1000_0000;
const SEQUENCE_NUMBER_NEAR_WRAP: u32 = u32::MAX - 700;
const SECOND_SEQUENCE_NUMBER_AFTER_WRAP: u32 = 699;
const START_MS: u64 = 1_000;
const IDLE_TIMEOUT_MS: u64 = 5_000;
const MUTATION_ITERATIONS: u64 = 20_000;

fn u8_prefixed(data: &[u8]) -> Vec<u8> {
    [&[data.len() as u8], data].concat()
}

fn u16_prefixed(data: &[u8]) -> Vec<u8> {
    [&(data.len() as u16).to_be_bytes(), data].concat()
}

fn u24_prefixed(data: &[u8]) -> Vec<u8> {
    [&(data.len() as u32).to_be_bytes()[1..], data].concat()
}

fn extension(extension_type: u16, data: &[u8]) -> Vec<u8> {
    [extension_type.to_be_bytes().as_slice(), &u16_prefixed(data)].concat()
}

fn server_name_extension(host: &str) -> Vec<u8> {
    let entry = [
        &[SERVER_NAME_TYPE_HOST],
        u16_prefixed(host.as_bytes()).as_slice(),
    ]
    .concat();
    extension(EXTENSION_SERVER_NAME, &u16_prefixed(&entry))
}

fn client_hello(host: &str, filler_len: usize) -> Vec<u8> {
    let extensions = [
        extension(EXTENSION_KEY_SHARE, &vec![FILLER_BYTE; filler_len]),
        server_name_extension(host),
        extension(EXTENSION_PADDING, &[0; TRAILING_PADDING_LEN]),
    ]
    .concat();
    let body = [
        CLIENT_VERSION_TLS_1_2.as_slice(),
        &[0xab; RANDOM_LEN],
        &u8_prefixed(&[0xcd; SESSION_ID_LEN]),
        &u16_prefixed(&CIPHER_TLS_AES_128_GCM_SHA256.to_be_bytes()),
        &u8_prefixed(&[COMPRESSION_NULL]),
        &u16_prefixed(&extensions),
    ]
    .concat();
    let handshake = [
        &[HANDSHAKE_TYPE_CLIENT_HELLO],
        u24_prefixed(&body).as_slice(),
    ]
    .concat();
    [
        &[CONTENT_TYPE_HANDSHAKE],
        RECORD_VERSION_TLS_1_0.as_slice(),
        &u16_prefixed(&handshake),
    ]
    .concat()
}

fn client_hello_with_host_at(host: &str, host_start: usize) -> Vec<u8> {
    let hello = client_hello(host, host_start - HOST_START_WITHOUT_FILLER);
    assert_eq!(&hello[host_start..host_start + host.len()], host.as_bytes());
    hello
}

fn tcp_packet(src_port: u16, dst_port: u16, sequence_number: u32, payload: &[u8]) -> Vec<u8> {
    let tcp_header = [
        src_port.to_be_bytes().as_slice(),
        &dst_port.to_be_bytes(),
        &sequence_number.to_be_bytes(),
        &ACKNOWLEDGMENT_NUMBER.to_be_bytes(),
        &[TCP_DATA_OFFSET_MIN_HEADER, TCP_FLAGS_PSH_ACK],
        &WINDOW.to_be_bytes(),
        &UNFILLED_CHECKSUM,
        &URGENT_POINTER,
    ]
    .concat();
    let total_len = (IPV4_HEADER_LEN + TCP_HEADER_LEN + payload.len()) as u16;
    [
        &[IPV4_VERSION_AND_MIN_HEADER, 0],
        total_len.to_be_bytes().as_slice(),
        &IPV4_IDENTIFICATION.to_be_bytes(),
        &IPV4_DONT_FRAGMENT.to_be_bytes(),
        &[TIME_TO_LIVE, PROTOCOL_TCP],
        &UNFILLED_CHECKSUM,
        &CLIENT.octets(),
        &SERVER.octets(),
        &tcp_header,
        payload,
    ]
    .concat()
}

struct TwoPacketHello {
    first: Vec<u8>,
    second: Vec<u8>,
}

impl TwoPacketHello {
    fn new(host: &str, host_start: usize, src_port: u16, first_sequence_number: u32) -> Self {
        let stream = client_hello_with_host_at(host, host_start);
        let (first, second) = stream.split_at(PACKET_BOUNDARY);
        let second_sequence_number = first_sequence_number.wrapping_add(PACKET_BOUNDARY as u32);
        Self {
            first: tcp_packet(src_port, HTTPS_PORT, first_sequence_number, first),
            second: tcp_packet(src_port, HTTPS_PORT, second_sequence_number, second),
        }
    }

    fn listed_at(host_start: usize) -> Self {
        Self::new(HOST, host_start, CLIENT_PORT, FIRST_SEQUENCE_NUMBER)
    }

    fn from_port(src_port: u16, host_start: usize) -> Self {
        Self::new(HOST, host_start, src_port, FIRST_SEQUENCE_NUMBER)
    }
}

fn http_request(host: &str, path: &str) -> Vec<u8> {
    format!("GET {path} HTTP/1.1\r\nUser-Agent: Valve/Steam\r\nHost: {host}\r\nAccept: text/html\r\n\r\n")
        .into_bytes()
}

fn http_packet(host: &str) -> Vec<u8> {
    tcp_packet(
        CLIENT_PORT,
        HTTP_PORT,
        FIRST_SEQUENCE_NUMBER,
        &http_request(host, "/"),
    )
}

fn strategy(order: Order) -> Strategy {
    Strategy {
        order,
        positions: vec![SplitPosition::HostStart, SplitPosition::DomainMid],
    }
}

fn profile(listed: &str, tls: Option<Strategy>, http: Option<Strategy>) -> Profile {
    Profile {
        domains: DomainList::new([listed]),
        tls,
        http,
    }
}

fn steam_profile() -> Profile {
    let both = Some(strategy(Order::InSequence));
    profile(LISTED, both.clone(), both)
}

fn test_limits() -> Limits {
    Limits {
        idle_timeout_ms: IDLE_TIMEOUT_MS,
        ..Limits::default()
    }
}

fn steam_engine() -> Engine {
    Engine::new(vec![steam_profile()], test_limits())
}

fn segments_of(verdict: &Verdict) -> &[Vec<u8>] {
    match verdict {
        Verdict::Replace(segments) => segments,
        Verdict::Pass => panic!("expected the packet to be split, got Pass"),
    }
}

fn payloads_of(verdict: &Verdict) -> Vec<&[u8]> {
    segments_of(verdict)
        .iter()
        .map(|segment| parse_tcp(segment).unwrap().payload)
        .collect()
}

fn assert_split_at(verdict: &Verdict, original: &[u8], payload_cuts: &[usize]) {
    let source = parse_tcp(original).unwrap();
    let bounds: Vec<usize> = once(0)
        .chain(payload_cuts.iter().copied())
        .chain(once(source.payload.len()))
        .collect();
    let segments = segments_of(verdict);
    assert_eq!(segments.len(), payload_cuts.len() + 1);
    let mut joined = Vec::new();
    for (segment, piece) in segments.iter().zip(bounds.windows(2)) {
        let parsed = parse_tcp(segment).unwrap();
        assert_eq!(parsed.payload, &source.payload[piece[0]..piece[1]]);
        assert_eq!(
            parsed.sequence_number,
            source.sequence_number.wrapping_add(piece[0] as u32)
        );
        joined.extend_from_slice(parsed.payload);
    }
    assert_eq!(joined, source.payload);
}

fn assert_piece_starts_with(verdict: &Verdict, index: usize, text: &str) {
    let piece = payloads_of(verdict)[index];
    assert!(
        piece.starts_with(text.as_bytes()),
        "piece {index} does not start with {text}"
    );
}

fn assert_retransmissions_repeat(engine: &mut Engine, sent: &[(&[u8], &Verdict)]) {
    for (packet, verdict) in sent.iter().chain(sent.iter().rev()) {
        assert_eq!(&engine.process(packet, START_MS + 1), *verdict);
    }
}

#[test]
fn default_limits_hold_the_largest_client_hello_record() {
    let largest_record_with_header = 5 + 16_384;
    let limits = Limits::default();
    assert!(limits.max_prefix_bytes >= largest_record_with_header);
    assert!(limits.max_flows > 0);
    assert!(limits.idle_timeout_ms > 0);
}

#[test]
fn host_wholly_in_first_packet_splits_the_first_packet() {
    let hello = TwoPacketHello::listed_at(HOST_START_IN_FIRST_PACKET);
    let mut engine = steam_engine();
    let first = engine.process(&hello.first, START_MS);
    assert_split_at(&first, &hello.first, &CUTS_HOST_IN_FIRST);
    assert_piece_starts_with(&first, 1, HOST_BEFORE_DOMAIN_MID);
    assert_piece_starts_with(&first, 2, HOST_AFTER_DOMAIN_MID);
    let second = engine.process(&hello.second, START_MS);
    assert_eq!(second, Verdict::Pass);
    assert_retransmissions_repeat(
        &mut engine,
        &[(&hello.first, &first), (&hello.second, &second)],
    );
}

#[test]
fn host_wholly_in_second_packet_splits_the_second_packet() {
    let hello = TwoPacketHello::listed_at(HOST_START_IN_SECOND_PACKET);
    let mut engine = steam_engine();
    let first = engine.process(&hello.first, START_MS);
    assert_eq!(first, Verdict::Pass);
    let second = engine.process(&hello.second, START_MS);
    assert_split_at(&second, &hello.second, &CUTS_HOST_IN_SECOND);
    assert_piece_starts_with(&second, 1, HOST_BEFORE_DOMAIN_MID);
    assert_piece_starts_with(&second, 2, HOST_AFTER_DOMAIN_MID);
    assert_retransmissions_repeat(
        &mut engine,
        &[(&hello.first, &first), (&hello.second, &second)],
    );
}

#[test]
fn host_straddling_the_packets_splits_only_inside_the_second_packet() {
    let hello = TwoPacketHello::listed_at(HOST_START_STRADDLING);
    let mut engine = steam_engine();
    let first = engine.process(&hello.first, START_MS);
    assert_eq!(first, Verdict::Pass);
    let second = engine.process(&hello.second, START_MS);
    assert_split_at(&second, &hello.second, &CUTS_HOST_STRADDLING);
    assert_piece_starts_with(&second, 1, HOST_AFTER_DOMAIN_MID);
    assert_retransmissions_repeat(
        &mut engine,
        &[(&hello.first, &first), (&hello.second, &second)],
    );
}

#[test]
fn second_packet_arriving_first_passes_and_is_split_when_retransmitted() {
    let in_first = TwoPacketHello::listed_at(HOST_START_IN_FIRST_PACKET);
    let mut engine = steam_engine();
    assert_eq!(engine.process(&in_first.second, START_MS), Verdict::Pass);
    let first = engine.process(&in_first.first, START_MS);
    assert_split_at(&first, &in_first.first, &CUTS_HOST_IN_FIRST);
    assert_eq!(engine.process(&in_first.second, START_MS), Verdict::Pass);

    let later_cases: [(usize, &[usize]); 2] = [
        (HOST_START_IN_SECOND_PACKET, &CUTS_HOST_IN_SECOND),
        (HOST_START_STRADDLING, &CUTS_HOST_STRADDLING),
    ];
    for (host_start, cuts) in later_cases {
        let hello = TwoPacketHello::listed_at(host_start);
        let mut engine = steam_engine();
        assert_eq!(engine.process(&hello.second, START_MS), Verdict::Pass);
        assert_eq!(engine.process(&hello.first, START_MS), Verdict::Pass);
        let retransmitted = engine.process(&hello.second, START_MS);
        assert_split_at(&retransmitted, &hello.second, cuts);
    }
}

#[test]
fn sequence_numbers_wrapping_between_the_packets_are_followed() {
    let cases: [(usize, &[usize]); 2] = [
        (HOST_START_IN_SECOND_PACKET, &CUTS_HOST_IN_SECOND),
        (HOST_START_STRADDLING, &CUTS_HOST_STRADDLING),
    ];
    for (host_start, cuts) in cases {
        let hello = TwoPacketHello::new(HOST, host_start, CLIENT_PORT, SEQUENCE_NUMBER_NEAR_WRAP);
        assert_eq!(
            parse_tcp(&hello.second).unwrap().sequence_number,
            SECOND_SEQUENCE_NUMBER_AFTER_WRAP
        );
        let mut engine = steam_engine();
        assert_eq!(engine.process(&hello.first, START_MS), Verdict::Pass);
        let second = engine.process(&hello.second, START_MS);
        assert_split_at(&second, &hello.second, cuts);
    }
}

#[test]
fn reversed_order_sends_the_last_piece_first() {
    let hello = TwoPacketHello::listed_at(HOST_START_IN_FIRST_PACKET);
    let reversed = profile(LISTED, Some(strategy(Order::Reversed)), None);
    let mut engine = Engine::new(vec![reversed], test_limits());
    let verdict = engine.process(&hello.first, START_MS);
    let payloads = payloads_of(&verdict);
    assert_eq!(payloads.len(), CUTS_HOST_IN_FIRST.len() + 1);
    assert!(payloads[0].starts_with(HOST_AFTER_DOMAIN_MID.as_bytes()));
    assert_eq!(payloads[1], HOST_BEFORE_DOMAIN_MID.as_bytes());
    let joined: Vec<u8> = payloads
        .iter()
        .rev()
        .flat_map(|piece| piece.to_vec())
        .collect();
    assert_eq!(joined, parse_tcp(&hello.first).unwrap().payload);
}

#[test]
fn unlisted_host_passes_untouched() {
    for host_start in [HOST_START_IN_FIRST_PACKET, HOST_START_IN_SECOND_PACKET] {
        let hello = TwoPacketHello::new(
            UNLISTED_HOST,
            host_start,
            CLIENT_PORT,
            FIRST_SEQUENCE_NUMBER,
        );
        let mut engine = steam_engine();
        assert_retransmissions_repeat(
            &mut engine,
            &[
                (&hello.first, &Verdict::Pass),
                (&hello.second, &Verdict::Pass),
            ],
        );
    }
}

#[test]
fn first_matching_profile_wins_even_without_a_tls_strategy() {
    let hello = TwoPacketHello::listed_at(HOST_START_IN_FIRST_PACKET);
    let http_only = profile(LISTED, None, Some(strategy(Order::InSequence)));
    let offset_only = Strategy {
        order: Order::InSequence,
        positions: vec![SplitPosition::Offset(3)],
    };
    let later = profile(LISTED, Some(strategy(Order::InSequence)), Some(offset_only));
    let mut engine = Engine::new(vec![http_only, later], test_limits());
    assert_eq!(engine.process(&hello.first, START_MS), Verdict::Pass);
    let request = http_packet(HOST);
    let host_start = 47;
    let domain_mid = host_start + HOST_BEFORE_DOMAIN_MID.len();
    let verdict = engine.process(&request, START_MS);
    assert_split_at(&verdict, &request, &[host_start, domain_mid]);
    assert_piece_starts_with(&verdict, 1, HOST_BEFORE_DOMAIN_MID);
}

#[test]
fn profiles_that_do_not_match_are_skipped() {
    let hello = TwoPacketHello::listed_at(HOST_START_IN_FIRST_PACKET);
    let other = profile(UNLISTED_HOST, None, None);
    let mut engine = Engine::new(vec![other, steam_profile()], test_limits());
    let first = engine.process(&hello.first, START_MS);
    assert_split_at(&first, &hello.first, &CUTS_HOST_IN_FIRST);
}

#[test]
fn every_http_request_on_a_keep_alive_connection_is_split() {
    let first_request = http_request(HOST, "/depot/731/chunk/aa");
    let second_request = http_request(HOST, "/depot/731/chunk/bbbb");
    let first_host_start = 65;
    let second_host_start = 67;
    let second_sequence_number = FIRST_SEQUENCE_NUMBER + first_request.len() as u32;
    let first = tcp_packet(
        CLIENT_PORT,
        HTTP_PORT,
        FIRST_SEQUENCE_NUMBER,
        &first_request,
    );
    let second = tcp_packet(
        CLIENT_PORT,
        HTTP_PORT,
        second_sequence_number,
        &second_request,
    );
    let mut engine = steam_engine();
    for (packet, host_start) in [(&first, first_host_start), (&second, second_host_start)] {
        let verdict = engine.process(packet, START_MS);
        let domain_mid = host_start + HOST_BEFORE_DOMAIN_MID.len();
        assert_split_at(&verdict, packet, &[host_start, domain_mid]);
        assert_piece_starts_with(&verdict, 1, HOST_BEFORE_DOMAIN_MID);
        assert_piece_starts_with(&verdict, 2, HOST_AFTER_DOMAIN_MID);
    }
}

#[test]
fn http_requests_without_a_listed_host_or_strategy_pass() {
    let mut engine = steam_engine();
    assert_eq!(
        engine.process(&http_packet(UNLISTED_HOST), START_MS),
        Verdict::Pass
    );
    let tls_only = profile(LISTED, Some(strategy(Order::InSequence)), None);
    let mut engine = Engine::new(vec![tls_only], test_limits());
    assert_eq!(engine.process(&http_packet(HOST), START_MS), Verdict::Pass);
}

#[test]
fn full_flow_table_passes_new_connections_until_an_entry_idles_out() {
    let limits = Limits {
        max_flows: 1,
        ..test_limits()
    };
    let mut engine = Engine::new(vec![steam_profile()], limits);
    let waiting = TwoPacketHello::listed_at(HOST_START_IN_SECOND_PACKET);
    let newcomer = TwoPacketHello::from_port(CLIENT_PORT + 1, HOST_START_IN_FIRST_PACKET);
    assert_eq!(engine.process(&waiting.first, START_MS), Verdict::Pass);
    assert_eq!(engine.process(&newcomer.first, START_MS + 1), Verdict::Pass);
    let after_idle = START_MS + IDLE_TIMEOUT_MS;
    let first = engine.process(&newcomer.first, after_idle);
    assert_split_at(&first, &newcomer.first, &CUTS_HOST_IN_FIRST);
    assert_eq!(engine.process(&waiting.second, after_idle), Verdict::Pass);
}

#[test]
fn idle_flow_is_forgotten() {
    let hello = TwoPacketHello::listed_at(HOST_START_IN_SECOND_PACKET);
    let mut engine = steam_engine();
    assert_eq!(engine.process(&hello.first, START_MS), Verdict::Pass);
    let just_in_time = START_MS + IDLE_TIMEOUT_MS - 1;
    let second = engine.process(&hello.second, just_in_time);
    assert_split_at(&second, &hello.second, &CUTS_HOST_IN_SECOND);
    let too_late = just_in_time + IDLE_TIMEOUT_MS;
    assert_eq!(engine.process(&hello.second, too_late), Verdict::Pass);
}

#[test]
fn host_beyond_the_prefix_cap_passes() {
    let limits = Limits {
        max_prefix_bytes: PACKET_BOUNDARY,
        ..test_limits()
    };
    let mut engine = Engine::new(vec![steam_profile()], limits);
    let beyond = TwoPacketHello::listed_at(HOST_START_IN_SECOND_PACKET);
    assert_eq!(engine.process(&beyond.first, START_MS), Verdict::Pass);
    assert_eq!(engine.process(&beyond.second, START_MS), Verdict::Pass);
    let within = TwoPacketHello::from_port(CLIENT_PORT + 1, HOST_START_IN_FIRST_PACKET);
    let first = engine.process(&within.first, START_MS);
    assert_split_at(&first, &within.first, &CUTS_HOST_IN_FIRST);
}

#[test]
fn non_tcp_and_empty_payloads_pass() {
    let mut udp = http_packet(HOST);
    udp[IPV4_PROTOCOL_OFFSET] = PROTOCOL_UDP;
    let empty = tcp_packet(CLIENT_PORT, HTTPS_PORT, FIRST_SEQUENCE_NUMBER, &[]);
    let mut engine = steam_engine();
    assert_eq!(engine.process(&udp, START_MS), Verdict::Pass);
    assert_eq!(engine.process(&empty, START_MS), Verdict::Pass);
    assert_eq!(engine.process(&[], START_MS), Verdict::Pass);
}

struct XorShift(u32);

impl XorShift {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0
    }

    fn below(&mut self, bound: usize) -> usize {
        self.next() as usize % bound.max(1)
    }
}

#[test]
fn mutated_packets_never_panic_and_splits_round_trip() {
    let hello = TwoPacketHello::listed_at(HOST_START_STRADDLING);
    let originals = [hello.first, hello.second, http_packet(HOST)];
    let limits = Limits {
        max_flows: 8,
        max_prefix_bytes: 2_000,
        idle_timeout_ms: IDLE_TIMEOUT_MS,
    };
    let mut engine = Engine::new(vec![steam_profile()], limits);
    let mut rng = XorShift(0x9e37_79b9);
    let mut splits = 0;
    for now_ms in 0..MUTATION_ITERATIONS {
        let mut packet = originals[rng.below(originals.len())].clone();
        for _ in 0..rng.below(4) {
            let offset = rng.below(packet.len());
            packet[offset] = rng.next() as u8;
        }
        if rng.below(4) == 0 {
            packet.truncate(rng.below(packet.len() + 1));
        }
        if let Verdict::Replace(segments) = engine.process(&packet, now_ms) {
            let joined: Vec<u8> = segments
                .iter()
                .flat_map(|segment| parse_tcp(segment).unwrap().payload.to_vec())
                .collect();
            assert_eq!(joined, parse_tcp(&packet).unwrap().payload);
            splits += 1;
        }
    }
    assert!(splits > 0);
}
