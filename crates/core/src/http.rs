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
mod tests;
