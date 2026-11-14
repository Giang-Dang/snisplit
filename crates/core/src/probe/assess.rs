use super::{Agreement, Attempt, AttemptRecord, positions_text};
use crate::first_flight::Protocol;
use crate::strategy::SplitPosition;

pub const MAX_PROFILE_NAME_LEN: usize = 32;

const TLS_PORT: u16 = 443;
const HTTP_PORT: u16 = 80;
const LABEL_SEPARATOR: char = '.';
const NAME_SEPARATOR: char = '-';
const HOST_SPELL: &str = "host-spell";
const PASS: &str = "pass";
const NONE: &str = "none";

const CUT_AT_ONE: &[SplitPosition] = &[SplitPosition::Offset(1)];
const CUT_AT_ONE_AND_DOMAIN_MID: &[SplitPosition] =
    &[SplitPosition::Offset(1), SplitPosition::DomainMid];
const CUT_AT_DOMAIN_MID: &[SplitPosition] = &[SplitPosition::DomainMid];
const CUT_AT_HOST_START: &[SplitPosition] = &[SplitPosition::HostStart];
const SPLIT_PREFERENCE: [&[SplitPosition]; 4] = [
    CUT_AT_ONE,
    CUT_AT_ONE_AND_DOMAIN_MID,
    CUT_AT_DOMAIN_MID,
    CUT_AT_HOST_START,
];

const NOT_BLOCKED: &str = "not blocked here; no profile needed";
const SITE_CAUSES: &str =
    "the site may be down, blocked by address, or blocked by a method snisplit does not have";
const PROTOCOL_CAUSES: &str = "the failing protocol may not be served there, or be blocked by address or by a method snisplit does not have";
const SPLITS_EVERY_HELLO: &str =
    "a profile splits every HTTPS hello, and no split answered every run";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    Open,
    Bypassed,
    Failing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    NotBlocked,
    ProfileSuggested,
    NoProfileHelps,
    NothingWorked,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assessment {
    pub verdict: Verdict,
    pub https: Reach,
    pub http: Reach,
    pub tls_split: Option<Vec<SplitPosition>>,
    pub profile: Option<String>,
}

impl Assessment {
    pub fn summary(&self) -> String {
        let https = self.https_phrase();
        let http = http_phrase(self.http);
        match self.verdict {
            Verdict::NotBlocked => NOT_BLOCKED.to_owned(),
            Verdict::NothingWorked => format!("nothing got through: {SITE_CAUSES}"),
            Verdict::ProfileSuggested => {
                format!("{https}; {http}; the profile below should get it through")
            }
            Verdict::NoProfileHelps if self.http == Reach::Bypassed => {
                format!("{https}; {http}; no profile suggested: {SPLITS_EVERY_HELLO}")
            }
            Verdict::NoProfileHelps => {
                format!("{https}; {http}; no snisplit method helps: {PROTOCOL_CAUSES}")
            }
        }
    }

    fn https_phrase(&self) -> String {
        match (self.https, &self.tls_split) {
            (Reach::Open, _) => "HTTPS answered as is".to_owned(),
            (Reach::Bypassed, Some(split)) => format!(
                "HTTPS fails as is, tls-split {} answered",
                positions_text(split)
            ),
            (Reach::Bypassed | Reach::Failing, _) => "HTTPS failed with every split".to_owned(),
        }
    }
}

fn http_phrase(reach: Reach) -> &'static str {
    match reach {
        Reach::Open => "HTTP answered as is",
        Reach::Bypassed => "HTTP fails as is, hoSt: answered",
        Reach::Failing => "HTTP failed with and without hoSt:",
    }
}

enum TlsLines<'a> {
    Split(&'a [SplitPosition]),
    Off,
}

pub fn assess(domain: &str, records: &[AttemptRecord]) -> Assessment {
    let works = |wanted: &Attempt| {
        records
            .iter()
            .any(|record| record.attempt == *wanted && record.agreement() == Agreement::EveryRun)
    };
    let tls_split = SPLIT_PREFERENCE
        .into_iter()
        .find(|split| works(&Attempt::TlsSplit(split.to_vec())));
    let plain_http_works = works(&Attempt::HttpPlain);
    let respelling_works = works(&Attempt::HttpRespelled);
    let https = reach(works(&Attempt::TlsWhole), tls_split.is_some());
    let http = reach(plain_http_works, respelling_works);
    let tls_lines = match tls_split {
        Some(split) => Some(TlsLines::Split(split)),
        None if no_tls_attempt_answered(records) => Some(TlsLines::Off),
        None => None,
    };
    let anything_works = records
        .iter()
        .any(|record| record.agreement() == Agreement::EveryRun);
    let profile_helps = https == Reach::Bypassed || http == Reach::Bypassed;
    let verdict = if https == Reach::Open && http == Reach::Open {
        Verdict::NotBlocked
    } else if !anything_works {
        Verdict::NothingWorked
    } else if profile_helps && tls_lines.is_some() {
        Verdict::ProfileSuggested
    } else {
        Verdict::NoProfileHelps
    };
    let http_method = if plain_http_works && !respelling_works {
        PASS
    } else {
        HOST_SPELL
    };
    let profile = match (verdict, tls_lines) {
        (Verdict::ProfileSuggested, Some(tls_lines)) => {
            Some(profile_text(domain, &tls_lines, http_method))
        }
        _ => None,
    };
    Assessment {
        verdict,
        https,
        http,
        tls_split: tls_split.map(<[SplitPosition]>::to_vec),
        profile,
    }
}

fn reach(as_is: bool, with_snisplit: bool) -> Reach {
    match (as_is, with_snisplit) {
        (true, _) => Reach::Open,
        (false, true) => Reach::Bypassed,
        (false, false) => Reach::Failing,
    }
}

fn no_tls_attempt_answered(records: &[AttemptRecord]) -> bool {
    records
        .iter()
        .filter(|record| record.attempt.protocol() == Protocol::Tls)
        .all(|record| record.agreement() == Agreement::NoRun)
}

fn profile_text(domain: &str, tls_lines: &TlsLines, http_method: &str) -> String {
    let name = profile_name(domain);
    let tls = match tls_lines {
        TlsLines::Split(split) => format!(
            "tls-ports {TLS_PORT}\ntls-split {}\n",
            positions_text(split)
        ),
        TlsLines::Off => format!("tls-ports {NONE}\n"),
    };
    format!(
        "name {name}\ndomains {domain}\n{tls}http-ports {HTTP_PORT}\nhttp-method {http_method}\n"
    )
}

pub fn profile_name(domain: &str) -> String {
    let dashed: String = domain
        .chars()
        .map(|character| {
            if character == LABEL_SEPARATOR {
                NAME_SEPARATOR
            } else {
                character
            }
        })
        .take(MAX_PROFILE_NAME_LEN)
        .collect();
    dashed.trim_end_matches(NAME_SEPARATOR).to_owned()
}

#[cfg(test)]
mod tests;
