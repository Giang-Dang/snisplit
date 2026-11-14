use std::process::{Command, Output};

fn snisplit(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_snisplit"))
        .args(args)
        .output()
        .expect("run snisplit")
}

#[cfg(windows)]
#[test]
fn service_run_from_a_terminal_says_it_is_only_for_the_service_control_manager() {
    let output = snisplit(&["service", "run", "--profile", "medium"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(output.stdout.is_empty());
    assert!(
        stderr.starts_with(
            "snisplit: error: snisplit service run is only for the Service Control Manager"
        ),
        "{stderr}"
    );
}

#[cfg(not(windows))]
#[test]
fn service_commands_parse_but_fail_as_windows_only() {
    for (args, name) in [
        (vec!["service", "install", "--profile", "medium"], "install"),
        (vec!["service", "uninstall"], "uninstall"),
        (vec!["service", "run"], "run"),
    ] {
        let output = snisplit(&args);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(1), "{stderr}");
        assert_eq!(
            stderr,
            format!("snisplit: error: snisplit service {name} is Windows only\n")
        );
    }
}
