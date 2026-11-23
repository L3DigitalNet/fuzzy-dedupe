// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! PV-T10-001 (plan T10): the per-user directory resolver, the app-owned
//! writer, open-location validation and the non-blocking picker handle.
//!
//! Every test sits in the `adapters` module so `-E 'test(adapters)'` selects
//! them all.
//!
//! The environment is injected as a map, never set on the process:
//! `std::env::set_var` is `unsafe` in edition 2024 and would race the other
//! tests nextest runs in the same binary. Fixture directories live under
//! `CARGO_TARGET_TMPDIR` with a per-run unique name, because the C-09 guard
//! bars any delete call in a test file, so a fixture cannot be cleaned up and
//! a fixed name would carry state between runs.

mod adapters {
    use std::collections::HashMap;
    use std::ffi::OsString;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::sync::{Arc, mpsc};
    use std::time::{Duration, Instant, SystemTime};

    use fuzzy_dedupe_platform::app_files::{AppFileError, AppFiles};
    use fuzzy_dedupe_platform::dirs::AppDirs;
    use fuzzy_dedupe_platform::open::{
        Location, OpenError, OpenHandle, Opener, SystemOpener, check,
    };
    use fuzzy_dedupe_platform::picker::{PickError, PickHandle, PickOutcome};

    fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let map: HashMap<String, OsString> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), OsString::from(v)))
            .collect();
        move |name| map.get(name).cloned()
    }

    const HOME: &str = "/home/tester";

    /// A new, empty directory under `CARGO_TARGET_TMPDIR`, unique to this run
    /// and call.
    #[expect(
        clippy::disallowed_methods,
        reason = "fixture directory under CARGO_TARGET_TMPDIR (C-09 test allowance)"
    )]
    fn fresh_dir(tag: &str) -> PathBuf {
        static SEQ: AtomicU32 = AtomicU32::new(0);
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("clock after 1970")
            .as_nanos();
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
            "adapters-{tag}-{}-{nanos}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).expect("create fixture directory");
        dir
    }

    /// Directories rooted in a fresh fixture: config under `<root>/config`,
    /// state under `<root>/state`.
    fn fixture_dirs(tag: &str) -> (PathBuf, AppDirs) {
        let root = fresh_dir(tag);
        let config = root.join("config");
        let state = root.join("state");
        let env = move |name: &str| match name {
            "XDG_CONFIG_HOME" => Some(config.clone().into_os_string()),
            "XDG_STATE_HOME" => Some(state.clone().into_os_string()),
            _ => None,
        };
        let dirs = AppDirs::resolve(env, Path::new(HOME));
        (root, dirs)
    }

    fn listing(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .expect("read fixture directory")
            .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    // ── Directory resolver ────────────────────────────────────────────────

    #[test]
    fn absolute_xdg_values_are_used() {
        let dirs = AppDirs::resolve(
            env_of(&[("XDG_CONFIG_HOME", "/cfg"), ("XDG_STATE_HOME", "/st")]),
            Path::new(HOME),
        );
        assert_eq!(dirs.config_dir(), Path::new("/cfg/fuzzy-dedupe"));
        assert_eq!(dirs.state_dir(), Path::new("/st/fuzzy-dedupe"));
        assert_eq!(dirs.log_dir(), Path::new("/st/fuzzy-dedupe/logs"));
    }

    #[test]
    fn unset_xdg_values_fall_back_to_home() {
        let dirs = AppDirs::resolve(env_of(&[]), Path::new(HOME));
        assert_eq!(
            dirs.config_dir(),
            Path::new("/home/tester/.config/fuzzy-dedupe")
        );
        assert_eq!(
            dirs.state_dir(),
            Path::new("/home/tester/.local/state/fuzzy-dedupe")
        );
        assert_eq!(
            dirs.log_dir(),
            Path::new("/home/tester/.local/state/fuzzy-dedupe/logs")
        );
    }

    #[test]
    fn relative_and_empty_xdg_values_are_ignored() {
        // The XDG Base Directory specification: a relative path in these
        // variables is invalid and must be ignored. An empty value is the
        // degenerate relative case.
        for (config, state) in [("relative/cfg", "./st"), ("", "")] {
            let dirs = AppDirs::resolve(
                env_of(&[("XDG_CONFIG_HOME", config), ("XDG_STATE_HOME", state)]),
                Path::new(HOME),
            );
            assert_eq!(
                dirs.config_dir(),
                Path::new("/home/tester/.config/fuzzy-dedupe"),
                "XDG_CONFIG_HOME={config:?}"
            );
            assert_eq!(
                dirs.log_dir(),
                Path::new("/home/tester/.local/state/fuzzy-dedupe/logs"),
                "XDG_STATE_HOME={state:?}"
            );
        }
    }

    #[test]
    fn absolute_xdg_values_with_a_parent_component_are_ignored() {
        // An absolute value that climbs with `..` is treated like a relative
        // one, so the writer is never handed a root it would itself refuse.
        let dirs = AppDirs::resolve(
            env_of(&[
                ("XDG_CONFIG_HOME", "/cfg/../etc"),
                ("XDG_STATE_HOME", "/st/.."),
            ]),
            Path::new(HOME),
        );
        assert_eq!(
            dirs.config_dir(),
            Path::new("/home/tester/.config/fuzzy-dedupe")
        );
        assert_eq!(
            dirs.log_dir(),
            Path::new("/home/tester/.local/state/fuzzy-dedupe/logs")
        );
    }

    #[test]
    fn one_variable_set_does_not_affect_the_other() {
        let dirs = AppDirs::resolve(env_of(&[("XDG_STATE_HOME", "/st")]), Path::new(HOME));
        assert_eq!(
            dirs.config_dir(),
            Path::new("/home/tester/.config/fuzzy-dedupe")
        );
        assert_eq!(dirs.log_dir(), Path::new("/st/fuzzy-dedupe/logs"));
    }

    // ── App-owned writer ──────────────────────────────────────────────────

    #[test]
    fn writer_creates_missing_directories_and_writes() {
        let (_root, dirs) = fixture_dirs("write");
        let files = AppFiles::new(&dirs);
        let target = dirs.config_dir().join("preferences.toml");
        files
            .write_atomic(&target, b"version = 1\n")
            .expect("write");
        assert_eq!(std::fs::read(&target).expect("read back"), b"version = 1\n");

        let nested = dirs.state_dir().join("a").join("b.txt");
        files.write_atomic(&nested, b"x").expect("nested write");
        assert_eq!(std::fs::read(&nested).expect("read nested"), b"x");
    }

    #[test]
    fn writer_replaces_a_file_atomically() {
        let (_root, dirs) = fixture_dirs("replace");
        let files = AppFiles::new(&dirs);
        let target = dirs.config_dir().join("preferences.toml");
        files
            .write_atomic(&target, b"old contents")
            .expect("first write");

        // A handle opened before the replace keeps reading the old file: the
        // writer swaps in a new file by rename instead of truncating and
        // rewriting the one in place, so no reader ever sees a partial file.
        let before = std::fs::File::open(&target).expect("open old file");
        files.write_atomic(&target, b"new").expect("replace");

        assert_eq!(std::fs::read(&target).expect("read new"), b"new");
        let mut old = String::new();
        std::io::Read::read_to_string(&mut &before, &mut old).expect("read old handle");
        assert_eq!(old, "old contents");
        // No temporary file is left beside the target.
        assert_eq!(listing(dirs.config_dir()), ["preferences.toml"]);
    }

    #[test]
    fn writer_refuses_paths_outside_its_directories() {
        let (root, dirs) = fixture_dirs("refuse");
        let files = AppFiles::new(&dirs);
        let refused = [
            // A sibling of the app directories inside the same fixture.
            root.join("elsewhere.txt"),
            // Escaping through a parent component.
            dirs.config_dir().join("..").join("escape.txt"),
            // A relative path, whatever it resolves against.
            PathBuf::from("preferences.toml"),
            // The directory itself is not a file inside it.
            dirs.config_dir().to_path_buf(),
            // A prefix that is a string prefix but not a path prefix.
            PathBuf::from(format!("{}-other/x", dirs.config_dir().display())),
        ];
        for path in refused {
            let err = files
                .write_atomic(&path, b"x")
                .expect_err(&format!("{} must be refused", path.display()));
            assert!(
                matches!(err, AppFileError::OutsideAppDirs(_)),
                "{}: {err:?}",
                path.display()
            );
        }
        assert!(!root.join("elsewhere.txt").exists());
        assert!(!root.join("config").join("escape.txt").exists());
    }

    #[test]
    fn writer_refuses_to_pass_through_a_non_directory() {
        // A regular file standing where a directory is needed takes the same
        // refusal path as a symbolic link would, which a test file may not
        // create (C-09).
        let (_root, dirs) = fixture_dirs("blocked");
        let files = AppFiles::new(&dirs);
        let blocker = dirs.state_dir().join("blocker");
        files.write_atomic(&blocker, b"x").expect("fixture file");
        let err = files
            .write_atomic(&blocker.join("inner.txt"), b"y")
            .expect_err("a file in the path must be refused");
        assert!(
            matches!(err, AppFileError::UnsafePath(ref p) if *p == blocker),
            "{err:?}"
        );
        assert_eq!(std::fs::read(&blocker).expect("blocker intact"), b"x");
    }

    #[test]
    fn writer_refuses_a_target_that_is_not_a_regular_file() {
        let (_root, dirs) = fixture_dirs("target-dir");
        let files = AppFiles::new(&dirs);
        let target = dirs.config_dir().join("sub");
        files.ensure_dir(&target).expect("fixture dir");
        let err = files
            .write_atomic(&target, b"x")
            .expect_err("a directory at the target must be refused");
        assert!(
            matches!(err, AppFileError::UnsafePath(ref p) if *p == target),
            "{err:?}"
        );
        // Refused before any temporary file was created.
        assert_eq!(listing(dirs.config_dir()), ["sub"]);
    }

    #[test]
    fn writer_creates_a_directory_inside_and_refuses_one_outside() {
        let (root, dirs) = fixture_dirs("ensure");
        let files = AppFiles::new(&dirs);
        files.ensure_dir(dirs.log_dir()).expect("log dir");
        assert!(dirs.log_dir().is_dir());
        let err = files
            .ensure_dir(&root.join("outside"))
            .expect_err("outside directory must be refused");
        assert!(matches!(err, AppFileError::OutsideAppDirs(_)), "{err:?}");
        assert!(!root.join("outside").exists());
    }

    // ── Committed symbolic-link fixtures ──────────────────────────────────
    //
    // `crates/fuzzy-dedupe-platform/fixtures/` (see its README) holds real
    // links, committed with mode 120000, because a test may not create one
    // (C-09) and a regular-file stand-in cannot tell `symlink_metadata` from
    // `metadata`. Every test below only reads the tree when the code under
    // test is correct; a regression would write into the source checkout,
    // which `git status` then shows.

    fn fixture(rel: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures")
            .join(rel)
    }

    /// Fails loudly when a checkout or sync dereferenced the fixture link,
    /// so the test below it cannot pass vacuously.
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

    /// Writer over a committed config base; state goes to a fresh fixture.
    fn linked_config_files(base: &str) -> (AppDirs, AppFiles) {
        let config = fixture(base);
        let state = fresh_dir("linked-state");
        let dirs = AppDirs::resolve(
            move |name| match name {
                "XDG_CONFIG_HOME" => Some(config.clone().into_os_string()),
                "XDG_STATE_HOME" => Some(state.clone().into_os_string()),
                _ => None,
            },
            Path::new(HOME),
        );
        let files = AppFiles::new(&dirs);
        (dirs, files)
    }

    #[test]
    fn writer_refuses_a_symlinked_directory_below_the_root() {
        let (dirs, files) = linked_config_files("linked-config/config");
        let sub = dirs.config_dir().join("sub");
        assert_is_link(&sub);
        let decoy = fixture("linked-config/decoy-dir");
        let before = listing(&decoy);

        let err = files
            .write_atomic(&sub.join("new.toml"), b"x")
            .expect_err("a link below the root must be refused");
        assert!(
            matches!(err, AppFileError::UnsafePath(ref p) if *p == sub),
            "{err:?}"
        );
        let err = files
            .ensure_dir(&sub.join("nested"))
            .expect_err("a link below the root must be refused");
        assert!(
            matches!(err, AppFileError::UnsafePath(ref p) if *p == sub),
            "{err:?}"
        );
        assert_eq!(listing(&decoy), before, "the link target was not written");
    }

    #[test]
    fn writer_refuses_a_symlinked_target_file() {
        let (dirs, files) = linked_config_files("linked-config/config");
        let target = dirs.config_dir().join("preferences.toml");
        assert_is_link(&target);
        let decoy = fixture("linked-config/decoy-dir/target.toml");
        let before = std::fs::read(&decoy).expect("read decoy");

        let err = files
            .write_atomic(&target, b"replaced = true\n")
            .expect_err("a linked target must be refused");
        assert!(
            matches!(err, AppFileError::UnsafePath(ref p) if *p == target),
            "{err:?}"
        );
        assert_eq!(std::fs::read(&decoy).expect("read decoy"), before);
        assert_is_link(&target);
        // Refused before any temporary file was created beside the target.
        assert_eq!(listing(dirs.config_dir()), ["preferences.toml", "sub"]);
    }

    #[test]
    fn a_symlinked_app_root_is_trusted() {
        // The root may be a link (GNU Stow and similar dotfile managers).
        // Proven without writing into the checkout: the resolver hands out
        // the linked root as is, and `ensure_dir` on a real directory below
        // it walks the root and that directory without refusal, touching
        // nothing because both already exist.
        let (dirs, files) = linked_config_files("linked-root/config");
        assert_is_link(dirs.config_dir());
        assert_eq!(
            dirs.config_dir(),
            fixture("linked-root/config/fuzzy-dedupe")
        );
        let real_root = fixture("linked-root/config/real-root");
        let before = listing(&real_root.join("sub"));

        files
            .ensure_dir(&dirs.config_dir().join("sub"))
            .expect("a real directory below a trusted linked root");
        assert_eq!(listing(&real_root), ["sub"]);
        assert_eq!(listing(&real_root.join("sub")), before);
    }

    // ── Open location ─────────────────────────────────────────────────────

    /// Wait up to `DEADLINE` for `flag`, so a check of a worker's
    /// completion callback does not race the worker thread.
    fn wait_for(flag: &AtomicBool) -> bool {
        let deadline = Instant::now() + DEADLINE;
        while !flag.load(Ordering::SeqCst) {
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        true
    }

    const DEADLINE: Duration = Duration::from_secs(10);

    fn poll_open(handle: &mut OpenHandle) -> Result<(), OpenError> {
        let deadline = Instant::now() + DEADLINE;
        loop {
            if let Some(result) = handle.poll() {
                return result;
            }
            assert!(Instant::now() < deadline, "open result never arrived");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Open through the worker and wait for its verdict. Only refused
    /// targets reach this helper, so no desktop opener is ever launched.
    fn open_and_wait(opener: &SystemOpener, location: Location) -> Result<(), OpenError> {
        let notified = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&notified);
        let mut handle = opener.open(
            location,
            Box::new(move || flag.store(true, Ordering::SeqCst)),
        );
        let result = poll_open(&mut handle);
        assert!(wait_for(&notified), "notify runs on completion");
        assert!(handle.poll().is_none(), "the result is delivered once");
        result
    }

    #[test]
    fn open_rejects_a_non_directory_non_https_target() {
        let (_root, dirs) = fixture_dirs("open");
        let files = AppFiles::new(&dirs);
        let regular = dirs.config_dir().join("plain.txt");
        files.write_atomic(&regular, b"x").expect("fixture file");
        let opener = SystemOpener::start().expect("opener worker");

        for folder in [
            regular.clone(),
            dirs.config_dir().join("missing"),
            PathBuf::from("relative/dir"),
        ] {
            let err = open_and_wait(&opener, Location::Folder(folder.clone()))
                .expect_err(&format!("{} must be refused", folder.display()));
            assert!(
                matches!(err, OpenError::NotADirectory(_)),
                "{}: {err:?}",
                folder.display()
            );
        }
        for address in [
            "http://example.com/",
            "file:///etc/passwd",
            "ftp://example.com/",
            "javascript:alert(1)",
            "https://",
            "https:///path",
            "https://exa mple.com/",
            "-https://example.com/",
            "",
            "https://@/",
            "https://:443/",
            "https://user@host/",
            "https://user:pw@example.com/",
            "https://example.com:/",
            "https://example.com:44x/",
            "https://exa_mple.com/",
            "https://[]/",
            "https://[fe80::1%25eth0]/",
            "https://[::1/",
            "https://example.com\\@evil/",
            "https://[:::]/",
            "https://[1:2]/",
            "https://example.com:99999/",
            "https://example.com:0/",
        ] {
            let err = open_and_wait(&opener, Location::WebAddress(address.to_owned()))
                .expect_err(&format!("{address:?} must be refused"));
            assert!(
                matches!(err, OpenError::NotHttps(_)),
                "{address:?}: {err:?}"
            );
        }
    }

    #[test]
    fn opener_survives_a_panicking_notify() {
        let opener = SystemOpener::start().expect("opener worker");
        let mut first = opener.open(
            Location::WebAddress("http://refused/".to_owned()),
            Box::new(|| panic!("the UI's repaint hook panicked")),
        );
        assert!(matches!(poll_open(&mut first), Err(OpenError::NotHttps(_))));
        // The panic is contained after the result was published, so the
        // worker keeps serving later opens.
        let second = open_and_wait(&opener, Location::WebAddress("ftp://refused/".to_owned()));
        assert!(matches!(second, Err(OpenError::NotHttps(_))), "{second:?}");
    }

    #[test]
    fn opener_result_is_ready_when_notify_fires() {
        // A consumer driven only by the notification, as egui's repaint is:
        // it never polls before the signal, so `notify` must run after the
        // result is published.
        let opener = SystemOpener::start().expect("opener worker");
        let (signal, woken) = mpsc::channel::<()>();
        let mut handle = opener.open(
            Location::WebAddress("http://refused/".to_owned()),
            Box::new(move || signal.send(()).expect("signal")),
        );
        woken.recv_timeout(DEADLINE).expect("notified");
        assert!(
            matches!(handle.poll(), Some(Err(OpenError::NotHttps(_)))),
            "the result must be visible by the time notify runs"
        );
    }

    #[test]
    fn picker_result_is_ready_when_notify_fires() {
        let (signal, woken) = mpsc::channel::<()>();
        let mut handle = PickHandle::spawn(
            || PickOutcome::Dismissed,
            move || signal.send(()).expect("signal"),
        );
        woken.recv_timeout(DEADLINE).expect("notified");
        assert_eq!(
            handle.poll(),
            Some(Ok(PickOutcome::Dismissed)),
            "the result must be visible by the time notify runs"
        );
    }

    #[test]
    fn open_accepts_a_directory_or_an_https_address() {
        let (_root, dirs) = fixture_dirs("open-ok");
        let files = AppFiles::new(&dirs);
        files.ensure_dir(dirs.config_dir()).expect("fixture dir");
        // `check` is the validation `open` runs before it launches anything;
        // calling it alone keeps the test from spawning a desktop opener.
        check(&Location::Folder(dirs.config_dir().to_path_buf())).expect("directory");
        check(&Location::WebAddress(
            "https://github.com/L3DigitalNet/fuzzy-dedupe".to_owned(),
        ))
        .expect("https");
        for address in [
            "HTTPS://example.com",
            "https://example.com",
            "https://example.com/",
            "https://example.com:8443/path?q=1#frag",
            "https://sub-1.example.com?q",
            "https://example.com#top",
            "https://192.0.2.1/",
            "https://[2001:db8::1]/",
            "https://[::1]:443",
            "https://example.com:000443/p?q#f",
            "https://[::1]:443/",
        ] {
            check(&Location::WebAddress(address.to_owned()))
                .unwrap_or_else(|e| panic!("{address:?} must be accepted: {e:?}"));
        }
    }

    // ── Picker handle ─────────────────────────────────────────────────────

    fn poll_until_ready(handle: &mut PickHandle) -> Result<PickOutcome, PickError> {
        let deadline = Instant::now() + DEADLINE;
        loop {
            if let Some(result) = handle.poll() {
                return result;
            }
            assert!(Instant::now() < deadline, "picker result never arrived");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn picker_poll_never_blocks_while_the_dialog_is_open() {
        let (release, gate) = mpsc::channel::<()>();
        let notified = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&notified);
        let mut handle = PickHandle::spawn(
            move || {
                // Stands in for a dialog the user has not answered yet.
                gate.recv().expect("released");
                PickOutcome::Chosen(vec![PathBuf::from("/data/a"), PathBuf::from("/data/b")])
            },
            move || flag.store(true, Ordering::SeqCst),
        );

        let started = Instant::now();
        for _ in 0..10 {
            assert!(
                handle.poll().is_none(),
                "no result while the dialog is open"
            );
        }
        // Ten polls against a blocked worker return at once; a blocking
        // poll would hang here until the release below, which never comes
        // first.
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(!notified.load(Ordering::SeqCst));

        release.send(()).expect("release worker");
        let outcome = poll_until_ready(&mut handle).expect("worker result");
        assert_eq!(
            outcome,
            PickOutcome::Chosen(vec![PathBuf::from("/data/a"), PathBuf::from("/data/b")])
        );
        // The worker sends before it calls notify, so the result can be
        // seen a moment before the flag is set; wait instead of reading it
        // once.
        assert!(wait_for(&notified), "notify runs on completion");
        assert!(handle.poll().is_none(), "the result is delivered once");
    }

    #[test]
    fn picker_dismissal_and_worker_failure_are_reported() {
        let mut dismissed = PickHandle::spawn(|| PickOutcome::Dismissed, || {});
        assert_eq!(poll_until_ready(&mut dismissed), Ok(PickOutcome::Dismissed));

        let mut failed = PickHandle::spawn(|| panic!("dialog backend crashed"), || {});
        assert_eq!(poll_until_ready(&mut failed), Err(PickError::WorkerFailed));
    }
}
