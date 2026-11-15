use std::future::Future;
use std::io;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Rules {
    Removed,
    MayRemain,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ending {
    Clean,
    RulesMayRemain,
    TimedOut,
}

#[derive(Default)]
struct Tracked {
    stop: Option<oneshot::Sender<()>>,
    tidying: Option<JoinHandle<Rules>>,
}

type Stopped = oneshot::Receiver<()>;
type Ready = Box<dyn FnOnce() + Send>;

pub struct Keeper {
    runtime: Runtime,
    tracked: Mutex<Tracked>,
}

impl Keeper {
    pub fn new() -> io::Result<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;
        Ok(Self {
            runtime,
            tracked: Mutex::new(Tracked::default()),
        })
    }

    pub fn driver(&self, folders: Folders, wake: Wake) -> (Driver<'_>, Receiver<Event>) {
        let (events, received) = channel();
        let driver = Driver {
            keeper: self,
            inbox: Inbox { events, wake },
            folders,
            probe: None,
        };
        (driver, received)
    }

    pub fn finish(self) -> Ending {
        self.finish_within(SHUTDOWN_LIMIT)
    }

    fn finish_within(self, limit: Duration) -> Ending {
        let (stop, tidying) = {
            let mut tracked = self.tracked();
            (tracked.stop.take(), tracked.tidying.take())
        };
        if let Some(stop) = stop {
            let _ = stop.send(());
        }
        let ending = match tidying {
            None => Ending::Clean,
            Some(task) => self.runtime.block_on(wait_for_tidying(task, limit)),
        };
        self.runtime.shutdown_timeout(RUNTIME_LIMIT);
        ending
    }

    fn tracked(&self) -> MutexGuard<'_, Tracked> {
        self.tracked.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

pub struct Driver<'keeper> {
    keeper: &'keeper Keeper,
    inbox: Inbox,
    folders: Folders,
    probe: Option<(u64, JoinHandle<()>)>,
}

impl Driver<'_> {
    fn runtime(&self) -> &Runtime {
        &self.keeper.runtime
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

    fn start_session(&mut self) {
        let options = RunOptions {
            verbose: false,
            profiles: BUILT_IN.iter().map(|built_in| built_in.name).collect(),
            profile_files: Vec::new(),
            site_folder: self.folders.sites.clone(),
            routing: true,
            upstreams: Vec::new(),
        };
        self.start_session_with(move |stopped, out, ready| async move {
            let stop_requested = async move {
                let _ = stopped.await;
            };
            session::serve_with_ready(options, stop_requested, out, ready).await
        });
    }

    fn start_session_with<Work>(&mut self, serve: impl FnOnce(Stopped, Output, Ready) -> Work)
    where
        Work: Future<Output = io::Result<()>> + Send + 'static,
    {
        let (stop, stopped) = oneshot::channel::<()>();
        let ready = self.inbox.clone();
        let ready: Ready = Box::new(move || ready.send(Event::SessionReady { at: Instant::now() }));
        let work = self
            .runtime()
            .spawn(serve(stopped, self.inbox.output(), ready));
        let task = self
            .runtime()
            .spawn(report_session_end(self.inbox.clone(), work));
        let mut tracked = self.keeper.tracked();
        tracked.stop = Some(stop);
        tracked.tidying = Some(task);
    }

    fn stop_session(&mut self) {
        if let Some(stop) = self.keeper.tracked().stop.take() {
            let _ = stop.send(());
        }
    }

    fn remove_leftovers(&self) {
        self.remove_leftovers_with(session::remove_rules);
    }

    fn remove_leftovers_with(
        &self,
        remove: impl FnOnce(&Output) -> io::Result<()> + Send + 'static,
    ) {
        let out = self.inbox.output();
        let work = self
            .runtime()
            .spawn_blocking(move || remove(&out).map_err(|error| error.to_string()));
        let task = self
            .runtime()
            .spawn(report_leftovers_removed(self.inbox.clone(), work));
        self.keeper.tracked().tidying = Some(task);
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

fn refusal(error: &io::Error) -> SaveRefusal {
    match sites::covered(error) {
        Some(covered) => SaveRefusal::Covered {
            domain: covered.domain.clone(),
            owner: covered.owner.clone(),
        },
        None => SaveRefusal::Other(error.to_string()),
    }
}

async fn wait_for_tidying(task: JoinHandle<Rules>, limit: Duration) -> Ending {
    match tokio::time::timeout(limit, task).await {
        Ok(Ok(Rules::Removed)) => Ending::Clean,
        Ok(Ok(Rules::MayRemain) | Err(_)) => Ending::RulesMayRemain,
        Err(_) => Ending::TimedOut,
    }
}

fn rules_after(end: &SessionEnd) -> Rules {
    match end {
        SessionEnd::Failed {
            failure: Failure::RemovingRules,
            ..
        } => Rules::MayRemain,
        SessionEnd::Clean | SessionEnd::Failed { .. } => Rules::Removed,
    }
}

async fn report_session_end(inbox: Inbox, work: JoinHandle<io::Result<()>>) -> Rules {
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
    let rules = rules_after(&end);
    inbox.send(Event::SessionEnded(end));
    rules
}

async fn report_leftovers_removed(inbox: Inbox, work: JoinHandle<Result<(), String>>) -> Rules {
    let removed = work.await.unwrap_or_else(|error| {
        Err(format!(
            "the removal {} before it reported",
            how_it_ended(&error)
        ))
    });
    let rules = if removed.is_ok() {
        Rules::Removed
    } else {
        Rules::MayRemain
    };
    inbox.send(Event::LeftoversRemoved(removed));
    rules
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
