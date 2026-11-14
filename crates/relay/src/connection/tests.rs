use std::future::Future;
use std::net::SocketAddr;

use snisplit_core::domains::DomainList;
use snisplit_core::http_stream::StopReason;
use tokio::net::TcpListener;
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};
use tokio::task::JoinHandle;

use super::*;
use crate::config::Config;
use crate::resolve::Resolve;

const PATIENCE: Duration = Duration::from_secs(30);
const MEDIUM_HOST: &str = "medium.com";

const REQUEST_1: &[u8] = b"GET /1 HTTP/1.1\r\nHost: medium.com\r\n\r\n";
const REQUEST_1_RESPELLED: &[u8] = b"GET /1 HTTP/1.1\r\nhoSt: medium.com\r\n\r\n";
const REQUEST_2: &[u8] = b"GET /2 HTTP/1.1\r\nHOST: medium.com\r\nAccept: */*\r\n\r\n";
const REQUEST_2_RESPELLED: &[u8] = b"GET /2 HTTP/1.1\r\nhoSt: medium.com\r\nAccept: */*\r\n\r\n";
const RESPONSE_1: &[u8] =
    b"HTTP/1.1 301 Moved Permanently\r\nHost: kept\r\nLocation: https://medium.com/\r\n\
      Content-Length: 0\r\n\r\n";
const RESPONSE_2: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nHost:";
const HEAD_WITHOUT_ITS_END: &[u8] = b"GET / HTTP/1.1\r\nHost: medium.com\r\n";
const MEBIBYTE: usize = 1 << 20;

fn block_on<F: Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("runtime builds")
        .block_on(future)
}

async fn within<T>(future: impl Future<Output = T>) -> T {
    timeout(PATIENCE, future)
        .await
        .expect("operation finished in time")
}

fn closed_addresses<const N: usize>() -> [SocketAddr; N] {
    let listeners: Vec<std::net::TcpListener> = (0..N)
        .map(|_| std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap())
        .collect();
    let addresses: Vec<SocketAddr> = listeners
        .iter()
        .map(|listener| listener.local_addr().unwrap())
        .collect();
    addresses.try_into().unwrap()
}

#[test]
fn the_next_candidate_is_tried_when_the_first_refuses() {
    block_on(async {
        let live = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let live_address = live.local_addr().unwrap();
        let [closed] = closed_addresses();
        let candidates = [closed, live_address];

        let stream = connect_first(&candidates, PATIENCE).await.unwrap();

        assert_eq!(stream.peer_addr().unwrap(), live_address);
    });
}

#[test]
fn when_every_candidate_refuses_the_error_names_the_last_one() {
    block_on(async {
        let [first, last] = closed_addresses();

        let error = connect_first(&[first, last], PATIENCE).await.unwrap_err();

        assert!(error.contains(&last.to_string()), "{error}");
    });
}

struct LoopbackResolver;

impl Resolve for LoopbackResolver {
    async fn resolve(&self, _host: &str) -> io::Result<Vec<Ipv4Addr>> {
        Ok(vec![Ipv4Addr::LOCALHOST])
    }
}

fn medium_profile(http_method: HttpMethod) -> RelayProfile {
    RelayProfile {
        name: "medium".to_owned(),
        domains: DomainList::new([MEDIUM_HOST]),
        tls_positions: Vec::new(),
        tls_ports: vec![443],
        http_ports: vec![80],
        http_method,
    }
}

struct Relayed {
    client: TcpStream,
    at_server: TcpStream,
    upstream_port: u16,
    events: UnboundedReceiver<Event>,
    serving: JoinHandle<()>,
}

impl Relayed {
    async fn start(http_method: HttpMethod, first_write: &[u8]) -> Relayed {
        let upstream = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let upstream_port = upstream.local_addr().unwrap().port();
        let (sender, events) = unbounded_channel();
        let on_event: EventSink = Arc::new(move |event| {
            let _ = sender.send(event);
        });
        let config = Config {
            listeners: Vec::new(),
            profiles: vec![medium_profile(http_method)],
            piece_gap: Duration::from_millis(5),
            first_flight_timeout: PATIENCE,
            max_first_flight: 1 << 14,
            connect_timeout: PATIENCE,
        };
        let shared = Arc::new(Shared::new(
            config,
            Box::new(LoopbackResolver),
            on_event,
            |_| true,
        ));
        let route = Route {
            protocol: Protocol::Http,
            profile: 0,
            upstream_port,
        };
        let entrance = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let mut client = within(TcpStream::connect(entrance.local_addr().unwrap()))
            .await
            .unwrap();
        let (accepted, _) = within(entrance.accept()).await.unwrap();
        let serving = tokio::spawn(serve(accepted, shared, route));
        client.write_all(first_write).await.unwrap();
        let (at_server, _) = within(upstream.accept()).await.unwrap();
        Relayed {
            client,
            at_server,
            upstream_port,
            events,
            serving,
        }
    }

    async fn next_event(&mut self) -> Event {
        within(self.events.recv()).await.expect("an event arrives")
    }

    async fn forwarded(&mut self, pieces: usize) {
        assert_eq!(
            self.next_event().await,
            Event::Forwarded {
                host: MEDIUM_HOST.to_owned(),
                port: self.upstream_port,
                pieces,
            }
        );
    }

    async fn closes_both_ways(&mut self) {
        self.client.shutdown().await.unwrap();
        assert_eq!(read_to_end(&mut self.at_server).await, b"");
        self.at_server.shutdown().await.unwrap();
        assert_eq!(read_to_end(&mut self.client).await, b"");
    }

    async fn remaining_events(mut self) -> Vec<Event> {
        within(self.serving).await.expect("serve returns");
        let mut remaining = Vec::new();
        while let Some(event) = within(self.events.recv()).await {
            remaining.push(event);
        }
        remaining
    }

    fn closed(&self, to_server: usize, to_client: usize, respelled: usize) -> Event {
        Event::Closed {
            host: MEDIUM_HOST.to_owned(),
            port: self.upstream_port,
            to_server: u64::try_from(to_server).unwrap(),
            to_client: u64::try_from(to_client).unwrap(),
            respelled,
            stopped: None,
        }
    }
}

async fn read_exactly(stream: &mut TcpStream, len: usize) -> Vec<u8> {
    let mut bytes = vec![0; len];
    within(stream.read_exact(&mut bytes)).await.unwrap();
    bytes
}

async fn read_to_end(stream: &mut TcpStream) -> Vec<u8> {
    let mut bytes = Vec::new();
    within(stream.read_to_end(&mut bytes)).await.unwrap();
    bytes
}

#[test]
fn two_requests_on_one_connection_reach_the_server_respelled_and_the_replies_come_back_unchanged() {
    block_on(async {
        let mut relayed = Relayed::start(HttpMethod::HostSpell, REQUEST_1).await;

        assert_eq!(
            read_exactly(&mut relayed.at_server, REQUEST_1_RESPELLED.len()).await,
            REQUEST_1_RESPELLED
        );
        relayed.forwarded(1).await;
        relayed.at_server.write_all(RESPONSE_1).await.unwrap();
        assert_eq!(
            read_exactly(&mut relayed.client, RESPONSE_1.len()).await,
            RESPONSE_1
        );
        relayed.client.write_all(REQUEST_2).await.unwrap();
        assert_eq!(
            read_exactly(&mut relayed.at_server, REQUEST_2_RESPELLED.len()).await,
            REQUEST_2_RESPELLED
        );
        relayed.at_server.write_all(RESPONSE_2).await.unwrap();
        assert_eq!(
            read_exactly(&mut relayed.client, RESPONSE_2.len()).await,
            RESPONSE_2
        );
        relayed.closes_both_ways().await;

        let expected = relayed.closed(
            REQUEST_1.len() + REQUEST_2.len(),
            RESPONSE_1.len() + RESPONSE_2.len(),
            2,
        );
        assert_eq!(relayed.remaining_events().await, [expected]);
    });
}

#[test]
fn a_client_that_half_closes_after_its_request_still_gets_the_whole_reply() {
    block_on(async {
        let mut relayed = Relayed::start(HttpMethod::HostSpell, REQUEST_1).await;
        relayed.client.shutdown().await.unwrap();

        assert_eq!(
            read_to_end(&mut relayed.at_server).await,
            REQUEST_1_RESPELLED
        );
        relayed.at_server.write_all(RESPONSE_1).await.unwrap();
        relayed.at_server.write_all(RESPONSE_2).await.unwrap();
        relayed.at_server.shutdown().await.unwrap();

        let reply = read_to_end(&mut relayed.client).await;
        assert_eq!(reply, [RESPONSE_1, RESPONSE_2].concat());
        relayed.forwarded(1).await;
        let expected = relayed.closed(REQUEST_1.len(), RESPONSE_1.len() + RESPONSE_2.len(), 1);
        assert_eq!(relayed.remaining_events().await, [expected]);
    });
}

#[test]
fn a_server_that_closes_first_ends_the_reply_and_the_client_can_still_close_cleanly() {
    block_on(async {
        let mut relayed = Relayed::start(HttpMethod::HostSpell, REQUEST_1).await;
        read_exactly(&mut relayed.at_server, REQUEST_1_RESPELLED.len()).await;
        relayed.at_server.write_all(RESPONSE_1).await.unwrap();
        relayed.at_server.shutdown().await.unwrap();

        assert_eq!(read_to_end(&mut relayed.client).await, RESPONSE_1);
        relayed.client.shutdown().await.unwrap();
        assert_eq!(read_to_end(&mut relayed.at_server).await, b"");

        relayed.forwarded(1).await;
        let expected = relayed.closed(REQUEST_1.len(), RESPONSE_1.len(), 1);
        assert_eq!(relayed.remaining_events().await, [expected]);
    });
}

#[test]
fn a_first_flight_that_ends_inside_the_head_is_respelled_once_the_head_is_complete() {
    block_on(async {
        let mut relayed = Relayed::start(HttpMethod::HostSpell, HEAD_WITHOUT_ITS_END).await;
        relayed.forwarded(0).await;

        relayed
            .client
            .write_all(b"Accept: */*\r\n\r\n")
            .await
            .unwrap();

        let expected: &[u8] = b"GET / HTTP/1.1\r\nhoSt: medium.com\r\nAccept: */*\r\n\r\n";
        assert_eq!(
            read_exactly(&mut relayed.at_server, expected.len()).await,
            expected
        );
        relayed.closes_both_ways().await;
        let closed = relayed.closed(expected.len(), 0, 1);
        assert_eq!(relayed.remaining_events().await, [closed]);
    });
}

#[test]
fn a_head_still_held_when_the_client_closes_reaches_the_server_unchanged() {
    block_on(async {
        let mut relayed = Relayed::start(HttpMethod::HostSpell, HEAD_WITHOUT_ITS_END).await;
        relayed.forwarded(0).await;

        relayed.client.shutdown().await.unwrap();

        assert_eq!(
            read_to_end(&mut relayed.at_server).await,
            b"GET / HTTP/1.1\r\nHost: medium.com\r\n"
        );
        relayed.at_server.shutdown().await.unwrap();
        assert_eq!(read_to_end(&mut relayed.client).await, b"");
        let closed = relayed.closed(HEAD_WITHOUT_ITS_END.len(), 0, 0);
        assert_eq!(relayed.remaining_events().await, [closed]);
    });
}

fn mebibyte_body() -> Vec<u8> {
    let phrase: &[u8] = b"\r\n\r\nGET / HTTP/1.1\r\nHost: medium.com\r\n\r\n";
    (0_u8..=250)
        .cycle()
        .flat_map(|counter| phrase.iter().copied().chain([counter]))
        .take(MEBIBYTE)
        .collect()
}

#[test]
fn a_mebibyte_body_arrives_intact_and_the_request_after_it_is_respelled() {
    block_on(async {
        let body = mebibyte_body();
        assert_eq!(body.len(), 1_048_576);
        let head: &[u8] =
            b"POST /upload HTTP/1.1\r\nHost: medium.com\r\nContent-Length: 1048576\r\n\r\n";
        let request = [head, &body, REQUEST_2].concat();
        let mut relayed = Relayed::start(HttpMethod::HostSpell, head).await;
        let mut client = relayed.client;
        let rest = request[head.len()..].to_vec();
        let sending = tokio::spawn(async move {
            client.write_all(&rest).await.unwrap();
            client
        });

        let expected = [
            b"POST /upload HTTP/1.1\r\nhoSt: medium.com\r\nContent-Length: 1048576\r\n\r\n"
                .as_slice(),
            &body,
            REQUEST_2_RESPELLED,
        ]
        .concat();
        let arrived = read_exactly(&mut relayed.at_server, expected.len()).await;
        assert!(arrived == expected, "the relayed bytes differ");
        relayed.client = within(sending).await.unwrap();
        relayed.forwarded(1).await;
        relayed.closes_both_ways().await;
        let closed = relayed.closed(request.len(), 0, 2);
        assert_eq!(relayed.remaining_events().await, [closed]);
    });
}

#[test]
fn a_pass_through_profile_delivers_host_unchanged_and_reports_nothing_respelled() {
    block_on(async {
        let mut relayed = Relayed::start(HttpMethod::PassThrough, REQUEST_1).await;
        relayed.client.write_all(REQUEST_2).await.unwrap();

        let expected = [REQUEST_1, REQUEST_2].concat();
        assert_eq!(
            read_exactly(&mut relayed.at_server, expected.len()).await,
            expected
        );
        relayed.at_server.write_all(RESPONSE_1).await.unwrap();
        assert_eq!(
            read_exactly(&mut relayed.client, RESPONSE_1.len()).await,
            RESPONSE_1
        );
        relayed.forwarded(1).await;
        relayed.closes_both_ways().await;
        let closed = relayed.closed(expected.len(), RESPONSE_1.len(), 0);
        assert_eq!(relayed.remaining_events().await, [closed]);
    });
}

#[test]
fn a_chunked_request_is_respelled_then_passed_unchanged_and_the_stop_is_reported() {
    block_on(async {
        let request: &[u8] =
            b"POST / HTTP/1.1\r\nHost: medium.com\r\nTransfer-Encoding: chunked\r\n\r\n\
            5\r\nHost:\r\n0\r\n\r\nGET / HTTP/1.1\r\nHost: medium.com\r\n\r\n";
        let mut relayed = Relayed::start(HttpMethod::HostSpell, request).await;

        let expected: &[u8] =
            b"POST / HTTP/1.1\r\nhoSt: medium.com\r\nTransfer-Encoding: chunked\r\n\r\n\
            5\r\nHost:\r\n0\r\n\r\nGET / HTTP/1.1\r\nHost: medium.com\r\n\r\n";
        assert_eq!(
            read_exactly(&mut relayed.at_server, expected.len()).await,
            expected
        );
        relayed.forwarded(1).await;
        relayed.closes_both_ways().await;
        let closed = Event::Closed {
            host: MEDIUM_HOST.to_owned(),
            port: relayed.upstream_port,
            to_server: u64::try_from(request.len()).unwrap(),
            to_client: 0,
            respelled: 1,
            stopped: Some(StopReason::TransferEncoding),
        };
        assert_eq!(relayed.remaining_events().await, [closed]);
    });
}

#[test]
fn an_error_in_either_direction_ends_the_relay_while_the_other_is_still_open() {
    block_on(async {
        let failing = || async { Err(io::Error::other("reset by peer")) };
        let open_forever = std::future::pending::<io::Result<()>>;

        let first_fails = within(until_both_end(failing(), open_forever())).await;
        let second_fails = within(until_both_end(open_forever(), failing())).await;

        assert_eq!(first_fails.unwrap_err().to_string(), "reset by peer");
        assert_eq!(second_fails.unwrap_err().to_string(), "reset by peer");
    });
}
