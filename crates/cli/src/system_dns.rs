#![cfg_attr(not(windows), allow(dead_code))]

use std::io;
use std::net::Ipv4Addr;

#[cfg(windows)]
mod windows;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Adapter {
    up: bool,
    servers: Vec<Ipv4Addr>,
}

#[cfg(windows)]
pub fn servers() -> io::Result<Vec<Ipv4Addr>> {
    windows::adapters().map(|adapters| usable_servers(&adapters))
}

#[cfg(not(windows))]
pub fn servers() -> io::Result<Vec<Ipv4Addr>> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "reading the system DNS servers is implemented for Windows only",
    ))
}

fn usable_servers(adapters: &[Adapter]) -> Vec<Ipv4Addr> {
    let mut servers: Vec<Ipv4Addr> = Vec::new();
    let candidates = adapters
        .iter()
        .filter(|adapter| adapter.up)
        .flat_map(|adapter| &adapter.servers);
    for &server in candidates {
        if !server.is_loopback() && !servers.contains(&server) {
            servers.push(server);
        }
    }
    servers
}

#[cfg(test)]
mod tests;
