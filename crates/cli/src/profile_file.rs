use std::collections::HashSet;
use std::fmt;

use snisplit_core::domains::DomainList;
use snisplit_core::first_flight::{HttpMethod, RelayProfile};
use snisplit_core::strategy::SplitPosition;

const KEY_NAME: &str = "name";
const KEY_DOMAINS: &str = "domains";
const KEY_TLS_PORTS: &str = "tls-ports";
const KEY_TLS_SPLIT: &str = "tls-split";
const KEY_HTTP_PORTS: &str = "http-ports";
const KEY_HTTP_METHOD: &str = "http-method";

const NONE: &str = "none";
const HOST_START: &str = "host-start";
const DOMAIN_MID: &str = "domain-mid";
const HOST_SPELL: &str = "host-spell";
const PASS: &str = "pass";

const COMMENT_MARK: char = '#';
const BYTE_ORDER_MARK: char = '\u{feff}';
const BLANKS: [char; 2] = [' ', '\t'];
const LABEL_SEPARATOR: char = '.';
const RANGE_SEPARATOR: char = '-';

const MAX_NAME_LEN: usize = 32;
const MAX_DOMAIN_LEN: usize = 253;
const MAX_LABEL_LEN: usize = 63;
const MIN_LABELS: usize = 2;
const DNS_PORT: u16 = 53;
const DEFAULT_TLS_PORT: u16 = 443;
const DEFAULT_HTTP_PORT: u16 = 80;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileSpec {
    pub name: String,
    pub domains: Vec<String>,
    pub tls_ports: Vec<u16>,
    pub tls_positions: Vec<SplitPosition>,
    pub http_ports: Vec<u16>,
    pub http_method: HttpMethod,
}

impl ProfileSpec {
    pub fn relay_profile(&self) -> RelayProfile {
        RelayProfile {
            name: self.name.clone(),
            domains: DomainList::new(&self.domains),
            tls_positions: self.tls_positions.clone(),
            tls_ports: self.tls_ports.clone(),
            http_ports: self.http_ports.clone(),
            http_method: self.http_method,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    UnknownKey(String),
    RepeatedKey(String),
    NoValue(String),
    ExtraValue(String),
    MissingKey(&'static str),
    NameLength(String),
    NameCharacters(String),
    NameReserved(String),
    DomainCharacters(String),
    DomainTooLong(String),
    DomainSingleLabel(String),
    DomainEmptyLabel(String),
    DomainLabelTooLong(String),
    DomainHyphenAtEdge(String),
    DuplicateDomain(String),
    PortNotNumber(String),
    PortOutOfRange(String),
    PortIsDns(String),
    ReversedRange(String),
    DuplicatePort(u16),
    PortInBothLists(u16),
    NoneWithPorts,
    BothProtocolsDisabled,
    BadPosition(String),
    BadMethod(String),
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownKey(key) => write!(f, "unknown key {key:?}"),
            Self::RepeatedKey(key) => write!(f, "key {key} given twice"),
            Self::NoValue(key) => write!(f, "{key} needs a value"),
            Self::ExtraValue(key) => write!(f, "{key} takes exactly one value"),
            Self::MissingKey(key) => write!(f, "missing required key {key}"),
            Self::NameLength(name) => write!(
                f,
                "bad name {name:?}: must be 1-{MAX_NAME_LEN} characters long"
            ),
            Self::NameCharacters(name) => {
                write!(f, "bad name {name:?}: only a-z, 0-9 and '-' are allowed")
            }
            Self::NameReserved(name) => {
                write!(f, "bad name {name:?}: it is the name of a built-in profile")
            }
            Self::DomainCharacters(domain) => write!(
                f,
                "bad domain {domain:?}: only a-z, 0-9, '-' and '.' are allowed"
            ),
            Self::DomainTooLong(domain) => write!(
                f,
                "bad domain {domain:?}: longer than {MAX_DOMAIN_LEN} characters"
            ),
            Self::DomainSingleLabel(domain) => write!(
                f,
                "bad domain {domain:?}: needs at least {MIN_LABELS} labels"
            ),
            Self::DomainEmptyLabel(domain) => {
                write!(f, "bad domain {domain:?}: empty label")
            }
            Self::DomainLabelTooLong(domain) => write!(
                f,
                "bad domain {domain:?}: a label is longer than {MAX_LABEL_LEN} characters"
            ),
            Self::DomainHyphenAtEdge(domain) => {
                write!(f, "bad domain {domain:?}: a label starts or ends with '-'")
            }
            Self::DuplicateDomain(domain) => write!(f, "domain {domain} listed twice"),
            Self::PortNotNumber(value) => {
                write!(f, "bad port {value:?}: not a number or a range like 1-2")
            }
            Self::PortOutOfRange(value) => {
                write!(f, "bad port {value:?}: ports are 1-65535")
            }
            Self::PortIsDns(value) => {
                write!(f, "bad port {value:?}: port {DNS_PORT} is the DNS port")
            }
            Self::ReversedRange(value) => {
                write!(
                    f,
                    "bad port range {value:?}: the first port is after the last"
                )
            }
            Self::DuplicatePort(port) => write!(f, "port {port} listed twice"),
            Self::PortInBothLists(port) => write!(
                f,
                "port {port} is in both {KEY_TLS_PORTS} and {KEY_HTTP_PORTS}"
            ),
            Self::NoneWithPorts => write!(f, "{NONE} cannot be combined with ports"),
            Self::BothProtocolsDisabled => write!(
                f,
                "{KEY_TLS_PORTS} and {KEY_HTTP_PORTS} are both {NONE}: nothing to relay"
            ),
            Self::BadPosition(value) => write!(
                f,
                "bad split position {value:?}: use a byte offset from 1, {HOST_START} or {DOMAIN_MID}"
            ),
            Self::BadMethod(value) => {
                write!(f, "bad http method {value:?}: use {HOST_SPELL} or {PASS}")
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileError {
    pub source: String,
    pub line: Option<usize>,
    pub problem: Problem,
}

impl fmt::Display for ProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.line {
            Some(line) => write!(f, "{}:{line}: {}", self.source, self.problem),
            None => write!(f, "{}: {}", self.source, self.problem),
        }
    }
}

impl std::error::Error for ProfileError {}

pub fn parse(
    source: &str,
    text: &str,
    reserved_names: &[&str],
) -> Result<ProfileSpec, ProfileError> {
    let located = |line: Option<usize>, problem: Problem| ProfileError {
        source: source.to_string(),
        line,
        problem,
    };
    let text = text.strip_prefix(BYTE_ORDER_MARK).unwrap_or(text);
    let mut draft = Draft::new();
    for (index, raw) in text.lines().enumerate() {
        let line = raw.trim_matches(BLANKS);
        if line.is_empty() || line.starts_with(COMMENT_MARK) {
            continue;
        }
        let mut tokens = line.split(BLANKS).filter(|token| !token.is_empty());
        let key = tokens.next().unwrap_or_default();
        let values: Vec<&str> = tokens.collect();
        draft
            .apply(key, &values, index + 1, reserved_names)
            .map_err(|problem| located(Some(index + 1), problem))?;
    }
    draft
        .finish()
        .map_err(|(line, problem)| located(line, problem))
}

struct Draft {
    name: Option<String>,
    domains: Vec<String>,
    tls_ports: Vec<u16>,
    tls_ports_line: Option<usize>,
    tls_positions: Vec<SplitPosition>,
    http_ports: Vec<u16>,
    http_ports_line: Option<usize>,
    http_method: HttpMethod,
    seen: Vec<&'static str>,
}

impl Draft {
    fn new() -> Self {
        Self {
            name: None,
            domains: Vec::new(),
            tls_ports: vec![DEFAULT_TLS_PORT],
            tls_ports_line: None,
            tls_positions: vec![SplitPosition::Offset(1)],
            http_ports: vec![DEFAULT_HTTP_PORT],
            http_ports_line: None,
            http_method: HttpMethod::HostSpell,
            seen: Vec::new(),
        }
    }

    fn apply(
        &mut self,
        key: &str,
        values: &[&str],
        line: usize,
        reserved_names: &[&str],
    ) -> Result<(), Problem> {
        let key = [
            KEY_NAME,
            KEY_DOMAINS,
            KEY_TLS_PORTS,
            KEY_TLS_SPLIT,
            KEY_HTTP_PORTS,
            KEY_HTTP_METHOD,
        ]
        .into_iter()
        .find(|known| *known == key)
        .ok_or_else(|| Problem::UnknownKey(key.to_string()))?;
        if key != KEY_DOMAINS {
            if self.seen.contains(&key) {
                return Err(Problem::RepeatedKey(key.to_string()));
            }
            self.seen.push(key);
        }
        if values.is_empty() {
            return Err(Problem::NoValue(key.to_string()));
        }
        match key {
            KEY_NAME => self.name = Some(parse_name(single(key, values)?, reserved_names)?),
            KEY_DOMAINS => self.add_domains(values)?,
            KEY_TLS_PORTS => {
                self.tls_ports = parse_ports(values)?;
                self.tls_ports_line = Some(line);
            }
            KEY_TLS_SPLIT => self.tls_positions = parse_positions(values)?,
            KEY_HTTP_PORTS => {
                self.http_ports = parse_ports(values)?;
                self.http_ports_line = Some(line);
            }
            _ => self.http_method = parse_method(single(key, values)?)?,
        }
        Ok(())
    }

    fn add_domains(&mut self, values: &[&str]) -> Result<(), Problem> {
        for value in values {
            let domain = parse_domain(value)?;
            if self.domains.contains(&domain) {
                return Err(Problem::DuplicateDomain(domain));
            }
            self.domains.push(domain);
        }
        Ok(())
    }

    fn finish(self) -> Result<ProfileSpec, (Option<usize>, Problem)> {
        let name = self.name.ok_or((None, Problem::MissingKey(KEY_NAME)))?;
        if self.domains.is_empty() {
            return Err((None, Problem::MissingKey(KEY_DOMAINS)));
        }
        let ports_line = self.tls_ports_line.max(self.http_ports_line);
        if self.tls_ports.is_empty() && self.http_ports.is_empty() {
            return Err((ports_line, Problem::BothProtocolsDisabled));
        }
        let tls: HashSet<u16> = self.tls_ports.iter().copied().collect();
        if let Some(port) = self.http_ports.iter().find(|port| tls.contains(port)) {
            return Err((ports_line, Problem::PortInBothLists(*port)));
        }
        Ok(ProfileSpec {
            name,
            domains: self.domains,
            tls_ports: self.tls_ports,
            tls_positions: self.tls_positions,
            http_ports: self.http_ports,
            http_method: self.http_method,
        })
    }
}

fn single<'a>(key: &str, values: &[&'a str]) -> Result<&'a str, Problem> {
    match values {
        [value] => Ok(value),
        _ => Err(Problem::ExtraValue(key.to_string())),
    }
}

fn parse_name(value: &str, reserved_names: &[&str]) -> Result<String, Problem> {
    if value.is_empty() || value.len() > MAX_NAME_LEN {
        return Err(Problem::NameLength(value.to_string()));
    }
    let allowed = |byte: u8| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-';
    if !value.bytes().all(allowed) {
        return Err(Problem::NameCharacters(value.to_string()));
    }
    if reserved_names.contains(&value) {
        return Err(Problem::NameReserved(value.to_string()));
    }
    Ok(value.to_string())
}

pub fn parse_domain(value: &str) -> Result<String, Problem> {
    let domain = value.to_ascii_lowercase();
    let allowed = |byte: u8| {
        byte.is_ascii_lowercase()
            || byte.is_ascii_digit()
            || byte == b'-'
            || byte == LABEL_SEPARATOR as u8
    };
    if !domain.bytes().all(allowed) {
        return Err(Problem::DomainCharacters(value.to_string()));
    }
    if domain.len() > MAX_DOMAIN_LEN {
        return Err(Problem::DomainTooLong(value.to_string()));
    }
    let labels: Vec<&str> = domain.split(LABEL_SEPARATOR).collect();
    if labels.len() < MIN_LABELS {
        return Err(Problem::DomainSingleLabel(value.to_string()));
    }
    for label in labels {
        if label.is_empty() {
            return Err(Problem::DomainEmptyLabel(value.to_string()));
        }
        if label.len() > MAX_LABEL_LEN {
            return Err(Problem::DomainLabelTooLong(value.to_string()));
        }
        if label.starts_with(RANGE_SEPARATOR) || label.ends_with(RANGE_SEPARATOR) {
            return Err(Problem::DomainHyphenAtEdge(value.to_string()));
        }
    }
    Ok(domain)
}

fn parse_ports(values: &[&str]) -> Result<Vec<u16>, Problem> {
    if values == [NONE] {
        return Ok(Vec::new());
    }
    let mut ports = Vec::new();
    let mut taken = HashSet::new();
    for value in values {
        if *value == NONE {
            return Err(Problem::NoneWithPorts);
        }
        let (first, last) = match value.split_once(RANGE_SEPARATOR) {
            Some((first, last)) => (parse_port(first, value)?, parse_port(last, value)?),
            None => {
                let port = parse_port(value, value)?;
                (port, port)
            }
        };
        if first > last {
            return Err(Problem::ReversedRange(value.to_string()));
        }
        if (first..=last).contains(&DNS_PORT) {
            return Err(Problem::PortIsDns(value.to_string()));
        }
        for port in first..=last {
            if !taken.insert(port) {
                return Err(Problem::DuplicatePort(port));
            }
            ports.push(port);
        }
    }
    Ok(ports)
}

fn parse_port(text: &str, whole: &str) -> Result<u16, Problem> {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(Problem::PortNotNumber(whole.to_string()));
    }
    text.parse::<u16>()
        .ok()
        .filter(|port| *port != 0)
        .ok_or_else(|| Problem::PortOutOfRange(whole.to_string()))
}

fn parse_positions(values: &[&str]) -> Result<Vec<SplitPosition>, Problem> {
    values
        .iter()
        .map(|value| match *value {
            HOST_START => Ok(SplitPosition::HostStart),
            DOMAIN_MID => Ok(SplitPosition::DomainMid),
            _ => parse_offset(value),
        })
        .collect()
}

fn parse_offset(value: &str) -> Result<SplitPosition, Problem> {
    let digits = value.bytes().all(|byte| byte.is_ascii_digit());
    value
        .parse::<usize>()
        .ok()
        .filter(|offset| digits && *offset >= 1)
        .map(SplitPosition::Offset)
        .ok_or_else(|| Problem::BadPosition(value.to_string()))
}

fn parse_method(value: &str) -> Result<HttpMethod, Problem> {
    match value {
        HOST_SPELL => Ok(HttpMethod::HostSpell),
        PASS => Ok(HttpMethod::PassThrough),
        _ => Err(Problem::BadMethod(value.to_string())),
    }
}

#[cfg(test)]
mod tests;
