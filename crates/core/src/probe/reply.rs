use std::fmt;
use std::io::ErrorKind;

use crate::first_flight::Protocol;

pub const MAX_STATUS_LINE: usize = 200;
pub const MAX_UNEXPECTED_SHOWN: usize = 8;

const RECORD_HEADER_LEN: usize = 5;
const CONTENT_TYPE_ALERT: u8 = 21;
const CONTENT_TYPE_HANDSHAKE: u8 = 22;
const TLS_MAJOR_VERSION: u8 = 3;
const HIGHEST_TLS_MINOR_VERSION: u8 = 4;
const MAX_RECORD_LEN: usize = (1 << 14) + 2048;
const HTTP_1_PREFIX: &[u8] = b"HTTP/1.";
const LINE_FEED: u8 = b'\n';
const CARRIAGE_RETURN: u8 = b'\r';
const UNPRINTABLE: char = '?';

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordType {
    Handshake,
    Alert,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    Tls { record: RecordType, version: u16 },
    Http { status_line: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Answered(Reply),
    Unexpected(Vec<u8>),
    Reset,
    Closed,
    TimedOut,
    ConnectFailed(ErrorKind),
    Failed(ErrorKind),
}

impl Outcome {
    pub fn answered(&self) -> bool {
        matches!(self, Self::Answered(_))
    }

    pub fn of_stream_error(kind: ErrorKind) -> Self {
        match kind {
            ErrorKind::ConnectionReset | ErrorKind::ConnectionAborted | ErrorKind::BrokenPipe => {
                Self::Reset
            }
            _ => Self::Failed(kind),
        }
    }
}

impl fmt::Display for Outcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Answered(Reply::Tls { record, version }) => {
                let record = match record {
                    RecordType::Handshake => "handshake",
                    RecordType::Alert => "alert",
                };
                write!(f, "{record} {version:#06x}")
            }
            Self::Answered(Reply::Http { status_line }) => write!(f, "{status_line}"),
            Self::Unexpected(bytes) => {
                write!(f, "unexpected")?;
                for byte in bytes {
                    write!(f, " {byte:02x}")?;
                }
                Ok(())
            }
            Self::Reset => write!(f, "reset"),
            Self::Closed => write!(f, "closed"),
            Self::TimedOut => write!(f, "timed out"),
            Self::ConnectFailed(kind) => write!(f, "connect failed: {kind}"),
            Self::Failed(kind) => write!(f, "failed: {kind}"),
        }
    }
}

pub fn reply_outcome(protocol: Protocol, received: &[u8], ended: bool) -> Option<Outcome> {
    if received.is_empty() {
        return ended.then_some(Outcome::Closed);
    }
    match protocol {
        Protocol::Tls => tls_reply(received, ended),
        Protocol::Http => http_reply(received, ended),
    }
}

fn tls_reply(received: &[u8], ended: bool) -> Option<Outcome> {
    let Some(&[content_type, major, minor, length_high, length_low]) =
        received.get(..RECORD_HEADER_LEN)
    else {
        return ended.then(|| unexpected(received));
    };
    let record = match content_type {
        CONTENT_TYPE_HANDSHAKE => RecordType::Handshake,
        CONTENT_TYPE_ALERT => RecordType::Alert,
        _ => return Some(unexpected(received)),
    };
    let length = usize::from(u16::from_be_bytes([length_high, length_low]));
    let plausible = major == TLS_MAJOR_VERSION
        && minor <= HIGHEST_TLS_MINOR_VERSION
        && (1..=MAX_RECORD_LEN).contains(&length);
    if !plausible {
        return Some(unexpected(received));
    }
    Some(Outcome::Answered(Reply::Tls {
        record,
        version: u16::from_be_bytes([major, minor]),
    }))
}

fn http_reply(received: &[u8], ended: bool) -> Option<Outcome> {
    let compared = received.len().min(HTTP_1_PREFIX.len());
    if received[..compared] != HTTP_1_PREFIX[..compared] {
        return Some(unexpected(received));
    }
    let line_end = received.iter().position(|&byte| byte == LINE_FEED);
    let complete = line_end.is_some() || received.len() >= MAX_STATUS_LINE || ended;
    if !complete {
        return None;
    }
    if compared < HTTP_1_PREFIX.len() {
        return Some(unexpected(received));
    }
    let line = &received[..line_end.unwrap_or(received.len()).min(MAX_STATUS_LINE)];
    let line = line.strip_suffix(&[CARRIAGE_RETURN]).unwrap_or(line);
    Some(Outcome::Answered(Reply::Http {
        status_line: printable(line),
    }))
}

fn printable(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&byte| {
            if byte == b' ' || byte.is_ascii_graphic() {
                char::from(byte)
            } else {
                UNPRINTABLE
            }
        })
        .collect()
}

fn unexpected(received: &[u8]) -> Outcome {
    Outcome::Unexpected(received[..received.len().min(MAX_UNEXPECTED_SHOWN)].to_vec())
}

#[cfg(test)]
mod tests;
