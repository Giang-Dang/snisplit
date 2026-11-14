use std::net::{Ipv4Addr, SocketAddr};
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

async fn start(upstreams: Vec<SocketAddr>, upstream_timeout: Duration) -> Running {
    let server = DnsServer::bind_with(
        loopback(),
        vec![steam_profile()],
        ANSWER,
        upstreams,
        upstream_timeout,
    )
    .await
    .expect("server binds");
    let addr = server.local_addr();
    let (stop, stopped) = oneshot::channel::<()>();
    let task = tokio::spawn(server.run(async {
        let _ = stopped.await;
    }));
    Running { addr, stop, task }
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
        let running = start(
            vec![first.local_addr().unwrap(), second.local_addr().unwrap()],
            SHORT_PATIENCE,
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
        let running = start(
            vec![first.local_addr().unwrap(), second.local_addr().unwrap()],
            SHORT_PATIENCE,
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
        let running = start(vec![silent.local_addr().unwrap()], PATIENCE).await;
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
        let running = start(vec![silent.local_addr().unwrap()], PATIENCE).await;
        let client = client_to(running.addr).await;
        client.send(MEDIUM_QUERY).await.unwrap();
        let (_, asker) = receive_with_peer(&silent).await;
        assert_ne!(asker, running.addr);
        running.shut_down().await;
    });
}
