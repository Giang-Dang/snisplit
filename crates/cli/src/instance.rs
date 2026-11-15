use std::ffi::OsString;
use std::fs::{self, File, Metadata, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

use platform::{held_by_another, is_link, open_exclusive};

const MACHINE_FOLDER_VARIABLE: &str = "ProgramData";
const APP_FOLDER: &str = "snisplit";
const LOCK_FILE: &str = "instance.lock";
const NO_MACHINE_FOLDER: &str =
    "no machine-wide snisplit folder: the ProgramData environment variable is not set";
const FOLDER_REFUSED: &str =
    "the snisplit lock folder is a link or not a real folder, so it is not used";
const FILE_REFUSED: &str =
    "the snisplit lock file is a link or not a regular file, so it is not used";

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
    if let Err(error) = usable_folder(folder) {
        return Claim::Unlocked(error);
    }
    let path = lock_path(folder);
    if let Err(error) = usable_existing_file(&path) {
        return Claim::Unlocked(error);
    }
    match open_exclusive(&path) {
        Ok(open) => Claim::Ours(Instance { _open: open }),
        Err(error) if held_by_another(&error) => Claim::Elsewhere,
        Err(error) => Claim::Unlocked(error),
    }
}

fn usable_folder(folder: &Path) -> io::Result<()> {
    if existing(folder)?.is_some() {
        return real_folder(folder);
    }
    fs::create_dir_all(folder)?;
    real_folder(folder)
}

fn real_folder(folder: &Path) -> io::Result<()> {
    match existing(folder)? {
        Some(metadata) if metadata.is_dir() && !is_link(&metadata) => Ok(()),
        _ => Err(refused(FOLDER_REFUSED)),
    }
}

fn usable_existing_file(path: &Path) -> io::Result<()> {
    match existing(path)? {
        Some(metadata) if !metadata.is_file() || is_link(&metadata) => Err(refused(FILE_REFUSED)),
        _ => Ok(()),
    }
}

fn existing(path: &Path) -> io::Result<Option<Metadata>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn refused(why: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, why)
}

fn creating_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    options
}

#[cfg(windows)]
mod platform {
    use std::fs::{File, Metadata};
    use std::io;
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    use std::path::Path;

    const SHARE_NOTHING: u32 = 0;
    const FILE_READ_DATA: u32 = 0x0001;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    const ERROR_SHARING_VIOLATION: i32 = 32;

    pub(super) fn open_exclusive(path: &Path) -> io::Result<File> {
        super::creating_options()
            .access_mode(FILE_READ_DATA)
            .share_mode(SHARE_NOTHING)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)
    }

    pub(super) fn held_by_another(error: &io::Error) -> bool {
        error.raw_os_error() == Some(ERROR_SHARING_VIOLATION)
    }

    pub(super) fn is_link(metadata: &Metadata) -> bool {
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
}

#[cfg(not(windows))]
mod platform {
    use std::fs::{File, Metadata};
    use std::io;
    use std::path::Path;

    pub(super) fn open_exclusive(path: &Path) -> io::Result<File> {
        super::creating_options().open(path)
    }

    pub(super) fn held_by_another(_error: &io::Error) -> bool {
        false
    }

    pub(super) fn is_link(metadata: &Metadata) -> bool {
        metadata.file_type().is_symlink()
    }
}

#[cfg(test)]
mod tests;
