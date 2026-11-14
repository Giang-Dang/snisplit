mod common;

use std::io;
use std::net::Ipv4Addr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::{Answer, FakeResolver, LOOPBACK, block_on};
use snisplit_core::first_flight::Protocol;
use snisplit_core::probe::{Agreement, Outcome, RecordType, Reply, Verdict};
use snisplit_core::strategy::SplitPosition;
use snisplit_relay::probe::{ProbeReport, ProbeSettings, ProbeTarget, probe, probe_target};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::sleep;

const DOMAIN: &str = "example.org";
const HOST_IN_HELLO: std::ops::Range<usize> = 109..120;
const HELLO_LEN: usize = 223;
const SERVER_HELLO_HEADER: &[u8] = &[0x16, 0x03, 0x03, 0x00, 0x7a];
const PROTOCOL_VERSION_ALERT: &[u8] = &[0x15, 0x03, 0x03, 0x00, 0x02, 0x02, 0x46];
const FOUND: &[u8] = b"HTTP/1.1 302 Found\r\nLocation: https://example.org/\r\n\r\n";
const PLAIN_REQUEST: &[u8] =
    b"GET / HTTP/1.1\r\nHost: example.org\r\nAccept: */*\r\nConnection: close\r\n\r\n";
const RESPELLED_REQUEST: &[u8] =
    b"GET / HTTP/1.1\r\nhoSt: example.org\r\nAccept: */*\r\nConnection: close\r\n\r\n";
const GAP: Duration = Duration::from_millis(100);
const SHORT_WAIT: Duration = Duration::from_millis(300);
const LONG_WAIT: Duration = Duration::from_secs(5);
const SILENCE: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Copy)]
enum Reaction {
    Answer(&'static [u8]),
    Reset,
    Close,
    Silent,
}

type Reads = Vec<Vec<u8>>;
type Decide = dyn Fn(usize, &Reads) -> Reaction + Send + Sync;

struct FakeServer {
    port: u16,
    connections: Arc<Mutex<Vec<Reads>>>,
}

impl FakeServer {
    async fn start(
        protocol: Protocol,
        decide: impl Fn(usize, &Reads) -> Reaction + Send + Sync + 'static,
    ) -> Self {
        let listener = TcpListener::bind((LOOPBACK, 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let connections = Arc::new(Mutex::new(Vec::new()));
        let decide: Arc<Decide> = Arc::new(decide);
        let seen = Arc::clone(&connections);
        let accepted = Arc::new(AtomicUsize::new(0));
        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    return;
                };
                let index = accepted.fetch_add(1, Ordering::SeqCst);
                let decide = Arc::clone(&decide);
                let seen = Arc::clone(&seen);
                tokio::spawn(async move {
                    serve(stream, protocol, index, decide, seen).await;
                });
            }
        });
        Self { port, connections }
    }

    fn connections(&self) -> Vec<Reads> {
        self.connections.lock().unwrap().clone()
    }
}

async fn serve(
    mut stream: TcpStream,
    protocol: Protocol,
    index: usize,
    decide: Arc<Decide>,
    seen: Arc<Mutex<Vec<Reads>>>,
) {
    let mut reads: Reads = Vec::new();
    let mut buffer = [0u8; 4096];
    while !first_flight_complete(protocol, &reads.concat()) {
        match stream.read(&mut buffer).await {
            Ok(0) | Err(_) => break,
            Ok(read) => reads.push(buffer[..read].to_vec()),
        }
    }
    let reaction = decide(index, &reads);
    seen.lock().unwrap().push(reads);
    match reaction {
        Reaction::Answer(bytes) => {
            let _ = stream.write_all(bytes).await;
            let _ = stream.shutdown().await;
            sleep(SHORT_WAIT).await;
        }
        Reaction::Reset => {
            let _ = stream.set_zero_linger();
        }
        Reaction::Close => {
            let _ = stream.shutdown().await;
            sleep(SHORT_WAIT).await;
        }
        Reaction::Silent => sleep(SILENCE).await,
    }
}

fn first_flight_complete(protocol: Protocol, received: &[u8]) -> bool {
    match protocol {
        Protocol::Tls => {
            received.len() >= 5
                && received.len() >= 5 + usize::from(u16::from_be_bytes([received[3], received[4]]))
        }
        Protocol::Http => received.windows(4).any(|window| window == b"\r\n\r\n"),
    }
}

fn first_read_has_the_whole_host(reads: &Reads) -> bool {
    reads.first().is_some_and(|first| {
        first.len() >= HOST_IN_HELLO.end && &first[HOST_IN_HELLO] == DOMAIN.as_bytes()
    })
}

fn has_plain_host_header(reads: &Reads) -> bool {
    reads
        .concat()
        .windows(7)
        .any(|window| window == b"\nHost: ")
}

fn settings(runs: usize, wait: Duration) -> ProbeSettings {
    ProbeSettings {
        runs,
        timeout: wait,
        piece_gap: GAP,
    }
}

async fn probe_servers(
    tls: &FakeServer,
    http: &FakeServer,
    settings: &ProbeSettings,
) -> ProbeReport {
    let target = ProbeTarget {
        domain: DOMAIN.to_owned(),
        address: LOOPBACK,
        tls_port: tls.port,
        http_port: http.port,
    };
    probe_target(&target, settings).await.unwrap()
}

fn outcomes(report: &ProbeReport) -> Vec<Vec<Outcome>> {
    report
        .records
        .iter()
        .map(|record| record.outcomes.clone())
        .collect()
}

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

#[test]
fn an_isp_that_resets_a_whole_host_gets_a_profile_with_a_cut_at_1_and_respelling() {
    block_on(async {
        let tls = FakeServer::start(Protocol::Tls, |_, reads| {
            if first_read_has_the_whole_host(reads) {
                Reaction::Reset
            } else {
                Reaction::Answer(SERVER_HELLO_HEADER)
            }
        })
        .await;
        let http = FakeServer::start(Protocol::Http, |_, reads| {
            if has_plain_host_header(reads) {
                Reaction::Reset
            } else {
                Reaction::Answer(FOUND)
            }
        })
        .await;
        let report = probe_servers(&tls, &http, &settings(2, LONG_WAIT)).await;
        assert_eq!(
            outcomes(&report),
            [
                vec![Outcome::Reset, Outcome::Reset],
                vec![handshake(), handshake()],
                vec![handshake(), handshake()],
                vec![handshake(), handshake()],
                vec![handshake(), handshake()],
                vec![Outcome::Reset, Outcome::Reset],
                vec![found(), found()],
            ]
        );
        assert_eq!(report.assessment.verdict, Verdict::ProfileSuggested);
        assert_eq!(
            report.assessment.profile.as_deref(),
            Some(
                "name example-org\ndomains example.org\ntls-ports 443\ntls-split 1\nhttp-ports 80\nhttp-method host-spell\n"
            )
        );
        let requests: Vec<Vec<u8>> = http
            .connections()
            .iter()
            .map(|reads| reads.concat())
            .collect();
        assert_eq!(
            requests,
            [
                PLAIN_REQUEST,
                RESPELLED_REQUEST,
                PLAIN_REQUEST,
                RESPELLED_REQUEST
            ]
        );
    });
}

#[test]
fn each_cut_reaches_the_server_as_its_own_read_in_the_relays_positions() {
    block_on(async {
        let tls = FakeServer::start(Protocol::Tls, |_, reads| {
            if reads.len() >= 3 {
                Reaction::Answer(SERVER_HELLO_HEADER)
            } else {
                Reaction::Reset
            }
        })
        .await;
        let http = FakeServer::start(Protocol::Http, |_, _| Reaction::Answer(FOUND)).await;
        let report = probe_servers(&tls, &http, &settings(1, LONG_WAIT)).await;
        assert_eq!(
            report.assessment.tls_split,
            Some(vec![SplitPosition::Offset(1), SplitPosition::DomainMid])
        );
        let read_sizes: Vec<Vec<usize>> = tls
            .connections()
            .iter()
            .map(|reads| reads.iter().map(Vec::len).collect())
            .collect();
        assert_eq!(
            read_sizes,
            [
                vec![HELLO_LEN],
                vec![1, HELLO_LEN - 1],
                vec![109, HELLO_LEN - 109],
                vec![112, HELLO_LEN - 112],
                vec![1, 111, HELLO_LEN - 112],
            ]
        );
    });
}

#[test]
fn a_whole_hello_answered_in_one_run_only_is_mixed_and_still_needs_the_split() {
    block_on(async {
        let tls = FakeServer::start(Protocol::Tls, |index, _| {
            if index == 5 {
                Reaction::Reset
            } else {
                Reaction::Answer(SERVER_HELLO_HEADER)
            }
        })
        .await;
        let http = FakeServer::start(Protocol::Http, |_, _| Reaction::Answer(FOUND)).await;
        let report = probe_servers(&tls, &http, &settings(2, LONG_WAIT)).await;
        assert_eq!(report.records[0].outcomes, [handshake(), Outcome::Reset]);
        assert_eq!(report.records[0].agreement(), Agreement::SomeRuns);
        assert_eq!(report.assessment.verdict, Verdict::ProfileSuggested);
        assert!(
            report
                .assessment
                .profile
                .unwrap()
                .contains("\ntls-split 1\n")
        );
    });
}

#[test]
fn an_open_site_is_not_blocked_and_gets_no_profile() {
    block_on(async {
        let tls =
            FakeServer::start(Protocol::Tls, |_, _| Reaction::Answer(SERVER_HELLO_HEADER)).await;
        let http = FakeServer::start(Protocol::Http, |_, _| Reaction::Answer(FOUND)).await;
        let report = probe_servers(&tls, &http, &settings(2, LONG_WAIT)).await;
        assert_eq!(report.assessment.verdict, Verdict::NotBlocked);
        assert_eq!(report.assessment.profile, None);
        assert_eq!(report.address, LOOPBACK);
        assert_eq!(report.domain, DOMAIN);
    });
}

#[test]
fn resets_closes_silence_and_foreign_bytes_are_told_apart() {
    block_on(async {
        let cases = [
            (Reaction::Reset, Outcome::Reset),
            (Reaction::Close, Outcome::Closed),
            (Reaction::Silent, Outcome::TimedOut),
        ];
        for (reaction, expected) in cases {
            let tls = FakeServer::start(Protocol::Tls, move |_, _| reaction).await;
            let http = FakeServer::start(Protocol::Http, move |_, _| reaction).await;
            let report = probe_servers(&tls, &http, &settings(1, SHORT_WAIT)).await;
            assert_eq!(
                outcomes(&report),
                vec![vec![expected.clone()]; 7],
                "{reaction:?}"
            );
            assert_eq!(report.assessment.verdict, Verdict::NothingWorked);
        }
        let tls = FakeServer::start(Protocol::Tls, |_, _| Reaction::Answer(FOUND)).await;
        let http =
            FakeServer::start(Protocol::Http, |_, _| Reaction::Answer(SERVER_HELLO_HEADER)).await;
        let report = probe_servers(&tls, &http, &settings(1, SHORT_WAIT)).await;
        let mut expected = vec![vec![Outcome::Unexpected(b"HTTP/1.1".to_vec())]; 5];
        expected.extend(vec![
            vec![Outcome::Unexpected(SERVER_HELLO_HEADER.to_vec())];
            2
        ]);
        assert_eq!(outcomes(&report), expected);
    });
}

#[test]
fn an_alert_counts_as_an_answer_and_says_so() {
    block_on(async {
        let tls = FakeServer::start(Protocol::Tls, |_, _| {
            Reaction::Answer(PROTOCOL_VERSION_ALERT)
        })
        .await;
        let http = FakeServer::start(Protocol::Http, |_, _| Reaction::Answer(FOUND)).await;
        let report = probe_servers(&tls, &http, &settings(1, LONG_WAIT)).await;
        let alert = Outcome::Answered(Reply::Tls {
            record: RecordType::Alert,
            version: 0x0303,
        });
        assert_eq!(report.records[0].outcomes, [alert]);
        assert_eq!(report.assessment.verdict, Verdict::NotBlocked);
    });
}

#[test]
fn a_port_nobody_listens_on_is_a_failed_connect() {
    block_on(async {
        let closed_port = {
            let listener = TcpListener::bind((LOOPBACK, 0)).await.unwrap();
            listener.local_addr().unwrap().port()
        };
        let target = ProbeTarget {
            domain: DOMAIN.to_owned(),
            address: LOOPBACK,
            tls_port: closed_port,
            http_port: closed_port,
        };
        let report = probe_target(&target, &settings(1, SHORT_WAIT))
            .await
            .unwrap();
        for record in &report.records {
            assert!(
                matches!(record.outcomes[..], [Outcome::ConnectFailed(_)]),
                "{record:?}"
            );
        }
        assert_eq!(report.assessment.verdict, Verdict::NothingWorked);
    });
}

#[test]
fn a_name_that_resolves_only_to_loopback_is_never_probed() {
    block_on(async {
        let resolver = FakeResolver {
            answer: Answer::Addresses(vec![LOOPBACK, Ipv4Addr::new(127, 0, 0, 2)]),
        };
        let error = probe(&resolver, DOMAIN, &settings(1, SHORT_WAIT))
            .await
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert!(error.to_string().contains("loopback"), "{error}");

        let nothing = FakeResolver {
            answer: Answer::Addresses(Vec::new()),
        };
        let error = probe(&nothing, DOMAIN, &settings(1, SHORT_WAIT))
            .await
            .unwrap_err();
        assert_eq!(error.to_string(), "example.org has no IPv4 address");

        let failing = FakeResolver {
            answer: Answer::Failure("no upstream answered"),
        };
        let error = probe(&failing, DOMAIN, &settings(1, SHORT_WAIT))
            .await
            .unwrap_err();
        assert_eq!(error.to_string(), "no upstream answered");
    });
}

#[test]
fn a_name_the_relay_could_not_send_is_an_input_error() {
    block_on(async {
        let target = ProbeTarget {
            domain: "bad host.org".to_owned(),
            address: LOOPBACK,
            tls_port: 1,
            http_port: 1,
        };
        let error = probe_target(&target, &settings(1, SHORT_WAIT))
            .await
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    });
}

#[test]
fn a_reset_between_two_pieces_is_a_reset() {
    block_on(async {
        let listener = TcpListener::bind((LOOPBACK, 0)).await.unwrap();
        let tls_port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let mut first = [0u8; 4096];
                let _ = stream.read(&mut first).await;
                let _ = stream.set_zero_linger();
            }
        });
        let http = FakeServer::start(Protocol::Http, |_, _| Reaction::Answer(FOUND)).await;
        let target = ProbeTarget {
            domain: DOMAIN.to_owned(),
            address: LOOPBACK,
            tls_port,
            http_port: http.port,
        };
        let report = probe_target(&target, &settings(1, LONG_WAIT))
            .await
            .unwrap();
        let tls_outcomes: Vec<Vec<Outcome>> = outcomes(&report).into_iter().take(5).collect();
        assert_eq!(tls_outcomes, vec![vec![Outcome::Reset]; 5]);
    });
}
