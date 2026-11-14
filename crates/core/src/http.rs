use core::ops::Range;

use crate::inspect::Inspection;

const METHODS: [&[u8]; 9] = [
    b"GET", b"POST", b"HEAD", b"PUT", b"DELETE", b"OPTIONS", b"PATCH", b"CONNECT", b"TRACE",
];
const METHOD_SEPARATOR: u8 = b' ';
const LINE_FEED: u8 = b'\n';
const CARRIAGE_RETURN: u8 = b'\r';
const HEADER_NAME_SEPARATOR: u8 = b':';
const PORT_SEPARATOR: u8 = b':';
const IPV6_LITERAL_START: u8 = b'[';
const HOST_HEADER_NAME: &[u8] = b"Host";

pub fn inspect_request(request_start: &[u8]) -> Inspection {
    match request_method(request_start) {
        MethodMatch::Complete => {}
        MethodMatch::Partial => return Inspection::NeedMoreData,
        MethodMatch::Mismatch => return Inspection::NoHost,
    }
    let mut lines = CompleteLines::new(request_start);
    let _request_line = lines.next();
    for line in lines {
        if line.is_empty() {
            return Inspection::NoHost;
        }
        if let Some(value) = host_header_value(request_start, line) {
            return host_without_port(request_start, value);
        }
    }
    Inspection::NeedMoreData
}

enum MethodMatch {
    Complete,
    Partial,
    Mismatch,
}

fn request_method(request: &[u8]) -> MethodMatch {
    let mut partial = false;
    for method in METHODS {
        let token = method.iter().chain([&METHOD_SEPARATOR]);
        let mut compared = request.iter().zip(token);
        if compared.all(|(actual, expected)| actual == expected) {
            if request.len() > method.len() {
                return MethodMatch::Complete;
            }
            partial = true;
        }
    }
    if partial {
        MethodMatch::Partial
    } else {
        MethodMatch::Mismatch
    }
}

struct CompleteLines<'a> {
    bytes: &'a [u8],
    next_start: usize,
}

impl<'a> CompleteLines<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            next_start: 0,
        }
    }
}

impl Iterator for CompleteLines<'_> {
    type Item = Range<usize>;

    fn next(&mut self) -> Option<Range<usize>> {
        let start = self.next_start;
        let rest = self.bytes.get(start..)?;
        let line_feed = start + rest.iter().position(|&byte| byte == LINE_FEED)?;
        self.next_start = line_feed + 1;
        let line_bytes = self.bytes.get(start..line_feed)?;
        let end = line_feed - usize::from(line_bytes.last() == Some(&CARRIAGE_RETURN));
        Some(start..end)
    }
}

fn host_header_value(request: &[u8], line: Range<usize>) -> Option<Range<usize>> {
    let line_bytes = request.get(line.clone())?;
    let separator = line_bytes
        .iter()
        .position(|&byte| byte == HEADER_NAME_SEPARATOR)?;
    let name = line_bytes.get(..separator)?;
    if !name.eq_ignore_ascii_case(HOST_HEADER_NAME) {
        return None;
    }
    Some(trim_whitespace(
        request,
        line.start + separator + 1..line.end,
    ))
}

fn is_whitespace(byte: u8) -> bool {
    byte == b' ' || byte == b'\t'
}

fn trim_whitespace(bytes: &[u8], range: Range<usize>) -> Range<usize> {
    let value = bytes.get(range.clone()).unwrap_or_default();
    let leading = value
        .iter()
        .take_while(|&&byte| is_whitespace(byte))
        .count();
    let trailing = value
        .iter()
        .rev()
        .take_while(|&&byte| is_whitespace(byte))
        .count();
    let start = range.start + leading;
    start..range.end.saturating_sub(trailing).max(start)
}

fn host_without_port(request: &[u8], value: Range<usize>) -> Inspection {
    let Some(value_bytes) = request.get(value.clone()) else {
        return Inspection::NoHost;
    };
    let host_len = value_bytes
        .iter()
        .position(|&byte| byte == PORT_SEPARATOR)
        .unwrap_or(value_bytes.len());
    let host = value_bytes.get(..host_len).unwrap_or_default();
    let port = value_bytes.get(host_len + 1..).unwrap_or_default();
    let valid = !host.is_empty()
        && host.first() != Some(&IPV6_LITERAL_START)
        && host.iter().all(u8::is_ascii_graphic)
        && port.iter().all(u8::is_ascii_digit);
    if valid {
        Inspection::Host(value.start..value.start + host_len)
    } else {
        Inspection::NoHost
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOST: &str = "store.steampowered.com";
    const CRLF: &str = "\r\n";

    fn request_with(request_line: &str, headers: &[&str]) -> Vec<u8> {
        let mut text = format!("{request_line}{CRLF}");
        for header in headers {
            text.push_str(header);
            text.push_str(CRLF);
        }
        text.push_str(CRLF);
        text.into_bytes()
    }

    fn get(headers: &[&str]) -> Vec<u8> {
        request_with("GET /app/730 HTTP/1.1", headers)
    }

    fn position_of(haystack: &[u8], needle: &[u8]) -> Range<usize> {
        let start = haystack
            .windows(needle.len())
            .position(|window| window == needle)
            .unwrap();
        start..start + needle.len()
    }

    fn host_of(request: &[u8]) -> Option<&[u8]> {
        match inspect_request(request) {
            Inspection::Host(host) => request.get(host),
            Inspection::NeedMoreData | Inspection::NoHost => None,
        }
    }

    struct XorShift(u32);

    impl XorShift {
        fn next(&mut self) -> u32 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 17;
            self.0 ^= self.0 << 5;
            self.0
        }

        fn below(&mut self, bound: usize) -> usize {
            self.next() as usize % bound.max(1)
        }
    }

    #[test]
    fn finds_host_header() {
        let request = get(&["User-Agent: Valve/Steam", &format!("Host: {HOST}")]);
        assert_eq!(
            inspect_request(&request),
            Inspection::Host(position_of(&request, HOST.as_bytes()))
        );
    }

    #[test]
    fn every_method_is_recognised() {
        for method in METHODS {
            let method = core::str::from_utf8(method).unwrap();
            let request =
                request_with(&format!("{method} / HTTP/1.1"), &[&format!("Host: {HOST}")]);
            assert_eq!(host_of(&request), Some(HOST.as_bytes()), "{method}");
        }
    }

    #[test]
    fn header_name_is_case_insensitive() {
        for name in ["HOST", "host", "hOsT"] {
            let request = get(&[&format!("{name}: {HOST}")]);
            assert_eq!(host_of(&request), Some(HOST.as_bytes()), "{name}");
        }
    }

    #[test]
    fn port_and_surrounding_whitespace_are_excluded() {
        for header in [
            format!("Host: {HOST}:8080"),
            format!("Host:{HOST}"),
            format!("Host: \t{HOST} \t"),
            format!("Host:  {HOST}:80  "),
            format!("Host: {HOST}:"),
        ] {
            let request = get(&[&header]);
            assert_eq!(host_of(&request), Some(HOST.as_bytes()), "{header:?}");
        }
    }

    #[test]
    fn x_forwarded_host_is_ignored() {
        let request = get(&["X-Forwarded-Host: evil.example", &format!("Host: {HOST}")]);
        assert_eq!(host_of(&request), Some(HOST.as_bytes()));
        let only_forwarded = get(&["X-Forwarded-Host: evil.example"]);
        assert_eq!(inspect_request(&only_forwarded), Inspection::NoHost);
    }

    #[test]
    fn host_in_request_target_is_ignored() {
        let request = get(&["Accept: */*"]);
        let with_host_in_target = [b"GET /Host:x".as_slice(), &request[3..]].concat();
        assert_eq!(inspect_request(&with_host_in_target), Inspection::NoHost);
    }

    #[test]
    fn first_host_header_wins() {
        let request = get(&[&format!("Host: {HOST}"), "Host: other.example"]);
        assert_eq!(host_of(&request), Some(HOST.as_bytes()));
    }

    #[test]
    fn bare_line_feeds_end_lines() {
        let request = format!("GET / HTTP/1.1\nHost: {HOST}\n\n").into_bytes();
        assert_eq!(host_of(&request), Some(HOST.as_bytes()));
    }

    #[test]
    fn headers_ending_without_host_are_no_host() {
        let request = get(&["User-Agent: curl/8.0", "Accept: */*"]);
        assert_eq!(inspect_request(&request), Inspection::NoHost);
        let http_1_0 = request_with("GET / HTTP/1.0", &[]);
        assert_eq!(inspect_request(&http_1_0), Inspection::NoHost);
    }

    #[test]
    fn unusable_host_values_are_no_host() {
        for header in [
            "Host:",
            "Host:   ",
            "Host: [::1]:80",
            "Host: [::1]",
            "Host: example.com:http",
            "Host: a b",
            "Host: :80",
        ] {
            let request = get(&[header]);
            assert_eq!(inspect_request(&request), Inspection::NoHost, "{header:?}");
        }
    }

    #[test]
    fn header_name_must_be_exactly_host() {
        for header in [
            format!("Host : {HOST}"),
            format!(" Host: {HOST}"),
            format!("Hostname: {HOST}"),
        ] {
            let request = get(&[&header]);
            assert_eq!(inspect_request(&request), Inspection::NoHost, "{header:?}");
        }
    }

    #[test]
    fn partial_method_needs_more_data() {
        for partial in ["", "G", "GE", "GET", "OPTIO", "DELETE", "C"] {
            assert_eq!(
                inspect_request(partial.as_bytes()),
                Inspection::NeedMoreData,
                "{partial:?}"
            );
        }
    }

    #[test]
    fn other_protocols_are_no_host() {
        for input in [
            b"get / HTTP/1.1\r\n".as_slice(),
            b"GETS / HTTP/1.1\r\n",
            b"GET\t/ HTTP/1.1\r\n",
            b"X",
            b"\x16\x03\x01",
            b"PRI * HTTP/2.0\r\n",
        ] {
            assert_eq!(inspect_request(input), Inspection::NoHost, "{input:?}");
        }
    }

    #[test]
    fn every_prefix_needs_more_data_until_host_line_is_complete() {
        let request = get(&[
            "User-Agent: Valve/Steam HTTP Client 1.0",
            &format!("Host: {HOST}:8080"),
            "Accept: */*",
        ]);
        let host = position_of(&request, HOST.as_bytes());
        let host_line_end = host.end + position_of(&request[host.end..], CRLF.as_bytes()).end;
        for cut in 0..=request.len() {
            let expected = if cut >= host_line_end {
                Inspection::Host(host.clone())
            } else {
                Inspection::NeedMoreData
            };
            assert_eq!(inspect_request(&request[..cut]), expected, "cut at {cut}");
        }
    }

    #[test]
    fn every_prefix_needs_more_data_until_headers_end_without_host() {
        let request = get(&["User-Agent: curl/8.0", "Accept: */*"]);
        for cut in 0..request.len() {
            assert_eq!(
                inspect_request(&request[..cut]),
                Inspection::NeedMoreData,
                "cut at {cut}"
            );
        }
        assert_eq!(inspect_request(&request), Inspection::NoHost);
    }

    #[test]
    fn random_and_mutated_input_never_panics_and_prefixes_agree() {
        let original = get(&[
            "User-Agent: Valve/Steam HTTP Client 1.0",
            &format!("Host: {HOST}:8080"),
            "Accept: */*",
        ]);
        let interesting = b"\r\n: \t[]GHOST".as_slice();
        let mut rng = XorShift(0x2545_f491);
        for _ in 0..12_000 {
            let mut input = if rng.next() % 4 == 0 {
                (0..rng.below(200)).map(|_| rng.next() as u8).collect()
            } else {
                original.clone()
            };
            for _ in 0..=rng.below(3) {
                if input.is_empty() {
                    break;
                }
                let offset = rng.below(input.len());
                input[offset] = if rng.next() % 2 == 0 {
                    interesting[rng.below(interesting.len())]
                } else {
                    rng.next() as u8
                };
            }
            if rng.next() % 4 == 0 {
                input.truncate(rng.below(input.len() + 1));
            }
            let whole = inspect_request(&input);
            if let Inspection::Host(host) = &whole {
                assert!(host.start < host.end && host.end <= input.len());
            }
            for _ in 0..3 {
                let cut = rng.below(input.len() + 1);
                let prefix = inspect_request(&input[..cut]);
                assert!(
                    prefix == Inspection::NeedMoreData || prefix == whole,
                    "cut {cut}: prefix {prefix:?}, whole {whole:?}"
                );
            }
        }
    }
}
