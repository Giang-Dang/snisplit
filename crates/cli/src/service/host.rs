use std::ffi::c_void;
use std::io;
use std::ptr;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::Duration;

use tokio::sync::watch;
use windows_sys::Win32::Foundation::{
    ERROR_CALL_NOT_IMPLEMENTED, ERROR_FAILED_SERVICE_CONTROLLER_CONNECT,
    ERROR_SERVICE_SPECIFIC_ERROR, NO_ERROR,
};
use windows_sys::Win32::System::Services::{
    LPHANDLER_FUNCTION_EX, LPSERVICE_MAIN_FUNCTIONW, RegisterServiceCtrlHandlerExW,
    SERVICE_ACCEPT_SHUTDOWN, SERVICE_ACCEPT_STOP, SERVICE_CONTROL_INTERROGATE,
    SERVICE_CONTROL_SHUTDOWN, SERVICE_CONTROL_STOP, SERVICE_RUNNING, SERVICE_STATUS,
    SERVICE_STATUS_HANDLE, SERVICE_STOP_PENDING, SERVICE_STOPPED, SERVICE_TABLE_ENTRYW,
    SERVICE_WIN32_OWN_PROCESS, SetServiceStatus, StartServiceCtrlDispatcherW,
};
use windows_sys::core::PWSTR;

use super::SERVICE_NAME;
use super::layout::Layout;
use super::log::{self, LogFile};
use super::scm::{is_error, wide};
use crate::VERSION;
use crate::args::RunOptions;
use crate::session::{self, Output, StartPolicy};

const STOP_WAIT_HINT_MS: u32 = 30_000;
const STOP_PENDING_EVERY: Duration = Duration::from_secs(1);
const RUNTIME_SHUTDOWN_LIMIT: Duration = Duration::from_secs(5);
const SESSION_FAILED: u32 = 1;
const LOG_UNAVAILABLE: u32 = 2;
const STARTED_BY_WINDOWS: StartPolicy = StartPolicy {
    wait_for_upstreams: true,
};

static SERVICE: OnceLock<Service> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Exit {
    Clean,
    ServiceSpecific(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Running,
    StopPending,
    Stopped(Exit),
}

fn service_status(phase: Phase, checkpoint: u32) -> SERVICE_STATUS {
    let (state, accepted, exit_code, specific_code, wait_hint) = match phase {
        Phase::Running => (
            SERVICE_RUNNING,
            SERVICE_ACCEPT_STOP | SERVICE_ACCEPT_SHUTDOWN,
            NO_ERROR,
            0,
            0,
        ),
        Phase::StopPending => (SERVICE_STOP_PENDING, 0, NO_ERROR, 0, STOP_WAIT_HINT_MS),
        Phase::Stopped(Exit::Clean) => (SERVICE_STOPPED, 0, NO_ERROR, 0, 0),
        Phase::Stopped(Exit::ServiceSpecific(code)) => {
            (SERVICE_STOPPED, 0, ERROR_SERVICE_SPECIFIC_ERROR, code, 0)
        }
    };
    SERVICE_STATUS {
        dwServiceType: SERVICE_WIN32_OWN_PROCESS,
        dwCurrentState: state,
        dwControlsAccepted: accepted,
        dwWin32ExitCode: exit_code,
        dwServiceSpecificExitCode: specific_code,
        dwCheckPoint: checkpoint,
        dwWaitHint: wait_hint,
    }
}

#[derive(Debug, Default)]
struct Reported {
    checkpoint: u32,
    stopped: bool,
}

impl Reported {
    fn next(&mut self, phase: Phase) -> Option<SERVICE_STATUS> {
        if self.stopped {
            return None;
        }
        let checkpoint = if phase == Phase::StopPending {
            self.checkpoint += 1;
            self.checkpoint
        } else {
            0
        };
        self.stopped = matches!(phase, Phase::Stopped(_));
        Some(service_status(phase, checkpoint))
    }
}

struct StatusReporter {
    handle: AtomicPtr<c_void>,
    reported: Mutex<Reported>,
}

impl StatusReporter {
    fn new() -> Self {
        Self {
            handle: AtomicPtr::new(ptr::null_mut()),
            reported: Mutex::new(Reported::default()),
        }
    }

    fn attach(&self, handle: SERVICE_STATUS_HANDLE) {
        self.handle.store(handle, Ordering::Release);
    }

    fn report(&self, phase: Phase) {
        let mut reported = self.reported.lock().unwrap_or_else(PoisonError::into_inner);
        let handle = self.handle.load(Ordering::Acquire);
        if handle.is_null() {
            return;
        }
        if let Some(next) = reported.next(phase) {
            let status = ptr::from_ref(&next);
            unsafe { SetServiceStatus(handle, status) };
        }
    }
}

struct Service {
    options: RunOptions,
    stop: watch::Sender<bool>,
    status: StatusReporter,
}

impl Service {
    fn main(&'static self) {
        let wide_name = wide(SERVICE_NAME);
        let name = wide_name.as_ptr();
        let handler: LPHANDLER_FUNCTION_EX = Some(handle_control);
        let no_context = ptr::null();
        let handle = unsafe { RegisterServiceCtrlHandlerExW(name, handler, no_context) };
        if handle.is_null() {
            return;
        }
        self.status.attach(handle);
        self.status.report(Phase::Running);
        let exit = self.relay_until_stopped();
        self.status.report(Phase::Stopped(exit));
    }

    fn relay_until_stopped(&'static self) -> Exit {
        let Ok(out) = open_log() else {
            return Exit::ServiceSpecific(LOG_UNAVAILABLE);
        };
        out(&format!("snisplit {VERSION} service starting"));
        match self.serve(Arc::clone(&out)) {
            Ok(()) => {
                out("service stopped");
                Exit::Clean
            }
            Err(error) => {
                out(&format!("service stopped with an error: {error}"));
                Exit::ServiceSpecific(SESSION_FAILED)
            }
        }
    }

    fn serve(&'static self, out: Output) -> io::Result<()> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;
        runtime.spawn(keep_reporting_stop_pending(
            self.stop.subscribe(),
            &self.status,
        ));
        let served = runtime.block_on(session::serve(
            self.options.clone(),
            STARTED_BY_WINDOWS,
            stop_requested(self.stop.subscribe()),
            out,
        ));
        runtime.shutdown_timeout(RUNTIME_SHUTDOWN_LIMIT);
        served
    }

    fn request_stop(&self) {
        self.status.report(Phase::StopPending);
        self.stop.send_replace(true);
    }
}

fn open_log() -> io::Result<Output> {
    let layout = Layout::from_environment()?;
    let log = LogFile::open(
        &layout.log_file(),
        &layout.old_log_file(),
        log::ROTATE_AFTER,
    )?;
    Ok(log.into_output())
}

async fn stop_requested(mut stop: watch::Receiver<bool>) {
    let _ = stop.wait_for(|&stop| stop).await;
}

async fn keep_reporting_stop_pending(
    mut stop: watch::Receiver<bool>,
    status: &'static StatusReporter,
) {
    if stop.wait_for(|&stop| stop).await.is_err() {
        return;
    }
    loop {
        tokio::time::sleep(STOP_PENDING_EVERY).await;
        status.report(Phase::StopPending);
    }
}

extern "system" fn service_main(_argument_count: u32, _arguments: *mut PWSTR) {
    if let Some(service) = SERVICE.get() {
        service.main();
    }
}

extern "system" fn handle_control(
    control: u32,
    _event_type: u32,
    _event_data: *mut c_void,
    _context: *mut c_void,
) -> u32 {
    match control {
        SERVICE_CONTROL_STOP | SERVICE_CONTROL_SHUTDOWN => {
            if let Some(service) = SERVICE.get() {
                service.request_stop();
            }
            NO_ERROR
        }
        SERVICE_CONTROL_INTERROGATE => NO_ERROR,
        _ => ERROR_CALL_NOT_IMPLEMENTED,
    }
}

pub fn run(options: RunOptions) -> io::Result<()> {
    let service = Service {
        options,
        stop: watch::Sender::new(false),
        status: StatusReporter::new(),
    };
    if SERVICE.set(service).is_err() {
        return Err(io::Error::other("the service host can start only once"));
    }
    let mut wide_name = wide(SERVICE_NAME);
    let main: LPSERVICE_MAIN_FUNCTIONW = Some(service_main);
    let entries = [
        SERVICE_TABLE_ENTRYW {
            lpServiceName: wide_name.as_mut_ptr(),
            lpServiceProc: main,
        },
        SERVICE_TABLE_ENTRYW::default(),
    ];
    let table = entries.as_ptr();
    let dispatched = unsafe { StartServiceCtrlDispatcherW(table) };
    if dispatched == 0 {
        return Err(dispatcher_failure(io::Error::last_os_error()));
    }
    Ok(())
}

fn dispatcher_failure(error: io::Error) -> io::Error {
    if is_error(&error, ERROR_FAILED_SERVICE_CONTROLLER_CONNECT) {
        io::Error::other(
            "snisplit service run is only for the Service Control Manager; \
             install the service with snisplit service install, or use snisplit run in a terminal",
        )
    } else {
        error
    }
}

#[cfg(test)]
mod tests;
