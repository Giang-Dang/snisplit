use std::fmt;
use std::net::Ipv4Addr;
use std::path::PathBuf;

use crate::profile;

pub const USAGE: &str = "usage:
    snisplit run [options]      relay the listed domains until Ctrl+C
    snisplit cleanup            remove snisplit's DNS rules (after a crash)
    snisplit show-profile <name>
                                print a built-in profile's file, as a template
    snisplit --version          print the version
    snisplit help               print this text

run options:
    --profile <name>            relay this built-in profile (steam, medium); repeatable
    --profile-file <path>       relay the profile in this file; repeatable
                                (with neither option, every built-in profile runs)
    --no-routing                do not add DNS rules; only the relay and DNS server run
    --upstream <ipv4>           DNS server used to find real addresses; repeatable
                                (default: the system's DNS servers)
    -v, --verbose               log every relayed connection";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOptions {
    pub verbose: bool,
    pub profiles: Vec<&'static str>,
    pub profile_files: Vec<PathBuf>,
    pub routing: bool,
    pub upstreams: Vec<Ipv4Addr>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Run(RunOptions),
    Cleanup,
    ShowProfile(&'static str),
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
        }
    }
}

pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Command, UsageError> {
    let mut args = args.into_iter();
    let command = args.next().ok_or(UsageError::MissingCommand)?;
    let parsed = match command.as_str() {
        "run" => return parse_run_options(args).map(Command::Run),
        "cleanup" => Command::Cleanup,
        "show-profile" => return parse_show_profile(args),
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

fn parse_run_options(mut args: impl Iterator<Item = String>) -> Result<RunOptions, UsageError> {
    let mut options = RunOptions {
        verbose: false,
        profiles: Vec::new(),
        profile_files: Vec::new(),
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

fn value_of(option: &str, value: Option<String>) -> Result<String, UsageError> {
    value.ok_or_else(|| UsageError::MissingValue(option.to_string()))
}

#[cfg(test)]
mod tests;
