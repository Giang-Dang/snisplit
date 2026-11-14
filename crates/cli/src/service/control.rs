use std::env;
use std::fs;
use std::io;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{
    ERROR_ACCESS_DENIED, ERROR_SERVICE_CANNOT_ACCEPT_CTRL, ERROR_SERVICE_EXISTS,
    ERROR_SERVICE_MARKED_FOR_DELETE, ERROR_SERVICE_NOT_ACTIVE,
};
use windows_sys::Win32::System::Services::{
    SC_ACTION, SC_ACTION_RESTART, SC_MANAGER_CONNECT, SC_MANAGER_CREATE_SERVICE,
    SERVICE_ALL_ACCESS, SERVICE_QUERY_STATUS, SERVICE_STOP_PENDING, SERVICE_STOPPED,
};

use super::SERVICE_NAME;
use super::command_line::{command_line, service_arguments};
use super::layout::{Layout, is_inside, is_same_file};
use super::scm::{self, Handle, is_error};
use crate::args::RunOptions;
use crate::{plan, profile, session};

const DESCRIPTION: &str = "Answers DNS for the listed domains with a loopback address and relays their connections, so SNI and Host blocking does not apply to them.";
const RESET_FAILURE_COUNT_AFTER: Duration = Duration::from_secs(86_400);
const RESTART_DELAYS: [Duration; 3] = [
    Duration::from_secs(5),
    Duration::from_secs(5),
    Duration::from_secs(30),
];
const STOP_WAIT: Duration = Duration::from_secs(35);
const REMOVE_WAIT: Duration = Duration::from_secs(10);
const POLL_EVERY: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Removal {
    Removed,
    NothingThere,
    LeftInUse,
}

pub fn install(options: &RunOptions) -> io::Result<()> {
    check_profiles(options)?;
    let layout = Layout::from_environment()?;
    let manager = scm::open_manager(SC_MANAGER_CONNECT | SC_MANAGER_CREATE_SERVICE)
        .map_err(|error| needs_administrator(error, "install"))?;
    if scm::open_service(&manager, SERVICE_QUERY_STATUS)?.is_some() {
        return Err(already_installed());
    }
    let installed = copy_files(&layout, options, &env::current_exe()?)?;
    let line = command_line(
        &layout.executable().to_string_lossy(),
        &service_arguments(&installed),
    );
    let service = scm::create_service(&manager, &line).map_err(creation_failure)?;
    if let Err(error) = configure(&service) {
        let _ = scm::delete(&service);
        return Err(io::Error::new(
            error.kind(),
            format!("configuring the service failed, so it was removed again: {error}"),
        ));
    }
    println!("service {SERVICE_NAME} installed: {line}");
    scm::start(&service).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "service {SERVICE_NAME} is installed but did not start: {error}; see {}",
                layout.log_file().display()
            ),
        )
    })?;
    println!(
        "service {SERVICE_NAME} started; it logs to {}",
        layout.log_file().display()
    );
    Ok(())
}

pub fn uninstall() -> io::Result<()> {
    let layout = Layout::from_environment()?;
    let manager = scm::open_manager(SC_MANAGER_CONNECT)?;
    let service = scm::open_service(&manager, SERVICE_ALL_ACCESS)
        .map_err(|error| needs_administrator(error, "uninstall"))?;
    match service {
        Some(service) => {
            stop(&service)?;
            scm::delete(&service)?;
            stop(&service)?;
            println!("service {SERVICE_NAME} stopped and removed");
        }
        None => println!("service {SERVICE_NAME} is not installed"),
    }
    drop(manager);
    session::remove_rules(&session::stdout())?;
    let root = layout.root();
    match remove_files(&layout, &env::current_exe()?)? {
        Removal::Removed => println!("removed {}", root.display()),
        Removal::NothingThere => {}
        Removal::LeftInUse => println!(
            "left {}: this snisplit.exe runs from it; delete the folder after this command ends",
            root.display()
        ),
    }
    Ok(())
}

fn check_profiles(options: &RunOptions) -> io::Result<()> {
    let files = options
        .profile_files
        .iter()
        .map(|path| profile::read_file(path))
        .collect::<io::Result<Vec<_>>>()?;
    plan::select(&options.profiles, files)
        .map(|_| ())
        .map_err(io::Error::other)
}

fn copy_files(layout: &Layout, options: &RunOptions, executable: &Path) -> io::Result<RunOptions> {
    fs::create_dir_all(layout.root())?;
    let installed_executable = layout.executable();
    if !is_same_file(executable, &installed_executable) {
        fs::copy(executable, &installed_executable)
            .map_err(|error| copy_failure(executable, error))?;
    }
    let profiles = layout.profiles();
    if profiles.exists() {
        fs::remove_dir_all(&profiles)?;
    }
    fs::create_dir_all(&profiles)?;
    let mut copies = Vec::new();
    for (index, source) in options.profile_files.iter().enumerate() {
        let copy = layout.profile(index + 1);
        fs::copy(source, &copy).map_err(|error| copy_failure(source, error))?;
        copies.push(copy);
    }
    Ok(RunOptions {
        profile_files: copies,
        ..options.clone()
    })
}

fn configure(service: &Handle) -> io::Result<()> {
    scm::set_description(service, DESCRIPTION)?;
    scm::set_failure_actions(service, reset_period_seconds(), restart_actions())?;
    scm::apply_failure_actions_to_error_exits(service)
}

fn reset_period_seconds() -> u32 {
    u32::try_from(RESET_FAILURE_COUNT_AFTER.as_secs()).unwrap_or(u32::MAX)
}

fn restart_actions() -> Vec<SC_ACTION> {
    RESTART_DELAYS
        .iter()
        .map(|delay| SC_ACTION {
            Type: SC_ACTION_RESTART,
            Delay: u32::try_from(delay.as_millis()).unwrap_or(u32::MAX),
        })
        .collect()
}

fn stop(service: &Handle) -> io::Result<()> {
    let deadline = Instant::now() + STOP_WAIT;
    loop {
        let state = scm::current_state(service)?;
        if state == SERVICE_STOPPED {
            return Ok(());
        }
        if state != SERVICE_STOP_PENDING {
            send_stop(service)?;
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!(
                    "service {SERVICE_NAME} did not stop within {} s; nothing was removed",
                    STOP_WAIT.as_secs()
                ),
            ));
        }
        thread::sleep(POLL_EVERY);
    }
}

fn send_stop(service: &Handle) -> io::Result<()> {
    match scm::send_stop(service) {
        Err(error)
            if is_error(&error, ERROR_SERVICE_NOT_ACTIVE)
                || is_error(&error, ERROR_SERVICE_CANNOT_ACCEPT_CTRL) =>
        {
            Ok(())
        }
        other => other,
    }
}

fn remove_files(layout: &Layout, running: &Path) -> io::Result<Removal> {
    let root = layout.root();
    if !root.exists() {
        return Ok(Removal::NothingThere);
    }
    if is_inside(running, root) {
        return Ok(Removal::LeftInUse);
    }
    let deadline = Instant::now() + REMOVE_WAIT;
    loop {
        match fs::remove_dir_all(root) {
            Ok(()) => return Ok(Removal::Removed),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Removal::Removed),
            Err(error) if Instant::now() >= deadline => {
                return Err(io::Error::new(
                    error.kind(),
                    format!("cannot remove {}: {error}", root.display()),
                ));
            }
            Err(_) => thread::sleep(POLL_EVERY),
        }
    }
}

fn needs_administrator(error: io::Error, command: &str) -> io::Error {
    if is_error(&error, ERROR_ACCESS_DENIED) {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("snisplit service {command} needs an administrator terminal"),
        )
    } else {
        error
    }
}

fn already_installed() -> io::Error {
    io::Error::new(
        io::ErrorKind::AlreadyExists,
        format!("service {SERVICE_NAME} already exists; run snisplit service uninstall first"),
    )
}

fn creation_failure(error: io::Error) -> io::Error {
    if is_error(&error, ERROR_SERVICE_EXISTS) {
        already_installed()
    } else if is_error(&error, ERROR_SERVICE_MARKED_FOR_DELETE) {
        io::Error::new(
            error.kind(),
            format!(
                "service {SERVICE_NAME} is still being removed; close the Services window and try again"
            ),
        )
    } else {
        error
    }
}

fn copy_failure(source: &Path, error: io::Error) -> io::Error {
    io::Error::new(
        error.kind(),
        format!("cannot copy {}: {error}", source.display()),
    )
}

#[cfg(test)]
mod tests;
