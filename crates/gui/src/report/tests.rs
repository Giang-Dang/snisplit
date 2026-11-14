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
fn a_blocked_site_offers_the_suggested_profile_and_keeps_every_cell_of_the_table() {
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
    assert_eq!(view.rows.len(), 7);
    assert_eq!(
        view.rows[2],
        Row {
            attempt: "https, tls-split host-start".to_string(),
            result: "fails".to_string(),
            runs: vec!["reset".to_string(), "timed out".to_string()],
        }
    );
    assert_eq!(view.rows[6].attempt, "http, hoSt:");
    assert_eq!(
        view.rows[6].runs,
        ["HTTP/1.1 302 Found", "HTTP/1.1 302 Found"]
    );
    assert_eq!(
        view.summary,
        "HTTPS fails as is, tls-split 1 answered; HTTP fails as is, hoSt: answered; the profile below should get it through"
    );
    assert_eq!(
        view.offer,
        Offer::Add(
            "name news-example-com\ndomains news.example.com\ntls-ports 443\ntls-split 1\nhttp-ports 80\nhttp-method host-spell\n"
                .to_string()
        )
    );
    assert_eq!(view.offer.label(), "Add");
}

#[test]
fn an_open_site_has_nothing_to_add() {
    let view = view(&report(
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
    assert_eq!(view.offer, Offer::NotBlocked);
    assert_eq!(view.offer.label(), "Not blocked, nothing to add");
    assert_eq!(view.summary, "not blocked here; no profile needed");
}

#[test]
fn nothing_answering_and_no_profile_helping_both_say_no_method_worked() {
    let silent = report("down.example.org", std::array::from_fn(|_| resets()));
    assert_eq!(silent.assessment.verdict, Verdict::NothingWorked);
    let http_dead = report(
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
    );
    assert_eq!(http_dead.assessment.verdict, Verdict::NoProfileHelps);
    for probed in [silent, http_dead] {
        let view = view(&probed);
        assert_eq!(view.offer, Offer::NothingWorked);
        assert_eq!(view.offer.label(), "No method worked");
    }
}
