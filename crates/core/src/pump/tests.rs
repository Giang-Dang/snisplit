use std::collections::VecDeque;
use std::net::Ipv4Addr;

use super::*;
use crate::domains::DomainList;
use crate::engine::{Limits, Profile};
use crate::packet::parse_tcp;
use crate::segment::Order;
use crate::strategy::{SplitPosition, Strategy};

const CLIENT: Ipv4Addr = Ipv4Addr::new(192, 168, 1, 23);
const SERVER: Ipv4Addr = Ipv4Addr::new(23, 54, 187, 9);
const HTTPS_PORT: u16 = 443;
const FIRST_SEQUENCE_NUMBER: u32 = 0x1000_0000;
const UNFILLED_CHECKSUM: [u8; 2] = [0, 0];

const CONTENT_TYPE_HANDSHAKE: u8 = 0x16;
const FILLER_BYTE: u8 = 0x5a;
const LISTED: &str = "steampowered.com";
const LISTED_HOST: &str = "a.store.steampowered.com";
const UNLISTED_HOST: &str = "example.org";
const HOST_BEFORE_DOMAIN_MID: &str = "a.store.steamp";
const HOST_AFTER_DOMAIN_MID: &str = "owered.com";

const PACKET_BOUNDARY: usize = 1400;
const FILLER_PUSHING_HOST_PAST_BOUNDARY: usize = 1500;
const IDLE_TIMEOUT_MS: u64 = 5_000;
const START_MS: u64 = 1_000;
const SHORTER_THAN_IDLE_MS: u64 = 4_000;
const LONGER_THAN_IDLE_MS: u64 = 10_000;
const LARGEST_PACKET_LEN: usize = 65535;

const ADDRESS_A: u32 = 11;
const ADDRESS_B: u32 = 22;
const ADDRESS_C: u32 = 33;

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

fn client_hello(host: &str, filler_len: usize) -> Vec<u8> {
    let name_entry = [&[0x00], u16_prefixed(host.as_bytes()).as_slice()].concat();
    let extensions = [
        extension(0x0033, &vec![FILLER_BYTE; filler_len]),
        extension(0x0000, &u16_prefixed(&name_entry)),
        extension(0x0015, &[0; 200]),
    ]
    .concat();
    let body = [
        [0x03, 0x03].as_slice(),
        &[0xab; 32],
        &u8_prefixed(&[0xcd; 32]),
        &u16_prefixed(&[0x13, 0x01]),
        &u8_prefixed(&[0x00]),
        &u16_prefixed(&extensions),
    ]
    .concat();
    let handshake = [&[0x01], u24_prefixed(&body).as_slice()].concat();
    [
        &[CONTENT_TYPE_HANDSHAKE, 0x03, 0x01],
        u16_prefixed(&handshake).as_slice(),
    ]
    .concat()
}

fn tcp_packet(src_port: u16, sequence_number: u32, payload: &[u8]) -> Vec<u8> {
    let tcp_header = [
        src_port.to_be_bytes().as_slice(),
        &HTTPS_PORT.to_be_bytes(),
        &sequence_number.to_be_bytes(),
        &0x2000_0000_u32.to_be_bytes(),
        &[0x50, 0x18],
        &0xfaf0_u16.to_be_bytes(),
        &UNFILLED_CHECKSUM,
        &[0, 0],
    ]
    .concat();
    let total_len = (20 + tcp_header.len() + payload.len()) as u16;
    [
        &[0x45, 0],
        total_len.to_be_bytes().as_slice(),
        &0x4321_u16.to_be_bytes(),
        &0x4000_u16.to_be_bytes(),
        &[128, 6],
        &UNFILLED_CHECKSUM,
        &CLIENT.octets(),
        &SERVER.octets(),
        &tcp_header,
        payload,
    ]
    .concat()
}

fn hello_packet(host: &str, src_port: u16) -> Vec<u8> {
    tcp_packet(src_port, FIRST_SEQUENCE_NUMBER, &client_hello(host, 0))
}

fn listed_packet() -> Vec<u8> {
    hello_packet(LISTED_HOST, 50001)
}

fn unlisted_packet() -> Vec<u8> {
    hello_packet(UNLISTED_HOST, 50002)
}

fn bare_ack_packet() -> Vec<u8> {
    tcp_packet(50003, FIRST_SEQUENCE_NUMBER, &[])
}

fn not_an_ip_packet() -> Vec<u8> {
    vec![0; 30]
}

fn host_in_second_packet() -> (Vec<u8>, Vec<u8>) {
    let stream = client_hello(LISTED_HOST, FILLER_PUSHING_HOST_PAST_BOUNDARY);
    let (first, second) = stream.split_at(PACKET_BOUNDARY);
    let second_sequence_number = FIRST_SEQUENCE_NUMBER + PACKET_BOUNDARY as u32;
    (
        tcp_packet(50004, FIRST_SEQUENCE_NUMBER, first),
        tcp_packet(50004, second_sequence_number, second),
    )
}

fn engine_with(positions: &[SplitPosition], order: Order) -> Engine {
    let strategy = Strategy {
        order,
        positions: positions.to_vec(),
    };
    let profile = Profile {
        domains: DomainList::new([LISTED]),
        tls: Some(strategy.clone()),
        http: Some(strategy),
    };
    let limits = Limits {
        idle_timeout_ms: IDLE_TIMEOUT_MS,
        ..Limits::default()
    };
    Engine::new(vec![profile], limits)
}

fn steam_like_engine() -> Engine {
    engine_with(
        &[SplitPosition::Offset(1), SplitPosition::DomainMid],
        Order::InSequence,
    )
}

enum Input {
    Packet(Vec<u8>, u32),
    RecvFails(io::ErrorKind),
    ClaimsMoreThanBuffer,
}

struct FakeIo {
    inputs: VecDeque<Input>,
    sends: Vec<(Vec<u8>, u32)>,
    failing_attempts: Vec<usize>,
    shut_down: bool,
}

impl FakeIo {
    fn new(inputs: Vec<Input>) -> Self {
        Self {
            inputs: inputs.into(),
            sends: Vec::new(),
            failing_attempts: Vec::new(),
            shut_down: false,
        }
    }

    fn with_packets(packets: Vec<(Vec<u8>, u32)>) -> Self {
        Self::new(
            packets
                .into_iter()
                .map(|(packet, address)| Input::Packet(packet, address))
                .collect(),
        )
    }

    fn sent_addresses(&self) -> Vec<u32> {
        self.sends.iter().map(|(_, address)| *address).collect()
    }

    fn sent_payloads(&self) -> Vec<&[u8]> {
        self.sends
            .iter()
            .map(|(packet, _)| parse_tcp(packet).unwrap().payload)
            .collect()
    }
}

impl PacketIo for FakeIo {
    type Address = u32;

    fn recv(&mut self, buf: &mut [u8]) -> io::Result<Option<(usize, u32)>> {
        assert!(!self.shut_down, "recv called after shutdown");
        match self.inputs.pop_front() {
            None => {
                self.shut_down = true;
                Ok(None)
            }
            Some(Input::Packet(packet, address)) => {
                buf[..packet.len()].copy_from_slice(&packet);
                Ok(Some((packet.len(), address)))
            }
            Some(Input::RecvFails(kind)) => Err(io::Error::from(kind)),
            Some(Input::ClaimsMoreThanBuffer) => Ok(Some((buf.len() + 1, 0))),
        }
    }

    fn send(&mut self, packet: &[u8], address: &u32) -> io::Result<()> {
        let attempt = self.sends.len();
        self.sends.push((packet.to_vec(), *address));
        if self.failing_attempts.contains(&attempt) {
            return Err(io::Error::from(io::ErrorKind::OutOfMemory));
        }
        Ok(())
    }
}

fn run_plain(io: &mut FakeIo, engine: &mut Engine) -> Result<Stats, RunError> {
    run(io, engine, || START_MS, |_, _| {})
}

#[test]
fn packets_that_are_not_split_are_resent_unchanged_with_their_address() {
    let inputs = vec![
        (unlisted_packet(), ADDRESS_A),
        (bare_ack_packet(), ADDRESS_B),
        (not_an_ip_packet(), ADDRESS_C),
    ];
    let mut io = FakeIo::with_packets(inputs.clone());
    let stats = run_plain(&mut io, &mut steam_like_engine()).unwrap();
    assert_eq!(io.sends, inputs);
    let expected = Stats {
        received: 3,
        passed: 3,
        ..Stats::default()
    };
    assert_eq!(stats, expected);
}

#[test]
fn a_full_size_packet_survives_the_receive_buffer() {
    let packet: Vec<u8> = (0..LARGEST_PACKET_LEN)
        .map(|index| (index % 251) as u8)
        .collect();
    let mut io = FakeIo::with_packets(vec![(packet.clone(), ADDRESS_A)]);
    run_plain(&mut io, &mut steam_like_engine()).unwrap();
    assert_eq!(io.sends, [(packet, ADDRESS_A)]);
}

#[test]
fn a_listed_client_hello_is_resent_only_as_its_pieces() {
    let original = listed_packet();
    let mut io = FakeIo::with_packets(vec![(original.clone(), ADDRESS_A)]);
    let stats = run_plain(&mut io, &mut steam_like_engine()).unwrap();

    let original_payload = parse_tcp(&original).unwrap().payload;
    assert!(
        original_payload
            .windows(LISTED_HOST.len())
            .any(|window| window == LISTED_HOST.as_bytes())
    );
    assert_eq!(io.sends.len(), 3);
    assert!(io.sends.iter().all(|(packet, _)| packet != &original));
    assert_eq!(io.sent_addresses(), [ADDRESS_A; 3]);
    assert_eq!(io.sent_payloads().concat(), original_payload);
    assert_eq!(io.sent_payloads()[0], &original_payload[..1]);
    assert!(io.sent_payloads()[1].ends_with(HOST_BEFORE_DOMAIN_MID.as_bytes()));
    assert!(io.sent_payloads()[2].starts_with(HOST_AFTER_DOMAIN_MID.as_bytes()));
    let expected = Stats {
        received: 1,
        replaced: 1,
        segments_sent: 3,
        ..Stats::default()
    };
    assert_eq!(stats, expected);
}

#[test]
fn reversed_order_pieces_are_sent_last_first() {
    let mut engine = engine_with(
        &[SplitPosition::Offset(1), SplitPosition::DomainMid],
        Order::Reversed,
    );
    let original = listed_packet();
    let mut io = FakeIo::with_packets(vec![(original.clone(), ADDRESS_A)]);
    run_plain(&mut io, &mut engine).unwrap();

    let original_payload = parse_tcp(&original).unwrap().payload;
    let payloads = io.sent_payloads();
    assert_eq!(payloads.len(), 3);
    assert!(payloads[0].starts_with(HOST_AFTER_DOMAIN_MID.as_bytes()));
    assert_eq!(payloads[2], &original_payload[..1]);
}

#[test]
fn arrival_order_is_kept_around_a_split_packet() {
    let before = unlisted_packet();
    let after = hello_packet(UNLISTED_HOST, 50009);
    let mut io = FakeIo::with_packets(vec![
        (before.clone(), ADDRESS_A),
        (listed_packet(), ADDRESS_B),
        (after.clone(), ADDRESS_C),
    ]);
    let stats = run_plain(&mut io, &mut steam_like_engine()).unwrap();
    assert_eq!(
        io.sent_addresses(),
        [ADDRESS_A, ADDRESS_B, ADDRESS_B, ADDRESS_B, ADDRESS_C]
    );
    assert_eq!(io.sends[0].0, before);
    assert_eq!(io.sends[4].0, after);
    let expected = Stats {
        received: 3,
        passed: 2,
        replaced: 1,
        segments_sent: 3,
        send_failures: 0,
    };
    assert_eq!(stats, expected);
}

#[test]
fn send_failures_are_counted_and_later_packets_still_flow() {
    let later = unlisted_packet();
    let mut io = FakeIo::with_packets(vec![
        (listed_packet(), ADDRESS_A),
        (later.clone(), ADDRESS_B),
    ]);
    io.failing_attempts = vec![1, 3];
    let stats = run_plain(&mut io, &mut steam_like_engine()).unwrap();
    assert_eq!(io.sends.len(), 4);
    assert_eq!(io.sends[3], (later, ADDRESS_B));
    let expected = Stats {
        received: 2,
        passed: 1,
        replaced: 1,
        segments_sent: 2,
        send_failures: 2,
    };
    assert_eq!(stats, expected);
}

#[test]
fn a_receive_error_stops_the_loop_and_reports_the_counts_so_far() {
    let mut io = FakeIo::new(vec![
        Input::Packet(unlisted_packet(), ADDRESS_A),
        Input::RecvFails(io::ErrorKind::ConnectionReset),
        Input::Packet(unlisted_packet(), ADDRESS_B),
    ]);
    let failure = run_plain(&mut io, &mut steam_like_engine()).unwrap_err();
    assert_eq!(failure.error.kind(), io::ErrorKind::ConnectionReset);
    assert_eq!(failure.stats.received, 1);
    assert_eq!(failure.stats.passed, 1);
    assert_eq!(io.sends.len(), 1);
    assert_eq!(io.inputs.len(), 1);
}

#[test]
fn a_length_beyond_the_buffer_is_an_error_not_a_panic() {
    let mut io = FakeIo::new(vec![Input::ClaimsMoreThanBuffer]);
    let failure = run_plain(&mut io, &mut steam_like_engine()).unwrap_err();
    assert_eq!(failure.error.kind(), io::ErrorKind::InvalidData);
    assert!(io.sends.is_empty());
}

#[test]
fn shutdown_with_no_traffic_returns_zero_counts_and_sends_nothing() {
    let mut io = FakeIo::new(Vec::new());
    let stats = run_plain(&mut io, &mut steam_like_engine()).unwrap();
    assert_eq!(stats, Stats::default());
    assert!(io.sends.is_empty());
}

#[test]
fn the_clock_is_read_for_every_packet_and_reaches_the_flow_table() {
    let (first, second) = host_in_second_packet();
    let positions = [SplitPosition::DomainMid];
    let sent_when_second_arrives_after = |gap_ms: u64| {
        let mut io = FakeIo::with_packets(vec![
            (first.clone(), ADDRESS_A),
            (second.clone(), ADDRESS_A),
        ]);
        let mut engine = engine_with(&positions, Order::InSequence);
        let mut times = [START_MS, START_MS + gap_ms].into_iter();
        run(&mut io, &mut engine, || times.next().unwrap(), |_, _| {}).unwrap();
        io.sends
    };

    let prompt = sent_when_second_arrives_after(SHORTER_THAN_IDLE_MS);
    assert_eq!(prompt.len(), 3);
    assert_eq!(prompt[0].0, first);
    let tail = parse_tcp(&prompt[2].0).unwrap().payload.to_vec();
    assert!(tail.starts_with(HOST_AFTER_DOMAIN_MID.as_bytes()));

    let late = sent_when_second_arrives_after(LONGER_THAN_IDLE_MS);
    assert_eq!(late, [(first, ADDRESS_A), (second, ADDRESS_A)]);
}

#[test]
fn the_split_observer_sees_each_split_packet_and_its_pieces_only() {
    let original = listed_packet();
    let mut io = FakeIo::with_packets(vec![
        (unlisted_packet(), ADDRESS_A),
        (original.clone(), ADDRESS_B),
    ]);
    let mut observed: Vec<(Vec<u8>, Vec<Vec<u8>>)> = Vec::new();
    run(
        &mut io,
        &mut steam_like_engine(),
        || START_MS,
        |packet, segments| observed.push((packet.to_vec(), segments.to_vec())),
    )
    .unwrap();
    assert_eq!(observed.len(), 1);
    assert_eq!(observed[0].0, original);
    let resent: Vec<Vec<u8>> = io.sends[1..]
        .iter()
        .map(|(packet, _)| packet.clone())
        .collect();
    assert_eq!(observed[0].1, resent);
}
