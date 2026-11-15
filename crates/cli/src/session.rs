use std::fmt;
use std::future::{Future, poll_fn};
use std::io::{self, Write};
use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::pin::{Pin, pin};
use std::sync::Arc;
use std::task::Poll;
use std::time::Duration;

use snisplit_core::first_flight::Refusal;
use snisplit_core::http_stream::StopReason;
use snisplit_relay::dns_server::DnsServer;
use snisplit_relay::upstream::UdpResolver;
use snisplit_relay::{Event, Relay, Upstreams};
use tokio::sync::watch;
use tokio::task::{JoinError, JoinSet, spawn_blocking};

use crate::VERSION;
use crate::plan::{self, Selected};
use crate::profile_file::ProfileSpec;
use crate::routing::{self, ProfileRule, StartFailure};
use crate::{profile, sites, system_dns};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOptions {
    pub verbose: bool,
    pub profiles: Vec<&'static str>,
    pub profile_files: Vec<PathBuf>,
    pub site_folder: Option<PathBuf>,
    pub routing: bool,
    pub upstreams: Vec<Ipv4Addr>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    Binding,
    ChangingRules,
    RemovingRules,
    NothingSwitchedOn,
    Other,
}

const NO_UPSTREAM: &str = "no usable upstream DNS server found; pass --upstream <ipv4>";
const NOTHING_SWITCHED_ON: &str = "every site is switched off: turn one on in snisplit-gui or take its name out of off.list in the snisplit folder, or choose sites with --profile or --profile-file";
const TIMING: Timing = Timing {
    refresh_every: Duration::from_secs(30),
};

pub type Output = Arc<dyn Fn(&str) + Send + Sync>;

pub fn stdout() -> Output {
    Arc::new(|line| {
        let mut stdout = io::stdout().lock();
        let _ = writeln!(stdout, "{line}");
    })
}

#[cfg(windows)]
pub fn stop_signals() -> io::Result<impl Future<Output = ()> + Send> {
    use tokio::signal::windows::{ctrl_break, ctrl_c, ctrl_close, ctrl_logoff, ctrl_shutdown};

    let mut interrupt = ctrl_c()?;
    let mut break_key = ctrl_break()?;
    let mut window_closed = ctrl_close()?;
    let mut logoff = ctrl_logoff()?;
    let mut shutdown = ctrl_shutdown()?;
    Ok(poll_fn(move |context| {
        let signalled = [
            interrupt.poll_recv(context).is_ready(),
            break_key.poll_recv(context).is_ready(),
            window_closed.poll_recv(context).is_ready(),
            logoff.poll_recv(context).is_ready(),
            shutdown.poll_recv(context).is_ready(),
        ];
        if signalled.contains(&true) {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }))
}

#[cfg(not(windows))]
pub fn stop_signals() -> io::Result<impl Future<Output = ()> + Send> {
    use tokio::signal::unix::{SignalKind, signal};

    let mut interrupt = signal(SignalKind::interrupt())?;
    let mut terminate = signal(SignalKind::terminate())?;
    let mut hangup = signal(SignalKind::hangup())?;
    Ok(poll_fn(move |context| {
        let signalled = [
            interrupt.poll_recv(context).is_ready(),
            terminate.poll_recv(context).is_ready(),
            hangup.poll_recv(context).is_ready(),
        ];
        if signalled.contains(&true) {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }))
}

struct Timing {
    refresh_every: Duration,
}

trait Host: Clone + Send + Sync + 'static {
    type Start: StartRules;

    fn dns_servers(&self) -> io::Result<Vec<Ipv4Addr>>;
    fn begin_rules(&self, rules: &[ProfileRule]) -> io::Result<Self::Start>;
    fn remove_rules(&self) -> io::Result<usize>;
}

trait StartRules: Send + 'static {
    fn add(self) -> Result<(), StartFailure>;
    fn abandon(self);
}

#[derive(Debug, Clone, Copy)]
struct System;

impl Host for System {
    type Start = routing::PendingStart;

    fn dns_servers(&self) -> io::Result<Vec<Ipv4Addr>> {
        system_dns::servers()
    }

    fn begin_rules(&self, rules: &[ProfileRule]) -> io::Result<Self::Start> {
        routing::begin_start(rules)
    }

    fn remove_rules(&self) -> io::Result<usize> {
        routing::remove_all()
    }
}

impl StartRules for routing::PendingStart {
    fn add(self) -> Result<(), StartFailure> {
        routing::PendingStart::add(self)
    }

    fn abandon(self) {
        let _ = routing::PendingStart::abandon(self);
    }
}

enum Raced<T> {
    Done(T),
    Stopped,
}

async fn until_stopped<T>(
    work: impl Future<Output = T>,
    mut stop: Pin<&mut impl Future<Output = ()>>,
) -> Raced<T> {
    let mut work = pin!(work);
    poll_fn(|context| {
        if stop.as_mut().poll(context).is_ready() {
            return Poll::Ready(Raced::Stopped);
        }
        work.as_mut().poll(context).map(Raced::Done)
    })
    .await
}

struct Bound {
    relay: Relay,
    dns_servers: Vec<DnsServer>,
    upstreams: Upstreams,
    first_upstreams: Vec<SocketAddr>,
}

pub async fn serve(
    options: RunOptions,
    stop_requested: impl Future<Output = ()> + Send,
    out: Output,
) -> io::Result<()> {
    serve_on(System, &TIMING, options, stop_requested, out, || {}).await
}

pub async fn serve_with_ready(
    options: RunOptions,
    stop_requested: impl Future<Output = ()> + Send,
    out: Output,
    ready: impl FnOnce() + Send,
) -> io::Result<()> {
    serve_on(System, &TIMING, options, stop_requested, out, ready).await
}

pub fn routing_failure_cause(error: &io::Error) -> Option<&str> {
    error
        .get_ref()?
        .downcast_ref::<RoutingFailure>()
        .map(|failure| failure.cause.as_str())
}

pub fn failure(error: &io::Error) -> Failure {
    let Some(inner) = error.get_ref() else {
        return Failure::Other;
    };
    if inner.downcast_ref::<RoutingFailure>().is_some() {
        return Failure::ChangingRules;
    }
    inner
        .downcast_ref::<Tagged>()
        .map_or(Failure::Other, |tagged| tagged.failure)
}

pub fn remove_rules(out: &Output) -> io::Result<()> {
    remove_rules_with(&System, out)
}

async fn serve_on<H: Host>(
    host: H,
    timing: &Timing,
    options: RunOptions,
    stop_requested: impl Future<Output = ()> + Send,
    out: Output,
    ready: impl FnOnce() + Send,
) -> io::Result<()> {
    let selected = load_selection(&options, &out)?;
    let mut stop = pin!(stop_requested);
    let pending = if options.routing {
        Some(host.begin_rules(&profile_rules(&selected))?)
    } else {
        None
    };
    let session = Session {
        host,
        timing,
        options: &options,
        selected: &selected,
        out: &out,
    };
    match session.start_up(pending, stop.as_mut()).await {
        Ok(Raced::Done(bound)) => session.relay_until_stopped(bound, stop, ready).await,
        Ok(Raced::Stopped) => {
            out("stopping");
            session.remove_rules_if_routed().await
        }
        Err(error) => match session.remove_rules_if_routed().await {
            Ok(()) => Err(error),
            Err(removal) => {
                out(&format!("snisplit could not start: {error}"));
                Err(removal)
            }
        },
    }
}

struct Session<'a, H> {
    host: H,
    timing: &'a Timing,
    options: &'a RunOptions,
    selected: &'a [Selected],
    out: &'a Output,
}

impl<H: Host> Session<'_, H> {
    async fn start_up(
        &self,
        pending: Option<H::Start>,
        mut stop: Pin<&mut impl Future<Output = ()>>,
    ) -> io::Result<Raced<Bound>> {
        let bound = match until_stopped(self.find_and_bind(), stop.as_mut()).await {
            Raced::Done(Ok(bound)) => bound,
            Raced::Done(Err(error)) => {
                abandon(pending).await;
                return Err(error);
            }
            Raced::Stopped => {
                abandon(pending).await;
                return Ok(Raced::Stopped);
            }
        };
        let Some(pending) = pending else {
            return Ok(Raced::Done(bound));
        };
        let mut adding = spawn_blocking(move || pending.add());
        match until_stopped(&mut adding, stop).await {
            Raced::Done(added) => {
                added.map_err(join_failure)?.map_err(start_failure)?;
                Ok(Raced::Done(bound))
            }
            Raced::Stopped => {
                let _ = adding.await;
                Ok(Raced::Stopped)
            }
        }
    }

    async fn find_and_bind(&self) -> io::Result<Bound> {
        let upstreams = self.find_upstreams().await?;
        bind(self.selected, upstreams, self.options.verbose, self.out).await
    }

    async fn find_upstreams(&self) -> io::Result<Vec<SocketAddr>> {
        if !self.options.upstreams.is_empty() {
            return non_empty(plan::upstream_servers(&self.options.upstreams, &[]));
        }
        non_empty(plan::upstream_servers(
            &[],
            &read_system_dns(&self.host).await?,
        ))
    }

    async fn relay_until_stopped(
        &self,
        bound: Bound,
        stop: Pin<&mut impl Future<Output = ()>>,
        ready: impl FnOnce(),
    ) -> io::Result<()> {
        (self.out)(&describe_start(
            self.selected,
            &bound.first_upstreams,
            self.options.routing,
        ));
        ready();
        let (stop_tasks, stopped) = watch::channel(false);
        let mut tasks = JoinSet::new();
        tasks.spawn(bound.relay.run(wait_for_stop(stopped.clone())));
        for server in bound.dns_servers {
            tasks.spawn(server.run(wait_for_stop(stopped.clone())));
        }
        if self.options.upstreams.is_empty() {
            tasks.spawn(refresh_upstreams(
                self.host.clone(),
                bound.upstreams,
                self.timing.refresh_every,
                Arc::clone(self.out),
                stopped,
            ));
        }
        stop.await;
        (self.out)("stopping");
        let rules_removed = self.remove_rules_if_routed().await;
        let _ = stop_tasks.send(true);
        while tasks.join_next().await.is_some() {}
        rules_removed
    }

    async fn remove_rules_if_routed(&self) -> io::Result<()> {
        if !self.options.routing {
            return Ok(());
        }
        let host = self.host.clone();
        let out = Arc::clone(self.out);
        spawn_blocking(move || remove_rules_with(&host, &out))
            .await
            .map_err(join_failure)
            .and_then(|removed| removed)
            .map_err(|error| tagged(Failure::RemovingRules, &error))
    }
}

async fn abandon<S: StartRules>(pending: Option<S>) {
    if let Some(pending) = pending {
        let _ = spawn_blocking(move || pending.abandon()).await;
    }
}

fn remove_rules_with<H: Host>(host: &H, out: &Output) -> io::Result<()> {
    let left = host.remove_rules()?;
    out(&format!("snisplit DNS rules left: {left}"));
    if left > 0 {
        let remaining = io::Error::other(format!(
            "snisplit DNS rules still in place after removing them: {left}"
        ));
        return Err(tagged(Failure::RemovingRules, &remaining));
    }
    Ok(())
}

async fn read_system_dns<H: Host>(host: &H) -> io::Result<Vec<Ipv4Addr>> {
    let host = host.clone();
    spawn_blocking(move || host.dns_servers())
        .await
        .map_err(join_failure)?
}

async fn refresh_upstreams<H: Host>(
    host: H,
    upstreams: Upstreams,
    every: Duration,
    out: Output,
    stopped: watch::Receiver<bool>,
) {
    let mut stop = pin!(wait_for_stop(stopped));
    loop {
        if let Raced::Stopped = until_stopped(tokio::time::sleep(every), stop.as_mut()).await {
            return;
        }
        let read = match until_stopped(read_system_dns(&host), stop.as_mut()).await {
            Raced::Done(read) => read,
            Raced::Stopped => return,
        };
        let Some(servers) = read.ok().and_then(|system| refreshed_upstreams(&system)) else {
            continue;
        };
        if upstreams.replace(servers.clone()) {
            out(&describe_upstream_change(&servers));
        }
    }
}

fn refreshed_upstreams(system: &[Ipv4Addr]) -> Option<Vec<SocketAddr>> {
    Some(plan::upstream_servers(&[], system)).filter(|servers| !servers.is_empty())
}

fn non_empty(servers: Vec<SocketAddr>) -> io::Result<Vec<SocketAddr>> {
    if servers.is_empty() {
        return Err(io::Error::other(NO_UPSTREAM));
    }
    Ok(servers)
}

fn join_failure(error: JoinError) -> io::Error {
    io::Error::other(error)
}

fn start_failure(failure: StartFailure) -> io::Error {
    let (cause, failed) = match failure {
        StartFailure::RemovingStale(error) => (first_line(&error), error),
        StartFailure::Adding(error) => (first_line(&error), routing_failure(&error)),
    };
    let reported = RoutingFailure {
        message: failed.to_string(),
        cause,
    };
    io::Error::new(failed.kind(), reported)
}

fn first_line(error: &io::Error) -> String {
    let message = error.to_string();
    message.lines().next().unwrap_or_default().to_string()
}

#[derive(Debug)]
struct RoutingFailure {
    message: String,
    cause: String,
}

impl fmt::Display for RoutingFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for RoutingFailure {}

#[derive(Debug)]
struct Tagged {
    failure: Failure,
    message: String,
}

impl fmt::Display for Tagged {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Tagged {}

fn tagged(failure: Failure, error: &io::Error) -> io::Error {
    io::Error::new(
        error.kind(),
        Tagged {
            failure,
            message: error.to_string(),
        },
    )
}

fn load_selection(options: &RunOptions, out: &Output) -> io::Result<Vec<Selected>> {
    let mut files = options
        .profile_files
        .iter()
        .map(|path| profile::read_file(path))
        .collect::<io::Result<Vec<_>>>()?;
    let mut built_ins = options.profiles.clone();
    if let Some(folder) = &options.site_folder {
        let sites = load_sites(&options.profiles, &files, folder, out)?;
        let off = read_switched_off(folder, out);
        let is_on = |name: &str| !off.iter().any(|off| off == name);
        let named_off: Vec<&str> = built_ins
            .iter()
            .copied()
            .chain(sites.iter().map(|site| site.name.as_str()))
            .filter(|name| !is_on(name))
            .collect();
        if !named_off.is_empty() {
            out(&format!("switched off: {}", named_off.join(", ")));
        }
        built_ins.retain(|name| is_on(name));
        files.extend(sites.into_iter().filter(|site| is_on(&site.name)));
    }
    let selected = plan::select(&built_ins, files).map_err(io::Error::other)?;
    if selected.is_empty() {
        return Err(tagged(
            Failure::NothingSwitchedOn,
            &io::Error::new(io::ErrorKind::InvalidInput, NOTHING_SWITCHED_ON),
        ));
    }
    Ok(selected)
}

fn read_switched_off(folder: &Path, out: &Output) -> Vec<String> {
    sites::switched_off(folder).unwrap_or_else(|error| {
        out(&format!("off.list not read, so every site is on: {error}"));
        Vec::new()
    })
}

fn load_sites(
    built_in_names: &[&str],
    files: &[ProfileSpec],
    folder: &Path,
    out: &Output,
) -> io::Result<Vec<ProfileSpec>> {
    let site_files = match sites::list(folder) {
        Ok(site_files) => site_files,
        Err(error) => {
            out(&format!(
                "user sites not loaded: cannot read the site folder: {error}"
            ));
            return Ok(Vec::new());
        }
    };
    let mut taken = Vec::new();
    for name in built_in_names {
        if let Some(built_in) = profile::find(name) {
            taken.push(built_in.spec().map_err(io::Error::other)?);
        }
    }
    taken.extend_from_slice(files);
    let admitted = sites::admit(&taken, site_files);
    for note in &admitted.notes {
        out(note);
    }
    Ok(admitted.sites)
}

fn profile_rules(selected: &[Selected]) -> Vec<ProfileRule<'_>> {
    selected
        .iter()
        .map(|selection| ProfileRule {
            suffixes: selection
                .profile
                .domains
                .iter()
                .map(String::as_str)
                .collect(),
            server: selection.address,
        })
        .collect()
}

async fn bind(
    selected: &[Selected],
    first_upstreams: Vec<SocketAddr>,
    verbose: bool,
    out: &Output,
) -> io::Result<Bound> {
    let upstreams = Upstreams::new(first_upstreams.clone());
    let resolver = Arc::new(UdpResolver::new(upstreams.clone(), plan::UPSTREAM_TIMEOUT));
    let relay = Relay::bind(
        plan::relay_config(selected),
        Arc::clone(&resolver),
        event_printer(verbose, Arc::clone(out)),
    )
    .await
    .map_err(|error| tagged(Failure::Binding, &error))?;
    let mut dns_servers = Vec::new();
    for selection in selected {
        let listen = SocketAddr::from((selection.address, plan::DNS_PORT));
        let profiles = vec![selection.profile.relay_profile()];
        let server = DnsServer::bind(
            listen,
            profiles,
            selection.address,
            upstreams.clone(),
            Arc::clone(&resolver),
        )
        .await
        .map_err(|error| tagged(Failure::Binding, &error))?;
        dns_servers.push(server);
    }
    Ok(Bound {
        relay,
        dns_servers,
        upstreams,
        first_upstreams,
    })
}

async fn wait_for_stop(mut stopped: watch::Receiver<bool>) {
    let _ = stopped.wait_for(|&stop| stop).await;
}

fn routing_failure(error: &io::Error) -> io::Error {
    let message = error.to_string();
    let first_line = message.lines().next().unwrap_or_default();
    io::Error::new(
        error.kind(),
        format!(
            "adding DNS rules failed (run from an administrator terminal, or pass --no-routing): {first_line}"
        ),
    )
}

fn describe_upstream_change(servers: &[SocketAddr]) -> String {
    let servers: Vec<String> = servers.iter().map(SocketAddr::to_string).collect();
    format!("upstream DNS now {}", servers.join(", "))
}

fn event_printer(verbose: bool, out: Output) -> Arc<dyn Fn(Event) + Send + Sync> {
    Arc::new(move |event| {
        if verbose || !matches!(event, Event::Forwarded { .. } | Event::Closed { .. }) {
            out(&describe_event(&event));
        }
    })
}

fn describe_start(selected: &[Selected], upstreams: &[SocketAddr], routing: bool) -> String {
    let profiles: Vec<String> = selected
        .iter()
        .map(|s| {
            let count = s.profile.domains.len();
            let noun = if count == 1 { "domain" } else { "domains" };
            format!("{} ({count} {noun}) on {}", s.profile.name, s.address)
        })
        .collect();
    let upstreams: Vec<String> = upstreams.iter().map(SocketAddr::to_string).collect();
    let routing = if routing {
        "DNS rules added"
    } else {
        "no DNS rules"
    };
    format!(
        "snisplit {VERSION}: relaying {}; upstream DNS {}; {routing}; Ctrl+C to stop",
        profiles.join(", "),
        upstreams.join(", ")
    )
}

fn describe_event(event: &Event) -> String {
    match event {
        Event::Forwarded {
            host,
            port,
            pieces: 0,
        } => format!("relayed {host}:{port} with the request head still arriving"),
        Event::Forwarded { host, port, pieces } => {
            let noun = if *pieces == 1 { "piece" } else { "pieces" };
            format!("relayed {host}:{port} in {pieces} {noun}")
        }
        Event::Refused { port, refusal } => {
            format!(
                "refused a connection on port {port}: {}",
                describe_refusal(*refusal)
            )
        }
        Event::Failed { host, port, error } if host.is_empty() => {
            format!("failed a connection on port {port}: {error}")
        }
        Event::Failed { host, port, error } => format!("failed {host}:{port}: {error}"),
        Event::Closed {
            host,
            port,
            to_server,
            to_client,
            respelled,
            stopped,
        } => {
            let stopped = stopped.map_or(String::new(), |reason| {
                format!("; stopped respelling: {}", describe_stop(reason))
            });
            let noun = if *respelled == 1 { "header" } else { "headers" };
            let respelled = match respelled {
                0 => String::new(),
                count => format!(", {count} Host {noun} respelled"),
            };
            format!(
                "closed {host}:{port}: {to_server} bytes sent, {to_client} received\
                 {respelled}{stopped}"
            )
        }
    }
}

fn describe_stop(reason: StopReason) -> &'static str {
    match reason {
        StopReason::TransferEncoding => "the request has a Transfer-Encoding body",
        StopReason::ContentLength => "the request has an unusable Content-Length",
        StopReason::Upgrade => "the request asks for a protocol upgrade",
        StopReason::Connect => "the request is a CONNECT",
        StopReason::HeadTooLong => "a request head is longer than 16 KiB",
        StopReason::NotHttp => "the bytes are not an HTTP/1.x request",
    }
}

fn describe_refusal(refusal: Refusal) -> &'static str {
    match refusal {
        Refusal::NoHost => "no host name in the first bytes",
        Refusal::NotListed => "host is not in the profile",
        Refusal::TooLong => "first bytes too long without a host name",
    }
}

#[cfg(test)]
mod tests;
