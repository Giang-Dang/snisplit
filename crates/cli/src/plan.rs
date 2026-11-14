use std::collections::HashMap;
use std::fmt;
use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use snisplit_core::first_flight::Protocol;
use snisplit_relay::{Config, Listener};

use crate::profile::BUILT_IN;
use crate::profile_file::{ProfileError, ProfileSpec};

pub const DNS_PORT: u16 = 53;
const FIRST_PROFILE_ADDRESS: Ipv4Addr = Ipv4Addr::new(127, 0, 0, 2);
const LAST_PROFILE_HOST_BYTE: u8 = 254;
pub const PIECE_GAP: Duration = Duration::from_millis(30);
const FIRST_FLIGHT_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_FIRST_FLIGHT: usize = 16 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
pub const UPSTREAM_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selected {
    pub profile: ProfileSpec,
    pub address: Ipv4Addr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectionError {
    BuiltIn(ProfileError),
    DuplicateName(String),
    DuplicateDomain {
        domain: String,
        first: String,
        second: String,
    },
    TooManyProfiles,
}

impl fmt::Display for SelectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BuiltIn(error) => write!(f, "{error}"),
            Self::DuplicateName(name) => write!(f, "two profiles are named {name}"),
            Self::DuplicateDomain {
                domain,
                first,
                second,
            } => write!(
                f,
                "domain {domain} is in both profile {first} and profile {second}"
            ),
            Self::TooManyProfiles => write!(f, "too many profiles: no loopback address left"),
        }
    }
}

impl std::error::Error for SelectionError {}

pub fn select(
    built_in_names: &[&str],
    files: Vec<ProfileSpec>,
) -> Result<Vec<Selected>, SelectionError> {
    let mut profiles = Vec::new();
    for (index, built_in) in BUILT_IN.iter().enumerate() {
        if built_in_names.contains(&built_in.name) {
            let profile = built_in.spec().map_err(SelectionError::BuiltIn)?;
            profiles.push((index, profile));
        }
    }
    let first_file_index = BUILT_IN.len();
    profiles.extend(
        files
            .into_iter()
            .enumerate()
            .map(|(offset, profile)| (first_file_index + offset, profile)),
    );
    let selected = profiles
        .into_iter()
        .map(|(index, profile)| {
            profile_address(index)
                .map(|address| Selected { profile, address })
                .ok_or(SelectionError::TooManyProfiles)
        })
        .collect::<Result<Vec<_>, _>>()?;
    reject_clashes(&selected)?;
    Ok(selected)
}

fn reject_clashes(selected: &[Selected]) -> Result<(), SelectionError> {
    let mut names: Vec<&str> = Vec::new();
    let mut owners: HashMap<&str, &str> = HashMap::new();
    for selection in selected {
        let name = selection.profile.name.as_str();
        if names.contains(&name) {
            return Err(SelectionError::DuplicateName(name.to_string()));
        }
        names.push(name);
        for domain in &selection.profile.domains {
            if let Some(first) = owners.insert(domain, name) {
                return Err(SelectionError::DuplicateDomain {
                    domain: domain.clone(),
                    first: first.to_string(),
                    second: name.to_string(),
                });
            }
        }
    }
    Ok(())
}

fn profile_address(index: usize) -> Option<Ipv4Addr> {
    let [a, b, c, d] = FIRST_PROFILE_ADDRESS.octets();
    let host_byte = u8::try_from(index)
        .ok()
        .and_then(|offset| d.checked_add(offset))
        .filter(|&host_byte| host_byte <= LAST_PROFILE_HOST_BYTE)?;
    Some(Ipv4Addr::new(a, b, c, host_byte))
}

pub fn relay_config(selected: &[Selected]) -> Config {
    let profiles: Vec<_> = selected.iter().map(|s| s.profile.relay_profile()).collect();
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
