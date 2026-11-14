use std::net::{Ipv4Addr, UdpSocket};
use std::process::{Command, Output};
use std::thread;

use snisplit_core::dns;

const LOOPBACK: Ipv4Addr = Ipv4Addr::LOCALHOST;

fn snisplit(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_snisplit"))
        .args(args)
        .output()
        .expect("run snisplit")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn probe_without_a_domain_is_a_usage_error() {
    let output = snisplit(&["probe"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(text(&output.stderr).starts_with("snisplit: probe needs a value\n"));
}

#[test]
fn probe_of_a_single_label_name_is_a_usage_error() {
    let output = snisplit(&["probe", "localhost"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        text(&output.stderr).contains("bad domain \"localhost\": needs at least 2 labels"),
        "{}",
        text(&output.stderr)
    );
}

#[test]
fn a_silent_dns_server_is_a_runtime_error_after_the_starting_line() {
    let silent = UdpSocket::bind((LOOPBACK, 0)).unwrap();
    let upstream = silent.local_addr().unwrap().to_string();
    let output = snisplit(&[
        "probe",
        "example.org",
        "--upstream",
        &upstream,
        "--timeout",
        "1",
    ]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        text(&output.stdout),
        format!(
            "probing example.org through DNS {upstream}: 7 attempts, 2 runs each, timeout 1 s\n"
        )
    );
    assert_eq!(
        text(&output.stderr),
        "snisplit: error: no upstream DNS server answered for example.org\n"
    );
}

#[test]
fn a_name_that_resolves_to_loopback_is_not_probed() {
    let server = UdpSocket::bind((LOOPBACK, 0)).unwrap();
    let upstream = server.local_addr().unwrap().to_string();
    let answering = thread::spawn(move || {
        let mut query = [0u8; 512];
        let (length, client) = server.recv_from(&mut query).unwrap();
        let answer = dns::answer_a(&query[..length], LOOPBACK, 60).unwrap();
        server.send_to(&answer, client).unwrap();
    });
    let output = snisplit(&[
        "probe",
        "example.org",
        "--upstream",
        &upstream,
        "--runs",
        "1",
    ]);
    answering.join().unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        text(&output.stderr),
        "snisplit: error: example.org resolved only to loopback, unspecified or broadcast addresses: [127.0.0.1]\n"
    );
}
