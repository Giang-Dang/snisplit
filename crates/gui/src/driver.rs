use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use snisplit::profile::BUILT_IN;
use snisplit::session::{self, Failure, Output, RunOptions};
use snisplit::{
    DEFAULT_PROBE_RUNS, DEFAULT_PROBE_TIMEOUT, PIECE_GAP, UPSTREAM_TIMEOUT, sites, system_dns,
    upstream_servers,
};
use snisplit_relay::probe::{ProbeSettings, probe};
use snisplit_relay::{UdpResolver, Upstreams};
use tokio::runtime::Runtime;
use tokio::sync::oneshot;
use tokio::task::{JoinError, JoinHandle};

use crate::log_file;
use crate::report;
use crate::state::{Effect, Event, ProbeView, SaveRefusal, SessionEnd, site_list};

const SHUTDOWN_LIMIT: Duration = Duration::from_secs(60);
const RUNTIME_LIMIT: Duration = Duration::from_secs(1);
const NO_FOLDER: &str = "no site folder: the APPDATA environment variable is not set";
const NO_SYSTEM_DNS: &str = "no usable system DNS server found";

pub type Wake = Arc<dyn Fn() + Send + Sync>;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Folders {
    pub sites: Option<PathBuf>,
    pub home: Option<PathBuf>,
}

#[derive(Clone)]
struct Inbox {
    events: Sender<Event>,
    wake: Wake,
}

impl Inbox {
    fn send(&self, event: Event) {
        if self.events.send(event).is_ok() {
            (self.wake)();
        }
    }

    fn output(&self) -> Output {
        let lines = self.clone();
        Arc::new(move |line| lines.send(Event::SessionLine(line.to_string())))
    }
}

struct Session {
    stop: Option<oneshot::Sender<()>>,
    ended: JoinHandle<()>,
}

pub struct Driver {
    runtime: Option<Runtime>,
    inbox: Inbox,
    folders: Folders,
    session: Option<Session>,
    probe: Option<(u64, JoinHandle<()>)>,
}

impl Driver {
    pub fn new(folders: Folders, wake: Wake) -> io::Result<(Self, Receiver<Event>)> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;
        let (events, received) = channel();
        let driver = Self {
            runtime: Some(runtime),
            inbox: Inbox { events, wake },
            folders,
            session: None,
            probe: None,
        };
        Ok((driver, received))
    }

    fn runtime(&self) -> &Runtime {
        self.runtime
            .as_ref()
            .expect("the runtime lives until the driver is dropped")
    }

    pub fn run(&mut self, effect: Effect) {
        match effect {
            Effect::StartSession => self.start_session(),
            Effect::StopSession => self.stop_session(),
            Effect::RemoveLeftovers => self.remove_leftovers(),
            Effect::Probe { id, domain } => self.probe(id, domain),
            Effect::CancelProbe(id) => self.cancel_probe(id),
            Effect::SetSwitch { name, on } => self.set_switch(name, on),
            Effect::SaveSite(text) => self.save_site(&text),
            Effect::DeleteSite(name) => self.delete_site(&name),
            Effect::LoadSites => self.load_sites(),
            Effect::SaveLog(lines) => self.save_log(&lines),
            Effect::Exit => {}
        }
    }

    pub fn shutdown(&mut self) {
        self.stop_session();
        if let Some(session) = self.session.take() {
            wait_until_ended(self.runtime(), session.ended, SHUTDOWN_LIMIT);
        }
    }

    fn start_session(&mut self) {
        let (stop, stopped) = oneshot::channel::<()>();
        let options = RunOptions {
            verbose: false,
            profiles: BUILT_IN.iter().map(|built_in| built_in.name).collect(),
            profile_files: Vec::new(),
            site_folder: self.folders.sites.clone(),
            routing: true,
            upstreams: Vec::new(),
        };
        let out = self.inbox.output();
        let ready = self.inbox.clone();
        let work = self.runtime().spawn(async move {
            let stop_requested = async move {
                let _ = stopped.await;
            };
            session::serve_with_ready(options, stop_requested, out, move || {
                ready.send(Event::SessionReady { at: Instant::now() });
            })
            .await
        });
        let task = self
            .runtime()
            .spawn(report_session_end(self.inbox.clone(), work));
        self.session = Some(Session {
            stop: Some(stop),
            ended: task,
        });
    }

    fn stop_session(&mut self) {
        if let Some(stop) = self
            .session
            .as_mut()
            .and_then(|session| session.stop.take())
        {
            let _ = stop.send(());
        }
    }

    fn remove_leftovers(&self) {
        let out = self.inbox.output();
        let work = self
            .runtime()
            .spawn_blocking(move || session::remove_rules(&out).map_err(|error| error.to_string()));
        self.runtime()
            .spawn(report_leftovers_removed(self.inbox.clone(), work));
    }

    fn probe(&mut self, id: u64, domain: String) {
        let inbox = self.inbox.clone();
        let task = self.runtime().spawn(async move {
            let result = probe_domain(&domain).await;
            inbox.send(Event::ProbeFinished {
                id,
                result,
                at: Instant::now(),
            });
        });
        if let Some((_, earlier)) = self.probe.replace((id, task)) {
            earlier.abort();
        }
    }

    fn cancel_probe(&mut self, id: u64) {
        if self
            .probe
            .as_ref()
            .is_some_and(|(running, _)| *running == id)
            && let Some((_, task)) = self.probe.take()
        {
            task.abort();
        }
    }

    fn set_switch(&self, name: String, on: bool) {
        let saved = match &self.folders.sites {
            Some(folder) => sites::set_switch(folder, &name, on)
                .map(|()| (name, on))
                .map_err(|error| error.to_string()),
            None => Err(NO_FOLDER.to_string()),
        };
        self.inbox.send(Event::SwitchSaved(saved));
    }

    fn save_site(&self, text: &str) {
        let saved = match &self.folders.sites {
            Some(folder) => sites::add(folder, text)
                .map(|spec| spec.name)
                .map_err(|error| refusal(&error)),
            None => Err(SaveRefusal::Other(NO_FOLDER.to_string())),
        };
        self.inbox.send(Event::SiteSaved(saved));
    }

    fn delete_site(&self, name: &str) {
        let removed = match &self.folders.sites {
            Some(folder) => sites::remove(folder, name)
                .map(|()| name.to_string())
                .map_err(|error| error.to_string()),
            None => Err(NO_FOLDER.to_string()),
        };
        self.inbox.send(Event::SiteRemoved(removed));
    }

    fn load_sites(&self) {
        let built_ins = match sites::built_in_specs() {
            Ok(built_ins) => built_ins,
            Err(error) => {
                self.inbox.send(Event::SessionLine(error.to_string()));
                Vec::new()
            }
        };
        let (files, off) = match &self.folders.sites {
            Some(folder) => (
                sites::list(folder).unwrap_or_else(|error| {
                    self.inbox.send(Event::SessionLine(format!(
                        "cannot read the site folder: {error}"
                    )));
                    Vec::new()
                }),
                sites::switched_off(folder).unwrap_or_else(|error| {
                    self.inbox.send(Event::SessionLine(format!(
                        "off.list not read, so every site is on: {error}"
                    )));
                    Vec::new()
                }),
            ),
            None => (Vec::new(), Vec::new()),
        };
        self.inbox
            .send(Event::SitesLoaded(site_list(&built_ins, files, &off)));
    }

    fn save_log(&self, lines: &[String]) {
        let app_folder = self
            .folders
            .sites
            .as_deref()
            .and_then(std::path::Path::parent);
        let saved = log_file::save(lines, self.folders.home.as_deref(), app_folder)
            .map_err(|error| error.to_string());
        self.inbox.send(Event::LogSaved(saved));
    }
}

impl Drop for Driver {
    fn drop(&mut self) {
        self.shutdown();
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_timeout(RUNTIME_LIMIT);
        }
    }
}

fn refusal(error: &io::Error) -> SaveRefusal {
    match sites::covered(error) {
        Some(covered) => SaveRefusal::Covered {
            domain: covered.domain.clone(),
            owner: covered.owner.clone(),
        },
        None => SaveRefusal::Other(error.to_string()),
    }
}

fn wait_until_ended(runtime: &Runtime, task: JoinHandle<()>, limit: Duration) -> bool {
    runtime.block_on(async move { tokio::time::timeout(limit, task).await.is_ok() })
}

async fn report_session_end(inbox: Inbox, work: JoinHandle<io::Result<()>>) {
    let end = match work.await {
        Ok(result) => session_end(result),
        Err(error) => SessionEnd::Failed {
            failure: Failure::RemovingRules,
            detail: format!(
                "the session {}, so DNS rules may remain",
                how_it_ended(&error)
            ),
        },
    };
    inbox.send(Event::SessionEnded(end));
}

async fn report_leftovers_removed(inbox: Inbox, work: JoinHandle<Result<(), String>>) {
    let removed = work.await.unwrap_or_else(|error| {
        Err(format!(
            "the removal {} before it reported",
            how_it_ended(&error)
        ))
    });
    inbox.send(Event::LeftoversRemoved(removed));
}

fn how_it_ended(error: &JoinError) -> &'static str {
    if error.is_panic() {
        "crashed"
    } else {
        "was cancelled"
    }
}

fn session_end(result: io::Result<()>) -> SessionEnd {
    let error = match result {
        Ok(()) => return SessionEnd::Clean,
        Err(error) => error,
    };
    let detail = match session::routing_failure_cause(&error) {
        Some(cause) => format!("changing the DNS rules failed: {cause}"),
        None => error.to_string(),
    };
    SessionEnd::Failed {
        failure: session::failure(&error),
        detail,
    }
}

async fn probe_domain(domain: &str) -> Result<ProbeView, String> {
    let system = tokio::task::spawn_blocking(system_dns::servers)
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())?;
    let servers = upstream_servers(&[], &system);
    if servers.is_empty() {
        return Err(NO_SYSTEM_DNS.to_string());
    }
    let settings = ProbeSettings {
        runs: DEFAULT_PROBE_RUNS,
        timeout: DEFAULT_PROBE_TIMEOUT,
        piece_gap: PIECE_GAP,
    };
    let resolver = UdpResolver::new(Upstreams::new(servers), UPSTREAM_TIMEOUT);
    probe(&resolver, domain, &settings)
        .await
        .map(|report| report::view(&report))
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests;
