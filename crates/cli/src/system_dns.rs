use std::io;
use std::net::Ipv4Addr;

#[cfg(windows)]
mod windows;

#[cfg(any(windows, test))]
#[derive(Debug, Clone, PartialEq, Eq)]
struct Adapter {
    up: bool,
    servers: Vec<Ipv4Addr>,
}

#[cfg(target_os = "linux")]
const RESOLVED_UPSTREAM_FILE: &str = "/run/systemd/resolve/resolv.conf";
#[cfg(target_os = "linux")]
const SYSTEM_RESOLV_CONF: &str = "/etc/resolv.conf";
#[cfg(any(target_os = "linux", test))]
const NAMESERVER_KEYWORD: &str = "nameserver";
#[cfg(any(target_os = "linux", test))]
const NO_SERVERS: &str =
    "no usable IPv4 DNS server in the resolv.conf files (loopback and IPv6 entries are skipped)";

#[cfg(windows)]
pub fn servers() -> io::Result<Vec<Ipv4Addr>> {
    windows::adapters().map(|adapters| usable_servers(&adapters))
}

#[cfg(target_os = "linux")]
pub fn servers() -> io::Result<Vec<Ipv4Addr>> {
    let text = std::fs::read_to_string(RESOLVED_UPSTREAM_FILE)
        .or_else(|_| std::fs::read_to_string(SYSTEM_RESOLV_CONF))?;
    resolv_conf_servers(&text)
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn servers() -> io::Result<Vec<Ipv4Addr>> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "reading the system DNS servers is implemented for Windows and Linux only",
    ))
}

#[cfg(any(windows, test))]
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

#[cfg(any(target_os = "linux", test))]
fn resolv_conf_servers(text: &str) -> io::Result<Vec<Ipv4Addr>> {
    let mut servers: Vec<Ipv4Addr> = Vec::new();
    for server in text.lines().filter_map(nameserver_address) {
        if !server.is_loopback() && !servers.contains(&server) {
            servers.push(server);
        }
    }
    if servers.is_empty() {
        return Err(io::Error::new(io::ErrorKind::NotFound, NO_SERVERS));
    }
    Ok(servers)
}

#[cfg(any(target_os = "linux", test))]
pub(crate) fn nameserver_address(line: &str) -> Option<Ipv4Addr> {
    let mut words = line.split_whitespace();
    if words.next()? != NAMESERVER_KEYWORD {
        return None;
    }
    words.next()?.parse().ok()
}

#[cfg(test)]
mod tests;
