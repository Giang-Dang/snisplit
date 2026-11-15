use std::time::{Duration, Instant};

use snisplit::session::Failure;
use snisplit_core::probe::Verdict;

use super::*;
use crate::state::{Claimed, ProbeView, Row, SessionEnd, Site, SiteKind};

fn site(name: &str, title: &str, kind: SiteKind, on: bool) -> Site {
    let domains = match name {
        "steam" => vec![
            "steampowered.com".to_string(),
            "steamstatic.com".to_string(),
        ],
        "medium" => vec!["medium.com".to_string()],
        _ => vec![title.to_string()],
    };
    Site {
        name: name.to_string(),
        title: title.to_string(),
        domains,
        kind,
        on,
    }
}

fn state_with(sites: Vec<Site>) -> State {
    let mut state = State::new();
    state.update(Event::SitesLoaded(sites));
    state
}

fn three_sites() -> State {
    state_with(vec![
        site("steam", "Steam", SiteKind::BuiltIn, true),
        site("medium", "Medium", SiteKind::BuiltIn, false),
        site("example-com", "example.com", SiteKind::User, true),
    ])
}

fn on(mut state: State, at: Instant) -> State {
    state.update(Event::StartPressed);
    state.update(Event::SessionReady { at });
    state
}

fn ended(failure: Failure) -> Event {
    Event::SessionEnded(SessionEnd::Failed {
        failure,
        detail: "raw cause".to_string(),
    })
}

fn labels(buttons: &[Button]) -> Vec<&str> {
    buttons.iter().map(|button| button.label.as_str()).collect()
}

fn result(domain: &str, verdict: Verdict) -> ProbeView {
    ProbeView {
        domain: domain.to_string(),
        address: "192.0.2.10".to_string(),
        verdict,
        profile: (verdict == Verdict::ProfileSuggested)
            .then(|| "name example-net\ndomains example.net\n".to_string()),
        rows: vec![Row {
            attempt: "Normal secure connection".to_string(),
            outcomes: vec!["cut off".to_string(), "cut off".to_string()],
        }],
        details: Vec::new(),
    }
}

fn panel_after(verdict: Verdict, at: Instant) -> (State, AddPanel) {
    let mut state = three_sites();
    state.update(Event::AddRowPressed);
    state.update(Event::FieldEdited("example.net".to_string()));
    state.update(Event::AddTestPressed { at });
    state.update(Event::ProbeFinished {
        id: 1,
        result: Ok(result("example.net", verdict)),
        at,
    });
    let panel = add_panel(&state, at).unwrap();
    (state, panel)
}

#[test]
fn off_names_start_and_says_what_it_does() {
    let hero = hero(&three_sites());
    assert_eq!(hero.ring, Ring::Hollow);
    assert_eq!(hero.action, "Start");
    assert_eq!(hero.press, Some(Event::StartPressed));
    assert_eq!(hero.state_line, "Off");
    assert_eq!(
        hero.sentences,
        ["The sites switched on below will not open until you press Start."]
    );
}

#[test]
fn with_every_site_off_start_is_disabled_and_the_hint_says_why() {
    let state = state_with(vec![
        site("steam", "Steam", SiteKind::BuiltIn, false),
        site("medium", "Medium", SiteKind::BuiltIn, false),
    ]);
    let hero = hero(&state);
    assert_eq!(hero.action, "Start");
    assert_eq!(hero.press, None);
    assert_eq!(
        hero.sentences,
        ["Turn on at least one site below, then press Start."]
    );
}

#[test]
fn starting_turns_the_arc_and_offers_cancel() {
    let mut state = three_sites();
    state.update(Event::StartPressed);
    let hero = hero(&state);
    assert_eq!(hero.ring, Ring::Turning);
    assert_eq!(hero.action, "Cancel");
    assert_eq!(hero.press, Some(Event::StopPressed));
    assert_eq!(hero.state_line, "Starting...");
    assert_eq!(hero.sentences, ["About 2 seconds."]);
}

#[test]
fn on_fills_the_button_names_the_open_sites_and_counts_the_minutes() {
    let since = Instant::now();
    let state = on(three_sites(), since);
    let hero = hero(&state);
    assert_eq!(hero.ring, Ring::Filled);
    assert_eq!(hero.tone, Tone::On);
    assert_eq!(hero.action, "Stop");
    assert_eq!(hero.state_line, "On");
    assert_eq!(
        hero.sentences,
        [
            "Steam and example.com open through snisplit.",
            "Keep this window open: closing it turns snisplit off.",
            "If a site still does not open, close and reopen your browser."
        ]
    );
    assert_eq!(
        hero.compact,
        "Steam and example.com open through snisplit. Closing this window turns it off."
    );
    assert_eq!(
        footer_note(&state, since + Duration::from_secs(59)),
        "on for under 1 min"
    );
    assert_eq!(
        footer_note(&state, since + Duration::from_secs(125)),
        "on for 2 min"
    );
}

#[test]
fn site_names_are_listed_while_short_and_counted_otherwise() {
    let names = |titles: &[&str]| {
        sites_phrase(
            &titles
                .iter()
                .map(|title| title.to_string())
                .collect::<Vec<_>>(),
        )
    };
    assert_eq!(names(&["Steam"]), ("Steam".to_string(), false));
    assert_eq!(
        names(&["Steam", "Medium", "example.com"]),
        ("Steam, Medium and example.com".to_string(), true)
    );
    assert_eq!(
        names(&["Steam", "Medium", "example.com", "example.net"]),
        ("4 sites".to_string(), true)
    );
    assert_eq!(
        names(&["Steam", "a-very-long-site-name.example.org"]),
        ("2 sites".to_string(), true)
    );
    let one = on(
        state_with(vec![site("steam", "Steam", SiteKind::BuiltIn, true)]),
        Instant::now(),
    );
    assert_eq!(hero(&one).sentences[0], "Steam opens through snisplit.");
}

#[test]
fn a_switch_change_while_on_shows_applying_and_the_switch_turning() {
    let mut state = on(three_sites(), Instant::now());
    state.update(Event::SwitchSaved(Ok(("medium".to_string(), true))));
    let hero = hero(&state);
    assert_eq!(hero.action, "Wait");
    assert_eq!(hero.press, None);
    assert_eq!(hero.ring, Ring::Turning);
    assert_eq!(hero.tone, Tone::On);
    assert_eq!(hero.state_line, "Applying your change...");
    assert_eq!(
        hero.sentences,
        ["snisplit restarts for about 2 seconds. Sites may pause briefly."]
    );
    assert_eq!(
        footer_note(&state, Instant::now()),
        "several quick changes apply together"
    );
    let medium = rows(&state).remove(1);
    assert_eq!(
        medium.switch,
        Some(Switch {
            on: true,
            label: "Turning on",
            busy: true,
            enabled: false
        })
    );
    state.update(Event::SessionEnded(SessionEnd::Clean));
    assert_eq!(hero_state_line(&state), "Applying your change...");
    assert_eq!(view_action(&state), "Cancel");
}

fn hero_state_line(state: &State) -> &'static str {
    hero(state).state_line
}

fn view_action(state: &State) -> &'static str {
    hero(state).action
}

#[test]
fn an_explicit_stop_turns_off_and_waits() {
    let mut state = on(three_sites(), Instant::now());
    state.update(Event::StopPressed);
    let hero = hero(&state);
    assert_eq!(hero.state_line, "Turning off...");
    assert_eq!(hero.action, "Wait");
    assert_eq!(hero.press, None);
    assert!(hero.sentences.is_empty());
}

#[test]
fn closing_while_on_says_the_window_closes_by_itself() {
    let mut state = on(three_sites(), Instant::now());
    state.update(Event::CloseRequested);
    let hero = hero(&state);
    assert_eq!(hero.state_line, "Turning off...");
    assert_eq!(hero.action, "Wait");
    assert_eq!(
        hero.sentences,
        [
            "This window closes by itself in a moment. Closing it again now may leave sites unable to open until you start snisplit once more."
        ]
    );
    assert!(
        rows(&state)
            .iter()
            .all(|row| row.switch.as_ref().is_none_or(|switch| !switch.enabled))
    );
}

#[test]
fn each_failure_cause_has_its_own_sentence_and_the_raw_cause_stays_in_activity() {
    let cases = [
        (
            Failure::Binding,
            "Another program is using something snisplit needs. Close other copies of snisplit, then try again.",
            "nothing was changed on this PC",
        ),
        (
            Failure::ChangingRules,
            "Windows refused the change snisplit needs. If you did not start it with Run as administrator, close this window, right-click snisplit-gui and choose Run as administrator.",
            "nothing was changed on this PC",
        ),
        (
            Failure::Other,
            "snisplit stopped unexpectedly.",
            "version 0.2.0",
        ),
    ];
    for (failure, sentence, footer) in cases {
        let mut state = three_sites();
        state.update(Event::StartPressed);
        state.update(ended(failure));
        let hero = hero(&state);
        assert_eq!(hero.ring, Ring::Warning);
        assert_eq!(hero.tone, Tone::Trouble);
        assert_eq!(hero.action, "Try again");
        assert_eq!(hero.press, Some(Event::StartPressed));
        assert_eq!(hero.state_line, "Could not start");
        assert_eq!(hero.sentences, [sentence]);
        assert_eq!(labels(&hero.buttons), ["See what happened"]);
        assert_eq!(hero.buttons[0].press, Event::ActivityPressed);
        assert!(!hero.sentences[0].contains("raw cause"));
        assert_eq!(footer_note(&state, Instant::now()), footer);
    }
}

#[test]
fn leftovers_name_the_sites_offer_the_removal_and_report_its_outcome() {
    let mut state = on(three_sites(), Instant::now());
    state.update(Event::StopPressed);
    state.update(ended(Failure::RemovingRules));
    let hero = hero(&state);
    assert_eq!(hero.ring, Ring::Warning);
    assert_eq!(hero.action, "Start");
    assert_eq!(hero.state_line, "Off, with leftovers");
    assert_eq!(
        hero.sentences,
        [
            "snisplit could not tidy up after itself. Steam and example.com may not open until the leftover settings are removed."
        ]
    );
    assert_eq!(
        labels(&hero.buttons),
        ["Remove leftover settings", "See what happened"]
    );
    assert_eq!(hero.buttons[0].style, Style::Light);
    assert_eq!(
        footer_note(&state, Instant::now()),
        "pressing Start also removes them"
    );
    state.update(Event::RemoveLeftoversPressed);
    let removing = view_hero(&state);
    assert_eq!(removing.state_line, "Removing leftover settings...");
    assert_eq!(removing.action, "Wait");
    assert_eq!(removing.press, None);
    state.update(Event::LeftoversRemoved(Err("denied".to_string())));
    assert_eq!(
        view_hero(&state).sentences[1],
        "Windows refused to remove them. If you did not start it with Run as administrator, close this window, right-click snisplit-gui and choose Run as administrator."
    );
    state.update(Event::RemoveLeftoversPressed);
    state.update(Event::LeftoversRemoved(Ok(())));
    let cleaned = view_hero(&state);
    assert_eq!(cleaned.state_line, "Off");
    assert_eq!(
        cleaned.sentences,
        [
            "The leftover settings were removed.",
            "The sites switched on below will not open until you press Start."
        ]
    );
}

fn view_hero(state: &State) -> Hero {
    hero(state)
}

fn launched(claimed: Claimed) -> State {
    let mut state = three_sites();
    state.update(Event::Launched);
    state.update(Event::InstanceClaimed(claimed));
    state
}

#[test]
fn leftovers_found_at_launch_say_it_did_not_turn_off_last_time_and_offer_the_removal() {
    let mut state = launched(Claimed::Ours);
    state.update(Event::LeftoversCounted(Ok(3)));
    let hero = hero(&state);
    assert_eq!(hero.ring, Ring::Warning);
    assert_eq!(hero.tone, Tone::Trouble);
    assert_eq!(hero.action, "Start");
    assert_eq!(hero.press, Some(Event::StartPressed));
    assert_eq!(hero.state_line, "Off, with leftovers");
    assert_eq!(
        hero.sentences,
        [
            "snisplit did not get to turn off properly last time. Some of its settings are still in place, so the sites below may not open."
        ]
    );
    assert_eq!(
        labels(&hero.buttons),
        ["Remove leftover settings", "See what happened"]
    );
    assert_eq!(
        footer_note(&state, Instant::now()),
        "pressing Start also removes them"
    );
    state.update(Event::RemoveLeftoversPressed);
    state.update(Event::LeftoversRemoved(Err("denied".to_string())));
    assert_eq!(
        view_hero(&state).sentences,
        [
            "snisplit did not get to turn off properly last time. Some of its settings are still in place, so the sites below may not open.",
            "Windows refused to remove them. If you did not start it with Run as administrator, close this window, right-click snisplit-gui and choose Run as administrator."
        ]
    );
}

#[test]
fn already_open_offers_only_try_again_and_disables_every_switch() {
    let state = launched(Claimed::Elsewhere);
    let hero = hero(&state);
    assert_eq!(hero.ring, Ring::Warning);
    assert_eq!(hero.action, "Try again");
    assert_eq!(hero.press, Some(Event::ClaimAgainPressed));
    assert_eq!(hero.state_line, "Already open");
    assert_eq!(
        hero.sentences,
        [
            "snisplit is already running, or is still turning off. Wait a few seconds, then press Try again."
        ]
    );
    assert!(hero.buttons.is_empty());
    let enabled: Vec<bool> = rows(&state)
        .iter()
        .filter_map(|row| row.switch.as_ref().map(|switch| switch.enabled))
        .collect();
    assert_eq!(enabled, [false, false, false]);
    let mut freed = state.clone();
    freed.update(Event::ClaimAgainPressed);
    freed.update(Event::InstanceClaimed(Claimed::Ours));
    assert_eq!(view_hero(&freed).state_line, "Off");
    assert_eq!(view_hero(&freed).action, "Start");
}

#[test]
fn rows_say_where_a_site_comes_from_and_a_broken_file_has_no_switch() {
    let state = state_with(vec![
        site("steam", "Steam", SiteKind::BuiltIn, true),
        site("example-com", "example.com", SiteKind::User, false),
        site(
            "bad",
            "bad",
            SiteKind::Broken("bad.profile:2: domains needs a value".to_string()),
            false,
        ),
    ]);
    let rows = rows(&state);
    let subs: Vec<&str> = rows.iter().map(|row| row.sub.as_str()).collect();
    assert_eq!(
        subs,
        [
            "comes with snisplit",
            "added by you",
            "This file has a mistake and is skipped: bad.profile:2: domains needs a value"
        ]
    );
    assert_eq!(
        rows[0].switch,
        Some(Switch {
            on: true,
            label: "On",
            busy: false,
            enabled: true
        })
    );
    assert_eq!(
        rows[1].switch.as_ref().map(|switch| switch.label),
        Some("Off")
    );
    assert!(rows[1].dimmed && !rows[0].dimmed);
    assert_eq!(rows[2].switch, None);
}

#[test]
fn an_opened_row_offers_test_and_remove_and_after_a_test_says_what_was_found() {
    let at = Instant::now();
    let mut state = three_sites();
    state.update(Event::RowPressed(id("example-com")));
    let fresh = details(&state, at).unwrap();
    assert_eq!(fresh.domains, "example.com");
    assert_eq!(fresh.sentence, None);
    assert_eq!(labels(&fresh.buttons), ["Test", "Remove"]);
    state.update(Event::TestPressed {
        site: id("example-com"),
        at,
    });
    let testing = details(&state, at + Duration::from_secs(3)).unwrap();
    assert_eq!(
        testing.testing.as_deref(),
        Some("Testing example.com... up to 10 seconds")
    );
    assert_eq!(labels(&testing.buttons), ["Cancel"]);
    let slow = details(&state, at + Duration::from_secs(11)).unwrap();
    assert_eq!(
        slow.testing.as_deref(),
        Some("Testing example.com... this one is taking longer than usual")
    );
    state.update(Event::ProbeFinished {
        id: 1,
        result: Ok(result("example.com", Verdict::ProfileSuggested)),
        at,
    });
    let tested = details(&state, at + Duration::from_secs(5)).unwrap();
    assert_eq!(
        tested.sentence.as_deref(),
        Some("Tested example.com just now: blocked without snisplit, and snisplit can open it.")
    );
    assert_eq!(
        labels(&tested.buttons),
        ["Test again", "Remove", "What snisplit tried"]
    );
    assert!(tested.attempts.is_empty());
    state.update(Event::AttemptsPressed);
    let shown = details(&state, at + Duration::from_secs(185)).unwrap();
    assert_eq!(
        shown.sentence.as_deref(),
        Some("Tested example.com 3 min ago: blocked without snisplit, and snisplit can open it.")
    );
    assert_eq!(shown.buttons[2].label, "Hide what snisplit tried");
    assert_eq!(shown.attempts[0].attempt, "Normal secure connection");
}

#[test]
fn a_built_in_row_has_no_remove_and_a_broken_one_shows_its_mistake_with_remove_only() {
    let at = Instant::now();
    let mut state = state_with(vec![
        site("steam", "Steam", SiteKind::BuiltIn, true),
        site(
            "bad",
            "bad",
            SiteKind::Broken("bad.profile:1: oops".to_string()),
            false,
        ),
    ]);
    state.update(Event::RowPressed(id("steam")));
    let steam = details(&state, at).unwrap();
    assert_eq!(steam.domains, "steampowered.com, steamstatic.com");
    assert_eq!(labels(&steam.buttons), ["Test"]);
    state.update(Event::RowPressed(id("bad")));
    let bad = details(&state, at).unwrap();
    assert_eq!(
        bad.sentence.as_deref(),
        Some("This file has a mistake and is skipped: bad.profile:1: oops")
    );
    assert_eq!(labels(&bad.buttons), ["Remove"]);
}

#[test]
fn test_sentences_name_the_tested_address_for_each_finding() {
    let now = Duration::from_secs(2);
    let domain = "store.steampowered.com";
    assert_eq!(
        test_sentence(domain, Some(Verdict::ProfileSuggested), now),
        "Tested store.steampowered.com just now: blocked without snisplit, and snisplit can open it."
    );
    assert_eq!(
        test_sentence("example.org", Some(Verdict::NotBlocked), now),
        "Tested example.org just now: opens without snisplit."
    );
    for verdict in [Verdict::NoProfileHelps, Verdict::NothingWorked] {
        assert_eq!(
            test_sentence(domain, Some(verdict), now),
            "Tested store.steampowered.com just now: snisplit cannot open it from here."
        );
    }
    assert_eq!(
        test_sentence(domain, None, Duration::from_secs(60)),
        "Could not test store.steampowered.com 1 min ago. Check your internet connection and try again."
    );
}

#[test]
fn removing_asks_with_the_restart_sentence_only_when_it_restarts() {
    let mut state = on(three_sites(), Instant::now());
    state.update(Event::RowPressed(id("example-com")));
    state.update(Event::RemovePressed);
    let asked = details(&state, Instant::now()).unwrap().confirm.unwrap();
    assert_eq!(asked.question, "Remove example.com?");
    assert_eq!(asked.note, Some("snisplit restarts for about 2 seconds."));
    assert_eq!(labels(&asked.buttons), ["Keep", "Remove"]);
    assert_eq!(asked.buttons[1].press, Event::RemoveConfirmed);
    let mut off = three_sites();
    off.update(Event::RowPressed(id("example-com")));
    off.update(Event::RemovePressed);
    assert_eq!(
        details(&off, Instant::now()).unwrap().confirm.unwrap().note,
        None
    );
}

#[test]
fn the_add_panel_starts_with_a_field_and_refuses_a_mistyped_address_with_a_hint() {
    let at = Instant::now();
    let mut state = three_sites();
    state.update(Event::AddRowPressed);
    let empty = add_panel(&state, at).unwrap();
    assert!(empty.field_enabled);
    assert_eq!(labels(&empty.buttons), ["Test", "Close"]);
    assert!(!empty.buttons[0].enabled);
    state.update(Event::FieldEdited("my site".to_string()));
    state.update(Event::AddTestPressed { at });
    let mistyped = add_panel(&state, at).unwrap();
    assert_eq!(mistyped.field, "my site");
    assert_eq!(
        mistyped.hint,
        Some(
            "Type only the address, like example.net. A pasted link such as https://example.net/page is accepted and shortened."
        )
    );
    assert!(mistyped.buttons[0].enabled);
    state.update(Event::FieldEdited("caf\u{e9}.example.com".to_string()));
    state.update(Event::AddTestPressed { at });
    assert_eq!(
        add_panel(&state, at).unwrap().hint,
        Some(
            "Addresses with letters other than a to z, such as accented letters or other scripts, are not supported yet. Type an address like example.net."
        )
    );
}

#[test]
fn while_testing_the_field_is_locked_and_cancel_is_offered() {
    let at = Instant::now();
    let mut state = three_sites();
    state.update(Event::AddRowPressed);
    state.update(Event::FieldEdited("example.net".to_string()));
    state.update(Event::AddTestPressed { at });
    let panel = add_panel(&state, at).unwrap();
    assert!(!panel.field_enabled);
    assert_eq!(panel.field, "example.net");
    assert_eq!(
        panel.testing.as_deref(),
        Some("Testing example.net... up to 10 seconds")
    );
    assert_eq!(labels(&panel.buttons), ["Cancel"]);
}

#[test]
fn each_test_outcome_has_the_mockup_words_and_buttons() {
    let at = Instant::now();
    let (_, blocked) = panel_after(Verdict::ProfileSuggested, at);
    assert_eq!(blocked.mark, Some(Mark::Opens));
    assert_eq!(
        blocked.headline.as_deref(),
        Some("example.net is blocked here, and snisplit can open it.")
    );
    assert_eq!(
        labels(&blocked.buttons),
        ["Add and turn on", "Not now", "What snisplit tried"]
    );
    assert_eq!(blocked.buttons[0].style, Style::Primary);
    let (_, opens) = panel_after(Verdict::NotBlocked, at);
    assert_eq!(opens.mark, Some(Mark::Same));
    assert_eq!(
        opens.headline.as_deref(),
        Some("example.net already opens normally. Nothing to add.")
    );
    assert_eq!(
        opens.sub,
        Some(
            "If it fails in your browser anyway, the cause is something snisplit does not handle."
        )
    );
    assert_eq!(
        labels(&opens.buttons),
        ["Done", "Test another", "What snisplit tried"]
    );
    let (_, dead) = panel_after(Verdict::NothingWorked, at);
    assert_eq!(dead.mark, Some(Mark::Cannot));
    assert_eq!(
        dead.headline.as_deref(),
        Some("snisplit cannot open example.net from here.")
    );
    assert_eq!(
        dead.sub,
        Some(
            "Nothing snisplit tried got an answer. The site may be down, or blocked in a way snisplit cannot get around."
        )
    );
    assert_eq!(
        labels(&dead.buttons),
        ["Close", "Test another", "What snisplit tried"]
    );
    let (_, partly) = panel_after(Verdict::NoProfileHelps, at);
    assert_eq!(
        partly.sub,
        Some(
            "Part of what snisplit tried got an answer, but snisplit has no setting that opens the whole site."
        )
    );
    let (mut state, _) = panel_after(Verdict::NotBlocked, at);
    state.update(Event::AttemptsPressed);
    let shown = add_panel(&state, at).unwrap();
    assert_eq!(shown.buttons[2].label, "Hide what snisplit tried");
    assert_eq!(shown.attempts.len(), 1);
}

#[test]
fn a_failed_test_offers_try_again_and_a_covered_save_names_the_owner() {
    let at = Instant::now();
    let mut state = three_sites();
    state.update(Event::AddRowPressed);
    state.update(Event::FieldEdited("example.net".to_string()));
    state.update(Event::AddTestPressed { at });
    state.update(Event::ProbeFinished {
        id: 1,
        result: Err("timed out".to_string()),
        at,
    });
    let failed = add_panel(&state, at).unwrap();
    assert_eq!(failed.mark, Some(Mark::Trouble));
    assert_eq!(
        failed.headline.as_deref(),
        Some("Could not test example.net.")
    );
    assert_eq!(
        failed.sub,
        Some("Check your internet connection and try again.")
    );
    assert_eq!(labels(&failed.buttons), ["Try again", "Close"]);
    let (mut covered, _) = panel_after(Verdict::ProfileSuggested, at);
    covered.update(Event::SiteSaved(Err(crate::state::SaveRefusal::Covered {
        domain: "example.net".to_string(),
        owner: "example-com".to_string(),
    })));
    let refused = add_panel(&covered, at).unwrap();
    assert_eq!(
        refused.headline.as_deref(),
        Some("example.net is already covered by example.com.")
    );
    assert_eq!(labels(&refused.buttons), ["Close", "Test another"]);
}

#[test]
fn a_save_that_fails_for_another_reason_says_so_and_points_at_activity() {
    let at = Instant::now();
    let (mut state, _) = panel_after(Verdict::ProfileSuggested, at);
    state.update(Event::SiteSaved(Err(crate::state::SaveRefusal::Other(
        "a site named example-net already exists".to_string(),
    ))));
    let panel = add_panel(&state, at).unwrap();
    assert_eq!(panel.mark, Some(Mark::Trouble));
    assert_eq!(
        panel.headline.as_deref(),
        Some("Could not add example.net.")
    );
    assert_eq!(labels(&panel.buttons), ["Close", "See what happened"]);
}

#[test]
fn an_unexpected_end_after_it_was_on_says_off_not_could_not_start() {
    let mut state = on(three_sites(), Instant::now());
    state.update(ended(Failure::Other));
    let hero = hero(&state);
    assert_eq!(hero.state_line, "Off");
    assert_eq!(hero.action, "Try again");
    assert_eq!(hero.sentences, ["snisplit stopped unexpectedly."]);
}

#[test]
fn activity_counts_lines_and_says_where_the_file_was_saved() {
    let mut state = State::new();
    state.update(Event::SessionLine("starting snisplit".to_string()));
    assert_eq!(lines_count(&state), "1 line");
    state.update(Event::SessionLine("stopping".to_string()));
    assert_eq!(lines_count(&state), "2 lines");
    assert_eq!(
        save_note(&state),
        "Saves snisplit-log.txt to send to whoever helps you, and shows where it went."
    );
    state.update(Event::LogSaved(Ok(std::path::PathBuf::from(
        "D:\\Shared\\Desktop\\snisplit-log.txt",
    ))));
    assert_eq!(
        save_note(&state),
        "Saved as D:\\Shared\\Desktop\\snisplit-log.txt"
    );
    state.update(Event::LogSaved(Err("access denied".to_string())));
    assert_eq!(save_note(&state), "Could not save the file: access denied");
    assert_eq!(footer_note(&state, Instant::now()), "version 0.2.0");
}

fn id(name: &str) -> SiteId {
    match name {
        "steam" | "medium" => SiteId::built_in(name),
        _ => SiteId::file(name),
    }
}

#[test]
fn a_broken_file_named_like_a_built_in_is_its_own_row_with_remove_and_the_built_in_keeps_its_switch()
 {
    let at = Instant::now();
    let mut state = state_with(vec![
        site("steam", "Steam", SiteKind::BuiltIn, true),
        site(
            "steam",
            "steam",
            SiteKind::Broken("steam.profile:1: oops".to_string()),
            false,
        ),
    ]);
    state.update(Event::RowPressed(SiteId::file("steam")));
    let listed = rows(&state);
    let opened: Vec<bool> = listed.iter().map(|row| row.open).collect();
    assert_eq!(opened, [false, true]);
    assert!(listed[0].switch.is_some() && listed[1].switch.is_none());
    let broken = details(&state, at).unwrap();
    assert_eq!(
        broken.sentence.as_deref(),
        Some("This file has a mistake and is skipped: steam.profile:1: oops")
    );
    assert_eq!(labels(&broken.buttons), ["Remove"]);
    state.update(Event::RemovePressed);
    assert_eq!(
        details(&state, at)
            .unwrap()
            .confirm
            .map(|confirm| confirm.question),
        Some("Remove steam?".to_string())
    );
    state.update(Event::RowPressed(SiteId::built_in("steam")));
    assert_eq!(labels(&details(&state, at).unwrap().buttons), ["Test"]);
}
