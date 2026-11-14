use core::ops::Range;

use crate::domains::DomainList;
use crate::http;
use crate::inspect::Inspection;
use crate::segment::Order;
use crate::strategy::{SplitPosition, Strategy};
use crate::tls;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    Tls,
    Http,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    PassThrough,
    HostSpell,
}

#[derive(Debug, Clone)]
pub struct RelayProfile {
    pub name: String,
    pub domains: DomainList,
    pub tls_positions: Vec<SplitPosition>,
    pub tls_ports: Vec<u16>,
    pub http_ports: Vec<u16>,
    pub http_method: HttpMethod,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    NeedMoreData,
    Forward { host: String, cuts: Vec<usize> },
    Refuse(Refusal),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    NoHost,
    NotListed,
    TooLong,
}

pub fn decide(
    profile: &RelayProfile,
    protocol: Protocol,
    first_bytes: &[u8],
    max_len: usize,
) -> Decision {
    match inspect(protocol, first_bytes) {
        Inspection::NeedMoreData if first_bytes.len() >= max_len => {
            Decision::Refuse(Refusal::TooLong)
        }
        Inspection::NeedMoreData => Decision::NeedMoreData,
        Inspection::NoHost => Decision::Refuse(Refusal::NoHost),
        Inspection::Host(host_in_stream) => forward(profile, protocol, first_bytes, host_in_stream),
    }
}

fn inspect(protocol: Protocol, first_bytes: &[u8]) -> Inspection {
    match protocol {
        Protocol::Tls => tls::inspect_client_hello(first_bytes),
        Protocol::Http => http::inspect_request(first_bytes),
    }
}

fn forward(
    profile: &RelayProfile,
    protocol: Protocol,
    first_bytes: &[u8],
    host_in_stream: Range<usize>,
) -> Decision {
    let Some(host_bytes) = first_bytes.get(host_in_stream.clone()) else {
        return Decision::Refuse(Refusal::NoHost);
    };
    let Some(host) = printable_ascii(host_bytes) else {
        return Decision::Refuse(Refusal::NoHost);
    };
    let Some(matched_domain) = profile.domains.matched_domain(host_bytes) else {
        return Decision::Refuse(Refusal::NotListed);
    };
    let cuts = match protocol {
        Protocol::Tls => tls_cuts(
            profile,
            first_bytes.len(),
            host_in_stream,
            host_bytes,
            matched_domain,
        ),
        Protocol::Http => Vec::new(),
    };
    Decision::Forward {
        host: host.to_ascii_lowercase(),
        cuts,
    }
}

fn printable_ascii(bytes: &[u8]) -> Option<&str> {
    bytes
        .iter()
        .all(u8::is_ascii_graphic)
        .then(|| core::str::from_utf8(bytes).ok())
        .flatten()
}

fn tls_cuts(
    profile: &RelayProfile,
    stream_len: usize,
    host_in_stream: Range<usize>,
    host_bytes: &[u8],
    matched_domain: Range<usize>,
) -> Vec<usize> {
    let strategy = Strategy {
        order: Order::InSequence,
        positions: profile.tls_positions.clone(),
    };
    let mut cuts = strategy.stream_offsets(host_in_stream, host_bytes, matched_domain);
    cuts.retain(|&cut| (1..stream_len).contains(&cut));
    cuts
}

#[cfg(test)]
mod tests;
