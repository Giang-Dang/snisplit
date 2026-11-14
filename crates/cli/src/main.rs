mod args;
mod platform;
mod profile;

use std::env;
use std::process::ExitCode;
use std::time::Instant;

use snisplit_core::engine::{Engine, Limits};
use snisplit_core::packet::parse_tcp;
use snisplit_core::pump::Stats;

use args::{Command, USAGE};

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
        Ok(Command::Run { verbose }) => run(verbose),
        Err(error) => {
            eprintln!("snisplit: {error}\n\n{USAGE}");
            ExitCode::from(EXIT_USAGE_ERROR)
        }
    }
}

fn run(verbose: bool) -> ExitCode {
    let mut engine = Engine::new(vec![profile::steam()], Limits::default());
    println!(
        "snisplit {VERSION}: splitting {} Steam domains, filter: {}",
        profile::STEAM_DOMAINS.len(),
        platform::DIVERT_FILTER
    );
    let started = Instant::now();
    let now_ms = move || u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let on_split = move |packet: &[u8], segments: &[Vec<u8>]| {
        if verbose {
            println!("{}", describe_split(packet, segments.len()));
        }
    };
    match platform::divert(platform::DIVERT_FILTER, &mut engine, now_ms, on_split) {
        Ok(stats) => {
            println!("stopped: {}", describe_stats(&stats));
            ExitCode::SUCCESS
        }
        Err(failure) => {
            eprintln!("snisplit: error: {}", failure.error);
            if failure.stats != Stats::default() {
                eprintln!("stopped: {}", describe_stats(&failure.stats));
            }
            ExitCode::from(EXIT_RUNTIME_ERROR)
        }
    }
}

fn describe_split(packet: &[u8], segment_count: usize) -> String {
    match parse_tcp(packet) {
        Some(tcp) => format!(
            "split {}:{} -> {}:{} ({} payload bytes) into {segment_count} segments",
            tcp.src,
            tcp.src_port,
            tcp.dst,
            tcp.dst_port,
            tcp.payload.len()
        ),
        None => format!("split an unparsable packet into {segment_count} segments"),
    }
}

fn describe_stats(stats: &Stats) -> String {
    format!(
        "received {}, passed {}, replaced {}, segments sent {}, send failures {}",
        stats.received, stats.passed, stats.replaced, stats.segments_sent, stats.send_failures
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const IPV4_TCP_PACKET: [u8; 43] = [
        0x45, 0x00, 0x00, 0x2b, 0x00, 0x01, 0x40, 0x00, 0x40, 0x06, 0x00, 0x00, 10, 0, 0, 2, 93,
        184, 216, 34, 0xc3, 0x50, 0x01, 0xbb, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x50,
        0x18, 0x10, 0x00, 0x00, 0x00, 0x00, 0x00, b'a', b'b', b'c',
    ];

    #[test]
    fn a_split_is_described_from_client_to_server_with_its_size() {
        assert_eq!(
            describe_split(&IPV4_TCP_PACKET, 3),
            "split 10.0.0.2:50000 -> 93.184.216.34:443 (3 payload bytes) into 3 segments"
        );
    }

    #[test]
    fn an_unparsable_split_is_still_described() {
        assert_eq!(
            describe_split(&[0, 1, 2], 2),
            "split an unparsable packet into 2 segments"
        );
    }

    #[test]
    fn stats_are_described_with_every_counter_under_its_own_label() {
        let stats = Stats {
            received: 10,
            passed: 7,
            replaced: 3,
            segments_sent: 9,
            send_failures: 2,
        };
        assert_eq!(
            describe_stats(&stats),
            "received 10, passed 7, replaced 3, segments sent 9, send failures 2"
        );
    }
}
