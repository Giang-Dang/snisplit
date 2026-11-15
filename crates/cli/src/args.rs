use std::fmt;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::time::Duration;

use snisplit::session::RunOptions;
use snisplit::{DEFAULT_PROBE_RUNS, DEFAULT_PROBE_TIMEOUT, DNS_PORT, parse_domain, profile};

const MAX_PROBE_TIMEOUT_SECONDS: u64 = 60;
const MAX_PROBE_RUNS: usize = 10;

pub const USAGE: &str = "usage:
    snisplit run [options]      relay the listed domains until Ctrl+C or the window closes
    snisplit cleanup            remove snisplit's DNS rules (after a crash)
    snisplit show-profile <name>
                                print a built-in profile's file, as a template
    snisplit probe <domain> [options]
                                test whether the domain is blocked here and which
                                split gets it through; print a profile for it
    snisplit --version          print the version
    snisplit help               print this text

run options:
    --profile <name>            relay this built-in profile (steam, medium); repeatable
    --profile-file <path>       relay the profile in this file; repeatable
                                (with neither option, every built-in profile runs,
                                plus every site file in the user site folder,
                                %APPDATA%\\snisplit\\sites\\<name>.profile on Windows;
                                a site file that does not parse is skipped; a site
                                named in %APPDATA%\\snisplit\\off.list, one name per
                                line, is switched off and not relayed; with every
                                site switched off, run stops before any DNS change)
    --no-routing                do not add DNS rules; only the relay and DNS server run
    --upstream <ipv4>           DNS server used to find real addresses; repeatable
                                (default: the system's DNS servers)
    -v, --verbose               log every relayed connection

probe options (no administrator rights needed; works with or without run):
    --upstream <ip[:port]>      DNS server used to find the real address; repeatable
                                (default: the system's DNS servers)
    --timeout <seconds>         wait this long to connect and for each reply,
                                1-60 (default 5)
    --runs <n>                  send every attempt n times, 1-10 (default 2)";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeOptions {
    pub domain: String,
    pub upstreams: Vec<SocketAddr>,
    pub timeout: Duration,
    pub runs: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Run(RunOptions),
    Cleanup,
    ShowProfile(&'static str),
    Probe(ProbeOptions),
    Version,
    Help,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UsageError {
    MissingCommand,
    UnknownCommand(String),
    UnknownOption(String),
    UnexpectedArgument(String),
    MissingValue(String),
    UnknownProfile(String),
    DuplicateProfileFile(String),
    InvalidAddress(String),
    InvalidDomain(String),
    InvalidUpstream(String),
    InvalidTimeout(String),
    InvalidRuns(String),
}

impl fmt::Display for UsageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingCommand => write!(f, "no command given"),
            Self::UnknownCommand(name) => write!(f, "unknown command: {name}"),
            Self::UnknownOption(name) => write!(f, "unknown option: {name}"),
            Self::UnexpectedArgument(name) => write!(f, "unexpected argument: {name}"),
            Self::MissingValue(option) => write!(f, "{option} needs a value"),
            Self::UnknownProfile(name) => write!(f, "unknown profile: {name}"),
            Self::DuplicateProfileFile(path) => write!(f, "profile file given twice: {path}"),
            Self::InvalidAddress(value) => write!(f, "not an IPv4 address: {value}"),
            Self::InvalidDomain(problem) => write!(f, "{problem}"),
            Self::InvalidUpstream(value) => {
                write!(
                    f,
                    "not an IP address or IP:port with a port from 1: {value}"
                )
            }
            Self::InvalidTimeout(value) => write!(
                f,
                "bad timeout {value:?}: whole seconds from 1 to {MAX_PROBE_TIMEOUT_SECONDS}"
            ),
            Self::InvalidRuns(value) => {
                write!(
                    f,
                    "bad run count {value:?}: a whole number from 1 to {MAX_PROBE_RUNS}"
                )
            }
        }
    }
}

pub fn parse(
    args: impl IntoIterator<Item = String>,
    site_folder: Option<PathBuf>,
) -> Result<Command, UsageError> {
    let mut args = args.into_iter();
    let command = args.next().ok_or(UsageError::MissingCommand)?;
    let parsed = match command.as_str() {
        "run" => return parse_run_options(args, site_folder).map(Command::Run),
        "cleanup" => Command::Cleanup,
        "show-profile" => return parse_show_profile(args),
        "probe" => return parse_probe(args).map(Command::Probe),
        "help" | "--help" | "-h" => Command::Help,
        "--version" | "-V" => Command::Version,
        _ if command.starts_with('-') => return Err(UsageError::UnknownOption(command)),
        _ => return Err(UsageError::UnknownCommand(command)),
    };
    match args.next() {
        Some(extra) => Err(UsageError::UnexpectedArgument(extra)),
        None => Ok(parsed),
    }
}

fn parse_run_options(
    mut args: impl Iterator<Item = String>,
    site_folder: Option<PathBuf>,
) -> Result<RunOptions, UsageError> {
    let mut options = RunOptions {
        verbose: false,
        profiles: Vec::new(),
        profile_files: Vec::new(),
        site_folder: None,
        routing: true,
        upstreams: Vec::new(),
    };
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--verbose" | "-v" => options.verbose = true,
            "--no-routing" => options.routing = false,
            "--profile" => {
                let name = value_of(&arg, args.next())?;
                let built_in = profile::find(&name).ok_or(UsageError::UnknownProfile(name))?;
                if !options.profiles.contains(&built_in.name) {
                    options.profiles.push(built_in.name);
                }
            }
            "--profile-file" => {
                let path = value_of(&arg, args.next())?;
                if options
                    .profile_files
                    .iter()
                    .any(|given| given.as_os_str() == path.as_str())
                {
                    return Err(UsageError::DuplicateProfileFile(path));
                }
                options.profile_files.push(PathBuf::from(path));
            }
            "--upstream" => {
                let value = value_of(&arg, args.next())?;
                let address = value
                    .parse()
                    .map_err(|_| UsageError::InvalidAddress(value))?;
                options.upstreams.push(address);
            }
            _ if arg.starts_with('-') => return Err(UsageError::UnknownOption(arg)),
            _ => return Err(UsageError::UnexpectedArgument(arg)),
        }
    }
    if options.profiles.is_empty() && options.profile_files.is_empty() {
        options.profiles = profile::BUILT_IN.iter().map(|b| b.name).collect();
        options.site_folder = site_folder;
    }
    Ok(options)
}

fn parse_show_profile(mut args: impl Iterator<Item = String>) -> Result<Command, UsageError> {
    let name = value_of("show-profile", args.next())?;
    let built_in = profile::find(&name).ok_or(UsageError::UnknownProfile(name))?;
    match args.next() {
        Some(extra) => Err(UsageError::UnexpectedArgument(extra)),
        None => Ok(Command::ShowProfile(built_in.name)),
    }
}

fn parse_probe(mut args: impl Iterator<Item = String>) -> Result<ProbeOptions, UsageError> {
    let mut domain = None;
    let mut upstreams = Vec::new();
    let mut timeout = DEFAULT_PROBE_TIMEOUT;
    let mut runs = DEFAULT_PROBE_RUNS;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--upstream" => {
                let server = parse_upstream(value_of(&arg, args.next())?)?;
                if !upstreams.contains(&server) {
                    upstreams.push(server);
                }
            }
            "--timeout" => timeout = parse_timeout(value_of(&arg, args.next())?)?,
            "--runs" => runs = parse_runs(value_of(&arg, args.next())?)?,
            _ if arg.starts_with('-') => return Err(UsageError::UnknownOption(arg)),
            _ if domain.is_none() => {
                let parsed = parse_domain(&arg)
                    .map_err(|problem| UsageError::InvalidDomain(problem.to_string()))?;
                domain = Some(parsed);
            }
            _ => return Err(UsageError::UnexpectedArgument(arg)),
        }
    }
    Ok(ProbeOptions {
        domain: domain.ok_or_else(|| UsageError::MissingValue("probe".to_string()))?,
        upstreams,
        timeout,
        runs,
    })
}

fn parse_upstream(value: String) -> Result<SocketAddr, UsageError> {
    let server = value
        .parse::<SocketAddr>()
        .ok()
        .or_else(|| {
            value
                .parse::<IpAddr>()
                .ok()
                .map(|address| SocketAddr::new(address, DNS_PORT))
        })
        .filter(|server| server.port() != 0);
    server.ok_or(UsageError::InvalidUpstream(value))
}

fn parse_timeout(value: String) -> Result<Duration, UsageError> {
    whole_number(&value)
        .filter(|seconds| (1..=MAX_PROBE_TIMEOUT_SECONDS).contains(seconds))
        .map(Duration::from_secs)
        .ok_or(UsageError::InvalidTimeout(value))
}

fn parse_runs(value: String) -> Result<usize, UsageError> {
    whole_number(&value)
        .and_then(|runs| usize::try_from(runs).ok())
        .filter(|runs| (1..=MAX_PROBE_RUNS).contains(runs))
        .ok_or(UsageError::InvalidRuns(value))
}

fn whole_number(value: &str) -> Option<u64> {
    let digits = !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit());
    digits.then(|| value.parse().ok()).flatten()
}

fn value_of(option: &str, value: Option<String>) -> Result<String, UsageError> {
    value.ok_or_else(|| UsageError::MissingValue(option.to_string()))
}

#[cfg(test)]
mod tests;
