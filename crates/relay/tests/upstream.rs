use std::future::Future;
use std::io;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use snisplit_relay::{Resolve, UdpResolver, Upstreams};
use tokio::net::UdpSocket;
use tokio::task::JoinHandle;

const MEDIUM_ANSWER: &[u8] =
    include_bytes!("../../core/tests/fixtures/dns/upstream_answer_medium_com.bin");

const SHORT_PATIENCE: Duration = Duration::from_millis(250);
const GENEROUS_PATIENCE: Duration = Duration::from_secs(30);
const WELL_UNDER_GENEROUS_PATIENCE: Duration = Duration::from_secs(5);
const MEDIUM_ADDRESSES: [Ipv4Addr; 2] = [
    Ipv4Addr::new(0xa2, 0x9f, 0x99, 0x04),
    Ipv4Addr::new(0xa2, 0x9f, 0x98, 0x04),
];
const FIRST_RDATA_OFFSET: usize = 0x28;
const ID_LEN: usize = 2;
const EMPTY_ANSWER_TAIL: [u8; 10] = [0x81, 0x80, 0, 1, 0, 0, 0, 0, 0, 0];
const NAME_ERROR_TAIL: [u8; 10] = [0x81, 0x83, 0, 1, 0, 0, 0, 0, 0, 0];

fn block_on<F: Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("runtime builds")
        .block_on(future)
}

#[derive(Clone, Copy)]
enum Script {
    Silent,
    MediumAnswer,
    StrayThenMediumAnswer,
    EmptyAnswer,
    NameError,
}

struct FakeDns {
    addr: SocketAddr,
    queries: Arc<AtomicUsize>,
    task: JoinHandle<()>,
}

impl Drop for FakeDns {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl FakeDns {
    async fn start(script: Script) -> FakeDns {
        let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("fake dns binds");
        let addr = socket.local_addr().unwrap();
        let queries = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&queries);
        let task = tokio::spawn(async move {
            let mut buffer = [0u8; 4096];
            loop {
                let Ok((length, asker)) = socket.recv_from(&mut buffer).await else {
                    continue;
                };
                counted.fetch_add(1, Ordering::SeqCst);
                for reply in replies(script, &buffer[..length]) {
                    let _ = socket.send_to(&reply, asker).await;
                }
            }
        });
        FakeDns {
            addr,
            queries,
            task,
        }
    }

    fn queries_seen(&self) -> usize {
        self.queries.load(Ordering::SeqCst)
    }
}

fn replies(script: Script, query: &[u8]) -> Vec<Vec<u8>> {
    let id = &query[..ID_LEN];
    match script {
        Script::Silent => Vec::new(),
        Script::MediumAnswer => vec![medium_answer_with(id)],
        Script::StrayThenMediumAnswer => {
            let wrong_id = [id[0] ^ 0xff, id[1]];
            let mut stray = medium_answer_with(&wrong_id);
            stray[FIRST_RDATA_OFFSET..FIRST_RDATA_OFFSET + 4].copy_from_slice(&[6, 6, 6, 6]);
            let mut wrong_data_right_id = medium_answer_with(id);
            wrong_data_right_id[3] = 0x82;
            vec![stray, wrong_data_right_id, medium_answer_with(id)]
        }
        Script::EmptyAnswer => {
            let question = &query[ID_LEN + 10..];
            vec![[id, &EMPTY_ANSWER_TAIL, question].concat()]
        }
        Script::NameError => {
            let question = &query[ID_LEN + 10..];
            vec![[id, &NAME_ERROR_TAIL, question].concat()]
        }
    }
}

fn medium_answer_with(id: &[u8]) -> Vec<u8> {
    [id, &MEDIUM_ANSWER[ID_LEN..]].concat()
}

fn resolver(servers: &[&FakeDns], timeout: Duration) -> UdpResolver {
    UdpResolver::new(
        Upstreams::new(servers.iter().map(|server| server.addr).collect()),
        timeout,
    )
}

#[test]
fn addresses_come_from_a_real_upstream_answer_in_order() {
    block_on(async {
        let server = FakeDns::start(Script::MediumAnswer).await;
        let resolver = resolver(&[&server], GENEROUS_PATIENCE);

        let addresses = resolver.resolve("medium.com").await.unwrap();

        assert_eq!(addresses, MEDIUM_ADDRESSES);
    });
}

#[test]
fn replies_with_a_wrong_id_or_an_error_code_are_ignored_until_the_right_one() {
    block_on(async {
        let server = FakeDns::start(Script::StrayThenMediumAnswer).await;
        let resolver = resolver(&[&server], GENEROUS_PATIENCE);

        let addresses = resolver.resolve("medium.com").await.unwrap();

        assert_eq!(addresses, MEDIUM_ADDRESSES);
    });
}

#[test]
fn the_second_server_is_used_when_the_first_stays_silent() {
    block_on(async {
        let silent = FakeDns::start(Script::Silent).await;
        let working = FakeDns::start(Script::MediumAnswer).await;
        let resolver = resolver(&[&silent, &working], SHORT_PATIENCE);

        let addresses = resolver.resolve("medium.com").await.unwrap();

        assert_eq!(addresses, MEDIUM_ADDRESSES);
        assert_eq!(silent.queries_seen(), 1);
        assert_eq!(working.queries_seen(), 1);
    });
}

#[test]
fn every_server_silent_is_an_error_naming_the_host() {
    block_on(async {
        let first = FakeDns::start(Script::Silent).await;
        let second = FakeDns::start(Script::Silent).await;
        let resolver = resolver(&[&first, &second], SHORT_PATIENCE);

        let error = resolver.resolve("medium.com").await.unwrap_err();

        assert!(error.to_string().contains("medium.com"), "{error}");
        assert_eq!(first.queries_seen(), 1);
        assert_eq!(second.queries_seen(), 1);
    });
}

#[test]
fn a_successful_answer_is_cached_by_lowercased_name() {
    block_on(async {
        let server = FakeDns::start(Script::MediumAnswer).await;
        let resolver = resolver(&[&server], GENEROUS_PATIENCE);

        let first = resolver.resolve("medium.com").await.unwrap();
        let second = resolver.resolve("MEDIUM.com").await.unwrap();

        assert_eq!(first, MEDIUM_ADDRESSES);
        assert_eq!(second, MEDIUM_ADDRESSES);
        assert_eq!(server.queries_seen(), 1);
    });
}

#[test]
fn an_empty_answer_is_returned_empty_and_asked_again_next_time() {
    block_on(async {
        let server = FakeDns::start(Script::EmptyAnswer).await;
        let resolver = resolver(&[&server], GENEROUS_PATIENCE);

        let first = resolver.resolve("medium.com").await.unwrap();
        let second = resolver.resolve("medium.com").await.unwrap();

        assert_eq!(first, Vec::<Ipv4Addr>::new());
        assert_eq!(second, Vec::<Ipv4Addr>::new());
        assert_eq!(server.queries_seen(), 2);
    });
}

#[test]
fn a_name_that_does_not_exist_is_answered_empty_at_once_without_asking_the_next_server() {
    block_on(async {
        let missing = FakeDns::start(Script::NameError).await;
        let next = FakeDns::start(Script::MediumAnswer).await;
        let resolver = resolver(&[&missing, &next], GENEROUS_PATIENCE);

        let addresses =
            tokio::time::timeout(WELL_UNDER_GENEROUS_PATIENCE, resolver.resolve("medium.com"))
                .await
                .expect("a name error ends the lookup without waiting for the timeout")
                .unwrap();

        assert_eq!(addresses, Vec::<Ipv4Addr>::new());
        assert_eq!(missing.queries_seen(), 1);
        assert_eq!(next.queries_seen(), 0);
    });
}

#[test]
fn an_invalid_host_is_rejected_without_sending_anything() {
    block_on(async {
        let server = FakeDns::start(Script::MediumAnswer).await;
        let resolver = resolver(&[&server], SHORT_PATIENCE);
        let overlong_label = "a".repeat(64) + ".com";

        for host in ["bad..name", "", overlong_label.as_str(), "sp ace.com"] {
            let error = resolver.resolve(host).await.unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput, "{host:?}");
        }
        let probe = resolver.resolve("medium.com").await.unwrap();
        assert_eq!(probe, MEDIUM_ADDRESSES);
        assert_eq!(server.queries_seen(), 1);
    });
}
