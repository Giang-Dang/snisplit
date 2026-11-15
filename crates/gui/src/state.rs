use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::Instant;

use snisplit::ProfileSpec;
use snisplit::session::Failure;
use snisplit::sites::SiteFile;
use snisplit_core::probe::{Verdict, profile_name};

use crate::address;

pub const LOG_LINES: usize = 500;

const RULES_MAY_REMAIN: &str = "DNS rules may remain: press Start and then Stop, or run snisplit cleanup from an administrator terminal, to remove them";
const TEST_DOMAINS: [(&str, &str); 1] = [("steam", "store.steampowered.com")];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Relay {
    Off,
    Starting,
    On,
    Stopping,
    Failed {
        failure: Failure,
        while_starting: bool,
    },
    Leftovers {
        removal_refused: bool,
    },
    RemovingLeftovers,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SiteKind {
    BuiltIn,
    User,
    Broken(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Site {
    pub name: String,
    pub title: String,
    pub domains: Vec<String>,
    pub kind: SiteKind,
    pub on: bool,
}

impl Site {
    pub fn removable(&self) -> bool {
        self.kind != SiteKind::BuiltIn
    }

    pub fn switchable(&self) -> bool {
        !matches!(self.kind, SiteKind::Broken(_))
    }

    pub fn relayed(&self) -> bool {
        self.on && self.switchable()
    }

    pub fn test_domain(&self) -> Option<&str> {
        if !self.switchable() {
            return None;
        }
        TEST_DOMAINS
            .iter()
            .find(|(name, _)| self.kind == SiteKind::BuiltIn && *name == self.name)
            .map(|(_, domain)| *domain)
            .or_else(|| self.domains.first().map(String::as_str))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub attempt: String,
    pub outcomes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeView {
    pub domain: String,
    pub address: String,
    pub verdict: Verdict,
    pub profile: Option<String>,
    pub rows: Vec<Row>,
    pub details: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Site(String),
    Adding,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Probing {
    pub id: u64,
    pub domain: String,
    pub target: Target,
    pub started: Instant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tested {
    pub domain: String,
    pub at: Instant,
    pub view: Option<ProbeView>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Adding {
    Typing { mistyped: bool },
    Testing,
    Tested(ProbeView),
    CouldNotTest(String),
    Covered { domain: String, by: String },
    NotSaved(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Open {
    Nothing,
    Site(String),
    Add,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Sites,
    Activity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Note {
    LeftoversRemoved,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionEnd {
    Clean,
    Failed { failure: Failure, detail: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveRefusal {
    Covered { domain: String, owner: String },
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    StartPressed,
    StopPressed,
    SessionReady {
        at: Instant,
    },
    SessionLine(String),
    SessionEnded(SessionEnd),
    RemoveLeftoversPressed,
    LeftoversRemoved(Result<(), String>),
    SitesLoaded(Vec<Site>),
    RowPressed(String),
    SwitchPressed(String),
    SwitchSaved(Result<(String, bool), String>),
    TestPressed {
        name: String,
        at: Instant,
    },
    CancelTestPressed,
    ProbeFinished {
        id: u64,
        result: Result<ProbeView, String>,
        at: Instant,
    },
    AttemptsPressed,
    RemovePressed,
    RemoveConfirmed,
    RemoveKept,
    SiteRemoved(Result<String, String>),
    AddRowPressed,
    FieldEdited(String),
    AddTestPressed {
        at: Instant,
    },
    TryAgainPressed {
        at: Instant,
    },
    AddPressed,
    TestAnotherPressed,
    PanelClosed,
    SiteSaved(Result<String, SaveRefusal>),
    ActivityPressed,
    BackPressed,
    SaveLogPressed,
    LogSaved(Result<PathBuf, String>),
    CloseRequested,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    StartSession,
    StopSession,
    RemoveLeftovers,
    Probe { id: u64, domain: String },
    CancelProbe(u64),
    SetSwitch { name: String, on: bool },
    SaveSite(String),
    DeleteSite(String),
    LoadSites,
    SaveLog(Vec<String>),
    Exit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct State {
    relay: Relay,
    sites: Vec<Site>,
    log: VecDeque<String>,
    restart_pending: bool,
    applying: bool,
    closing: bool,
    session_sites: Vec<String>,
    live_sites: Option<Vec<String>>,
    on_since: Option<Instant>,
    note: Option<Note>,
    open: Open,
    confirming_remove: bool,
    showing_attempts: bool,
    probe: Option<Probing>,
    probes_started: u64,
    tests: Vec<(String, Tested)>,
    adding: Adding,
    field: String,
    page: Page,
    log_saved: Option<Result<PathBuf, String>>,
}

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}

impl State {
    pub fn new() -> Self {
        Self {
            relay: Relay::Off,
            sites: Vec::new(),
            log: VecDeque::new(),
            restart_pending: false,
            applying: false,
            closing: false,
            session_sites: Vec::new(),
            live_sites: None,
            on_since: None,
            note: None,
            open: Open::Nothing,
            confirming_remove: false,
            showing_attempts: false,
            probe: None,
            probes_started: 0,
            tests: Vec::new(),
            adding: Adding::Typing { mistyped: false },
            field: String::new(),
            page: Page::Sites,
            log_saved: None,
        }
    }

    pub fn relay(&self) -> Relay {
        self.relay
    }

    pub fn sites(&self) -> &[Site] {
        &self.sites
    }

    pub fn site(&self, name: &str) -> Option<&Site> {
        self.sites.iter().find(|site| site.name == name)
    }

    pub fn log(&self) -> impl Iterator<Item = &str> {
        self.log.iter().map(String::as_str)
    }

    pub fn log_len(&self) -> usize {
        self.log.len()
    }

    pub fn restart_pending(&self) -> bool {
        self.restart_pending
    }

    pub fn applying(&self) -> bool {
        self.applying
    }

    pub fn closing(&self) -> bool {
        self.closing
    }

    pub fn anything_on(&self) -> bool {
        self.sites.iter().any(Site::relayed)
    }

    pub fn session_sites(&self) -> &[String] {
        &self.session_sites
    }

    pub fn live_sites(&self) -> Option<&[String]> {
        self.live_sites.as_deref()
    }

    pub fn on_since(&self) -> Option<Instant> {
        self.on_since
    }

    pub fn note(&self) -> Option<Note> {
        self.note
    }

    pub fn open(&self) -> &Open {
        &self.open
    }

    pub fn confirming_remove(&self) -> bool {
        self.confirming_remove
    }

    pub fn showing_attempts(&self) -> bool {
        self.showing_attempts
    }

    pub fn probe(&self) -> Option<&Probing> {
        self.probe.as_ref()
    }

    pub fn tested(&self, name: &str) -> Option<&Tested> {
        self.tests
            .iter()
            .find(|(tested, _)| tested == name)
            .map(|(_, tested)| tested)
    }

    pub fn adding(&self) -> &Adding {
        &self.adding
    }

    pub fn field(&self) -> &str {
        &self.field
    }

    pub fn page(&self) -> Page {
        self.page
    }

    pub fn log_saved(&self) -> Option<&Result<PathBuf, String>> {
        self.log_saved.as_ref()
    }

    pub fn switch_pending(&self, site: &Site) -> bool {
        let Some(live) = self.live_sites.as_ref().filter(|_| self.applying) else {
            return false;
        };
        site.switchable() && live.contains(&site.name) != site.relayed()
    }

    pub fn title_of(&self, name: &str) -> String {
        self.site(name)
            .map_or_else(|| name.to_string(), |site| site.title.clone())
    }

    pub fn update(&mut self, event: Event) -> Vec<Effect> {
        match event {
            Event::StartPressed => self.start(),
            Event::StopPressed => {
                self.restart_pending = false;
                self.applying = false;
                self.stop()
            }
            Event::SessionReady { at } => {
                self.session_ready(at);
                Vec::new()
            }
            Event::SessionLine(line) => {
                self.push_log(line);
                Vec::new()
            }
            Event::SessionEnded(end) => self.session_ended(end),
            Event::RemoveLeftoversPressed => self.remove_leftovers(),
            Event::LeftoversRemoved(removed) => self.leftovers_removed(removed),
            Event::SitesLoaded(sites) => {
                self.sites_loaded(sites);
                Vec::new()
            }
            Event::RowPressed(name) => self.row_pressed(name),
            Event::SwitchPressed(name) => self.switch_pressed(&name),
            Event::SwitchSaved(saved) => self.switch_saved(saved),
            Event::TestPressed { name, at } => self.test_site(&name, at),
            Event::CancelTestPressed => self.cancel_probe(),
            Event::ProbeFinished { id, result, at } => {
                self.probe_finished(id, result, at);
                Vec::new()
            }
            Event::AttemptsPressed => {
                self.showing_attempts = !self.showing_attempts;
                Vec::new()
            }
            Event::RemovePressed => {
                self.remove_pressed();
                Vec::new()
            }
            Event::RemoveKept => {
                self.confirming_remove = false;
                Vec::new()
            }
            Event::RemoveConfirmed => self.remove_confirmed(),
            Event::SiteRemoved(removed) => self.site_removed(removed),
            Event::AddRowPressed => self.open_add(),
            Event::FieldEdited(text) => {
                if matches!(self.adding, Adding::Typing { .. }) {
                    self.field = text;
                    self.adding = Adding::Typing { mistyped: false };
                }
                Vec::new()
            }
            Event::AddTestPressed { at } => self.test_typed(at),
            Event::TryAgainPressed { at } => self.try_again(at),
            Event::AddPressed => self.add_pressed(),
            Event::TestAnotherPressed => {
                if self.open == Open::Add && self.probe_target() != Some(&Target::Adding) {
                    self.reset_panel();
                }
                Vec::new()
            }
            Event::PanelClosed => self.close_panel(),
            Event::SiteSaved(saved) => self.site_saved(saved),
            Event::ActivityPressed => {
                self.page = Page::Activity;
                self.confirming_remove = false;
                Vec::new()
            }
            Event::BackPressed => {
                self.page = Page::Sites;
                self.confirming_remove = false;
                Vec::new()
            }
            Event::SaveLogPressed => vec![Effect::SaveLog(self.log.iter().cloned().collect())],
            Event::LogSaved(saved) => {
                if let Err(message) = &saved {
                    self.push_log(format!("could not save the activity file: {message}"));
                }
                self.log_saved = Some(saved);
                Vec::new()
            }
            Event::CloseRequested => self.close(),
        }
    }

    fn start(&mut self) -> Vec<Effect> {
        let startable = matches!(
            self.relay,
            Relay::Off | Relay::Failed { .. } | Relay::Leftovers { .. }
        );
        if self.closing || !startable {
            return Vec::new();
        }
        if !self.anything_on() {
            self.applying = false;
            return Vec::new();
        }
        self.relay = Relay::Starting;
        self.note = None;
        self.session_sites = self
            .sites
            .iter()
            .filter(|site| site.relayed())
            .map(|site| site.name.clone())
            .collect();
        self.push_log("starting snisplit".to_string());
        vec![Effect::StartSession]
    }

    fn stop(&mut self) -> Vec<Effect> {
        if !matches!(self.relay, Relay::Starting | Relay::On) {
            return Vec::new();
        }
        self.relay = Relay::Stopping;
        vec![Effect::StopSession]
    }

    fn session_ready(&mut self, at: Instant) {
        if self.relay != Relay::Starting {
            return;
        }
        self.relay = Relay::On;
        self.applying = false;
        self.live_sites = Some(self.session_sites.clone());
        self.on_since.get_or_insert(at);
    }

    fn session_ended(&mut self, end: SessionEnd) -> Vec<Effect> {
        let was_starting = self.relay == Relay::Starting;
        self.relay = match end {
            SessionEnd::Clean => Relay::Off,
            SessionEnd::Failed { failure, detail } => {
                self.restart_pending = false;
                self.push_log(format!("snisplit stopped with an error: {detail}"));
                match failure {
                    Failure::RemovingRules => Relay::Leftovers {
                        removal_refused: false,
                    },
                    Failure::NothingSwitchedOn => Relay::Off,
                    failure => Relay::Failed {
                        failure,
                        while_starting: was_starting,
                    },
                }
            }
        };
        if self.closing {
            return self.close_ended();
        }
        if std::mem::take(&mut self.restart_pending) {
            let effects = self.start();
            if !effects.is_empty() {
                return effects;
            }
        }
        self.applying = false;
        self.live_sites = None;
        self.on_since = None;
        Vec::new()
    }

    fn close_ended(&mut self) -> Vec<Effect> {
        if !matches!(self.relay, Relay::Leftovers { .. }) {
            return vec![Effect::Exit];
        }
        self.closing = false;
        self.applying = false;
        self.push_log(format!("closing stopped: {RULES_MAY_REMAIN}"));
        Vec::new()
    }

    fn remove_leftovers(&mut self) -> Vec<Effect> {
        if self.closing || !matches!(self.relay, Relay::Leftovers { .. }) {
            return Vec::new();
        }
        self.relay = Relay::RemovingLeftovers;
        self.push_log("removing the leftover DNS rules".to_string());
        vec![Effect::RemoveLeftovers]
    }

    fn leftovers_removed(&mut self, removed: Result<(), String>) -> Vec<Effect> {
        if self.relay != Relay::RemovingLeftovers {
            return Vec::new();
        }
        match removed {
            Ok(()) => {
                self.relay = Relay::Off;
                self.note = Some(Note::LeftoversRemoved);
                self.push_log("the leftover DNS rules were removed".to_string());
                if self.closing {
                    return vec![Effect::Exit];
                }
            }
            Err(message) => {
                self.relay = Relay::Leftovers {
                    removal_refused: true,
                };
                self.closing = false;
                self.push_log(format!("removing the leftover DNS rules failed: {message}"));
            }
        }
        Vec::new()
    }

    fn sites_loaded(&mut self, sites: Vec<Site>) {
        self.sites = sites;
        self.confirming_remove = false;
        let known = |name: &str| self.sites.iter().any(|site| site.name == name);
        if let Open::Site(name) = &self.open
            && !known(name)
        {
            self.open = Open::Nothing;
        }
        let names: Vec<String> = self.sites.iter().map(|site| site.name.clone()).collect();
        self.tests.retain(|(name, _)| names.contains(name));
    }

    fn row_pressed(&mut self, name: String) -> Vec<Effect> {
        if self.open == Open::Site(name.clone()) {
            self.open = Open::Nothing;
            self.confirming_remove = false;
            return Vec::new();
        }
        if self.site(&name).is_none() {
            return Vec::new();
        }
        let effects = self.cancel_adding_probe();
        self.open = Open::Site(name);
        self.confirming_remove = false;
        self.showing_attempts = false;
        effects
    }

    fn open_site(&self) -> Option<&Site> {
        match &self.open {
            Open::Site(name) => self.site(name),
            Open::Nothing | Open::Add => None,
        }
    }

    fn switch_pressed(&mut self, name: &str) -> Vec<Effect> {
        let Some(site) = self.site(name) else {
            return Vec::new();
        };
        if self.closing || !site.switchable() || self.switch_pending(site) {
            return Vec::new();
        }
        vec![Effect::SetSwitch {
            name: name.to_string(),
            on: !site.on,
        }]
    }

    fn switch_saved(&mut self, saved: Result<(String, bool), String>) -> Vec<Effect> {
        let (name, on) = match saved {
            Ok(saved) => saved,
            Err(message) => {
                self.push_log(format!("the switch was not saved: {message}"));
                return Vec::new();
            }
        };
        let Some(site) = self.sites.iter_mut().find(|site| site.name == name) else {
            return Vec::new();
        };
        if site.on == on {
            return Vec::new();
        }
        site.on = on;
        let verb = if on { "switched on" } else { "switched off" };
        let anything_on = self.anything_on();
        self.apply_change(format!("site {name} {verb}"), anything_on)
    }

    fn apply_change(&mut self, mut line: String, anything_on: bool) -> Vec<Effect> {
        let mut effects = Vec::new();
        match self.relay {
            Relay::Starting | Relay::On if !self.closing => {
                if anything_on {
                    line.push_str("; restarting snisplit so the change applies");
                    self.restart_pending = true;
                    self.applying = true;
                } else {
                    self.restart_pending = false;
                    self.applying = false;
                    line.push_str("; every site is switched off, so snisplit turns off");
                }
                effects.extend(self.stop());
            }
            Relay::Stopping if !self.closing && self.restart_pending => {
                if anything_on {
                    line.push_str("; snisplit starts again once it has stopped");
                } else {
                    self.restart_pending = false;
                    self.applying = false;
                    line.push_str("; every site is switched off, so snisplit stays off");
                }
            }
            _ => {}
        }
        self.push_log(line);
        effects
    }

    fn test_site(&mut self, name: &str, at: Instant) -> Vec<Effect> {
        let Some(domain) = self
            .site(name)
            .and_then(Site::test_domain)
            .map(str::to_string)
        else {
            return Vec::new();
        };
        self.begin_probe(domain, Target::Site(name.to_string()), at)
    }

    fn test_typed(&mut self, at: Instant) -> Vec<Effect> {
        if self.open != Open::Add || !matches!(self.adding, Adding::Typing { .. }) {
            return Vec::new();
        }
        let Some(domain) = address::site_address(&self.field) else {
            self.adding = Adding::Typing { mistyped: true };
            return Vec::new();
        };
        if self.probe.is_some() {
            return Vec::new();
        }
        self.field = domain.clone();
        self.adding = Adding::Testing;
        self.begin_probe(domain, Target::Adding, at)
    }

    fn try_again(&mut self, at: Instant) -> Vec<Effect> {
        let Adding::CouldNotTest(domain) = &self.adding else {
            return Vec::new();
        };
        if self.probe.is_some() {
            return Vec::new();
        }
        let domain = domain.clone();
        self.adding = Adding::Testing;
        self.begin_probe(domain, Target::Adding, at)
    }

    fn begin_probe(&mut self, domain: String, target: Target, at: Instant) -> Vec<Effect> {
        if self.probe.is_some() {
            return Vec::new();
        }
        self.probes_started += 1;
        let id = self.probes_started;
        self.probe = Some(Probing {
            id,
            domain: domain.clone(),
            target,
            started: at,
        });
        vec![Effect::Probe { id, domain }]
    }

    fn probe_target(&self) -> Option<&Target> {
        self.probe.as_ref().map(|probing| &probing.target)
    }

    fn cancel_probe(&mut self) -> Vec<Effect> {
        let Some(probing) = self.probe.take() else {
            return Vec::new();
        };
        self.push_log(format!("test of {} cancelled", probing.domain));
        if probing.target == Target::Adding {
            self.reset_panel();
        }
        vec![Effect::CancelProbe(probing.id)]
    }

    fn cancel_adding_probe(&mut self) -> Vec<Effect> {
        if self.probe_target() == Some(&Target::Adding) {
            self.cancel_probe()
        } else {
            Vec::new()
        }
    }

    fn probe_finished(&mut self, id: u64, result: Result<ProbeView, String>, at: Instant) {
        if self.probe.as_ref().is_none_or(|probing| probing.id != id) {
            return;
        }
        let Some(probing) = self.probe.take() else {
            return;
        };
        match &result {
            Ok(view) => {
                self.push_log(format!("test of {} at {}:", view.domain, view.address));
                for line in &view.details {
                    self.push_log(line.clone());
                }
            }
            Err(message) => {
                self.push_log(format!("could not test {}: {message}", probing.domain));
            }
        }
        match probing.target {
            Target::Site(name) => {
                self.tests.retain(|(tested, _)| *tested != name);
                self.tests.push((
                    name,
                    Tested {
                        domain: probing.domain,
                        at,
                        view: result.ok(),
                    },
                ));
            }
            Target::Adding => {
                self.adding = match result {
                    Ok(view) => Adding::Tested(view),
                    Err(_) => Adding::CouldNotTest(probing.domain),
                };
            }
        }
    }

    fn removable_open_site(&self) -> Option<String> {
        self.open_site()
            .filter(|site| site.removable())
            .map(|site| site.name.clone())
    }

    fn remove_pressed(&mut self) {
        let asked = self.removable_open_site();
        self.confirming_remove = asked.is_some();
        if let Some(name) = asked {
            self.push_log(format!("remove {name} asked"));
        }
    }

    fn remove_confirmed(&mut self) -> Vec<Effect> {
        if !std::mem::take(&mut self.confirming_remove) {
            return Vec::new();
        }
        let Some(name) = self.removable_open_site() else {
            return Vec::new();
        };
        self.push_log(format!("remove {name} confirmed"));
        vec![Effect::DeleteSite(name)]
    }

    fn site_removed(&mut self, removed: Result<String, String>) -> Vec<Effect> {
        let name = match removed {
            Ok(name) => name,
            Err(message) => {
                self.push_log(format!("the site was not removed: {message}"));
                return Vec::new();
            }
        };
        let was_relayed = self.site(&name).is_some_and(Site::relayed);
        self.sites.retain(|site| site.name != name);
        self.tests.retain(|(tested, _)| *tested != name);
        if self.open == Open::Site(name.clone()) {
            self.open = Open::Nothing;
        }
        let mut effects = vec![Effect::LoadSites];
        let line = format!("site {name} removed");
        if was_relayed {
            let anything_on = self.anything_on();
            effects.extend(self.apply_change(line, anything_on));
        } else {
            self.push_log(line);
        }
        effects
    }

    fn open_add(&mut self) -> Vec<Effect> {
        if self.open == Open::Add {
            return Vec::new();
        }
        self.open = Open::Add;
        self.confirming_remove = false;
        self.reset_panel();
        Vec::new()
    }

    fn reset_panel(&mut self) {
        self.adding = Adding::Typing { mistyped: false };
        self.field.clear();
        self.showing_attempts = false;
    }

    fn close_panel(&mut self) -> Vec<Effect> {
        if self.open != Open::Add {
            return Vec::new();
        }
        let effects = self.cancel_adding_probe();
        self.open = Open::Nothing;
        self.reset_panel();
        effects
    }

    fn add_pressed(&mut self) -> Vec<Effect> {
        match &self.adding {
            Adding::Tested(ProbeView {
                verdict: Verdict::ProfileSuggested,
                profile: Some(profile),
                ..
            }) => vec![Effect::SaveSite(profile.clone())],
            _ => Vec::new(),
        }
    }

    fn site_saved(&mut self, saved: Result<String, SaveRefusal>) -> Vec<Effect> {
        let tested_domain = match &self.adding {
            Adding::Tested(view) => view.domain.clone(),
            _ => String::new(),
        };
        match saved {
            Ok(name) => {
                if self.open == Open::Add {
                    self.open = Open::Nothing;
                }
                self.reset_panel();
                let mut effects = vec![Effect::LoadSites];
                effects.extend(self.apply_change(format!("site {name} added"), true));
                effects
            }
            Err(SaveRefusal::Covered { domain, owner }) => {
                self.push_log(format!(
                    "not added: {domain} is already covered by the site {owner}"
                ));
                let by = self.title_of(&owner);
                self.adding = Adding::Covered { domain, by };
                Vec::new()
            }
            Err(SaveRefusal::Other(message)) => {
                self.push_log(format!("not added: {message}"));
                self.adding = Adding::NotSaved(tested_domain);
                Vec::new()
            }
        }
    }

    fn close(&mut self) -> Vec<Effect> {
        let waiting = matches!(self.relay, Relay::Stopping | Relay::RemovingLeftovers);
        if self.closing && waiting {
            self.push_log(format!(
                "closing before snisplit finished stopping: {RULES_MAY_REMAIN}"
            ));
            return vec![Effect::Exit];
        }
        self.closing = true;
        self.restart_pending = false;
        self.applying = false;
        match self.relay {
            Relay::Starting | Relay::On => {
                self.push_log("closing: stopping snisplit and removing its DNS rules".to_string());
                self.stop()
            }
            Relay::Stopping | Relay::RemovingLeftovers => Vec::new(),
            Relay::Off | Relay::Failed { .. } | Relay::Leftovers { .. } => vec![Effect::Exit],
        }
    }

    fn push_log(&mut self, line: String) {
        if self.log.len() == LOG_LINES {
            self.log.pop_front();
        }
        self.log.push_back(line);
    }
}

pub fn site_list(built_ins: &[ProfileSpec], files: Vec<SiteFile>, off: &[String]) -> Vec<Site> {
    let is_on = |name: &str| !off.iter().any(|off| off == name);
    let built_in = built_ins.iter().map(|spec| Site {
        name: spec.name.clone(),
        title: built_in_title(&spec.name),
        domains: spec.domains.clone(),
        kind: SiteKind::BuiltIn,
        on: is_on(&spec.name),
    });
    let user = files.into_iter().map(|file| match file.loaded {
        Ok(spec) => Site {
            title: user_title(&file.name, &spec.domains),
            on: is_on(&file.name),
            name: file.name,
            domains: spec.domains,
            kind: SiteKind::User,
        },
        Err(problem) => Site {
            title: file.name.clone(),
            on: false,
            name: file.name,
            domains: Vec::new(),
            kind: SiteKind::Broken(problem),
        },
    });
    built_in.chain(user).collect()
}

fn built_in_title(name: &str) -> String {
    let mut characters = name.chars();
    characters.next().map_or_else(String::new, |first| {
        first.to_ascii_uppercase().to_string() + characters.as_str()
    })
}

fn user_title(name: &str, domains: &[String]) -> String {
    match domains.first() {
        Some(domain) if profile_name(domain) == name => domain.clone(),
        _ => name.to_string(),
    }
}

#[cfg(test)]
mod tests;
