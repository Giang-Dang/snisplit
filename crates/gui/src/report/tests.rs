use std::io::ErrorKind;
use std::net::Ipv4Addr;

use snisplit_core::probe::{AttemptRecord, Outcome, RecordType, Reply, Verdict, assess, attempts};
use snisplit_relay::probe::ProbeReport;

use super::*;

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

fn report(domain: &str, outcomes: [[Outcome; 2]; 7]) -> ProbeReport {
    let records: Vec<AttemptRecord> = attempts()
        .into_iter()
        .zip(outcomes)
        .map(|(attempt, outcomes)| AttemptRecord {
            attempt,
            outcomes: outcomes.to_vec(),
        })
        .collect();
    ProbeReport {
        domain: domain.to_owned(),
        address: ADDRESS,
        assessment: assess(domain, &records),
        records,
    }
}

fn resets() -> [Outcome; 2] {
    [Outcome::Reset, Outcome::Reset]
}

fn handshakes() -> [Outcome; 2] {
    [handshake(), handshake()]
}

#[test]
fn every_attempt_has_a_plain_label() {
    let labels: Vec<String> = attempts().iter().map(attempt_label).collect();
    assert_eq!(
        labels,
        [
            "Normal secure connection",
            "Sent in pieces (1)",
            "Sent in pieces (host-start)",
            "Sent in pieces (domain-mid)",
            "Sent in pieces (1 domain-mid)",
            "Normal plain connection",
            "Header respelled",
        ]
    );
}

#[test]
fn every_outcome_is_answered_cut_off_or_no_answer() {
    let cases = [
        (handshake(), "answered"),
        (found(), "answered"),
        (
            Outcome::Answered(Reply::Tls {
                record: RecordType::Alert,
                version: 0x0303,
            }),
            "answered",
        ),
        (Outcome::Reset, "cut off"),
        (Outcome::Closed, "cut off"),
        (Outcome::Unexpected(vec![0x48, 0x54]), "no answer"),
        (Outcome::TimedOut, "no answer"),
        (
            Outcome::ConnectFailed(ErrorKind::ConnectionRefused),
            "no answer",
        ),
        (Outcome::Failed(ErrorKind::Other), "no answer"),
    ];
    for (outcome, word) in cases {
        assert_eq!(outcome_word(&outcome), word, "{outcome:?}");
        assert_eq!(outcome.answered(), word == "answered", "{outcome:?}");
    }
}

#[test]
fn a_blocked_site_keeps_its_profile_and_every_cell_in_plain_words_and_in_activity() {
    let view = view(&report(
        "news.example.com",
        [
            resets(),
            handshakes(),
            [Outcome::Reset, Outcome::TimedOut],
            resets(),
            handshakes(),
            resets(),
            [found(), found()],
        ],
    ));
    assert_eq!(view.domain, "news.example.com");
    assert_eq!(view.address, "192.0.2.10");
    assert_eq!(view.verdict, Verdict::ProfileSuggested);
    assert_eq!(view.rows.len(), 7);
    assert_eq!(
        view.rows[2],
        Row {
            attempt: "Sent in pieces (host-start)".to_string(),
            outcomes: vec!["cut off".to_string(), "no answer".to_string()],
        }
    );
    assert_eq!(view.rows[6].outcomes, ["answered", "answered"]);
    assert_eq!(
        view.profile.as_deref(),
        Some(
            "name news-example-com\ndomains news.example.com\ntls-ports 443\ntls-split 1\nhttp-ports 80\nhttp-method host-spell\n"
        )
    );
    assert_eq!(
        view.details,
        [
            "  https, whole hello: reset, reset",
            "  https, tls-split 1: handshake 0x0303, handshake 0x0303",
            "  https, tls-split host-start: reset, timed out",
            "  https, tls-split domain-mid: reset, reset",
            "  https, tls-split 1 domain-mid: handshake 0x0303, handshake 0x0303",
            "  http, Host:: reset, reset",
            "  http, hoSt:: HTTP/1.1 302 Found, HTTP/1.1 302 Found",
            "  verdict: HTTPS fails as is, tls-split 1 answered; HTTP fails as is, hoSt: answered; the profile below should get it through",
        ]
    );
}

#[test]
fn an_open_site_and_a_site_nothing_helps_carry_no_profile() {
    let open = view(&report(
        "example.org",
        [
            handshakes(),
            handshakes(),
            handshakes(),
            handshakes(),
            handshakes(),
            [found(), found()],
            [found(), found()],
        ],
    ));
    assert_eq!(open.verdict, Verdict::NotBlocked);
    assert_eq!(open.profile, None);
    let silent = view(&report(
        "down.example.org",
        std::array::from_fn(|_| resets()),
    ));
    assert_eq!(silent.verdict, Verdict::NothingWorked);
    assert_eq!(silent.profile, None);
    let http_dead = view(&report(
        "web.example.org",
        [
            handshakes(),
            handshakes(),
            handshakes(),
            handshakes(),
            handshakes(),
            resets(),
            resets(),
        ],
    ));
    assert_eq!(http_dead.verdict, Verdict::NoProfileHelps);
    assert_eq!(http_dead.profile, None);
}
