use std::time::{Duration, Instant};

use snisplit::sites::{SiteFile, built_in_specs};
use snisplit_core::first_flight::HttpMethod;

use super::*;

const NEWS_PROFILE: &str = "name news-example-com\ndomains news.example.com\n";

fn site(name: &str, title: &str, domains: &[&str], kind: SiteKind, on: bool) -> Site {
    Site {
        name: name.to_string(),
        title: title.to_string(),
        domains: domains.iter().map(|domain| domain.to_string()).collect(),
        kind,
        on,
    }
}

fn loaded(sites: Vec<Site>) -> State {
    let mut state = State::new();
    assert_eq!(state.update(Event::SitesLoaded(sites)), []);
    state
}

fn with_sites() -> State {
    loaded(vec![
        site("medium", "Medium", &["medium.com"], SiteKind::BuiltIn, true),
        site(
            "news",
            "news",
            &["news.example.com", "cdn.example.com"],
            SiteKind::User,
            true,
        ),
        site(
            "bad",
            "bad",
            &[],
            SiteKind::Broken("bad.profile: oops".to_string()),
            false,
        ),
    ])
}

fn ready(state: &mut State, at: Instant) {
    state.update(Event::SessionReady { at });
}

fn running() -> State {
    let mut state = with_sites();
    state.update(Event::StartPressed);
    ready(&mut state, Instant::now());
    assert_eq!(state.relay(), Relay::On);
    state
}

fn failed(failure: Failure, detail: &str) -> Event {
    Event::SessionEnded(SessionEnd::Failed {
        failure,
        detail: detail.to_string(),
    })
}

fn switched(name: &str, on: bool) -> Event {
    Event::SwitchSaved(Ok((name.to_string(), on)))
}

fn view(domain: &str, verdict: Verdict, profile: Option<&str>) -> ProbeView {
    ProbeView {
        domain: domain.to_string(),
        address: "192.0.2.10".to_string(),
        verdict,
        profile: profile.map(str::to_string),
        rows: Vec::new(),
        details: vec!["  verdict: summary".to_string()],
    }
}

fn adding_tested(verdict: Verdict, profile: Option<&str>) -> State {
    let mut state = with_sites();
    let at = Instant::now();
    state.update(Event::AddRowPressed);
    state.update(Event::FieldEdited("news.example.org".to_string()));
    assert_eq!(
        state.update(Event::AddTestPressed { at }),
        [Effect::Probe {
            id: 1,
            domain: "news.example.org".to_string()
        }]
    );
    state.update(Event::ProbeFinished {
        id: 1,
        result: Ok(view("news.example.org", verdict, profile)),
        at,
    });
    state
}

fn log_of(state: &State) -> Vec<String> {
    state.log().map(str::to_string).collect()
}

fn last_log(state: &State) -> String {
    log_of(state).last().cloned().unwrap_or_default()
}

#[test]
fn start_ready_stop_walks_the_relay_states_with_one_button() {
    let mut state = with_sites();
    assert_eq!(state.update(Event::StartPressed), [Effect::StartSession]);
    assert_eq!(state.relay(), Relay::Starting);
    assert_eq!(state.update(Event::StartPressed), []);
    ready(&mut state, Instant::now());
    assert_eq!(state.relay(), Relay::On);
    assert_eq!(state.update(Event::StopPressed), [Effect::StopSession]);
    assert_eq!(state.relay(), Relay::Stopping);
    assert_eq!(state.update(Event::StopPressed), []);
    assert_eq!(state.update(Event::SessionEnded(SessionEnd::Clean)), []);
    assert_eq!(state.relay(), Relay::Off);
    assert_eq!(state.live_sites(), None);
}

#[test]
fn a_stop_during_start_up_is_not_undone_by_a_late_ready() {
    let mut state = with_sites();
    state.update(Event::StartPressed);
    assert_eq!(state.update(Event::StopPressed), [Effect::StopSession]);
    ready(&mut state, Instant::now());
    assert_eq!(state.relay(), Relay::Stopping);
}

#[test]
fn start_is_refused_while_every_site_is_switched_off() {
    let mut state = loaded(vec![
        site(
            "steam",
            "Steam",
            &["steampowered.com"],
            SiteKind::BuiltIn,
            false,
        ),
        site("bad", "bad", &[], SiteKind::Broken("x".to_string()), false),
    ]);
    assert!(!state.anything_on());
    assert_eq!(state.update(Event::StartPressed), []);
    assert_eq!(state.relay(), Relay::Off);
}

#[test]
fn the_session_starts_with_the_switched_on_sites_and_they_go_live_when_ready() {
    let mut state = with_sites();
    state.update(Event::StartPressed);
    assert_eq!(state.session_sites(), ["medium", "news"]);
    assert_eq!(state.live_sites(), None);
    let at = Instant::now();
    ready(&mut state, at);
    assert_eq!(
        state.live_sites(),
        Some(&["medium".to_string(), "news".to_string()][..])
    );
    assert_eq!(state.on_since(), Some(at));
}

#[test]
fn a_bind_failure_while_starting_is_a_failed_start_and_start_can_be_pressed_again() {
    let mut state = with_sites();
    state.update(Event::StartPressed);
    let detail = "binding 127.0.0.2:53 failed: address in use";
    assert_eq!(state.update(failed(Failure::Binding, detail)), []);
    assert_eq!(
        state.relay(),
        Relay::Failed {
            failure: Failure::Binding,
            while_starting: true
        }
    );
    assert_eq!(
        log_of(&state),
        [
            "starting snisplit",
            "snisplit stopped with an error: binding 127.0.0.2:53 failed: address in use"
        ]
    );
    assert_eq!(state.update(Event::StartPressed), [Effect::StartSession]);
}

#[test]
fn a_session_refusing_to_run_with_nothing_switched_on_ends_plainly_off() {
    let mut state = with_sites();
    state.update(Event::StartPressed);
    state.update(failed(
        Failure::NothingSwitchedOn,
        "every site is switched off",
    ));
    assert_eq!(state.relay(), Relay::Off);
}

#[test]
fn a_failed_rule_removal_on_stop_leaves_leftovers_and_start_still_works() {
    let mut state = running();
    state.update(Event::StopPressed);
    state.update(failed(
        Failure::RemovingRules,
        "powershell: Access is denied.",
    ));
    assert_eq!(
        state.relay(),
        Relay::Leftovers {
            removal_refused: false
        }
    );
    assert_eq!(state.update(Event::StartPressed), [Effect::StartSession]);
}

#[test]
fn removing_leftovers_has_its_own_state_and_reports_a_refusal_and_a_success() {
    let mut state = running();
    state.update(Event::StopPressed);
    state.update(failed(
        Failure::RemovingRules,
        "powershell: Access is denied.",
    ));
    assert_eq!(
        state.update(Event::RemoveLeftoversPressed),
        [Effect::RemoveLeftovers]
    );
    assert_eq!(state.relay(), Relay::RemovingLeftovers);
    assert_eq!(state.update(Event::RemoveLeftoversPressed), []);
    state.update(Event::LeftoversRemoved(Err(
        "powershell: Access is denied.".to_string(),
    )));
    assert_eq!(
        state.relay(),
        Relay::Leftovers {
            removal_refused: true
        }
    );
    assert_eq!(
        last_log(&state),
        "removing the leftover DNS rules failed: powershell: Access is denied."
    );
    state.update(Event::RemoveLeftoversPressed);
    assert_eq!(state.update(Event::LeftoversRemoved(Ok(()))), []);
    assert_eq!(state.relay(), Relay::Off);
    assert_eq!(state.note(), Some(Note::LeftoversRemoved));
    assert_eq!(last_log(&state), "the leftover DNS rules were removed");
    state.update(Event::StartPressed);
    assert_eq!(state.note(), None);
}

#[test]
fn closing_while_removing_leftovers_waits_for_a_success_then_exits() {
    let mut state = running();
    state.update(Event::StopPressed);
    state.update(failed(Failure::RemovingRules, "denied"));
    state.update(Event::RemoveLeftoversPressed);
    assert_eq!(state.update(Event::CloseRequested), []);
    assert_eq!(
        state.update(Event::LeftoversRemoved(Ok(()))),
        [Effect::Exit]
    );
}

#[test]
fn closing_while_stopped_exits_at_once() {
    let mut state = State::new();
    assert_eq!(state.update(Event::CloseRequested), [Effect::Exit]);
}

#[test]
fn closing_while_running_stops_first_and_exits_only_when_the_session_has_ended() {
    let mut state = running();
    assert_eq!(state.update(Event::CloseRequested), [Effect::StopSession]);
    assert!(state.closing());
    assert_eq!(state.update(Event::StartPressed), []);
    assert_eq!(state.relay(), Relay::Stopping);
    assert_eq!(
        state.update(Event::SessionEnded(SessionEnd::Clean)),
        [Effect::Exit]
    );
}

#[test]
fn a_failed_removal_while_closing_keeps_the_window_open_with_leftovers_and_the_next_close_exits() {
    let mut state = with_sites();
    state.update(Event::StartPressed);
    assert_eq!(state.update(Event::CloseRequested), [Effect::StopSession]);
    assert_eq!(
        state.update(failed(Failure::RemovingRules, "rules could not be removed")),
        []
    );
    assert_eq!(
        state.relay(),
        Relay::Leftovers {
            removal_refused: false
        }
    );
    assert!(!state.closing());
    assert_eq!(
        last_log(&state),
        "closing stopped: DNS rules may remain: press Start and then Stop, or run snisplit cleanup from an administrator terminal, to remove them"
    );
    assert_eq!(state.update(Event::CloseRequested), [Effect::Exit]);
}

#[test]
fn a_start_failure_while_closing_exits_because_the_session_removed_its_rules() {
    let mut state = with_sites();
    state.update(Event::StartPressed);
    state.update(Event::CloseRequested);
    assert_eq!(
        state.update(failed(
            Failure::ChangingRules,
            "changing the DNS rules failed: powershell: Access is denied."
        )),
        [Effect::Exit]
    );
}

#[test]
fn a_second_close_while_stopping_exits_and_logs_that_rules_may_remain() {
    let mut state = running();
    assert_eq!(state.update(Event::CloseRequested), [Effect::StopSession]);
    assert_eq!(state.update(Event::CloseRequested), [Effect::Exit]);
    assert_eq!(
        last_log(&state),
        "closing before snisplit finished stopping: DNS rules may remain: press Start and then Stop, or run snisplit cleanup from an administrator terminal, to remove them"
    );
}

#[test]
fn the_first_close_while_stopping_keeps_waiting() {
    let mut state = running();
    state.update(Event::StopPressed);
    assert_eq!(state.update(Event::CloseRequested), []);
    assert_eq!(log_of(&state), ["starting snisplit"]);
}

#[test]
fn a_site_added_while_running_restarts_the_session_and_says_so() {
    let mut state = running();
    assert_eq!(
        state.update(Event::SiteSaved(Ok("shop".to_string()))),
        [Effect::LoadSites, Effect::StopSession]
    );
    assert_eq!(state.relay(), Relay::Stopping);
    assert!(state.applying() && state.restart_pending());
    assert_eq!(
        state.update(Event::SessionEnded(SessionEnd::Clean)),
        [Effect::StartSession]
    );
    assert_eq!(state.relay(), Relay::Starting);
    assert!(state.applying() && !state.restart_pending());
    ready(&mut state, Instant::now());
    assert!(!state.applying());
    assert_eq!(
        log_of(&state),
        [
            "starting snisplit",
            "site shop added; restarting snisplit so the change applies",
            "starting snisplit",
        ]
    );
}

#[test]
fn two_changes_while_stopping_coalesce_into_one_restart() {
    let mut state = running();
    state.update(Event::SiteSaved(Ok("shop".to_string())));
    assert_eq!(
        state.update(Event::SiteRemoved(Ok("news".to_string()))),
        [Effect::LoadSites]
    );
    assert_eq!(
        state.update(Event::SessionEnded(SessionEnd::Clean)),
        [Effect::StartSession]
    );
    ready(&mut state, Instant::now());
    assert_eq!(state.update(Event::SessionEnded(SessionEnd::Clean)), []);
    assert_eq!(state.relay(), Relay::Off);
    assert!(
        log_of(&state)
            .contains(&"site news removed; snisplit starts again once it has stopped".to_string())
    );
}

#[test]
fn a_site_removed_after_an_explicit_stop_does_not_restart_the_session() {
    let mut state = running();
    assert_eq!(state.update(Event::StopPressed), [Effect::StopSession]);
    assert_eq!(
        state.update(Event::SiteRemoved(Ok("news".to_string()))),
        [Effect::LoadSites]
    );
    assert_eq!(state.update(Event::SessionEnded(SessionEnd::Clean)), []);
    assert_eq!(state.relay(), Relay::Off);
    assert_eq!(last_log(&state), "site news removed");
}

#[test]
fn a_site_added_after_an_explicit_stop_does_not_restart_the_session() {
    let mut state = running();
    state.update(Event::StopPressed);
    assert_eq!(
        state.update(Event::SiteSaved(Ok("shop".to_string()))),
        [Effect::LoadSites]
    );
    assert_eq!(state.update(Event::SessionEnded(SessionEnd::Clean)), []);
    assert_eq!(state.relay(), Relay::Off);
}

#[test]
fn a_close_during_a_pending_restart_cancels_the_restart() {
    let mut state = running();
    state.update(Event::SiteSaved(Ok("shop".to_string())));
    assert_eq!(state.update(Event::CloseRequested), []);
    assert!(!state.restart_pending() && !state.applying());
    assert_eq!(
        state.update(Event::SessionEnded(SessionEnd::Clean)),
        [Effect::Exit]
    );
}

#[test]
fn a_failed_stop_during_a_restart_does_not_start_again() {
    let mut state = running();
    state.update(Event::SiteRemoved(Ok("news".to_string())));
    let ended = state.update(failed(Failure::RemovingRules, "rules could not be removed"));
    assert_eq!(ended, []);
    assert_eq!(
        state.relay(),
        Relay::Leftovers {
            removal_refused: false
        }
    );
    assert!(!state.applying());
}

#[test]
fn a_site_change_while_off_only_reloads_the_list() {
    let mut state = with_sites();
    assert_eq!(
        state.update(Event::SiteRemoved(Ok("news".to_string()))),
        [Effect::LoadSites]
    );
    assert_eq!(state.relay(), Relay::Off);
    assert_eq!(last_log(&state), "site news removed");
}

#[test]
fn a_switch_turned_off_while_on_restarts_and_is_pending_until_the_session_is_ready() {
    let mut state = running();
    assert_eq!(
        state.update(Event::SwitchPressed(id("medium"))),
        [Effect::SetSwitch {
            name: "medium".to_string(),
            on: false
        }]
    );
    assert_eq!(
        state.update(switched("medium", false)),
        [Effect::StopSession]
    );
    assert!(state.applying());
    let medium = state.site(&id("medium")).unwrap().clone();
    assert!(!medium.on);
    assert!(state.switch_pending(&medium));
    assert!(!state.switch_pending(state.site(&id("news")).unwrap()));
    assert_eq!(state.update(Event::SwitchPressed(id("medium"))), []);
    assert_eq!(
        state.update(Event::SessionEnded(SessionEnd::Clean)),
        [Effect::StartSession]
    );
    assert_eq!(state.session_sites(), ["news"]);
    assert!(state.switch_pending(&medium));
    ready(&mut state, Instant::now());
    assert!(!state.switch_pending(&medium));
    assert_eq!(state.live_sites(), Some(&["news".to_string()][..]));
    assert_eq!(
        log_of(&state)[1],
        "site medium switched off; restarting snisplit so the change applies"
    );
}

#[test]
fn switch_changes_during_a_restart_coalesce_and_one_during_its_start_restarts_once_more() {
    let mut state = running();
    state.update(switched("medium", false));
    assert_eq!(state.update(switched("medium", true)), []);
    assert_eq!(
        last_log(&state),
        "site medium switched on; snisplit starts again once it has stopped"
    );
    assert_eq!(
        state.update(Event::SessionEnded(SessionEnd::Clean)),
        [Effect::StartSession]
    );
    assert_eq!(state.update(switched("news", false)), [Effect::StopSession]);
    assert!(state.restart_pending());
    assert_eq!(
        state.update(Event::SessionEnded(SessionEnd::Clean)),
        [Effect::StartSession]
    );
    assert_eq!(state.session_sites(), ["medium"]);
}

#[test]
fn switching_the_last_site_off_turns_snisplit_off_instead_of_restarting() {
    let mut state = running();
    state.update(switched("medium", false));
    assert_eq!(state.update(switched("news", false)), []);
    assert!(!state.restart_pending() && !state.applying());
    assert_eq!(
        last_log(&state),
        "site news switched off; every site is switched off, so snisplit stays off"
    );
    assert_eq!(state.update(Event::SessionEnded(SessionEnd::Clean)), []);
    assert_eq!(state.relay(), Relay::Off);

    let mut alone = running();
    alone.update(switched("medium", false));
    alone.update(Event::SessionEnded(SessionEnd::Clean));
    ready(&mut alone, Instant::now());
    assert_eq!(alone.update(switched("news", false)), [Effect::StopSession]);
    assert!(!alone.restart_pending());
    assert_eq!(
        last_log(&alone),
        "site news switched off; every site is switched off, so snisplit turns off"
    );
}

#[test]
fn switching_the_last_site_off_during_the_starting_half_of_a_restart_ends_the_applying() {
    let mut state = running();
    state.update(switched("medium", false));
    state.update(Event::SessionEnded(SessionEnd::Clean));
    assert_eq!(state.relay(), Relay::Starting);
    assert!(state.applying());
    assert_eq!(state.update(switched("news", false)), [Effect::StopSession]);
    assert!(!state.applying() && !state.restart_pending());
    assert_eq!(crate::view::hero(&state).state_line, "Turning off...");
    assert_eq!(state.update(Event::SessionEnded(SessionEnd::Clean)), []);
    assert_eq!(state.relay(), Relay::Off);
}

#[test]
fn an_explicit_stop_while_a_switch_change_applies_wins() {
    let mut state = running();
    state.update(switched("medium", false));
    state.update(Event::SessionEnded(SessionEnd::Clean));
    assert_eq!(state.relay(), Relay::Starting);
    assert_eq!(state.update(Event::StopPressed), [Effect::StopSession]);
    assert!(!state.applying());
    assert_eq!(state.update(Event::SessionEnded(SessionEnd::Clean)), []);
    assert_eq!(state.relay(), Relay::Off);
}

#[test]
fn a_switch_while_off_or_after_an_explicit_stop_only_saves() {
    let mut state = with_sites();
    assert_eq!(state.update(switched("medium", false)), []);
    assert_eq!(last_log(&state), "site medium switched off");
    let mut stopping = running();
    stopping.update(Event::StopPressed);
    assert_eq!(stopping.update(switched("news", false)), []);
    assert_eq!(stopping.update(Event::SessionEnded(SessionEnd::Clean)), []);
}

#[test]
fn broken_files_have_no_switch_and_switches_are_locked_while_closing() {
    let mut state = running();
    assert_eq!(state.update(Event::SwitchPressed(id("bad"))), []);
    assert_eq!(state.update(Event::SwitchPressed(id("nobody"))), []);
    state.update(Event::CloseRequested);
    assert_eq!(state.update(Event::SwitchPressed(id("news"))), []);
}

#[test]
fn a_failed_switch_write_changes_nothing_and_is_logged() {
    let mut state = running();
    assert_eq!(
        state.update(Event::SwitchSaved(Err("disk full".to_string()))),
        []
    );
    assert!(state.site(&id("medium")).unwrap().on);
    assert_eq!(last_log(&state), "the switch was not saved: disk full");
}

#[test]
fn removing_asks_once_in_the_open_row_and_only_user_and_broken_sites_can_be_removed() {
    let mut state = with_sites();
    state.update(Event::RemovePressed);
    assert!(!state.confirming_remove());
    state.update(Event::RowPressed(id("medium")));
    state.update(Event::RemovePressed);
    assert!(!state.confirming_remove());
    assert_eq!(state.update(Event::RemoveConfirmed), []);
    state.update(Event::RowPressed(id("news")));
    assert_eq!(*state.open(), Open::Site(id("news")));
    state.update(Event::RemovePressed);
    assert!(state.confirming_remove());
    state.update(Event::RemoveKept);
    assert!(!state.confirming_remove());
    assert_eq!(state.update(Event::RemoveConfirmed), []);
    state.update(Event::RemovePressed);
    assert_eq!(
        state.update(Event::RemoveConfirmed),
        [Effect::DeleteSite("news".to_string())]
    );
    assert_eq!(state.update(Event::RemoveConfirmed), []);
    state.update(Event::RowPressed(id("bad")));
    state.update(Event::RemovePressed);
    assert_eq!(
        state.update(Event::RemoveConfirmed),
        [Effect::DeleteSite("bad".to_string())]
    );
}

#[test]
fn a_confirmation_armed_on_one_row_deletes_nothing_once_another_row_is_open() {
    let mut state = with_sites();
    state.update(Event::RowPressed(id("news")));
    state.update(Event::RemovePressed);
    assert!(state.confirming_remove());
    state.update(Event::RowPressed(id("bad")));
    assert_eq!(state.update(Event::RemoveConfirmed), []);
}

#[test]
fn closing_or_leaving_a_row_while_it_asks_drops_the_question() {
    let mut state = with_sites();
    state.update(Event::RowPressed(id("news")));
    state.update(Event::RemovePressed);
    state.update(Event::RowPressed(id("news")));
    assert_eq!(*state.open(), Open::Nothing);
    assert!(!state.confirming_remove());
    state.update(Event::RowPressed(id("news")));
    assert!(!state.confirming_remove());
    assert_eq!(state.update(Event::RemoveConfirmed), []);

    state.update(Event::RemovePressed);
    state.update(Event::ActivityPressed);
    assert!(!state.confirming_remove());
    state.update(Event::BackPressed);
    state.update(Event::RemovePressed);
    state.update(Event::BackPressed);
    assert!(!state.confirming_remove());

    state.update(Event::RemovePressed);
    let sites = state.sites().to_vec();
    state.update(Event::SitesLoaded(sites));
    assert_eq!(*state.open(), Open::Site(id("news")));
    assert!(!state.confirming_remove());
}

#[test]
fn a_removal_is_traced_in_activity_by_site_name() {
    let mut state = with_sites();
    state.update(Event::RowPressed(id("medium")));
    state.update(Event::RemovePressed);
    assert_eq!(log_of(&state), Vec::<String>::new());
    state.update(Event::RowPressed(id("news")));
    state.update(Event::RemovePressed);
    assert_eq!(last_log(&state), "remove news asked");
    state.update(Event::RemoveConfirmed);
    assert_eq!(
        log_of(&state),
        ["remove news asked", "remove news confirmed"]
    );
}

#[test]
fn removing_a_switched_off_site_while_on_needs_no_restart() {
    let mut state = loaded(vec![
        site("medium", "Medium", &["medium.com"], SiteKind::BuiltIn, true),
        site("news", "news", &["news.example.com"], SiteKind::User, false),
    ]);
    state.update(Event::StartPressed);
    ready(&mut state, Instant::now());
    state.update(Event::RowPressed(id("news")));
    assert_eq!(
        state.update(Event::SiteRemoved(Ok("news".to_string()))),
        [Effect::LoadSites]
    );
    assert_eq!(state.relay(), Relay::On);
    assert_eq!(*state.open(), Open::Nothing);
}

#[test]
fn a_typed_link_is_shortened_and_tested_for_adding() {
    let mut state = with_sites();
    let at = Instant::now();
    state.update(Event::AddRowPressed);
    assert_eq!(*state.open(), Open::Add);
    state.update(Event::FieldEdited(
        " https://Example.NET/page?x=1 ".to_string(),
    ));
    assert_eq!(
        state.update(Event::AddTestPressed { at }),
        [Effect::Probe {
            id: 1,
            domain: "example.net".to_string()
        }]
    );
    assert_eq!(state.field(), "example.net");
    assert_eq!(*state.adding(), Adding::Testing);
    assert_eq!(
        state.probe(),
        Some(&Probing {
            id: 1,
            domain: "example.net".to_string(),
            target: Target::Adding,
            started: at,
        })
    );
    state.update(Event::FieldEdited("other.example.org".to_string()));
    assert_eq!(state.field(), "example.net");
}

#[test]
fn a_mistyped_address_is_refused_without_testing_and_editing_clears_the_hint() {
    let mut state = with_sites();
    state.update(Event::AddRowPressed);
    state.update(Event::FieldEdited("my site".to_string()));
    assert_eq!(
        state.update(Event::AddTestPressed { at: Instant::now() }),
        []
    );
    assert_eq!(*state.adding(), Adding::Typing { mistyped: true });
    assert_eq!(state.probe(), None);
    state.update(Event::FieldEdited("example.net".to_string()));
    assert_eq!(*state.adding(), Adding::Typing { mistyped: false });
}

#[test]
fn a_cancelled_test_returns_to_an_empty_field_and_its_late_result_is_ignored() {
    let mut state = with_sites();
    let at = Instant::now();
    state.update(Event::AddRowPressed);
    state.update(Event::FieldEdited("example.net".to_string()));
    state.update(Event::AddTestPressed { at });
    assert_eq!(
        state.update(Event::CancelTestPressed),
        [Effect::CancelProbe(1)]
    );
    assert_eq!(state.field(), "");
    assert_eq!(*state.adding(), Adding::Typing { mistyped: false });
    state.update(Event::FieldEdited("example.net".to_string()));
    assert_eq!(
        state.update(Event::AddTestPressed { at }),
        [Effect::Probe {
            id: 2,
            domain: "example.net".to_string()
        }]
    );
    state.update(Event::ProbeFinished {
        id: 1,
        result: Ok(view("example.net", Verdict::NotBlocked, None)),
        at,
    });
    assert_eq!(*state.adding(), Adding::Testing);
    state.update(Event::ProbeFinished {
        id: 2,
        result: Err("no upstream DNS server answered for example.net".to_string()),
        at,
    });
    assert_eq!(
        *state.adding(),
        Adding::CouldNotTest("example.net".to_string())
    );
    assert_eq!(
        last_log(&state),
        "could not test example.net: no upstream DNS server answered for example.net"
    );
    assert_eq!(
        state.update(Event::TryAgainPressed { at }),
        [Effect::Probe {
            id: 3,
            domain: "example.net".to_string()
        }]
    );
}

#[test]
fn one_test_runs_at_a_time() {
    let mut state = with_sites();
    let at = Instant::now();
    assert_eq!(
        state.update(Event::TestPressed {
            site: id("medium"),
            at
        }),
        [Effect::Probe {
            id: 1,
            domain: "medium.com".to_string()
        }]
    );
    assert_eq!(
        state.update(Event::TestPressed {
            site: id("news"),
            at
        }),
        []
    );
    state.update(Event::AddRowPressed);
    state.update(Event::FieldEdited("example.org".to_string()));
    assert_eq!(state.update(Event::AddTestPressed { at }), []);
    assert_eq!(
        state.update(Event::TestPressed {
            site: id("bad"),
            at
        }),
        []
    );
}

#[test]
fn a_site_test_uses_its_test_address_and_keeps_the_result_with_the_site() {
    let mut state = loaded(vec![site(
        "steam",
        "Steam",
        &["steampowered.com", "steamcommunity.com"],
        SiteKind::BuiltIn,
        true,
    )]);
    let pressed_at = Instant::now();
    let finished_at = pressed_at + Duration::from_secs(4);
    assert_eq!(
        state.update(Event::TestPressed {
            site: id("steam"),
            at: pressed_at
        }),
        [Effect::Probe {
            id: 1,
            domain: "store.steampowered.com".to_string()
        }]
    );
    let result = view("store.steampowered.com", Verdict::ProfileSuggested, None);
    state.update(Event::ProbeFinished {
        id: 1,
        result: Ok(result.clone()),
        at: finished_at,
    });
    assert_eq!(
        state.tested(&id("steam")),
        Some(&Tested {
            domain: "store.steampowered.com".to_string(),
            at: finished_at,
            view: Some(result),
        })
    );
    assert_eq!(
        log_of(&state),
        [
            "test of store.steampowered.com at 192.0.2.10:",
            "  verdict: summary"
        ]
    );
    state.update(Event::TestPressed {
        site: id("steam"),
        at: finished_at,
    });
    state.update(Event::ProbeFinished {
        id: 2,
        result: Err("timed out".to_string()),
        at: finished_at,
    });
    assert_eq!(state.tested(&id("steam")).unwrap().view, None);
}

#[test]
fn add_saves_the_suggested_profile_only_when_snisplit_can_open_the_site() {
    let mut open = adding_tested(Verdict::NotBlocked, None);
    assert_eq!(open.update(Event::AddPressed), []);
    let mut blocked = adding_tested(Verdict::ProfileSuggested, Some(NEWS_PROFILE));
    assert_eq!(
        blocked.update(Event::AddPressed),
        [Effect::SaveSite(NEWS_PROFILE.to_string())]
    );
}

#[test]
fn a_saved_site_closes_the_panel_and_reloads_the_list() {
    let mut state = adding_tested(Verdict::ProfileSuggested, Some(NEWS_PROFILE));
    assert_eq!(
        state.update(Event::SiteSaved(Ok("news-example-org".to_string()))),
        [Effect::LoadSites]
    );
    assert_eq!(*state.open(), Open::Nothing);
    assert_eq!(state.field(), "");
    assert_eq!(last_log(&state), "site news-example-org added");
}

#[test]
fn a_covered_refusal_names_the_covering_site_by_its_title() {
    let mut state = adding_tested(Verdict::ProfileSuggested, Some(NEWS_PROFILE));
    state.update(Event::SiteSaved(Err(SaveRefusal::Covered {
        domain: "medium.com".to_string(),
        owner: "medium".to_string(),
    })));
    assert_eq!(
        *state.adding(),
        Adding::Covered {
            domain: "medium.com".to_string(),
            by: "Medium".to_string()
        }
    );
    assert_eq!(
        last_log(&state),
        "not added: medium.com is already covered by the site medium"
    );
    let mut other = adding_tested(Verdict::ProfileSuggested, Some(NEWS_PROFILE));
    other.update(Event::SiteSaved(Err(SaveRefusal::Other(
        "a site named news-example-org already exists".to_string(),
    ))));
    assert_eq!(
        *other.adding(),
        Adding::NotSaved("news.example.org".to_string())
    );
}

#[test]
fn test_another_empties_the_field_and_closing_the_panel_resets_it() {
    let mut state = adding_tested(Verdict::NotBlocked, None);
    state.update(Event::TestAnotherPressed);
    assert_eq!(*state.adding(), Adding::Typing { mistyped: false });
    assert_eq!(state.field(), "");
    state.update(Event::FieldEdited("example.org".to_string()));
    state.update(Event::PanelClosed);
    assert_eq!(*state.open(), Open::Nothing);
    assert_eq!(state.field(), "");
}

#[test]
fn opening_a_row_closes_the_add_panel_and_cancels_its_test() {
    let mut state = with_sites();
    state.update(Event::AddRowPressed);
    state.update(Event::FieldEdited("example.net".to_string()));
    state.update(Event::AddTestPressed { at: Instant::now() });
    assert_eq!(
        state.update(Event::RowPressed(id("news"))),
        [Effect::CancelProbe(1)]
    );
    assert_eq!(*state.open(), Open::Site(id("news")));
    assert_eq!(state.probe(), None);
    assert_eq!(state.update(Event::RowPressed(id("news"))), []);
    assert_eq!(*state.open(), Open::Nothing);
}

#[test]
fn closing_the_panel_cancels_its_test_but_a_site_test_survives_its_row_closing() {
    let mut state = with_sites();
    let at = Instant::now();
    state.update(Event::AddRowPressed);
    state.update(Event::FieldEdited("example.net".to_string()));
    state.update(Event::AddTestPressed { at });
    assert_eq!(state.update(Event::PanelClosed), [Effect::CancelProbe(1)]);
    state.update(Event::RowPressed(id("news")));
    state.update(Event::TestPressed {
        site: id("news"),
        at,
    });
    assert_eq!(state.update(Event::RowPressed(id("news"))), []);
    assert_eq!(state.update(Event::AddRowPressed), []);
    assert_eq!(state.probe().map(|probing| probing.id), Some(2));
}

#[test]
fn a_reloaded_list_forgets_the_rows_and_results_of_removed_sites() {
    let mut state = with_sites();
    let at = Instant::now();
    state.update(Event::RowPressed(id("news")));
    state.update(Event::TestPressed {
        site: id("news"),
        at,
    });
    state.update(Event::ProbeFinished {
        id: 1,
        result: Err("timed out".to_string()),
        at,
    });
    assert!(state.tested(&id("news")).is_some());
    state.update(Event::SitesLoaded(vec![site(
        "medium",
        "Medium",
        &["medium.com"],
        SiteKind::BuiltIn,
        true,
    )]));
    assert_eq!(*state.open(), Open::Nothing);
    assert_eq!(state.tested(&id("news")), None);
}

#[test]
fn activity_is_a_page_of_its_own_and_saving_hands_over_every_line() {
    let mut state = with_sites();
    state.update(Event::SessionLine("first".to_string()));
    state.update(Event::ActivityPressed);
    assert_eq!(state.page(), Page::Activity);
    assert_eq!(
        state.update(Event::SaveLogPressed),
        [Effect::SaveLog(vec!["first".to_string()])]
    );
    state.update(Event::LogSaved(Ok(PathBuf::from(
        "D:\\Shared\\Desktop\\snisplit-log.txt",
    ))));
    assert_eq!(log_of(&state), ["first"]);
    state.update(Event::LogSaved(Err("disk full".to_string())));
    assert_eq!(
        last_log(&state),
        "could not save the activity file: disk full"
    );
    state.update(Event::BackPressed);
    assert_eq!(state.page(), Page::Sites);
}

#[test]
fn on_time_survives_a_restart_and_ends_with_the_session() {
    let mut state = with_sites();
    let first = Instant::now();
    state.update(Event::StartPressed);
    ready(&mut state, first);
    state.update(switched("medium", false));
    state.update(Event::SessionEnded(SessionEnd::Clean));
    ready(&mut state, first + Duration::from_secs(90));
    assert_eq!(state.on_since(), Some(first));
    state.update(Event::StopPressed);
    state.update(Event::SessionEnded(SessionEnd::Clean));
    assert_eq!(state.on_since(), None);
}

#[test]
fn the_log_keeps_the_newest_lines_up_to_its_bound() {
    let mut state = State::new();
    for number in 0..LOG_LINES + 3 {
        state.update(Event::SessionLine(format!("line {number}")));
    }
    let log = log_of(&state);
    assert_eq!(log.len(), 500);
    assert_eq!(log[0], "line 3");
    assert_eq!(log[499], "line 502");
}

#[test]
fn the_site_list_titles_sites_and_switches_off_the_listed_names() {
    let spec = |name: &str, domain: &str| ProfileSpec {
        name: name.to_string(),
        domains: vec![domain.to_string()],
        tls_ports: vec![443],
        tls_positions: Vec::new(),
        http_ports: vec![80],
        http_method: HttpMethod::HostSpell,
    };
    let files = vec![
        SiteFile {
            name: "news-example-com".to_string(),
            loaded: Ok(spec("news-example-com", "news.example.com")),
        },
        SiteFile {
            name: "my-sites".to_string(),
            loaded: Ok(spec("my-sites", "example.com")),
        },
        SiteFile {
            name: "bad".to_string(),
            loaded: Err("bad.profile:1: unknown key \"x\"".to_string()),
        },
    ];
    let off = [
        "medium".to_string(),
        "my-sites".to_string(),
        "ghost".to_string(),
    ];
    let sites = site_list(&built_in_specs().unwrap(), files, &off);
    let summary: Vec<(&str, &str, bool)> = sites
        .iter()
        .map(|site| (site.name.as_str(), site.title.as_str(), site.on))
        .collect();
    assert_eq!(
        summary,
        [
            ("steam", "Steam", true),
            ("medium", "Medium", false),
            ("news-example-com", "news.example.com", true),
            ("my-sites", "my-sites", false),
            ("bad", "bad", false),
        ]
    );
    assert_eq!(sites[1].domains, ["medium.com"]);
    assert_eq!(
        sites[4].kind,
        SiteKind::Broken("bad.profile:1: unknown key \"x\"".to_string())
    );
    assert!(!sites[0].removable());
    assert!(sites[2].removable() && sites[4].removable());
    assert!(!sites[4].switchable());
    assert_eq!(sites[0].test_domain(), Some("store.steampowered.com"));
    assert_eq!(sites[1].test_domain(), Some("medium.com"));
    assert_eq!(sites[2].test_domain(), Some("news.example.com"));
    assert_eq!(sites[4].test_domain(), None);
}

fn launched(claimed: Claimed) -> State {
    let mut state = with_sites();
    assert_eq!(
        state.update(Event::Launched),
        [Effect::LoadSites, Effect::ClaimInstance]
    );
    state.update(Event::InstanceClaimed(claimed));
    state
}

#[test]
fn a_claimed_instance_counts_leftovers_and_rules_found_show_the_leftovers_from_last_time() {
    let mut state = with_sites();
    state.update(Event::Launched);
    assert_eq!(
        state.update(Event::InstanceClaimed(Claimed::Ours)),
        [Effect::CountLeftovers]
    );
    assert_eq!(state.relay(), Relay::Off);
    state.update(Event::LeftoversCounted(Ok(2)));
    assert_eq!(
        state.relay(),
        Relay::Leftovers {
            removal_refused: false
        }
    );
    assert!(state.leftovers_from_last_time());
    assert_eq!(
        last_log(&state),
        "snisplit DNS rules left from an earlier run were found"
    );
    assert_eq!(
        state.update(Event::RemoveLeftoversPressed),
        [Effect::RemoveLeftovers]
    );
    state.update(Event::LeftoversRemoved(Ok(())));
    assert_eq!(state.relay(), Relay::Off);
    assert!(!state.leftovers_from_last_time());
}

#[test]
fn start_still_works_from_leftovers_found_at_launch_and_forgets_where_they_came_from() {
    let mut state = launched(Claimed::Ours);
    state.update(Event::LeftoversCounted(Ok(1)));
    assert_eq!(state.update(Event::StartPressed), [Effect::StartSession]);
    assert!(!state.leftovers_from_last_time());
}

#[test]
fn no_rules_found_at_launch_stays_off() {
    let mut state = launched(Claimed::Ours);
    state.update(Event::LeftoversCounted(Ok(0)));
    assert_eq!(state.relay(), Relay::Off);
    assert!(!state.leftovers_from_last_time());
}

#[test]
fn a_failed_check_logs_the_raw_cause_and_stays_off() {
    let mut state = launched(Claimed::Ours);
    state.update(Event::LeftoversCounted(Err(
        "powershell: Access is denied.".to_string(),
    )));
    assert_eq!(state.relay(), Relay::Off);
    assert_eq!(
        last_log(&state),
        "could not check for leftover DNS rules: powershell: Access is denied."
    );
}

#[test]
fn a_count_arriving_after_start_was_pressed_changes_nothing() {
    let mut state = launched(Claimed::Ours);
    state.update(Event::StartPressed);
    state.update(Event::LeftoversCounted(Ok(3)));
    assert_eq!(state.relay(), Relay::Starting);
    let mut stopped = launched(Claimed::Ours);
    stopped.update(Event::StartPressed);
    stopped.update(Event::StopPressed);
    stopped.update(Event::SessionEnded(SessionEnd::Clean));
    assert_eq!(stopped.relay(), Relay::Off);
    stopped.update(Event::LeftoversCounted(Ok(3)));
    assert_eq!(stopped.relay(), Relay::Off);
    assert!(!stopped.leftovers_from_last_time());
}

#[test]
fn no_lock_is_logged_and_the_leftovers_are_still_counted() {
    let mut state = with_sites();
    state.update(Event::Launched);
    assert_eq!(
        state.update(Event::InstanceClaimed(Claimed::Unlocked(
            "Access is denied. (os error 5)".to_string()
        ))),
        [Effect::CountLeftovers]
    );
    assert_eq!(state.relay(), Relay::Off);
    assert_eq!(
        last_log(&state),
        "not checking for another running snisplit: Access is denied. (os error 5)"
    );
}

#[test]
fn already_open_refuses_start_switches_and_leftover_removal_until_a_retry_claims_it() {
    let mut state = launched(Claimed::Elsewhere);
    assert_eq!(state.relay(), Relay::AlreadyOpen);
    assert_eq!(state.update(Event::StartPressed), []);
    assert_eq!(state.update(Event::SwitchPressed(id("medium"))), []);
    assert_eq!(state.update(Event::RemoveLeftoversPressed), []);
    assert_eq!(state.relay(), Relay::AlreadyOpen);
    assert_eq!(
        state.update(Event::ClaimAgainPressed),
        [Effect::ClaimInstance]
    );
    state.update(Event::InstanceClaimed(Claimed::Elsewhere));
    assert_eq!(state.relay(), Relay::AlreadyOpen);
    assert_eq!(
        state.update(Event::InstanceClaimed(Claimed::Ours)),
        [Effect::CountLeftovers]
    );
    assert_eq!(state.relay(), Relay::Off);
    assert_eq!(state.update(Event::ClaimAgainPressed), []);
    assert_eq!(state.update(Event::StartPressed), [Effect::StartSession]);
}

#[test]
fn closing_while_already_open_exits_at_once() {
    let mut state = launched(Claimed::Elsewhere);
    assert_eq!(state.update(Event::CloseRequested), [Effect::Exit]);
}

fn id(name: &str) -> SiteId {
    match name {
        "steam" | "medium" => SiteId::built_in(name),
        _ => SiteId::file(name),
    }
}

fn with_a_broken_steam_file() -> State {
    loaded(vec![
        site(
            "steam",
            "Steam",
            &["steampowered.com"],
            SiteKind::BuiltIn,
            true,
        ),
        site(
            "steam",
            "steam",
            &[],
            SiteKind::Broken("steam.profile:1: unknown key \"x\"".to_string()),
            false,
        ),
    ])
}

fn kinds(state: &State) -> Vec<(String, SiteKind)> {
    state
        .sites()
        .iter()
        .map(|site| (site.name.clone(), site.kind.clone()))
        .collect()
}

#[test]
fn a_broken_file_named_like_a_built_in_opens_asks_and_is_deleted_on_its_own() {
    let mut state = with_a_broken_steam_file();
    state.update(Event::RowPressed(SiteId::file("steam")));
    assert_eq!(*state.open(), Open::Site(SiteId::file("steam")));
    state.update(Event::RemovePressed);
    assert!(state.confirming_remove());
    assert_eq!(last_log(&state), "remove steam asked");
    assert_eq!(
        state.update(Event::RemoveConfirmed),
        [Effect::DeleteSite("steam".to_string())]
    );
    assert_eq!(
        state.update(Event::SiteRemoved(Ok("steam".to_string()))),
        [Effect::LoadSites]
    );
    assert_eq!(kinds(&state), [("steam".to_string(), SiteKind::BuiltIn)]);
    assert_eq!(*state.open(), Open::Nothing);
}

#[test]
fn the_built_in_of_the_same_name_opens_apart_from_the_file_and_offers_no_removal() {
    let mut state = with_a_broken_steam_file();
    state.update(Event::RowPressed(SiteId::built_in("steam")));
    assert_eq!(*state.open(), Open::Site(SiteId::built_in("steam")));
    state.update(Event::RemovePressed);
    assert!(!state.confirming_remove());
    assert_eq!(state.update(Event::RemoveConfirmed), []);
    assert_eq!(
        state.update(Event::SwitchPressed(SiteId::built_in("steam"))),
        [Effect::SetSwitch {
            name: "steam".to_string(),
            on: false
        }]
    );
    assert_eq!(
        state.update(Event::SwitchPressed(SiteId::file("steam"))),
        []
    );
}

#[test]
fn deleting_the_broken_file_while_on_keeps_the_built_in_running_without_a_restart() {
    let mut state = with_a_broken_steam_file();
    state.update(Event::StartPressed);
    ready(&mut state, Instant::now());
    assert_eq!(state.relay(), Relay::On);
    assert_eq!(
        state.update(Event::SiteRemoved(Ok("steam".to_string()))),
        [Effect::LoadSites]
    );
    assert_eq!(state.relay(), Relay::On);
    assert!(!state.applying());
    assert_eq!(kinds(&state), [("steam".to_string(), SiteKind::BuiltIn)]);
}
