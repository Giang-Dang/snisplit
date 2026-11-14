mod common;

use std::ops::Range;

use common::{fixture_stems, read_fixture_file, read_stream};
use snisplit_core::inspect::Inspection;
use snisplit_core::packet::parse_tcp;
use snisplit_core::segment::{Order, split};
use snisplit_core::tls::inspect_client_hello;

const PACKET_LENGTHS_EXTENSION: &str = "txt";
const MIN_TWO_PACKET_FIXTURES: usize = 1;

const IPV4_VERSION_AND_MIN_HEADER: u8 = 0x45;
const IPV4_TYPE_OF_SERVICE: u8 = 0;
const IPV4_IDENTIFICATION: u16 = 0x1234;
const IPV4_DONT_FRAGMENT: u16 = 0x4000;
const IPV4_TIME_TO_LIVE: u8 = 64;
const IPV4_PROTOCOL_TCP: u8 = 6;
const IPV4_UNFILLED_CHECKSUM: u16 = 0;
const CLIENT_ADDRESS: [u8; 4] = [10, 0, 0, 2];
const SERVER_ADDRESS: [u8; 4] = [203, 0, 113, 9];

const CLIENT_PORT: u16 = 50_000;
const HTTPS_PORT: u16 = 443;
const TCP_INITIAL_SEQUENCE_NUMBER: u32 = 0xffff_ff00;
const TCP_ACKNOWLEDGMENT_NUMBER: u32 = 1;
const TCP_DATA_OFFSET_WITHOUT_OPTIONS: u8 = 5 << 4;
const TCP_FLAGS_PSH_ACK: u8 = 0x18;
const TCP_WINDOW: u16 = 65_535;
const TCP_UNFILLED_CHECKSUM: u16 = 0;
const TCP_URGENT_POINTER: u16 = 0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HostPosition {
    WhollyInFirstPacket,
    WhollyInSecondPacket,
    StraddlesBoundary,
    SinglePacket,
}

struct Fixture {
    stem: String,
    host_name: String,
    stream: Vec<u8>,
    packet_lengths: Vec<usize>,
}

impl Fixture {
    fn load(stem: String) -> Self {
        let host_name = host_name_of(&stem);
        let stream = read_stream(&stem);
        let packet_lengths = read_packet_lengths(&stem);
        assert_eq!(
            packet_lengths.iter().sum::<usize>(),
            stream.len(),
            "{stem}: packet lengths do not add up to the stream length"
        );
        Self {
            stem,
            host_name,
            stream,
            packet_lengths,
        }
    }

    fn host(&self) -> Range<usize> {
        match inspect_client_hello(&self.stream) {
            Inspection::Host(host) => host,
            other => panic!("{}: whole stream gave {other:?}", self.stem),
        }
    }

    fn packet_ranges(&self) -> Vec<Range<usize>> {
        let mut start = 0;
        self.packet_lengths
            .iter()
            .map(|&len| {
                let range = start..start + len;
                start = range.end;
                range
            })
            .collect()
    }

    fn host_position(&self) -> HostPosition {
        let host = self.host();
        match self.packet_ranges().as_slice() {
            [_] => HostPosition::SinglePacket,
            [first, ..] if host.end <= first.end => HostPosition::WhollyInFirstPacket,
            [first, ..] if host.start >= first.end => HostPosition::WhollyInSecondPacket,
            _ => HostPosition::StraddlesBoundary,
        }
    }
}

fn read_packet_lengths(stem: &str) -> Vec<usize> {
    let text = String::from_utf8(read_fixture_file(stem, PACKET_LENGTHS_EXTENSION))
        .expect("packet lengths file is text");
    text.split_whitespace()
        .map(|word| word.parse().expect("packet length is a number"))
        .collect()
}

fn host_name_of(stem: &str) -> String {
    let (host_name, _connection_number) = stem
        .rsplit_once('-')
        .unwrap_or_else(|| panic!("{stem}: file name has no -<n> suffix"));
    host_name.to_owned()
}

fn fixtures() -> Vec<Fixture> {
    fixture_stems().into_iter().map(Fixture::load).collect()
}

fn ipv4_header(total_len: u16) -> Vec<u8> {
    [
        vec![IPV4_VERSION_AND_MIN_HEADER, IPV4_TYPE_OF_SERVICE],
        total_len.to_be_bytes().to_vec(),
        IPV4_IDENTIFICATION.to_be_bytes().to_vec(),
        IPV4_DONT_FRAGMENT.to_be_bytes().to_vec(),
        vec![IPV4_TIME_TO_LIVE, IPV4_PROTOCOL_TCP],
        IPV4_UNFILLED_CHECKSUM.to_be_bytes().to_vec(),
        CLIENT_ADDRESS.to_vec(),
        SERVER_ADDRESS.to_vec(),
    ]
    .concat()
}

fn tcp_header() -> Vec<u8> {
    [
        CLIENT_PORT.to_be_bytes().to_vec(),
        HTTPS_PORT.to_be_bytes().to_vec(),
        TCP_INITIAL_SEQUENCE_NUMBER.to_be_bytes().to_vec(),
        TCP_ACKNOWLEDGMENT_NUMBER.to_be_bytes().to_vec(),
        vec![TCP_DATA_OFFSET_WITHOUT_OPTIONS, TCP_FLAGS_PSH_ACK],
        TCP_WINDOW.to_be_bytes().to_vec(),
        TCP_UNFILLED_CHECKSUM.to_be_bytes().to_vec(),
        TCP_URGENT_POINTER.to_be_bytes().to_vec(),
    ]
    .concat()
}

fn ipv4_tcp_packet(payload: &[u8]) -> Vec<u8> {
    let tcp_header = tcp_header();
    let total_len = ipv4_header(0).len() + tcp_header.len() + payload.len();
    let ip_header = ipv4_header(u16::try_from(total_len).expect("packet fits in an IPv4 length"));
    [ip_header.as_slice(), &tcp_header, payload].concat()
}

fn middle_of_host_inside_packet(host: &Range<usize>, packet: &Range<usize>) -> Option<usize> {
    let start = host.start.max(packet.start);
    let end = host.end.min(packet.end);
    (start < end).then(|| (start + (end - start) / 2) - packet.start)
}

#[test]
fn fixture_directory_is_not_empty() {
    assert!(!fixture_stems().is_empty(), "no ClientHello fixtures found");
}

#[test]
fn whole_stream_yields_the_host_named_by_the_file() {
    for fixture in fixtures() {
        let host = fixture.host();
        assert_eq!(
            &fixture.stream[host],
            fixture.host_name.as_bytes(),
            "{}",
            fixture.stem
        );
    }
}

#[test]
fn every_prefix_needs_more_data_or_matches_the_full_result() {
    for fixture in fixtures() {
        let full = Inspection::Host(fixture.host());
        for cut in 0..=fixture.stream.len() {
            let result = inspect_client_hello(&fixture.stream[..cut]);
            assert!(
                result == Inspection::NeedMoreData || result == full,
                "{}: prefix {cut} gave {result:?}, full is {full:?}",
                fixture.stem
            );
        }
    }
}

#[test]
fn first_packet_alone_needs_more_data_unless_it_holds_the_whole_host() {
    for fixture in fixtures() {
        let host = fixture.host();
        let first_packet = fixture.packet_ranges()[0].clone();
        let result = inspect_client_hello(&fixture.stream[first_packet.clone()]);
        let expected = if host.end <= first_packet.end {
            Inspection::Host(host)
        } else {
            Inspection::NeedMoreData
        };
        assert_eq!(result, expected, "{}", fixture.stem);
    }
}

#[test]
fn host_position_relative_to_the_packet_boundary_is_reported() {
    let fixtures = fixtures();
    for fixture in &fixtures {
        println!(
            "{}: packets {:?}, host {:?}, {:?}",
            fixture.stem,
            fixture.packet_lengths,
            fixture.host(),
            fixture.host_position()
        );
    }
    let two_packet_fixtures = fixtures
        .iter()
        .filter(|fixture| fixture.packet_lengths.len() == 2)
        .count();
    assert!(
        two_packet_fixtures >= MIN_TWO_PACKET_FIXTURES,
        "no two-packet ClientHello among the fixtures"
    );
}

#[test]
fn splitting_in_the_middle_of_the_host_keeps_every_payload_intact() {
    let mut split_count = 0;
    for fixture in fixtures() {
        let host = fixture.host();
        for packet_range in fixture.packet_ranges() {
            let payload = &fixture.stream[packet_range.clone()];
            let Some(offset) = middle_of_host_inside_packet(&host, &packet_range) else {
                continue;
            };
            let packet = ipv4_tcp_packet(payload);
            let segments = split(&packet, &[offset], Order::InSequence)
                .unwrap_or_else(|| panic!("{}: split refused offset {offset}", fixture.stem));
            let payloads: Vec<&[u8]> = segments
                .iter()
                .map(|segment| parse_tcp(segment).expect("segment parses").payload)
                .collect();
            assert_eq!(payloads.len(), 2, "{}", fixture.stem);
            assert_eq!(payloads[0].len(), offset, "{}", fixture.stem);
            assert_eq!(payloads.concat(), payload, "{}", fixture.stem);
            split_count += 1;
        }
    }
    assert!(split_count > 0, "no fixture was split");
}
