mod args;
mod plan;
mod profile;
mod profile_file;
mod routing;

use std::env;
use std::io::{self, Write};
use std::net::SocketAddr;
use std::process::ExitCode;
use std::sync::Arc;

use snisplit_core::first_flight::Refusal;
use snisplit_core::http_stream::StopReason;
use snisplit_relay::dns_server::DnsServer;
use snisplit_relay::upstream::UdpResolver;
use snisplit_relay::{Event, Relay};
use tokio::sync::watch;
use tokio::task::JoinSet;

use args::{Command, RunOptions, USAGE};
use plan::Selected;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const EXIT_RUNTIME_ERROR: u8 = 1;
const EXIT_USAGE_ERROR: u8 = 2;

fn main() -> ExitCode {
    let args = env::args_os()
        .skip(1)
        .map(|arg| arg.to_string_lossy().into_owned());
    match args::parse(args) {
        Ok(Command::Help) => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Ok(Command::Version) => {
            println!("snisplit {VERSION}");
            ExitCode::SUCCESS
        }
        Ok(Command::Run(options)) => finish(run(options)),
        Ok(Command::Cleanup) => finish(remove_rules()),
        Ok(Command::ShowProfile(name)) => finish(show_profile(name)),
        Err(error) => {
            eprintln!("snisplit: {error}\n\n{USAGE}");
            ExitCode::from(EXIT_USAGE_ERROR)
        }
    }
}

fn finish(result: io::Result<()>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("snisplit: error: {error}");
            ExitCode::from(EXIT_RUNTIME_ERROR)
        }
    }
}

fn show_profile(name: &str) -> io::Result<()> {
    let built_in = profile::find(name).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("no built-in profile {name}"),
        )
    })?;
    io::stdout().write_all(built_in.text.as_bytes())
}

fn run(options: RunOptions) -> io::Result<()> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(serve(options))
}

fn load_selection(options: &RunOptions) -> io::Result<Vec<Selected>> {
    let files = options
        .profile_files
        .iter()
        .map(|path| profile::read_file(path))
        .collect::<io::Result<Vec<_>>>()?;
    plan::select(&options.profiles, files).map_err(io::Error::other)
}

async fn serve(options: RunOptions) -> io::Result<()> {
    let selected = load_selection(&options)?;
    let system = if options.upstreams.is_empty() {
        routing::system_dns_servers()?
    } else {
        Vec::new()
    };
    let upstreams = plan::upstream_servers(&options.upstreams, &system);
    if upstreams.is_empty() {
        return Err(io::Error::other(
            "no usable upstream DNS server found; pass --upstream <ipv4>",
        ));
    }
    if options.routing {
        routing::remove_all()?;
    }
    let resolver = Arc::new(UdpResolver::new(upstreams.clone(), plan::UPSTREAM_TIMEOUT));
    let relay = Relay::bind(
        plan::relay_config(&selected),
        Arc::clone(&resolver),
        event_printer(options.verbose),
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
            upstreams.clone(),
            Arc::clone(&resolver),
        )
        .await?;
        dns_servers.push(server);
    }
    if options.routing {
        if let Err(error) = install_rules(&selected) {
            let _ = remove_rules();
            return Err(routing_failure(&error));
        }
    }
    println!("{}", describe_start(&selected, &upstreams, options.routing));

    let (stop, stopped) = watch::channel(false);
    let mut tasks = JoinSet::new();
    tasks.spawn(relay.run(wait_for_stop(stopped.clone())));
    for server in dns_servers {
        tasks.spawn(server.run(wait_for_stop(stopped.clone())));
    }
    let signal = tokio::signal::ctrl_c().await;
    println!("stopping");
    let rules_removed = if options.routing {
        remove_rules()
    } else {
        Ok(())
    };
    let _ = stop.send(true);
    while tasks.join_next().await.is_some() {}
    signal.and(rules_removed)
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

fn remove_rules() -> io::Result<()> {
    routing::remove_all()?;
    routing::flush_cache()?;
    println!("snisplit DNS rules left: {}", routing::count()?);
    Ok(())
}

fn event_printer(verbose: bool) -> Arc<dyn Fn(Event) + Send + Sync> {
    Arc::new(move |event| {
        if verbose || !matches!(event, Event::Forwarded { .. } | Event::Closed { .. }) {
            println!("{}", describe_event(&event));
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
mod tests {
    use super::*;

    #[test]
    fn a_relayed_connection_names_host_port_and_pieces() {
        let event = Event::Forwarded {
            host: "medium.com".to_string(),
            port: 443,
            pieces: 2,
        };
        assert_eq!(describe_event(&event), "relayed medium.com:443 in 2 pieces");
        let single = Event::Forwarded {
            host: "medium.com".to_string(),
            port: 80,
            pieces: 1,
        };
        assert_eq!(describe_event(&single), "relayed medium.com:80 in 1 piece");
    }

    #[test]
    fn refusals_say_why_in_words() {
        let cases = [
            (
                Refusal::NoHost,
                "refused a connection on port 80: no host name in the first bytes",
            ),
            (
                Refusal::NotListed,
                "refused a connection on port 80: host is not in the profile",
            ),
            (
                Refusal::TooLong,
                "refused a connection on port 80: first bytes too long without a host name",
            ),
        ];
        for (refusal, expected) in cases {
            assert_eq!(
                describe_event(&Event::Refused { port: 80, refusal }),
                expected
            );
        }
    }

    #[test]
    fn failures_name_the_host_when_it_is_known() {
        let known = Event::Failed {
            host: "store.steampowered.com".to_string(),
            port: 443,
            error: "connecting timed out".to_string(),
        };
        let unknown = Event::Failed {
            host: String::new(),
            port: 27019,
            error: "first flight timed out".to_string(),
        };
        assert_eq!(
            describe_event(&known),
            "failed store.steampowered.com:443: connecting timed out"
        );
        assert_eq!(
            describe_event(&unknown),
            "failed a connection on port 27019: first flight timed out"
        );
    }

    #[test]
    fn a_closed_connection_reports_bytes_respelling_and_why_it_stopped() {
        let plain = Event::Closed {
            host: "medium.com".to_string(),
            port: 80,
            to_server: 210,
            to_client: 4096,
            respelled: 2,
            stopped: None,
        };
        let stopped = Event::Closed {
            host: "store.steampowered.com".to_string(),
            port: 80,
            to_server: 90,
            to_client: 0,
            respelled: 1,
            stopped: Some(StopReason::Upgrade),
        };
        assert_eq!(
            describe_event(&plain),
            "closed medium.com:80: 210 bytes sent, 4096 received, 2 Host headers respelled"
        );
        assert_eq!(
            describe_event(&stopped),
            "closed store.steampowered.com:80: 90 bytes sent, 0 received, 1 Host header respelled; \
             stopped respelling: the request asks for a protocol upgrade"
        );
        let tls = Event::Closed {
            host: "medium.com".to_string(),
            port: 443,
            to_server: 693,
            to_client: 10713,
            respelled: 0,
            stopped: None,
        };
        assert_eq!(
            describe_event(&tls),
            "closed medium.com:443: 693 bytes sent, 10713 received"
        );
    }

    #[test]
    fn a_routing_failure_keeps_only_the_first_line_and_says_what_to_do() {
        let raw = io::Error::other(
            "powershell: Add-DnsClientNrptRule : Failed to load the NRPT information.\nAt line:1 char:88\n+ ...",
        );
        let shown = routing_failure(&raw);
        assert_eq!(
            shown.to_string(),
            "adding DNS rules failed (run from an administrator terminal, or pass --no-routing): \
             powershell: Add-DnsClientNrptRule : Failed to load the NRPT information."
        );
        assert_eq!(shown.kind(), io::ErrorKind::Other);
    }

    #[test]
    fn the_start_line_lists_profiles_addresses_upstreams_and_routing() {
        let selected = plan::select(&["steam", "medium"], Vec::new()).unwrap();
        let upstreams = ["1.1.1.1:53".parse().unwrap(), "1.0.0.1:53".parse().unwrap()];
        assert_eq!(
            describe_start(&selected, &upstreams, true),
            format!(
                "snisplit {VERSION}: relaying steam (14 domains) on 127.0.0.2, \
                 medium (1 domain) on 127.0.0.3; upstream DNS 1.1.1.1:53, 1.0.0.1:53; \
                 DNS rules added; Ctrl+C to stop"
            )
        );
        assert!(describe_start(&selected, &upstreams, false).contains("; no DNS rules;"));
    }

    #[test]
    fn the_start_line_shows_file_profiles_after_the_built_ins_with_their_own_addresses() {
        let file = profile::parse_file(
            "game.profile",
            "name game\ndomains game.example.com cdn.example.org\n",
        )
        .unwrap();
        let selected = plan::select(&["medium"], vec![file]).unwrap();
        let upstreams = ["1.1.1.1:53".parse().unwrap()];
        assert_eq!(
            describe_start(&selected, &upstreams, false),
            format!(
                "snisplit {VERSION}: relaying medium (1 domain) on 127.0.0.3, \
                 game (2 domains) on 127.0.0.4; upstream DNS 1.1.1.1:53; \
                 no DNS rules; Ctrl+C to stop"
            )
        );
    }
}
