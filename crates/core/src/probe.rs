mod assess;
mod hello;
mod reply;

use std::fmt;

use crate::domains::DomainList;
use crate::first_flight::{Decision, HttpMethod, Protocol, RelayProfile, decide};
use crate::http_stream::HostRespeller;
use crate::strategy::SplitPosition;

pub use assess::{Assessment, Reach, Verdict, assess, profile_name};
pub use hello::{HelloRandom, MAX_HOST_LEN, RANDOM_LEN, client_hello};
pub use reply::{MAX_STATUS_LINE, Outcome, RecordType, Reply, reply_outcome};

const PROBE_PROFILE_NAME: &str = "probe";
const HOST_START: &str = "host-start";
const DOMAIN_MID: &str = "domain-mid";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Attempt {
    TlsWhole,
    TlsSplit(Vec<SplitPosition>),
    HttpPlain,
    HttpRespelled,
}

impl Attempt {
    pub fn protocol(&self) -> Protocol {
        match self {
            Self::TlsWhole | Self::TlsSplit(_) => Protocol::Tls,
            Self::HttpPlain | Self::HttpRespelled => Protocol::Http,
        }
    }
}

impl fmt::Display for Attempt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TlsWhole => write!(f, "https, whole hello"),
            Self::TlsSplit(positions) => {
                write!(f, "https, tls-split {}", positions_text(positions))
            }
            Self::HttpPlain => write!(f, "http, Host:"),
            Self::HttpRespelled => write!(f, "http, hoSt:"),
        }
    }
}

pub fn attempts() -> Vec<Attempt> {
    vec![
        Attempt::TlsWhole,
        Attempt::TlsSplit(vec![SplitPosition::Offset(1)]),
        Attempt::TlsSplit(vec![SplitPosition::HostStart]),
        Attempt::TlsSplit(vec![SplitPosition::DomainMid]),
        Attempt::TlsSplit(vec![SplitPosition::Offset(1), SplitPosition::DomainMid]),
        Attempt::HttpPlain,
        Attempt::HttpRespelled,
    ]
}

pub fn positions_text(positions: &[SplitPosition]) -> String {
    let words: Vec<String> = positions
        .iter()
        .map(|position| match position {
            SplitPosition::HostStart => HOST_START.to_owned(),
            SplitPosition::DomainMid => DOMAIN_MID.to_owned(),
            SplitPosition::Offset(offset) => offset.to_string(),
        })
        .collect();
    words.join(" ")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirstFlight {
    pub bytes: Vec<u8>,
    pub cuts: Vec<usize>,
}

pub fn first_flight(attempt: &Attempt, domain: &str, secrets: &HelloRandom) -> Option<FirstFlight> {
    let (bytes, positions) = match attempt {
        Attempt::TlsWhole => (client_hello(domain, secrets)?, Vec::new()),
        Attempt::TlsSplit(positions) => (client_hello(domain, secrets)?, positions.clone()),
        Attempt::HttpPlain | Attempt::HttpRespelled => (http_request(domain), Vec::new()),
    };
    let profile = RelayProfile {
        name: PROBE_PROFILE_NAME.to_owned(),
        domains: DomainList::new([domain]),
        tls_positions: positions,
        tls_ports: Vec::new(),
        http_ports: Vec::new(),
        http_method: HttpMethod::HostSpell,
    };
    let Decision::Forward { cuts, .. } = decide(&profile, attempt.protocol(), &bytes, bytes.len())
    else {
        return None;
    };
    let bytes = match attempt {
        Attempt::HttpRespelled => respelled(&bytes),
        _ => bytes,
    };
    Some(FirstFlight { bytes, cuts })
}

fn http_request(domain: &str) -> Vec<u8> {
    format!("GET / HTTP/1.1\r\nHost: {domain}\r\nAccept: */*\r\nConnection: close\r\n\r\n")
        .into_bytes()
}

fn respelled(request: &[u8]) -> Vec<u8> {
    let mut respeller = HostRespeller::new();
    let mut output = Vec::with_capacity(request.len());
    respeller.feed(request, &mut output);
    respeller.finish(&mut output);
    output
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Agreement {
    EveryRun,
    SomeRuns,
    NoRun,
}

impl fmt::Display for Agreement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let word = match self {
            Self::EveryRun => "works",
            Self::SomeRuns => "mixed",
            Self::NoRun => "fails",
        };
        write!(f, "{word}")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttemptRecord {
    pub attempt: Attempt,
    pub outcomes: Vec<Outcome>,
}

impl AttemptRecord {
    pub fn agreement(&self) -> Agreement {
        let answered = self
            .outcomes
            .iter()
            .filter(|outcome| outcome.answered())
            .count();
        match answered {
            0 => Agreement::NoRun,
            _ if answered == self.outcomes.len() => Agreement::EveryRun,
            _ => Agreement::SomeRuns,
        }
    }
}

#[cfg(test)]
mod tests;
