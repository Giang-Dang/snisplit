use std::io::{self, Write};
use std::net::{Ipv4Addr, SocketAddr};

use snisplit_core::probe::attempts;
use snisplit_relay::probe::{ProbeReport, ProbeSettings, probe};
use snisplit_relay::{UdpResolver, Upstreams};

use crate::args::ProbeOptions;
use crate::{plan, system_dns};

const NO_UPSTREAM: &str = "no usable system DNS server found; pass --upstream <ip[:port]>";
const COLUMN_GAP: &str = "  ";
const PROFILE_HINT: &str =
    "suggested profile (save it to a file, then: snisplit run --profile-file <file>):";

pub fn run(options: &ProbeOptions) -> io::Result<()> {
    let servers = upstream_servers(&options.upstreams, system_dns::servers)?;
    let settings = ProbeSettings {
        runs: options.runs,
        timeout: options.timeout,
        piece_gap: plan::PIECE_GAP,
    };
    let mut out = io::stdout();
    writeln!(out, "{}", starting_line(options, &servers))?;
    out.flush()?;
    let report = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(async {
            let resolver = UdpResolver::new(Upstreams::new(servers), plan::UPSTREAM_TIMEOUT);
            probe(&resolver, &options.domain, &settings).await
        })?;
    out.write_all(render(&report).as_bytes())
}

fn upstream_servers(
    explicit: &[SocketAddr],
    system: impl FnOnce() -> io::Result<Vec<Ipv4Addr>>,
) -> io::Result<Vec<SocketAddr>> {
    if !explicit.is_empty() {
        return Ok(explicit.to_vec());
    }
    let servers = plan::upstream_servers(&[], &system()?);
    if servers.is_empty() {
        return Err(io::Error::new(io::ErrorKind::NotFound, NO_UPSTREAM));
    }
    Ok(servers)
}

fn starting_line(options: &ProbeOptions, servers: &[SocketAddr]) -> String {
    let servers: Vec<String> = servers.iter().map(ToString::to_string).collect();
    format!(
        "probing {} through DNS {}: {} attempts, {} runs each, timeout {} s",
        options.domain,
        servers.join(", "),
        attempts().len(),
        options.runs,
        options.timeout.as_secs()
    )
}

fn render(report: &ProbeReport) -> String {
    let runs = report
        .records
        .iter()
        .map(|record| record.outcomes.len())
        .max()
        .unwrap_or(0);
    let mut header = vec!["attempt".to_owned(), "result".to_owned()];
    header.extend((1..=runs).map(|run| format!("run {run}")));
    let mut rows = vec![header];
    rows.extend(report.records.iter().map(|record| {
        let mut row = vec![record.attempt.to_string(), record.agreement().to_string()];
        row.extend(record.outcomes.iter().map(ToString::to_string));
        row
    }));
    let mut text = format!("address {}\n\n{}\n", report.address, table(&rows));
    text.push_str(&format!("verdict: {}\n", report.assessment.summary()));
    if let Some(profile) = &report.assessment.profile {
        text.push_str(&format!("\n{PROFILE_HINT}\n\n{profile}"));
    }
    text
}

fn table(rows: &[Vec<String>]) -> String {
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
    let widths: Vec<usize> = (0..columns)
        .map(|column| {
            rows.iter()
                .filter_map(|row| row.get(column))
                .map(String::len)
                .max()
                .unwrap_or(0)
        })
        .collect();
    let lines: Vec<String> = rows
        .iter()
        .map(|row| {
            let cells: Vec<String> = row
                .iter()
                .zip(&widths)
                .map(|(cell, &width)| format!("{cell:width$}"))
                .collect();
            cells.join(COLUMN_GAP).trim_end().to_owned()
        })
        .collect();
    lines.join("\n") + "\n"
}

#[cfg(test)]
mod tests;
