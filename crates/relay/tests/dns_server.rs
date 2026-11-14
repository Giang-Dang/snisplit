use std::future::Future;
use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use snisplit_core::domains::DomainList;
use snisplit_core::first_flight::RelayProfile;
use snisplit_relay::{DnsServer, UdpResolver, Upstreams};
use tokio::net::UdpSocket;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio::time::timeout;

const MEDIUM_QUERY: &[u8] =
    include_bytes!("../../core/tests/fixtures/dns/windows_query_medium_com_a.bin");
const MEDIUM_ANSWER: &[u8] =
    include_bytes!("../../core/tests/fixtures/dns/upstream_answer_medium_com.bin");
const MEDIUM_AAAA_QUERY: &[u8] =
    include_bytes!("../../core/tests/fixtures/dns/windows_query_medium_com_aaaa.bin");
const MEDIUM_HTTPS_QUERY: &[u8] =
    include_bytes!("../../core/tests/fixtures/dns/windows_query_medium_com_https.bin");

const PATIENCE: Duration = Duration::from_secs(30);
const ANSWER: Ipv4Addr = Ipv4Addr::new(10, 1, 2, 3);
const STORE_NAME: &[u8] = b"\x05store\x0csteampowered\x03com\x00";
const APEX_NAME: &[u8] = b"\x0csteampowered\x03com\x00";
const LOOKALIKE_NAME: &[u8] = b"\x0fnotsteampowered\x03com\x00";
const TYPE_A: [u8; 2] = [0, 1];
const TYPE_AAAA: [u8; 2] = [0, 28];
const TYPE_HTTPS: [u8; 2] = [0, 65];
const CLASS_IN: [u8; 2] = [0, 1];
const QUERY_FLAGS: [u8; 2] = [0x01, 0x00];
const RESPONSE_FLAGS: [u8; 2] = [0x81, 0x80];
const ONE: [u8; 2] = [0, 1];
const ZERO: [u8; 2] = [0, 0];
const A_RECORD_AFTER_NAME: [u8; 16] = [0xc0, 0x0c, 0, 1, 0, 1, 0, 0, 0, 5, 0, 4, 10, 1, 2, 3];

fn block_on<F: Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("runtime builds")
        .block_on(future)
}

fn query(id: [u8; 2], name: &[u8], qtype: [u8; 2]) -> Vec<u8> {
    [
        &id[..],
        &QUERY_FLAGS,
        &ONE,
        &ZERO,
        &ZERO,
        &ZERO,
        name,
        &qtype,
        &CLASS_IN,
    ]
    .concat()
}

fn expected_empty(id: [u8; 2], name: &[u8], qtype: [u8; 2]) -> Vec<u8> {
    [
        &id[..],
        &RESPONSE_FLAGS,
        &ONE,
        &ZERO,
        &ZERO,
        &ZERO,
        name,
        &qtype,
        &CLASS_IN,
    ]
    .concat()
}

fn expected_a(id: [u8; 2], name: &[u8]) -> Vec<u8> {
    [
        &id[..],
        &RESPONSE_FLAGS,
        &ONE,
        &ONE,
        &ZERO,
        &ZERO,
        name,
        &TYPE_A,
        &CLASS_IN,
        &A_RECORD_AFTER_NAME,
    ]
    .concat()
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

fn loopback() -> SocketAddr {
    SocketAddr::from((Ipv4Addr::LOCALHOST, 0))
}

async fn udp_socket() -> UdpSocket {
    UdpSocket::bind(loopback()).await.expect("socket binds")
}

async fn client_to(server: SocketAddr) -> UdpSocket {
    let client = udp_socket().await;
    client.connect(server).await.expect("client connects");
    client
}

struct Running {
    addr: SocketAddr,
    stop: oneshot::Sender<()>,
    task: JoinHandle<()>,
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

async fn start(upstreams: Vec<SocketAddr>) -> Running {
    start_sharing(Upstreams::new(upstreams)).await
}

async fn start_sharing(upstreams: Upstreams) -> Running {
    let resolver = UdpResolver::new(upstreams.clone(), PATIENCE);
    let server = DnsServer::bind(
        loopback(),
        vec![steam_profile()],
        ANSWER,
        upstreams,
        resolver,
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

const UPSTREAM_A_RECORD: [u8; 16] = [0xc0, 0x0c, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4, 203, 0, 113, 7];
const UPSTREAM_AAAA_RECORD: [u8; 28] = [
    0xc0, 0x0c, 0, 28, 0, 1, 0, 0, 0, 60, 0, 16, 0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0x42,
];
const QUESTION_TAIL_LEN: usize = 4;
const HEADER_LEN: usize = 12;

#[derive(Clone, Copy)]
enum Presence {
    Ipv4,
    Ipv6Only,
}

fn upstream_reply(query: &[u8], presence: Presence) -> Vec<u8> {
    let qtype = [query[query.len() - 4], query[query.len() - 3]];
    let record: &[u8] = match (qtype, presence) {
        (TYPE_A, Presence::Ipv4) => &UPSTREAM_A_RECORD,
        (TYPE_AAAA, Presence::Ipv6Only) => &UPSTREAM_AAAA_RECORD,
        _ => &[],
    };
    let answers = u8::from(!record.is_empty());
    let mut reply = vec![query[0], query[1], 0x81, 0x80, 0, 1, 0, answers, 0, 0, 0, 0];
    reply.extend_from_slice(&query[HEADER_LEN..]);
    reply.extend_from_slice(record);
    reply
}

async fn serve_dns(socket: UdpSocket, presence: Presence) {
    let mut buffer = [0u8; 4096];
    while let Ok((length, asker)) = socket.recv_from(&mut buffer).await {
        if length >= HEADER_LEN + QUESTION_TAIL_LEN {
            let reply = upstream_reply(&buffer[..length], presence);
            let _ = socket.send_to(&reply, asker).await;
        }
    }
}

async fn start_with_dns_upstream(presence: Presence) -> Running {
    let upstream = udp_socket().await;
    let address = upstream.local_addr().unwrap();
    tokio::spawn(serve_dns(upstream, presence));
    start(vec![address]).await
}

async fn receive(socket: &UdpSocket) -> Vec<u8> {
    receive_with_peer(socket).await.0
}

async fn receive_with_peer(socket: &UdpSocket) -> (Vec<u8>, SocketAddr) {
    let mut buffer = [0u8; 4096];
    let (length, peer) = timeout(PATIENCE, socket.recv_from(&mut buffer))
        .await
        .expect("a datagram arrives in time")
        .expect("receive works");
    (buffer[..length].to_vec(), peer)
}

async fn ask(server: SocketAddr, packet: &[u8]) -> Vec<u8> {
    let client = client_to(server).await;
    client.send(packet).await.unwrap();
    receive(&client).await
}

fn with_id(reply: &[u8], id: [u8; 2]) -> Vec<u8> {
    [&id[..], &reply[2..]].concat()
}

#[test]
fn a_listed_name_gets_the_answer_address_with_ttl_5() {
    block_on(async {
        let running = start_with_dns_upstream(Presence::Ipv4).await;

        let reply = ask(running.addr, &query([0x33, 0x44], STORE_NAME, TYPE_A)).await;

        assert_eq!(reply, expected_a([0x33, 0x44], STORE_NAME));
        running.shut_down().await;
    });
}

#[test]
fn the_listed_apex_itself_is_answered_too() {
    block_on(async {
        let running = start_with_dns_upstream(Presence::Ipv4).await;

        let reply = ask(running.addr, &query([0x01, 0x02], APEX_NAME, TYPE_A)).await;

        assert_eq!(reply, expected_a([0x01, 0x02], APEX_NAME));
        running.shut_down().await;
    });
}

#[test]
fn aaaa_and_https_for_a_listed_name_get_noerror_without_answers() {
    block_on(async {
        let running = start_with_dns_upstream(Presence::Ipv4).await;

        let aaaa = ask(running.addr, &query([0x54, 0xe5], STORE_NAME, TYPE_AAAA)).await;
        let https = ask(running.addr, &query([0x11, 0xd0], STORE_NAME, TYPE_HTTPS)).await;

        assert_eq!(aaaa, expected_empty([0x54, 0xe5], STORE_NAME, TYPE_AAAA));
        assert_eq!(https, expected_empty([0x11, 0xd0], STORE_NAME, TYPE_HTTPS));
        running.shut_down().await;
    });
}

#[test]
fn an_unlisted_name_is_forwarded_unchanged_and_the_reply_relayed_unchanged() {
    block_on(async {
        let upstream = udp_socket().await;
        let running = start(vec![upstream.local_addr().unwrap()]).await;
        let client = client_to(running.addr).await;
        client.send(MEDIUM_QUERY).await.unwrap();

        let (forwarded, asker) = receive_with_peer(&upstream).await;
        let reply = with_id(MEDIUM_ANSWER, [MEDIUM_QUERY[0], MEDIUM_QUERY[1]]);
        upstream.send_to(&reply, asker).await.unwrap();

        assert_eq!(forwarded, MEDIUM_QUERY);
        assert_eq!(receive(&client).await, reply);
        running.shut_down().await;
    });
}

#[test]
fn a_name_that_merely_ends_like_a_listed_one_is_forwarded() {
    block_on(async {
        let upstream = udp_socket().await;
        let running = start(vec![upstream.local_addr().unwrap()]).await;
        let client = client_to(running.addr).await;
        let lookalike = query([0x0a, 0x0b], LOOKALIKE_NAME, TYPE_A);

        client.send(&lookalike).await.unwrap();

        assert_eq!(receive(&upstream).await, lookalike);
        running.shut_down().await;
    });
}

#[test]
fn other_types_of_unlisted_names_are_forwarded_not_answered_empty() {
    block_on(async {
        let upstream = udp_socket().await;
        let running = start(vec![upstream.local_addr().unwrap()]).await;
        let client = client_to(running.addr).await;

        client.send(MEDIUM_AAAA_QUERY).await.unwrap();
        client.send(MEDIUM_HTTPS_QUERY).await.unwrap();

        let mut forwarded = vec![receive(&upstream).await, receive(&upstream).await];
        forwarded.sort();
        let mut expected = vec![MEDIUM_AAAA_QUERY.to_vec(), MEDIUM_HTTPS_QUERY.to_vec()];
        expected.sort();
        assert_eq!(forwarded, expected);
        running.shut_down().await;
    });
}

#[test]
fn a_reply_with_the_wrong_id_is_not_relayed() {
    block_on(async {
        let upstream = udp_socket().await;
        let running = start(vec![upstream.local_addr().unwrap()]).await;
        let client = client_to(running.addr).await;
        client.send(MEDIUM_QUERY).await.unwrap();
        let (_, asker) = receive_with_peer(&upstream).await;
        let stray = with_id(MEDIUM_ANSWER, [0xde, 0xad]);
        let genuine = with_id(MEDIUM_ANSWER, [MEDIUM_QUERY[0], MEDIUM_QUERY[1]]);

        upstream.send_to(&stray, asker).await.unwrap();
        upstream.send_to(&genuine, asker).await.unwrap();

        assert_eq!(receive(&client).await, genuine);
        running.shut_down().await;
    });
}

#[test]
fn garbage_is_ignored_and_the_server_keeps_serving() {
    block_on(async {
        let running = start_with_dns_upstream(Presence::Ipv4).await;
        let client = client_to(running.addr).await;
        let truncated_header = [0x12u8, 0x34, 0x01];
        let junk = vec![0xffu8; 4096];
        let response_not_query = expected_a([0x07, 0x07], STORE_NAME);

        client.send(&truncated_header).await.unwrap();
        client.send(&junk).await.unwrap();
        client.send(&response_not_query).await.unwrap();
        client
            .send(&query([0x33, 0x44], STORE_NAME, TYPE_A))
            .await
            .unwrap();

        assert_eq!(receive(&client).await, expected_a([0x33, 0x44], STORE_NAME));
        running.shut_down().await;
    });
}

#[test]
fn shutdown_releases_the_port() {
    block_on(async {
        let running = start_with_dns_upstream(Presence::Ipv4).await;
        let addr = running.addr;
        assert_eq!(
            ask(addr, &query([0, 9], STORE_NAME, TYPE_A)).await,
            expected_a([0, 9], STORE_NAME)
        );

        running.shut_down().await;

        UdpSocket::bind(addr).await.expect("the port is free again");
    });
}

#[test]
fn a_listed_name_without_ipv4_gets_the_upstream_answer_for_a_and_aaaa() {
    block_on(async {
        let running = start_with_dns_upstream(Presence::Ipv6Only).await;

        let a = ask(running.addr, &query([0x21, 0x22], STORE_NAME, TYPE_A)).await;
        let aaaa = ask(running.addr, &query([0x23, 0x24], STORE_NAME, TYPE_AAAA)).await;

        assert_eq!(a, expected_empty([0x21, 0x22], STORE_NAME, TYPE_A));
        let mut expected_aaaa = expected_empty([0x23, 0x24], STORE_NAME, TYPE_AAAA);
        expected_aaaa[7] = 1;
        expected_aaaa.extend_from_slice(&UPSTREAM_AAAA_RECORD);
        assert_eq!(aaaa, expected_aaaa);
        running.shut_down().await;
    });
}

#[test]
fn a_replaced_upstream_list_is_used_by_the_next_forwarded_query() {
    block_on(async {
        let first = udp_socket().await;
        let second = udp_socket().await;
        let upstreams = Upstreams::new(vec![first.local_addr().unwrap()]);
        let running = start_sharing(upstreams.clone()).await;
        assert!(upstreams.replace(vec![second.local_addr().unwrap()]));
        let client = client_to(running.addr).await;
        client.send(MEDIUM_QUERY).await.unwrap();

        let (forwarded, asker) = receive_with_peer(&second).await;
        let reply = with_id(MEDIUM_ANSWER, [MEDIUM_QUERY[0], MEDIUM_QUERY[1]]);
        second.send_to(&reply, asker).await.unwrap();

        assert_eq!(forwarded, MEDIUM_QUERY);
        assert_eq!(receive(&client).await, reply);
        running.shut_down().await;
    });
}

#[test]
fn an_occupied_address_fails_the_bind_and_the_error_names_it() {
    block_on(async {
        let occupant = udp_socket().await;
        let taken = occupant.local_addr().unwrap();

        let result = DnsServer::bind(
            taken,
            vec![steam_profile()],
            ANSWER,
            Upstreams::new(Vec::new()),
            UdpResolver::new(Upstreams::new(Vec::new()), PATIENCE),
        )
        .await;

        let error = result.err().expect("the occupied address fails the bind");
        assert_eq!(error.kind(), std::io::ErrorKind::AddrInUse);
        assert!(
            error
                .to_string()
                .starts_with(&format!("binding {taken} failed: ")),
            "{error}"
        );
    });
}
