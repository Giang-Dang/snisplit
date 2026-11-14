mod args;
mod plan;
mod profile;
mod profile_file;
mod routing;
mod service;
mod session;
mod system_dns;

use std::env;
use std::io::{self, Write};
use std::process::ExitCode;

use args::{Command, RunOptions, USAGE};
use session::StartPolicy;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const EXIT_RUNTIME_ERROR: u8 = 1;
const EXIT_USAGE_ERROR: u8 = 2;
const FOREGROUND: StartPolicy = StartPolicy {
    wait_for_upstreams: false,
};

fn main() -> ExitCode {
    let args = env::args_os()
        .skip(1)
        .map(|arg| arg.to_string_lossy().into_owned());
    match args::parse(args) {
        Ok(Command::Help) => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Ok(Command::Version) => {
            println!("snisplit {VERSION}");
            ExitCode::SUCCESS
        }
        Ok(Command::Run(options)) => finish(run(options)),
        Ok(Command::Service(command)) => finish(service::execute(command)),
        Ok(Command::Cleanup) => finish(session::remove_rules(&session::stdout())),
        Ok(Command::ShowProfile(name)) => finish(show_profile(name)),
        Err(error) => {
            eprintln!("snisplit: {error}\n\n{USAGE}");
            ExitCode::from(EXIT_USAGE_ERROR)
        }
    }
}

fn finish(result: io::Result<()>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("snisplit: error: {error}");
            ExitCode::from(EXIT_RUNTIME_ERROR)
        }
    }
}

fn show_profile(name: &str) -> io::Result<()> {
    let built_in = profile::find(name).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("no built-in profile {name}"),
        )
    })?;
    io::stdout().write_all(built_in.text.as_bytes())
}

fn run(options: RunOptions) -> io::Result<()> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(async {
            let ctrl_c = session::ctrl_c()?;
            session::serve(options, FOREGROUND, ctrl_c, session::stdout()).await
        })
}
