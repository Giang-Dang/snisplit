use super::*;

fn parsed(args: &[&str]) -> Result<Command, UsageError> {
    parse(args.iter().map(|arg| arg.to_string()))
}

fn run_options(args: &[&str]) -> RunOptions {
    match parsed(args) {
        Ok(Command::Run(options)) => options,
        other => panic!("expected run options, got {other:?}"),
    }
}

#[test]
fn plain_run_relays_every_built_in_profile_with_routing_and_system_dns() {
    assert_eq!(
        run_options(&["run"]),
        RunOptions {
            verbose: false,
            profiles: vec!["steam", "medium"],
            routing: true,
            upstreams: vec![],
        }
    );
}

#[test]
fn run_options_are_read_in_any_order() {
    assert_eq!(
        run_options(&[
            "run",
            "--upstream",
            "9.9.9.9",
            "-v",
            "--profile",
            "medium",
            "--no-routing",
            "--upstream",
            "1.0.0.1",
        ]),
        RunOptions {
            verbose: true,
            profiles: vec!["medium"],
            routing: false,
            upstreams: vec![Ipv4Addr::new(9, 9, 9, 9), Ipv4Addr::new(1, 0, 0, 1)],
        }
    );
}

#[test]
fn a_profile_named_twice_is_relayed_once_in_the_order_given() {
    assert_eq!(
        run_options(&[
            "run",
            "--profile",
            "medium",
            "--profile",
            "steam",
            "--profile",
            "medium"
        ])
        .profiles,
        vec!["medium", "steam"]
    );
}

#[test]
fn bad_option_values_are_usage_errors() {
    assert_eq!(
        parsed(&["run", "--profile", "netflix"]),
        Err(UsageError::UnknownProfile("netflix".to_string()))
    );
    assert_eq!(
        parsed(&["run", "--profile"]),
        Err(UsageError::MissingValue("--profile".to_string()))
    );
    assert_eq!(
        parsed(&["run", "--upstream", "dns.google"]),
        Err(UsageError::InvalidAddress("dns.google".to_string()))
    );
    assert_eq!(
        parsed(&["run", "--upstream", "2606:4700::1111"]),
        Err(UsageError::InvalidAddress("2606:4700::1111".to_string()))
    );
    assert_eq!(
        parsed(&["run", "--upstream"]),
        Err(UsageError::MissingValue("--upstream".to_string()))
    );
}

#[test]
fn cleanup_version_and_help_have_their_spellings() {
    assert_eq!(parsed(&["cleanup"]), Ok(Command::Cleanup));
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
        parsed(&["cleanup", "all"]),
        Err(UsageError::UnexpectedArgument("all".to_string()))
    );
    assert_eq!(
        parsed(&["--version", "run"]),
        Err(UsageError::UnexpectedArgument("run".to_string()))
    );
}

#[test]
fn usage_text_names_every_command_and_option() {
    for word in [
        "snisplit run",
        "snisplit cleanup",
        "--profile",
        "--no-routing",
        "--upstream",
        "--verbose",
        "--version",
        "snisplit help",
    ] {
        assert!(USAGE.contains(word), "{word}");
    }
}
