use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const PROGRAM_FILES: &str = "ProgramFiles";
const FOLDER: &str = "snisplit";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    root: PathBuf,
}

impl Layout {
    pub fn under(program_files: &Path) -> Self {
        Self {
            root: program_files.join(FOLDER),
        }
    }

    pub fn from_environment() -> io::Result<Self> {
        Self::from_program_files(env::var_os(PROGRAM_FILES).map(PathBuf::from))
    }

    fn from_program_files(program_files: Option<PathBuf>) -> io::Result<Self> {
        match program_files {
            Some(folder) if folder.is_absolute() => Ok(Self::under(&folder)),
            Some(folder) => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "%ProgramFiles% is not an absolute path: {:?}",
                    folder.display().to_string()
                ),
            )),
            None => Err(io::Error::new(
                io::ErrorKind::NotFound,
                "%ProgramFiles% is not set",
            )),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn executable(&self) -> PathBuf {
        self.root.join("snisplit.exe")
    }

    pub fn profiles(&self) -> PathBuf {
        self.root.join("profiles")
    }

    pub fn profile(&self, number: usize) -> PathBuf {
        self.profiles().join(format!("{number}.profile"))
    }

    pub fn logs(&self) -> PathBuf {
        self.root.join("logs")
    }

    pub fn log_file(&self) -> PathBuf {
        self.logs().join("snisplit.log")
    }

    pub fn old_log_file(&self) -> PathBuf {
        self.logs().join("snisplit.old.log")
    }
}

pub fn is_same_file(first: &Path, second: &Path) -> bool {
    match (fs::canonicalize(first), fs::canonicalize(second)) {
        (Ok(first), Ok(second)) => first == second,
        _ => false,
    }
}

pub fn is_inside(file: &Path, folder: &Path) -> bool {
    match (fs::canonicalize(file), fs::canonicalize(folder)) {
        (Ok(file), Ok(folder)) => file.starts_with(folder),
        _ => false,
    }
}

#[cfg(test)]
mod tests;
