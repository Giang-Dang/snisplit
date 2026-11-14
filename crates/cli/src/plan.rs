use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use snisplit_core::first_flight::Protocol;
use snisplit_relay::{Config, Listener};

use crate::profile::{BUILT_IN, BuiltIn};

pub const DNS_PORT: u16 = 53;
const FIRST_PROFILE_ADDRESS: Ipv4Addr = Ipv4Addr::new(127, 0, 0, 2);
const PIECE_GAP: Duration = Duration::from_millis(30);
const FIRST_FLIGHT_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_FIRST_FLIGHT: usize = 16 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
pub const UPSTREAM_TIMEOUT: Duration = Duration::from_secs(2);

pub struct Selected {
    pub built_in: &'static BuiltIn,
    pub address: Ipv4Addr,
}

pub fn select(names: &[&str]) -> Vec<Selected> {
    BUILT_IN
        .iter()
        .enumerate()
        .filter(|(_, built_in)| names.contains(&built_in.name))
        .map(|(index, built_in)| Selected {
            built_in,
            address: profile_address(index),
        })
        .collect()
}

fn profile_address(index: usize) -> Ipv4Addr {
    let [a, b, c, d] = FIRST_PROFILE_ADDRESS.octets();
    let offset = u8::try_from(index).unwrap_or(u8::MAX);
    Ipv4Addr::new(a, b, c, d.saturating_add(offset))
}

pub fn relay_config(selected: &[Selected]) -> Config {
    let profiles: Vec<_> = selected.iter().map(|s| (s.built_in.profile)()).collect();
    let mut listeners = Vec::new();
    for (index, (selection, profile)) in selected.iter().zip(&profiles).enumerate() {
        let ports = profile
            .tls_ports
            .iter()
            .map(|&port| (port, Protocol::Tls))
            .chain(
                profile
                    .http_ports
                    .iter()
                    .map(|&port| (port, Protocol::Http)),
            );
        for (port, protocol) in ports {
            listeners.push(Listener {
                listen: SocketAddr::from((selection.address, port)),
                upstream_port: port,
                protocol,
                profile: index,
            });
        }
    }
    Config {
        listeners,
        profiles,
        piece_gap: PIECE_GAP,
        first_flight_timeout: FIRST_FLIGHT_TIMEOUT,
        max_first_flight: MAX_FIRST_FLIGHT,
        connect_timeout: CONNECT_TIMEOUT,
    }
}

pub fn upstream_servers(explicit: &[Ipv4Addr], system: &[Ipv4Addr]) -> Vec<SocketAddr> {
    let chosen = if explicit.is_empty() {
        system
    } else {
        explicit
    };
    let mut servers: Vec<SocketAddr> = Vec::new();
    for &address in chosen {
        let server = SocketAddr::from((address, DNS_PORT));
        if !address.is_loopback() && !address.is_unspecified() && !servers.contains(&server) {
            servers.push(server);
        }
    }
    servers
}

#[cfg(test)]
mod tests;
