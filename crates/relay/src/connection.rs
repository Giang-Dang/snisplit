use std::future::{Future, poll_fn};
use std::io;
use std::mem;
use std::net::{Ipv4Addr, SocketAddr};
use std::pin::{Pin, pin};
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use snisplit_core::first_flight::{Decision, HttpMethod, Protocol, Refusal, RelayProfile, decide};
use snisplit_core::http_stream::HostRespeller;
use tokio::io::{
    AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf, copy, copy_bidirectional,
};
use tokio::net::TcpStream;
use tokio::time::timeout;

use crate::config::Config;
use crate::event::Event;
use crate::pieces::write_pieces;
use crate::resolve::ResolveBoxed;

const READ_CHUNK: usize = 4096;
const COPY_CHUNK: usize = 8192;
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
    let mut flight = match read_first_flight(&mut client, &shared, route).await {
        Ok(flight) => flight,
        Err(stop) => return shared.emit(stop.into_event(port)),
    };
    let mut respeller = respeller_for(&shared.profiles[route.profile], route.protocol);
    if let Some(respeller) = respeller.as_mut() {
        flight.bytes = respelled_first_flight(respeller, &flight.bytes);
    }
    match open_upstream(&shared, port, &flight).await {
        Ok((upstream, pieces)) => {
            shared.emit(Event::Forwarded {
                host: flight.host.clone(),
                port,
                pieces,
            });
            let mut report = CloseReport {
                shared: &shared,
                host: flight.host,
                port,
                to_server: byte_count(flight.bytes.len()),
                to_client: 0,
                respeller,
            };
            report.relay(client, upstream).await;
        }
        Err(stop) => shared.emit(stop.into_event(port)),
    }
}

fn respeller_for(profile: &RelayProfile, protocol: Protocol) -> Option<HostRespeller> {
    let respells = protocol == Protocol::Http && profile.http_method == HttpMethod::HostSpell;
    respells.then(HostRespeller::new)
}

fn respelled_first_flight(respeller: &mut HostRespeller, bytes: &[u8]) -> Vec<u8> {
    let mut respelled = Vec::with_capacity(bytes.len());
    respeller.feed(bytes, &mut respelled);
    respelled
}

fn byte_count(len: usize) -> u64 {
    u64::try_from(len).unwrap_or(u64::MAX)
}

struct CloseReport<'a> {
    shared: &'a Shared,
    host: String,
    port: u16,
    to_server: u64,
    to_client: u64,
    respeller: Option<HostRespeller>,
}

impl CloseReport<'_> {
    async fn relay(&mut self, mut client: TcpStream, mut upstream: TcpStream) {
        match self.respeller.as_mut() {
            Some(respeller) => {
                let (mut client_reader, client_writer) = client.split();
                let (mut upstream_reader, upstream_writer) = upstream.split();
                let mut to_server = Counted::new(upstream_writer, &mut self.to_server);
                let mut to_client = Counted::new(client_writer, &mut self.to_client);
                let requests = copy_respelled(&mut client_reader, &mut to_server, respeller);
                let responses = copy_then_shut_down(&mut upstream_reader, &mut to_client);
                let _ = until_both_end(requests, responses).await;
            }
            None => {
                let mut client = Counted::new(&mut client, &mut self.to_client);
                let mut upstream = Counted::new(&mut upstream, &mut self.to_server);
                let _ = copy_bidirectional(&mut client, &mut upstream).await;
            }
        }
    }
}

impl Drop for CloseReport<'_> {
    fn drop(&mut self) {
        let respeller = self.respeller.as_ref();
        self.shared.emit(Event::Closed {
            host: mem::take(&mut self.host),
            port: self.port,
            to_server: self.to_server,
            to_client: self.to_client,
            respelled: respeller.map_or(0, HostRespeller::respelled),
            stopped: respeller.and_then(HostRespeller::stopped),
        });
    }
}

async fn copy_respelled<R, W>(
    reader: &mut R,
    writer: &mut W,
    respeller: &mut HostRespeller,
) -> io::Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut chunk = vec![0; COPY_CHUNK];
    let mut respelled = Vec::with_capacity(COPY_CHUNK);
    loop {
        let read = reader.read(&mut chunk).await?;
        respelled.clear();
        if read == 0 {
            respeller.finish(&mut respelled);
            writer.write_all(&respelled).await?;
            return writer.shutdown().await;
        }
        respeller.feed(&chunk[..read], &mut respelled);
        writer.write_all(&respelled).await?;
    }
}

async fn copy_then_shut_down<R, W>(reader: &mut R, writer: &mut W) -> io::Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    copy(reader, writer).await?;
    writer.shutdown().await
}

async fn until_both_end(
    first: impl Future<Output = io::Result<()>>,
    second: impl Future<Output = io::Result<()>>,
) -> io::Result<()> {
    let mut first = pin!(first);
    let mut second = pin!(second);
    let mut first_ended = false;
    let mut second_ended = false;
    poll_fn(|context| {
        if !first_ended {
            match first.as_mut().poll(context) {
                Poll::Ready(Ok(())) => first_ended = true,
                Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
                Poll::Pending => {}
            }
        }
        if !second_ended {
            match second.as_mut().poll(context) {
                Poll::Ready(Ok(())) => second_ended = true,
                Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
                Poll::Pending => {}
            }
        }
        if first_ended && second_ended {
            Poll::Ready(Ok(()))
        } else {
            Poll::Pending
        }
    })
    .await
}

struct Counted<'a, S> {
    stream: S,
    written: &'a mut u64,
}

impl<'a, S> Counted<'a, S> {
    fn new(stream: S, written: &'a mut u64) -> Self {
        Self { stream, written }
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for Counted<'_, S> {
    fn poll_read(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().stream).poll_read(context, buffer)
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for Counted<'_, S> {
    fn poll_write(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        let written = Pin::new(&mut this.stream).poll_write(context, bytes);
        if let Poll::Ready(Ok(len)) = written {
            *this.written += byte_count(len);
        }
        written
    }

    fn poll_flush(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().stream).poll_flush(context)
    }

    fn poll_shutdown(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().stream).poll_shutdown(context)
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
