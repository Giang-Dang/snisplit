use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::session::Output;

pub const ROTATE_AFTER: u64 = 10 * 1024 * 1024;
const SECONDS_PER_DAY: u64 = 86_400;
const DAYS_FROM_MARCH_OF_YEAR_0_TO_1970: u64 = 719_468;
const DAYS_PER_400_YEARS: u64 = 146_097;

pub struct LogFile {
    path: PathBuf,
    old_path: PathBuf,
    rotate_after: u64,
    open: Mutex<OpenLog>,
}

struct OpenLog {
    file: Option<File>,
    size: u64,
}

impl LogFile {
    pub fn open(path: &Path, old_path: &Path, rotate_after: u64) -> io::Result<Self> {
        if let Some(folder) = path.parent() {
            fs::create_dir_all(folder)?;
        }
        let file = append_to(path)?;
        let size = file.metadata()?.len();
        Ok(Self {
            path: path.to_path_buf(),
            old_path: old_path.to_path_buf(),
            rotate_after,
            open: Mutex::new(OpenLog {
                file: Some(file),
                size,
            }),
        })
    }

    pub fn into_output(self) -> Output {
        let log = Arc::new(self);
        Arc::new(move |text| {
            let _ = log.write_at(SystemTime::now(), text);
        })
    }

    pub fn write_at(&self, time: SystemTime, text: &str) -> io::Result<()> {
        let line = format!("{} {text}\n", utc_timestamp(time));
        let mut open = self.open.lock().unwrap_or_else(PoisonError::into_inner);
        if open.size > self.rotate_after {
            open.file = None;
            let _ = fs::rename(&self.path, &self.old_path);
        }
        let file = match open.file.take() {
            Some(file) => file,
            None => {
                let file = append_to(&self.path)?;
                open.size = file.metadata()?.len();
                file
            }
        };
        let file = open.file.insert(file);
        file.write_all(line.as_bytes())?;
        file.flush()?;
        open.size += line.len() as u64;
        Ok(())
    }
}

fn append_to(path: &Path) -> io::Result<File> {
    OpenOptions::new().create(true).append(true).open(path)
}

pub fn utc_timestamp(time: SystemTime) -> String {
    let seconds = time
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let (year, month, day) = civil_date(seconds / SECONDS_PER_DAY);
    let second_of_day = seconds % SECONDS_PER_DAY;
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        second_of_day / 3600,
        second_of_day / 60 % 60,
        second_of_day % 60
    )
}

fn civil_date(days_since_1970: u64) -> (u64, u64, u64) {
    let days = days_since_1970 + DAYS_FROM_MARCH_OF_YEAR_0_TO_1970;
    let era = days / DAYS_PER_400_YEARS;
    let day_of_era = days % DAYS_PER_400_YEARS;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_from_march = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_from_march + 2) / 5 + 1;
    let month = if month_from_march < 10 {
        month_from_march + 3
    } else {
        month_from_march - 9
    };
    let year = year_of_era + era * 400 + u64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests;
