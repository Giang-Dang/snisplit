use std::fmt;

pub const USAGE: &str = "usage:
    snisplit run [--verbose]    divert and split traffic of the listed domains
    snisplit --version          print the version
    snisplit help               print this text

options:
    -v, --verbose               log every split packet";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Run { verbose: bool },
    Version,
    Help,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UsageError {
    MissingCommand,
    UnknownCommand(String),
    UnknownOption(String),
    UnexpectedArgument(String),
}

impl fmt::Display for UsageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingCommand => write!(f, "no command given"),
            Self::UnknownCommand(name) => write!(f, "unknown command: {name}"),
            Self::UnknownOption(name) => write!(f, "unknown option: {name}"),
            Self::UnexpectedArgument(name) => write!(f, "unexpected argument: {name}"),
        }
    }
}

pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Command, UsageError> {
    let mut args = args.into_iter();
    let command = args.next().ok_or(UsageError::MissingCommand)?;
    let parsed = match command.as_str() {
        "run" => return parse_run_options(args),
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

fn parse_run_options(args: impl Iterator<Item = String>) -> Result<Command, UsageError> {
    let mut verbose = false;
    for arg in args {
        match arg.as_str() {
            "--verbose" | "-v" => verbose = true,
            _ if arg.starts_with('-') => return Err(UsageError::UnknownOption(arg)),
            _ => return Err(UsageError::UnexpectedArgument(arg)),
        }
    }
    Ok(Command::Run { verbose })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(args: &[&str]) -> Result<Command, UsageError> {
        parse(args.iter().map(|arg| arg.to_string()))
    }

    #[test]
    fn run_is_quiet_unless_verbose_is_given() {
        assert_eq!(parsed(&["run"]), Ok(Command::Run { verbose: false }));
        assert_eq!(
            parsed(&["run", "--verbose"]),
            Ok(Command::Run { verbose: true })
        );
        assert_eq!(parsed(&["run", "-v"]), Ok(Command::Run { verbose: true }));
    }

    #[test]
    fn version_and_help_have_their_spellings() {
        assert_eq!(parsed(&["--version"]), Ok(Command::Version));
        assert_eq!(parsed(&["-V"]), Ok(Command::Version));
        assert_eq!(parsed(&["help"]), Ok(Command::Help));
        assert_eq!(parsed(&["--help"]), Ok(Command::Help));
        assert_eq!(parsed(&["-h"]), Ok(Command::Help));
    }

    #[test]
    fn no_arguments_is_a_usage_error() {
        assert_eq!(parsed(&[]), Err(UsageError::MissingCommand));
    }

    #[test]
    fn unknown_commands_and_options_are_usage_errors() {
        assert_eq!(
            parsed(&["start"]),
            Err(UsageError::UnknownCommand("start".to_string()))
        );
        assert_eq!(
            parsed(&["--bogus"]),
            Err(UsageError::UnknownOption("--bogus".to_string()))
        );
        assert_eq!(
            parsed(&["run", "--quiet"]),
            Err(UsageError::UnknownOption("--quiet".to_string()))
        );
    }

    #[test]
    fn extra_arguments_are_usage_errors_not_ignored() {
        assert_eq!(
            parsed(&["run", "now"]),
            Err(UsageError::UnexpectedArgument("now".to_string()))
        );
        assert_eq!(
            parsed(&["--version", "run"]),
            Err(UsageError::UnexpectedArgument("run".to_string()))
        );
        assert_eq!(
            parsed(&["help", "run"]),
            Err(UsageError::UnexpectedArgument("run".to_string()))
        );
    }

    #[test]
    fn usage_text_names_every_command() {
        for word in ["snisplit run", "--verbose", "--version", "snisplit help"] {
            assert!(USAGE.contains(word), "{word}");
        }
    }
}
