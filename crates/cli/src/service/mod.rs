#[cfg(windows)]
mod command_line;
#[cfg(windows)]
mod control;
#[cfg(windows)]
mod host;
#[cfg(windows)]
mod layout;
#[cfg(windows)]
mod log;
#[cfg(windows)]
mod scm;

use std::io;

use crate::args::ServiceCommand;

#[cfg(windows)]
const SERVICE_NAME: &str = "snisplit";

#[cfg(windows)]
pub fn execute(command: ServiceCommand) -> io::Result<()> {
    match command {
        ServiceCommand::Install(options) => control::install(&options),
        ServiceCommand::Uninstall => control::uninstall(),
        ServiceCommand::Run(options) => host::run(options),
    }
}

#[cfg(not(windows))]
pub fn execute(command: ServiceCommand) -> io::Result<()> {
    let name = match command {
        ServiceCommand::Install(_) => "install",
        ServiceCommand::Uninstall => "uninstall",
        ServiceCommand::Run(_) => "run",
    };
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        format!("snisplit service {name} is Windows only"),
    ))
}
