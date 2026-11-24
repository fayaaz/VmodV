//! Structured daily file logger for the VmodV shell.
//!
//! The shell has always written human-readable lines to stderr (captured by
//! `packaging/vmodv/log-run.sh` when installed). This module adds the additive
//! structured channel: every line is appended to
//! `<userData>/logs/vmodv-YYYY-MM-DD.log` in a machine-readable form:
//!
//! ```text
//! 2026-11-24T12:00:00 level=INFO target=startup message="VmodV shell starting"
//! ```
//!
//! The writer is a process-wide [`OnceLock`] around a [`DailyLog`]. Each write
//! checks the current local date and reopens the file when the day rolled
//! over, so a shell left running past midnight keeps logging without a
//! restart. Startup prunes the directory to the seven newest `vmodv-*.log`
//! files (the same retention the packaged launcher applies).

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::media_dialogs::logs_dir;

/// Daily log files kept on disk; older `vmodv-*.log` files are removed at
/// startup. Matches the launcher's seven-day retention.
pub const KEEP_DAILY_LOGS: usize = 7;

/// Prefix/suffix identifying a daily log file.
const LOG_PREFIX: &str = "vmodv-";
const LOG_SUFFIX: &str = ".log";

/// The process-wide writer, installed once by [`init`].
static LOGGER: OnceLock<Mutex<DailyLog>> = OnceLock::new();

/// A broken-down local wall-clock time.
///
/// Kept plain so [`format_line`] is deterministic and unit-testable without a
/// clock: [`DateTime::now`] is the only place that reads the system time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DateTime {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

impl DateTime {
    /// The current local time (the shell logs in the user's timezone so the
    /// daily file matches the calendar day they see).
    pub fn now() -> Self {
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs())
            .unwrap_or(0);

        #[cfg(unix)]
        {
            let raw = secs as libc::time_t;
            // SAFETY: `raw` points to a valid `time_t` (`localtime_r` only reads
            // it) and `tm` is a properly aligned, writable `libc::tm`. The
            // struct is zero-initialised (all-integer fields, so the all-zero
            // bit pattern is valid). We only read its fields when the call
            // succeeds; an out-of-range `time_t` makes it return null and we
            // fall through to the UTC civil-date path below.
            let mut tm: libc::tm = unsafe { std::mem::zeroed() };
            let ok = unsafe { libc::localtime_r(&raw, &mut tm) };
            if !ok.is_null() {
                return Self {
                    year: tm.tm_year + 1900,
                    month: (tm.tm_mon + 1) as u32,
                    day: tm.tm_mday as u32,
                    hour: tm.tm_hour as u32,
                    minute: tm.tm_min as u32,
                    second: tm.tm_sec as u32,
                };
            }
        }

        // Fallback (and the non-unix path): UTC civil time.
        Self::from_unix_utc(secs)
    }

    fn from_unix_utc(secs: u64) -> Self {
        let days = (secs / 86_400) as i64;
        let seconds_of_day = secs % 86_400;
        let (year, month, day) = civil_from_days(days);
        Self {
            year,
            month,
            day,
            hour: (seconds_of_day / 3600) as u32,
            minute: ((seconds_of_day % 3600) / 60) as u32,
            second: (seconds_of_day % 60) as u32,
        }
    }

    /// `YYYY-MM-DD`, used for the file name.
    pub fn date(&self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }

    /// `YYYY-MM-DDTHH:MM:SS`, the leading field of every line.
    pub fn timestamp(&self) -> String {
        format!(
            "{}T{:02}:{:02}:{:02}",
            self.date(),
            self.hour,
            self.minute,
            self.second
        )
    }
}

/// Civil date for a day count since the Unix epoch (Howard Hinnant's
/// `civil_from_days`, the inverse of `days_from_civil`).
fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if month <= 2 { year + 1 } else { year } as i32, month, day)
}

/// One structured log line, escaped so it stays on a single line.
pub fn format_line(at: DateTime, level: &str, target: &str, message: &str) -> String {
    let escaped = message
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\r', "")
        .replace('\n', "\\n");
    format!(
        "{} level={} target={} message=\"{}\"",
        at.timestamp(),
        level,
        target,
        escaped
    )
}

/// `<dir>/vmodv-<date>.log`, where `dir` is a logs directory.
fn daily_log_file(dir: &Path, date: &str) -> PathBuf {
    dir.join(format!("{LOG_PREFIX}{date}{LOG_SUFFIX}"))
}

/// `<userData>/logs/vmodv-<date>.log`.
pub fn daily_log_path(user_data: &Path, date: &str) -> PathBuf {
    daily_log_file(&logs_dir(user_data), date)
}

/// Delete all but the `keep` newest `vmodv-*.log` files in `dir`, returning
/// how many were removed. Names embed `YYYY-MM-DD`, so a lexical sort is
/// chronological. Missing directories are treated as empty.
pub fn prune(dir: &Path, keep: usize) -> std::io::Result<usize> {
    let read = match fs::read_dir(dir) {
        Ok(read) => read,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error),
    };

    let mut names: Vec<String> = read
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_type()
                .map(|kind| kind.is_file())
                .unwrap_or(false)
        })
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with(LOG_PREFIX) && name.ends_with(LOG_SUFFIX))
        .collect();
    names.sort();

    let mut removed = 0;
    let excess = names.len().saturating_sub(keep);
    for name in names.into_iter().take(excess) {
        match fs::remove_file(dir.join(&name)) {
            Ok(()) => removed += 1,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                eprintln!("ui: WARN could not prune the daily log {name}: {error}");
            }
        }
    }
    Ok(removed)
}

/// The open file (if any) plus the date it belongs to.
struct DailyLog {
    dir: PathBuf,
    date: String,
    file: Option<File>,
}

impl DailyLog {
    fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            date: String::new(),
            file: None,
        }
    }

    /// Open (creating the directory if needed) `<dir>/vmodv-<date>.log`.
    fn open(&mut self, date: &str) {
        if let Err(error) = fs::create_dir_all(&self.dir) {
            eprintln!(
                "ui: WARN could not create the log directory {}: {error}",
                self.dir.display()
            );
        }
        let path = daily_log_file(&self.dir, date);
        self.date = date.to_string();
        self.file = match OpenOptions::new().create(true).append(true).open(&path) {
            Ok(file) => Some(file),
            Err(error) => {
                eprintln!(
                    "ui: WARN could not open the daily log {}: {error}",
                    path.display()
                );
                None
            }
        };
    }

    /// Reopen the file when the local date changed (daily rollover).
    fn ensure_open(&mut self) {
        let today = DateTime::now().date();
        if self.file.is_none() || self.date != today {
            self.open(&today);
        }
    }

    fn write_line(&mut self, level: &str, target: &str, message: &str) {
        self.ensure_open();
        if let Some(file) = &mut self.file {
            // Flush every line: the e2e harness and a crashed shell must see
            // the startup line without a graceful shutdown.
            let _ = writeln!(
                file,
                "{}",
                format_line(DateTime::now(), level, target, message)
            );
            let _ = file.flush();
        }
    }

    fn flush(&mut self) {
        if let Some(file) = &mut self.file {
            let _ = file.flush();
        }
    }
}

fn lock(logger: &Mutex<DailyLog>) -> MutexGuard<'_, DailyLog> {
    logger
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Install the writer for `user_data`, prune old daily logs and record a
/// startup line.
///
/// Called as early as possible in `VmodvApp::new`. Repeated calls append to the
/// already-installed writer rather than reopening the file.
pub fn init(user_data: &Path) {
    let dir = logs_dir(user_data);
    if let Err(error) = fs::create_dir_all(&dir) {
        eprintln!(
            "ui: WARN could not create the log directory {}: {error}",
            dir.display()
        );
    }
    if let Err(error) = prune(&dir, KEEP_DAILY_LOGS) {
        eprintln!("ui: WARN could not prune the daily logs: {error}");
    }

    let logger = LOGGER.get_or_init(|| Mutex::new(DailyLog::new(dir)));
    let mut log = lock(logger);
    let message = format!(
        "VmodV shell starting version={} build={} user_data={}",
        env!("CARGO_PKG_VERSION"),
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
        user_data.display()
    );
    // `write_line` flushes each line, so the startup record is already on disk
    // before the control socket can answer a harness ping.
    log.write_line("INFO", "startup", &message);
}

/// Append one structured line to the daily log (a no-op before [`init`]).
pub fn log(level: &str, target: &str, message: &str) {
    let Some(logger) = LOGGER.get() else {
        return;
    };
    lock(logger).write_line(level, target, message);
}

/// [`log`] at `INFO`.
pub fn info(target: &str, message: &str) {
    log("INFO", target, message);
}

/// [`log`] at `WARN`.
pub fn warn(target: &str, message: &str) {
    log("WARN", target, message);
}

/// [`log`] at `ERROR`.
pub fn error(target: &str, message: &str) {
    log("ERROR", target, message);
}

/// Flush the installed writer (a no-op before [`init`]).
pub fn flush() {
    if let Some(logger) = LOGGER.get() {
        lock(logger).flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at() -> DateTime {
        DateTime {
            year: 2026,
            month: 11,
            day: 24,
            hour: 12,
            minute: 0,
            second: 5,
        }
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("modv-logging-{tag}-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn timestamp_and_format_line_are_structured() {
        assert_eq!(at().date(), "2026-11-24");
        assert_eq!(at().timestamp(), "2026-11-24T12:00:05");
        assert_eq!(
            format_line(at(), "INFO", "startup", "VmodV shell starting"),
            "2026-11-24T12:00:05 level=INFO target=startup message=\"VmodV shell starting\""
        );
    }

    #[test]
    fn format_line_escapes_quotes_and_newlines() {
        let line = format_line(at(), "WARN", "ui", "a \"b\"\nc");
        assert_eq!(
            line,
            "2026-11-24T12:00:05 level=WARN target=ui message=\"a \\\"b\\\"\\nc\""
        );
        assert_eq!(line.lines().count(), 1);
    }

    #[test]
    fn daily_log_path_lives_under_the_logs_directory() {
        assert_eq!(
            daily_log_path(Path::new("/tmp/vmodv"), "2026-11-24"),
            Path::new("/tmp/vmodv/logs/vmodv-2026-11-24.log")
        );
    }

    #[test]
    fn prune_keeps_the_seven_newest_daily_logs() {
        let dir = temp_dir("prune");
        for day in 1..=10 {
            fs::write(dir.join(format!("vmodv-2026-11-{day:02}.log")), "x").unwrap();
        }
        // Unrelated files are never touched.
        fs::write(dir.join("other.log"), "x").unwrap();

        let removed = prune(&dir, KEEP_DAILY_LOGS).unwrap();
        assert_eq!(removed, 3);

        let mut names: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(
            names,
            [
                "other.log",
                "vmodv-2026-11-04.log",
                "vmodv-2026-11-05.log",
                "vmodv-2026-11-06.log",
                "vmodv-2026-11-07.log",
                "vmodv-2026-11-08.log",
                "vmodv-2026-11-09.log",
                "vmodv-2026-11-10.log",
            ]
        );

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn prune_treats_a_missing_directory_as_empty() {
        let dir = temp_dir("missing").join("logs");
        assert_eq!(prune(&dir, KEEP_DAILY_LOGS).unwrap(), 0);
        fs::remove_dir_all(dir.parent().unwrap()).unwrap();
    }

    #[test]
    fn write_creates_the_daily_file_and_rolls_over() {
        let dir = temp_dir("rollover");
        let mut log = DailyLog::new(dir.clone());
        log.write_line("INFO", "startup", "hello");

        let today = DateTime::now().date();
        assert_eq!(log.date, today);
        assert!(dir.join(format!("vmodv-{today}.log")).is_file());

        // A stale date forces the writer to reopen the current day's file.
        log.date = "1999-01-01".to_string();
        log.write_line("INFO", "startup", "again");
        assert_eq!(log.date, today);

        let text = fs::read_to_string(dir.join(format!("vmodv-{today}.log"))).unwrap();
        assert_eq!(text.lines().count(), 2);

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn utc_civil_fallback_matches_known_instants() {
        // This pure-UTC path is taken off-unix or when `localtime_r` rejects a
        // time; it must still produce the right calendar date (including
        // leap days) rather than drifting.
        assert_eq!(
            DateTime::from_unix_utc(0).timestamp(),
            "1970-01-01T00:00:00"
        );
        assert_eq!(
            DateTime::from_unix_utc(951_782_400).timestamp(),
            "2000-02-29T00:00:00"
        );
        assert_eq!(
            DateTime::from_unix_utc(1_795_521_605).timestamp(),
            "2026-11-24T12:00:05"
        );
    }
}
