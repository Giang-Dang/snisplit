use std::future::Future;
use std::io::{self, Write};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use snisplit_core::first_flight::Refusal;
use snisplit_core::http_stream::StopReason;
use snisplit_relay::dns_server::DnsServer;
use snisplit_relay::upstream::UdpResolver;
use snisplit_relay::{Event, Relay, Upstreams};
use tokio::sync::watch;
use tokio::task::JoinSet;

use crate::VERSION;
use crate::args::RunOptions;
use crate::plan::{self, Selected};
use crate::{profile, routing, system_dns};

const UPSTREAM_RETRY: Duration = Duration::from_secs(2);

pub type Output = Arc<dyn Fn(&str) + Send + Sync>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StartPolicy {
    pub wait_for_upstreams: bool,
}

pub fn stdout() -> Output {
    Arc::new(|line| {
        let mut stdout = io::stdout().lock();
        let _ = writeln!(stdout, "{line}");
    })
}

fn load_selection(options: &RunOptions) -> io::Result<Vec<Selected>> {
    let files = options
        .profile_files
        .iter()
        .map(|path| profile::read_file(path))
        .collect::<io::Result<Vec<_>>>()?;
    plan::select(&options.profiles, files).map_err(io::Error::other)
}

fn find_upstreams(options: &RunOptions) -> io::Result<Vec<SocketAddr>> {
    let system = if options.upstreams.is_empty() {
        system_dns::servers()?
    } else {
        Vec::new()
    };
    Ok(plan::upstream_servers(&options.upstreams, &system))
}

async fn wait_for_upstreams(
    options: &RunOptions,
    start: StartPolicy,
) -> io::Result<Vec<SocketAddr>> {
    loop {
        let upstreams = find_upstreams(options)?;
        if !upstreams.is_empty() {
            return Ok(upstreams);
        }
        if !start.wait_for_upstreams {
            return Err(io::Error::other(
                "no usable upstream DNS server found; pass --upstream <ipv4>",
            ));
        }
        tokio::time::sleep(UPSTREAM_RETRY).await;
    }
}

pub async fn serve(
    options: RunOptions,
    start: StartPolicy,
    stop_requested: impl Future<Output = ()> + Send,
    out: Output,
) -> io::Result<()> {
    let selected = load_selection(&options)?;
    let upstreams = wait_for_upstreams(&options, start).await?;
    if options.routing {
        routing::remove_all()?;
    }
    let shared_upstreams = Upstreams::new(upstreams.clone());
    let resolver = Arc::new(UdpResolver::new(
        shared_upstreams.clone(),
        plan::UPSTREAM_TIMEOUT,
    ));
    let relay = Relay::bind(
        plan::relay_config(&selected),
        Arc::clone(&resolver),
        event_printer(options.verbose, Arc::clone(&out)),
    )
    .await?;
    let mut dns_servers = Vec::new();
    for selection in &selected {
        let listen = SocketAddr::from((selection.address, plan::DNS_PORT));
        let profiles = vec![selection.profile.relay_profile()];
        let server = DnsServer::bind(
            listen,
            profiles,
            selection.address,
            shared_upstreams.clone(),
            Arc::clone(&resolver),
        )
        .await?;
        dns_servers.push(server);
    }
    if options.routing {
        if let Err(error) = install_rules(&selected) {
            let _ = remove_rules(&out);
            return Err(routing_failure(&error));
        }
    }
    out(&describe_start(&selected, &upstreams, options.routing));

    let (stop, stopped) = watch::channel(false);
    let mut tasks = JoinSet::new();
    tasks.spawn(relay.run(wait_for_stop(stopped.clone())));
    for server in dns_servers {
        tasks.spawn(server.run(wait_for_stop(stopped.clone())));
    }
    stop_requested.await;
    out("stopping");
    let rules_removed = if options.routing {
        remove_rules(&out)
    } else {
        Ok(())
    };
    let _ = stop.send(true);
    while tasks.join_next().await.is_some() {}
    rules_removed
}

async fn wait_for_stop(mut stopped: watch::Receiver<bool>) {
    let _ = stopped.wait_for(|&stop| stop).await;
}

fn install_rules(selected: &[Selected]) -> io::Result<()> {
    for selection in selected {
        let domains: Vec<&str> = selection
            .profile
            .domains
            .iter()
            .map(String::as_str)
            .collect();
        routing::install(&domains, selection.address)?;
    }
    routing::flush_cache()
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

pub fn remove_rules(out: &Output) -> io::Result<()> {
    routing::remove_all()?;
    routing::flush_cache()?;
    out(&format!("snisplit DNS rules left: {}", routing::count()?));
    Ok(())
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
