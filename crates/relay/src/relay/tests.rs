use std::future::Future;
use std::io;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use snisplit_core::domains::DomainList;
use snisplit_core::first_flight::{Protocol, RelayProfile};
use snisplit_core::strategy::SplitPosition;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio::time::{sleep, timeout};

use super::*;
use crate::config::Listener;

const STORE_HELLO: &[u8] =
    include_bytes!("../../../core/tests/fixtures/client_hello/store.steampowered.com-4.bin");
const STORE_FIRST_PACKET_LEN: usize = 1452;
const STORE_HOST: &str = "store.steampowered.com";
const STORE_PIECES: usize = 3;

const PATIENCE: Duration = Duration::from_secs(30);
const LET_RELAY_READ_THE_FIRST_PART: Duration = Duration::from_millis(50);
const PIECE_GAP: Duration = Duration::from_millis(5);

const LOOPBACK: Ipv4Addr = Ipv4Addr::LOCALHOST;
const NOT_LISTENING_ON_LOOPBACK: Ipv4Addr = Ipv4Addr::new(127, 0, 0, 2);

const SERVER_WORDS: &[u8] = b"server says hello";
const CLIENT_FOLLOW_UP: &[u8] = b"application data after the hello";
const HTTP_REQUEST: &[u8] =
    b"GET /app/220 HTTP/1.1\r\nHost: Store.SteamPowered.com:80\r\nAccept: */*\r\n\r\n";

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

fn allow_everything(_address: Ipv4Addr) -> bool {
    true
}

struct FakeResolver {
    answers: Vec<Ipv4Addr>,
    asked: Arc<Mutex<Vec<String>>>,
}

impl Resolve for FakeResolver {
    fn resolve(&self, host: &str) -> impl Future<Output = io::Result<Vec<Ipv4Addr>>> + Send {
        self.asked.lock().unwrap().push(host.to_owned());
        let answers = self.answers.clone();
        async move { Ok(answers) }
    }
}

fn steam_profile() -> RelayProfile {
    RelayProfile {
        name: "steam".to_owned(),
        domains: DomainList::new(["steampowered.com"]),
        tls_positions: vec![SplitPosition::Offset(1), SplitPosition::DomainMid],
        tls_ports: vec![443],
        http_ports: vec![80],
    }
}

fn config(protocol: Protocol, upstream_port: u16) -> Config {
    Config {
        listeners: vec![Listener {
            listen: SocketAddr::from((LOOPBACK, 0)),
            upstream_port,
            protocol,
            profile: 0,
        }],
        profiles: vec![steam_profile()],
        piece_gap: PIECE_GAP,
        first_flight_timeout: PATIENCE,
        max_first_flight: 1 << 14,
        connect_timeout: PATIENCE,
    }
}

struct Harness {
    relay_addr: SocketAddr,
    events: UnboundedReceiver<Event>,
    asked: Arc<Mutex<Vec<String>>>,
    stop: oneshot::Sender<()>,
    running: JoinHandle<()>,
}

impl Harness {
    async fn start(protocol: Protocol, upstream_port: u16, answers: Vec<Ipv4Addr>) -> Harness {
        let (sender, events) = unbounded_channel();
        let asked = Arc::new(Mutex::new(Vec::new()));
        let resolver = FakeResolver {
            answers,
            asked: Arc::clone(&asked),
        };
        let on_event: EventSink = Arc::new(move |event| {
            let _ = sender.send(event);
        });
        let relay = Relay::bind_with(
            config(protocol, upstream_port),
            resolver,
            on_event,
            allow_everything,
        )
        .await
        .expect("relay binds");
        let relay_addr = relay.local_addrs()[0];
        let (stop, stopped) = oneshot::channel::<()>();
        let running = tokio::spawn(relay.run(async move {
            let _ = stopped.await;
        }));
        Harness {
            relay_addr,
            events,
            asked,
            stop,
            running,
        }
    }

    async fn next_event(&mut self) -> Event {
        within(self.events.recv()).await.expect("an event arrives")
    }

    async fn connect(&self) -> TcpStream {
        within(TcpStream::connect(self.relay_addr))
            .await
            .expect("client connects")
    }

    async fn shut_down(self) -> UnboundedReceiver<Event> {
        let _ = self.stop.send(());
        within(self.running).await.expect("run returns");
        self.events
    }
}

async fn fake_upstream() -> (TcpListener, u16) {
    let listener = TcpListener::bind((LOOPBACK, 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    (listener, port)
}

fn closed_port() -> u16 {
    let listener = std::net::TcpListener::bind((LOOPBACK, 0)).unwrap();
    listener.local_addr().unwrap().port()
}

async fn read_exactly(stream: &mut TcpStream, len: usize) -> Vec<u8> {
    let mut bytes = vec![0; len];
    within(stream.read_exact(&mut bytes)).await.unwrap();
    bytes
}

async fn reaches_end_of_stream(stream: &mut TcpStream) -> bool {
    let mut buffer = [0; 16];
    match within(stream.read(&mut buffer)).await {
        Ok(read) => read == 0,
        Err(_) => true,
    }
}

#[test]
fn a_real_client_hello_reaches_the_upstream_unchanged_in_three_pieces_and_the_answer_comes_back() {
    block_on(async {
        let (upstream, upstream_port) = fake_upstream().await;
        let mut harness = Harness::start(Protocol::Tls, upstream_port, vec![LOOPBACK]).await;
        let mut client = harness.connect().await;
        client.write_all(STORE_HELLO).await.unwrap();
        let (mut at_server, _) = within(upstream.accept()).await.unwrap();

        assert_eq!(
            read_exactly(&mut at_server, STORE_HELLO.len()).await,
            STORE_HELLO
        );
        assert_eq!(
            harness.next_event().await,
            Event::Forwarded {
                host: STORE_HOST.to_owned(),
                port: upstream_port,
                pieces: STORE_PIECES,
            }
        );
        assert_eq!(*harness.asked.lock().unwrap(), [STORE_HOST]);

        at_server.write_all(SERVER_WORDS).await.unwrap();
        assert_eq!(
            read_exactly(&mut client, SERVER_WORDS.len()).await,
            SERVER_WORDS
        );
        client.write_all(CLIENT_FOLLOW_UP).await.unwrap();
        assert_eq!(
            read_exactly(&mut at_server, CLIENT_FOLLOW_UP.len()).await,
            CLIENT_FOLLOW_UP
        );
        harness.shut_down().await;
    });
}

#[test]
fn a_hello_arriving_in_two_parts_is_reassembled_before_forwarding() {
    block_on(async {
        let (upstream, upstream_port) = fake_upstream().await;
        let mut harness = Harness::start(Protocol::Tls, upstream_port, vec![LOOPBACK]).await;
        let mut client = harness.connect().await;
        client
            .write_all(&STORE_HELLO[..STORE_FIRST_PACKET_LEN])
            .await
            .unwrap();
        sleep(LET_RELAY_READ_THE_FIRST_PART).await;
        client
            .write_all(&STORE_HELLO[STORE_FIRST_PACKET_LEN..])
            .await
            .unwrap();
        let (mut at_server, _) = within(upstream.accept()).await.unwrap();

        assert_eq!(
            read_exactly(&mut at_server, STORE_HELLO.len()).await,
            STORE_HELLO
        );
        assert_eq!(
            harness.next_event().await,
            Event::Forwarded {
                host: STORE_HOST.to_owned(),
                port: upstream_port,
                pieces: STORE_PIECES,
            }
        );
        harness.shut_down().await;
    });
}

#[test]
fn an_http_request_is_relayed_unchanged_in_one_piece_by_its_lowercased_host() {
    block_on(async {
        let (upstream, upstream_port) = fake_upstream().await;
        let mut harness = Harness::start(Protocol::Http, upstream_port, vec![LOOPBACK]).await;
        let mut client = harness.connect().await;
        client.write_all(HTTP_REQUEST).await.unwrap();
        let (mut at_server, _) = within(upstream.accept()).await.unwrap();

        assert_eq!(
            read_exactly(&mut at_server, HTTP_REQUEST.len()).await,
            HTTP_REQUEST
        );
        assert_eq!(
            harness.next_event().await,
            Event::Forwarded {
                host: STORE_HOST.to_owned(),
                port: upstream_port,
                pieces: 1,
            }
        );
        assert_eq!(*harness.asked.lock().unwrap(), [STORE_HOST]);
        at_server.write_all(SERVER_WORDS).await.unwrap();
        assert_eq!(
            read_exactly(&mut client, SERVER_WORDS.len()).await,
            SERVER_WORDS
        );
        harness.shut_down().await;
    });
}

#[test]
fn the_next_address_is_tried_when_the_first_cannot_be_reached() {
    block_on(async {
        let (upstream, upstream_port) = fake_upstream().await;
        let answers = vec![NOT_LISTENING_ON_LOOPBACK, LOOPBACK];
        let mut harness = Harness::start(Protocol::Tls, upstream_port, answers).await;
        let mut client = harness.connect().await;
        client.write_all(STORE_HELLO).await.unwrap();
        let (mut at_server, _) = within(upstream.accept()).await.unwrap();

        assert_eq!(
            read_exactly(&mut at_server, STORE_HELLO.len()).await,
            STORE_HELLO
        );
        assert!(matches!(
            harness.next_event().await,
            Event::Forwarded {
                pieces: STORE_PIECES,
                ..
            }
        ));
        harness.shut_down().await;
    });
}

#[test]
fn an_unreachable_upstream_is_a_failure_that_names_the_host_and_closes_the_client() {
    block_on(async {
        let port = closed_port();
        let mut harness = Harness::start(Protocol::Tls, port, vec![LOOPBACK]).await;
        let mut client = harness.connect().await;
        client.write_all(STORE_HELLO).await.unwrap();

        match harness.next_event().await {
            Event::Failed {
                host,
                port: event_port,
                error,
            } => {
                assert_eq!(host, STORE_HOST);
                assert_eq!(event_port, port);
                assert!(error.contains(&port.to_string()), "{error}");
            }
            other => panic!("expected a failure, got {other:?}"),
        }
        assert!(reaches_end_of_stream(&mut client).await);
        harness.shut_down().await;
    });
}

#[test]
fn shutdown_closes_connections_that_are_in_flight() {
    block_on(async {
        let (upstream, upstream_port) = fake_upstream().await;
        let mut harness = Harness::start(Protocol::Tls, upstream_port, vec![LOOPBACK]).await;
        let mut client = harness.connect().await;
        client.write_all(STORE_HELLO).await.unwrap();
        let (mut at_server, _) = within(upstream.accept()).await.unwrap();
        read_exactly(&mut at_server, STORE_HELLO.len()).await;
        assert!(matches!(
            harness.next_event().await,
            Event::Forwarded { .. }
        ));

        let mut events = harness.shut_down().await;

        assert!(reaches_end_of_stream(&mut client).await);
        assert!(reaches_end_of_stream(&mut at_server).await);
        assert!(events.try_recv().is_err());
    });
}

#[test]
fn a_connection_that_ends_normally_emits_nothing_after_forwarded() {
    block_on(async {
        let (upstream, upstream_port) = fake_upstream().await;
        let mut harness = Harness::start(Protocol::Tls, upstream_port, vec![LOOPBACK]).await;
        let mut client = harness.connect().await;
        client.write_all(STORE_HELLO).await.unwrap();
        let (mut at_server, _) = within(upstream.accept()).await.unwrap();
        read_exactly(&mut at_server, STORE_HELLO.len()).await;
        assert!(matches!(
            harness.next_event().await,
            Event::Forwarded { .. }
        ));

        client.shutdown().await.unwrap();
        assert!(reaches_end_of_stream(&mut at_server).await);
        at_server.shutdown().await.unwrap();
        assert!(reaches_end_of_stream(&mut client).await);

        let mut events = harness.shut_down().await;
        assert!(events.try_recv().is_err());
    });
}
