use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use snisplit::profile::BUILT_IN;
use snisplit::session::{self, Output, RunOptions};
use snisplit::{
    DEFAULT_PROBE_RUNS, DEFAULT_PROBE_TIMEOUT, PIECE_GAP, UPSTREAM_TIMEOUT, sites, system_dns,
    upstream_servers,
};
use snisplit_relay::probe::{ProbeSettings, probe};
use snisplit_relay::{UdpResolver, Upstreams};
use tokio::runtime::Runtime;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use crate::report;
use crate::state::{Effect, Event, SessionEnd, site_list};

const SHUTDOWN_LIMIT: Duration = Duration::from_secs(60);
const NO_FOLDER: &str = "no site folder: the APPDATA environment variable is not set";
const NO_SYSTEM_DNS: &str = "no usable system DNS server found";

pub type Wake = Arc<dyn Fn() + Send + Sync>;

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
}

struct Session {
    stop: Option<oneshot::Sender<()>>,
    ended: JoinHandle<()>,
}

pub struct Driver {
    runtime: Runtime,
    inbox: Inbox,
    folder: Option<PathBuf>,
    session: Option<Session>,
}

impl Driver {
    pub fn new(folder: Option<PathBuf>, wake: Wake) -> io::Result<(Self, Receiver<Event>)> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;
        let (events, received) = channel();
        let driver = Self {
            runtime,
            inbox: Inbox { events, wake },
            folder,
            session: None,
        };
        Ok((driver, received))
    }

    pub fn run(&mut self, effect: Effect) {
        match effect {
            Effect::StartSession => self.start_session(),
            Effect::StopSession => self.stop_session(),
            Effect::Probe(domain) => self.probe(domain),
            Effect::SaveSite(text) => self.save_site(&text),
            Effect::DeleteSite(name) => self.delete_site(&name),
            Effect::LoadSites => self.load_sites(),
            Effect::Exit => {}
        }
    }

    pub fn shutdown(&mut self) {
        self.stop_session();
        if let Some(session) = self.session.take() {
            let _ = self
                .runtime
                .block_on(tokio::time::timeout(SHUTDOWN_LIMIT, session.ended));
        }
    }

    fn start_session(&mut self) {
        let (stop, stopped) = oneshot::channel::<()>();
        let options = RunOptions {
            verbose: false,
            profiles: BUILT_IN.iter().map(|built_in| built_in.name).collect(),
            profile_files: Vec::new(),
            site_folder: self.folder.clone(),
            routing: true,
            upstreams: Vec::new(),
        };
        let lines = self.inbox.clone();
        let out: Output = Arc::new(move |line| lines.send(Event::SessionLine(line.to_string())));
        let ready = self.inbox.clone();
        let ended = self.inbox.clone();
        let task = self.runtime.spawn(async move {
            let stop_requested = async move {
                let _ = stopped.await;
            };
            let result = session::serve_with_ready(options, stop_requested, out, move || {
                ready.send(Event::SessionReady);
            })
            .await;
            ended.send(Event::SessionEnded(session_end(result)));
        });
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

    fn probe(&self, domain: String) {
        let inbox = self.inbox.clone();
        self.runtime.spawn(async move {
            let result = probe_domain(&domain).await;
            inbox.send(Event::ProbeFinished { domain, result });
        });
    }

    fn save_site(&self, text: &str) {
        let saved = match &self.folder {
            Some(folder) => sites::add(folder, text)
                .map(|spec| spec.name)
                .map_err(|error| error.to_string()),
            None => Err(NO_FOLDER.to_string()),
        };
        self.inbox.send(Event::SiteSaved(saved));
    }

    fn delete_site(&self, name: &str) {
        let removed = match &self.folder {
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
        let files = match &self.folder {
            Some(folder) => sites::list(folder).unwrap_or_else(|error| {
                self.inbox.send(Event::SessionLine(format!(
                    "cannot read the site folder: {error}"
                )));
                Vec::new()
            }),
            None => Vec::new(),
        };
        self.inbox
            .send(Event::SitesLoaded(site_list(&built_ins, files)));
    }
}

impl Drop for Driver {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn session_end(result: io::Result<()>) -> SessionEnd {
    match result {
        Ok(()) => SessionEnd::Clean,
        Err(error) => SessionEnd::Failed {
            routing: session::is_routing_failure(&error),
            message: error.to_string(),
        },
    }
}

async fn probe_domain(domain: &str) -> Result<crate::state::ProbeView, String> {
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
