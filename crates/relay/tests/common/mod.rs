#![allow(dead_code)]

use std::future::Future;
use std::io;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use snisplit_core::domains::DomainList;
use snisplit_core::first_flight::{Protocol, RelayProfile};
use snisplit_core::strategy::SplitPosition;
use snisplit_relay::{Config, Event, Listener, Relay, Resolve};
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio::time::timeout;

pub const STORE_HELLO: &[u8] =
    include_bytes!("../../../core/tests/fixtures/client_hello/store.steampowered.com-4.bin");
pub const API_HELLO: &[u8] =
    include_bytes!("../../../core/tests/fixtures/client_hello/api.steampowered.com-1.bin");
pub const COMMUNITY_HELLO: &[u8] =
    include_bytes!("../../../core/tests/fixtures/client_hello/steamcommunity.com-12.bin");

pub const STORE_HOST: &str = "store.steampowered.com";
pub const LOOPBACK: Ipv4Addr = Ipv4Addr::LOCALHOST;
pub const PATIENCE: Duration = Duration::from_secs(30);
pub const DEFAULT_FIRST_FLIGHT_CAP: usize = 1 << 14;
pub const UNUSED_UPSTREAM_PORT: u16 = 9;

pub fn block_on<F: Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("runtime builds")
        .block_on(future)
}

pub async fn within<T>(future: impl Future<Output = T>) -> T {
    timeout(PATIENCE, future)
        .await
        .expect("operation finished in time")
}

pub enum Answer {
    Addresses(Vec<Ipv4Addr>),
    Failure(&'static str),
}

pub struct FakeResolver {
    pub answer: Answer,
}

impl Resolve for FakeResolver {
    fn resolve(&self, _host: &str) -> impl Future<Output = io::Result<Vec<Ipv4Addr>>> + Send {
        let result = match &self.answer {
            Answer::Addresses(addresses) => Ok(addresses.clone()),
            Answer::Failure(message) => Err(io::Error::other(*message)),
        };
        async move { result }
    }
}

pub fn profile(domains: &[&str]) -> RelayProfile {
    RelayProfile {
        name: "test".to_owned(),
        domains: DomainList::new(domains),
        tls_positions: vec![SplitPosition::Offset(1), SplitPosition::DomainMid],
        tls_ports: vec![443],
        http_ports: vec![80],
        http_method: snisplit_core::first_flight::HttpMethod::HostSpell,
    }
}

pub fn listener(listen_port: u16, upstream_port: u16, protocol: Protocol) -> Listener {
    Listener {
        listen: SocketAddr::from((LOOPBACK, listen_port)),
        upstream_port,
        protocol,
        profile: 0,
    }
}

pub fn config(listeners: Vec<Listener>, domains: &[&str]) -> Config {
    Config {
        listeners,
        profiles: vec![profile(domains)],
        piece_gap: Duration::from_millis(5),
        first_flight_timeout: PATIENCE,
        max_first_flight: DEFAULT_FIRST_FLIGHT_CAP,
        connect_timeout: PATIENCE,
    }
}

pub fn single_tls_config(domains: &[&str]) -> Config {
    config(
        vec![listener(0, UNUSED_UPSTREAM_PORT, Protocol::Tls)],
        domains,
    )
}

pub struct Harness {
    pub addrs: Vec<SocketAddr>,
    pub events: UnboundedReceiver<Event>,
    stop: oneshot::Sender<()>,
    running: JoinHandle<()>,
}

impl Harness {
    pub async fn start(config: Config, answer: Answer) -> Harness {
        let (sender, events) = unbounded_channel();
        let on_event: Arc<dyn Fn(Event) + Send + Sync> = Arc::new(move |event| {
            let _ = sender.send(event);
        });
        let relay = Relay::bind(config, FakeResolver { answer }, on_event)
            .await
            .expect("relay binds");
        let addrs = relay.local_addrs();
        let (stop, stopped) = oneshot::channel::<()>();
        let running = tokio::spawn(relay.run(async move {
            let _ = stopped.await;
        }));
        Harness {
            addrs,
            events,
            stop,
            running,
        }
    }

    pub async fn connect(&self) -> TcpStream {
        self.connect_to(0).await
    }

    pub async fn connect_to(&self, listener: usize) -> TcpStream {
        within(TcpStream::connect(self.addrs[listener]))
            .await
            .expect("client connects")
    }

    pub async fn next_event(&mut self) -> Event {
        within(self.events.recv()).await.expect("an event arrives")
    }

    pub async fn shut_down(self) -> UnboundedReceiver<Event> {
        let _ = self.stop.send(());
        within(self.running).await.expect("run returns");
        self.events
    }
}

pub async fn reaches_end_of_stream(stream: &mut TcpStream) -> bool {
    let mut buffer = [0; 16];
    match within(stream.read(&mut buffer)).await {
        Ok(read) => read == 0,
        Err(_) => true,
    }
}
