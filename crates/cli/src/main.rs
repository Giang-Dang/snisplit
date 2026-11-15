mod args;
mod probe;

use std::env;
use std::io::{self, Write};
use std::path::Path;
use std::process::ExitCode;

use args::{Command, USAGE};
use snisplit::instance::{self, Claim, Instance};
use snisplit::session::{self, RunOptions};
use snisplit::sites;
use snisplit::{VERSION, profile};

const EXIT_RUNTIME_ERROR: u8 = 1;
const EXIT_USAGE_ERROR: u8 = 2;
const ALREADY_RUNNING: &str = "another snisplit is already running, or is still turning off: wait a few seconds, then try again";
const CLEANUP_WHILE_RUNNING: &str =
    "snisplit: warning: another snisplit is running and will lose its DNS rules";

fn main() -> ExitCode {
    let args = env::args_os()
        .skip(1)
        .map(|arg| arg.to_string_lossy().into_owned());
    match args::parse(args, sites::folder()) {
        Ok(Command::Help) => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Ok(Command::Version) => {
            println!("snisplit {VERSION}");
            ExitCode::SUCCESS
        }
        Ok(Command::Run(options)) => finish(run(options)),
        Ok(Command::Cleanup) => finish(cleanup()),
        Ok(Command::ShowProfile(name)) => finish(show_profile(name)),
        Ok(Command::Probe(options)) => finish(probe::run(&options)),
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

fn claim_instance() -> Claim {
    let sites_folder = sites::folder();
    instance::claim(sites_folder.as_deref().and_then(Path::parent))
}

fn cleanup() -> io::Result<()> {
    if let Claim::Elsewhere = claim_instance() {
        eprintln!("{CLEANUP_WHILE_RUNNING}");
    }
    session::remove_rules(&session::stdout())
}

fn only_instance() -> io::Result<Option<Instance>> {
    match claim_instance() {
        Claim::Ours(held) => Ok(Some(held)),
        Claim::Elsewhere => Err(io::Error::new(io::ErrorKind::AddrInUse, ALREADY_RUNNING)),
        Claim::Unlocked(error) => {
            eprintln!("snisplit: warning: not checking for another running snisplit: {error}");
            Ok(None)
        }
    }
}

fn run(options: RunOptions) -> io::Result<()> {
    let _held = if options.routing {
        only_instance()?
    } else {
        None
    };
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(async {
            let stop = session::stop_signals()?;
            session::serve(options, stop, session::stdout()).await
        })
}
