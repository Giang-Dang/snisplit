use super::*;
use std::env;
use std::process;
use std::time::Duration;

const LINE_TIME: u64 = 1_794_683_185;

fn at(seconds: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(seconds)
}

struct TempFolder(PathBuf);

impl TempFolder {
    fn new(name: &str) -> Self {
        let path = env::temp_dir().join(format!("snisplit-log-{}-{name}", process::id()));
        let _ = fs::remove_dir_all(&path);
        Self(path)
    }

    fn log(&self) -> PathBuf {
        self.0.join("logs").join("snisplit.log")
    }

    fn old_log(&self) -> PathBuf {
        self.0.join("logs").join("snisplit.old.log")
    }

    fn read(path: &Path) -> String {
        fs::read_to_string(path).expect("read log")
    }
}

impl Drop for TempFolder {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn timestamps_match_date_u_for_the_same_epoch_seconds() {
    for (seconds, expected) in [
        (0, "1970-01-01T00:00:00Z"),
        (951_782_400, "2000-02-29T00:00:00Z"),
        (951_868_799, "2000-02-29T23:59:59Z"),
        (1_794_683_185, "2026-11-14T19:06:25Z"),
        (4_107_542_399, "2100-02-28T23:59:59Z"),
        (4_107_542_400, "2100-03-01T00:00:00Z"),
        (253_402_300_799, "9999-12-31T23:59:59Z"),
    ] {
        assert_eq!(utc_timestamp(at(seconds)), expected, "{seconds}");
    }
}

#[test]
fn a_time_before_1970_is_written_as_the_epoch() {
    let before = UNIX_EPOCH - Duration::from_secs(1);
    assert_eq!(utc_timestamp(before), "1970-01-01T00:00:00Z");
}

#[test]
fn each_line_is_the_utc_time_a_space_and_the_text_in_a_new_logs_folder() {
    let temp = TempFolder::new("lines");
    let log = LogFile::open(&temp.log(), &temp.old_log(), ROTATE_AFTER).expect("open log");
    log.write_at(at(LINE_TIME), "relayed medium.com:443 in 2 pieces")
        .expect("write");
    log.write_at(at(LINE_TIME + 1), "stopping").expect("write");

    assert_eq!(
        TempFolder::read(&temp.log()),
        "2026-11-14T19:06:25Z relayed medium.com:443 in 2 pieces\n\
         2026-11-14T19:06:26Z stopping\n"
    );
}

#[test]
fn reopening_appends_after_what_is_already_in_the_file() {
    let temp = TempFolder::new("append");
    fs::create_dir_all(temp.0.join("logs")).expect("create logs");
    fs::write(temp.log(), "2026-11-14T19:00:00Z earlier\n").expect("seed log");

    let log = LogFile::open(&temp.log(), &temp.old_log(), ROTATE_AFTER).expect("open log");
    log.write_at(at(LINE_TIME), "later").expect("write");

    assert_eq!(
        TempFolder::read(&temp.log()),
        "2026-11-14T19:00:00Z earlier\n2026-11-14T19:06:25Z later\n"
    );
}

#[test]
fn a_file_already_over_the_limit_is_rotated_before_the_first_new_line() {
    let temp = TempFolder::new("reopen");
    fs::create_dir_all(temp.0.join("logs")).expect("create logs");
    fs::write(temp.log(), "x".repeat(41)).expect("seed log");

    let log = LogFile::open(&temp.log(), &temp.old_log(), 40).expect("open log");
    log.write_at(at(LINE_TIME), "line-1").expect("write");

    assert_eq!(TempFolder::read(&temp.old_log()), "x".repeat(41));
    assert_eq!(
        TempFolder::read(&temp.log()),
        "2026-11-14T19:06:25Z line-1\n"
    );
}

#[test]
fn a_file_over_the_limit_becomes_the_one_old_copy_and_a_new_file_starts() {
    let temp = TempFolder::new("rotate");
    let log = LogFile::open(&temp.log(), &temp.old_log(), 40).expect("open log");
    for number in 1..=5 {
        log.write_at(at(LINE_TIME), &format!("line-{number}"))
            .expect("write");
    }

    assert_eq!(
        TempFolder::read(&temp.old_log()),
        "2026-11-14T19:06:25Z line-3\n2026-11-14T19:06:25Z line-4\n"
    );
    assert_eq!(
        TempFolder::read(&temp.log()),
        "2026-11-14T19:06:25Z line-5\n"
    );
    let names = fs::read_dir(temp.0.join("logs"))
        .expect("list logs")
        .map(|entry| entry.expect("entry").file_name())
        .collect::<Vec<_>>();
    assert_eq!(names.len(), 2, "{names:?}");
}

#[test]
fn the_default_limit_is_ten_mebibytes() {
    assert_eq!(ROTATE_AFTER, 10_485_760);
}
