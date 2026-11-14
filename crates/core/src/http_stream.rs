use core::ops::Range;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    TransferEncoding,
    ContentLength,
    Upgrade,
    Connect,
    HeadTooLong,
    NotHttp,
}

pub const MAX_HEAD: usize = 16 * 1024;

const LINE_FEED: u8 = b'\n';
const CARRIAGE_RETURN: u8 = b'\r';
const HEADER_NAME_SEPARATOR: u8 = b':';
const REQUEST_LINE_SEPARATOR: u8 = b' ';
const TOKEN_SYMBOLS: &[u8] = b"!#$%&'*+-.^_`|~";
const HTTP_1_VERSION_PREFIX: &[u8] = b"HTTP/1.";
const CONNECT_METHOD: &[u8] = b"CONNECT";
const HOST: &[u8] = b"host";
const RESPELLED_HOST: &[u8] = b"hoSt";
const TRANSFER_ENCODING: &[u8] = b"transfer-encoding";
const CONTENT_LENGTH: &[u8] = b"content-length";
const UPGRADE: &[u8] = b"upgrade";

#[derive(Debug, Default, Clone, Copy)]
enum Phase {
    #[default]
    Head,
    Body {
        remaining: u64,
    },
    PassThrough,
}

#[derive(Debug, Default, Clone, Copy)]
enum LineSoFar {
    #[default]
    Empty,
    CarriageReturn,
    Text,
}

#[derive(Debug, Default)]
pub struct HostRespeller {
    phase: Phase,
    held: Vec<u8>,
    line_so_far: LineSoFar,
    request_line_checked: bool,
    respelled: usize,
    stopped: Option<StopReason>,
}

impl HostRespeller {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn feed(&mut self, input: &[u8], output: &mut Vec<u8>) {
        let mut rest = input;
        while !rest.is_empty() {
            rest = match self.phase {
                Phase::Head => self.feed_head(rest, output),
                Phase::Body { remaining } => self.feed_body(remaining, rest, output),
                Phase::PassThrough => {
                    output.extend_from_slice(rest);
                    &[]
                }
            };
        }
    }

    pub fn finish(&mut self, output: &mut Vec<u8>) {
        output.extend_from_slice(&self.held);
        self.start_next_head();
    }

    pub fn respelled(&self) -> usize {
        self.respelled
    }

    pub fn stopped(&self) -> Option<StopReason> {
        self.stopped
    }

    fn feed_body<'a>(&mut self, remaining: u64, input: &'a [u8], output: &mut Vec<u8>) -> &'a [u8] {
        let taken = usize::try_from(remaining).map_or(input.len(), |left| left.min(input.len()));
        let (body, rest) = input.split_at(taken);
        output.extend_from_slice(body);
        let left = remaining - u64::try_from(taken).unwrap_or(remaining);
        self.phase = if left == 0 {
            Phase::Head
        } else {
            Phase::Body { remaining: left }
        };
        rest
    }

    fn feed_head<'a>(&mut self, input: &'a [u8], output: &mut Vec<u8>) -> &'a [u8] {
        let room = MAX_HEAD - self.held.len();
        let window = &input[..input.len().min(room)];
        let head_end = self.find_head_end(window);
        let (taken, rest) = input.split_at(head_end.unwrap_or(window.len()));
        let new_bytes_start = self.held.len();
        self.held.extend_from_slice(taken);
        if !self.request_line_may_be_http(new_bytes_start) {
            self.stop(StopReason::NotHttp, output);
            return rest;
        }
        match head_end {
            Some(_) => self.complete_head(output),
            None if !rest.is_empty() => self.stop(StopReason::HeadTooLong, output),
            None => {}
        }
        rest
    }

    fn find_head_end(&mut self, window: &[u8]) -> Option<usize> {
        for (index, &byte) in window.iter().enumerate() {
            self.line_so_far = match (byte, self.line_so_far) {
                (LINE_FEED, LineSoFar::Empty | LineSoFar::CarriageReturn) => {
                    return Some(index + 1);
                }
                (LINE_FEED, LineSoFar::Text) => LineSoFar::Empty,
                (CARRIAGE_RETURN, LineSoFar::Empty) => LineSoFar::CarriageReturn,
                _ => LineSoFar::Text,
            };
        }
        None
    }

    fn request_line_may_be_http(&mut self, new_bytes_start: usize) -> bool {
        if self.request_line_checked {
            return true;
        }
        let Some(request_line) = CompleteLines::new(&self.held, new_bytes_start).next() else {
            return true;
        };
        self.request_line_checked = true;
        request_method(&self.held[request_line]).is_some()
    }

    fn complete_head(&mut self, output: &mut Vec<u8>) {
        let outcome = write_respelled_head(&self.held, output);
        if outcome.respelled {
            self.respelled += 1;
        }
        self.start_next_head();
        self.phase = match outcome.next {
            Next::Head => Phase::Head,
            Next::Body(remaining) => Phase::Body { remaining },
            Next::Stop(reason) => {
                self.stopped = Some(reason);
                Phase::PassThrough
            }
        };
    }

    fn stop(&mut self, reason: StopReason, output: &mut Vec<u8>) {
        output.extend_from_slice(&self.held);
        self.start_next_head();
        self.stopped = Some(reason);
        self.phase = Phase::PassThrough;
    }

    fn start_next_head(&mut self) {
        self.held.clear();
        self.line_so_far = LineSoFar::Empty;
        self.request_line_checked = false;
    }
}

enum Next {
    Head,
    Body(u64),
    Stop(StopReason),
}

struct HeadOutcome {
    respelled: bool,
    next: Next,
}

fn write_respelled_head(head: &[u8], output: &mut Vec<u8>) -> HeadOutcome {
    let head_start = output.len();
    output.extend_from_slice(head);
    let mut lines = CompleteLines::new(head, 0);
    let Some(method) = lines.next().and_then(|line| request_method(&head[line])) else {
        return HeadOutcome {
            respelled: false,
            next: Next::Stop(StopReason::NotHttp),
        };
    };
    let mut framing = Framing::default();
    let mut respelled = false;
    for line in lines.take_while(|line| !line.is_empty()) {
        let Some((name, value)) = split_header(&head[line.clone()]) else {
            continue;
        };
        if name.eq_ignore_ascii_case(HOST) {
            let name_in_output = head_start + line.start..head_start + line.start + HOST.len();
            output[name_in_output].copy_from_slice(RESPELLED_HOST);
            respelled = true;
        } else {
            framing.note(name, value);
        }
    }
    HeadOutcome {
        respelled,
        next: framing.next(method),
    }
}

fn split_header(line: &[u8]) -> Option<(&[u8], &[u8])> {
    let separator = line
        .iter()
        .position(|&byte| byte == HEADER_NAME_SEPARATOR)?;
    Some((&line[..separator], &line[separator + 1..]))
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum ContentLength {
    #[default]
    Absent,
    Length(u64),
    Unusable,
}

#[derive(Debug, Default)]
struct Framing {
    transfer_encoding: bool,
    upgrade: bool,
    content_length: ContentLength,
}

impl Framing {
    fn note(&mut self, name: &[u8], value: &[u8]) {
        if name.eq_ignore_ascii_case(TRANSFER_ENCODING) {
            self.transfer_encoding = true;
        } else if name.eq_ignore_ascii_case(UPGRADE) {
            self.upgrade = true;
        } else if name.eq_ignore_ascii_case(CONTENT_LENGTH) {
            self.content_length = match (self.content_length, decimal(trim_whitespace(value))) {
                (ContentLength::Absent, Some(length)) => ContentLength::Length(length),
                (ContentLength::Length(seen), Some(length)) if seen == length => {
                    ContentLength::Length(seen)
                }
                _ => ContentLength::Unusable,
            };
        }
    }

    fn next(&self, method: &[u8]) -> Next {
        if self.transfer_encoding {
            Next::Stop(StopReason::TransferEncoding)
        } else if self.content_length == ContentLength::Unusable {
            Next::Stop(StopReason::ContentLength)
        } else if self.upgrade {
            Next::Stop(StopReason::Upgrade)
        } else if method == CONNECT_METHOD {
            Next::Stop(StopReason::Connect)
        } else {
            match self.content_length {
                ContentLength::Length(length) if length > 0 => Next::Body(length),
                _ => Next::Head,
            }
        }
    }
}

fn request_method(request_line: &[u8]) -> Option<&[u8]> {
    let mut parts = request_line.split(|&byte| byte == REQUEST_LINE_SEPARATOR);
    let (Some(method), Some(target), Some(version), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return None;
    };
    let well_formed = is_token(method) && is_target(target) && is_http_1(version);
    well_formed.then_some(method)
}

fn is_token(bytes: &[u8]) -> bool {
    !bytes.is_empty()
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || TOKEN_SYMBOLS.contains(byte))
}

fn is_target(bytes: &[u8]) -> bool {
    !bytes.is_empty() && bytes.iter().all(u8::is_ascii_graphic)
}

fn is_http_1(version: &[u8]) -> bool {
    version
        .strip_prefix(HTTP_1_VERSION_PREFIX)
        .is_some_and(|minor| matches!(minor, [digit] if digit.is_ascii_digit()))
}

fn decimal(digits: &[u8]) -> Option<u64> {
    if digits.is_empty() {
        return None;
    }
    digits.iter().try_fold(0_u64, |value, &byte| {
        let digit = char::from(byte).to_digit(10)?;
        value.checked_mul(10)?.checked_add(u64::from(digit))
    })
}

fn is_whitespace(byte: &u8) -> bool {
    matches!(byte, b' ' | b'\t')
}

fn trim_whitespace(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|byte| !is_whitespace(byte))
        .unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|byte| !is_whitespace(byte))
        .map_or(start, |last| last + 1);
    &bytes[start..end]
}

struct CompleteLines<'a> {
    bytes: &'a [u8],
    line_start: usize,
    search_from: usize,
}

impl<'a> CompleteLines<'a> {
    fn new(bytes: &'a [u8], search_from: usize) -> Self {
        Self {
            bytes,
            line_start: 0,
            search_from,
        }
    }
}

impl Iterator for CompleteLines<'_> {
    type Item = Range<usize>;

    fn next(&mut self) -> Option<Range<usize>> {
        let unsearched = self.bytes.get(self.search_from..)?;
        let line_feed = self.search_from + unsearched.iter().position(|&byte| byte == LINE_FEED)?;
        let start = self.line_start;
        self.line_start = line_feed + 1;
        self.search_from = line_feed + 1;
        let ends_with_carriage_return =
            line_feed > start && self.bytes[line_feed - 1] == CARRIAGE_RETURN;
        Some(start..line_feed - usize::from(ends_with_carriage_return))
    }
}
