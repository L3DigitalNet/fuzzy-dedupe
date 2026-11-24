// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! PV-T20-001 (plan T20): `preferences.toml` version 1 (C-08) survives a
//! restart, never carries the queue, decisions or results, and an
//! unreadable or newer file starts on the defaults without being rewritten
//! until a preference changes.
//!
//! Every test sits in the `preferences` module so `-E 'test(preferences)'`
//! selects them. A restart is a second `FuzzyDedupeApp` built against the
//! same overridden `AppDirs`, after the first one's `on_exit` (eframe's
//! exit hook, which forces the throttled write). Every directory lives
//! under `CARGO_TARGET_TMPDIR`; fixture files are placed through the
//! app-owned writer, since the no-file-mutation guard forbids `std::fs`
//! writes here too.

mod preferences {
    use std::collections::BTreeMap;
    use std::io::{self, Write};
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};
    use std::time::{SystemTime, UNIX_EPOCH};

    use eframe::App as _;
    use egui::Vec2;
    use egui_kittest::Harness;
    use fuzzy_dedupe_app::app::{AppDeps, Clock, FuzzyDedupeApp, viewport_for};
    use fuzzy_dedupe_app::measure::NoMeasurement;
    use fuzzy_dedupe_app::presentation::commands::UiIntent;
    use fuzzy_dedupe_app::presentation::preferences;
    use fuzzy_dedupe_app::presentation::state::{
        ColumnId, PreferencesState, PresentationState, ThemeOverride, WindowPreference,
    };
    use fuzzy_dedupe_app::ui::theme::Zoom;
    use fuzzy_dedupe_domain::{
        IgnoreOptions, MatchMode, MatchingRules, Permille, RulesRevision, ScanFolder, ScanSetup,
    };
    use fuzzy_dedupe_engine::service::{EngineConfig, ReviewCommand};
    use fuzzy_dedupe_platform::app_files::AppFiles;
    use fuzzy_dedupe_platform::dirs::AppDirs;
    use fuzzy_dedupe_platform::fonts::SystemFontLookup;
    use fuzzy_dedupe_platform::log_retention::LogDate;
    use fuzzy_dedupe_platform::open::{Location, OpenHandle, Opener};
    use fuzzy_dedupe_platform::picker::{FolderPicker, PickHandle, PickOutcome, PickRequest};

    /// Step bound for every `Harness::run` here, in place of kittest's
    /// default of 4: engine results arrive from another thread and the
    /// widgets they first draw ask for extra frames (PR #40). Each harness
    /// test file under `tests/` declares its own copy; keep them equal.
    const SETTLE_STEPS: u64 = 16;

    /// The keys a version 1 file may hold at its top level (C-08). Anything
    /// else, a queue or decisions table in particular, is a leak.
    const ALLOWED_KEYS: [&str; 7] = [
        "version",
        "window",
        "view",
        "findings",
        "compare",
        "scan_setup",
        "matching_rules",
    ];

    // ── Doubles ──────────────────────────────────────────────────────────

    struct NoOpener;

    impl Opener for NoOpener {
        fn open(&self, _: Location, notify: Box<dyn FnOnce() + Send>) -> OpenHandle {
            notify();
            OpenHandle::ready(Ok(()))
        }
    }

    struct NoPicker;

    impl FolderPicker for NoPicker {
        fn pick_folders(&self, _: PickRequest, notify: Box<dyn FnOnce() + Send>) -> PickHandle {
            PickHandle::spawn(|| PickOutcome::Dismissed, notify)
        }
    }

    struct FixedClock;

    impl Clock for FixedClock {
        fn today(&self) -> LogDate {
            LogDate::new(2026, 11, 23).expect("valid date")
        }
    }

    /// A `tracing` writer that keeps everything written to it.
    #[derive(Clone, Default)]
    struct Captured(Arc<Mutex<Vec<u8>>>);

    impl Write for Captured {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl Captured {
        fn text(&self) -> String {
            String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned()
        }
    }

    // ── Harness ─────────────────────────────────────────────────────────

    type Shell = Harness<'static, Option<FuzzyDedupeApp>>;

    /// Fresh configuration and state directories for one test, both
    /// overridden so nothing reaches the real `$HOME`.
    fn dirs_for(name: &str) -> AppDirs {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after 1970")
            .as_nanos();
        let root = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("preferences")
            .join(format!("{name}-{nanos}"));
        let config = root.join("config");
        let state = root.join("state");
        AppDirs::resolve(
            move |var| match var {
                "XDG_CONFIG_HOME" => Some(config.clone().into_os_string()),
                "XDG_STATE_HOME" => Some(state.clone().into_os_string()),
                _ => None,
            },
            &root.join("home"),
        )
    }

    fn file_path(dirs: &AppDirs) -> PathBuf {
        dirs.config_dir().join("preferences.toml")
    }

    fn file_bytes(dirs: &AppDirs) -> Option<Vec<u8>> {
        std::fs::read(file_path(dirs)).ok()
    }

    fn place_file(dirs: &AppDirs, contents: &str) {
        AppFiles::new(dirs)
            .write_atomic(&file_path(dirs), contents.as_bytes())
            .expect("fixture file written");
    }

    fn launch(dirs: &AppDirs) -> Shell {
        let deps = AppDeps {
            dirs: dirs.clone(),
            opener: Box::new(NoOpener),
            picker: Box::new(NoPicker),
            fonts: Box::new(SystemFontLookup),
            clock: Box::new(FixedClock),
            log_expiry: Box::new(|_| {}),
            engine: EngineConfig::default(),
            measure: Box::new(NoMeasurement),
        };
        let mut harness = Harness::builder()
            .with_max_steps(SETTLE_STEPS)
            .with_size(Vec2::new(1600.0, 960.0))
            .build_ui_state(
                |ui, slot: &mut Option<FuzzyDedupeApp>| {
                    if let Some(app) = slot.as_mut() {
                        app.run_logic(ui.ctx());
                        app.draw(ui);
                    }
                },
                None,
            );
        let app = FuzzyDedupeApp::new(&harness.ctx, deps).expect("engine starts");
        *harness.state_mut() = Some(app);
        harness.run();
        harness
    }

    fn app(h: &mut Shell) -> &mut FuzzyDedupeApp {
        h.state_mut().as_mut().expect("app")
    }

    fn state(h: &Shell) -> &PresentationState {
        &h.state().as_ref().expect("app").state
    }

    /// eframe's exit path: the forced final write.
    fn exit(mut h: Shell) {
        app(&mut h).on_exit();
    }

    fn intent(h: &mut Shell, intent: UiIntent) {
        app(h).push_intent(intent);
        h.run();
    }

    fn saved_rules() -> MatchingRules {
        MatchingRules {
            mode: MatchMode::ExactAndFuzzy,
            minimum_similarity: Permille::new(853).expect("in range"),
            ignore: IgnoreOptions {
                whitespace: true,
                line_endings: false,
                letter_case: true,
                punctuation: false,
            },
            revision: RulesRevision::new(7),
        }
    }

    fn scan_setup() -> ScanSetup {
        ScanSetup {
            folders: vec![
                ScanFolder {
                    path: PathBuf::from("/srv/archive/notes"),
                    protect: true,
                },
                ScanFolder {
                    path: PathBuf::from("/home/user/文書/مستندات"),
                    protect: false,
                },
            ],
            file_types: vec!["txt".to_owned(), "md".to_owned()],
        }
    }

    fn column_widths() -> BTreeMap<ColumnId, f32> {
        BTreeMap::from([(ColumnId::FileName, 312.5), (ColumnId::Folder, 151.0)])
    }

    /// The preferences after every C-08 field moved off its default.
    fn changed() -> PreferencesState {
        PreferencesState {
            version: PreferencesState::VERSION,
            window: Some(WindowPreference {
                width: 1440.0,
                height: 870.0,
                maximized: true,
            }),
            zoom: Zoom::from_percent(120).expect("on the grid"),
            theme: ThemeOverride::Dark,
            rail_collapsed: true,
            findings_column_widths: column_widths(),
            compare_split_ratio: 0.35,
            compare_wrap: true,
            compare_linked_scroll: false,
            scan_setup: scan_setup(),
            saved_rules: Some(saved_rules()),
        }
    }

    /// Moves every C-08 field the way the app does: intents for theme,
    /// zoom, saved rules and scan setup; the Findings view's own width
    /// state, which it mirrors into the preferences; and the fields whose
    /// writers are the window observer, the rail and Compare, set directly
    /// because kittest has no real window and Compare is not open.
    fn change_every_field(h: &mut Shell) {
        intent(h, UiIntent::SetTheme(ThemeOverride::Dark));
        intent(h, UiIntent::ZoomIn);
        intent(h, UiIntent::ZoomIn);
        intent(h, UiIntent::SetSavedRules(saved_rules()));
        intent(h, UiIntent::SetScanSetup(scan_setup()));
        {
            let state = &mut app(h).state;
            state.findings.column_widths = column_widths();
            let preferences = &mut state.preferences;
            preferences.window = changed().window;
            preferences.rail_collapsed = true;
            preferences.compare_split_ratio = 0.35;
            preferences.compare_wrap = true;
            preferences.compare_linked_scroll = false;
        }
        h.run();
        assert_eq!(state(h).preferences, changed(), "every field changed");
    }

    // ── Round trip ──────────────────────────────────────────────────────

    #[test]
    fn restart_restores_every_preferences_field() {
        let dirs = dirs_for("round-trip");
        let mut first = launch(&dirs);
        change_every_field(&mut first);
        exit(first);
        // The file alone, before any view of the second run can touch it.
        assert_eq!(preferences::load(&dirs), changed());

        let second = launch(&dirs);
        assert_eq!(state(&second).preferences, changed());
        exit(second);

        // The viewport the binary opens with restores size and maximized.
        let viewport = viewport_for(&preferences::load(&dirs));
        assert_eq!(viewport.inner_size, Some(Vec2::new(1440.0, 870.0)));
        assert_eq!(viewport.maximized, Some(true));
    }

    #[test]
    fn preferences_file_is_version_1_with_only_c08_keys() {
        let dirs = dirs_for("keys");
        let mut h = launch(&dirs);
        change_every_field(&mut h);
        exit(h);

        let text = String::from_utf8(file_bytes(&dirs).expect("file written")).expect("UTF-8");
        let table: toml::Table = toml::from_str(&text).expect("valid TOML");
        assert_eq!(table.get("version"), Some(&toml::Value::Integer(1)));
        for key in table.keys() {
            assert!(ALLOWED_KEYS.contains(&key.as_str()), "unexpected key {key}");
        }
        for leak in ["queue", "decisions", "results", "position"] {
            assert!(!text.contains(leak), "{leak} written:\n{text}");
        }
    }

    #[test]
    fn preferences_restart_restores_no_queue_decisions_or_results() {
        // What any start shows: the startup scenario's seeded review state.
        let pristine_dirs = dirs_for("pristine");
        let pristine = launch(&pristine_dirs);
        let seeded_queue: Vec<_> = state(&pristine).queue.entries.keys().copied().collect();
        let seeded_decisions = state(&pristine).queue.decisions.clone();
        let seeded_results = state(&pristine).scan.results.clone();
        assert!(
            !seeded_queue.is_empty(),
            "the design scenario seeds a queue"
        );
        exit(pristine);

        let dirs = dirs_for("excluded");
        let mut first = launch(&dirs);
        change_every_field(&mut first);
        intent(
            &mut first,
            UiIntent::Review {
                command: ReviewCommand::ClearForNewScan,
                invoker: None,
            },
        );
        // The clear leaves each file's entry in place at its cleared value,
        // so the proof is that it differs from the seeded decisions.
        assert!(state(&first).queue.entries.is_empty());
        assert_ne!(state(&first).queue.decisions, seeded_decisions);
        exit(first);

        // The restart shows the seeded state again, not the cleared one:
        // nothing of the first run's review state came back from the file.
        let second = launch(&dirs);
        let restored_queue: Vec<_> = state(&second).queue.entries.keys().copied().collect();
        assert_eq!(restored_queue, seeded_queue);
        assert_eq!(state(&second).queue.decisions, seeded_decisions);
        assert_eq!(state(&second).scan.results, seeded_results);
        assert_eq!(state(&second).preferences, changed());
        exit(second);
    }

    /// The regression the Findings sync caused before it adopted the saved
    /// widths: the restarted view copied its empty startup map over the
    /// restored widths, and the next write stored that empty map.
    #[test]
    fn preferences_column_widths_survive_the_next_write() {
        let dirs = dirs_for("widths-kept");
        let mut first = launch(&dirs);
        app(&mut first).state.findings.column_widths = column_widths();
        first.run();
        exit(first);

        let mut second = launch(&dirs);
        assert_eq!(state(&second).findings.column_widths, column_widths());
        // Another preference forces a write after the first frames.
        intent(&mut second, UiIntent::ZoomIn);
        exit(second);
        assert_eq!(
            preferences::load(&dirs).findings_column_widths,
            column_widths()
        );

        // A resize after adoption still reaches the preferences and the file.
        let mut third = launch(&dirs);
        app(&mut third)
            .state
            .findings
            .column_widths
            .insert(ColumnId::Size, 90.0);
        third.run();
        let mut wanted = column_widths();
        wanted.insert(ColumnId::Size, 90.0);
        assert_eq!(state(&third).preferences.findings_column_widths, wanted);
        exit(third);
        assert_eq!(preferences::load(&dirs).findings_column_widths, wanted);
    }

    #[test]
    fn preferences_first_start_writes_nothing() {
        let dirs = dirs_for("first-start");
        let h = launch(&dirs);
        assert_eq!(state(&h).preferences, PreferencesState::default());
        exit(h);
        assert_eq!(file_bytes(&dirs), None);
    }

    // ── Error rules ─────────────────────────────────────────────────────

    /// A file C-08 says to ignore: the app starts on the defaults with a
    /// warning, leaves the file alone through frames and exit, and replaces
    /// it with a version 1 file once a preference changes.
    fn assert_ignored_until_change(name: &str, contents: &str) {
        let dirs = dirs_for(name);
        place_file(&dirs, contents);

        let log = Captured::default();
        let writer = log.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_writer(move || writer.clone())
            .finish();
        let loaded = tracing::subscriber::with_default(subscriber, || preferences::load(&dirs));
        assert_eq!(loaded, PreferencesState::default());
        assert!(log.text().contains("WARN"), "no warning: {}", log.text());

        let h = launch(&dirs);
        assert_eq!(state(&h).preferences, PreferencesState::default());
        exit(h);
        assert_eq!(file_bytes(&dirs).as_deref(), Some(contents.as_bytes()));

        let mut h = launch(&dirs);
        intent(&mut h, UiIntent::ZoomIn);
        exit(h);
        let text = String::from_utf8(file_bytes(&dirs).expect("file")).expect("UTF-8");
        assert_ne!(text, contents);
        let rewritten = preferences::load(&dirs);
        assert_eq!(
            rewritten.zoom,
            Zoom::from_percent(110).expect("on the grid")
        );
        assert_eq!(rewritten.version, 1);
    }

    #[test]
    fn preferences_malformed_file_starts_on_defaults() {
        assert_ignored_until_change("malformed", "version = 1\n[view\nzoom_percent = 150\n");
    }

    #[test]
    fn preferences_wrong_type_file_starts_on_defaults() {
        assert_ignored_until_change(
            "wrong-type",
            "version = 1\n[view]\nzoom_percent = \"big\"\n",
        );
    }

    #[test]
    fn preferences_version_2_file_starts_on_defaults() {
        assert_ignored_until_change(
            "version-2",
            "version = 2\n[view]\nzoom_percent = 150\ntheme = \"dark\"\n",
        );
    }

    #[test]
    fn preferences_missing_version_starts_on_defaults() {
        assert_ignored_until_change("no-version", "[view]\nzoom_percent = 150\n");
    }

    #[test]
    fn preferences_off_grid_values_fall_back_per_field() {
        // Out-of-range values are dropped one field at a time, so one bad
        // value does not throw away the rest of a hand-edited file.
        let dirs = dirs_for("off-grid");
        place_file(
            &dirs,
            "version = 1\n\
             [view]\nzoom_percent = 115\ntheme = \"light\"\nrail_collapsed = true\n\
             [compare]\nsplit_ratio = 7.5\nwrap = true\n\
             [findings.column_widths]\nfile_name = -4.0\nfolder = 200.0\nno_such_column = 9.0\n\
             [window]\nwidth = 0.0\nheight = 700.0\nmaximized = false\n",
        );
        let loaded = preferences::load(&dirs);
        let defaults = PreferencesState::default();
        assert_eq!(loaded.zoom, defaults.zoom);
        assert_eq!(loaded.theme, ThemeOverride::Light);
        assert!(loaded.rail_collapsed);
        assert_eq!(loaded.compare_split_ratio.to_bits(), 0.5_f32.to_bits());
        assert!(loaded.compare_wrap);
        assert_eq!(
            loaded.findings_column_widths,
            BTreeMap::from([(ColumnId::Folder, 200.0)])
        );
        assert_eq!(loaded.window, None);
    }

    /// A folder path that is not UTF-8 loads and survives a rewrite byte
    /// for byte. The fixture spells the path in serde's `OsString` form,
    /// `{ Unix = [..] }`, which `path_os` writes; the test names no OS of
    /// its own because the boundaries check (R5) scans `tests/` too, and
    /// `as_encoded_bytes` is the portable, safe view of the bytes. On a
    /// non-Unix host the `Unix` variant does not deserialize, the file is
    /// unreadable and the test fails on the missing folder; the app crate
    /// runs only on Linux.
    #[test]
    fn preferences_non_utf8_folder_survives_losslessly() {
        // 0xE9 is Latin-1 "é" and never valid UTF-8 on its own.
        const RAW: &[u8] = b"/srv/caf\xe9/notes";
        let bytes: Vec<String> = RAW.iter().map(u8::to_string).collect();
        let dirs = dirs_for("non-utf8");
        place_file(
            &dirs,
            &format!(
                "version = 1\n\
                 [[scan_setup.folders]]\n\
                 path_os = {{ Unix = [{}] }}\n\
                 protect = true\n",
                bytes.join(", ")
            ),
        );

        let loaded = preferences::load(&dirs);
        let [folder] = loaded.scan_setup.folders.as_slice() else {
            panic!("one folder expected: {:?}", loaded.scan_setup);
        };
        assert_eq!(folder.path.to_str(), None, "the path is not UTF-8");
        assert_eq!(folder.path.as_os_str().as_encoded_bytes(), RAW);
        assert!(folder.protect);

        let mut h = launch(&dirs);
        intent(&mut h, UiIntent::ZoomIn);
        exit(h);
        let reloaded = preferences::load(&dirs);
        assert_eq!(
            reloaded.zoom,
            Zoom::from_percent(110).expect("on the grid"),
            "the file was rewritten"
        );
        let [folder] = reloaded.scan_setup.folders.as_slice() else {
            panic!("one folder expected: {:?}", reloaded.scan_setup);
        };
        assert_eq!(folder.path.as_os_str().as_encoded_bytes(), RAW);
        assert!(folder.protect);
    }
}
