// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! PV-T12-001 to PV-T12-005 (plan T12): the shell, presentation core and
//! dialog host, driven headless through `egui_kittest`.
//!
//! Every test sits in the `shell` module so `-E 'test(shell)'` selects them.
//! The app is built on the harness's own egui context before its first
//! frame, as eframe builds it, and each frame runs `run_logic` then `draw`,
//! eframe's `App::logic` then `App::ui`. Adapters are test doubles: no test
//! starts `xdg-open`, opens a portal or reads the real clock for expiry.
//! Every directory lives under `CARGO_TARGET_TMPDIR`, unique per run.
//!
//! Colours are asserted on painted shapes, never as literals: the expected
//! value is always `theme::palette(mode).color(token)`.

mod shell {
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};
    use std::time::{SystemTime, UNIX_EPOCH};

    use egui::accesskit::Role;
    use egui::{Event, Key, Modifiers, Theme, Vec2, ViewportCommand, ViewportId};
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use fuzzy_dedupe_app::app::{
        self, AppDeps, Clock, FuzzyDedupeApp, MIN_WINDOW_SIZE, log_subscriber, platform_log_expiry,
    };
    use fuzzy_dedupe_app::measure::NoMeasurement;
    use fuzzy_dedupe_app::presentation::commands::{
        Chord, GLOBAL_KEYMAP, IntentQueue, SHORTCUT_TABLE, ShortcutScope, UiIntent,
    };
    use fuzzy_dedupe_app::presentation::events::{self, Applied};
    use fuzzy_dedupe_app::presentation::state::{
        CleanupPhase, DefaultAction, DialogKind, DialogSubject, MenuKind, PresentationState,
        ScanStage, ThemeOverride, UniqueTextSubject, WorkspaceState,
    };
    use fuzzy_dedupe_app::ui::shell::{self as ui_shell, MAIN_MENU_ENTRIES};
    use fuzzy_dedupe_app::ui::theme::{self, Mode, Token};
    use fuzzy_dedupe_app::ui::widgets::{dialog_host, status};
    use fuzzy_dedupe_domain::{
        CleanupPlan, CompareMode, FileId, MemberRole, PlanItem, PlanItemStatus, PlanRevision,
        QueueOrigin, QueueScope, Refusal, ScanId,
    };
    use fuzzy_dedupe_engine::cleanup_sim::{CleanupEvent, DialogStatus, RunReport};
    use fuzzy_dedupe_engine::fixtures::design;
    use fuzzy_dedupe_engine::service::{
        BuildCleanupPlan, EngineConfig, EngineEvent, EngineRequest, ExecuteApprovedPlan,
        ReviewCommand, ScanEvent,
    };
    use fuzzy_dedupe_platform::app_files::AppFiles;
    use fuzzy_dedupe_platform::dirs::AppDirs;
    use fuzzy_dedupe_platform::fonts::SystemFontLookup;
    use fuzzy_dedupe_platform::log_retention::{LogDate, log_file_name};
    use fuzzy_dedupe_platform::open::{Location, OpenHandle, Opener};
    use fuzzy_dedupe_platform::picker::{FolderPicker, PickHandle, PickOutcome, PickRequest};

    // ── Doubles ──────────────────────────────────────────────────────────

    #[derive(Clone, Default)]
    struct OpenRecorder(Arc<Mutex<Vec<Location>>>);

    impl Opener for OpenRecorder {
        fn open(&self, location: Location, notify: Box<dyn FnOnce() + Send>) -> OpenHandle {
            self.0.lock().unwrap().push(location);
            notify();
            OpenHandle::ready(Ok(()))
        }
    }

    struct DismissPicker;

    impl FolderPicker for DismissPicker {
        fn pick_folders(&self, _: PickRequest, notify: Box<dyn FnOnce() + Send>) -> PickHandle {
            PickHandle::spawn(|| PickOutcome::Dismissed, notify)
        }
    }

    #[derive(Clone)]
    struct FakeClock(Arc<Mutex<LogDate>>);

    impl Clock for FakeClock {
        fn today(&self) -> LogDate {
            *self.0.lock().unwrap()
        }
    }

    struct Fixture {
        dirs: AppDirs,
        opens: OpenRecorder,
        clock: FakeClock,
        expiries: Arc<Mutex<Vec<LogDate>>>,
    }

    fn date(year: i32, month: u8, day: u8) -> LogDate {
        LogDate::new(year, month, day).expect("valid date")
    }

    /// A fresh home under the test's own target directory, unique per run
    /// because tests may not delete what an earlier run left behind.
    fn scratch(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after 1970")
            .as_nanos();
        Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("shell")
            .join(format!("{name}-{nanos}"))
    }

    /// Directories with `XDG_STATE_HOME` overridden, so logs land in an
    /// explicit state directory rather than a fallback under the home.
    fn dirs_for(name: &str) -> AppDirs {
        let root = scratch(name);
        let state = root.join("state");
        AppDirs::resolve(
            move |var| (var == "XDG_STATE_HOME").then(|| state.clone().into_os_string()),
            &root.join("home"),
        )
    }

    fn fixture(name: &str) -> (AppDeps, Fixture) {
        let dirs = dirs_for(name);
        let opens = OpenRecorder::default();
        let clock = FakeClock(Arc::new(Mutex::new(date(2026, 11, 23))));
        let expiries = Arc::new(Mutex::new(Vec::new()));
        let record = Arc::clone(&expiries);
        let deps = AppDeps {
            dirs: dirs.clone(),
            opener: Box::new(opens.clone()),
            picker: Box::new(DismissPicker),
            fonts: Box::new(SystemFontLookup),
            clock: Box::new(clock.clone()),
            log_expiry: Box::new(move |today| record.lock().unwrap().push(today)),
            engine: EngineConfig::default(),
            measure: Box::new(NoMeasurement),
        };
        (
            deps,
            Fixture {
                dirs,
                opens,
                clock,
                expiries,
            },
        )
    }

    /// The harness state: the app, plus whether frames run `run_logic`.
    struct Slot {
        app: Option<FuzzyDedupeApp>,
        logic: bool,
    }

    type Shell = Harness<'static, Slot>;

    fn launch_sized(name: &str, size: Vec2) -> (Shell, Fixture) {
        let (deps, fixture) = fixture(name);
        let mut harness = Harness::builder().with_size(size).build_ui_state(
            |ui, slot: &mut Slot| {
                if let Some(app) = slot.app.as_mut() {
                    if slot.logic {
                        app.run_logic(ui.ctx());
                    }
                    app.draw(ui);
                }
            },
            Slot {
                app: None,
                logic: true,
            },
        );
        let app = FuzzyDedupeApp::new(&harness.ctx, deps).expect("engine starts");
        harness.state_mut().app = Some(app);
        harness.run();
        (harness, fixture)
    }

    fn launch(name: &str) -> (Shell, Fixture) {
        launch_sized(name, Vec2::new(1600.0, 960.0))
    }

    fn state(harness: &Shell) -> &PresentationState {
        &harness.state().app.as_ref().expect("app").state
    }

    fn state_mut(harness: &mut Shell) -> &mut PresentationState {
        &mut harness.state_mut().app.as_mut().expect("app").state
    }

    /// Presses and releases `key` with `modifiers` in one frame.
    fn chord(harness: &mut Shell, modifiers: Modifiers, key: Key) {
        for pressed in [true, false] {
            harness.input_mut().events.push(Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers,
            });
        }
        harness.step();
    }

    fn ctrl(harness: &mut Shell, key: Key) {
        chord(harness, Modifiers::CTRL, key);
    }

    fn press(harness: &mut Shell, key: Key) {
        chord(harness, Modifiers::NONE, key);
    }

    fn open_dialog(harness: &Shell) -> Option<DialogKind> {
        state(harness).dialog.dialog().map(|d| d.kind)
    }

    fn close_requested(harness: &Shell) -> bool {
        harness
            .output()
            .viewport_output
            .get(&ViewportId::ROOT)
            .is_some_and(|v| v.commands.contains(&ViewportCommand::Close))
    }

    fn close_enough(a: Vec2, b: Vec2) -> bool {
        (a - b).length() < 0.6
    }

    /// Every filled rectangle this frame painted, nested shapes included
    /// (a frame with a shadow paints a shape list).
    fn rects(harness: &Shell) -> Vec<egui::epaint::RectShape> {
        fn walk(shape: &egui::Shape, out: &mut Vec<egui::epaint::RectShape>) {
            match shape {
                egui::Shape::Rect(rect) => out.push(rect.clone()),
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, out)),
                _ => {}
            }
        }
        let mut out = Vec::new();
        for clipped in &harness.output().shapes {
            walk(&clipped.shape, &mut out);
        }
        out
    }

    /// Whether this frame painted a rectangle of `size` filled with `token`.
    fn painted(harness: &Shell, mode: Mode, token: Token, size: Vec2) -> bool {
        let fill = theme::palette(mode).color(token);
        let rects = rects(harness);
        let found = rects
            .iter()
            .any(|rect| rect.fill == fill && close_enough(rect.rect.size(), size));
        if !found {
            let sizes: Vec<Vec2> = rects
                .iter()
                .filter(|rect| rect.fill == fill)
                .map(|rect| rect.rect.size())
                .collect();
            eprintln!("no {size:?} rect in {mode:?} {token:?}; painted sizes: {sizes:?}");
        }
        found
    }

    fn rail_width(harness: &Shell) -> f32 {
        harness.get_by_label(ui_shell::RAIL_NAME).rect().width()
    }

    // ── PV-T12-001: theme follow and override ───────────────────────────

    #[test]
    fn shell_theme_follows_system_and_each_override_in_the_next_frame() {
        let (mut h, _f) = launch("theme");
        assert_eq!(state(&h).preferences.theme, ThemeOverride::System);
        let rail = h.get_by_label(ui_shell::RAIL_NAME).rect().size();

        // A popup stays open throughout, so its frame must recolor too.
        press(&mut h, Key::F1);
        h.run();
        assert_eq!(open_dialog(&h), Some(DialogKind::KeyboardShortcuts));
        let popup = dialog_host::popup_size(&h.ctx, DialogKind::KeyboardShortcuts);

        h.input_mut().system_theme = Some(Theme::Light);
        h.step();
        assert!(painted(&h, Mode::Light, Token::Rail, rail), "light rail");
        assert!(
            painted(&h, Mode::Light, Token::Surface, popup),
            "light popup"
        );

        // A system change alone recolors the very next frame.
        h.input_mut().system_theme = Some(Theme::Dark);
        h.step();
        assert!(
            painted(&h, Mode::Dark, Token::Rail, rail),
            "system dark: rail"
        );
        assert!(
            painted(&h, Mode::Dark, Token::Surface, popup),
            "system dark: popup"
        );

        // Each override, with the system still dark, applies on the next frame.
        for (choice, mode) in [
            (ThemeOverride::Light, Mode::Light),
            (ThemeOverride::Dark, Mode::Dark),
            (ThemeOverride::System, Mode::Dark),
        ] {
            state_mut(&mut h).preferences.theme = choice;
            h.step();
            assert!(painted(&h, mode, Token::Rail, rail), "{choice:?}: rail");
            assert!(
                painted(&h, mode, Token::Surface, popup),
                "{choice:?}: popup"
            );
        }
        assert_eq!(open_dialog(&h), Some(DialogKind::KeyboardShortcuts));
    }

    #[test]
    fn shell_appearance_choice_in_the_main_menu_recolors_the_shell() {
        let (mut h, _f) = launch("appearance");
        h.input_mut().system_theme = Some(Theme::Light);
        h.run();
        let rail = h.get_by_label(ui_shell::RAIL_NAME).rect().size();
        press(&mut h, Key::F10);
        h.run();
        h.get_by_role_and_label(Role::Button, "Dark").click();
        h.step();
        h.step();
        assert_eq!(state(&h).preferences.theme, ThemeOverride::Dark);
        assert!(painted(&h, Mode::Dark, Token::Rail, rail));
    }

    // ── PV-T12-002: title, rail, main menu, status bar, minimum ─────────

    #[test]
    fn shell_window_has_the_native_title_and_the_minimum_size() {
        let viewport = app::viewport();
        assert_eq!(viewport.title.as_deref(), Some("Fuzzy DeDupe"));
        assert_eq!(viewport.min_inner_size, Some(Vec2::new(1024.0, 600.0)));
        let options = app::native_options();
        assert_eq!(options.viewport.min_inner_size, Some(MIN_WINDOW_SIZE));
        assert_eq!(options.viewport.title.as_deref(), Some("Fuzzy DeDupe"));
    }

    #[test]
    fn shell_rail_is_152_wide_and_collapses_to_48_only_below_1280() {
        for (width, expected) in [(1600.0, 152.0), (1280.0, 152.0), (1279.0, 48.0)] {
            let (h, _f) = launch_sized("rail", Vec2::new(width, 960.0));
            assert!(
                (rail_width(&h) - expected).abs() < 0.5,
                "window {width}: rail {} != {expected}",
                rail_width(&h)
            );
        }
        let (h, _f) = launch_sized("minimum", MIN_WINDOW_SIZE);
        assert!((rail_width(&h) - 48.0).abs() < 0.5);
        assert!((h.get_by_label(status::ACCESSIBLE_NAME).rect().height() - 26.0).abs() < 0.5);
    }

    #[test]
    fn shell_status_bar_is_26_high_and_states_counts() {
        let (h, _f) = launch("status");
        let bar = h.get_by_label(status::ACCESSIBLE_NAME).rect();
        // The spec §11.3 value itself, not the constant, so a wrong
        // constant fails here.
        assert!((bar.height() - 26.0).abs() < 0.5, "{bar:?}");
        // The design scenario's frame-01 values.
        h.get_by_label_contains("12,648 text files scanned");
        h.get_by_label("3 queued");
    }

    #[test]
    fn shell_f10_opens_the_six_entry_main_menu_and_closing_refocuses_menu() {
        let (mut h, _f) = launch("menu");
        press(&mut h, Key::F10);
        h.run();
        assert_eq!(state(&h).dialog.menu(), Some(MenuKind::Main));
        assert_eq!(MAIN_MENU_ENTRIES.len(), 6);
        for entry in MAIN_MENU_ENTRIES {
            h.get_by_label(entry);
        }
        assert!(!h.get_by_role_and_label(Role::Button, "Menu").is_focused());

        press(&mut h, Key::Escape);
        h.run();
        assert_eq!(state(&h).dialog.menu(), None);
        assert!(h.get_by_role_and_label(Role::Button, "Menu").is_focused());

        // F10 toggles: open, then F10 again closes and refocuses too.
        press(&mut h, Key::F10);
        h.run();
        press(&mut h, Key::F10);
        h.run();
        assert_eq!(state(&h).dialog.menu(), None);
        assert!(h.get_by_role_and_label(Role::Button, "Menu").is_focused());
    }

    // ── PV-T12-003: logging, expiry, Open log folder ────────────────────

    fn log_text(dirs: &AppDirs, rust_log: Option<&str>) -> (Vec<String>, String) {
        let setup = log_subscriber(dirs, rust_log).expect("logging starts");
        tracing::subscriber::with_default(setup.subscriber, || {
            tracing::info!("marker-info");
            tracing::debug!("marker-debug");
        });
        drop(setup.guard);
        let mut names = Vec::new();
        let mut text = String::new();
        for entry in std::fs::read_dir(dirs.log_dir()).expect("log dir exists") {
            let entry = entry.expect("entry");
            names.push(entry.file_name().to_string_lossy().into_owned());
            text.push_str(&std::fs::read_to_string(entry.path()).expect("log readable"));
        }
        names.sort();
        (names, text)
    }

    #[test]
    fn shell_logs_into_the_overridden_state_dir_at_info_by_default() {
        let dirs = dirs_for("log-default");
        let before = LogDate::today_utc();
        let (names, text) = log_text(&dirs, None);
        let after = LogDate::today_utc();
        assert!(dirs.log_dir().ends_with("state/fuzzy-dedupe/logs"));
        assert_eq!(names.len(), 1, "{names:?}");
        // The roller's name is exactly the C-07 expiry's pattern; any other
        // name would make every log "not ours" and never expire.
        assert!(
            names[0] == log_file_name(before) || names[0] == log_file_name(after),
            "{names:?}"
        );
        assert!(text.contains("marker-info"));
        assert!(
            !text.contains("marker-debug"),
            "debug must be off by default"
        );
    }

    #[test]
    fn shell_rust_log_debug_raises_the_level() {
        let (_, text) = log_text(&dirs_for("log-debug"), Some("debug"));
        assert!(text.contains("marker-info"));
        assert!(text.contains("marker-debug"));
    }

    #[test]
    fn shell_roller_has_no_count_cap_that_deletes_recent_logs() {
        let dirs = dirs_for("log-cap");
        let files = AppFiles::new(&dirs);
        files.ensure_dir(dirs.log_dir()).expect("log dir");
        // Six recent days plus today: within the 7-day window, so only a
        // file-count cap could remove any of them.
        let today = LogDate::today_utc();
        let recent: Vec<String> = (1..=6)
            .map(|back| {
                let day = LogDate::from_system_time(
                    SystemTime::now() - std::time::Duration::from_secs(back * 86_400),
                );
                log_file_name(day)
            })
            .collect();
        for name in &recent {
            files
                .write_atomic(&dirs.log_dir().join(name), b"older\n")
                .expect("seed log");
        }
        let (names, _) = log_text(&dirs, None);
        for name in &recent {
            assert!(names.contains(name), "{name} was removed: {names:?}");
        }
        assert!(names.contains(&log_file_name(today)) || names.len() == recent.len() + 1);
    }

    #[test]
    fn shell_platform_expiry_deletes_only_expired_logs() {
        let dirs = dirs_for("expiry");
        let files = AppFiles::new(&dirs);
        files.ensure_dir(dirs.log_dir()).expect("log dir");
        let old = dirs.log_dir().join(log_file_name(date(2026, 11, 1)));
        let kept = dirs.log_dir().join(log_file_name(date(2026, 11, 20)));
        let other = dirs.log_dir().join("notes.txt");
        for path in [&old, &kept, &other] {
            files.write_atomic(path, b"x\n").expect("seed");
        }
        let mut expire = platform_log_expiry(dirs.clone());
        expire(date(2026, 11, 23));
        assert!(!old.exists(), "a log 22 days old is expired");
        assert!(kept.exists());
        assert!(other.exists());
    }

    #[test]
    fn shell_log_expiry_runs_at_startup_and_each_rollover_outside_the_ui_pass() {
        let (mut h, f) = launch("rollover");
        assert_eq!(*f.expiries.lock().unwrap(), vec![date(2026, 11, 23)]);
        h.run();
        assert_eq!(f.expiries.lock().unwrap().len(), 1, "not every frame");

        // The date changes, but frames run only the UI pass: no expiry.
        *f.clock.0.lock().unwrap() = date(2026, 11, 24);
        h.state_mut().logic = false;
        h.run();
        assert_eq!(
            f.expiries.lock().unwrap().len(),
            1,
            "the UI pass never expires logs"
        );

        // The first logic pass after the rollover expires once, with the date.
        h.state_mut().logic = true;
        h.run();
        assert_eq!(
            *f.expiries.lock().unwrap(),
            vec![date(2026, 11, 23), date(2026, 11, 24)]
        );
    }

    #[test]
    fn shell_open_log_folder_opens_the_log_directory() {
        let (mut h, f) = launch("open-log");
        press(&mut h, Key::F10);
        h.run();
        h.get_by_role_and_label(Role::Button, "Open log folder")
            .click();
        h.run();
        assert_eq!(
            *f.opens.0.lock().unwrap(),
            vec![Location::Folder(f.dirs.log_dir().to_path_buf())]
        );
        assert_eq!(state(&h).dialog.menu(), None);
    }

    // ── PV-T12-004: global chords ───────────────────────────────────────

    #[test]
    fn shell_every_global_chord_dispatches_its_action() {
        let (mut h, _f) = launch("chords");
        let close = |h: &mut Shell| {
            press(h, Key::Escape);
            h.run();
            assert_eq!(open_dialog(h), None);
        };
        for (key, kind) in [
            (Key::N, DialogKind::NewScan),
            (Key::Comma, DialogKind::MatchingRules),
            (Key::Enter, DialogKind::Cleanup),
        ] {
            ctrl(&mut h, key);
            h.run();
            assert_eq!(open_dialog(&h), Some(kind), "Ctrl+{key:?}");
            close(&mut h);
        }
        press(&mut h, Key::F1);
        h.run();
        assert_eq!(open_dialog(&h), Some(DialogKind::KeyboardShortcuts));
        close(&mut h);

        ctrl(&mut h, Key::Num2);
        assert_eq!(state(&h).workspace, WorkspaceState::Compare);
        ctrl(&mut h, Key::Num1);
        assert_eq!(state(&h).workspace, WorkspaceState::Findings);

        ctrl(&mut h, Key::Equals);
        assert_eq!(state(&h).preferences.zoom.percent(), 110);
        h.run();
        assert!((h.ctx.zoom_factor() - 1.1).abs() < 1e-4, "zoom applied");
        ctrl(&mut h, Key::Minus);
        ctrl(&mut h, Key::Minus);
        assert_eq!(state(&h).preferences.zoom.percent(), 90);
        ctrl(&mut h, Key::Num0);
        assert_eq!(state(&h).preferences.zoom.percent(), 100);
        h.run();

        press(&mut h, Key::F10);
        assert_eq!(state(&h).dialog.menu(), Some(MenuKind::Main));
        press(&mut h, Key::F10);
        h.run();

        ctrl(&mut h, Key::Q);
        assert!(close_requested(&h), "Ctrl+Q closes the window");
    }

    #[test]
    fn shell_an_open_popup_blocks_every_global_chord() {
        let (mut h, _f) = launch("blocked");
        ctrl(&mut h, Key::Comma);
        h.run();
        assert_eq!(open_dialog(&h), Some(DialogKind::MatchingRules));
        for key in [
            Key::N,
            Key::Num2,
            Key::Enter,
            Key::Equals,
            Key::Minus,
            Key::Q,
        ] {
            ctrl(&mut h, key);
            assert!(!close_requested(&h), "Ctrl+{key:?} reached the shell");
        }
        press(&mut h, Key::F10);
        press(&mut h, Key::F1);
        h.run();
        let s = state(&h);
        assert_eq!(open_dialog(&h), Some(DialogKind::MatchingRules));
        assert_eq!(s.workspace, WorkspaceState::Findings);
        assert_eq!(s.preferences.zoom.percent(), 100);
        assert_eq!(s.dialog.menu(), None);
    }

    #[test]
    fn shell_binds_no_single_letter_key() {
        for (chord, action) in GLOBAL_KEYMAP {
            let Chord { ctrl, alt, key, .. } = *chord;
            let letter =
                key.name().len() == 1 && key.name().chars().all(|c| c.is_ascii_alphabetic());
            assert!(
                !letter || ctrl || alt,
                "{action:?} is bound to a bare {key:?}"
            );
        }
        let (mut h, _f) = launch("letters");
        let before = state(&h).clone();
        for key in Key::ALL.iter().copied().filter(|k| {
            let name = k.name();
            name.len() == 1 && name.chars().all(|c| c.is_ascii_alphanumeric())
        }) {
            press(&mut h, key);
            chord(&mut h, Modifiers::SHIFT, key);
            assert!(!close_requested(&h), "{key:?}");
        }
        h.run();
        let after = state(&h);
        assert_eq!(after.workspace, before.workspace);
        assert_eq!(after.dialog, before.dialog);
        assert_eq!(after.preferences, before.preferences);
    }

    // ── PV-T12-005: dialog host ─────────────────────────────────────────

    fn settings(h: &Shell) -> egui_kittest::Node<'_> {
        h.get_by_role_and_label(Role::Button, ui_shell::SETTINGS_NAME)
    }

    #[test]
    fn shell_a_second_popup_request_is_refused() {
        let (mut h, _f) = launch("second");
        settings(&h).click();
        h.run();
        assert_eq!(open_dialog(&h), Some(DialogKind::MatchingRules));
        let mut intents = IntentQueue::default();
        let refused = dialog_host::open(
            state_mut(&mut h),
            DialogKind::About,
            DialogSubject::None,
            None,
            &mut intents,
        );
        assert_eq!(refused, Err(dialog_host::Refused::OverlayOpen));
        let menu = dialog_host::open_menu(&mut state_mut(&mut h).dialog, MenuKind::Main, None);
        assert_eq!(menu, Err(dialog_host::Refused::OverlayOpen));
        h.run();
        assert_eq!(open_dialog(&h), Some(DialogKind::MatchingRules));
    }

    #[test]
    fn shell_esc_cancels_and_focus_returns_to_the_invoker() {
        let (mut h, _f) = launch("esc");
        settings(&h).click();
        h.run();
        assert_eq!(open_dialog(&h), Some(DialogKind::MatchingRules));
        assert!(
            h.get_by_role_and_label(Role::Button, "Close").is_focused(),
            "focus inside"
        );
        press(&mut h, Key::Escape);
        h.run();
        assert_eq!(open_dialog(&h), None);
        assert!(settings(&h).is_focused(), "focus back on the gear");
    }

    #[test]
    fn shell_close_control_closes_and_refocuses() {
        let (mut h, _f) = launch("close");
        settings(&h).click();
        h.run();
        h.get_by_role_and_label(Role::Button, "Close").click();
        h.run();
        assert_eq!(open_dialog(&h), None);
        assert!(settings(&h).is_focused());
    }

    #[test]
    fn shell_enter_runs_the_non_destructive_default_and_refocuses() {
        let (mut h, _f) = launch("enter");
        // Primary default (Save rules, a stub until T16): runs and closes.
        settings(&h).click();
        h.run();
        press(&mut h, Key::Enter);
        h.run();
        assert_eq!(open_dialog(&h), None);
        assert!(settings(&h).is_focused());

        // Review cleanup: Enter only dismisses, never moving to Trash.
        assert_eq!(DialogKind::Cleanup.default_action(), DefaultAction::Dismiss);
        let queued = state(&h).queue.clone();
        let review = h.get_by_role_and_label(Role::Button, "Review cleanup (3)");
        review.click();
        h.run();
        assert_eq!(open_dialog(&h), Some(DialogKind::Cleanup));
        press(&mut h, Key::Enter);
        h.run();
        assert_eq!(open_dialog(&h), None);
        assert_eq!(state(&h).queue, queued, "nothing left the queue");
        assert_eq!(state(&h).operation.revision, None, "no plan was submitted");
        assert!(
            h.get_by_role_and_label(Role::Button, "Review cleanup (3)")
                .is_focused()
        );
    }

    #[test]
    fn shell_menu_launched_popup_returns_focus_to_menu() {
        let (mut h, _f) = launch("about");
        press(&mut h, Key::F10);
        h.run();
        h.get_by_role_and_label(Role::Button, "About Fuzzy DeDupe")
            .click();
        h.run();
        assert_eq!(open_dialog(&h), Some(DialogKind::About));
        press(&mut h, Key::Escape);
        h.run();
        assert!(h.get_by_role_and_label(Role::Button, "Menu").is_focused());
    }

    // ── C-12: stale-origin events ───────────────────────────────────────

    #[test]
    fn shell_events_from_a_non_current_origin_are_discarded() {
        let (h, _f) = launch("stale");
        let mut s = state(&h).clone();
        let current = s.scan.scan.expect("design scan current");
        let stale = ScanId::new(current.get() + 100);
        let event = EngineEvent::Scan {
            scan: stale,
            event: ScanEvent::Started,
        };
        assert_eq!(events::apply(&mut s, event), Applied::DiscardedStale);
        assert_eq!(s.scan.stage, ScanStage::Completed);
        let event = EngineEvent::Scan {
            scan: current,
            event: ScanEvent::Started,
        };
        assert_eq!(events::apply(&mut s, event), Applied::Installed);
        assert_eq!(s.scan.stage, ScanStage::Running);
    }

    // ── Round 2: comparison, cleanup, NeedsReview, subjects ─────────────

    fn app_mut(h: &mut Shell) -> &mut FuzzyDedupeApp {
        h.state_mut().app.as_mut().expect("app")
    }

    /// Runs frames until `done` holds, for engine work on worker threads.
    fn run_until(h: &mut Shell, what: &str, done: impl Fn(&PresentationState) -> bool) {
        for _ in 0..500 {
            h.step();
            if done(state(h)) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("timed out waiting for {what}");
    }

    fn file_named(h: &Shell, name: &str) -> (FileId, FileId) {
        let results = state(h).scan.results.clone().expect("results");
        let member = results
            .groups
            .iter()
            .flat_map(|g| g.members.iter())
            .find(|m| m.name == name)
            .unwrap_or_else(|| panic!("{name} in the design scenario"));
        let MemberRole::Member(link) = member.role else {
            panic!("{name} is a reference");
        };
        (member.file, link.counterpart)
    }

    #[test]
    fn shell_comparison_result_is_stored_and_a_stale_one_discarded() {
        let (mut h, _f) = launch("comparison");
        let pair = design::sample().last_compared.expect("design pair");
        app_mut(&mut h).push_intent(UiIntent::OpenCompare {
            target: pair.b,
            counterpart: pair.a,
            return_row: None,
        });
        run_until(&mut h, "the comparison", |s| {
            s.compare.comparison.aligned().is_some()
        });
        let s = state(&h);
        assert_eq!(s.workspace, WorkspaceState::Compare);
        let slot = &s.compare.comparison;
        let key = slot.key.expect("key recorded");
        assert_eq!((key.target, key.counterpart), (pair.b, pair.a));
        assert_eq!(
            key.target_snapshot,
            s.queue.snapshot(pair.b).expect("snapshot")
        );
        assert_eq!(key.mode, CompareMode::Original);
        assert_eq!(slot.result.as_ref().expect("result").request, key);
        let aligned = slot.aligned().expect("ready");
        assert!(!aligned.rows.is_empty());
        assert_eq!(
            aligned.copy_only_in_a().text.lines().count() as u64,
            aligned.only_in_a.count()
        );

        // A newer request supersedes it: the old answer is now stale.
        let old_request = slot.request.expect("request");
        let old = Arc::clone(slot.result.as_ref().expect("result"));
        let mut newer = key;
        newer.mode = CompareMode::Normalized;
        app_mut(&mut h).push_intent(UiIntent::Submit(EngineRequest::LoadComparison(newer)));
        h.step();
        let mut s = state(&h).clone();
        assert_ne!(s.compare.comparison.request, Some(old_request));
        let event = EngineEvent::Comparison {
            request: old_request,
            comparison: old,
        };
        assert_eq!(events::apply(&mut s, event), Applied::DiscardedStale);
        assert!(
            s.compare.comparison.result.is_none(),
            "stale result not installed"
        );
    }

    fn item(target: u64, status: PlanItemStatus) -> PlanItem {
        PlanItem {
            target: FileId::new(target),
            counterpart: FileId::new(target + 1000),
            size_bytes: 10,
            status,
            reason: None,
        }
    }

    #[test]
    fn shell_cleanup_events_drive_the_operation_state() {
        let (h, _f) = launch("cleanup-events");
        let mut s = state(&h).clone();
        let scan = s.scan.scan.expect("scan");
        let (r1, r2) = (PlanRevision::new(41), PlanRevision::new(42));
        s.operation.revision = Some(r1);
        let plan = Arc::new(CleanupPlan {
            revision: r1,
            scan,
            items: vec![
                item(1, PlanItemStatus::Planned),
                item(2, PlanItemStatus::Blocked),
                item(3, PlanItemStatus::Planned),
            ],
        });
        let cleanup = |plan, event| EngineEvent::Cleanup { plan, event };
        assert_eq!(
            events::apply(
                &mut s,
                cleanup(r1, CleanupEvent::PlanBuilt(Arc::clone(&plan)))
            ),
            Applied::Installed
        );
        assert_eq!(s.operation.phase, CleanupPhase::Review);
        assert_eq!(s.operation.confirmed_count(), 2, "blocked item excluded");
        assert_eq!(s.operation.items.len(), 3);

        s.operation.authorized = true;
        s.operation.phase = CleanupPhase::Running;
        events::apply(
            &mut s,
            cleanup(
                r1,
                CleanupEvent::ItemChanged(item(1, PlanItemStatus::Succeeded)),
            ),
        );
        assert_eq!(s.operation.items[0].status, PlanItemStatus::Succeeded);
        let statuses: Vec<DialogStatus> = s.operation.statuses().map(|(_, d)| d).collect();
        assert_eq!(statuses[0], DialogStatus::Moved);
        assert_eq!(statuses[1], DialogStatus::BlockedNoTrash);

        // A stale plan's event changes nothing.
        let before = s.operation.clone();
        let stale = cleanup(
            PlanRevision::new(7),
            CleanupEvent::ItemChanged(item(3, PlanItemStatus::Failed)),
        );
        assert_eq!(events::apply(&mut s, stale), Applied::DiscardedStale);
        assert_eq!(s.operation, before);

        // The run ends with a revised plan: it becomes current, unconfirmed.
        let report = Arc::new(RunReport {
            plan: r1,
            items: vec![
                item(1, PlanItemStatus::Succeeded),
                item(2, PlanItemStatus::Blocked),
                item(3, PlanItemStatus::Failed),
            ],
            journal: Vec::new(),
        });
        let revised = Arc::new(CleanupPlan {
            revision: r2,
            scan,
            items: vec![item(4, PlanItemStatus::Planned)],
        });
        events::apply(
            &mut s,
            cleanup(
                r1,
                CleanupEvent::RunEnded {
                    revised: Some(Arc::clone(&revised)),
                    report: Arc::clone(&report),
                },
            ),
        );
        assert_eq!(s.operation.revision, Some(r2));
        assert_eq!(s.operation.plan.as_deref(), Some(&*revised));
        assert_eq!(s.operation.phase, CleanupPhase::Stopped);
        assert!(!s.operation.authorized, "a revised plan needs confirming");
        assert_eq!(s.operation.report.as_ref(), Some(&report));
        let summary = s.operation.summary().expect("summary");
        assert_eq!((summary.moved, summary.failed, summary.blocked), (1, 1, 1));
        // The old revision is stale now.
        let late = cleanup(
            r1,
            CleanupEvent::ItemChanged(item(1, PlanItemStatus::Planned)),
        );
        assert_eq!(events::apply(&mut s, late), Applied::DiscardedStale);

        // A refused execution reports it and drops the authorization.
        s.operation.authorized = true;
        s.status = None;
        events::apply(&mut s, cleanup(r2, CleanupEvent::ExecutionRefused));
        assert!(!s.operation.authorized);
        assert!(s.status.is_some());

        // A run that ends with nothing left finishes.
        events::apply(
            &mut s,
            cleanup(
                r2,
                CleanupEvent::RunEnded {
                    revised: None,
                    report,
                },
            ),
        );
        assert_eq!(s.operation.phase, CleanupPhase::Finished);
    }

    #[test]
    fn shell_cleanup_plan_builds_and_runs_through_the_engine() {
        let (mut h, _f) = launch("cleanup-run");
        let scan = state(&h).scan.scan.expect("scan");
        app_mut(&mut h).push_intent(UiIntent::Submit(EngineRequest::BuildCleanupPlan(
            BuildCleanupPlan { scan },
        )));
        h.step();
        assert_eq!(state(&h).operation.phase, CleanupPhase::Building);
        run_until(&mut h, "the plan", |s| {
            s.operation.phase == CleanupPhase::Review
        });
        let s = state(&h);
        assert_eq!(s.operation.items.len(), s.queue.queued_count());
        let plan = s.operation.revision.expect("revision");
        app_mut(&mut h).push_intent(UiIntent::Submit(EngineRequest::ExecuteApprovedPlan(
            ExecuteApprovedPlan { plan },
        )));
        h.step();
        run_until(&mut h, "the run's end", |s| s.operation.report.is_some());
        let s = state(&h);
        assert!(matches!(
            s.operation.phase,
            CleanupPhase::Finished | CleanupPhase::Stopped
        ));
        let report = s.operation.report.as_ref().expect("report");
        let summary = report.summary();
        assert_eq!(
            summary.moved + summary.failed + summary.stopped + summary.blocked + summary.returned,
            report.items.len()
        );
    }

    #[test]
    fn shell_needs_review_opens_review_unique_text_and_refocuses_the_sender() {
        let (mut h, _f) = launch("needs-review");
        let (file, counterpart) = file_named(&h, "Design_Spec_draft.txt");
        let sender = ui_shell::menu_entry_id();
        app_mut(&mut h).push_intent(UiIntent::Review {
            command: ReviewCommand::RequestQueue {
                file,
                counterpart,
                origin: QueueOrigin::Shortcut,
                reviewed: None,
            },
            invoker: Some(sender),
        });
        h.run();
        let s = state(&h);
        let dialog = s.dialog.dialog().expect("a popup opened");
        assert_eq!(dialog.kind, DialogKind::UniqueTextReview);
        let DialogSubject::UniqueText(subject) = dialog.subject else {
            panic!("Review unique text without its pair");
        };
        assert_eq!((subject.target, subject.counterpart), (file, counterpart));
        assert_eq!(Some(subject.target_snapshot), s.queue.snapshot(file));
        assert_eq!(
            Some(subject.counterpart_snapshot),
            s.queue.snapshot(counterpart)
        );
        assert_eq!(subject.origin, QueueOrigin::Shortcut);
        assert!(!s.queue.entries.contains_key(&file), "nothing was queued");

        press(&mut h, Key::Escape);
        h.run();
        assert_eq!(open_dialog(&h), None);
        assert!(h.get_by_role_and_label(Role::Button, "Menu").is_focused());
    }

    #[test]
    fn shell_a_refusal_is_recorded_for_its_row() {
        let (mut h, _f) = launch("refusal");
        let pair = design::sample().last_compared.expect("design pair");
        // The pair's A side is its group's reference, which is never queued.
        app_mut(&mut h).push_intent(UiIntent::Review {
            command: ReviewCommand::RequestQueue {
                file: pair.a,
                counterpart: pair.b,
                origin: QueueOrigin::Checkbox,
                reviewed: None,
            },
            invoker: None,
        });
        h.run();
        assert_eq!(
            state(&h).queue.last_refusal,
            Some((pair.a, Refusal::Reference))
        );
        assert!(state(&h).status.is_some());
        assert_eq!(open_dialog(&h), None);
    }

    #[test]
    fn shell_popups_carry_the_subject_they_need() {
        let (mut h, _f) = launch("subjects");
        let (file, counterpart) = file_named(&h, "Design_Spec_draft.txt");
        let mut intents = IntentQueue::default();
        let s = state_mut(&mut h);
        for (kind, subject) in [
            (DialogKind::UniqueTextReview, DialogSubject::None),
            (DialogKind::QueueIdentical, DialogSubject::None),
            (
                DialogKind::About,
                DialogSubject::QueueIdentical(QueueScope::AllByteIdenticalGroups),
            ),
        ] {
            assert_eq!(
                dialog_host::open(s, kind, subject, None, &mut intents),
                Err(dialog_host::Refused::WrongSubject),
                "{kind:?}"
            );
            assert!(!s.dialog.is_open());
        }
        let scope = QueueScope::Groups(vec![s.scan.results.as_ref().unwrap().groups[0].id]);
        dialog_host::open(
            s,
            DialogKind::QueueIdentical,
            DialogSubject::QueueIdentical(scope.clone()),
            None,
            &mut intents,
        )
        .expect("opens with its scope");
        assert_eq!(
            s.dialog.dialog().map(|d| &d.subject),
            Some(&DialogSubject::QueueIdentical(scope))
        );
        dialog_host::close(&mut s.dialog);
        let subject = UniqueTextSubject {
            scan: s.scan.scan.unwrap(),
            target: file,
            counterpart,
            target_snapshot: s.queue.snapshot(file).unwrap(),
            counterpart_snapshot: s.queue.snapshot(counterpart).unwrap(),
            rules: s.queue.view.as_ref().unwrap().rules,
            reviewed: None,
            origin: QueueOrigin::RowMenu,
        };
        dialog_host::open(
            s,
            DialogKind::UniqueTextReview,
            DialogSubject::UniqueText(subject),
            None,
            &mut intents,
        )
        .expect("opens with its pair");
        let request = subject.comparison(CompareMode::Original);
        assert_eq!((request.target, request.counterpart), (file, counterpart));
    }

    #[test]
    fn shell_shortcut_table_lists_the_global_keymap() {
        let global: Vec<_> = SHORTCUT_TABLE
            .iter()
            .filter(|row| row.scope == ShortcutScope::Global)
            .collect();
        assert_eq!(global.len(), 7);
        assert_eq!(SHORTCUT_TABLE.len(), 23, "every §11.10 row");
        for row in SHORTCUT_TABLE {
            for chord in row.keys.split(" / ") {
                let letter = chord.len() == 1 && chord.chars().all(|c| c.is_ascii_alphabetic());
                assert!(!letter, "{chord} is a single-letter binding");
            }
        }
    }
}
