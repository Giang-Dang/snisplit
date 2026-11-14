#![cfg_attr(not(windows), allow(dead_code))]

use std::io;
use std::net::Ipv4Addr;

const RULE_COMMENT: &str = "snisplit";
const MAX_LABEL_LEN: usize = 63;
const MAX_NAME_LEN: usize = 253;
const STOP_ON_ERROR: &str = "$ErrorActionPreference = 'Stop'";

pub fn install(suffixes: &[&str], server: Ipv4Addr) -> io::Result<()> {
    let script = install_script(suffixes, server)?;
    run_powershell(&script).map(drop)
}

pub fn remove_all() -> io::Result<()> {
    run_powershell(&remove_script()).map(drop)
}

pub fn count() -> io::Result<usize> {
    parse_count(&run_powershell(&count_script())?)
}

pub fn flush_cache() -> io::Result<()> {
    run_powershell(&flush_script()).map(drop)
}

pub fn system_dns_servers() -> io::Result<Vec<Ipv4Addr>> {
    parse_dns_servers(&run_powershell(&dns_servers_script())?)
}

fn invalid_input(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

fn invalid_data(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn is_name_byte(byte: u8) -> bool {
    byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'.' || byte == b'-'
}

fn validate_suffix(suffix: &str) -> io::Result<()> {
    let reject = |reason: &str| Err(invalid_input(format!("rejected name {suffix:?}: {reason}")));
    if suffix.len() > MAX_NAME_LEN {
        return reject("longer than 253 bytes");
    }
    if !suffix.bytes().all(is_name_byte) {
        return reject("only a-z, 0-9, '.' and '-' are allowed");
    }
    for label in suffix.split('.') {
        if label.is_empty() {
            return reject("empty label");
        }
        if label.len() > MAX_LABEL_LEN {
            return reject("label longer than 63 bytes");
        }
    }
    Ok(())
}

fn namespaces(suffixes: &[&str]) -> io::Result<Vec<String>> {
    if suffixes.is_empty() {
        return Err(invalid_input("no suffixes to install".to_string()));
    }
    let mut result = Vec::with_capacity(suffixes.len() * 2);
    for suffix in suffixes {
        validate_suffix(suffix)?;
        result.push(format!(".{suffix}"));
        result.push((*suffix).to_string());
    }
    Ok(result)
}

fn install_script(suffixes: &[&str], server: Ipv4Addr) -> io::Result<String> {
    let list = namespaces(suffixes)?
        .iter()
        .map(|namespace| format!("'{namespace}'"))
        .collect::<Vec<_>>()
        .join(", ");
    Ok(format!(
        "{STOP_ON_ERROR}; foreach ($namespace in {list}) {{ Add-DnsClientNrptRule -Namespace $namespace -NameServers '{server}' -Comment '{RULE_COMMENT}' | Out-Null }}"
    ))
}

fn remove_script() -> String {
    format!(
        "{STOP_ON_ERROR}; Get-DnsClientNrptRule | Where-Object {{ $_.Comment -eq '{RULE_COMMENT}' }} | Remove-DnsClientNrptRule -Force"
    )
}

fn count_script() -> String {
    format!(
        "{STOP_ON_ERROR}; @(Get-DnsClientNrptRule | Where-Object {{ $_.Comment -eq '{RULE_COMMENT}' }}).Count"
    )
}

fn flush_script() -> String {
    format!("{STOP_ON_ERROR}; Clear-DnsClientCache")
}

fn dns_servers_script() -> String {
    format!(
        "{STOP_ON_ERROR}; $up = @(Get-NetAdapter | Where-Object {{ $_.Status -eq 'Up' }} | ForEach-Object {{ $_.ifIndex }}); Get-DnsClientServerAddress -AddressFamily IPv4 | Where-Object {{ $up -contains $_.InterfaceIndex }} | ForEach-Object {{ $_.ServerAddresses }}"
    )
}

fn parse_count(stdout: &str) -> io::Result<usize> {
    stdout
        .trim()
        .parse()
        .map_err(|_| invalid_data(format!("expected a rule count, got {:?}", stdout.trim())))
}

fn parse_dns_servers(stdout: &str) -> io::Result<Vec<Ipv4Addr>> {
    let mut servers: Vec<Ipv4Addr> = Vec::new();
    for line in stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let address: Ipv4Addr = line
            .parse()
            .map_err(|_| invalid_data(format!("expected an IPv4 address, got {line:?}")))?;
        if !address.is_loopback() && !servers.contains(&address) {
            servers.push(address);
        }
    }
    Ok(servers)
}

fn interpret_outcome(success: bool, stdout: &[u8], stderr: &[u8]) -> io::Result<String> {
    let stderr = String::from_utf8_lossy(stderr);
    let message = stderr.trim();
    if success && message.is_empty() {
        return Ok(String::from_utf8_lossy(stdout).into_owned());
    }
    let message = if message.is_empty() {
        "powershell failed without a message"
    } else {
        message
    };
    Err(io::Error::other(format!("powershell: {message}")))
}

#[cfg(windows)]
fn run_powershell(script: &str) -> io::Result<String> {
    use std::process::{Command, Stdio};

    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .stdin(Stdio::null())
        .output()?;
    interpret_outcome(output.status.success(), &output.stdout, &output.stderr)
}

#[cfg(not(windows))]
fn run_powershell(_script: &str) -> io::Result<String> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "DNS routing is implemented for Windows only",
    ))
}

#[cfg(test)]
mod tests;
