#![cfg_attr(windows, windows_subsystem = "windows")]

use std::process::ExitCode;

#[cfg(windows)]
mod window;

#[cfg(windows)]
fn main() -> ExitCode {
    window::run()
}

#[cfg(not(windows))]
fn main() -> ExitCode {
    eprintln!("snisplit-gui: the UI is Windows-only for now; use the snisplit command instead");
    ExitCode::FAILURE
}
