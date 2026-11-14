use std::net::Ipv4Addr;

use super::*;

const CLIENT: IpAddr = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 23));
const SERVER: IpAddr = IpAddr::V4(Ipv4Addr::new(23, 54, 187, 9));
const CLIENT_PORT: u16 = 50123;
const SERVER_PORT: u16 = 443;
const FIRST_SEQUENCE_NUMBER: u32 = 0x1000_0000;
const START_MS: u64 = 1_000;
const IDLE_TIMEOUT_MS: u64 = 5_000;
const MAX_FLOWS: usize = 2;
const MAX_PREFIX_BYTES: usize = 64;
const HANDSHAKE_START: [u8; 4] = [0x16, 0x03, 0x01, 0x00];
const APPLICATION_DATA_START: [u8; 4] = [0x17, 0x03, 0x03, 0x00];

fn limits() -> Limits {
    Limits {
        max_flows: MAX_FLOWS,
        max_prefix_bytes: MAX_PREFIX_BYTES,
        idle_timeout_ms: IDLE_TIMEOUT_MS,
    }
}

fn tcp_from(src_port: u16, sequence_number: u32, payload: &[u8]) -> TcpPacket<'_> {
    TcpPacket {
        src: CLIENT,
        dst: SERVER,
        src_port,
        dst_port: SERVER_PORT,
        sequence_number,
        ip_header_len: 20,
        tcp_header_len: 20,
        payload,
    }
}

fn handshake_payload(len: usize) -> Vec<u8> {
    let mut payload: Vec<u8> = (0..len).map(|index| index as u8).collect();
    let header_len = HANDSHAKE_START.len().min(len);
    payload[..header_len].copy_from_slice(&HANDSHAKE_START[..header_len]);
    payload
}

fn starts_flow(table: &mut FlowTable, src_port: u16, now_ms: u64) -> bool {
    let payload = handshake_payload(8);
    table
        .flow_for(&tcp_from(src_port, FIRST_SEQUENCE_NUMBER, &payload), now_ms)
        .is_some()
}

fn cuts_at(stream_offsets: &[usize]) -> Cuts {
    Cuts {
        order: Order::InSequence,
        stream_offsets: stream_offsets.to_vec(),
    }
}

struct Recorder {
    seen: Vec<Vec<u8>>,
}

impl Recorder {
    fn new() -> Self {
        Self { seen: Vec::new() }
    }

    fn feed(
        &mut self,
        flow: &mut Flow,
        sequence_number: u32,
        payload: &[u8],
        decide_at_len: usize,
        stream_offsets: &[usize],
    ) -> Option<PacketCuts> {
        flow.packet_cuts(sequence_number, payload, |prefix| {
            self.seen.push(prefix.to_vec());
            if prefix.len() >= decide_at_len {
                Progress::Decided(Some(cuts_at(stream_offsets)))
            } else {
                Progress::NeedMoreData
            }
        })
    }
}

fn payload_cuts(payload_offsets: &[usize]) -> Option<PacketCuts> {
    Some(PacketCuts {
        order: Order::InSequence,
        payload_offsets: payload_offsets.to_vec(),
    })
}

#[test]
fn only_tls_handshake_payloads_start_tracking() {
    let mut table = FlowTable::new(limits());
    let application_data = tcp_from(CLIENT_PORT, FIRST_SEQUENCE_NUMBER, &APPLICATION_DATA_START);
    assert!(table.flow_for(&application_data, START_MS).is_none());
    assert!(
        table
            .flow_for(&tcp_from(CLIENT_PORT, 0, &[]), START_MS)
            .is_none()
    );
    assert!(table.flows.is_empty());
    assert!(starts_flow(&mut table, CLIENT_PORT, START_MS));
    assert!(table.flow_for(&application_data, START_MS).is_some());
    assert_eq!(table.flows.len(), 1);
}

#[test]
fn flows_are_keyed_by_the_four_tuple() {
    let mut table = FlowTable::new(Limits {
        max_flows: 8,
        ..limits()
    });
    let payload = handshake_payload(8);
    let original = tcp_from(CLIENT_PORT, FIRST_SEQUENCE_NUMBER, &payload);
    let other_client = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 24));
    let other_server = IpAddr::V4(Ipv4Addr::new(23, 54, 187, 10));
    let variants = [
        original.clone(),
        TcpPacket {
            src: other_client,
            ..original.clone()
        },
        TcpPacket {
            src_port: CLIENT_PORT + 1,
            ..original.clone()
        },
        TcpPacket {
            dst: other_server,
            ..original.clone()
        },
        TcpPacket {
            dst_port: SERVER_PORT + 1,
            ..original.clone()
        },
    ];
    for (index, packet) in variants.iter().enumerate() {
        assert!(
            table.flow_for(packet, START_MS).is_some(),
            "variant {index}"
        );
        assert_eq!(table.flows.len(), index + 1, "variant {index}");
    }
    assert!(table.flow_for(&original, START_MS).is_some());
    assert_eq!(table.flows.len(), variants.len());
}

#[test]
fn full_table_does_not_track_new_flows() {
    let mut table = FlowTable::new(limits());
    for port in 0..MAX_FLOWS as u16 {
        assert!(starts_flow(&mut table, CLIENT_PORT + port, START_MS));
    }
    let later = START_MS + IDLE_TIMEOUT_MS - 1;
    assert!(!starts_flow(&mut table, CLIENT_PORT + 100, later));
    assert_eq!(table.flows.len(), MAX_FLOWS);
    assert!(starts_flow(&mut table, CLIENT_PORT, later));
}

#[test]
fn idle_flows_make_room_for_new_ones() {
    let mut table = FlowTable::new(limits());
    assert!(starts_flow(&mut table, CLIENT_PORT, START_MS));
    assert!(starts_flow(&mut table, CLIENT_PORT + 1, START_MS + 10));
    let first_is_idle = START_MS + IDLE_TIMEOUT_MS;
    assert!(starts_flow(&mut table, CLIENT_PORT + 2, first_is_idle));
    assert_eq!(table.flows.len(), MAX_FLOWS);
    let application_data = APPLICATION_DATA_START;
    let first_flow = tcp_from(CLIENT_PORT, FIRST_SEQUENCE_NUMBER, &application_data);
    assert!(table.flow_for(&first_flow, first_is_idle).is_none());
}

#[test]
fn idle_flow_is_forgotten_on_lookup() {
    let mut table = FlowTable::new(limits());
    assert!(starts_flow(&mut table, CLIENT_PORT, START_MS));
    let application_data = APPLICATION_DATA_START;
    let packet = tcp_from(CLIENT_PORT, FIRST_SEQUENCE_NUMBER, &application_data);
    let still_active = START_MS + IDLE_TIMEOUT_MS - 1;
    assert!(table.flow_for(&packet, still_active).is_some());
    let idle = still_active + IDLE_TIMEOUT_MS;
    assert!(table.flow_for(&packet, idle).is_none());
    assert!(table.flows.is_empty());
}

#[test]
fn time_going_backwards_does_not_expire_flows() {
    let mut table = FlowTable::new(limits());
    assert!(starts_flow(
        &mut table,
        CLIENT_PORT,
        START_MS + IDLE_TIMEOUT_MS
    ));
    let application_data = APPLICATION_DATA_START;
    let packet = tcp_from(CLIENT_PORT, FIRST_SEQUENCE_NUMBER, &application_data);
    assert!(table.flow_for(&packet, 0).is_some());
}

#[test]
fn zero_max_flows_tracks_nothing() {
    let mut table = FlowTable::new(Limits {
        max_flows: 0,
        ..limits()
    });
    assert!(!starts_flow(&mut table, CLIENT_PORT, START_MS));
}

#[test]
fn prefix_never_grows_past_the_byte_cap() {
    let mut flow = Flow::new(FIRST_SEQUENCE_NUMBER, START_MS, MAX_PREFIX_BYTES);
    let mut recorder = Recorder::new();
    let oversized = handshake_payload(MAX_PREFIX_BYTES * 3);
    let never = usize::MAX;
    let far = FIRST_SEQUENCE_NUMBER + (MAX_PREFIX_BYTES as u32) * 10;
    assert_eq!(recorder.feed(&mut flow, far, &oversized, never, &[]), None);
    let FlowState::Collecting(prefix) = &flow.state else {
        panic!("decided too early");
    };
    assert!(prefix.bytes.is_empty());
    assert_eq!(
        recorder.feed(&mut flow, FIRST_SEQUENCE_NUMBER, &oversized, never, &[]),
        None
    );
    assert_eq!(
        recorder.seen.last().unwrap(),
        &oversized[..MAX_PREFIX_BYTES]
    );
    assert!(matches!(flow.state, FlowState::Decided(None)));
}

#[test]
fn full_prefix_without_a_host_gives_up() {
    let mut flow = Flow::new(FIRST_SEQUENCE_NUMBER, START_MS, MAX_PREFIX_BYTES);
    let mut recorder = Recorder::new();
    let payload = handshake_payload(MAX_PREFIX_BYTES);
    let never = usize::MAX;
    assert_eq!(
        recorder.feed(&mut flow, FIRST_SEQUENCE_NUMBER, &payload, never, &[]),
        None
    );
    assert!(matches!(flow.state, FlowState::Decided(None)));
    let calls = recorder.seen.len();
    assert_eq!(
        recorder.feed(&mut flow, FIRST_SEQUENCE_NUMBER, &payload, 0, &[5]),
        None
    );
    assert_eq!(recorder.seen.len(), calls);
}

#[test]
fn packets_before_the_first_byte_are_ignored() {
    let mut flow = Flow::new(FIRST_SEQUENCE_NUMBER, START_MS, MAX_PREFIX_BYTES);
    let mut recorder = Recorder::new();
    let payload = handshake_payload(16);
    let before = FIRST_SEQUENCE_NUMBER - 8;
    assert_eq!(recorder.feed(&mut flow, before, &payload, 0, &[2]), None);
    assert!(recorder.seen.is_empty());
    assert_eq!(
        recorder.feed(&mut flow, FIRST_SEQUENCE_NUMBER, &payload, 0, &[2]),
        payload_cuts(&[2])
    );
    assert_eq!(recorder.feed(&mut flow, before, &payload, 0, &[2]), None);
}

#[test]
fn gaps_hold_back_the_prefix_until_filled() {
    let stream = handshake_payload(30);
    let (first, rest) = stream.split_at(10);
    let (second, third) = rest.split_at(10);
    let mut flow = Flow::new(FIRST_SEQUENCE_NUMBER, START_MS, MAX_PREFIX_BYTES);
    let mut recorder = Recorder::new();
    let whole = stream.len();
    let offsets = [5, 15, 25];
    assert_eq!(
        recorder.feed(&mut flow, FIRST_SEQUENCE_NUMBER, first, whole, &offsets),
        None
    );
    assert_eq!(
        recorder.feed(
            &mut flow,
            FIRST_SEQUENCE_NUMBER + 20,
            third,
            whole,
            &offsets
        ),
        None
    );
    assert_eq!(
        recorder.feed(
            &mut flow,
            FIRST_SEQUENCE_NUMBER + 10,
            second,
            whole,
            &offsets
        ),
        payload_cuts(&[5])
    );
    assert_eq!(
        recorder.seen,
        [first.to_vec(), first.to_vec(), stream.clone()]
    );
    assert_eq!(
        recorder.feed(&mut flow, FIRST_SEQUENCE_NUMBER, first, whole, &offsets),
        payload_cuts(&[])
    );
    assert_eq!(
        recorder.feed(
            &mut flow,
            FIRST_SEQUENCE_NUMBER + 20,
            third,
            whole,
            &offsets
        ),
        payload_cuts(&[])
    );
    assert_eq!(
        recorder.feed(
            &mut flow,
            FIRST_SEQUENCE_NUMBER + 10,
            second,
            whole,
            &offsets
        ),
        payload_cuts(&[5])
    );
}

#[test]
fn decided_cuts_apply_to_later_packets() {
    let mut flow = Flow::new(FIRST_SEQUENCE_NUMBER, START_MS, MAX_PREFIX_BYTES);
    let mut recorder = Recorder::new();
    let payload = handshake_payload(10);
    let later_offsets = [3, 14, 19, 20, 25];
    assert_eq!(
        recorder.feed(
            &mut flow,
            FIRST_SEQUENCE_NUMBER,
            &payload,
            0,
            &later_offsets
        ),
        payload_cuts(&[3])
    );
    assert_eq!(
        recorder.feed(&mut flow, FIRST_SEQUENCE_NUMBER + 10, &payload, 0, &[]),
        payload_cuts(&[4, 9])
    );
    assert_eq!(recorder.seen.len(), 1);
}

#[test]
fn sequence_numbers_wrap_around() {
    let first = u32::MAX - 4;
    let mut flow = Flow::new(first, START_MS, MAX_PREFIX_BYTES);
    let mut recorder = Recorder::new();
    let stream = handshake_payload(20);
    let (head, tail) = stream.split_at(10);
    assert_eq!(recorder.feed(&mut flow, first, head, 20, &[12]), None);
    assert_eq!(
        recorder.feed(&mut flow, first.wrapping_add(10), tail, 20, &[12]),
        payload_cuts(&[2])
    );
    assert_eq!(recorder.seen.last().unwrap(), &stream);
}
