use std::io;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use snisplit_core::domains::DomainList;
use snisplit_core::first_flight::RelayProfile;
use tokio::net::UdpSocket;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio::time::timeout;

use super::*;

const MEDIUM_QUERY: &[u8] =
    include_bytes!("../../../core/tests/fixtures/dns/windows_query_medium_com_a.bin");
const SHORT_PATIENCE: Duration = Duration::from_millis(150);
const NOTHING_ARRIVES_WITHIN: Duration = Duration::from_millis(1000);
const PATIENCE: Duration = Duration::from_secs(30);
const ANSWER: Ipv4Addr = Ipv4Addr::new(10, 1, 2, 3);
const STEAM_QUERY_ID: u16 = 0x3344;
const LONG_QUERY_NAME: &[u8] = b"\x05store\x0csteampowered\x03com\x00";

fn block_on<F: Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("runtime builds")
        .block_on(future)
}

fn steam_profile() -> RelayProfile {
    RelayProfile {
        name: "steam".to_owned(),
        domains: DomainList::new(["steampowered.com"]),
        tls_positions: Vec::new(),
        tls_ports: Vec::new(),
        http_ports: Vec::new(),
        http_method: snisplit_core::first_flight::HttpMethod::HostSpell,
    }
}

fn steam_a_query() -> Vec<u8> {
    let mut packet = vec![0x33, 0x44, 0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0];
    packet.extend_from_slice(LONG_QUERY_NAME);
    packet.extend_from_slice(&[0, 1, 0, 1]);
    packet
}

fn loopback() -> SocketAddr {
    SocketAddr::from((Ipv4Addr::LOCALHOST, 0))
}

async fn silent_upstream() -> UdpSocket {
    UdpSocket::bind(loopback())
        .await
        .expect("fake upstream binds")
}

async fn client_to(server: SocketAddr) -> UdpSocket {
    let client = UdpSocket::bind(loopback()).await.expect("client binds");
    client.connect(server).await.expect("client connects");
    client
}

struct Running {
    addr: SocketAddr,
    stop: oneshot::Sender<()>,
    task: JoinHandle<()>,
}

enum Outcome {
    Found(Vec<Ipv4Addr>),
    Empty,
    Fails,
    Never,
}

struct FakeResolver {
    outcome: Outcome,
    calls: Arc<AtomicUsize>,
}

impl Resolve for FakeResolver {
    async fn resolve(&self, _host: &str) -> io::Result<Vec<Ipv4Addr>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        match &self.outcome {
            Outcome::Found(addresses) => Ok(addresses.clone()),
            Outcome::Empty => Ok(Vec::new()),
            Outcome::Fails => Err(io::Error::other("fake lookup failure")),
            Outcome::Never => std::future::pending().await,
        }
    }
}

fn found() -> Outcome {
    Outcome::Found(vec![Ipv4Addr::new(203, 0, 113, 7)])
}

async fn start(
    upstreams: Vec<SocketAddr>,
    upstream_timeout: Duration,
    outcome: Outcome,
) -> (Running, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let resolver = FakeResolver {
        outcome,
        calls: Arc::clone(&calls),
    };
    let server = DnsServer::bind_with(
        loopback(),
        vec![steam_profile()],
        ANSWER,
        Upstreams::new(upstreams),
        upstream_timeout,
        Box::new(resolver),
    )
    .await
    .expect("server binds");
    let addr = server.local_addr();
    let (stop, stopped) = oneshot::channel::<()>();
    let task = tokio::spawn(server.run(async {
        let _ = stopped.await;
    }));
    (Running { addr, stop, task }, calls)
}

impl Running {
    async fn shut_down(self) {
        let _ = self.stop.send(());
        timeout(PATIENCE, self.task)
            .await
            .expect("run returns in time")
            .expect("run does not panic");
    }
}

async fn receive(socket: &UdpSocket) -> Vec<u8> {
    let mut buffer = [0u8; 4096];
    let length = timeout(PATIENCE, socket.recv(&mut buffer))
        .await
        .expect("a datagram arrives in time")
        .expect("receive works");
    buffer[..length].to_vec()
}

async fn receive_with_peer(socket: &UdpSocket) -> (Vec<u8>, SocketAddr) {
    let mut buffer = [0u8; 4096];
    let (length, peer) = timeout(PATIENCE, socket.recv_from(&mut buffer))
        .await
        .expect("a datagram arrives in time")
        .expect("receive works");
    (buffer[..length].to_vec(), peer)
}

#[test]
fn second_upstream_is_asked_after_the_first_stays_silent() {
    block_on(async {
        let first = silent_upstream().await;
        let second = silent_upstream().await;
        let (running, _) = start(
            vec![first.local_addr().unwrap(), second.local_addr().unwrap()],
            SHORT_PATIENCE,
            found(),
        )
        .await;
        let client = client_to(running.addr).await;

        client.send(MEDIUM_QUERY).await.unwrap();

        assert_eq!(receive(&first).await, MEDIUM_QUERY);
        let (query, asker) = receive_with_peer(&second).await;
        assert_eq!(query, MEDIUM_QUERY);
        let reply = [&MEDIUM_QUERY[..2], &[0x81, 0x80, 0, 1, 0, 0, 0, 0, 0, 0]].concat();
        second.send_to(&reply, asker).await.unwrap();
        assert_eq!(receive(&client).await, reply);
        running.shut_down().await;
    });
}

#[test]
fn nothing_is_sent_to_the_client_when_every_upstream_fails() {
    block_on(async {
        let first = silent_upstream().await;
        let second = silent_upstream().await;
        let (running, _) = start(
            vec![first.local_addr().unwrap(), second.local_addr().unwrap()],
            SHORT_PATIENCE,
            found(),
        )
        .await;
        let client = client_to(running.addr).await;

        client.send(MEDIUM_QUERY).await.unwrap();

        assert_eq!(receive(&first).await, MEDIUM_QUERY);
        assert_eq!(receive(&second).await, MEDIUM_QUERY);
        let mut buffer = [0u8; 4096];
        let nothing = timeout(NOTHING_ARRIVES_WITHIN, client.recv(&mut buffer)).await;
        assert!(nothing.is_err(), "the client received a datagram");
        running.shut_down().await;
    });
}

#[test]
fn a_pending_forward_neither_blocks_other_queries_nor_shutdown() {
    block_on(async {
        let silent = silent_upstream().await;
        let (running, _) = start(vec![silent.local_addr().unwrap()], PATIENCE, found()).await;
        let slow_client = client_to(running.addr).await;
        let quick_client = client_to(running.addr).await;
        slow_client.send(MEDIUM_QUERY).await.unwrap();
        assert_eq!(receive(&silent).await, MEDIUM_QUERY);

        quick_client.send(&steam_a_query()).await.unwrap();

        let answer = receive(&quick_client).await;
        assert_eq!(&answer[..2], &STEAM_QUERY_ID.to_be_bytes());
        running.shut_down().await;
    });
}

#[test]
fn a_forward_leaves_from_its_own_socket_not_the_listening_one() {
    block_on(async {
        let silent = silent_upstream().await;
        let (running, _) = start(vec![silent.local_addr().unwrap()], PATIENCE, found()).await;
        let client = client_to(running.addr).await;
        client.send(MEDIUM_QUERY).await.unwrap();
        let (_, asker) = receive_with_peer(&silent).await;
        assert_ne!(asker, running.addr);
        running.shut_down().await;
    });
}

const TYPE_A_BYTES: [u8; 2] = [0, 1];
const TYPE_AAAA_BYTES: [u8; 2] = [0, 28];
const CLASS_IN_BYTES: [u8; 2] = [0, 1];
const FIXED_AAAA_RECORD: [u8; 28] = [
    0xc0, 0x0c, 0, 28, 0, 1, 0, 0, 0, 60, 0, 16, 0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0x42,
];
const RELAY_A_RECORD: [u8; 16] = [0xc0, 0x0c, 0, 1, 0, 1, 0, 0, 0, 5, 0, 4, 10, 1, 2, 3];

fn steam_query(qtype: [u8; 2]) -> Vec<u8> {
    let mut packet = vec![0x33, 0x44, 0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0];
    packet.extend_from_slice(LONG_QUERY_NAME);
    packet.extend_from_slice(&qtype);
    packet.extend_from_slice(&CLASS_IN_BYTES);
    packet
}

fn steam_reply(qtype: [u8; 2], answers: u8, record: &[u8]) -> Vec<u8> {
    let mut packet = vec![0x33, 0x44, 0x81, 0x80, 0, 1, 0, answers, 0, 0, 0, 0];
    packet.extend_from_slice(LONG_QUERY_NAME);
    packet.extend_from_slice(&qtype);
    packet.extend_from_slice(&CLASS_IN_BYTES);
    packet.extend_from_slice(record);
    packet
}

fn medium_reply() -> Vec<u8> {
    [&MEDIUM_QUERY[..2], &[0x81, 0x80, 0, 1, 0, 0, 0, 0, 0, 0]].concat()
}

async fn nothing_arrives(socket: &UdpSocket) -> bool {
    let mut buffer = [0u8; 4096];
    timeout(NOTHING_ARRIVES_WITHIN, socket.recv(&mut buffer))
        .await
        .is_err()
}

async fn forwarded_and_relayed(outcome: Outcome) {
    let upstream = silent_upstream().await;
    let (running, calls) = start(vec![upstream.local_addr().unwrap()], PATIENCE, outcome).await;
    let client = client_to(running.addr).await;
    let cases = [
        (steam_query(TYPE_A_BYTES), steam_reply(TYPE_A_BYTES, 0, &[])),
        (
            steam_query(TYPE_AAAA_BYTES),
            steam_reply(TYPE_AAAA_BYTES, 1, &FIXED_AAAA_RECORD),
        ),
    ];
    for (query, upstream_reply) in cases {
        client.send(&query).await.unwrap();
        let (forwarded, asker) = receive_with_peer(&upstream).await;
        assert_eq!(forwarded, query);
        upstream.send_to(&upstream_reply, asker).await.unwrap();
        assert_eq!(receive(&client).await, upstream_reply);
    }
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    running.shut_down().await;
}

#[test]
fn a_listed_name_with_addresses_gets_the_relay_address_and_empty_other_types() {
    block_on(async {
        let upstream = silent_upstream().await;
        let (running, calls) = start(vec![upstream.local_addr().unwrap()], PATIENCE, found()).await;
        let client = client_to(running.addr).await;

        client.send(&steam_query(TYPE_A_BYTES)).await.unwrap();
        let a_reply = receive(&client).await;
        client.send(&steam_query(TYPE_AAAA_BYTES)).await.unwrap();
        let aaaa_reply = receive(&client).await;

        assert_eq!(a_reply, steam_reply(TYPE_A_BYTES, 1, &RELAY_A_RECORD));
        assert_eq!(aaaa_reply, steam_reply(TYPE_AAAA_BYTES, 0, &[]));
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert!(nothing_arrives(&upstream).await, "upstream was asked");
        running.shut_down().await;
    });
}

#[test]
fn a_listed_name_without_ipv4_addresses_is_forwarded_for_every_type() {
    block_on(forwarded_and_relayed(Outcome::Empty));
}

#[test]
fn a_listed_name_whose_lookup_fails_is_forwarded_for_every_type() {
    block_on(forwarded_and_relayed(Outcome::Fails));
}

#[test]
fn an_unlisted_name_is_forwarded_without_consulting_the_resolver() {
    block_on(async {
        let upstream = silent_upstream().await;
        let (running, calls) = start(vec![upstream.local_addr().unwrap()], PATIENCE, found()).await;
        let client = client_to(running.addr).await;

        client.send(MEDIUM_QUERY).await.unwrap();
        let (forwarded, asker) = receive_with_peer(&upstream).await;
        upstream.send_to(&medium_reply(), asker).await.unwrap();

        assert_eq!(forwarded, MEDIUM_QUERY);
        assert_eq!(receive(&client).await, medium_reply());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        running.shut_down().await;
    });
}

#[test]
fn a_slow_lookup_for_a_listed_name_does_not_delay_an_unlisted_name() {
    block_on(async {
        let upstream = silent_upstream().await;
        let (running, calls) = start(
            vec![upstream.local_addr().unwrap()],
            PATIENCE,
            Outcome::Never,
        )
        .await;
        let stuck_client = client_to(running.addr).await;
        let quick_client = client_to(running.addr).await;
        stuck_client.send(&steam_query(TYPE_A_BYTES)).await.unwrap();

        quick_client.send(MEDIUM_QUERY).await.unwrap();
        let (forwarded, asker) = receive_with_peer(&upstream).await;
        upstream.send_to(&medium_reply(), asker).await.unwrap();

        assert_eq!(forwarded, MEDIUM_QUERY);
        assert_eq!(receive(&quick_client).await, medium_reply());
        assert!(nothing_arrives(&stuck_client).await, "stuck query answered");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        running.shut_down().await;
    });
}
