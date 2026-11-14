use super::*;
use std::path::PathBuf;

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
            profile_files: vec![],
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
            profile_files: vec![],
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
        "snisplit show-profile",
        "--profile",
        "--profile-file",
        "--no-routing",
        "--upstream",
        "--verbose",
        "--version",
        "snisplit help",
    ] {
        assert!(USAGE.contains(word), "{word}");
    }
}

#[test]
fn show_profile_takes_exactly_one_built_in_name() {
    assert_eq!(
        parsed(&["show-profile", "steam"]),
        Ok(Command::ShowProfile("steam"))
    );
    assert_eq!(
        parsed(&["show-profile", "medium"]),
        Ok(Command::ShowProfile("medium"))
    );
    assert_eq!(
        parsed(&["show-profile", "netflix"]),
        Err(UsageError::UnknownProfile("netflix".to_string()))
    );
    assert_eq!(
        parsed(&["show-profile"]),
        Err(UsageError::MissingValue("show-profile".to_string()))
    );
    assert_eq!(
        parsed(&["show-profile", "steam", "medium"]),
        Err(UsageError::UnexpectedArgument("medium".to_string()))
    );
}

#[test]
fn profile_files_keep_the_command_line_order_and_are_paths_not_read() {
    let options = run_options(&[
        "run",
        "--profile-file",
        "dir/b.profile",
        "--profile-file",
        "a.profile",
    ]);
    assert_eq!(
        options.profile_files,
        vec![PathBuf::from("dir/b.profile"), PathBuf::from("a.profile")]
    );
}

#[test]
fn a_profile_file_alone_does_not_bring_the_built_ins_along() {
    let options = run_options(&["run", "--profile-file", "a.profile"]);
    assert!(options.profiles.is_empty());
    assert_eq!(options.profile_files, vec![PathBuf::from("a.profile")]);
}

#[test]
fn built_ins_and_files_can_be_mixed_and_only_the_given_built_ins_run() {
    let options = run_options(&["run", "--profile-file", "a.profile", "--profile", "medium"]);
    assert_eq!(options.profiles, vec!["medium"]);
    assert_eq!(options.profile_files, vec![PathBuf::from("a.profile")]);
}

#[test]
fn the_same_profile_file_twice_is_a_usage_error() {
    assert_eq!(
        parsed(&[
            "run",
            "--profile-file",
            "a.profile",
            "--profile-file",
            "a.profile"
        ]),
        Err(UsageError::DuplicateProfileFile("a.profile".to_string()))
    );
}

#[test]
fn a_profile_file_option_without_a_path_is_a_usage_error() {
    assert_eq!(
        parsed(&["run", "--profile-file"]),
        Err(UsageError::MissingValue("--profile-file".to_string()))
    );
}

#[test]
fn service_install_takes_the_run_options_in_the_order_given() {
    assert_eq!(
        parsed(&[
            "service",
            "install",
            "--profile-file",
            "a.profile",
            "--profile",
            "medium",
            "--upstream",
            "9.9.9.9",
            "--verbose",
        ]),
        Ok(Command::Service(ServiceCommand::Install(RunOptions {
            verbose: true,
            profiles: vec!["medium"],
            profile_files: vec![PathBuf::from("a.profile")],
            routing: true,
            upstreams: vec![Ipv4Addr::new(9, 9, 9, 9)],
        })))
    );
}

#[test]
fn service_run_without_options_relays_every_built_in_profile_with_routing() {
    assert_eq!(
        parsed(&["service", "run"]),
        Ok(Command::Service(ServiceCommand::Run(RunOptions {
            verbose: false,
            profiles: vec!["steam", "medium"],
            profile_files: vec![],
            routing: true,
            upstreams: vec![],
        })))
    );
}

#[test]
fn service_commands_refuse_no_routing() {
    assert_eq!(
        parsed(&["service", "install", "--no-routing"]),
        Err(UsageError::NotForService("--no-routing"))
    );
    assert_eq!(
        parsed(&["service", "run", "--profile", "medium", "--no-routing"]),
        Err(UsageError::NotForService("--no-routing"))
    );
}

#[test]
fn service_uninstall_takes_no_arguments() {
    assert_eq!(
        parsed(&["service", "uninstall"]),
        Ok(Command::Service(ServiceCommand::Uninstall))
    );
    assert_eq!(
        parsed(&["service", "uninstall", "--profile", "medium"]),
        Err(UsageError::UnexpectedArgument("--profile".to_string()))
    );
}

#[test]
fn service_needs_a_known_command() {
    assert_eq!(parsed(&["service"]), Err(UsageError::MissingServiceCommand));
    assert_eq!(
        parsed(&["service", "start"]),
        Err(UsageError::UnknownServiceCommand("start".to_string()))
    );
    assert_eq!(
        parsed(&["service", "install", "--bogus"]),
        Err(UsageError::UnknownOption("--bogus".to_string()))
    );
}

#[test]
fn usage_text_names_every_service_command_and_the_log_file() {
    for word in [
        "snisplit service install",
        "snisplit service uninstall",
        "snisplit service run",
        "Service Control Manager",
        r"%ProgramFiles%\snisplit\logs\snisplit.log",
    ] {
        assert!(USAGE.contains(word), "{word}");
    }
}
