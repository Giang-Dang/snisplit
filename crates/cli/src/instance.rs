use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

use platform::{held_by_another, open_exclusive};

const MACHINE_FOLDER_VARIABLE: &str = "ProgramData";
const APP_FOLDER: &str = "snisplit";
const LOCK_FILE: &str = "instance.lock";
const NO_MACHINE_FOLDER: &str =
    "no machine-wide snisplit folder: the ProgramData environment variable is not set";

#[derive(Debug)]
pub struct Instance {
    _open: File,
}

#[derive(Debug)]
pub enum Claim {
    Ours(Instance),
    Elsewhere,
    Unlocked(io::Error),
}

pub fn folder() -> Option<PathBuf> {
    folder_from(|key| std::env::var_os(key))
}

fn folder_from(lookup: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let program_data = lookup(MACHINE_FOLDER_VARIABLE).filter(|value| !value.is_empty())?;
    Some(PathBuf::from(program_data).join(APP_FOLDER))
}

pub fn lock_path(folder: &Path) -> PathBuf {
    folder.join(LOCK_FILE)
}

pub fn claim(folder: Option<&Path>) -> Claim {
    let Some(folder) = folder else {
        return Claim::Unlocked(io::Error::new(io::ErrorKind::NotFound, NO_MACHINE_FOLDER));
    };
    if let Err(error) = fs::create_dir_all(folder) {
        return Claim::Unlocked(error);
    }
    match open_exclusive(&lock_path(folder)) {
        Ok(open) => Claim::Ours(Instance { _open: open }),
        Err(error) if held_by_another(&error) => Claim::Elsewhere,
        Err(error) => Claim::Unlocked(error),
    }
}

fn creating_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    options
}

#[cfg(windows)]
mod platform {
    use std::fs::File;
    use std::io;
    use std::os::windows::fs::OpenOptionsExt;
    use std::path::Path;

    const SHARE_NOTHING: u32 = 0;
    const FILE_READ_DATA: u32 = 0x0001;
    const ERROR_SHARING_VIOLATION: i32 = 32;

    pub(super) fn open_exclusive(path: &Path) -> io::Result<File> {
        super::creating_options()
            .access_mode(FILE_READ_DATA)
            .share_mode(SHARE_NOTHING)
            .open(path)
    }

    pub(super) fn held_by_another(error: &io::Error) -> bool {
        error.raw_os_error() == Some(ERROR_SHARING_VIOLATION)
    }
}

#[cfg(not(windows))]
mod platform {
    use std::fs::File;
    use std::io;
    use std::path::Path;

    pub(super) fn open_exclusive(path: &Path) -> io::Result<File> {
        super::creating_options().open(path)
    }

    pub(super) fn held_by_another(_error: &io::Error) -> bool {
        false
    }
}

#[cfg(test)]
mod tests;
