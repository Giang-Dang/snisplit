use super::*;

const FIXED: HelloRandom = HelloRandom {
    random: [0x01; RANDOM_LEN],
    session_id: [0x02; RANDOM_LEN],
    key_share: [0x03; RANDOM_LEN],
};

fn flight(attempt: Attempt, domain: &str) -> FirstFlight {
    first_flight(&attempt, domain, &FIXED).expect("a first flight")
}

#[test]
fn the_whole_hello_has_no_cut_and_carries_the_host_at_109() {
    let whole = flight(Attempt::TlsWhole, "example.org");
    assert!(whole.cuts.is_empty());
    assert_eq!(whole.bytes.len(), 11 + 212);
    assert_eq!(&whole.bytes[109..120], b"example.org");
}

#[test]
fn each_split_attempt_cuts_where_the_relay_would_for_that_profile() {
    let split = |positions: Vec<SplitPosition>| flight(Attempt::TlsSplit(positions), "example.org");
    assert_eq!(split(vec![SplitPosition::Offset(1)]).cuts, [1]);
    assert_eq!(split(vec![SplitPosition::HostStart]).cuts, [109]);
    assert_eq!(split(vec![SplitPosition::DomainMid]).cuts, [109 + 3]);
    assert_eq!(
        split(vec![SplitPosition::Offset(1), SplitPosition::DomainMid]).cuts,
        [1, 112]
    );
    let bytes = split(vec![SplitPosition::DomainMid]).bytes;
    assert_eq!(&bytes[109..112], b"exa");
}

#[test]
fn domain_mid_cuts_the_first_label_of_the_probed_name_itself() {
    let store = flight(
        Attempt::TlsSplit(vec![SplitPosition::DomainMid]),
        "store.steampowered.com",
    );
    assert_eq!(store.cuts, [111]);
    assert_eq!(&store.bytes[109..111], b"st");
}

#[test]
fn the_plain_http_request_names_the_host_as_written() {
    let plain = flight(Attempt::HttpPlain, "example.org");
    assert_eq!(
        plain.bytes,
        b"GET / HTTP/1.1\r\nHost: example.org\r\nAccept: */*\r\nConnection: close\r\n\r\n"
    );
    assert!(plain.cuts.is_empty());
}

#[test]
fn the_respelled_request_differs_only_in_the_host_header_name() {
    let respelled = flight(Attempt::HttpRespelled, "example.org");
    assert_eq!(
        respelled.bytes,
        b"GET / HTTP/1.1\r\nhoSt: example.org\r\nAccept: */*\r\nConnection: close\r\n\r\n"
    );
    assert!(respelled.cuts.is_empty());
}

#[test]
fn a_name_the_relay_would_refuse_gives_no_first_flight() {
    for attempt in attempts() {
        assert_eq!(
            first_flight(&attempt, "bad host.com", &FIXED),
            None,
            "{attempt}"
        );
        assert_eq!(first_flight(&attempt, "", &FIXED), None, "{attempt}");
    }
}

#[test]
fn the_attempts_are_the_section_1_rows_in_table_order() {
    let labels: Vec<String> = attempts().iter().map(ToString::to_string).collect();
    assert_eq!(
        labels,
        [
            "https, whole hello",
            "https, tls-split 1",
            "https, tls-split host-start",
            "https, tls-split domain-mid",
            "https, tls-split 1 domain-mid",
            "http, Host:",
            "http, hoSt:",
        ]
    );
    let protocols: Vec<Protocol> = attempts().iter().map(Attempt::protocol).collect();
    assert_eq!(
        protocols,
        [
            Protocol::Tls,
            Protocol::Tls,
            Protocol::Tls,
            Protocol::Tls,
            Protocol::Tls,
            Protocol::Http,
            Protocol::Http,
        ]
    );
}

#[test]
fn an_attempt_works_only_when_every_run_answered() {
    let answer = Outcome::Answered(Reply::Http {
        status_line: "HTTP/1.1 200 OK".to_owned(),
    });
    let record = |outcomes: Vec<Outcome>| AttemptRecord {
        attempt: Attempt::HttpPlain,
        outcomes,
    };
    assert_eq!(
        record(vec![answer.clone(), answer.clone()]).agreement(),
        Agreement::EveryRun
    );
    assert_eq!(
        record(vec![answer.clone(), Outcome::Reset]).agreement(),
        Agreement::SomeRuns
    );
    assert_eq!(
        record(vec![Outcome::TimedOut, answer]).agreement(),
        Agreement::SomeRuns
    );
    assert_eq!(
        record(vec![Outcome::Reset, Outcome::Closed]).agreement(),
        Agreement::NoRun
    );
    assert_eq!(record(Vec::new()).agreement(), Agreement::NoRun);
    assert_eq!(Agreement::SomeRuns.to_string(), "mixed");
}
