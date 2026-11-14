use std::fs;
use std::net::Ipv4Addr;
use std::path::PathBuf;

use snisplit_core::dns::{
    Question, TYPE_A, TYPE_AAAA, TYPE_HTTPS, answer_a, answer_empty, build_a_query, parse_a_answer,
    parse_query,
};

const CLASS_IN: u16 = 1;
const TYPE_CNAME: u16 = 5;
const FLAGS_QUERY_RD: u16 = 0x0100;
const FLAGS_RESPONSE_RD_RA: u16 = 0x8180;
const FLAGS_RESPONSE_RA: u16 = 0x8080;
const HEADER_LEN: usize = 12;
const FLAGS_OFFSET: usize = 2;
const ADDITIONAL_COUNT_OFFSET: usize = 11;
const MEDIUM_FIRST_RDLEN_OFFSET: usize = 38;
const WWW_TEST_RECORDS_START: usize = 26;
const EDNS_OPT_RECORD: &str = "00002910000000000000";

fn fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("dns")
        .join(format!("{name}.bin"));
    fs::read(&path).unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}

fn hex(digits: &str) -> Vec<u8> {
    (0..digits.len())
        .step_by(2)
        .map(|start| u8::from_str_radix(&digits[start..start + 2], 16).expect("hex digits"))
        .collect()
}

fn ip(a: u8, b: u8, c: u8, d: u8) -> Ipv4Addr {
    Ipv4Addr::new(a, b, c, d)
}

fn wire_name(labels: &[&[u8]]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for label in labels {
        bytes.push(u8::try_from(label.len()).expect("label length fits"));
        bytes.extend_from_slice(label);
    }
    bytes.push(0);
    bytes
}

fn header(id: u16, flags: u16, counts: [u16; 4]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for field in [id, flags, counts[0], counts[1], counts[2], counts[3]] {
        bytes.extend_from_slice(&field.to_be_bytes());
    }
    bytes
}

fn query_for(name_bytes: &[u8], qtype: u16, flags: u16) -> Vec<u8> {
    let mut bytes = header(0x4242, flags, [1, 0, 0, 0]);
    bytes.extend_from_slice(name_bytes);
    bytes.extend_from_slice(&qtype.to_be_bytes());
    bytes.extend_from_slice(&CLASS_IN.to_be_bytes());
    bytes
}

fn record(owner: &[u8], record_type: u16, rdata: &[u8]) -> Vec<u8> {
    let mut bytes = owner.to_vec();
    bytes.extend_from_slice(&record_type.to_be_bytes());
    bytes.extend_from_slice(&CLASS_IN.to_be_bytes());
    bytes.extend_from_slice(&60u32.to_be_bytes());
    bytes.extend_from_slice(
        &u16::try_from(rdata.len())
            .expect("rdata fits")
            .to_be_bytes(),
    );
    bytes.extend_from_slice(rdata);
    bytes
}

fn response_with(question: &[u8], records: &[Vec<u8>]) -> Vec<u8> {
    let count = u16::try_from(records.len()).expect("record count fits");
    let mut bytes = header(0x4242, FLAGS_RESPONSE_RD_RA, [1, count, 0, 0]);
    bytes.extend_from_slice(question);
    bytes.extend_from_slice(&TYPE_A.to_be_bytes());
    bytes.extend_from_slice(&CLASS_IN.to_be_bytes());
    for record in records {
        bytes.extend_from_slice(record);
    }
    bytes
}

fn with_byte(mut bytes: Vec<u8>, index: usize, value: u8) -> Vec<u8> {
    bytes[index] = value;
    bytes
}

fn question(id: u16, name: &str, qtype: u16) -> Question {
    Question {
        id,
        name: name.to_owned(),
        qtype,
        qclass: CLASS_IN,
    }
}

fn a_label_of(length: usize) -> Vec<u8> {
    vec![b'a'; length]
}

#[test]
fn real_windows_queries_parse_to_their_question() {
    let cases = [
        (
            "windows_query_medium_com_a",
            question(0x6692, "medium.com", TYPE_A),
        ),
        (
            "windows_query_medium_com_aaaa",
            question(0x54e5, "medium.com", TYPE_AAAA),
        ),
        (
            "windows_query_medium_com_https",
            question(0x11d0, "medium.com", TYPE_HTTPS),
        ),
    ];
    for (stem, expected) in cases {
        assert_eq!(parse_query(&fixture(stem)), Some(expected), "{stem}");
    }
}

#[test]
fn build_a_query_reproduces_the_real_windows_a_query_byte_for_byte() {
    let built = build_a_query(0x6692, "medium.com");

    assert_eq!(built, Some(fixture("windows_query_medium_com_a")));
}

#[test]
fn query_name_is_lowercased_and_other_classes_pass_through() {
    let mut packet = query_for(&wire_name(&[b"MeDiUm", b"CoM"]), TYPE_A, FLAGS_QUERY_RD);
    let class_offset = packet.len() - 2;
    packet[class_offset + 1] = 3;

    let parsed = parse_query(&packet).expect("parses");

    assert_eq!(parsed.name, "medium.com");
    assert_eq!(parsed.qclass, 3);
}

#[test]
fn query_with_edns_record_parses() {
    let mut packet = fixture("windows_query_medium_com_a");
    packet.extend_from_slice(&hex(EDNS_OPT_RECORD));
    let packet = with_byte(packet, ADDITIONAL_COUNT_OFFSET, 1);

    assert_eq!(
        parse_query(&packet),
        Some(question(0x6692, "medium.com", TYPE_A))
    );
}

#[test]
fn queries_that_are_not_one_standard_question_are_rejected() {
    let real = fixture("windows_query_medium_com_a");
    let response_bit = with_byte(real.clone(), FLAGS_OFFSET, 0x81);
    let inverse_opcode = with_byte(real.clone(), FLAGS_OFFSET, 0x09);
    let two_questions = with_byte(real.clone(), 5, 2);
    let no_question = with_byte(real.clone(), 5, 0);
    let has_answer = with_byte(real.clone(), 7, 1);
    let has_authority = with_byte(real.clone(), 9, 1);
    for (label, packet) in [
        ("QR set", response_bit),
        ("opcode 1", inverse_opcode),
        ("two questions", two_questions),
        ("no question", no_question),
        ("answer count", has_answer),
        ("authority count", has_authority),
        ("empty", Vec::new()),
    ] {
        assert_eq!(parse_query(&packet), None, "{label}");
    }
}

#[test]
fn every_truncation_of_a_real_query_is_rejected() {
    let real = fixture("windows_query_medium_com_https");

    for length in 0..real.len() {
        assert_eq!(parse_query(&real[..length]), None, "length {length}");
    }
}

#[test]
fn compression_pointer_in_a_query_is_rejected() {
    let mut packet = header(1, FLAGS_QUERY_RD, [1, 0, 0, 0]);
    packet.extend_from_slice(&[0xc0, 0x0c, 0, 1, 0, 1]);
    let mut after_label = header(1, FLAGS_QUERY_RD, [1, 0, 0, 0]);
    after_label.extend_from_slice(&[1, b'a', 0xc0, 0x0c, 0, 1, 0, 1]);

    assert_eq!(parse_query(&packet), None);
    assert_eq!(parse_query(&after_label), None);
}

#[test]
fn label_and_name_length_limits_hold_at_their_boundaries() {
    let longest_label = a_label_of(63);
    let too_long_label_tag = {
        let mut bytes = vec![64];
        bytes.extend_from_slice(&a_label_of(64));
        bytes.push(0);
        bytes
    };
    let name_255 = wire_name(&[
        &longest_label,
        &longest_label,
        &longest_label,
        &a_label_of(61),
    ]);
    let name_256 = wire_name(&[
        &longest_label,
        &longest_label,
        &longest_label,
        &a_label_of(62),
    ]);

    assert_eq!(name_255.len(), 255);
    assert_eq!(name_256.len(), 256);
    let accepted_label = parse_query(&query_for(
        &wire_name(&[&longest_label]),
        TYPE_A,
        FLAGS_QUERY_RD,
    ));
    assert_eq!(accepted_label.map(|q| q.name.len()), Some(63));
    let accepted_name = parse_query(&query_for(&name_255, TYPE_A, FLAGS_QUERY_RD));
    assert_eq!(accepted_name.map(|q| q.name.len()), Some(3 * 64 + 61));
    assert_eq!(
        parse_query(&query_for(&name_256, TYPE_A, FLAGS_QUERY_RD)),
        None
    );
    assert_eq!(
        parse_query(&query_for(&too_long_label_tag, TYPE_A, FLAGS_QUERY_RD)),
        None
    );
}

#[test]
fn build_a_query_enforces_the_same_limits_and_character_rules() {
    let label_63 = "a".repeat(63);
    let name_254_text = format!("{label_63}.{label_63}.{label_63}.{}", "a".repeat(61));
    let name_255_text = format!("{label_63}.{label_63}.{label_63}.{}", "a".repeat(62));

    assert!(build_a_query(1, &label_63).is_some());
    assert!(build_a_query(1, &name_254_text).is_some());
    assert_eq!(build_a_query(1, &name_255_text), None);
    assert_eq!(build_a_query(1, &"a".repeat(64)), None);
    for invalid in ["", ".", "a..b", ".a", "exa mple.com", "caf\u{e9}.com"] {
        assert_eq!(build_a_query(1, invalid), None, "{invalid:?}");
    }
    assert_eq!(
        build_a_query(7, "medium.com."),
        build_a_query(7, "medium.com")
    );
}

#[test]
fn answer_a_matches_hand_derived_bytes_for_a_real_query() {
    let query = fixture("windows_query_medium_com_a");
    let expected = hex(concat!(
        "6692",
        "8180",
        "0001",
        "0001",
        "0000",
        "0000",
        "066d656469756d03636f6d0000010001",
        "c00c",
        "0001",
        "0001",
        "00000005",
        "0004",
        "7f000002",
    ));

    assert_eq!(answer_a(&query, ip(127, 0, 0, 2), 5), Some(expected));
}

#[test]
fn answer_a_drops_edns_and_copies_recursion_desired() {
    let plain = fixture("windows_query_medium_com_a");
    let mut with_edns = plain.clone();
    with_edns.extend_from_slice(&hex(EDNS_OPT_RECORD));
    let with_edns = with_byte(with_edns, ADDITIONAL_COUNT_OFFSET, 1);
    let without_rd = with_byte(plain.clone(), FLAGS_OFFSET, 0x00);

    let from_plain = answer_a(&plain, ip(10, 0, 0, 1), 5).expect("plain answers");
    let from_edns = answer_a(&with_edns, ip(10, 0, 0, 1), 5).expect("edns answers");
    let from_no_rd = answer_a(&without_rd, ip(10, 0, 0, 1), 5).expect("no-rd answers");

    assert_eq!(from_edns, from_plain);
    assert_eq!(from_plain.len(), HEADER_LEN + 16 + 16);
    assert_eq!(
        u16::from_be_bytes([from_no_rd[2], from_no_rd[3]]),
        FLAGS_RESPONSE_RA
    );
}

#[test]
fn answers_echo_the_question_exactly_including_case() {
    let query = query_for(&wire_name(&[b"MeDiUm", b"CoM"]), TYPE_A, FLAGS_QUERY_RD);
    let question_bytes = &query[HEADER_LEN..];

    let response = answer_a(&query, ip(127, 0, 0, 2), 5).expect("answers");

    assert_eq!(
        &response[HEADER_LEN..HEADER_LEN + question_bytes.len()],
        question_bytes
    );
    let empty = answer_empty(&query).expect("answers");
    assert_eq!(&empty[HEADER_LEN..], question_bytes);
}

#[test]
fn answer_empty_matches_hand_derived_bytes_for_a_real_aaaa_query() {
    let query = fixture("windows_query_medium_com_aaaa");
    let expected = hex(concat!(
        "54e5",
        "8180",
        "0001",
        "0000",
        "0000",
        "0000",
        "066d656469756d03636f6d00001c0001",
    ));

    assert_eq!(answer_empty(&query), Some(expected));
}

#[test]
fn answers_are_refused_for_packets_that_are_not_valid_queries() {
    let response =
        answer_a(&fixture("windows_query_medium_com_a"), ip(1, 2, 3, 4), 5).expect("answers");
    for packet in [
        response,
        Vec::new(),
        vec![0; 5],
        fixture("upstream_answer_medium_com"),
    ] {
        assert_eq!(answer_a(&packet, ip(1, 2, 3, 4), 5), None);
        assert_eq!(answer_empty(&packet), None);
    }
}

#[test]
fn real_upstream_answers_yield_their_addresses_in_order() {
    let cases = [
        (
            "medium_com",
            0x1234,
            "medium.com",
            vec![ip(162, 159, 153, 4), ip(162, 159, 152, 4)],
        ),
        (
            "store_steampowered_com",
            0x1235,
            "store.steampowered.com",
            vec![ip(23, 41, 38, 92)],
        ),
        (
            "cdn_akamai_steamstatic_com",
            0x1236,
            "cdn.akamai.steamstatic.com",
            vec![ip(2, 17, 106, 12), ip(2, 17, 106, 5)],
        ),
        (
            "www_github_com",
            0x2001,
            "www.github.com",
            vec![ip(20, 205, 243, 166)],
        ),
        (
            "www_microsoft_com",
            0x2000,
            "www.microsoft.com",
            vec![ip(23, 200, 181, 232)],
        ),
    ];
    for (stem, id, name, expected) in cases {
        let packet = fixture(&format!("upstream_answer_{stem}"));
        assert_eq!(parse_a_answer(&packet, id, name), Some(expected), "{stem}");
    }
}

#[test]
fn answer_checks_id_and_question_name_ignoring_case_and_trailing_dot() {
    let packet = fixture("upstream_answer_medium_com");
    let expected = Some(vec![ip(162, 159, 153, 4), ip(162, 159, 152, 4)]);

    assert_eq!(parse_a_answer(&packet, 0x1234, "MEDIUM.com."), expected);
    assert_eq!(parse_a_answer(&packet, 0x1235, "medium.com"), None);
    assert_eq!(parse_a_answer(&packet, 0x1234, "medium.org"), None);
    assert_eq!(parse_a_answer(&packet, 0x1234, "www.medium.com"), None);
}

#[test]
fn answer_with_bad_header_bits_is_rejected() {
    let packet = fixture("upstream_answer_medium_com");
    let server_failure = with_byte(packet.clone(), 3, 0x82);
    let not_a_response = with_byte(packet.clone(), 2, 0x01);
    let truncated_flag = with_byte(packet.clone(), 2, 0x83);
    let two_questions = with_byte(packet.clone(), 5, 2);
    for (label, mutated) in [
        ("SERVFAIL", server_failure),
        ("QR clear", not_a_response),
        ("TC set", truncated_flag),
        ("two questions", two_questions),
    ] {
        assert_eq!(
            parse_a_answer(&mutated, 0x1234, "medium.com"),
            None,
            "{label}"
        );
    }
}

#[test]
fn a_name_error_for_the_asked_question_means_no_addresses() {
    let packet = fixture("upstream_answer_medium_com");
    let name_error = with_byte(packet, 3, 0x83);

    assert_eq!(
        parse_a_answer(&name_error, 0x1234, "medium.com"),
        Some(Vec::new())
    );
    assert_eq!(parse_a_answer(&name_error, 0x1235, "medium.com"), None);
    assert_eq!(parse_a_answer(&name_error, 0x1234, "medium.org"), None);
}

#[test]
fn every_truncation_of_every_real_answer_is_rejected() {
    let cases = [
        ("medium_com", 0x1234, "medium.com"),
        ("www_microsoft_com", 0x2000, "www.microsoft.com"),
        ("www_github_com", 0x2001, "www.github.com"),
    ];
    for (stem, id, name) in cases {
        let packet = fixture(&format!("upstream_answer_{stem}"));
        for length in 0..packet.len() {
            assert_eq!(
                parse_a_answer(&packet[..length], id, name),
                None,
                "{stem} {length}"
            );
        }
    }
}

#[test]
fn a_record_with_a_wrong_length_is_rejected() {
    let packet = fixture("upstream_answer_medium_com");

    for bad_length in [3, 5] {
        let mutated = with_byte(packet.clone(), MEDIUM_FIRST_RDLEN_OFFSET + 1, bad_length);
        assert_eq!(
            parse_a_answer(&mutated, 0x1234, "medium.com"),
            None,
            "{bad_length}"
        );
    }
}

#[test]
fn valid_answer_without_a_records_is_some_empty() {
    let query = fixture("windows_query_medium_com_a");
    let empty = answer_empty(&query).expect("answers");

    assert_eq!(
        parse_a_answer(&empty, 0x6692, "medium.com"),
        Some(Vec::new())
    );
}

#[test]
fn answer_to_a_different_record_type_is_rejected() {
    let aaaa_query = fixture("windows_query_medium_com_aaaa");
    let empty = answer_empty(&aaaa_query).expect("answers");

    assert_eq!(parse_a_answer(&empty, 0x54e5, "medium.com"), None);
}

#[test]
fn query_built_answered_and_parsed_gives_the_address_back() {
    let query = build_a_query(0xabcd, "store.steampowered.com").expect("builds");

    let response = answer_a(&query, ip(127, 0, 0, 2), 5).expect("answers");

    assert_eq!(
        parse_query(&query),
        Some(question(0xabcd, "store.steampowered.com", TYPE_A))
    );
    assert_eq!(
        parse_a_answer(&response, 0xabcd, "store.steampowered.com"),
        Some(vec![ip(127, 0, 0, 2)])
    );
}

#[test]
fn only_a_records_of_the_final_chain_name_are_returned() {
    let name = wire_name(&[b"www", b"test"]);
    let other = wire_name(&[b"other", b"test"]);
    let target = wire_name(&[b"edge", b"test"]);
    let packet = response_with(
        &name,
        &[
            record(&other, TYPE_A, &[9, 9, 9, 9]),
            record(&[0xc0, 0x0c], TYPE_CNAME, &target),
            record(&[0xc0, 0x0c], TYPE_A, &[8, 8, 8, 8]),
            record(&target, 16, b"\x02hi"),
            record(&target, TYPE_A, &[1, 2, 3, 4]),
            record(&target, TYPE_A, &[5, 6, 7, 8]),
        ],
    );

    assert_eq!(
        parse_a_answer(&packet, 0x4242, "www.test"),
        Some(vec![ip(1, 2, 3, 4), ip(5, 6, 7, 8)])
    );
}

#[test]
fn cname_loop_is_rejected() {
    let first = wire_name(&[b"one", b"test"]);
    let second = wire_name(&[b"two", b"test"]);
    let packet = response_with(
        &first,
        &[
            record(&[0xc0, 0x0c], TYPE_CNAME, &second),
            record(&second, TYPE_CNAME, &first),
        ],
    );

    assert_eq!(parse_a_answer(&packet, 0x4242, "one.test"), None);
}

#[test]
fn compression_pointers_to_themselves_forward_or_around_labels_are_rejected() {
    let question_name = wire_name(&[b"www", b"test"]);
    let own_position = u8::try_from(WWW_TEST_RECORDS_START).expect("offset fits");
    let to_itself = response_with(
        &question_name,
        &[record(&[0xc0, own_position], TYPE_A, &[1, 2, 3, 4])],
    );
    let forward = response_with(
        &question_name,
        &[record(&[0xc0, 0xff], TYPE_A, &[1, 2, 3, 4])],
    );
    let label_cycle = response_with(
        &question_name,
        &[record(
            &[1, b'a', 0xc0, own_position],
            TYPE_A,
            &[1, 2, 3, 4],
        )],
    );

    assert_eq!(parse_a_answer(&to_itself, 0x4242, "www.test"), None);
    assert_eq!(parse_a_answer(&forward, 0x4242, "www.test"), None);
    assert_eq!(parse_a_answer(&label_cycle, 0x4242, "www.test"), None);
}

#[test]
fn reserved_label_tags_in_an_answer_are_rejected() {
    let question_name = wire_name(&[b"www", b"test"]);
    let packet = response_with(
        &question_name,
        &[record(&[0x80, 0x00], TYPE_A, &[1, 2, 3, 4])],
    );

    assert_eq!(parse_a_answer(&packet, 0x4242, "www.test"), None);
}

#[test]
fn cname_target_running_past_its_rdata_is_rejected() {
    let question_name = wire_name(&[b"www", b"test"]);
    let mut overlong = record(&[0xc0, 0x0c], TYPE_CNAME, &wire_name(&[b"edge", b"test"]));
    let rdlen_low_byte = overlong.len() - 12;
    overlong[rdlen_low_byte] -= 3;
    let packet = response_with(&question_name, &[overlong]);

    assert_eq!(parse_a_answer(&packet, 0x4242, "www.test"), None);
}
