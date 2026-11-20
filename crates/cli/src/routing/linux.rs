#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use std::fs;
use std::io;
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::{ProfileRule, StartFailure, invalid_input, namespaces};
use crate::system_dns::nameserver_address;

const TOOL_FOLDERS: [&str; 4] = ["/usr/sbin", "/usr/bin", "/sbin", "/bin"];
const TOOL_LOCALE: (&str, &str) = ("LC_ALL", "C");
const LINK_PREFIX: &str = "snisplit";
const LINK_KIND_DUMMY: &str = "dummy";
const LINK_DETAIL_SEPARATOR: char = '\\';
const NET_CLASS_FOLDER: &str = "/sys/class/net";
const PROCESS_STATUS_FILE: &str = "/proc/self/status";
const UID_KEY: &str = "Uid:";
const EFFECTIVE_UID_FIELD: usize = 1;
const ROOT_UID: u32 = 0;
const RESOLV_CONF: &str = "/etc/resolv.conf";
const RESOLV_CONF_FOLDER: &str = "/etc";
const STUB_RESOLV_CONF: &str = "/run/systemd/resolve/stub-resolv.conf";
const STUB_ADDRESS: Ipv4Addr = Ipv4Addr::new(127, 0, 0, 53);
const ADDRESS_NETWORK: [u8; 3] = [169, 254, 53];
const ADDRESS_PREFIX_LEN: u8 = 32;
const LAST_HOST_OCTET: u8 = 254;
const MAX_LINK_NAME_LEN: usize = 15;
const NEEDS_RESOLVED: &str = "snisplit on Linux needs systemd-resolved as the system resolver";
const NEEDS_ROOT: &str = "snisplit needs root to change DNS routing: run it with sudo";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tool {
    Ip,
    Resolvectl,
}

impl Tool {
    fn name(self) -> &'static str {
        match self {
            Tool::Ip => "ip",
            Tool::Resolvectl => "resolvectl",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Invocation {
    tool: Tool,
    args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Link {
    name: String,
    address: Ipv4Addr,
}

pub struct PendingStart {
    invocations: Vec<Invocation>,
}

pub fn begin_start(rules: &[ProfileRule]) -> io::Result<PendingStart> {
    let invocations = start_invocations(rules)?;
    preflight()?;
    Ok(PendingStart { invocations })
}

impl PendingStart {
    pub fn add(self) -> Result<(), StartFailure> {
        remove_links().map_err(StartFailure::RemovingStale)?;
        for invocation in &self.invocations {
            run(invocation).map_err(StartFailure::Adding)?;
        }
        Ok(())
    }

    pub fn abandon(self) -> io::Result<()> {
        Ok(())
    }
}

pub fn remove_all() -> io::Result<usize> {
    if remove_links()? {
        run(&flush_invocation())?;
    }
    count_all()
}

pub fn count_all() -> io::Result<usize> {
    dummy_links().map(|names| names.len())
}

fn remove_links() -> io::Result<bool> {
    let names = dummy_links()?;
    let outcomes: Vec<io::Result<String>> = names
        .iter()
        .map(|name| run(&delete_invocation(name)))
        .collect();
    outcomes.into_iter().collect::<io::Result<Vec<_>>>()?;
    Ok(!names.is_empty())
}

fn dummy_links() -> io::Result<Vec<String>> {
    let mut dummies = Vec::new();
    for name in snisplit_link_names(link_names()?) {
        if is_dummy_link(&run(&show_invocation(&name))?) {
            dummies.push(name);
        }
    }
    Ok(dummies)
}

fn link_names() -> io::Result<Vec<String>> {
    fs::read_dir(NET_CLASS_FOLDER)?
        .map(|entry| Ok(entry?.file_name().to_string_lossy().into_owned()))
        .collect()
}

fn snisplit_link_names(names: Vec<String>) -> Vec<String> {
    let mut named: Vec<String> = names
        .into_iter()
        .filter(|name| name.starts_with(LINK_PREFIX))
        .collect();
    named.sort();
    named
}

fn is_dummy_link(link_details: &str) -> bool {
    link_details
        .split(LINK_DETAIL_SEPARATOR)
        .any(|part| part.split_whitespace().next() == Some(LINK_KIND_DUMMY))
}

fn link_for(index: usize) -> io::Result<Link> {
    let host = u8::try_from(index + 1)
        .ok()
        .filter(|host| *host <= LAST_HOST_OCTET)
        .ok_or_else(|| {
            invalid_input(format!(
                "too many profiles: at most {LAST_HOST_OCTET} can be routed"
            ))
        })?;
    let name = format!("{LINK_PREFIX}{index}");
    if name.len() > MAX_LINK_NAME_LEN {
        return Err(invalid_input(format!(
            "link name {name:?} is longer than {MAX_LINK_NAME_LEN} bytes"
        )));
    }
    let [a, b, c] = ADDRESS_NETWORK;
    Ok(Link {
        name,
        address: Ipv4Addr::new(a, b, c, host),
    })
}

fn invocation(tool: Tool, args: &[&str]) -> Invocation {
    Invocation {
        tool,
        args: args.iter().map(|arg| (*arg).to_string()).collect(),
    }
}

fn show_invocation(name: &str) -> Invocation {
    invocation(Tool::Ip, &["-d", "-o", "link", "show", "dev", name])
}

fn delete_invocation(name: &str) -> Invocation {
    invocation(Tool::Ip, &["link", "del", name])
}

fn flush_invocation() -> Invocation {
    invocation(Tool::Resolvectl, &["flush-caches"])
}

fn status_invocation() -> Invocation {
    invocation(Tool::Resolvectl, &["status"])
}

fn add_invocations(link: &Link, rule: &ProfileRule) -> Vec<Invocation> {
    let name = link.name.as_str();
    let address = format!("{}/{ADDRESS_PREFIX_LEN}", link.address);
    let server = rule.server.to_string();
    let server_route = format!("{server}/{ADDRESS_PREFIX_LEN}");
    let domains: Vec<String> = rule
        .suffixes
        .iter()
        .map(|suffix| format!("~{suffix}"))
        .collect();
    let mut domain_args = vec!["domain", name];
    domain_args.extend(domains.iter().map(String::as_str));
    vec![
        invocation(Tool::Ip, &["link", "add", name, "type", LINK_KIND_DUMMY]),
        invocation(Tool::Ip, &["addr", "add", &address, "dev", name]),
        invocation(
            Tool::Ip,
            &[
                "route",
                "add",
                "local",
                &server_route,
                "dev",
                name,
                "table",
                "local",
            ],
        ),
        invocation(Tool::Ip, &["link", "set", name, "up"]),
        invocation(Tool::Resolvectl, &["dns", name, &server]),
        invocation(Tool::Resolvectl, &domain_args),
        invocation(Tool::Resolvectl, &["default-route", name, "no"]),
        invocation(Tool::Resolvectl, &["llmnr", name, "no"]),
        invocation(Tool::Resolvectl, &["mdns", name, "no"]),
        invocation(Tool::Resolvectl, &["dnsovertls", name, "no"]),
        invocation(Tool::Resolvectl, &["dnssec", name, "no"]),
    ]
}

fn start_invocations(rules: &[ProfileRule]) -> io::Result<Vec<Invocation>> {
    if rules.is_empty() {
        return Err(invalid_input("no profiles to add rules for".to_string()));
    }
    for rule in rules {
        namespaces(&rule.suffixes)?;
    }
    let mut invocations = Vec::new();
    for (index, rule) in rules.iter().enumerate() {
        invocations.extend(add_invocations(&link_for(index)?, rule));
    }
    invocations.push(flush_invocation());
    Ok(invocations)
}

fn preflight() -> io::Result<()> {
    check_root(&fs::read_to_string(PROCESS_STATUS_FILE)?)?;
    run(&status_invocation()).map_err(|error| {
        io::Error::other(format!(
            "systemd-resolved is not answering ({error}); {NEEDS_RESOLVED}"
        ))
    })?;
    let text = fs::read_to_string(RESOLV_CONF).unwrap_or_default();
    let link_target = fs::read_link(RESOLV_CONF)
        .ok()
        .map(|target| target.to_string_lossy().into_owned());
    if resolv_conf_reaches_stub(&text, link_target.as_deref()) {
        return Ok(());
    }
    Err(io::Error::other(format!(
        "{RESOLV_CONF} does not send queries to the systemd-resolved stub ({STUB_ADDRESS}); {NEEDS_RESOLVED}"
    )))
}

fn effective_uid(status: &str) -> Option<u32> {
    let line = status.lines().find_map(|line| line.strip_prefix(UID_KEY))?;
    line.split_whitespace()
        .nth(EFFECTIVE_UID_FIELD)?
        .parse()
        .ok()
}

fn check_root(status: &str) -> io::Result<()> {
    match effective_uid(status) {
        Some(ROOT_UID) => Ok(()),
        Some(_) => Err(io::Error::new(io::ErrorKind::PermissionDenied, NEEDS_ROOT)),
        None => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("cannot read the effective user id from {PROCESS_STATUS_FILE}"),
        )),
    }
}

fn resolv_conf_reaches_stub(text: &str, link_target: Option<&str>) -> bool {
    text.lines()
        .filter_map(nameserver_address)
        .any(|address| address == STUB_ADDRESS)
        || link_target.is_some_and(|target| resolved_link_target(target) == STUB_RESOLV_CONF)
}

fn resolved_link_target(target: &str) -> String {
    let absolute = if target.starts_with('/') {
        target.to_string()
    } else {
        format!("{RESOLV_CONF_FOLDER}/{target}")
    };
    let mut parts: Vec<&str> = Vec::new();
    for part in absolute.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            name => parts.push(name),
        }
    }
    format!("/{}", parts.join("/"))
}

fn tool_path(tool: Tool, exists: impl Fn(&Path) -> bool) -> io::Result<PathBuf> {
    TOOL_FOLDERS
        .iter()
        .map(|folder| PathBuf::from(format!("{folder}/{}", tool.name())))
        .find(|path| exists(path))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("{} not found in {}", tool.name(), TOOL_FOLDERS.join(", ")),
            )
        })
}

fn tool_outcome(tool: Tool, success: bool, stdout: &[u8], stderr: &[u8]) -> io::Result<String> {
    let stderr = String::from_utf8_lossy(stderr);
    let first_line = stderr.lines().map(str::trim).find(|line| !line.is_empty());
    match (success, first_line) {
        (true, None) => Ok(String::from_utf8_lossy(stdout).into_owned()),
        (_, Some(line)) => Err(io::Error::other(format!("{}: {line}", tool.name()))),
        (false, None) => Err(io::Error::other(format!(
            "{} failed without a message",
            tool.name()
        ))),
    }
}

fn run(invocation: &Invocation) -> io::Result<String> {
    let tool = invocation.tool;
    let program = tool_path(tool, Path::is_file)?;
    let output = Command::new(program)
        .args(&invocation.args)
        .env_clear()
        .env(TOOL_LOCALE.0, TOOL_LOCALE.1)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| io::Error::new(error.kind(), format!("{}: {error}", tool.name())))?;
    tool_outcome(
        tool,
        output.status.success(),
        &output.stdout,
        &output.stderr,
    )
}

#[cfg(test)]
mod tests;
