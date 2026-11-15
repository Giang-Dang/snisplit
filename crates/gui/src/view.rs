use std::time::{Duration, Instant};

use snisplit::session::Failure;
use snisplit_core::probe::Verdict;

use crate::address;
use crate::state::{Adding, Event, Note, Open, Relay, Row, Site, SiteId, SiteKind, State, Target};

const OFF_HINT: &str = "The sites switched on below will not open until you press Start.";
const NOTHING_ON: &str = "Turn on at least one site below, then press Start.";
const STARTING: &str = "About 2 seconds.";
const APPLYING: &str = "snisplit restarts for about 2 seconds. Sites may pause briefly.";
const KEEP_OPEN: &str = "Keep this window open: closing it turns snisplit off.";
const REOPEN_BROWSER: &str = "If a site still does not open, close and reopen your browser.";
const CLOSING_TURNS_OFF: &str = "Closing this window turns it off.";
const CLOSING_WAIT: &str = "This window closes by itself in a moment. Closing it again now may leave sites unable to open until you start snisplit once more.";
const IN_USE: &str = "Another program is using something snisplit needs. Close other copies of snisplit, then try again.";
const REFUSED: &str = "Windows refused the change snisplit needs. If you did not start it with Run as administrator, close this window, right-click snisplit-gui and choose Run as administrator.";
const UNEXPECTED: &str = "snisplit stopped unexpectedly.";
const LEFTOVERS_REFUSED: &str = "Windows refused to remove them. If you did not start it with Run as administrator, close this window, right-click snisplit-gui and choose Run as administrator.";
const LEFTOVERS_REMOVED: &str = "The leftover settings were removed.";
const LEFTOVERS_FROM_LAST_TIME: &str = "snisplit did not get to turn off properly last time. Some of its settings are still in place, so the sites below may not open.";
const ALREADY_OPEN: &str = "snisplit is already running, or is still turning off. Wait a few seconds, then press Try again.";
const RESTARTS: &str = "snisplit restarts for about 2 seconds.";
const MISTYPED: &str = "Type only the address, like example.net. A pasted link such as https://example.net/page is accepted and shortened.";
const NON_ASCII: &str = "Addresses with letters other than a to z, such as accented letters or other scripts, are not supported yet. Type an address like example.net.";
const BROWSER_ANYWAY: &str =
    "If it fails in your browser anyway, the cause is something snisplit does not handle.";
const NOTHING_ANSWERED: &str = "Nothing snisplit tried got an answer. The site may be down, or blocked in a way snisplit cannot get around.";
const PART_ANSWERED: &str = "Part of what snisplit tried got an answer, but snisplit has no setting that opens the whole site.";
const CHECK_CONNECTION: &str = "Check your internet connection and try again.";
const SAVE_HINT: &str =
    "Saves snisplit-log.txt to send to whoever helps you, and shows where it went.";
const APPLY_TOGETHER: &str = "several quick changes apply together";
const NOTHING_CHANGED: &str = "nothing was changed on this PC";
const START_REMOVES: &str = "pressing Start also removes them";
const COMES_WITH: &str = "comes with snisplit";
const ADDED_BY_YOU: &str = "added by you";
const SHOW_ATTEMPTS: &str = "What snisplit tried";
const HIDE_ATTEMPTS: &str = "Hide what snisplit tried";
const NAMES_BUDGET: usize = 32;
const MOST_NAMED: usize = 3;
const TEST_PROMISE: Duration = Duration::from_secs(10);
const MINUTE: u64 = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ring {
    Hollow,
    Turning,
    Filled,
    Warning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Plain,
    On,
    Trouble,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Primary,
    Plain,
    Light,
    Text,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    Opens,
    Same,
    Cannot,
    Trouble,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Button {
    pub label: String,
    pub press: Event,
    pub style: Style,
    pub enabled: bool,
}

impl Button {
    fn new(label: &str, press: Event, style: Style) -> Self {
        Self {
            label: label.to_string(),
            press,
            style,
            enabled: true,
        }
    }

    fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hero {
    pub ring: Ring,
    pub tone: Tone,
    pub action: &'static str,
    pub press: Option<Event>,
    pub state_line: &'static str,
    pub sentences: Vec<String>,
    pub compact: String,
    pub buttons: Vec<Button>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Switch {
    pub on: bool,
    pub label: &'static str,
    pub busy: bool,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiteRow {
    pub id: SiteId,
    pub title: String,
    pub sub: String,
    pub dimmed: bool,
    pub open: bool,
    pub switch: Option<Switch>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Confirm {
    pub question: String,
    pub note: Option<&'static str>,
    pub buttons: Vec<Button>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Details {
    pub domains: String,
    pub sentence: Option<String>,
    pub testing: Option<String>,
    pub confirm: Option<Confirm>,
    pub buttons: Vec<Button>,
    pub attempts: Vec<Row>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddPanel {
    pub field: String,
    pub field_enabled: bool,
    pub hint: Option<&'static str>,
    pub testing: Option<String>,
    pub mark: Option<Mark>,
    pub headline: Option<String>,
    pub sub: Option<&'static str>,
    pub buttons: Vec<Button>,
    pub attempts: Vec<Row>,
}

pub fn version_line() -> String {
    format!("version {}", snisplit::VERSION)
}

pub fn hero(state: &State) -> Hero {
    let start = state.anything_on().then_some(Event::StartPressed);
    let mut hero = Hero {
        ring: Ring::Hollow,
        tone: Tone::Plain,
        action: "Start",
        press: start.clone(),
        state_line: "Off",
        sentences: Vec::new(),
        compact: String::new(),
        buttons: Vec::new(),
    };
    match state.relay() {
        Relay::Off => {
            if !state.anything_on() {
                hero.sentences.push(NOTHING_ON.to_string());
            } else if state.note() == Some(Note::LeftoversRemoved) {
                hero.sentences.push(LEFTOVERS_REMOVED.to_string());
                hero.sentences.push(OFF_HINT.to_string());
            } else {
                hero.sentences.push(OFF_HINT.to_string());
            }
        }
        Relay::Starting => {
            hero.ring = Ring::Turning;
            hero.action = "Cancel";
            hero.press = Some(Event::StopPressed);
            if state.applying() {
                applying(&mut hero);
            } else {
                hero.state_line = "Starting...";
                hero.sentences.push(STARTING.to_string());
            }
        }
        Relay::On => {
            let opens = opens_sentence(state);
            hero.ring = Ring::Filled;
            hero.tone = Tone::On;
            hero.action = "Stop";
            hero.press = Some(Event::StopPressed);
            hero.state_line = "On";
            hero.compact = format!("{opens} {CLOSING_TURNS_OFF}");
            hero.sentences = vec![opens, KEEP_OPEN.to_string(), REOPEN_BROWSER.to_string()];
        }
        Relay::Stopping => {
            hero.ring = Ring::Turning;
            hero.action = "Wait";
            hero.press = None;
            hero.state_line = "Turning off...";
            if state.closing() {
                hero.sentences.push(CLOSING_WAIT.to_string());
            } else if state.applying() {
                applying(&mut hero);
            }
        }
        Relay::Failed {
            failure,
            while_starting,
        } => {
            trouble(&mut hero, state);
            hero.action = "Try again";
            let (state_line, sentence) = match failure {
                Failure::Binding => ("Could not start", IN_USE),
                Failure::ChangingRules => ("Could not start", REFUSED),
                Failure::RemovingRules | Failure::NothingSwitchedOn | Failure::Other => {
                    let line = if while_starting {
                        "Could not start"
                    } else {
                        "Off"
                    };
                    (line, UNEXPECTED)
                }
            };
            hero.state_line = state_line;
            hero.sentences.push(sentence.to_string());
        }
        Relay::Leftovers { removal_refused } => {
            trouble(&mut hero, state);
            hero.state_line = "Off, with leftovers";
            if state.leftovers_from_last_time() {
                hero.sentences.push(LEFTOVERS_FROM_LAST_TIME.to_string());
            } else {
                let (phrase, _) = sites_phrase(&titles(state, state.session_sites()));
                hero.sentences.push(format!(
                    "snisplit could not tidy up after itself. {phrase} may not open until the leftover settings are removed."
                ));
            }
            if removal_refused {
                hero.sentences.push(LEFTOVERS_REFUSED.to_string());
            }
            hero.buttons.insert(
                0,
                Button::new(
                    "Remove leftover settings",
                    Event::RemoveLeftoversPressed,
                    Style::Light,
                ),
            );
        }
        Relay::RemovingLeftovers => {
            hero.ring = Ring::Turning;
            hero.tone = Tone::Trouble;
            hero.action = "Wait";
            hero.press = None;
            hero.state_line = "Removing leftover settings...";
        }
        Relay::AlreadyOpen => {
            hero.ring = Ring::Warning;
            hero.tone = Tone::Trouble;
            hero.action = "Try again";
            hero.press = Some(Event::ClaimAgainPressed);
            hero.state_line = "Already open";
            hero.sentences.push(ALREADY_OPEN.to_string());
        }
    }
    if hero.compact.is_empty() {
        hero.compact = hero.sentences.first().cloned().unwrap_or_default();
    }
    hero
}

fn applying(hero: &mut Hero) {
    hero.tone = Tone::On;
    hero.state_line = "Applying your change...";
    hero.sentences = vec![APPLYING.to_string()];
}

fn trouble(hero: &mut Hero, state: &State) {
    hero.ring = Ring::Warning;
    hero.tone = Tone::Trouble;
    hero.press = state.anything_on().then_some(Event::StartPressed);
    hero.buttons.push(Button::new(
        "See what happened",
        Event::ActivityPressed,
        Style::Text,
    ));
}

fn titles(state: &State, names: &[String]) -> Vec<String> {
    names.iter().map(|name| state.title_of(name)).collect()
}

fn opens_sentence(state: &State) -> String {
    let names = state.live_sites().unwrap_or(state.session_sites());
    let (phrase, plural) = sites_phrase(&titles(state, names));
    let verb = if plural { "open" } else { "opens" };
    format!("{phrase} {verb} through snisplit.")
}

pub fn sites_phrase(titles: &[String]) -> (String, bool) {
    let named = match titles {
        [] => return ("No site".to_string(), false),
        [only] => return (only.clone(), false),
        [first, second] => format!("{first} and {second}"),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    };
    if titles.len() > MOST_NAMED || named.len() > NAMES_BUDGET {
        return (format!("{} sites", titles.len()), true);
    }
    (named, true)
}

pub fn footer_note(state: &State, now: Instant) -> String {
    if state.applying() {
        return APPLY_TOGETHER.to_string();
    }
    match state.relay() {
        Relay::On => state.on_since().map_or_else(version_line, |since| {
            let minutes = now.saturating_duration_since(since).as_secs() / MINUTE;
            if minutes == 0 {
                "on for under 1 min".to_string()
            } else {
                format!("on for {minutes} min")
            }
        }),
        Relay::Failed {
            failure: Failure::Binding | Failure::ChangingRules,
            ..
        } => NOTHING_CHANGED.to_string(),
        Relay::Leftovers { .. } => START_REMOVES.to_string(),
        _ => version_line(),
    }
}

pub fn lines_count(state: &State) -> String {
    match state.log_len() {
        1 => "1 line".to_string(),
        count => format!("{count} lines"),
    }
}

pub fn save_note(state: &State) -> String {
    match state.log_saved() {
        None => SAVE_HINT.to_string(),
        Some(Ok(path)) => format!("Saved as {}", path.display()),
        Some(Err(message)) => format!("Could not save the file: {message}"),
    }
}

pub fn rows(state: &State) -> Vec<SiteRow> {
    state
        .sites()
        .iter()
        .map(|site| {
            let pending = state.switch_pending(site);
            let switch = site.switchable().then(|| Switch {
                on: site.on,
                label: match (pending, site.on) {
                    (true, true) => "Turning on",
                    (true, false) => "Turning off",
                    (false, true) => "On",
                    (false, false) => "Off",
                },
                busy: pending,
                enabled: !pending && !state.closing() && state.relay() != Relay::AlreadyOpen,
            });
            SiteRow {
                id: site.id(),
                title: site.title.clone(),
                sub: match &site.kind {
                    SiteKind::BuiltIn => COMES_WITH.to_string(),
                    SiteKind::User => ADDED_BY_YOU.to_string(),
                    SiteKind::Broken(problem) => mistake(problem),
                },
                dimmed: !site.relayed(),
                open: *state.open() == Open::Site(site.id()),
                switch,
            }
        })
        .collect()
}

fn mistake(problem: &str) -> String {
    format!("This file has a mistake and is skipped: {problem}")
}

pub fn details(state: &State, now: Instant) -> Option<Details> {
    let Open::Site(id) = state.open() else {
        return None;
    };
    let site = state.site(id)?;
    let mut details = Details {
        domains: site.domains.join(", "),
        sentence: None,
        testing: None,
        confirm: None,
        buttons: Vec::new(),
        attempts: Vec::new(),
    };
    if let SiteKind::Broken(problem) = &site.kind {
        details.sentence = Some(mistake(problem));
    }
    let tested = state.tested(id);
    if let Some(tested) = tested {
        details.sentence = Some(test_sentence(
            &tested.domain,
            tested.view.as_ref().map(|view| view.verdict),
            now.saturating_duration_since(tested.at),
        ));
    }
    if state.confirming_remove() {
        details.confirm = Some(confirm(state, site));
        return Some(details);
    }
    let testing_here = state
        .probe()
        .filter(|probing| probing.target == Target::Site(id.clone()));
    if let Some(probing) = testing_here {
        details.testing = Some(testing_line(
            &probing.domain,
            now.saturating_duration_since(probing.started),
        ));
        details.buttons.push(Button::new(
            "Cancel",
            Event::CancelTestPressed,
            Style::Plain,
        ));
        return Some(details);
    }
    if site.test_domain().is_some() {
        let label = if tested.is_some() {
            "Test again"
        } else {
            "Test"
        };
        let press = Event::TestPressed {
            site: id.clone(),
            at: now,
        };
        details
            .buttons
            .push(Button::new(label, press, Style::Plain).enabled(state.probe().is_none()));
    }
    if site.removable() {
        details
            .buttons
            .push(Button::new("Remove", Event::RemovePressed, Style::Plain));
    }
    if let Some(view) = tested.and_then(|tested| tested.view.as_ref()) {
        details.buttons.push(attempts_button(state));
        if state.showing_attempts() {
            details.attempts = view.rows.clone();
        }
    }
    Some(details)
}

fn confirm(state: &State, site: &Site) -> Confirm {
    let running = matches!(state.relay(), Relay::Starting | Relay::On);
    let restarts = running && site.relayed() && !state.closing();
    Confirm {
        question: format!("Remove {}?", site.title),
        note: restarts.then_some(RESTARTS),
        buttons: vec![
            Button::new("Keep", Event::RemoveKept, Style::Plain),
            Button::new("Remove", Event::RemoveConfirmed, Style::Light),
        ],
    }
}

fn attempts_button(state: &State) -> Button {
    let label = if state.showing_attempts() {
        HIDE_ATTEMPTS
    } else {
        SHOW_ATTEMPTS
    };
    Button::new(label, Event::AttemptsPressed, Style::Text)
}

pub fn test_sentence(domain: &str, verdict: Option<Verdict>, age: Duration) -> String {
    let when = age_words(age);
    match verdict {
        Some(Verdict::ProfileSuggested) => {
            format!("Tested {domain} {when}: blocked without snisplit, and snisplit can open it.")
        }
        Some(Verdict::NotBlocked) => format!("Tested {domain} {when}: opens without snisplit."),
        Some(Verdict::NoProfileHelps | Verdict::NothingWorked) => {
            format!("Tested {domain} {when}: snisplit cannot open it from here.")
        }
        None => format!("Could not test {domain} {when}. {CHECK_CONNECTION}"),
    }
}

fn age_words(age: Duration) -> String {
    match age.as_secs() / MINUTE {
        0 => "just now".to_string(),
        minutes => format!("{minutes} min ago"),
    }
}

pub fn testing_line(domain: &str, elapsed: Duration) -> String {
    if elapsed < TEST_PROMISE {
        format!("Testing {domain}... up to 10 seconds")
    } else {
        format!("Testing {domain}... this one is taking longer than usual")
    }
}

fn mistyped_hint(field: &str) -> &'static str {
    if address::non_ascii_host(field) {
        NON_ASCII
    } else {
        MISTYPED
    }
}

pub fn add_panel(state: &State, now: Instant) -> Option<AddPanel> {
    if *state.open() != Open::Add {
        return None;
    }
    let mut panel = AddPanel {
        field: state.field().to_string(),
        field_enabled: false,
        hint: None,
        testing: None,
        mark: None,
        headline: None,
        sub: None,
        buttons: Vec::new(),
        attempts: Vec::new(),
    };
    let test_another = || Button::new("Test another", Event::TestAnotherPressed, Style::Plain);
    let close = |label: &str, style: Style| Button::new(label, Event::PanelClosed, style);
    match state.adding() {
        Adding::Typing { mistyped } => {
            panel.field_enabled = true;
            panel.hint = mistyped.then(|| mistyped_hint(state.field()));
            let can_test = state.probe().is_none() && !state.field().trim().is_empty();
            panel.buttons = vec![
                Button::new("Test", Event::AddTestPressed { at: now }, Style::Plain)
                    .enabled(can_test),
                close("Close", Style::Text),
            ];
        }
        Adding::Testing => {
            if let Some(probing) = state.probe() {
                panel.testing = Some(testing_line(
                    &probing.domain,
                    now.saturating_duration_since(probing.started),
                ));
            }
            panel.buttons = vec![Button::new(
                "Cancel",
                Event::CancelTestPressed,
                Style::Plain,
            )];
        }
        Adding::Tested(view) => {
            let domain = &view.domain;
            let (mark, headline, sub, mut buttons) = match view.verdict {
                Verdict::ProfileSuggested => (
                    Mark::Opens,
                    format!("{domain} is blocked here, and snisplit can open it."),
                    None,
                    vec![
                        Button::new("Add and turn on", Event::AddPressed, Style::Primary),
                        close("Not now", Style::Plain),
                    ],
                ),
                Verdict::NotBlocked => (
                    Mark::Same,
                    format!("{domain} already opens normally. Nothing to add."),
                    Some(BROWSER_ANYWAY),
                    vec![close("Done", Style::Primary), test_another()],
                ),
                Verdict::NothingWorked => (
                    Mark::Cannot,
                    format!("snisplit cannot open {domain} from here."),
                    Some(NOTHING_ANSWERED),
                    vec![close("Close", Style::Primary), test_another()],
                ),
                Verdict::NoProfileHelps => (
                    Mark::Cannot,
                    format!("snisplit cannot open {domain} from here."),
                    Some(PART_ANSWERED),
                    vec![close("Close", Style::Primary), test_another()],
                ),
            };
            buttons.push(attempts_button(state));
            panel.mark = Some(mark);
            panel.headline = Some(headline);
            panel.sub = sub;
            panel.buttons = buttons;
            if state.showing_attempts() {
                panel.attempts = view.rows.clone();
            }
        }
        Adding::CouldNotTest(domain) => {
            panel.mark = Some(Mark::Trouble);
            panel.headline = Some(format!("Could not test {domain}."));
            panel.sub = Some(CHECK_CONNECTION);
            panel.buttons = vec![
                Button::new(
                    "Try again",
                    Event::TryAgainPressed { at: now },
                    Style::Primary,
                )
                .enabled(state.probe().is_none()),
                close("Close", Style::Plain),
            ];
        }
        Adding::Covered { domain, by } => {
            panel.mark = Some(Mark::Same);
            panel.headline = Some(format!("{domain} is already covered by {by}."));
            panel.buttons = vec![close("Close", Style::Primary), test_another()];
        }
        Adding::NotSaved(domain) => {
            panel.mark = Some(Mark::Trouble);
            panel.headline = Some(format!("Could not add {domain}."));
            panel.buttons = vec![
                close("Close", Style::Primary),
                Button::new("See what happened", Event::ActivityPressed, Style::Text),
            ];
        }
    }
    Some(panel)
}

#[cfg(test)]
mod tests;
