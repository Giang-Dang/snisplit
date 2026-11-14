use super::*;

const API_HELLO: &[u8] =
    include_bytes!("../../tests/fixtures/client_hello/api.steampowered.com-1.bin");
const STORE_HELLO: &[u8] =
    include_bytes!("../../tests/fixtures/client_hello/store.steampowered.com-4.bin");
const COMMUNITY_HELLO: &[u8] =
    include_bytes!("../../tests/fixtures/client_hello/steamcommunity.com-12.bin");

const ROOMY: usize = 1 << 14;

const API_HOST: &str = "api.steampowered.com";
const API_HOST_START: usize = 126;
const API_MID_OF_STEAMPOWERED: usize = 136;

const STORE_HOST: &str = "store.steampowered.com";
const STORE_HOST_START: usize = 1702;
const STORE_HOST_END: usize = 1724;
const STORE_MID_OF_STEAMPOWERED: usize = 1714;
const STORE_FIRST_PACKET_LEN: usize = 1452;

const COMMUNITY_HOST: &str = "steamcommunity.com";
const COMMUNITY_MID_OF_STEAMCOMMUNITY: usize = 134;

const STEAM_DOMAINS: [&str; 2] = ["steampowered.com", "steamcommunity.com"];

fn profile(domains: &[&str], tls_positions: &[SplitPosition]) -> RelayProfile {
    RelayProfile {
        name: "test".to_owned(),
        domains: DomainList::new(domains),
        tls_positions: tls_positions.to_vec(),
        tls_ports: vec![443],
        http_ports: vec![80],
        http_method: HttpMethod::HostSpell,
    }
}

fn steam() -> RelayProfile {
    profile(
        &STEAM_DOMAINS,
        &[SplitPosition::Offset(1), SplitPosition::DomainMid],
    )
}

fn forward(host: &str, cuts: &[usize]) -> Decision {
    Decision::Forward {
        host: host.to_owned(),
        cuts: cuts.to_vec(),
    }
}

fn decide_tls(profile: &RelayProfile, bytes: &[u8]) -> Decision {
    decide(profile, Protocol::Tls, bytes, ROOMY)
}

fn decide_http(profile: &RelayProfile, bytes: &[u8]) -> Decision {
    decide(profile, Protocol::Http, bytes, ROOMY)
}

#[test]
fn real_steam_client_hellos_are_cut_at_byte_one_and_inside_the_second_level_label() {
    assert_eq!(
        decide_tls(&steam(), API_HELLO),
        forward(API_HOST, &[1, API_MID_OF_STEAMPOWERED])
    );
    assert_eq!(
        decide_tls(&steam(), STORE_HELLO),
        forward(STORE_HOST, &[1, STORE_MID_OF_STEAMPOWERED])
    );
    assert_eq!(
        decide_tls(&steam(), COMMUNITY_HELLO),
        forward(COMMUNITY_HOST, &[1, COMMUNITY_MID_OF_STEAMCOMMUNITY])
    );
}

#[test]
fn host_start_position_cuts_at_the_first_byte_of_the_sni() {
    let profile = profile(&STEAM_DOMAINS, &[SplitPosition::HostStart]);
    assert_eq!(
        decide_tls(&profile, API_HELLO),
        forward(API_HOST, &[API_HOST_START])
    );
}

#[test]
fn a_profile_without_positions_forwards_with_no_cuts() {
    let profile = profile(&STEAM_DOMAINS, &[]);
    assert_eq!(decide_tls(&profile, API_HELLO), forward(API_HOST, &[]));
}

#[test]
fn the_first_packet_of_a_split_hello_needs_more_data() {
    let first_packet = &STORE_HELLO[..STORE_FIRST_PACKET_LEN];
    assert_eq!(decide_tls(&steam(), first_packet), Decision::NeedMoreData);
}

#[test]
fn the_decision_is_made_as_soon_as_the_last_sni_byte_has_arrived() {
    assert_eq!(
        decide_tls(&steam(), &STORE_HELLO[..STORE_HOST_END - 1]),
        Decision::NeedMoreData
    );
    assert_eq!(
        decide_tls(&steam(), &STORE_HELLO[..STORE_HOST_END]),
        forward(STORE_HOST, &[1, STORE_MID_OF_STEAMPOWERED])
    );
}

#[test]
fn an_incomplete_hello_is_too_long_exactly_when_the_cap_is_reached() {
    let prefix = &STORE_HELLO[..STORE_HOST_END - 1];
    assert_eq!(
        decide(&steam(), Protocol::Tls, prefix, prefix.len() + 1),
        Decision::NeedMoreData
    );
    assert_eq!(
        decide(&steam(), Protocol::Tls, prefix, prefix.len()),
        Decision::Refuse(Refusal::TooLong)
    );
}

#[test]
fn a_complete_hello_exactly_as_long_as_the_cap_is_not_too_long() {
    assert_eq!(
        decide(&steam(), Protocol::Tls, API_HELLO, API_HELLO.len()),
        forward(API_HOST, &[1, API_MID_OF_STEAMPOWERED])
    );
}

#[test]
fn an_empty_first_flight_is_too_long_under_a_zero_cap() {
    assert_eq!(
        decide(&steam(), Protocol::Tls, &[], 0),
        Decision::Refuse(Refusal::TooLong)
    );
    assert_eq!(
        decide(&steam(), Protocol::Tls, &[], 1),
        Decision::NeedMoreData
    );
}

#[test]
fn cuts_stay_strictly_inside_the_stream() {
    let len = STORE_HELLO.len();
    let profile = profile(
        &STEAM_DOMAINS,
        &[
            SplitPosition::Offset(0),
            SplitPosition::Offset(1),
            SplitPosition::Offset(len - 1),
            SplitPosition::Offset(len),
            SplitPosition::Offset(len + 5000),
        ],
    );
    assert_eq!(
        decide_tls(&profile, STORE_HELLO),
        forward(STORE_HOST, &[1, len - 1])
    );
}

#[test]
fn a_cut_at_the_end_of_a_short_prefix_is_dropped() {
    let short = &STORE_HELLO[..STORE_HOST_END];
    let profile = profile(
        &STEAM_DOMAINS,
        &[
            SplitPosition::Offset(1),
            SplitPosition::Offset(STORE_HOST_END),
        ],
    );
    assert_eq!(decide_tls(&profile, short), forward(STORE_HOST, &[1]));
}

#[test]
fn an_uppercase_sni_is_matched_and_reported_in_lowercase_with_unchanged_cuts() {
    let mut shouting = STORE_HELLO.to_vec();
    shouting[STORE_HOST_START..STORE_HOST_END].make_ascii_uppercase();
    assert_eq!(&shouting[STORE_HOST_START..STORE_HOST_START + 5], b"STORE");
    assert_eq!(
        decide_tls(&steam(), &shouting),
        forward(STORE_HOST, &[1, STORE_MID_OF_STEAMPOWERED])
    );
}

#[test]
fn a_hello_for_an_unlisted_host_is_not_listed() {
    let medium = profile(&["medium.com"], &[SplitPosition::Offset(1)]);
    assert_eq!(
        decide_tls(&medium, API_HELLO),
        Decision::Refuse(Refusal::NotListed)
    );
}

#[test]
fn a_sni_that_is_not_printable_ascii_has_no_host() {
    for byte in [0xff, 0x00, 0x0a, b' '] {
        let mut corrupted = STORE_HELLO.to_vec();
        corrupted[STORE_HOST_START] = byte;
        assert_eq!(
            decide_tls(&steam(), &corrupted),
            Decision::Refuse(Refusal::NoHost),
            "byte {byte:#04x}"
        );
    }
}

#[test]
fn bytes_that_are_not_for_the_protocol_have_no_host() {
    let request = b"GET / HTTP/1.1\r\nHost: store.steampowered.com\r\n\r\n";
    assert_eq!(
        decide_tls(&steam(), request),
        Decision::Refuse(Refusal::NoHost)
    );
    assert_eq!(
        decide_http(&steam(), API_HELLO),
        Decision::Refuse(Refusal::NoHost)
    );
}

#[test]
fn http_host_loses_its_port_and_case_and_gets_no_cuts() {
    let request =
        b"GET /app/220 HTTP/1.1\r\nHost: Store.SteamPowered.com:80\r\nAccept: */*\r\n\r\n";
    assert_eq!(decide_http(&steam(), request), forward(STORE_HOST, &[]));
}

#[test]
fn http_without_a_complete_host_header_waits_then_refuses_by_the_cap() {
    let partial = b"GET / HTTP/1.1\r\nHost: store.stea";
    assert_eq!(decide_http(&steam(), partial), Decision::NeedMoreData);
    assert_eq!(
        decide(&steam(), Protocol::Http, partial, partial.len()),
        Decision::Refuse(Refusal::TooLong)
    );
}

#[test]
fn http_without_a_host_header_has_no_host() {
    let request = b"GET / HTTP/1.1\r\nAccept: */*\r\n\r\n";
    assert_eq!(
        decide_http(&steam(), request),
        Decision::Refuse(Refusal::NoHost)
    );
}

#[test]
fn http_hosts_outside_the_list_are_not_listed_even_with_a_listed_suffix_text() {
    for host in ["example.org", "evilsteampowered.com"] {
        let request = format!("GET / HTTP/1.1\r\nHost: {host}\r\n\r\n");
        assert_eq!(
            decide_http(&steam(), request.as_bytes()),
            Decision::Refuse(Refusal::NotListed),
            "{host}"
        );
    }
}
