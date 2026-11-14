use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use snisplit_core::first_flight::{Decision, Protocol, Refusal, RelayProfile, decide};
use tokio::io::{AsyncReadExt, copy_bidirectional};
use tokio::net::TcpStream;
use tokio::time::timeout;

use crate::config::Config;
use crate::event::Event;
use crate::pieces::write_pieces;
use crate::resolve::ResolveBoxed;

const READ_CHUNK: usize = 4096;
const FIRST_FLIGHT_TIMED_OUT: &str = "first flight timed out";
const CLIENT_CLOSED_EARLY: &str = "client closed before the first flight was complete";
const RESOLVE_TIMED_OUT: &str = "resolving timed out";
const CONNECT_TIMED_OUT: &str = "connecting timed out";
const NO_ADDRESS: &str = "no address";
const NO_USABLE_ADDRESS: &str =
    "no usable address: every answer was loopback, unspecified or broadcast";
const UNKNOWN_HOST: &str = "";

pub(crate) type EventSink = Arc<dyn Fn(Event) + Send + Sync>;

pub(crate) struct Shared {
    profiles: Vec<RelayProfile>,
    piece_gap: Duration,
    first_flight_timeout: Duration,
    max_first_flight: usize,
    connect_timeout: Duration,
    resolver: Box<dyn ResolveBoxed>,
    on_event: EventSink,
    is_usable: fn(Ipv4Addr) -> bool,
}

impl Shared {
    pub fn new(
        config: Config,
        resolver: Box<dyn ResolveBoxed>,
        on_event: EventSink,
        is_usable: fn(Ipv4Addr) -> bool,
    ) -> Self {
        Self {
            profiles: config.profiles,
            piece_gap: config.piece_gap,
            first_flight_timeout: config.first_flight_timeout,
            max_first_flight: config.max_first_flight,
            connect_timeout: config.connect_timeout,
            resolver,
            on_event,
            is_usable,
        }
    }

    pub fn emit(&self, event: Event) {
        (self.on_event)(event);
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Route {
    pub protocol: Protocol,
    pub profile: usize,
    pub upstream_port: u16,
}

struct FirstFlight {
    host: String,
    cuts: Vec<usize>,
    bytes: Vec<u8>,
}

enum Stop {
    Refused(Refusal),
    Failed { host: String, error: String },
}

impl Stop {
    fn failed(host: &str, error: impl Into<String>) -> Self {
        Self::Failed {
            host: host.to_owned(),
            error: error.into(),
        }
    }

    fn into_event(self, port: u16) -> Event {
        match self {
            Self::Refused(refusal) => Event::Refused { port, refusal },
            Self::Failed { host, error } => Event::Failed { host, port, error },
        }
    }
}

pub(crate) async fn serve(mut client: TcpStream, shared: Arc<Shared>, route: Route) {
    let port = route.upstream_port;
    let flight = match read_first_flight(&mut client, &shared, route).await {
        Ok(flight) => flight,
        Err(stop) => return shared.emit(stop.into_event(port)),
    };
    match open_upstream(&shared, port, &flight).await {
        Ok((mut upstream, pieces)) => {
            shared.emit(Event::Forwarded {
                host: flight.host,
                port,
                pieces,
            });
            let _ = copy_bidirectional(&mut client, &mut upstream).await;
        }
        Err(stop) => shared.emit(stop.into_event(port)),
    }
}

async fn read_first_flight(
    client: &mut TcpStream,
    shared: &Shared,
    route: Route,
) -> Result<FirstFlight, Stop> {
    let collecting = collect_first_flight(client, shared, route);
    timeout(shared.first_flight_timeout, collecting)
        .await
        .unwrap_or_else(|_| Err(Stop::failed(UNKNOWN_HOST, FIRST_FLIGHT_TIMED_OUT)))
}

async fn collect_first_flight(
    client: &mut TcpStream,
    shared: &Shared,
    route: Route,
) -> Result<FirstFlight, Stop> {
    let profile = &shared.profiles[route.profile];
    let mut bytes = Vec::new();
    loop {
        match decide(profile, route.protocol, &bytes, shared.max_first_flight) {
            Decision::Forward { host, cuts } => return Ok(FirstFlight { host, cuts, bytes }),
            Decision::Refuse(refusal) => return Err(Stop::Refused(refusal)),
            Decision::NeedMoreData => {
                read_more(client, &mut bytes, shared.max_first_flight).await?;
            }
        }
    }
}

async fn read_more(
    client: &mut TcpStream,
    bytes: &mut Vec<u8>,
    max_first_flight: usize,
) -> Result<(), Stop> {
    let room = max_first_flight.saturating_sub(bytes.len()).min(READ_CHUNK);
    let mut chunk = vec![0; room];
    let read = client.read(&mut chunk).await.map_err(|error| {
        Stop::failed(UNKNOWN_HOST, format!("reading the first flight: {error}"))
    })?;
    if read == 0 {
        return Err(Stop::failed(UNKNOWN_HOST, CLIENT_CLOSED_EARLY));
    }
    bytes.extend_from_slice(&chunk[..read]);
    Ok(())
}

async fn open_upstream(
    shared: &Shared,
    port: u16,
    flight: &FirstFlight,
) -> Result<(TcpStream, usize), Stop> {
    let host = flight.host.as_str();
    let fail = |error: String| Stop::failed(host, error);
    let addresses = usable_addresses(shared, host).await.map_err(fail)?;
    let candidates: Vec<SocketAddr> = addresses
        .iter()
        .map(|&address| SocketAddr::from((address, port)))
        .collect();
    let mut upstream = connect_first(&candidates, shared.connect_timeout)
        .await
        .map_err(fail)?;
    upstream
        .set_nodelay(true)
        .map_err(|error| fail(format!("setting TCP_NODELAY: {error}")))?;
    let pieces = write_pieces(&mut upstream, &flight.bytes, &flight.cuts, shared.piece_gap)
        .await
        .map_err(|error| fail(format!("writing the first flight: {error}")))?;
    Ok((upstream, pieces))
}

async fn usable_addresses(shared: &Shared, host: &str) -> Result<Vec<Ipv4Addr>, String> {
    let answers = timeout(shared.connect_timeout, shared.resolver.resolve_boxed(host))
        .await
        .map_err(|_| RESOLVE_TIMED_OUT.to_owned())?
        .map_err(|error| format!("resolving {host}: {error}"))?;
    if answers.is_empty() {
        return Err(NO_ADDRESS.to_owned());
    }
    let usable: Vec<Ipv4Addr> = answers
        .into_iter()
        .filter(|&address| (shared.is_usable)(address))
        .collect();
    if usable.is_empty() {
        return Err(NO_USABLE_ADDRESS.to_owned());
    }
    Ok(usable)
}

async fn connect_first(
    candidates: &[SocketAddr],
    connect_timeout: Duration,
) -> Result<TcpStream, String> {
    timeout(connect_timeout, connect_in_order(candidates))
        .await
        .unwrap_or_else(|_| Err(CONNECT_TIMED_OUT.to_owned()))
}

async fn connect_in_order(candidates: &[SocketAddr]) -> Result<TcpStream, String> {
    let mut last_error = NO_ADDRESS.to_owned();
    for &candidate in candidates {
        match TcpStream::connect(candidate).await {
            Ok(stream) => return Ok(stream),
            Err(error) => last_error = format!("connecting to {candidate}: {error}"),
        }
    }
    Err(last_error)
}

#[cfg(test)]
mod tests;
