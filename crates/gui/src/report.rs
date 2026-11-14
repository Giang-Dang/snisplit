use snisplit_core::probe::Verdict;
use snisplit_relay::probe::ProbeReport;

use crate::state::{Offer, ProbeView, Row};

pub fn view(report: &ProbeReport) -> ProbeView {
    let rows = report
        .records
        .iter()
        .map(|record| Row {
            attempt: record.attempt.to_string(),
            result: record.agreement().to_string(),
            runs: record.outcomes.iter().map(ToString::to_string).collect(),
        })
        .collect();
    let assessment = &report.assessment;
    let offer = match (assessment.verdict, &assessment.profile) {
        (Verdict::ProfileSuggested, Some(profile)) => Offer::Add(profile.clone()),
        (Verdict::NotBlocked, _) => Offer::NotBlocked,
        _ => Offer::NothingWorked,
    };
    ProbeView {
        domain: report.domain.clone(),
        address: report.address.to_string(),
        rows,
        summary: assessment.summary(),
        offer,
    }
}

#[cfg(test)]
mod tests;
