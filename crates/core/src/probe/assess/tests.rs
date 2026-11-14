use super::*;
use crate::probe::{Outcome, Reply};

const STEAM_STORE: &str = "store.steampowered.com";

fn answered() -> Outcome {
    Outcome::Answered(Reply::Http {
        status_line: "HTTP/1.1 200 OK".to_owned(),
    })
}

fn runs(pattern: &str) -> Vec<Outcome> {
    pattern
        .chars()
        .map(|run| match run {
            'y' => answered(),
            _ => Outcome::Reset,
        })
        .collect()
}

fn records(
    whole: &str,
    one: &str,
    host_start: &str,
    domain_mid: &str,
    one_and_mid: &str,
    plain: &str,
    respelled: &str,
) -> Vec<AttemptRecord> {
    let tls_split = |positions: &[SplitPosition]| Attempt::TlsSplit(positions.to_vec());
    [
        (Attempt::TlsWhole, whole),
        (tls_split(&[SplitPosition::Offset(1)]), one),
        (tls_split(&[SplitPosition::HostStart]), host_start),
        (tls_split(&[SplitPosition::DomainMid]), domain_mid),
        (
            tls_split(&[SplitPosition::Offset(1), SplitPosition::DomainMid]),
            one_and_mid,
        ),
        (Attempt::HttpPlain, plain),
        (Attempt::HttpRespelled, respelled),
    ]
    .into_iter()
    .map(|(attempt, pattern)| AttemptRecord {
        attempt,
        outcomes: runs(pattern),
    })
    .collect()
}

#[test]
fn section_1_steam_results_suggest_a_cut_at_byte_1_and_respelling() {
    let assessment = assess(
        STEAM_STORE,
        &records("nn", "yy", "nn", "nn", "yy", "nn", "yy"),
    );
    assert_eq!(assessment.verdict, Verdict::ProfileSuggested);
    assert_eq!(assessment.https, Reach::Bypassed);
    assert_eq!(assessment.http, Reach::Bypassed);
    assert_eq!(
        assessment.profile.as_deref(),
        Some(
            "name store-steampowered-com\ndomains store.steampowered.com\ntls-ports 443\ntls-split 1\nhttp-ports 80\nhttp-method host-spell\n"
        )
    );
    assert_eq!(
        assessment.summary(),
        "HTTPS fails as is, tls-split 1 answered; HTTP fails as is, hoSt: answered; the profile below should get it through"
    );
}

#[test]
fn the_steam_profile_split_is_next_when_a_cut_at_1_alone_is_mixed() {
    let assessment = assess(
        STEAM_STORE,
        &records("nn", "yn", "yy", "yy", "yy", "nn", "yy"),
    );
    assert_eq!(
        assessment.tls_split,
        Some(vec![SplitPosition::Offset(1), SplitPosition::DomainMid])
    );
    assert!(
        assessment
            .profile
            .unwrap()
            .contains("\ntls-split 1 domain-mid\n")
    );
}

#[test]
fn domain_mid_is_preferred_over_host_start() {
    let both = assess(
        "medium.com",
        &records("nn", "nn", "yy", "yy", "nn", "nn", "yy"),
    );
    assert_eq!(both.tls_split, Some(vec![SplitPosition::DomainMid]));
    let host_start_only = assess(
        "medium.com",
        &records("nn", "nn", "yy", "nn", "nn", "nn", "yy"),
    );
    assert_eq!(
        host_start_only.tls_split,
        Some(vec![SplitPosition::HostStart])
    );
    assert!(
        host_start_only
            .profile
            .unwrap()
            .contains("\ntls-split host-start\n")
    );
}

#[test]
fn both_plain_attempts_answering_every_run_is_not_blocked() {
    let assessment = assess(
        "example.org",
        &records("yy", "yy", "yy", "yy", "yy", "yy", "yy"),
    );
    assert_eq!(assessment.verdict, Verdict::NotBlocked);
    assert_eq!(assessment.profile, None);
    assert_eq!(assessment.summary(), "not blocked here; no profile needed");
}

#[test]
fn a_whole_hello_that_answered_only_once_still_needs_the_split() {
    let assessment = assess(
        "medium.com",
        &records("yn", "yy", "nn", "nn", "yy", "yy", "yy"),
    );
    assert_eq!(assessment.verdict, Verdict::ProfileSuggested);
    assert_eq!(assessment.https, Reach::Bypassed);
    assert_eq!(assessment.http, Reach::Open);
}

#[test]
fn no_attempt_answering_every_run_is_nothing_worked() {
    let assessment = assess(
        "medium.com",
        &records("nn", "yn", "nn", "nn", "ny", "nn", "nn"),
    );
    assert_eq!(assessment.verdict, Verdict::NothingWorked);
    assert_eq!(assessment.profile, None);
    assert_eq!(
        assessment.summary(),
        "nothing got through: the site may be down, blocked by address, or blocked by a method snisplit does not have"
    );
}

#[test]
fn silent_https_with_working_respelling_suggests_an_http_only_profile() {
    let assessment = assess(
        "medium.com",
        &records("nn", "nn", "nn", "nn", "nn", "nn", "yy"),
    );
    assert_eq!(assessment.verdict, Verdict::ProfileSuggested);
    assert_eq!(
        assessment.profile.as_deref(),
        Some(
            "name medium-com\ndomains medium.com\ntls-ports none\nhttp-ports 80\nhttp-method host-spell\n"
        )
    );
}

#[test]
fn respelling_that_helps_while_every_split_breaks_https_suggests_nothing() {
    let assessment = assess(
        "medium.com",
        &records("yy", "nn", "nn", "nn", "nn", "nn", "yy"),
    );
    assert_eq!(assessment.verdict, Verdict::NoProfileHelps);
    assert_eq!(assessment.profile, None);
    assert_eq!(
        assessment.summary(),
        "HTTPS answered as is; HTTP fails as is, hoSt: answered; no profile suggested: a profile splits every HTTPS hello, and no split answered every run"
    );
}

#[test]
fn https_that_answered_in_some_runs_is_not_switched_off_for_http() {
    let assessment = assess(
        "medium.com",
        &records("nn", "ny", "nn", "nn", "nn", "nn", "yy"),
    );
    assert_eq!(assessment.verdict, Verdict::NoProfileHelps);
    assert_eq!(assessment.profile, None);
}

#[test]
fn open_https_and_dead_http_is_something_no_method_helps() {
    let assessment = assess(
        "medium.com",
        &records("yy", "yy", "yy", "yy", "yy", "nn", "nn"),
    );
    assert_eq!(assessment.verdict, Verdict::NoProfileHelps);
    assert_eq!(
        assessment.summary(),
        "HTTPS answered as is; HTTP failed with and without hoSt:; no snisplit method helps: the failing protocol may not be served there, or be blocked by address or by a method snisplit does not have"
    );
}

#[test]
fn http_is_passed_through_only_when_plain_answered_and_respelling_did_not() {
    let pass = assess(
        "medium.com",
        &records("nn", "yy", "nn", "nn", "nn", "yy", "nn"),
    );
    assert!(pass.profile.unwrap().ends_with("\nhttp-method pass\n"));
    let both = assess(
        "medium.com",
        &records("nn", "yy", "nn", "nn", "nn", "yy", "yy"),
    );
    assert!(
        both.profile
            .unwrap()
            .ends_with("\nhttp-method host-spell\n")
    );
    let neither = assess(
        "medium.com",
        &records("nn", "yy", "nn", "nn", "nn", "nn", "nn"),
    );
    assert_eq!(neither.http, Reach::Failing);
    assert!(
        neither
            .profile
            .unwrap()
            .ends_with("\nhttp-method host-spell\n")
    );
}

#[test]
fn profile_names_are_the_domain_with_dashes_cut_to_32_characters() {
    assert_eq!(profile_name("medium.com"), "medium-com");
    assert_eq!(
        profile_name("steambroadcast-test.akamaized.net"),
        "steambroadcast-test-akamaized-ne"
    );
    assert_eq!(
        profile_name("abcdefghijklmnopqrstuvwxyz01234.com"),
        "abcdefghijklmnopqrstuvwxyz01234"
    );
}
