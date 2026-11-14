use std::collections::VecDeque;

use snisplit::ProfileSpec;
use snisplit::sites::SiteFile;

pub const LOG_LINES: usize = 500;

const RULES_MAY_REMAIN: &str = "DNS rules may remain: press Start and then Stop, or run snisplit cleanup from an administrator terminal, to remove them";

const ADMIN_NEEDED: &str = "Adding the DNS rules failed. snisplit needs administrator rights for that: close this window and start snisplit-gui again with Run as administrator.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Relay {
    Stopped,
    Starting,
    Running,
    Stopping,
    Failed(String),
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
    pub domains: Vec<String>,
    pub kind: SiteKind,
}

impl Site {
    pub fn removable(&self) -> bool {
        self.kind != SiteKind::BuiltIn
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    Check,
    Add,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Offer {
    Add(String),
    NotBlocked,
    NothingWorked,
}

impl Offer {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Add(_) => "Add",
            Self::NotBlocked => "Not blocked, nothing to add",
            Self::NothingWorked => "No method worked",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub attempt: String,
    pub result: String,
    pub runs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeView {
    pub domain: String,
    pub address: String,
    pub rows: Vec<Row>,
    pub summary: String,
    pub offer: Offer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Probe {
    Idle,
    Running { domain: String, purpose: Purpose },
    Done { purpose: Purpose, view: ProbeView },
    Failed { domain: String, message: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionEnd {
    Clean,
    Failed(String),
    RoutingFailed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    StartPressed,
    StopPressed,
    SessionReady,
    SessionLine(String),
    SessionEnded(SessionEnd),
    CheckPressed(String),
    ProbePressed(String),
    ProbeFinished {
        domain: String,
        result: Result<ProbeView, String>,
    },
    AddPressed,
    RemovePressed(String),
    SitesLoaded(Vec<Site>),
    SiteSaved(Result<String, String>),
    SiteRemoved(Result<String, String>),
    CloseRequested,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    StartSession,
    StopSession,
    Probe(String),
    SaveSite(String),
    DeleteSite(String),
    LoadSites,
    Exit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Change {
    Added,
    Removed,
}

impl Change {
    fn verb(self) -> &'static str {
        match self {
            Self::Added => "added",
            Self::Removed => "removed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct State {
    pub relay: Relay,
    pub sites: Vec<Site>,
    pub probe: Probe,
    pub notice: Option<String>,
    log: VecDeque<String>,
    restart_pending: bool,
    closing: bool,
}

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}

impl State {
    pub fn new() -> Self {
        Self {
            relay: Relay::Stopped,
            sites: Vec::new(),
            probe: Probe::Idle,
            notice: None,
            log: VecDeque::new(),
            restart_pending: false,
            closing: false,
        }
    }

    pub fn log(&self) -> impl Iterator<Item = &str> {
        self.log.iter().map(String::as_str)
    }

    pub fn status(&self) -> &'static str {
        match &self.relay {
            Relay::Stopped => "Stopped",
            Relay::Starting => "Starting...",
            Relay::Running => "Running: the listed sites go through snisplit",
            Relay::Stopping => "Stopping...",
            Relay::Failed(_) => "Failed",
        }
    }

    pub fn failure(&self) -> Option<&str> {
        match &self.relay {
            Relay::Failed(message) => Some(message),
            _ => None,
        }
    }

    pub fn start_stop_label(&self) -> Option<&'static str> {
        match self.relay {
            Relay::Stopped | Relay::Failed(_) => Some("Start"),
            Relay::Starting | Relay::Running => Some("Stop"),
            Relay::Stopping => None,
        }
    }

    pub fn probing(&self) -> bool {
        matches!(self.probe, Probe::Running { .. })
    }

    pub fn update(&mut self, event: Event) -> Vec<Effect> {
        match event {
            Event::StartPressed => self.start(),
            Event::StopPressed => {
                self.restart_pending = false;
                self.stop()
            }
            Event::SessionReady => {
                if self.relay == Relay::Starting {
                    self.relay = Relay::Running;
                }
                Vec::new()
            }
            Event::SessionLine(line) => {
                self.push_log(line);
                Vec::new()
            }
            Event::SessionEnded(end) => self.session_ended(end),
            Event::CheckPressed(name) => self.check(&name),
            Event::ProbePressed(text) => self.probe_typed(&text),
            Event::ProbeFinished { domain, result } => {
                self.probe_finished(domain, result);
                Vec::new()
            }
            Event::AddPressed => self.add(),
            Event::RemovePressed(name) => self.remove(&name),
            Event::SitesLoaded(sites) => {
                self.sites = sites;
                Vec::new()
            }
            Event::SiteSaved(saved) => self.site_changed(saved, Change::Added),
            Event::SiteRemoved(removed) => self.site_changed(removed, Change::Removed),
            Event::CloseRequested => self.close(),
        }
    }

    fn start(&mut self) -> Vec<Effect> {
        if self.closing || !matches!(self.relay, Relay::Stopped | Relay::Failed(_)) {
            return Vec::new();
        }
        self.relay = Relay::Starting;
        self.push_log("starting snisplit".to_string());
        vec![Effect::StartSession]
    }

    fn stop(&mut self) -> Vec<Effect> {
        if !matches!(self.relay, Relay::Starting | Relay::Running) {
            return Vec::new();
        }
        self.relay = Relay::Stopping;
        vec![Effect::StopSession]
    }

    fn session_ended(&mut self, end: SessionEnd) -> Vec<Effect> {
        self.relay = match end {
            SessionEnd::Clean => Relay::Stopped,
            SessionEnd::Failed(message) => {
                self.restart_pending = false;
                self.push_log(format!("snisplit stopped with an error: {message}"));
                Relay::Failed(message)
            }
            SessionEnd::RoutingFailed(cause) => {
                self.restart_pending = false;
                self.push_log(format!(
                    "snisplit stopped: adding the DNS rules failed: {cause}"
                ));
                Relay::Failed(format!("{ADMIN_NEEDED} Windows said: {cause}"))
            }
        };
        if self.closing {
            return self.close_ended();
        }
        if std::mem::take(&mut self.restart_pending) {
            return self.start();
        }
        Vec::new()
    }

    fn close_ended(&mut self) -> Vec<Effect> {
        let Relay::Failed(message) = &self.relay else {
            return vec![Effect::Exit];
        };
        self.relay = Relay::Failed(format!(
            "{}. {RULES_MAY_REMAIN}.",
            message.trim_end_matches('.')
        ));
        self.closing = false;
        Vec::new()
    }

    fn check(&mut self, name: &str) -> Vec<Effect> {
        let first_domain = self
            .sites
            .iter()
            .find(|site| site.name == name)
            .and_then(|site| site.domains.first())
            .cloned();
        match first_domain {
            Some(domain) => self.begin_probe(domain, Purpose::Check),
            None => Vec::new(),
        }
    }

    fn probe_typed(&mut self, text: &str) -> Vec<Effect> {
        if self.probing() {
            return Vec::new();
        }
        match snisplit::parse_domain(text.trim()) {
            Ok(domain) => self.begin_probe(domain, Purpose::Add),
            Err(problem) => {
                self.probe = Probe::Failed {
                    domain: text.trim().to_string(),
                    message: problem.to_string(),
                };
                Vec::new()
            }
        }
    }

    fn begin_probe(&mut self, domain: String, purpose: Purpose) -> Vec<Effect> {
        if self.probing() {
            return Vec::new();
        }
        self.notice = None;
        self.probe = Probe::Running {
            domain: domain.clone(),
            purpose,
        };
        vec![Effect::Probe(domain)]
    }

    fn probe_finished(&mut self, domain: String, result: Result<ProbeView, String>) {
        let Probe::Running {
            domain: running,
            purpose,
        } = &self.probe
        else {
            return;
        };
        if *running != domain {
            return;
        }
        let purpose = *purpose;
        self.probe = match result {
            Ok(view) => Probe::Done { purpose, view },
            Err(message) => Probe::Failed { domain, message },
        };
    }

    fn add(&mut self) -> Vec<Effect> {
        match &self.probe {
            Probe::Done {
                purpose: Purpose::Add,
                view:
                    ProbeView {
                        offer: Offer::Add(profile),
                        ..
                    },
            } => vec![Effect::SaveSite(profile.clone())],
            _ => Vec::new(),
        }
    }

    fn remove(&mut self, name: &str) -> Vec<Effect> {
        let removable = self
            .sites
            .iter()
            .any(|site| site.name == name && site.removable());
        if removable {
            vec![Effect::DeleteSite(name.to_string())]
        } else {
            Vec::new()
        }
    }

    fn site_changed(&mut self, changed: Result<String, String>, change: Change) -> Vec<Effect> {
        let name = match changed {
            Ok(name) => name,
            Err(message) => {
                self.push_log(message.clone());
                self.notice = Some(message);
                return Vec::new();
            }
        };
        let mut effects = vec![Effect::LoadSites];
        if change == Change::Added {
            self.probe = Probe::Idle;
        }
        let mut line = format!("site {name} {}", change.verb());
        match self.relay {
            Relay::Starting | Relay::Running if !self.closing => {
                line.push_str("; restarting snisplit so the change applies");
                self.restart_pending = true;
                effects.extend(self.stop());
            }
            Relay::Stopping if !self.closing && self.restart_pending => {
                line.push_str("; snisplit starts again once it has stopped");
            }
            _ => {}
        }
        self.notice = Some(line.clone());
        self.push_log(line);
        effects
    }

    fn close(&mut self) -> Vec<Effect> {
        self.closing = true;
        self.restart_pending = false;
        match self.relay {
            Relay::Starting | Relay::Running => {
                self.push_log("closing: stopping snisplit and removing its DNS rules".to_string());
                self.stop()
            }
            Relay::Stopping => Vec::new(),
            Relay::Stopped | Relay::Failed(_) => vec![Effect::Exit],
        }
    }

    fn push_log(&mut self, line: String) {
        if self.log.len() == LOG_LINES {
            self.log.pop_front();
        }
        self.log.push_back(line);
    }
}

pub fn site_list(built_ins: &[ProfileSpec], files: Vec<SiteFile>) -> Vec<Site> {
    let built_in = built_ins.iter().map(|spec| Site {
        name: spec.name.clone(),
        domains: spec.domains.clone(),
        kind: SiteKind::BuiltIn,
    });
    let user = files.into_iter().map(|file| match file.loaded {
        Ok(spec) => Site {
            name: file.name,
            domains: spec.domains,
            kind: SiteKind::User,
        },
        Err(problem) => Site {
            name: file.name,
            domains: Vec::new(),
            kind: SiteKind::Broken(problem),
        },
    });
    built_in.chain(user).collect()
}

#[cfg(test)]
mod tests;
