use snisplit_core::http_stream::{HostRespeller, MAX_HEAD, StopReason};

fn respell_in_pieces(pieces: &[&[u8]]) -> (Vec<u8>, HostRespeller) {
    let mut respeller = HostRespeller::new();
    let mut output = Vec::new();
    for piece in pieces {
        respeller.feed(piece, &mut output);
    }
    respeller.finish(&mut output);
    (output, respeller)
}

fn respell(input: &[u8]) -> (Vec<u8>, HostRespeller) {
    respell_in_pieces(&[input])
}

fn feed_once(respeller: &mut HostRespeller, input: &[u8]) -> Vec<u8> {
    let mut output = Vec::new();
    respeller.feed(input, &mut output);
    output
}

#[test]
fn every_spelling_of_host_is_respelled_and_its_value_and_spacing_are_kept() {
    let cases: [(&[u8], &[u8]); 4] = [
        (
            b"GET / HTTP/1.1\r\nHost: medium.com\r\n\r\n",
            b"GET / HTTP/1.1\r\nhoSt: medium.com\r\n\r\n",
        ),
        (
            b"GET / HTTP/1.1\r\nHOST:medium.com\r\n\r\n",
            b"GET / HTTP/1.1\r\nhoSt:medium.com\r\n\r\n",
        ),
        (
            b"GET /a HTTP/1.1\r\nAccept: */*\r\nhost: \t Medium.com:80 \r\n\r\n",
            b"GET /a HTTP/1.1\r\nAccept: */*\r\nhoSt: \t Medium.com:80 \r\n\r\n",
        ),
        (
            b"GET / HTTP/1.0\nUser-Agent: curl\nhOsT: medium.com\n\n",
            b"GET / HTTP/1.0\nUser-Agent: curl\nhoSt: medium.com\n\n",
        ),
    ];
    for (input, expected) in cases {
        let (output, respeller) = respell(input);
        assert_eq!(output, expected, "{}", String::from_utf8_lossy(input));
        assert_eq!(output.len(), input.len());
        assert_eq!(respeller.respelled(), 1);
        assert_eq!(respeller.stopped(), None);
    }
}

#[test]
fn headers_and_targets_that_only_resemble_host_are_left_alone() {
    let input: &[u8] = b"GET /?Host:x HTTP/1.1\r\nX-Forwarded-Host: a\r\nHostname: b\r\n\
        Host : c\r\nReferer: http://x/\r\nX-Note: Host: d\r\n\r\n";

    let (output, respeller) = respell(input);

    assert_eq!(output, input);
    assert_eq!(respeller.respelled(), 0);
    assert_eq!(respeller.stopped(), None);
}

#[test]
fn every_host_line_of_a_head_is_respelled_and_the_head_counts_once() {
    let (output, respeller) =
        respell(b"GET / HTTP/1.1\r\nHost: a.example\r\nAccept: */*\r\nHOST: b.example\r\n\r\n");

    assert_eq!(
        output,
        b"GET / HTTP/1.1\r\nhoSt: a.example\r\nAccept: */*\r\nhoSt: b.example\r\n\r\n"
    );
    assert_eq!(respeller.respelled(), 1);
}

#[test]
fn two_pipelined_requests_in_one_read_are_both_respelled() {
    let (output, respeller) = respell(
        b"GET /1 HTTP/1.1\r\nHost: medium.com\r\n\r\nGET /2 HTTP/1.1\r\nHost: medium.com\r\n\r\n",
    );

    assert_eq!(
        output,
        b"GET /1 HTTP/1.1\r\nhoSt: medium.com\r\n\r\nGET /2 HTTP/1.1\r\nhoSt: medium.com\r\n\r\n"
    );
    assert_eq!(respeller.respelled(), 2);
}

#[test]
fn three_keep_alive_requests_sent_one_after_another_are_each_respelled() {
    let mut respeller = HostRespeller::new();
    let requests: [(&[u8], &[u8]); 3] = [
        (
            b"GET /1 HTTP/1.1\r\nHost: medium.com\r\n\r\n",
            b"GET /1 HTTP/1.1\r\nhoSt: medium.com\r\n\r\n",
        ),
        (
            b"HEAD /2 HTTP/1.1\r\nConnection: keep-alive\r\nHost: medium.com\r\n\r\n",
            b"HEAD /2 HTTP/1.1\r\nConnection: keep-alive\r\nhoSt: medium.com\r\n\r\n",
        ),
        (
            b"GET /3 HTTP/1.1\r\nHOST: medium.com\r\n\r\n",
            b"GET /3 HTTP/1.1\r\nhoSt: medium.com\r\n\r\n",
        ),
    ];
    for (count, (request, expected)) in requests.into_iter().enumerate() {
        assert_eq!(feed_once(&mut respeller, request), expected);
        assert_eq!(respeller.respelled(), count + 1);
    }
}

#[test]
fn a_content_length_body_that_looks_like_a_request_passes_unchanged() {
    let body: &[u8] = b"\r\n\r\nGET / HTTP/1.1\r\nHost: x\r\n\r\n";
    assert_eq!(body.len(), 31);
    let input = [
        b"POST /submit HTTP/1.1\r\nHost: medium.com\r\nContent-Length: 31\r\n\r\n".as_slice(),
        body,
        b"GET /next HTTP/1.1\r\nHost: medium.com\r\n\r\n",
    ]
    .concat();

    let (output, respeller) = respell(&input);

    let expected = [
        b"POST /submit HTTP/1.1\r\nhoSt: medium.com\r\nContent-Length: 31\r\n\r\n".as_slice(),
        b"\r\n\r\nGET / HTTP/1.1\r\nHost: x\r\n\r\n",
        b"GET /next HTTP/1.1\r\nhoSt: medium.com\r\n\r\n",
    ]
    .concat();
    assert_eq!(output, expected);
    assert_eq!(respeller.respelled(), 2);
    assert_eq!(respeller.stopped(), None);
}

#[test]
fn repeated_equal_content_lengths_frame_one_body() {
    let (output, respeller) = respell(
        b"PUT /x HTTP/1.1\r\nContent-Length: 6\r\nHost: m\r\ncontent-length:  6 \r\n\r\n\
          Host: GET /y HTTP/1.1\r\nHost: m\r\n\r\n",
    );

    assert_eq!(
        output,
        b"PUT /x HTTP/1.1\r\nContent-Length: 6\r\nhoSt: m\r\ncontent-length:  6 \r\n\r\n\
          Host: GET /y HTTP/1.1\r\nhoSt: m\r\n\r\n"
    );
    assert_eq!(respeller.respelled(), 2);
    assert_eq!(respeller.stopped(), None);
}

#[test]
fn a_zero_content_length_means_the_next_head_follows_at_once() {
    let (output, respeller) = respell(
        b"POST /a HTTP/1.1\r\nHost: m\r\nContent-Length: 0\r\n\r\nGET /b HTTP/1.1\r\nHost: m\r\n\r\n",
    );

    assert_eq!(
        output,
        b"POST /a HTTP/1.1\r\nhoSt: m\r\nContent-Length: 0\r\n\r\nGET /b HTTP/1.1\r\nhoSt: m\r\n\r\n"
    );
    assert_eq!(respeller.respelled(), 2);
}

#[test]
fn body_bytes_are_sent_as_they_arrive_and_the_head_after_them_is_respelled() {
    let mut respeller = HostRespeller::new();

    assert_eq!(
        feed_once(
            &mut respeller,
            b"POST / HTTP/1.1\r\nHost: m\r\nContent-Length: 5\r\n\r\nHo"
        ),
        b"POST / HTTP/1.1\r\nhoSt: m\r\nContent-Length: 5\r\n\r\nHo"
    );
    assert_eq!(feed_once(&mut respeller, b"st"), b"st");
    assert_eq!(feed_once(&mut respeller, b":GET / HTTP/1.1\r\nHo"), b":");
    assert_eq!(
        feed_once(&mut respeller, b"st: m\r\n\r\n"),
        b"GET / HTTP/1.1\r\nhoSt: m\r\n\r\n"
    );
    assert_eq!(respeller.respelled(), 2);
}

const MIXED_REQUESTS: &[u8] = b"GET /a HTTP/1.1\r\nHost: medium.com\r\nAccept: */*\r\n\r\n\
POST /b HTTP/1.1\r\nhost:medium.com\r\nContent-Length: 12\r\n\r\nHost: body\r\n\
GET /c HTTP/1.1\nHOST: medium.com\n\n";

const MIXED_REQUESTS_RESPELLED: &[u8] =
    b"GET /a HTTP/1.1\r\nhoSt: medium.com\r\nAccept: */*\r\n\r\n\
POST /b HTTP/1.1\r\nhoSt:medium.com\r\nContent-Length: 12\r\n\r\nHost: body\r\n\
GET /c HTTP/1.1\nhoSt: medium.com\n\n";

#[test]
fn the_output_does_not_depend_on_where_the_reads_end() {
    let input = MIXED_REQUESTS;
    for cut in 0..=input.len() {
        let (output, respeller) = respell_in_pieces(&[&input[..cut], &input[cut..]]);
        assert_eq!(output, MIXED_REQUESTS_RESPELLED, "cut at {cut}");
        assert_eq!(respeller.respelled(), 3, "cut at {cut}");
    }
    for first in 0..=input.len() {
        for second in first..=input.len() {
            let pieces = [&input[..first], &input[first..second], &input[second..]];
            let (output, _) = respell_in_pieces(&pieces);
            assert_eq!(
                output, MIXED_REQUESTS_RESPELLED,
                "cuts at {first}, {second}"
            );
        }
    }
    let single_bytes: Vec<&[u8]> = input.chunks(1).collect();
    let (output, respeller) = respell_in_pieces(&single_bytes);
    assert_eq!(output, MIXED_REQUESTS_RESPELLED);
    assert_eq!(respeller.respelled(), 3);
}

#[test]
fn a_head_split_inside_the_host_name_is_held_until_complete() {
    let mut respeller = HostRespeller::new();

    assert_eq!(feed_once(&mut respeller, b"GET / HTTP/1.1\r\nHo"), b"");
    assert_eq!(feed_once(&mut respeller, b"st: medium.com\r\n"), b"");
    assert_eq!(
        feed_once(&mut respeller, b"\r\n"),
        b"GET / HTTP/1.1\r\nhoSt: medium.com\r\n\r\n"
    );
}

#[test]
fn finish_sends_a_partial_head_unchanged() {
    let mut respeller = HostRespeller::new();
    let mut output = Vec::new();
    respeller.feed(
        b"GET /1 HTTP/1.1\r\nHost: m\r\n\r\nGET /2 HTTP/1.1\r\nHost: m\r\nAcc",
        &mut output,
    );
    assert_eq!(output, b"GET /1 HTTP/1.1\r\nhoSt: m\r\n\r\n");

    respeller.finish(&mut output);

    assert_eq!(
        output,
        b"GET /1 HTTP/1.1\r\nhoSt: m\r\n\r\nGET /2 HTTP/1.1\r\nHost: m\r\nAcc"
    );
    assert_eq!(respeller.respelled(), 1);
}

fn assert_stops_after_first_head(input: &[u8], expected: &[u8], reason: StopReason) {
    let (output, respeller) = respell(input);
    assert_eq!(output, expected, "{}", String::from_utf8_lossy(input));
    assert_eq!(respeller.stopped(), Some(reason));
    assert_eq!(respeller.respelled(), 1);
}

#[test]
fn a_transfer_encoding_stops_respelling_after_its_head() {
    assert_stops_after_first_head(
        b"POST / HTTP/1.1\r\nHost: m\r\nTransfer-Encoding: chunked\r\n\r\n\
          7\r\nHost: m\r\n0\r\n\r\nGET / HTTP/1.1\r\nHost: m\r\n\r\n",
        b"POST / HTTP/1.1\r\nhoSt: m\r\nTransfer-Encoding: chunked\r\n\r\n\
          7\r\nHost: m\r\n0\r\n\r\nGET / HTTP/1.1\r\nHost: m\r\n\r\n",
        StopReason::TransferEncoding,
    );
}

#[test]
fn content_lengths_that_differ_or_do_not_parse_stop_respelling_after_their_head() {
    let values: [&[u8]; 8] = [
        b"Content-Length: 3\r\nContent-Length: 4",
        b"Content-Length: abc",
        b"Content-Length: +3",
        b"Content-Length: -3",
        b"Content-Length: 3, 3",
        b"Content-Length: 3 3",
        b"Content-Length:",
        b"Content-Length: 18446744073709551616",
    ];
    for value in values {
        let tail: &[u8] = b"\r\n\r\nabcGET / HTTP/1.1\r\nHost: m\r\n\r\n";
        let input = [b"POST / HTTP/1.1\r\nHost: m\r\n".as_slice(), value, tail].concat();
        let expected = [b"POST / HTTP/1.1\r\nhoSt: m\r\n".as_slice(), value, tail].concat();
        assert_stops_after_first_head(&input, &expected, StopReason::ContentLength);
    }
}

#[test]
fn the_largest_content_length_still_frames_a_body() {
    let mut respeller = HostRespeller::new();

    let output = feed_once(
        &mut respeller,
        b"PUT / HTTP/1.1\r\nHost: m\r\nContent-Length: 18446744073709551615\r\n\r\nGET / HTTP/1.1\r\nHost: m\r\n\r\n",
    );

    assert_eq!(
        output,
        b"PUT / HTTP/1.1\r\nhoSt: m\r\nContent-Length: 18446744073709551615\r\n\r\nGET / HTTP/1.1\r\nHost: m\r\n\r\n"
    );
    assert_eq!(respeller.stopped(), None);
    assert_eq!(respeller.respelled(), 1);
}

#[test]
fn an_upgrade_stops_respelling_after_its_head() {
    assert_stops_after_first_head(
        b"GET /chat HTTP/1.1\r\nHost: m\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n\
          GET / HTTP/1.1\r\nHost: m\r\n\r\n",
        b"GET /chat HTTP/1.1\r\nhoSt: m\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n\
          GET / HTTP/1.1\r\nHost: m\r\n\r\n",
        StopReason::Upgrade,
    );
}

#[test]
fn a_connect_stops_respelling_after_its_head() {
    assert_stops_after_first_head(
        b"CONNECT medium.com:443 HTTP/1.1\r\nHost: medium.com:443\r\n\r\nHost: tunnel bytes\r\n\r\n",
        b"CONNECT medium.com:443 HTTP/1.1\r\nhoSt: medium.com:443\r\n\r\nHost: tunnel bytes\r\n\r\n",
        StopReason::Connect,
    );
}

#[test]
fn after_a_stop_later_reads_pass_straight_through() {
    let mut respeller = HostRespeller::new();
    feed_once(
        &mut respeller,
        b"GET / HTTP/1.1\r\nHost: m\r\nUpgrade: h2c\r\n\r\n",
    );

    assert_eq!(
        feed_once(&mut respeller, b"GET / HTTP/1.1\r\nHo"),
        b"GET / HTTP/1.1\r\nHo"
    );
    assert_eq!(
        feed_once(&mut respeller, b"st: m\r\n\r\n"),
        b"st: m\r\n\r\n"
    );
    assert_eq!(respeller.stopped(), Some(StopReason::Upgrade));
}

fn head_of_len(host_name: &str, len: usize) -> Vec<u8> {
    let start = format!("GET / HTTP/1.1\r\n{host_name}: medium.com\r\nX-Pad: ");
    let end = "\r\n\r\n";
    let padding = "p".repeat(len - start.len() - end.len());
    format!("{start}{padding}{end}").into_bytes()
}

#[test]
fn a_head_of_exactly_the_limit_is_respelled() {
    let input = head_of_len("Host", MAX_HEAD);
    let expected = head_of_len("hoSt", MAX_HEAD);
    assert_eq!(input.len(), 16384);

    let (output, respeller) = respell(&input);

    assert_eq!(output, expected);
    assert_eq!(respeller.stopped(), None);
}

#[test]
fn a_head_over_the_limit_is_sent_unchanged_however_it_is_read() {
    let input = [
        head_of_len("Host", MAX_HEAD + 1),
        b"GET / HTTP/1.1\r\nHost: m\r\n\r\n".to_vec(),
    ]
    .concat();
    assert_eq!(input.len(), 16385 + 27);
    for piece_len in [1, 1000, MAX_HEAD, input.len()] {
        let pieces: Vec<&[u8]> = input.chunks(piece_len).collect();
        let (output, respeller) = respell_in_pieces(&pieces);
        assert_eq!(output, input, "pieces of {piece_len}");
        assert_eq!(respeller.stopped(), Some(StopReason::HeadTooLong));
        assert_eq!(respeller.respelled(), 0);
    }
}

#[test]
fn a_head_at_the_limit_is_held_and_one_more_byte_releases_it_unchanged() {
    let input = head_of_len("Host", MAX_HEAD + 1);
    let mut respeller = HostRespeller::new();

    assert_eq!(feed_once(&mut respeller, &input[..MAX_HEAD]), b"");
    assert_eq!(feed_once(&mut respeller, &input[MAX_HEAD..]), input);
    assert_eq!(respeller.stopped(), Some(StopReason::HeadTooLong));
}

#[test]
fn a_request_line_that_is_not_http_1_stops_respelling_and_passes_unchanged() {
    let request_lines: [&[u8]; 8] = [
        b"GET / HTTP/2.0",
        b"GET / HTTP/1.10",
        b"GET / http/1.1",
        b"GET  / HTTP/1.1",
        b"GET /a b HTTP/1.1",
        b"GET /",
        b"G(T / HTTP/1.1",
        b"",
    ];
    for request_line in request_lines {
        let input = [
            b"GET / HTTP/1.1\r\nHost: m\r\n\r\n".as_slice(),
            request_line,
            b"\r\nHost: m\r\n\r\nGET / HTTP/1.1\r\nHost: m\r\n\r\n",
        ]
        .concat();
        let expected = [
            b"GET / HTTP/1.1\r\nhoSt: m\r\n\r\n".as_slice(),
            request_line,
            b"\r\nHost: m\r\n\r\nGET / HTTP/1.1\r\nHost: m\r\n\r\n",
        ]
        .concat();
        assert_stops_after_first_head(&input, &expected, StopReason::NotHttp);
    }
}

#[test]
fn a_bad_request_line_is_released_without_waiting_for_the_end_of_its_head() {
    let mut respeller = HostRespeller::new();

    let output = feed_once(&mut respeller, b"\x16\x03\x01 not http\r\nHost: m");

    assert_eq!(output, b"\x16\x03\x01 not http\r\nHost: m");
    assert_eq!(respeller.stopped(), Some(StopReason::NotHttp));
    assert_eq!(respeller.respelled(), 0);
}

#[test]
fn request_lines_of_any_http_1_shape_are_respelled() {
    let request_lines: [&[u8]; 4] = [
        b"OPTIONS * HTTP/1.1",
        b"M-SEARCH * HTTP/1.1",
        b"GET http://medium.com/a?b=c HTTP/1.0",
        b"PATCH /x HTTP/1.9",
    ];
    for request_line in request_lines {
        let input = [request_line, b"\r\nHost: m\r\n\r\n".as_slice()].concat();
        let expected = [request_line, b"\r\nhoSt: m\r\n\r\n".as_slice()].concat();

        let (output, respeller) = respell(&input);

        assert_eq!(
            output,
            expected,
            "{}",
            String::from_utf8_lossy(request_line)
        );
        assert_eq!(respeller.stopped(), None);
    }
}
