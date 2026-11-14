use snisplit::sites::{SiteFile, built_in_specs};
use snisplit_core::first_flight::HttpMethod;

use super::*;

const NEWS_PROFILE: &str = "name news-example-com\ndomains news.example.com\n";

fn site(name: &str, domains: &[&str], kind: SiteKind) -> Site {
    Site {
        name: name.to_string(),
        domains: domains.iter().map(|domain| domain.to_string()).collect(),
        kind,
    }
}

fn with_sites() -> State {
    let mut state = State::new();
    state.update(Event::SitesLoaded(vec![
        site("medium", &["medium.com"], SiteKind::BuiltIn),
        site(
            "news",
            &["news.example.com", "cdn.example.com"],
            SiteKind::User,
        ),
        site(
            "bad",
            &[],
            SiteKind::Broken("bad.profile: oops".to_string()),
        ),
    ]));
    state
}

fn running() -> State {
    let mut state = with_sites();
    state.update(Event::StartPressed);
    state.update(Event::SessionReady);
    assert_eq!(state.relay, Relay::Running);
    state
}

fn view(offer: Offer) -> ProbeView {
    ProbeView {
        domain: "news.example.com".to_string(),
        address: "192.0.2.10".to_string(),
        rows: Vec::new(),
        summary: "summary".to_string(),
        offer,
    }
}

fn probed(purpose: Purpose, offer: Offer) -> State {
    let mut state = with_sites();
    let pressed = match purpose {
        Purpose::Add => Event::ProbePressed("news.example.com".to_string()),
        Purpose::Check => Event::CheckPressed("news".to_string()),
    };
    assert_eq!(
        state.update(pressed),
        [Effect::Probe("news.example.com".to_string())]
    );
    state.update(Event::ProbeFinished {
        domain: "news.example.com".to_string(),
        result: Ok(view(offer)),
    });
    state
}

fn log_of(state: &State) -> Vec<String> {
    state.log().map(str::to_string).collect()
}

#[test]
fn start_ready_stop_walks_the_relay_states_with_one_button() {
    let mut state = State::new();
    assert_eq!(state.start_stop_label(), Some("Start"));
    assert_eq!(state.update(Event::StartPressed), [Effect::StartSession]);
    assert_eq!(state.relay, Relay::Starting);
    assert_eq!(state.start_stop_label(), Some("Stop"));
    assert_eq!(state.update(Event::StartPressed), []);
    state.update(Event::SessionReady);
    assert_eq!(state.relay, Relay::Running);
    assert_eq!(state.update(Event::StopPressed), [Effect::StopSession]);
    assert_eq!(state.relay, Relay::Stopping);
    assert_eq!(state.start_stop_label(), None);
    assert_eq!(state.update(Event::StopPressed), []);
    assert_eq!(state.update(Event::SessionEnded(SessionEnd::Clean)), []);
    assert_eq!(state.relay, Relay::Stopped);
    assert_eq!(state.start_stop_label(), Some("Start"));
}

#[test]
fn a_stop_during_start_up_is_not_undone_by_a_late_ready() {
    let mut state = State::new();
    state.update(Event::StartPressed);
    assert_eq!(state.update(Event::StopPressed), [Effect::StopSession]);
    state.update(Event::SessionReady);
    assert_eq!(state.relay, Relay::Stopping);
}

#[test]
fn a_routing_failure_says_administrator_rights_are_needed_and_start_can_be_pressed_again() {
    let mut state = State::new();
    state.update(Event::StartPressed);
    state.update(Event::SessionEnded(SessionEnd::Failed {
        message: "powershell: Access is denied.".to_string(),
        routing: true,
    }));
    assert_eq!(
        state.relay,
        Relay::Failed(
            "Adding the DNS rules failed. snisplit needs administrator rights for that: close this window and start snisplit-gui again with Run as administrator. (powershell: Access is denied.)"
                .to_string()
        )
    );
    assert!(
        state
            .status()
            .starts_with("Failed: Adding the DNS rules failed.")
    );
    assert_eq!(state.start_stop_label(), Some("Start"));
    assert_eq!(state.update(Event::StartPressed), [Effect::StartSession]);
}

#[test]
fn any_other_failure_shows_its_own_message_only() {
    let mut state = State::new();
    state.update(Event::StartPressed);
    state.update(Event::SessionEnded(SessionEnd::Failed {
        message: "address in use".to_string(),
        routing: false,
    }));
    assert_eq!(state.relay, Relay::Failed("address in use".to_string()));
    assert_eq!(
        log_of(&state),
        [
            "starting snisplit",
            "snisplit stopped with an error: address in use"
        ]
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
    assert_eq!(state.update(Event::CloseRequested), []);
    assert_eq!(state.update(Event::StartPressed), []);
    assert_eq!(state.relay, Relay::Stopping);
    assert_eq!(
        state.update(Event::SessionEnded(SessionEnd::Clean)),
        [Effect::Exit]
    );
}

#[test]
fn closing_while_starting_stops_the_start_up_and_exits_after_it_ends() {
    let mut state = State::new();
    state.update(Event::StartPressed);
    assert_eq!(state.update(Event::CloseRequested), [Effect::StopSession]);
    let failed = SessionEnd::Failed {
        message: "rules could not be removed".to_string(),
        routing: false,
    };
    assert_eq!(state.update(Event::SessionEnded(failed)), [Effect::Exit]);
}

#[test]
fn a_site_added_while_running_restarts_the_session_and_says_so() {
    let mut state = running();
    assert_eq!(
        state.update(Event::SiteSaved(Ok("shop".to_string()))),
        [Effect::LoadSites, Effect::StopSession]
    );
    assert_eq!(state.relay, Relay::Stopping);
    assert_eq!(
        state.update(Event::SessionEnded(SessionEnd::Clean)),
        [Effect::StartSession]
    );
    assert_eq!(state.relay, Relay::Starting);
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
    state.update(Event::SessionReady);
    assert_eq!(state.update(Event::SessionEnded(SessionEnd::Clean)), []);
    assert_eq!(state.relay, Relay::Stopped);
    assert!(
        log_of(&state)
            .contains(&"site news removed; snisplit starts again once it has stopped".to_string())
    );
}

#[test]
fn a_close_during_a_pending_restart_cancels_the_restart() {
    let mut state = running();
    state.update(Event::SiteSaved(Ok("shop".to_string())));
    assert_eq!(state.update(Event::CloseRequested), []);
    assert_eq!(
        state.update(Event::SessionEnded(SessionEnd::Clean)),
        [Effect::Exit]
    );
}

#[test]
fn a_failed_stop_during_a_restart_does_not_start_again() {
    let mut state = running();
    state.update(Event::SiteRemoved(Ok("news".to_string())));
    let failed = SessionEnd::Failed {
        message: "rules could not be removed".to_string(),
        routing: false,
    };
    assert_eq!(state.update(Event::SessionEnded(failed)), []);
    assert_eq!(
        state.relay,
        Relay::Failed("rules could not be removed".to_string())
    );
}

#[test]
fn a_site_change_while_stopped_only_reloads_the_list() {
    let mut state = with_sites();
    assert_eq!(
        state.update(Event::SiteRemoved(Ok("news".to_string()))),
        [Effect::LoadSites]
    );
    assert_eq!(state.relay, Relay::Stopped);
    assert_eq!(state.notice.as_deref(), Some("site news removed"));
}

#[test]
fn a_typed_domain_is_trimmed_lowercased_and_probed_for_adding() {
    let mut state = State::new();
    assert_eq!(
        state.update(Event::ProbePressed("  News.Example.COM ".to_string())),
        [Effect::Probe("news.example.com".to_string())]
    );
    assert_eq!(
        state.probe,
        Probe::Running {
            domain: "news.example.com".to_string(),
            purpose: Purpose::Add
        }
    );
    assert!(state.probing());
}

#[test]
fn a_typed_text_that_is_not_a_domain_is_refused_without_probing() {
    let mut state = State::new();
    assert_eq!(
        state.update(Event::ProbePressed("localhost".to_string())),
        []
    );
    assert_eq!(
        state.probe,
        Probe::Failed {
            domain: "localhost".to_string(),
            message: "bad domain \"localhost\": needs at least 2 labels".to_string()
        }
    );
}

#[test]
fn one_probe_runs_at_a_time_and_only_its_own_result_is_taken() {
    let mut state = with_sites();
    state.update(Event::CheckPressed("medium".to_string()));
    assert_eq!(state.update(Event::CheckPressed("news".to_string())), []);
    assert_eq!(
        state.update(Event::ProbePressed("example.org".to_string())),
        []
    );
    state.update(Event::ProbeFinished {
        domain: "example.org".to_string(),
        result: Err("stale".to_string()),
    });
    assert!(state.probing());
    state.update(Event::ProbeFinished {
        domain: "medium.com".to_string(),
        result: Err("medium.com has no IPv4 address".to_string()),
    });
    assert_eq!(
        state.probe,
        Probe::Failed {
            domain: "medium.com".to_string(),
            message: "medium.com has no IPv4 address".to_string()
        }
    );
    assert_eq!(
        state.update(Event::CheckPressed("news".to_string())),
        [Effect::Probe("news.example.com".to_string())]
    );
}

#[test]
fn check_probes_the_first_domain_of_a_known_site_only() {
    let mut state = with_sites();
    assert_eq!(state.update(Event::CheckPressed("shop".to_string())), []);
    assert_eq!(state.update(Event::CheckPressed("bad".to_string())), []);
    assert_eq!(
        state.update(Event::CheckPressed("news".to_string())),
        [Effect::Probe("news.example.com".to_string())]
    );
    assert_eq!(
        state.probe,
        Probe::Running {
            domain: "news.example.com".to_string(),
            purpose: Purpose::Check
        }
    );
}

#[test]
fn add_saves_the_suggested_profile_of_an_add_probe_only() {
    let mut checked = probed(Purpose::Check, Offer::Add(NEWS_PROFILE.to_string()));
    assert_eq!(checked.update(Event::AddPressed), []);
    let mut open = probed(Purpose::Add, Offer::NotBlocked);
    assert_eq!(open.update(Event::AddPressed), []);
    let mut blocked = probed(Purpose::Add, Offer::Add(NEWS_PROFILE.to_string()));
    assert_eq!(
        blocked.update(Event::AddPressed),
        [Effect::SaveSite(NEWS_PROFILE.to_string())]
    );
    blocked.update(Event::SiteSaved(Ok("news-example-com".to_string())));
    assert_eq!(blocked.probe, Probe::Idle);
}

#[test]
fn a_refused_save_keeps_the_probe_and_shows_why() {
    let mut state = probed(Purpose::Add, Offer::Add(NEWS_PROFILE.to_string()));
    let refused = "news.example.com is already covered by the site news".to_string();
    assert_eq!(state.update(Event::SiteSaved(Err(refused.clone()))), []);
    assert_eq!(state.notice.as_deref(), Some(refused.as_str()));
    assert!(matches!(state.probe, Probe::Done { .. }));
    assert_eq!(log_of(&state), [refused]);
}

#[test]
fn only_user_and_broken_sites_can_be_removed() {
    let mut state = with_sites();
    assert_eq!(state.update(Event::RemovePressed("medium".to_string())), []);
    assert_eq!(
        state.update(Event::RemovePressed("news".to_string())),
        [Effect::DeleteSite("news".to_string())]
    );
    assert_eq!(
        state.update(Event::RemovePressed("bad".to_string())),
        [Effect::DeleteSite("bad".to_string())]
    );
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
fn the_site_list_shows_built_ins_first_then_user_sites_and_broken_files() {
    let files = vec![
        SiteFile {
            name: "news".to_string(),
            loaded: Ok(ProfileSpec {
                name: "news".to_string(),
                domains: vec!["news.example.com".to_string()],
                tls_ports: vec![443],
                tls_positions: Vec::new(),
                http_ports: vec![80],
                http_method: HttpMethod::HostSpell,
            }),
        },
        SiteFile {
            name: "bad".to_string(),
            loaded: Err("bad.profile:1: unknown key \"x\"".to_string()),
        },
    ];
    let sites = site_list(&built_in_specs().unwrap(), files);
    let names: Vec<&str> = sites.iter().map(|site| site.name.as_str()).collect();
    assert_eq!(names, ["steam", "medium", "news", "bad"]);
    assert_eq!(sites[1], site("medium", &["medium.com"], SiteKind::BuiltIn));
    assert_eq!(
        sites[2],
        site("news", &["news.example.com"], SiteKind::User)
    );
    assert_eq!(
        sites[3].kind,
        SiteKind::Broken("bad.profile:1: unknown key \"x\"".to_string())
    );
    assert!(!sites[0].removable());
    assert!(sites[2].removable() && sites[3].removable());
}
