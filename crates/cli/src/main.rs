mod args;
mod plan;
mod profile;
mod routing;

use std::env;
use std::io;
use std::net::SocketAddr;
use std::process::ExitCode;
use std::sync::Arc;

use snisplit_core::first_flight::Refusal;
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

fn run(options: RunOptions) -> io::Result<()> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(serve(options))
}

async fn serve(options: RunOptions) -> io::Result<()> {
    let selected = plan::select(&options.profiles);
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
    let relay = Relay::bind(
        plan::relay_config(&selected),
        UdpResolver::new(upstreams.clone(), plan::UPSTREAM_TIMEOUT),
        event_printer(options.verbose),
    )
    .await?;
    let mut dns_servers = Vec::new();
    for selection in &selected {
        let listen = SocketAddr::from((selection.address, plan::DNS_PORT));
        let profiles = vec![(selection.built_in.profile)()];
        dns_servers
            .push(DnsServer::bind(listen, profiles, selection.address, upstreams.clone()).await?);
    }
    if options.routing {
        if let Err(error) = install_rules(&selected) {
            let _ = remove_rules();
            return Err(error);
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
        routing::install(selection.built_in.domains, selection.address)?;
    }
    routing::flush_cache()
}

fn remove_rules() -> io::Result<()> {
    routing::remove_all()?;
    routing::flush_cache()?;
    println!("snisplit DNS rules left: {}", routing::count()?);
    Ok(())
}

fn event_printer(verbose: bool) -> Arc<dyn Fn(Event) + Send + Sync> {
    Arc::new(move |event| {
        if verbose || !matches!(event, Event::Forwarded { .. }) {
            println!("{}", describe_event(&event));
        }
    })
}

fn describe_start(selected: &[Selected], upstreams: &[SocketAddr], routing: bool) -> String {
    let profiles: Vec<String> = selected
        .iter()
        .map(|s| {
            let count = s.built_in.domains.len();
            let noun = if count == 1 { "domain" } else { "domains" };
            format!("{} ({count} {noun}) on {}", s.built_in.name, s.address)
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
    fn the_start_line_lists_profiles_addresses_upstreams_and_routing() {
        let selected = plan::select(&["steam", "medium"]);
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
}
