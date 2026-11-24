// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! PV-T39-001 (plan T39): a normal launch with `FUZZY_DEDUPE_SCENARIO` set
//! starts on that fixture scenario, and New scan replays it.
//!
//! Every test sits in the `startup_scenario` module so
//! `-E 'test(startup_scenario)'` selects them. The decision is proven on the
//! pure `measure::choose_startup`, which `measure::startup` applies once to
//! the process's own variables; the tests never set the environment
//! (`unsafe_code = "deny"`, and edition 2024 makes `set_var` unsafe) and
//! never spawn a child with it set (the no-file-mutation guard forbids
//! process items under `tests/`). The app half starts `FuzzyDedupeApp`
//! through `with_startup`, the body `FuzzyDedupeApp::new` runs on
//! `measure::startup()`, so the adoption path is the production one.
//!
//! Residue: the New scan dialog reads `measure::startup_scenario()`, the
//! process decision's kind, and this process's decision is always the
//! design scenario. The replay is therefore proven on the value that
//! function returns, `StartupScenario::kind` of the decision, submitted as
//! the dialog submits it, rather than through the dialog itself.

mod startup_scenario {
    use std::path::Path;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use eframe::App as _;
    use egui::Vec2;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable as _;
    use fuzzy_dedupe_app::app::{AppDeps, Clock, FuzzyDedupeApp};
    use fuzzy_dedupe_app::measure::{
        MODE_VAR, MeasureScenario, NoMeasurement, SCENARIO_VAR, ScenarioNotice, StartupChoice,
        StartupScenario, choose_startup, parse_mode, startup_for,
    };
    use fuzzy_dedupe_app::presentation::commands::UiIntent;
    use fuzzy_dedupe_app::presentation::preferences;
    use fuzzy_dedupe_app::presentation::state::{PresentationState, ScanStage, ThemeOverride};
    use fuzzy_dedupe_domain::ScanResults;
    use fuzzy_dedupe_engine::fixtures::{self, ScenarioKind};
    use fuzzy_dedupe_engine::service::{EngineConfig, EngineRequest, StartScan};
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

    const MEASURE_SCENARIOS: [MeasureScenario; 4] = [
        MeasureScenario::T1,
        MeasureScenario::T2,
        MeasureScenario::T3,
        MeasureScenario::T4,
    ];

    // ── The decision ─────────────────────────────────────────────────────

    fn design() -> StartupScenario {
        StartupScenario::Fixture(ScenarioKind::Design)
    }

    #[test]
    fn startup_scenario_unset_starts_on_design() {
        assert_eq!(
            choose_startup(None, None),
            StartupChoice {
                startup: design(),
                notice: None,
            }
        );
        assert_eq!(startup_for(None), design());
    }

    #[test]
    fn startup_scenario_each_name_selects_its_kind() {
        for kind in ScenarioKind::ALL {
            assert_eq!(
                choose_startup(None, Some(kind.name())),
                StartupChoice {
                    startup: StartupScenario::Fixture(kind),
                    notice: None,
                },
                "{}",
                kind.name()
            );
        }
    }

    #[test]
    fn startup_scenario_unknown_or_empty_value_starts_on_design_listing_every_name() {
        // Empty, a case variant, a padded name and a stranger: names match
        // `ScenarioKind::from_name` exactly, so each is unknown.
        for value in ["", "Unicode", " unicode", "japanese"] {
            let choice = choose_startup(None, Some(value));
            assert_eq!(choice.startup, design(), "{value:?}");
            let notice = choice.notice.expect("a notice");
            assert_eq!(notice, ScenarioNotice::Unknown(value.to_owned()));
            let line = notice.to_string();
            assert!(!line.contains('\n'), "one line: {line}");
            assert!(line.contains(SCENARIO_VAR), "{line}");
            assert!(line.contains(&format!("{value:?}")), "{line}");
            for kind in ScenarioKind::ALL {
                assert!(
                    line.contains(kind.name()),
                    "{} missing: {line}",
                    kind.name()
                );
            }
        }
    }

    #[test]
    fn startup_scenario_measurement_wins_with_a_note() {
        for scenario in MEASURE_SCENARIOS {
            let label = scenario.label().to_ascii_lowercase();
            let mode = parse_mode(Some(&label)).unwrap().expect("a config");
            // Without a scenario value, measurement's choice is unchanged.
            assert_eq!(
                choose_startup(Some(&mode), None),
                StartupChoice {
                    startup: scenario.startup(),
                    notice: None,
                }
            );
            assert_eq!(startup_for(Some(&mode)), scenario.startup());
            // Any value, valid, unknown or empty, is ignored with a note.
            for value in ["unicode", "design", "japanese", ""] {
                let choice = choose_startup(Some(&mode), Some(value));
                assert_eq!(choice.startup, scenario.startup(), "{label} {value:?}");
                assert_eq!(
                    choice.notice,
                    Some(ScenarioNotice::IgnoredForMeasurement),
                    "{label} {value:?}"
                );
            }
        }
        let line = ScenarioNotice::IgnoredForMeasurement.to_string();
        assert!(!line.contains('\n'), "one line: {line}");
        assert!(
            line.contains(SCENARIO_VAR) && line.contains(MODE_VAR),
            "{line}"
        );
    }

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
            LogDate::new(2026, 11, 24).expect("valid date")
        }
    }

    // ── Harness ──────────────────────────────────────────────────────────

    type Shell = Harness<'static, Option<FuzzyDedupeApp>>;

    /// Fresh configuration and state directories for one test, both
    /// overridden so nothing reaches the real `$HOME`.
    fn dirs_for(name: &str) -> AppDirs {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after 1970")
            .as_nanos();
        let root = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("startup_scenario")
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

    /// A normal (non-measurement) start on `value`'s decision, through
    /// `with_startup`, which installs the theme and the glyph registry
    /// before the first pass, as `FuzzyDedupeApp::new` does.
    fn launch(dirs: &AppDirs, value: &str) -> Shell {
        let choice = choose_startup(None, Some(value));
        assert_eq!(choice.notice, None, "{value} is a valid name");
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
        // These tests read labels and preference files, not glyph shapes.
        // The fonts come from the host through `SystemFontLookup`, and a CI
        // runner has no CJK font, so the `unicode` names would abort the
        // run under kittest's default missing-glyph policy. Rendering those
        // names is EV-001's job, on the reference workstation (T21).
        let mut harness = Harness::builder()
            .with_max_steps(SETTLE_STEPS)
            .with_size(Vec2::new(1600.0, 960.0))
            .allow_missing_glyphs()
            .build_ui_state(
                |ui, slot: &mut Option<FuzzyDedupeApp>| {
                    if let Some(app) = slot.as_mut() {
                        app.run_logic(ui.ctx());
                        app.draw(ui);
                    }
                },
                None,
            );
        let app = FuzzyDedupeApp::with_startup(&harness.ctx, deps, choice.startup)
            .expect("engine starts");
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

    fn results(h: &Shell) -> std::sync::Arc<ScanResults> {
        state(h).scan.results.clone().expect("results")
    }

    fn run_until(h: &mut Shell, what: &str, done: impl Fn(&PresentationState) -> bool) {
        for _ in 0..500 {
            h.step();
            if done(state(h)) {
                h.run();
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("timed out waiting for {what}");
    }

    /// Expands every group, so member rows draw, then asserts each member's
    /// name and folder is on screen. Names come from `results` itself: they
    /// are the fixture's own, so the check cannot drift from it.
    fn assert_every_member_shown(h: &mut Shell, results: &ScanResults, at: &str) {
        app(h).state.findings.expanded = results.groups.iter().map(|g| g.id).collect();
        h.run();
        for member in results.groups.iter().flat_map(|g| &g.members) {
            assert!(
                h.query_all_by_label_contains(&member.name).next().is_some(),
                "{at}: {:?} not shown",
                member.name
            );
            if !member.folder.is_empty() {
                assert!(
                    h.query_all_by_label_contains(&member.folder)
                        .next()
                        .is_some(),
                    "{at}: folder {:?} not shown",
                    member.folder
                );
            }
        }
    }

    // ── The unicode start ────────────────────────────────────────────────

    #[test]
    fn startup_scenario_unicode_start_shows_its_names_and_new_scan_replays_it() {
        let choice = choose_startup(None, Some(ScenarioKind::Unicode.name()));
        let mut h = launch(&dirs_for("unicode"), ScenarioKind::Unicode.name());

        let expected = StartupScenario::Fixture(ScenarioKind::Unicode)
            .build(fuzzy_dedupe_domain::ScanId::new(1), fixtures::DEFAULT_SEED)
            .expect("fixture")
            .results;
        let shown = results(&h);
        let names = |r: &ScanResults| -> Vec<String> {
            r.groups
                .iter()
                .flat_map(|g| &g.members)
                .map(|m| m.display_path.clone())
                .collect()
        };
        assert_eq!(names(&shown), names(&expected), "the unicode fixture");
        // The scenario's point: Japanese, Arabic, emoji, a decomposed
        // accent and a long path, each checked by property rather than by
        // retyping the fixture's names.
        let paths = names(&shown);
        let any = |p: fn(char) -> bool| paths.iter().any(|s| s.chars().any(p));
        assert!(any(|c| ('\u{3040}'..='\u{30ff}').contains(&c)), "Japanese");
        assert!(any(|c| ('\u{0600}'..='\u{06ff}').contains(&c)), "Arabic");
        assert!(any(|c| c >= '\u{1f300}'), "emoji");
        assert!(any(|c| c == '\u{301}'), "a decomposed accent");
        assert!(
            paths.iter().any(|s| s.chars().count() > 80),
            "a long path: {paths:?}"
        );
        assert_every_member_shown(&mut h, &shown, "startup");

        // New scan submits `measure::startup_scenario()`, which is the
        // process decision's `kind()`; this is that value for this choice.
        let replay = choice.startup.kind();
        assert_eq!(replay, ScenarioKind::Unicode);
        let before = state(&h).scan.scan;
        app(&mut h).push_intent(UiIntent::Submit(EngineRequest::StartScan(StartScan {
            setup: shown.setup.clone(),
            rules: shown.rules,
            scenario: replay,
            seed: fixtures::DEFAULT_SEED,
        })));
        run_until(&mut h, "the replayed scan", |s| {
            s.scan.scan != before && s.scan.stage == ScanStage::Completed
        });
        let replayed = results(&h);
        assert_eq!(names(&replayed), names(&expected), "the replay is unicode");
        assert_every_member_shown(&mut h, &replayed, "replay");
    }

    // ── Preferences ──────────────────────────────────────────────────────

    #[test]
    fn startup_scenario_start_still_loads_and_writes_preferences() {
        let dirs = dirs_for("preferences");
        let mut first = launch(&dirs, ScenarioKind::Unicode.name());
        assert_eq!(state(&first).preferences.theme, ThemeOverride::default());
        app(&mut first).push_intent(UiIntent::SetTheme(ThemeOverride::Dark));
        first.run();
        app(&mut first).on_exit();
        // Written: the file now holds the change.
        assert_eq!(preferences::load(&dirs).theme, ThemeOverride::Dark);

        // Loaded: a second scenario start comes up on the saved value.
        let second = launch(&dirs, ScenarioKind::Mixed.name());
        assert_eq!(state(&second).preferences.theme, ThemeOverride::Dark);
    }
}
