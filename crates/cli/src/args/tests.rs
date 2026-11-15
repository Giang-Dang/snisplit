use super::*;
use std::net::Ipv4Addr;
use std::path::PathBuf;

const USER_SITES: &str = "user-sites";

fn parsed(args: &[&str]) -> Result<Command, UsageError> {
    parse(
        args.iter().map(|arg| arg.to_string()),
        Some(PathBuf::from(USER_SITES)),
    )
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
            site_folder: Some(PathBuf::from(USER_SITES)),
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
            site_folder: None,
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
        "%APPDATA%\\snisplit\\off.list",
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
    assert_eq!(options.site_folder, None);
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

fn probe_options(args: &[&str]) -> ProbeOptions {
    match parsed(args) {
        Ok(Command::Probe(options)) => options,
        other => panic!("expected probe options, got {other:?}"),
    }
}

fn socket(text: &str) -> SocketAddr {
    text.parse().unwrap()
}

#[test]
fn a_bare_probe_uses_the_system_dns_a_five_second_timeout_and_two_runs() {
    assert_eq!(
        probe_options(&["probe", "example.org"]),
        ProbeOptions {
            domain: "example.org".to_string(),
            upstreams: vec![],
            timeout: Duration::from_secs(5),
            runs: 2,
        }
    );
}

#[test]
fn probe_options_come_in_any_order_and_the_domain_is_lowercased() {
    assert_eq!(
        probe_options(&[
            "probe",
            "--runs",
            "3",
            "Store.SteamPowered.com",
            "--timeout",
            "10",
            "--upstream",
            "1.1.1.1",
            "--upstream",
            "9.9.9.9:5353",
            "--upstream",
            "2606:4700::1111",
            "--upstream",
            "[::1]:53",
            "--upstream",
            "1.1.1.1:53",
        ]),
        ProbeOptions {
            domain: "store.steampowered.com".to_string(),
            upstreams: vec![
                socket("1.1.1.1:53"),
                socket("9.9.9.9:5353"),
                socket("[2606:4700::1111]:53"),
                socket("[::1]:53"),
            ],
            timeout: Duration::from_secs(10),
            runs: 3,
        }
    );
}

#[test]
fn probe_needs_exactly_one_domain_that_a_profile_file_would_accept() {
    assert_eq!(
        parsed(&["probe"]),
        Err(UsageError::MissingValue("probe".to_string()))
    );
    assert_eq!(
        parsed(&["probe", "a.com", "b.com"]),
        Err(UsageError::UnexpectedArgument("b.com".to_string()))
    );
    assert_eq!(
        parsed(&["probe", "localhost"]),
        Err(UsageError::InvalidDomain(
            "bad domain \"localhost\": needs at least 2 labels".to_string()
        ))
    );
    assert_eq!(
        parsed(&["probe", "bad_host.com"]),
        Err(UsageError::InvalidDomain(
            "bad domain \"bad_host.com\": only a-z, 0-9, '-' and '.' are allowed".to_string()
        ))
    );
    assert_eq!(
        parsed(&["probe", "a.com", "--verbose"]),
        Err(UsageError::UnknownOption("--verbose".to_string()))
    );
}

#[test]
fn probe_timeouts_are_whole_seconds_from_1_to_60() {
    assert_eq!(
        probe_options(&["probe", "a.com", "--timeout", "60"]).timeout,
        Duration::from_secs(60)
    );
    assert_eq!(
        probe_options(&["probe", "a.com", "--timeout", "1"]).timeout,
        Duration::from_secs(1)
    );
    for bad in ["0", "61", "1.5", "+5", "", "five"] {
        assert_eq!(
            parsed(&["probe", "a.com", "--timeout", bad]),
            Err(UsageError::InvalidTimeout(bad.to_string())),
            "{bad}"
        );
    }
    assert_eq!(
        UsageError::InvalidTimeout("0".to_string()).to_string(),
        "bad timeout \"0\": whole seconds from 1 to 60"
    );
}

#[test]
fn probe_runs_are_from_1_to_10() {
    assert_eq!(probe_options(&["probe", "a.com", "--runs", "1"]).runs, 1);
    assert_eq!(probe_options(&["probe", "a.com", "--runs", "10"]).runs, 10);
    for bad in ["0", "11", "two", "-1"] {
        assert_eq!(
            parsed(&["probe", "a.com", "--runs", bad]),
            Err(UsageError::InvalidRuns(bad.to_string())),
            "{bad}"
        );
    }
    assert_eq!(
        parsed(&["probe", "a.com", "--runs"]),
        Err(UsageError::MissingValue("--runs".to_string()))
    );
}

#[test]
fn probe_upstreams_need_an_ip_address_and_a_port_that_is_not_0() {
    for bad in ["1.1.1.1:0", "dns.google", "300.1.1.1", "1.1.1.1:", "[::1]"] {
        assert_eq!(
            parsed(&["probe", "a.com", "--upstream", bad]),
            Err(UsageError::InvalidUpstream(bad.to_string())),
            "{bad}"
        );
    }
    assert_eq!(
        parsed(&["probe", "a.com", "--upstream"]),
        Err(UsageError::MissingValue("--upstream".to_string()))
    );
}

#[test]
fn usage_describes_probe_and_its_options() {
    for words in [
        "snisplit probe <domain> [options]",
        "--upstream <ip[:port]>",
        "--timeout <seconds>",
        "--runs <n>",
    ] {
        assert!(USAGE.contains(words), "{words}");
    }
}
