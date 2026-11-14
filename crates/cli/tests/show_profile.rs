use std::process::{Command, Output};

const MEDIUM_FILE: &str = "name medium\ndomains medium.com\ntls-ports 443\ntls-split 1\nhttp-ports 80\nhttp-method host-spell\n";

fn snisplit(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_snisplit"))
        .args(args)
        .output()
        .expect("run snisplit")
}

#[test]
fn show_profile_prints_the_medium_file_byte_for_byte() {
    let output = snisplit(&["show-profile", "medium"]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&output.stdout), MEDIUM_FILE);
    assert!(output.stderr.is_empty());
}

#[test]
fn show_profile_prints_the_whole_steam_file_from_its_first_to_its_last_line() {
    let output = snisplit(&["show-profile", "steam"]);
    assert_eq!(output.status.code(), Some(0));
    let text = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 19);
    assert_eq!(lines[0], "name steam");
    assert_eq!(lines[1], "domains steampowered.com");
    assert_eq!(lines[14], "domains valvesoftware.com");
    assert_eq!(lines[15], "tls-ports 443 27015-27030");
    assert_eq!(lines[18], "http-method host-spell");
    assert!(text.ends_with("http-method host-spell\n"));
}

#[test]
fn show_profile_of_an_unknown_name_is_a_usage_error_with_nothing_on_stdout() {
    let output = snisplit(&["show-profile", "netflix"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("unknown profile: netflix"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
