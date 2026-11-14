use super::*;

fn tls(received: &[u8], ended: bool) -> Option<Outcome> {
    reply_outcome(Protocol::Tls, received, ended)
}

fn http(received: &[u8], ended: bool) -> Option<Outcome> {
    reply_outcome(Protocol::Http, received, ended)
}

fn status(line: &str) -> Option<Outcome> {
    Some(Outcome::Answered(Reply::Http {
        status_line: line.to_owned(),
    }))
}

#[test]
fn a_server_hello_record_header_is_a_handshake_answer() {
    assert_eq!(
        tls(&[0x16, 0x03, 0x03, 0x00, 0x7a, 0x02], false),
        Some(Outcome::Answered(Reply::Tls {
            record: RecordType::Handshake,
            version: 0x0303,
        }))
    );
}

#[test]
fn an_alert_record_is_an_answer_of_its_own_type() {
    assert_eq!(
        tls(&[0x15, 0x03, 0x01, 0x00, 0x02, 0x02, 0x28], false),
        Some(Outcome::Answered(Reply::Tls {
            record: RecordType::Alert,
            version: 0x0301,
        }))
    );
}

#[test]
fn a_record_header_cut_short_waits_or_is_unexpected_at_the_end() {
    assert_eq!(tls(&[0x16, 0x03, 0x03], false), None);
    assert_eq!(
        tls(&[0x16, 0x03, 0x03], true),
        Some(Outcome::Unexpected(vec![0x16, 0x03, 0x03]))
    );
}

#[test]
fn implausible_record_headers_are_unexpected() {
    let cases: [&[u8]; 6] = [
        &[0x17, 0x03, 0x03, 0x00, 0x10],
        &[0x16, 0x04, 0x00, 0x00, 0x10],
        &[0x16, 0x03, 0x05, 0x00, 0x10],
        &[0x16, 0x03, 0x03, 0x00, 0x00],
        &[0x16, 0x03, 0x03, 0x48, 0x01],
        b"HTTP/1.1 400 Bad Request\r\n",
    ];
    for case in cases {
        let shown = case[..case.len().min(8)].to_vec();
        assert_eq!(
            tls(case, false),
            Some(Outcome::Unexpected(shown)),
            "{case:?}"
        );
    }
}

#[test]
fn the_longest_ciphertext_record_and_ssl_3_are_still_plausible() {
    assert_eq!(
        tls(&[0x16, 0x03, 0x03, 0x48, 0x00], false),
        Some(Outcome::Answered(Reply::Tls {
            record: RecordType::Handshake,
            version: 0x0303,
        }))
    );
    assert_eq!(
        tls(&[0x15, 0x03, 0x00, 0x00, 0x02], false),
        Some(Outcome::Answered(Reply::Tls {
            record: RecordType::Alert,
            version: 0x0300,
        }))
    );
}

#[test]
fn nothing_received_is_closed_at_the_end_and_pending_before() {
    assert_eq!(tls(&[], true), Some(Outcome::Closed));
    assert_eq!(http(&[], true), Some(Outcome::Closed));
    assert_eq!(tls(&[], false), None);
    assert_eq!(http(&[], false), None);
}

#[test]
fn an_http_reply_is_its_status_line_without_the_line_ending() {
    assert_eq!(
        http(b"HTTP/1.1 302 Found\r\nLocation: /\r\n", false),
        status("HTTP/1.1 302 Found")
    );
    assert_eq!(http(b"HTTP/1.0 200 OK\n", false), status("HTTP/1.0 200 OK"));
}

#[test]
fn a_status_line_without_its_end_waits_until_the_stream_ends() {
    assert_eq!(http(b"HTTP/1.1 403", false), None);
    assert_eq!(http(b"HTTP/1.1 403", true), status("HTTP/1.1 403"));
    assert_eq!(http(b"HTTP/", false), None);
    assert_eq!(
        http(b"HTTP/", true),
        Some(Outcome::Unexpected(b"HTTP/".to_vec()))
    );
}

#[test]
fn replies_that_are_not_http_1_are_unexpected_as_soon_as_they_differ() {
    assert_eq!(
        http(b"<html><body>", false),
        Some(Outcome::Unexpected(b"<html><b".to_vec()))
    );
    assert_eq!(
        http(b"HTTP/2 200\r\n", false),
        Some(Outcome::Unexpected(b"HTTP/2 2".to_vec()))
    );
    assert_eq!(
        http(&[0x16, 0x03], false),
        Some(Outcome::Unexpected(vec![0x16, 0x03]))
    );
}

#[test]
fn an_over_long_status_line_is_cut_and_control_bytes_are_masked() {
    let mut long = b"HTTP/1.1 200 ".to_vec();
    long.resize(250, b'x');
    let Some(Outcome::Answered(Reply::Http { status_line })) = http(&long, false) else {
        panic!("expected an answer");
    };
    assert_eq!(status_line.len(), 200);
    assert!(status_line.starts_with("HTTP/1.1 200 xxx"));
    assert_eq!(
        http(b"HTTP/1.1 200 \x1b[31mOK\xff\r\n", false),
        status("HTTP/1.1 200 ?[31mOK?")
    );
}

#[test]
fn resets_aborts_and_broken_pipes_are_resets_and_other_errors_are_failures() {
    assert_eq!(
        Outcome::of_stream_error(ErrorKind::ConnectionReset),
        Outcome::Reset
    );
    assert_eq!(
        Outcome::of_stream_error(ErrorKind::ConnectionAborted),
        Outcome::Reset
    );
    assert_eq!(
        Outcome::of_stream_error(ErrorKind::BrokenPipe),
        Outcome::Reset
    );
    assert_eq!(
        Outcome::of_stream_error(ErrorKind::PermissionDenied),
        Outcome::Failed(ErrorKind::PermissionDenied)
    );
}

#[test]
fn outcomes_print_as_short_table_cells() {
    let handshake = Outcome::Answered(Reply::Tls {
        record: RecordType::Handshake,
        version: 0x0303,
    });
    assert_eq!(handshake.to_string(), "handshake 0x0303");
    let alert = Outcome::Answered(Reply::Tls {
        record: RecordType::Alert,
        version: 0x0301,
    });
    assert_eq!(alert.to_string(), "alert 0x0301");
    assert_eq!(
        Outcome::Unexpected(vec![0x3c, 0x68]).to_string(),
        "unexpected 3c 68"
    );
    assert_eq!(Outcome::TimedOut.to_string(), "timed out");
    assert_eq!(Outcome::Closed.to_string(), "closed");
    assert_eq!(Outcome::Reset.to_string(), "reset");
    assert_eq!(
        Outcome::ConnectFailed(ErrorKind::TimedOut).to_string(),
        "connect failed: timed out"
    );
}
