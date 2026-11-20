use std::io::ErrorKind;
use std::net::Ipv4Addr;

use super::*;

const SERVER: Ipv4Addr = Ipv4Addr::new(127, 0, 0, 2);

fn rule<'a>(suffixes: &[&'a str], server: Ipv4Addr) -> ProfileRule<'a> {
    ProfileRule {
        suffixes: suffixes.to_vec(),
        server,
    }
}

#[test]
fn the_start_script_removes_stale_rules_waits_then_adds_one_rule_per_profile_and_flushes() {
    let rules = [
        rule(&["steampowered.com", "steam-chat.com"], SERVER),
        rule(&["medium.com"], Ipv4Addr::new(127, 0, 0, 3)),
    ];
    assert_eq!(
        start_script(&rules).unwrap(),
        "$ErrorActionPreference = 'Stop'; \
         Get-DnsClientNrptRule | Where-Object { $_.Comment -eq 'snisplit' } | Remove-DnsClientNrptRule -Force; \
         Write-Output 'stale rules removed'; \
         if ($null -eq [Console]::In.ReadLine()) { exit }; \
         Add-DnsClientNrptRule -Namespace '.steampowered.com', 'steampowered.com', '.steam-chat.com', 'steam-chat.com' -NameServers '127.0.0.2' -Comment 'snisplit' | Out-Null; \
         Add-DnsClientNrptRule -Namespace '.medium.com', 'medium.com' -NameServers '127.0.0.3' -Comment 'snisplit' | Out-Null; \
         Clear-DnsClientCache"
    );
}

#[test]
fn the_start_script_formats_the_server_address() {
    let script = start_script(&[rule(&["a.com"], Ipv4Addr::new(10, 1, 2, 3))]).unwrap();
    assert!(script.ends_with(
        "Add-DnsClientNrptRule -Namespace '.a.com', 'a.com' -NameServers '10.1.2.3' -Comment 'snisplit' | Out-Null; Clear-DnsClientCache"
    ));
}

#[test]
fn the_start_script_needs_a_profile_and_every_profile_a_suffix() {
    assert_eq!(
        start_script(&[]).unwrap_err().kind(),
        ErrorKind::InvalidInput
    );
    let one_empty = [rule(&["a.com"], SERVER), rule(&[], SERVER)];
    assert_eq!(
        start_script(&one_empty).unwrap_err().kind(),
        ErrorKind::InvalidInput
    );
}

#[test]
fn rejected_names_never_reach_a_script() {
    let too_long_label = "a".repeat(64);
    let label_63 = "a".repeat(63);
    let too_long_name = [label_63.as_str(); 4].join(".");
    let rejected = [
        "",
        ".medium.com",
        "medium.com.",
        "a..com",
        "Medium.com",
        "medium.com'; Remove-Item C:\\ #",
        "a b.com",
        "a_b.com",
        "caf\u{e9}.com",
        "a.com\n",
        too_long_label.as_str(),
        too_long_name.as_str(),
    ];
    assert_eq!(too_long_name.len(), 255);
    for name in rejected {
        let rules = [
            rule(&["good.com"], SERVER),
            rule(&["fine.org", name], SERVER),
        ];
        let error = start_script(&rules).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::InvalidInput, "name {name:?}");
    }
}

#[test]
fn accepted_names_at_the_limits() {
    let label_63 = "a".repeat(63);
    let last_label = "a".repeat(61);
    let name_253 = [
        label_63.as_str(),
        label_63.as_str(),
        label_63.as_str(),
        last_label.as_str(),
    ]
    .join(".");
    assert_eq!(name_253.len(), 253);
    let name_254 = format!("{name_253}a");
    for name in [label_63.as_str(), name_253.as_str(), "0-9.x--y", "a"] {
        assert!(
            start_script(&[rule(&[name], SERVER)]).is_ok(),
            "name {name:?}"
        );
    }
    assert!(start_script(&[rule(&[name_254.as_str()], SERVER)]).is_err());
}

#[test]
fn the_stop_script_removes_flushes_and_counts_only_snisplit_rules() {
    assert_eq!(
        stop_script(),
        "$ErrorActionPreference = 'Stop'; \
         Get-DnsClientNrptRule | Where-Object { $_.Comment -eq 'snisplit' } | Remove-DnsClientNrptRule -Force; \
         Clear-DnsClientCache; \
         @(Get-DnsClientNrptRule | Where-Object { $_.Comment -eq 'snisplit' }).Count"
    );
}

#[test]
fn the_count_script_only_counts_snisplit_rules_and_changes_nothing() {
    assert_eq!(
        count_script(),
        "$ErrorActionPreference = 'Stop'; \
         @(Get-DnsClientNrptRule | Where-Object { $_.Comment -eq 'snisplit' }).Count"
    );
}

#[test]
fn parse_count_reads_windows_line_ending() {
    assert_eq!(parse_count("28\r\n").unwrap(), 28);
    assert_eq!(parse_count("0\r\n").unwrap(), 0);
}

#[test]
fn parse_count_rejects_garbage() {
    assert_eq!(parse_count("").unwrap_err().kind(), ErrorKind::InvalidData);
    assert_eq!(
        parse_count("three").unwrap_err().kind(),
        ErrorKind::InvalidData
    );
}

#[test]
fn a_start_that_failed_after_the_stale_removal_failed_while_adding() {
    let stderr = b"Add-DnsClientNrptRule : Access is denied.\r\n";
    match start_outcome(false, b"stale rules removed\r\n", stderr) {
        Err(StartFailure::Adding(error)) => assert_eq!(
            error.to_string(),
            "powershell: Add-DnsClientNrptRule : Access is denied."
        ),
        other => panic!("expected an adding failure, got {other:?}"),
    }
}

#[test]
fn a_start_that_failed_before_the_marker_failed_removing_stale_rules() {
    let stderr = b"Remove-DnsClientNrptRule : Access is denied.\r\n";
    match start_outcome(false, b"", stderr) {
        Err(StartFailure::RemovingStale(error)) => assert_eq!(
            error.to_string(),
            "powershell: Remove-DnsClientNrptRule : Access is denied."
        ),
        other => panic!("expected a stale removal failure, got {other:?}"),
    }
}

#[test]
fn a_start_with_a_clean_exit_succeeded() {
    assert!(start_outcome(true, b"stale rules removed\r\n", b"").is_ok());
}

#[test]
fn outcome_success_returns_stdout() {
    assert_eq!(interpret_outcome(true, b"5\r\n", b"").unwrap(), "5\r\n");
}

#[test]
fn outcome_failure_carries_powershell_message() {
    let error = interpret_outcome(false, b"", b"Access is denied.\r\n").unwrap_err();
    assert_eq!(error.to_string(), "powershell: Access is denied.");
}

#[test]
fn outcome_stderr_with_zero_exit_is_still_an_error() {
    let error = interpret_outcome(true, b"ok", b"warning text").unwrap_err();
    assert_eq!(error.to_string(), "powershell: warning text");
}

#[test]
fn outcome_failure_without_message_is_still_an_error() {
    let error = interpret_outcome(false, b"out", b"").unwrap_err();
    assert_eq!(
        error.to_string(),
        "powershell: powershell failed without a message"
    );
}

#[cfg(windows)]
mod gate {
    use std::thread;
    use std::time::Duration;

    use std::io::Write as _;
    use std::net::Ipv4Addr;
    use std::process::{Command, Stdio};

    use super::super::{
        Gated, PendingStart, ProfileRule, StartFailure, count_script, start_script, stop_script,
    };

    const HARMLESS_GATED: &str = "Write-Output 'first'; if ($null -eq [Console]::In.ReadLine()) { exit }; Write-Output 'second'";
    const CHILD_REACHES_THE_GATE: Duration = Duration::from_millis(1500);

    #[test]
    fn a_released_child_runs_past_the_gate() {
        let child = Gated::spawn(HARMLESS_GATED).unwrap();
        thread::sleep(CHILD_REACHES_THE_GATE);
        let output = child.release().unwrap();
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "first\r\nsecond\r\n"
        );
    }

    #[test]
    fn an_abandoned_child_stops_at_the_gate() {
        let child = Gated::spawn(HARMLESS_GATED).unwrap();
        thread::sleep(CHILD_REACHES_THE_GATE);
        let output = child.abandon().unwrap();
        assert!(output.status.success());
        assert_eq!(String::from_utf8_lossy(&output.stdout), "first\r\n");
    }

    #[test]
    fn a_child_released_before_it_reaches_the_gate_still_runs_past_it() {
        let output = Gated::spawn(HARMLESS_GATED).unwrap().release().unwrap();
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "first\r\nsecond\r\n"
        );
    }

    const FAILS_BEFORE_THE_MARKER: &str = "$ErrorActionPreference = 'Stop'; throw 'boom'; Write-Output 'stale rules removed'; if ($null -eq [Console]::In.ReadLine()) { exit }";
    const FAILS_AFTER_THE_GATE: &str = "$ErrorActionPreference = 'Stop'; Write-Output 'stale rules removed'; if ($null -eq [Console]::In.ReadLine()) { exit }; throw 'boom'";
    const COUNT_PARSE_ERRORS: &str = "$errors = $null; $tokens = $null; [void][System.Management.Automation.Language.Parser]::ParseInput([Console]::In.ReadToEnd(), [ref]$tokens, [ref]$errors); $errors.Count";

    fn first_line(error: &std::io::Error) -> String {
        error
            .to_string()
            .lines()
            .next()
            .unwrap_or_default()
            .to_string()
    }

    #[test]
    fn a_child_that_dies_before_the_marker_failed_removing_stale_rules() {
        let pending = PendingStart(Gated::spawn(FAILS_BEFORE_THE_MARKER).unwrap());
        match pending.add() {
            Err(StartFailure::RemovingStale(error)) => {
                assert_eq!(first_line(&error), "powershell: boom");
            }
            other => panic!("expected a stale removal failure, got {other:?}"),
        }
    }

    #[test]
    fn a_child_that_dies_after_the_gate_failed_while_adding() {
        let pending = PendingStart(Gated::spawn(FAILS_AFTER_THE_GATE).unwrap());
        thread::sleep(CHILD_REACHES_THE_GATE);
        match pending.add() {
            Err(StartFailure::Adding(error)) => {
                assert_eq!(first_line(&error), "powershell: boom");
            }
            other => panic!("expected an adding failure, got {other:?}"),
        }
    }

    #[test]
    fn an_abandoned_child_that_died_before_the_gate_reports_why() {
        let pending = PendingStart(Gated::spawn(FAILS_BEFORE_THE_MARKER).unwrap());
        let error = pending.abandon().unwrap_err();
        assert_eq!(first_line(&error), "powershell: boom");
    }

    fn parse_errors(script: &str) -> usize {
        let mut parser = Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                COUNT_PARSE_ERRORS,
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        parser
            .stdin
            .take()
            .unwrap()
            .write_all(script.as_bytes())
            .unwrap();
        let output = parser.wait_with_output().unwrap();
        String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse()
            .unwrap()
    }

    #[test]
    fn the_powershell_parser_accepts_the_start_stop_and_count_scripts() {
        assert_ne!(parse_errors("if ($true) { 'unclosed'"), 0);
        let rules = [
            ProfileRule {
                suffixes: vec!["steampowered.com", "steam-chat.com"],
                server: Ipv4Addr::new(127, 0, 0, 2),
            },
            ProfileRule {
                suffixes: vec!["medium.com"],
                server: Ipv4Addr::new(127, 0, 0, 3),
            },
        ];
        assert_eq!(parse_errors(&start_script(&rules).unwrap()), 0);
        assert_eq!(parse_errors(&stop_script()), 0);
        assert_eq!(parse_errors(&count_script()), 0);
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
#[test]
fn rule_changes_are_unsupported_off_windows_and_linux() {
    assert_eq!(remove_all().unwrap_err().kind(), ErrorKind::Unsupported);
    assert_eq!(count_all().unwrap_err().kind(), ErrorKind::Unsupported);
    let begun = begin_start(&[rule(&["a.com"], SERVER)]);
    assert_eq!(
        begun.err().map(|error| error.kind()),
        Some(ErrorKind::Unsupported)
    );
}

#[cfg(not(windows))]
#[test]
fn names_are_validated_before_the_platform_check() {
    let begun = begin_start(&[rule(&["A.com"], SERVER)]);
    assert_eq!(
        begun.err().map(|error| error.kind()),
        Some(ErrorKind::InvalidInput)
    );
}
