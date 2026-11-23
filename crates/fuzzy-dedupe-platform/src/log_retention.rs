// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! Age-based expiry of the application's own log files (C-07, plan D-015;
//! NFR-009 "kept for 7 days", spec §19 MS-1 exit item 8 "rotating its own
//! logs is allowed").
//!
//! This is the only module in the workspace allowed to delete (C-09
//! allowlists `std::fs::remove_file` here and nothing else), so its scope is
//! deliberately narrow. [`expire_logs`] deletes an entry only when all of
//! these hold:
//!
//! - it is directly inside [`AppDirs::log_dir`], which the function takes
//!   from the resolved directories rather than as a free path, so no caller
//!   can aim it elsewhere, and that `logs` directory is itself a real
//!   directory, not a symbolic link;
//! - it is a regular file by its own file type, read without following a
//!   symbolic link, so a link is kept and its target never touched;
//! - its name is exactly `fuzzy-dedupe.YYYY-MM-DD.log` with a real calendar
//!   date ([`log_file_name`]);
//! - that date is more than [`RETENTION_DAYS`] days before the injected
//!   current date. A log dated exactly 7 days earlier is kept.
//!
//! Directories, links, unrelated and non-matching names, unparseable dates
//! and future dates are all kept.
//!
//! Confinement policy (owner decision 2026-11-23, "trust the root, no links
//! below"): the state root `<state>/fuzzy-dedupe` and the XDG directories
//! above it are trusted and may be links. Below the root, `logs` and each
//! entry are inspected with `symlink_metadata` (the entry, never what it
//! points to): `logs` once before listing and again immediately before
//! every deletion, and the entry immediately before its own deletion. A
//! `logs` that is a link or not a directory stops the pass with
//! [`ExpiryError::UnsafeLogDir`]; following it would aim the deletions at
//! whatever directory the link names.
//!
//! Threat model: the checks stop links that are already in place. Another
//! process running as the same user could still swap `logs` or an entry
//! between a check and the deletion; that is out of scope, because such a
//! process can already delete these files, and the user's own files,
//! directly. Closing the race would need handle-anchored deletion
//! (`unlinkat` on an opened directory), which `std` lacks and no approved
//! spec §8.3 crate provides. The current date is a parameter so tests
//! inject the clock; the app calls this at startup and at each daily
//! rollover, outside the UI pass (T12).
//!
//! Cross-file contract with the app's logging setup (T12): the roller must
//! be `tracing-appender`'s daily rotation with prefix [`LOG_FILE_PREFIX`]
//! and suffix [`LOG_FILE_SUFFIX`], which names files
//! `<prefix>.<YYYY-MM-DD>.<suffix>` in UTC; the app passes
//! [`LogDate::today_utc`] so both sides count days in the same time base.
//! A roller configured with any other prefix, suffix or format makes every
//! log "non-matching", so logs would silently never expire.

use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::dirs::AppDirs;

/// The roller's file-name prefix; see the module contract.
pub const LOG_FILE_PREFIX: &str = "fuzzy-dedupe";
/// The roller's file-name suffix; see the module contract.
pub const LOG_FILE_SUFFIX: &str = "log";
/// Logs dated more than this many days before today are deleted.
pub const RETENTION_DAYS: i64 = 7;

/// A proleptic Gregorian calendar date, the date part of a log file name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LogDate {
    year: i32,
    month: u8,
    day: u8,
}

impl LogDate {
    /// The date, or `None` when it does not exist (month 13, April 31, a
    /// February 29 outside a leap year) or the year is outside 0..=9999,
    /// the four-digit range a log file name can carry.
    #[must_use]
    pub fn new(year: i32, month: u8, day: u8) -> Option<Self> {
        let valid = (0..=9999).contains(&year)
            && (1..=12).contains(&month)
            && (1..=days_in_month(year, month)).contains(&day);
        valid.then_some(Self { year, month, day })
    }

    /// The UTC calendar date of `time`; times before 1970 clamp to
    /// 1970-01-01.
    #[must_use]
    pub fn from_system_time(time: SystemTime) -> Self {
        let secs = time
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        // u64 seconds / 86 400 fits i64 for any representable SystemTime.
        let days = i64::try_from(secs / 86_400).unwrap_or(i64::MAX);
        Self::from_days(days)
    }

    /// Today's UTC date, the time base `tracing-appender`'s roller names
    /// files in.
    #[must_use]
    pub fn today_utc() -> Self {
        Self::from_system_time(SystemTime::now())
    }

    #[must_use]
    pub fn year(self) -> i32 {
        self.year
    }

    #[must_use]
    pub fn month(self) -> u8 {
        self.month
    }

    #[must_use]
    pub fn day(self) -> u8 {
        self.day
    }

    /// Parse exactly `YYYY-MM-DD`, digits only.
    fn parse(text: &str) -> Option<Self> {
        let b = text.as_bytes();
        let digits = |r: std::ops::Range<usize>| -> Option<u32> {
            b.get(r.clone())?
                .iter()
                .all(u8::is_ascii_digit)
                .then(|| text[r].parse().ok())
                .flatten()
        };
        if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
            return None;
        }
        Self::new(
            i32::try_from(digits(0..4)?).ok()?,
            u8::try_from(digits(5..7)?).ok()?,
            u8::try_from(digits(8..10)?).ok()?,
        )
    }

    /// Days since 1970-01-01 (Howard Hinnant's `days_from_civil`).
    fn to_days(self) -> i64 {
        let (m, d) = (i64::from(self.month), i64::from(self.day));
        let y = i64::from(self.year) - i64::from(m <= 2);
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }

    /// The date `days` after 1970-01-01 (`civil_from_days`), clamped to the
    /// 0..=9999 year range [`LogDate::new`] accepts.
    fn from_days(days: i64) -> Self {
        let max = Self {
            year: 9999,
            month: 12,
            day: 31,
        };
        if days >= max.to_days() {
            return max;
        }
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = doy - (153 * mp + 2) / 5 + 1;
        let month = if mp < 10 { mp + 3 } else { mp - 9 };
        let year = yoe + era * 400 + i64::from(month <= 2);
        // Every component is in range by construction for days >= 0 and
        // below `max`; the fallbacks are unreachable.
        Self {
            year: i32::try_from(year).unwrap_or(1970),
            month: u8::try_from(month).unwrap_or(1),
            day: u8::try_from(day).unwrap_or(1),
        }
    }
}

impl std::fmt::Display for LogDate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

fn days_in_month(year: i32, month: u8) -> u8 {
    match month {
        4 | 6 | 9 | 11 => 30,
        2 if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        2 => 28,
        _ => 31,
    }
}

/// The name the roller gives the log file for `date`:
/// `fuzzy-dedupe.YYYY-MM-DD.log`.
#[must_use]
pub fn log_file_name(date: LogDate) -> String {
    format!("{LOG_FILE_PREFIX}.{date}.{LOG_FILE_SUFFIX}")
}

/// The date in an app log file name, or `None` for any other name.
fn log_file_date(name: &str) -> Option<LogDate> {
    let date = name
        .strip_prefix(LOG_FILE_PREFIX)?
        .strip_prefix('.')?
        .strip_suffix(LOG_FILE_SUFFIX)?
        .strip_suffix('.')?;
    LogDate::parse(date)
}

/// What one [`expire_logs`] pass did.
#[derive(Debug, Default)]
pub struct ExpiryReport {
    /// The file names deleted, sorted.
    pub deleted: Vec<String>,
    /// Expired files that could not be deleted, with the reason; each is
    /// retried on the next pass.
    pub failed: Vec<(String, std::io::Error)>,
}

/// Why an [`expire_logs`] pass stopped.
#[derive(Debug)]
pub enum ExpiryError {
    /// The log directory is a symbolic link or not a directory. Nothing in
    /// the pass was deleted after this was seen.
    UnsafeLogDir(PathBuf),
    /// The log directory exists but could not be inspected or listed.
    Unreadable { path: PathBuf, source: io::Error },
}

impl std::fmt::Display for ExpiryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsafeLogDir(p) => write!(
                f,
                "{} is a link or not a folder, so no logs were expired",
                p.display()
            ),
            Self::Unreadable { path, source } => write!(f, "{}: {source}", path.display()),
        }
    }
}

impl std::error::Error for ExpiryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Unreadable { source, .. } => Some(source),
            Self::UnsafeLogDir(_) => None,
        }
    }
}

/// `Ok(true)` when `dir` is a real directory, `Ok(false)` when it does not
/// exist, judged without following a link.
fn log_dir_state(dir: &Path) -> Result<bool, ExpiryError> {
    match std::fs::symlink_metadata(dir) {
        Ok(meta) if meta.file_type().is_dir() => Ok(true),
        Ok(_) => Err(ExpiryError::UnsafeLogDir(dir.to_path_buf())),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(ExpiryError::Unreadable {
            path: dir.to_path_buf(),
            source,
        }),
    }
}

/// Delete the application's expired log files from `dirs`' log directory;
/// see the module documentation for exactly which entries qualify.
///
/// One undeletable or unreadable entry does not stop the pass: per-file
/// deletion failures are collected in [`ExpiryReport::failed`] and entries
/// that cannot be inspected are kept. A log directory that does not exist
/// yet (first run) yields an empty report.
///
/// # Errors
///
/// [`ExpiryError::UnsafeLogDir`] when the log directory is a link or not a
/// directory, before listing or before any deletion; files already deleted
/// earlier in the same pass were genuine expired logs and stay deleted.
/// [`ExpiryError::Unreadable`] when it cannot be inspected or listed.
#[expect(
    clippy::disallowed_methods,
    reason = "log expiry: removes only the app's own expired daily log files in the adapter-resolved log directory (C-09 allowlist, D-015)"
)]
pub fn expire_logs(dirs: &AppDirs, today: LogDate) -> Result<ExpiryReport, ExpiryError> {
    let dir: &Path = dirs.log_dir();
    if !log_dir_state(dir)? {
        return Ok(ExpiryReport::default());
    }
    let entries = std::fs::read_dir(dir).map_err(|source| ExpiryError::Unreadable {
        path: dir.to_path_buf(),
        source,
    })?;
    let cutoff = today.to_days() - RETENTION_DAYS;
    let mut report = ExpiryReport::default();
    for entry in entries.flatten() {
        // `DirEntry::file_type` does not follow symbolic links, so a link
        // named like a log reports as a link and is kept.
        if !entry.file_type().is_ok_and(|t| t.is_file()) {
            continue;
        }
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let Some(date) = log_file_date(&name) else {
            continue;
        };
        if date.to_days() >= cutoff {
            continue;
        }
        // Immediately before the deletion: `logs` is still a real
        // directory, and the entry, addressed by the exact name that matched
        // the pattern, is still a regular file. `remove_file` unlinks the
        // entry itself and never follows a final link, so the most a lost
        // race (out of scope, see the module header) can remove is a link.
        if !log_dir_state(dir)? {
            break;
        }
        let path = dir.join(&name);
        if !std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_file()) {
            continue;
        }
        match std::fs::remove_file(&path) {
            Ok(()) => report.deleted.push(name),
            Err(e) => report.failed.push((name, e)),
        }
    }
    report.deleted.sort();
    report.failed.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(report)
}
