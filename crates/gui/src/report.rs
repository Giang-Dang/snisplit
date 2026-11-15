use snisplit_core::probe::{Attempt, Outcome, Verdict, positions_text};
use snisplit_relay::probe::ProbeReport;

use crate::state::{ProbeView, Row};

pub const ANSWERED: &str = "answered";
pub const CUT_OFF: &str = "cut off";
pub const NO_ANSWER: &str = "no answer";

pub fn view(report: &ProbeReport) -> ProbeView {
    let rows = report
        .records
        .iter()
        .map(|record| Row {
            attempt: attempt_label(&record.attempt),
            outcomes: record
                .outcomes
                .iter()
                .map(|outcome| outcome_word(outcome).to_string())
                .collect(),
        })
        .collect();
    let assessment = &report.assessment;
    let mut details: Vec<String> = report
        .records
        .iter()
        .map(|record| {
            let outcomes: Vec<String> = record.outcomes.iter().map(ToString::to_string).collect();
            format!("  {}: {}", record.attempt, outcomes.join(", "))
        })
        .collect();
    details.push(format!("  verdict: {}", assessment.summary()));
    let profile = match assessment.verdict {
        Verdict::ProfileSuggested => assessment.profile.clone(),
        Verdict::NotBlocked | Verdict::NoProfileHelps | Verdict::NothingWorked => None,
    };
    ProbeView {
        domain: report.domain.clone(),
        address: report.address.to_string(),
        verdict: assessment.verdict,
        profile,
        rows,
        details,
    }
}

pub fn attempt_label(attempt: &Attempt) -> String {
    match attempt {
        Attempt::TlsWhole => "Normal secure connection".to_string(),
        Attempt::TlsSplit(positions) => format!("Sent in pieces ({})", positions_text(positions)),
        Attempt::HttpPlain => "Normal plain connection".to_string(),
        Attempt::HttpRespelled => "Header respelled".to_string(),
    }
}

pub fn outcome_word(outcome: &Outcome) -> &'static str {
    match outcome {
        Outcome::Answered(_) => ANSWERED,
        Outcome::Reset | Outcome::Closed => CUT_OFF,
        Outcome::Unexpected(_)
        | Outcome::TimedOut
        | Outcome::ConnectFailed(_)
        | Outcome::Failed(_) => NO_ANSWER,
    }
}

#[cfg(test)]
mod tests;
