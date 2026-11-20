#![cfg_attr(not(windows), allow(dead_code))]

use std::io::{self, Write};
use std::net::Ipv4Addr;
use std::process::{Child, Output};

const RULE_COMMENT: &str = "snisplit";
const MAX_LABEL_LEN: usize = 63;
const MAX_NAME_LEN: usize = 253;
const STOP_ON_ERROR: &str = "$ErrorActionPreference = 'Stop'";
const STALE_RULES_REMOVED: &str = "stale rules removed";
const GO_AHEAD: &[u8] = b"\n";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileRule<'a> {
    pub suffixes: Vec<&'a str>,
    pub server: Ipv4Addr,
}

#[derive(Debug)]
pub enum StartFailure {
    RemovingStale(io::Error),
    Adding(io::Error),
}

#[cfg(not(target_os = "linux"))]
pub struct PendingStart(Gated);

#[cfg(target_os = "linux")]
pub use linux::{PendingStart, begin_start, count_all, remove_all};

#[cfg(not(target_os = "linux"))]
pub fn begin_start(rules: &[ProfileRule]) -> io::Result<PendingStart> {
    let script = start_script(rules)?;
    Gated::spawn(&script).map(PendingStart)
}

#[cfg(not(target_os = "linux"))]
impl PendingStart {
    pub fn add(self) -> Result<(), StartFailure> {
        let output = self.0.release().map_err(StartFailure::RemovingStale)?;
        start_outcome(output.status.success(), &output.stdout, &output.stderr)
    }

    pub fn abandon(self) -> io::Result<()> {
        let output = self.0.abandon()?;
        interpret_outcome(output.status.success(), &output.stdout, &output.stderr).map(drop)
    }
}

#[cfg(not(target_os = "linux"))]
pub fn remove_all() -> io::Result<usize> {
    parse_count(&run_powershell(&stop_script())?)
}

#[cfg(not(target_os = "linux"))]
pub fn count_all() -> io::Result<usize> {
    parse_count(&run_powershell(&count_script())?)
}

fn start_outcome(success: bool, stdout: &[u8], stderr: &[u8]) -> Result<(), StartFailure> {
    let removed_stale = String::from_utf8_lossy(stdout)
        .lines()
        .any(|line| line.trim() == STALE_RULES_REMOVED);
    interpret_outcome(success, stdout, stderr)
        .map(drop)
        .map_err(|error| {
            if removed_stale {
                StartFailure::Adding(error)
            } else {
                StartFailure::RemovingStale(error)
            }
        })
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

fn add_command(rule: &ProfileRule) -> io::Result<String> {
    let list = namespaces(&rule.suffixes)?
        .iter()
        .map(|namespace| format!("'{namespace}'"))
        .collect::<Vec<_>>()
        .join(", ");
    Ok(format!(
        "Add-DnsClientNrptRule -Namespace {list} -NameServers '{}' -Comment '{RULE_COMMENT}' | Out-Null",
        rule.server
    ))
}

fn remove_command() -> String {
    format!(
        "Get-DnsClientNrptRule | Where-Object {{ $_.Comment -eq '{RULE_COMMENT}' }} | Remove-DnsClientNrptRule -Force"
    )
}

fn start_script(rules: &[ProfileRule]) -> io::Result<String> {
    if rules.is_empty() {
        return Err(invalid_input("no profiles to add rules for".to_string()));
    }
    let adds = rules
        .iter()
        .map(add_command)
        .collect::<io::Result<Vec<_>>>()?
        .join("; ");
    Ok(format!(
        "{STOP_ON_ERROR}; {}; Write-Output '{STALE_RULES_REMOVED}'; if ($null -eq [Console]::In.ReadLine()) {{ exit }}; {adds}; Clear-DnsClientCache",
        remove_command()
    ))
}

fn count_command() -> String {
    format!("@(Get-DnsClientNrptRule | Where-Object {{ $_.Comment -eq '{RULE_COMMENT}' }}).Count")
}

fn stop_script() -> String {
    format!(
        "{STOP_ON_ERROR}; {}; Clear-DnsClientCache; {}",
        remove_command(),
        count_command()
    )
}

fn count_script() -> String {
    format!("{STOP_ON_ERROR}; {}", count_command())
}

fn parse_count(stdout: &str) -> io::Result<usize> {
    stdout
        .trim()
        .parse()
        .map_err(|_| invalid_data(format!("expected a rule count, got {:?}", stdout.trim())))
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

struct Gated(Child);

impl Gated {
    fn release(mut self) -> io::Result<Output> {
        if let Some(mut stdin) = self.0.stdin.take() {
            let _ = stdin.write_all(GO_AHEAD);
        }
        self.0.wait_with_output()
    }

    fn abandon(mut self) -> io::Result<Output> {
        drop(self.0.stdin.take());
        self.0.wait_with_output()
    }
}

#[cfg(windows)]
mod powershell {
    use std::io;
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};

    use super::{Gated, interpret_outcome};

    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    fn command(script: &str) -> Command {
        let mut command = Command::new("powershell.exe");
        command
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
        command
    }

    pub(super) fn run(script: &str) -> io::Result<String> {
        let output = command(script).stdin(Stdio::null()).output()?;
        interpret_outcome(output.status.success(), &output.stdout, &output.stderr)
    }

    pub(super) fn spawn_gated(script: &str) -> io::Result<Gated> {
        command(script)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map(Gated)
    }
}

#[cfg(windows)]
fn run_powershell(script: &str) -> io::Result<String> {
    powershell::run(script)
}

#[cfg(windows)]
impl Gated {
    fn spawn(script: &str) -> io::Result<Self> {
        powershell::spawn_gated(script)
    }
}

#[cfg(not(windows))]
fn unsupported() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "DNS routing is implemented for Windows only",
    )
}

#[cfg(not(windows))]
fn run_powershell(_script: &str) -> io::Result<String> {
    Err(unsupported())
}

#[cfg(not(windows))]
impl Gated {
    fn spawn(_script: &str) -> io::Result<Self> {
        Err(unsupported())
    }
}

#[cfg(any(target_os = "linux", test))]
mod linux;

#[cfg(test)]
mod tests;
