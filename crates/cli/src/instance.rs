use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::Path;

use platform::{held_by_another, open_exclusive};

const LOCK_FILE: &str = "instance.lock";
const NO_APP_FOLDER: &str =
    "no per-user snisplit folder: the APPDATA environment variable is not set";

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

pub fn claim(app_folder: Option<&Path>) -> Claim {
    let Some(app_folder) = app_folder else {
        return Claim::Unlocked(io::Error::new(io::ErrorKind::NotFound, NO_APP_FOLDER));
    };
    if let Err(error) = fs::create_dir_all(app_folder) {
        return Claim::Unlocked(error);
    }
    match open_exclusive(&app_folder.join(LOCK_FILE)) {
        Ok(open) => Claim::Ours(Instance { _open: open }),
        Err(error) if held_by_another(&error) => Claim::Elsewhere,
        Err(error) => Claim::Unlocked(error),
    }
}

fn lock_options() -> OpenOptions {
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
    const ERROR_SHARING_VIOLATION: i32 = 32;

    pub(super) fn open_exclusive(path: &Path) -> io::Result<File> {
        super::lock_options().share_mode(SHARE_NOTHING).open(path)
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
        super::lock_options().open(path)
    }

    pub(super) fn held_by_another(_error: &io::Error) -> bool {
        false
    }
}

#[cfg(test)]
mod tests;
