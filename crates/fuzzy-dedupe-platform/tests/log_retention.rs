// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! PV-T10-002 (plan T10, D-015): age-based expiry of the application's own
//! log files, with the current date injected.
//!
//! Every test sits in the `log_retention` module so
//! `-E 'test(log_retention)'` selects them all.
//!
//! Fixture directories live under `CARGO_TARGET_TMPDIR` with a per-run unique
//! name: the C-09 guard bars delete calls in test files, so nothing here can
//! clean up, and a fixed name would let one run's survivors leak into the
//! next. Symbolic-link entries are not exercised, because the guard also
//! bars creating links in a test file; `expire_logs` reads each entry's own
//! file type without following it, which the unit-level reasoning in
//! `log_retention.rs` records.

mod log_retention {
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::{Duration, SystemTime};

    use fuzzy_dedupe_platform::dirs::AppDirs;
    use fuzzy_dedupe_platform::log_retention::{ExpiryError, LogDate, expire_logs, log_file_name};

    fn date(y: i32, m: u8, d: u8) -> LogDate {
        LogDate::new(y, m, d).expect("valid fixture date")
    }

    /// Fresh `AppDirs` whose state directory is a new, empty directory
    /// under `CARGO_TARGET_TMPDIR`.
    fn fixture_dirs(tag: &str) -> AppDirs {
        static SEQ: AtomicU32 = AtomicU32::new(0);
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("clock after 1970")
            .as_nanos();
        let state = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
            "log-retention-{tag}-{}-{nanos}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        AppDirs::resolve(
            move |name| (name == "XDG_STATE_HOME").then(|| state.clone().into_os_string()),
            Path::new("/home/tester"),
        )
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "log fixtures under CARGO_TARGET_TMPDIR (C-09 test allowance)"
    )]
    fn populate(log_dir: &Path, files: &[&str], dirs: &[&str]) {
        assert!(log_dir.starts_with(env!("CARGO_TARGET_TMPDIR")));
        std::fs::create_dir_all(log_dir).expect("create log dir");
        for name in files {
            std::fs::write(log_dir.join(name), b"event\n").expect("write fixture");
        }
        for name in dirs {
            std::fs::create_dir(log_dir.join(name)).expect("create fixture dir");
        }
    }

    fn listing(dir: &Path) -> BTreeSet<String> {
        std::fs::read_dir(dir)
            .expect("read log dir")
            .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
            .collect()
    }

    fn names(list: &[&str]) -> BTreeSet<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn file_names_follow_the_roller_pattern() {
        assert_eq!(
            log_file_name(date(2026, 11, 23)),
            "fuzzy-dedupe.2026-11-23.log"
        );
        assert_eq!(
            log_file_name(date(987, 1, 2)),
            "fuzzy-dedupe.0987-01-02.log"
        );
    }

    #[test]
    fn expires_logs_older_than_seven_days_and_rolls_over() {
        let dirs = fixture_dirs("weeks");
        let log_dir: PathBuf = dirs.log_dir().to_path_buf();
        // Six app logs (fewer than seven) over four weeks: a count cap of
        // seven would delete none of them, which is the D-015 failure mode.
        let app_logs = [
            "fuzzy-dedupe.2026-10-26.log", // 28 days before today
            "fuzzy-dedupe.2026-11-02.log", // 21 days
            "fuzzy-dedupe.2026-11-15.log", // 8 days
            "fuzzy-dedupe.2026-11-16.log", // exactly 7 days: kept
            "fuzzy-dedupe.2026-11-20.log", // 3 days
            "fuzzy-dedupe.2026-11-23.log", // today
        ];
        let others = [
            "notes.txt",                     // unrelated file
            "other-app.2026-10-01.log",      // non-matching name
            "fuzzy-dedupe.2026-02-30.log",   // matching name, impossible date
            "fuzzy-dedupe.not-a-date.log",   // matching name, unparseable date
            "fuzzy-dedupe.2026-10-01.log.1", // suffix does not match
            "fuzzy-dedupe.2026-10-01",       // no suffix
        ];
        let old_directory = "fuzzy-dedupe.2026-09-01.log";
        let files: Vec<&str> = app_logs.iter().chain(&others).copied().collect();
        populate(&log_dir, &files, &[old_directory]);

        let deleted = expire_logs(&dirs, date(2026, 11, 23)).expect("expiry runs");
        assert!(deleted.failed.is_empty(), "{:?}", deleted.failed);
        assert_eq!(
            deleted.deleted,
            [
                "fuzzy-dedupe.2026-10-26.log",
                "fuzzy-dedupe.2026-11-02.log",
                "fuzzy-dedupe.2026-11-15.log",
            ]
        );
        let mut kept = names(&app_logs[3..]);
        kept.extend(names(&others));
        kept.insert(old_directory.to_owned());
        assert_eq!(listing(&log_dir), kept);

        // Daily rollover: one day later the 7-day-old log crosses the
        // boundary and is the only file removed.
        let next = expire_logs(&dirs, date(2026, 11, 24)).expect("rollover expiry");
        assert_eq!(next.deleted, ["fuzzy-dedupe.2026-11-16.log"]);
        kept.remove("fuzzy-dedupe.2026-11-16.log");
        assert_eq!(listing(&log_dir), kept);
    }

    #[test]
    fn age_counts_calendar_days_across_month_and_year_ends() {
        let dirs = fixture_dirs("year-end");
        populate(
            dirs.log_dir(),
            &[
                "fuzzy-dedupe.2026-12-26.log", // 8 days before 2027-01-03
                "fuzzy-dedupe.2026-12-27.log", // exactly 7
                "fuzzy-dedupe.2027-01-10.log", // in the future: kept
            ],
            &[],
        );
        let report = expire_logs(&dirs, date(2027, 1, 3)).expect("expiry runs");
        assert_eq!(report.deleted, ["fuzzy-dedupe.2026-12-26.log"]);
        assert_eq!(
            listing(dirs.log_dir()),
            names(&["fuzzy-dedupe.2026-12-27.log", "fuzzy-dedupe.2027-01-10.log"])
        );
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "log fixtures under CARGO_TARGET_TMPDIR (C-09 test allowance)"
    )]
    fn write_file(path: &Path) {
        assert!(path.starts_with(env!("CARGO_TARGET_TMPDIR")));
        std::fs::create_dir_all(path.parent().expect("parent")).expect("create parent");
        std::fs::write(path, b"not a directory\n").expect("write fixture");
    }

    #[test]
    fn a_log_path_that_is_not_a_real_directory_deletes_nothing() {
        // A regular file where `logs` should be takes the same no-follow
        // refusal as a symbolic link to a directory would; a test file may
        // not create links (C-09), so the link case itself is not exercised.
        let dirs = fixture_dirs("not-a-dir");
        write_file(dirs.log_dir());
        let err = expire_logs(&dirs, date(2026, 11, 23)).expect_err("refused");
        assert!(
            matches!(err, ExpiryError::UnsafeLogDir(ref p) if p == dirs.log_dir()),
            "{err:?}"
        );
        assert_eq!(
            std::fs::read(dirs.log_dir()).expect("still there"),
            b"not a directory\n"
        );
    }

    // Committed symbolic-link fixtures: see adapters.rs and
    // `crates/fuzzy-dedupe-platform/fixtures/README.md`.

    fn fixture(rel: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures")
            .join(rel)
    }

    #[track_caller]
    fn assert_is_link(path: &Path) {
        let meta = std::fs::symlink_metadata(path)
            .unwrap_or_else(|e| panic!("fixture {} is missing: {e}", path.display()));
        assert!(
            meta.file_type().is_symlink(),
            "fixture {} must be a symbolic link; was the checkout dereferenced?",
            path.display()
        );
    }

    fn state_dirs(base: &str) -> AppDirs {
        let state = fixture(base);
        AppDirs::resolve(
            move |name| (name == "XDG_STATE_HOME").then(|| state.clone().into_os_string()),
            Path::new("/home/tester"),
        )
    }

    #[test]
    fn a_symlinked_log_directory_deletes_nothing() {
        let dirs = state_dirs("linked-logs/state");
        assert_is_link(dirs.log_dir());
        let decoy = fixture("linked-logs/decoy-logs/fuzzy-dedupe.2026-01-01.log");
        assert!(decoy.is_file(), "decoy log present before the pass");

        let err = expire_logs(&dirs, date(2026, 11, 23)).expect_err("refused");
        assert!(
            matches!(err, ExpiryError::UnsafeLogDir(ref p) if p == dirs.log_dir()),
            "{err:?}"
        );
        assert!(decoy.is_file(), "the decoy log behind the link survives");
        assert_is_link(dirs.log_dir());
    }

    #[test]
    fn a_symlinked_log_entry_is_skipped() {
        let dirs = state_dirs("linked-entry/state");
        let entry = dirs.log_dir().join("fuzzy-dedupe.2026-01-01.log");
        assert_is_link(&entry);
        let decoy = fixture("linked-entry/decoy-file.log");
        assert!(decoy.is_file(), "decoy target present before the pass");

        let report = expire_logs(&dirs, date(2026, 11, 23)).expect("expiry runs");
        assert!(report.deleted.is_empty(), "{:?}", report.deleted);
        assert!(report.failed.is_empty(), "{:?}", report.failed);
        assert!(decoy.is_file(), "the link target survives");
        assert_is_link(&entry);
    }

    #[test]
    fn a_missing_log_directory_is_not_an_error() {
        let dirs = fixture_dirs("missing");
        let report = expire_logs(&dirs, date(2026, 11, 23)).expect("first run");
        assert!(report.deleted.is_empty());
        assert!(!dirs.log_dir().exists());
    }

    #[test]
    fn dates_are_validated_and_derived_in_utc() {
        assert!(LogDate::new(2028, 2, 29).is_some(), "leap day");
        assert!(LogDate::new(2026, 2, 29).is_none());
        assert!(LogDate::new(2100, 2, 29).is_none(), "century non-leap");
        assert!(LogDate::new(2000, 2, 29).is_some(), "400-year leap");
        assert!(LogDate::new(2026, 13, 1).is_none());
        assert!(LogDate::new(2026, 4, 31).is_none());
        assert!(LogDate::new(2026, 1, 0).is_none());

        assert_eq!(
            LogDate::from_system_time(SystemTime::UNIX_EPOCH),
            date(1970, 1, 1)
        );
        // 2026-11-23T23:59:59Z, one second before the UTC rollover.
        let late = SystemTime::UNIX_EPOCH + Duration::from_secs(1_795_478_399);
        assert_eq!(LogDate::from_system_time(late), date(2026, 11, 23));
        assert_eq!(
            LogDate::from_system_time(late + Duration::from_secs(1)),
            date(2026, 11, 24)
        );
    }
}
