use std::time::Duration;

use snisplit_core::first_flight::HttpMethod;
use snisplit_core::probe::{AttemptRecord, Outcome, RecordType, Reply, assess, attempts};
use snisplit_core::strategy::SplitPosition;

use super::*;
use crate::profile;
use crate::profile_file::ProfileSpec;

const ADDRESS: Ipv4Addr = Ipv4Addr::new(192, 0, 2, 10);

fn handshake() -> Outcome {
    Outcome::Answered(Reply::Tls {
        record: RecordType::Handshake,
        version: 0x0303,
    })
}

fn found() -> Outcome {
    Outcome::Answered(Reply::Http {
        status_line: "HTTP/1.1 302 Found".to_owned(),
    })
}

fn records(outcomes: [[Outcome; 2]; 7]) -> Vec<AttemptRecord> {
    attempts()
        .into_iter()
        .zip(outcomes)
        .map(|(attempt, outcomes)| AttemptRecord {
            attempt,
            outcomes: outcomes.to_vec(),
        })
        .collect()
}

fn steam_like(domain: &str) -> ProbeReport {
    let records = records([
        [Outcome::Reset, Outcome::Reset],
        [handshake(), handshake()],
        [Outcome::Reset, Outcome::TimedOut],
        [Outcome::Reset, handshake()],
        [handshake(), handshake()],
        [Outcome::Reset, Outcome::Reset],
        [found(), found()],
    ]);
    ProbeReport {
        domain: domain.to_owned(),
        address: ADDRESS,
        assessment: assess(domain, &records),
        records,
    }
}

fn parsed_suggestion(report: &ProbeReport) -> ProfileSpec {
    let text = report.assessment.profile.as_deref().expect("a suggestion");
    profile::parse_file("suggestion", text).expect("the suggestion parses")
}

#[test]
fn the_table_puts_every_cell_under_its_heading() {
    let text = render(&steam_like("store.steampowered.com"));
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "address 192.0.2.10");
    assert_eq!(lines[1], "");
    let expected = [
        ["attempt", "result", "run 1", "run 2"],
        ["https, whole hello", "fails", "reset", "reset"],
        [
            "https, tls-split 1",
            "works",
            "handshake 0x0303",
            "handshake 0x0303",
        ],
        ["https, tls-split host-start", "fails", "reset", "timed out"],
        [
            "https, tls-split domain-mid",
            "mixed",
            "reset",
            "handshake 0x0303",
        ],
        [
            "https, tls-split 1 domain-mid",
            "works",
            "handshake 0x0303",
            "handshake 0x0303",
        ],
        ["http, Host:", "fails", "reset", "reset"],
        [
            "http, hoSt:",
            "works",
            "HTTP/1.1 302 Found",
            "HTTP/1.1 302 Found",
        ],
    ];
    for (line, [attempt, result, first, second]) in lines[2..10].iter().zip(expected) {
        assert_eq!(line[..29].trim_end(), attempt, "{line}");
        assert_eq!(&line[29..31], "  ", "{line}");
        assert_eq!(line[31..37].trim_end(), result, "{line}");
        assert_eq!(line[39..57].trim_end(), first, "{line}");
        assert_eq!(&line[59..], second, "{line}");
    }
    assert_eq!(lines[10], "");
    assert_eq!(
        lines[11],
        "verdict: HTTPS fails as is, tls-split 1 answered; HTTP fails as is, hoSt: answered; the profile below should get it through"
    );
}

#[test]
fn a_suggestion_follows_the_verdict_after_a_blank_line_and_the_hint() {
    let text = render(&steam_like("store.steampowered.com"));
    assert!(text.ends_with(
        "the profile below should get it through\n\nsuggested profile (save it to a file, then: snisplit run --profile-file <file>):\n\nname store-steampowered-com\ndomains store.steampowered.com\ntls-ports 443\ntls-split 1\nhttp-ports 80\nhttp-method host-spell\n"
    ));
}

#[test]
fn an_open_site_prints_the_verdict_and_no_profile() {
    let records = records([
        [handshake(), handshake()],
        [handshake(), handshake()],
        [handshake(), handshake()],
        [handshake(), handshake()],
        [handshake(), handshake()],
        [found(), found()],
        [found(), found()],
    ]);
    let report = ProbeReport {
        domain: "example.org".to_owned(),
        address: ADDRESS,
        assessment: assess("example.org", &records),
        records,
    };
    let text = render(&report);
    assert!(text.ends_with("\n\nverdict: not blocked here; no profile needed\n"));
    assert!(!text.contains("name "));
}

#[test]
fn the_suggestion_for_a_33_character_steam_name_parses_as_a_profile_file() {
    let report = steam_like("steambroadcast-test.akamaized.net");
    assert_eq!(
        parsed_suggestion(&report),
        ProfileSpec {
            name: "steambroadcast-test-akamaized-ne".to_owned(),
            domains: vec!["steambroadcast-test.akamaized.net".to_owned()],
            tls_ports: vec![443],
            tls_positions: vec![SplitPosition::Offset(1)],
            http_ports: vec![80],
            http_method: HttpMethod::HostSpell,
        }
    );
}

#[test]
fn an_http_only_suggestion_parses_with_tls_switched_off() {
    let silent = [Outcome::TimedOut, Outcome::TimedOut];
    let records = records([
        silent.clone(),
        silent.clone(),
        silent.clone(),
        silent.clone(),
        silent,
        [Outcome::Reset, Outcome::Reset],
        [found(), found()],
    ]);
    let report = ProbeReport {
        domain: "medium.com".to_owned(),
        address: ADDRESS,
        assessment: assess("medium.com", &records),
        records,
    };
    let spec = parsed_suggestion(&report);
    assert_eq!(spec.name, "medium-com");
    assert_eq!(spec.tls_ports, Vec::<u16>::new());
    assert_eq!(spec.http_ports, vec![80]);
    assert_eq!(spec.http_method, HttpMethod::HostSpell);
}

#[test]
fn a_two_cut_pass_through_suggestion_parses_with_both_positions() {
    let records = records([
        [Outcome::Reset, Outcome::Reset],
        [handshake(), Outcome::Reset],
        [Outcome::Reset, Outcome::Reset],
        [Outcome::Reset, Outcome::Reset],
        [handshake(), handshake()],
        [found(), found()],
        [Outcome::Reset, Outcome::Reset],
    ]);
    let report = ProbeReport {
        domain: "store.steampowered.com".to_owned(),
        address: ADDRESS,
        assessment: assess("store.steampowered.com", &records),
        records,
    };
    let spec = parsed_suggestion(&report);
    assert_eq!(
        spec.tls_positions,
        vec![SplitPosition::Offset(1), SplitPosition::DomainMid]
    );
    assert_eq!(spec.http_method, HttpMethod::PassThrough);
}

#[test]
fn explicit_upstreams_are_used_as_given_and_the_system_is_not_asked() {
    let explicit: Vec<SocketAddr> = vec!["127.0.0.1:5353".parse().unwrap()];
    let servers = upstream_servers(&explicit, || panic!("the system was asked")).unwrap();
    assert_eq!(servers, explicit);
}

#[test]
fn system_upstreams_lose_loopback_and_duplicates_and_none_left_is_an_error() {
    let servers = upstream_servers(&[], || {
        Ok(vec![
            Ipv4Addr::new(127, 0, 0, 2),
            Ipv4Addr::new(192, 168, 1, 1),
            Ipv4Addr::new(192, 168, 1, 1),
        ])
    })
    .unwrap();
    assert_eq!(servers, ["192.168.1.1:53".parse::<SocketAddr>().unwrap()]);
    let error = upstream_servers(&[], || Ok(vec![Ipv4Addr::LOCALHOST])).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::NotFound);
    assert_eq!(
        error.to_string(),
        "no usable system DNS server found; pass --upstream <ip[:port]>"
    );
    let failure = upstream_servers(&[], || Err(io::Error::other("adapter list failed")));
    assert_eq!(failure.unwrap_err().to_string(), "adapter list failed");
}

#[test]
fn the_starting_line_names_the_servers_and_the_amount_of_work() {
    let options = ProbeOptions {
        domain: "example.org".to_owned(),
        upstreams: vec![],
        timeout: Duration::from_secs(5),
        runs: 2,
    };
    let servers: Vec<SocketAddr> = vec![
        "192.168.1.1:53".parse().unwrap(),
        "[2606:4700::1111]:53".parse().unwrap(),
    ];
    assert_eq!(
        starting_line(&options, &servers),
        "probing example.org through DNS 192.168.1.1:53, [2606:4700::1111]:53: 7 attempts, 2 runs each, timeout 5 s"
    );
}
