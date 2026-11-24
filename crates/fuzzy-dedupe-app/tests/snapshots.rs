// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! PV-T22-002 (plan T22, D-007; spec §11.1, §17.2): `egui_kittest`
//! snapshots of the 16 review states in the light and dark themes, 32
//! snapshots, compared at kittest's default threshold.
//!
//! The states are plan Appendix A C-11's 16 non-dark review frames, each
//! rendered by the whole app (`FuzzyDedupeApp::new`) on the `design`
//! scenario at its frame's window size: 1024 × 600 for 01d and 02b, 1600 ×
//! 960 for every other frame. Each snapshot is named after the frame's PNG
//! in `docs/design/reference/` plus `-light` or `-dark`, and
//! `the_frames_are_the_non_dark_review_frames` ties the list here to that
//! directory, so a frame added there without a snapshot here fails. Each
//! state is also asserted through the accessibility tree before its
//! snapshot is taken, so a baseline can never be recorded of the wrong state.
//!
//! Requirements: the `snapshot` feature (this file compiles only with it)
//! and a wgpu adapter. Baselines are valid only from Mesa lavapipe on the
//! pinned GitHub-hosted image of rust.yml's `ui-test` job (D-007), with
//! `WGPU_BACKEND=vulkan`; kittest's adapter choice prefers a CPU device.
//! Fonts are the embedded faces only: the app is built with a font lookup
//! that returns no system fallbacks, because the runner's fontconfig set
//! differs from any workstation's and would move glyph metrics.
//!
//! Determinism. Every capture first runs to a settled frame, removes the
//! pointer (no hover state), then pins egui's input time to
//! [`CAPTURE_TIME`] and renders. Pinning the time matters for 01b: its
//! indeterminate progress bar is drawn from `input.time`, which otherwise
//! depends on how many frames earlier engine work took. egui's own
//! transitions are already off (kittest sets `animation_time` to 0).
//!
//! Baseline procedure. Baselines are `tests/snapshots/<name>.png`, and they
//! come only from the `ui-test` job, never from rexec or a workstation:
//!
//! 1. Push the branch. A missing or differing baseline fails its test, and
//!    kittest writes `<name>.new.png` (plus `<name>.diff.png` for a
//!    difference) into `tests/snapshots/`. On failure the job uploads those
//!    files as the `ui-snapshots` artifact.
//! 2. Download it: `gh run download <run-id> -n ui-snapshots -D <dir>`.
//! 3. Review every image (and every `.diff.png` for a changed baseline),
//!    then rename each accepted `<name>.new.png` to `<name>.png` under
//!    `crates/fuzzy-dedupe-app/tests/snapshots/`. Never commit `.new.png`,
//!    `.diff.png` or `.old.png` files.
//! 4. Commit the baselines and push; the rerun must pass all 32.
//!
//! A runner-image or Mesa change that moves pixels is handled the same way,
//! in a reviewed PR (plan T22 recovery). Never set `UPDATE_SNAPSHOTS`: it
//! writes `<name>.png` directly from whatever adapter renders the run, which
//! is how a rexec or workstation render would become a baseline.

#![cfg(feature = "snapshot")]

mod snapshots {
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use egui::accesskit::{Role, Toggled};
    use egui::{Key, Theme, Vec2};
    use egui_kittest::Harness;
    use egui_kittest::kittest::{NodeT, Queryable};
    use fuzzy_dedupe_app::app::{AppDeps, Clock, FuzzyDedupeApp};
    use fuzzy_dedupe_app::measure::NoMeasurement;
    use fuzzy_dedupe_app::presentation::commands::UiIntent;
    use fuzzy_dedupe_app::presentation::state::{
        CleanupPhase, DialogKind, DialogSubject, MenuKind, PresentationState, RowId, ScanStage,
    };
    use fuzzy_dedupe_app::presentation::{events, projections};
    use fuzzy_dedupe_app::ui::dialogs::new_scan;
    use fuzzy_dedupe_domain::{CompareMode, MemberRole, QueueOrigin, ScanFolder, ScanSetup};
    use fuzzy_dedupe_engine::cleanup_sim::CleanupEvent;
    use fuzzy_dedupe_engine::fixtures::design;
    use fuzzy_dedupe_engine::service::{
        EngineConfig, EngineEvent, EngineRequest, ExecuteApprovedPlan, Origin, ReviewCommand,
    };
    use fuzzy_dedupe_platform::dirs::AppDirs;
    use fuzzy_dedupe_platform::fonts::{FontLookup, SystemFont};
    use fuzzy_dedupe_platform::log_retention::LogDate;
    use fuzzy_dedupe_platform::open::{Location, OpenHandle, Opener};
    use fuzzy_dedupe_platform::picker::{FolderPicker, PickHandle, PickOutcome, PickRequest};

    /// Step bound for every `Harness::run` here, in place of kittest's
    /// default of 4.
    ///
    /// A result from another thread (engine work, the folder picker, the
    /// opener) may arrive part-way through a run, and the widgets it draws for
    /// the first time each ask egui for up to two more frames (`widget_style`).
    /// On slow CI runners that overran 4 steps (PR #39, "would have settled
    /// after 6 steps"). 16 covers it with margin. A continuous repaint, from an
    /// animation or a repaint loop, still exhausts 16, so `run()` keeps
    /// catching runaway repaints.
    ///
    /// Each harness test file under `tests/` declares its own copy, because
    /// they share no module; keep the copies equal.
    const SETTLE_STEPS: u64 = 16;

    /// The egui input time, in seconds, of every captured frame. Far beyond
    /// any time a preparation reaches at kittest's 0.25 s step (it would
    /// need 4,000 steps), so pinning it never moves time backwards.
    const CAPTURE_TIME: f64 = 1000.0;

    const REFERENCE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/design/reference");

    const FULL: Vec2 = Vec2::new(1600.0, 960.0);
    const MINIMUM: Vec2 = Vec2::new(1024.0, 600.0);

    /// Review unique text's row in the design scenario (frame `16:932`): a
    /// fuzzy member that needs review against its counterpart.
    const FUZZY_ROW: &str = "Design_Spec_draft.txt";

    // ── Doubles ──────────────────────────────────────────────────────────

    /// No system fallback fonts: the embedded faces alone (D-007).
    struct EmbeddedFontsOnly;

    impl FontLookup for EmbeddedFontsOnly {
        fn fallback_fonts(&self) -> Vec<SystemFont> {
            Vec::new()
        }
    }

    struct NoOpener;

    impl Opener for NoOpener {
        fn open(&self, _: Location, notify: Box<dyn FnOnce() + Send>) -> OpenHandle {
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

    struct FixedClock;

    impl Clock for FixedClock {
        fn today(&self) -> LogDate {
            LogDate::new(2026, 11, 23).expect("valid date")
        }
    }

    /// A fresh home under the test target directory, unique per run because
    /// tests may not delete what an earlier run left behind. Nothing in it
    /// is ever read back: preferences start at their defaults every time.
    fn dirs_for(name: &str) -> AppDirs {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after 1970")
            .as_nanos();
        let root: PathBuf = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("snapshots")
            .join(format!("{name}-{nanos}"));
        let state = root.join("state");
        AppDirs::resolve(
            move |var| (var == "XDG_STATE_HOME").then(|| state.clone().into_os_string()),
            &root.join("home"),
        )
    }

    // ── The whole app, rendered through wgpu ────────────────────────────

    type Shell = Harness<'static, Option<FuzzyDedupeApp>>;

    /// The app on the design scenario at `size`, with the system theme
    /// reported as `theme`. The theme preference stays System, as on a
    /// first start, so the main menu shows the default override; the theme
    /// therefore comes in through the raw input's `system_theme`, which
    /// egui follows under that preference.
    fn launch(name: &str, size: Vec2, theme: Theme) -> Shell {
        let deps = AppDeps {
            dirs: dirs_for(name),
            opener: Box::new(NoOpener),
            picker: Box::new(DismissPicker),
            fonts: Box::new(EmbeddedFontsOnly),
            clock: Box::new(FixedClock),
            log_expiry: Box::new(|_| {}),
            engine: EngineConfig::default(),
            measure: Box::new(NoMeasurement),
        };
        let mut harness = Harness::builder()
            .with_max_steps(SETTLE_STEPS)
            .with_size(size)
            .with_theme(theme)
            .with_render_cursor(false)
            // kittest's default renderer: wgpu on its default test setup,
            // created at the first snapshot rather than here, so a missing
            // adapter fails only after the state has been reached and
            // asserted.
            .build_ui_state(
                |ui, app: &mut Option<FuzzyDedupeApp>| {
                    if let Some(app) = app.as_mut() {
                        app.run_logic(ui.ctx());
                        // kittest's `build_ui_state` hands over a Ui inside an
                        // 8-unit outer margin, which shifts and shrinks every
                        // panel. eframe gives `App::ui` the whole client area,
                        // so the app draws into a child spanning the content
                        // rect, as it would in the window. (`build_eframe`
                        // would avoid this, but needs kittest's `eframe`
                        // feature, which the manifest does not enable.)
                        // Same rig as `tests/layout.rs`'s `launch`.
                        let full = ui.ctx().content_rect();
                        let mut window = ui.new_child(egui::UiBuilder::new().max_rect(full));
                        app.draw(&mut window);
                    }
                },
                None,
            );
        harness.input_mut().system_theme = Some(theme);
        let app = FuzzyDedupeApp::new(&harness.ctx, deps).expect("engine starts");
        *harness.state_mut() = Some(app);
        harness.run();
        harness
    }

    fn app(h: &Shell) -> &FuzzyDedupeApp {
        h.state().as_ref().expect("app")
    }

    fn state(h: &Shell) -> &PresentationState {
        &app(h).state
    }

    fn state_mut(h: &mut Shell) -> &mut PresentationState {
        &mut h.state_mut().as_mut().expect("app").state
    }

    fn push(h: &mut Shell, intent: UiIntent) {
        h.state_mut().as_mut().expect("app").push_intent(intent);
    }

    /// Runs frames until `done` holds, for engine work on worker threads:
    /// at most 500 frames, 10 ms apart.
    fn run_until(h: &mut Shell, what: &str, done: impl Fn(&PresentationState) -> bool) {
        for _ in 0..500 {
            h.step();
            if done(state(h)) {
                h.run();
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("timed out waiting for {what}");
    }

    fn open_dialog(h: &mut Shell, kind: DialogKind) {
        push(
            h,
            UiIntent::OpenDialog {
                kind,
                subject: DialogSubject::None,
                invoker: None,
            },
        );
        h.run();
        assert_eq!(state(h).dialog.dialog().map(|d| d.kind), Some(kind));
    }

    /// Opens Review cleanup and waits for the engine's plan.
    fn open_cleanup(h: &mut Shell) {
        open_dialog(h, DialogKind::Cleanup);
        run_until(h, "the cleanup plan", |s| {
            s.operation.phase == CleanupPhase::Review
        });
    }

    #[track_caller]
    fn assert_shown(h: &Shell, text: &str) {
        assert!(
            h.query_all_by_label(text).next().is_some(),
            "{text:?} is not shown"
        );
    }

    // ── The 16 review states ─────────────────────────────────────────────

    /// One review frame of spec §11.1 (plan C-11), by its reference PNG.
    #[derive(Clone, Copy, Debug)]
    enum Frame {
        Findings,
        Compare,
        MatchingRules,
        NewScan,
        ReviewUniqueText,
        ReviewCleanup,
        BeforeFirstScan,
        Scanning,
        NoSearchMatches,
        CleanupRunning,
        CleanupFinished,
        FindingsMinimum,
        CompareMinimum,
        MainMenuOpen,
        KeyboardShortcuts,
        About,
    }

    impl Frame {
        /// The reference PNG's file stem in `docs/design/reference/`.
        const fn stem(self) -> &'static str {
            match self {
                Self::Findings => "13-648-findings",
                Self::Compare => "13-1304-compare",
                Self::MatchingRules => "16-735-matching-rules",
                Self::NewScan => "16-806-new-scan",
                Self::ReviewUniqueText => "16-932-review-unique-text",
                Self::ReviewCleanup => "16-886-review-cleanup",
                Self::BeforeFirstScan => "28-1399-findings-before-first-scan",
                Self::Scanning => "28-2171-findings-scanning",
                Self::NoSearchMatches => "30-2370-findings-no-search-matches",
                Self::CleanupRunning => "27-1390-cleanup-running",
                Self::CleanupFinished => "27-1533-cleanup-finished",
                Self::FindingsMinimum => "29-2214-findings-1024x600",
                Self::CompareMinimum => "29-3648-compare-1024x600",
                Self::MainMenuOpen => "44-3954-findings-main-menu-open",
                Self::KeyboardShortcuts => "44-3681-keyboard-shortcuts",
                Self::About => "44-3858-about",
            }
        }

        /// The frame's window size (spec §11.3).
        const fn size(self) -> Vec2 {
            match self {
                Self::FindingsMinimum | Self::CompareMinimum => MINIMUM,
                _ => FULL,
            }
        }

        /// Drives the freshly launched app into this state and asserts it
        /// is shown. Ends on a settled frame, except [`Self::Scanning`],
        /// whose progress bar repaints every frame.
        fn prepare(self, h: &mut Shell) {
            match self {
                Self::Findings => focus_frame_row(h),
                Self::FindingsMinimum => {
                    assert_eq!(state(h).scan.stage, ScanStage::Completed);
                    assert!(state(h).scan.results.is_some(), "design results");
                }
                Self::Compare | Self::CompareMinimum => open_design_pair(h),
                Self::MatchingRules => open_dialog(h, DialogKind::MatchingRules),
                Self::NewScan => open_seeded_new_scan(h),
                Self::KeyboardShortcuts => open_dialog(h, DialogKind::KeyboardShortcuts),
                Self::About => open_dialog(h, DialogKind::About),
                Self::ReviewUniqueText => open_unique_text_review(h),
                Self::ReviewCleanup => {
                    open_cleanup(h);
                    assert_shown(h, "Review cleanup");
                }
                Self::BeforeFirstScan => {
                    // The app always adopts the design scenario at start,
                    // so the before-first-scan state is a fresh
                    // presentation state, keeping only the preferences. No
                    // view reads the engine, and `Services::poll` mirrors
                    // the validation view only when its revision moves, so
                    // nothing refills the results.
                    let preferences = state(h).preferences.clone();
                    *state_mut(h) = PresentationState {
                        preferences,
                        ..PresentationState::default()
                    };
                    h.run();
                    assert_shown(h, "No findings yet");
                }
                Self::Scanning => scanning(h),
                Self::NoSearchMatches => {
                    let search = design::sample().search_without_matches;
                    state_mut(h).findings.search.clone_from(&search);
                    h.run();
                    assert_shown(h, &format!("No files match “{search}”"));
                }
                Self::CleanupRunning => cleanup_running(h),
                Self::CleanupFinished => {
                    open_cleanup(h);
                    // The accessibility action, not a pointer click: no
                    // pointer is left over the dialog, and the click does
                    // not depend on the host's scroll offset.
                    h.get_by_role_and_label(Role::Button, "Move 3 files to Trash")
                        .click_accesskit();
                    h.run();
                    run_until(h, "the run's end", |s| {
                        s.operation.phase == CleanupPhase::Finished
                    });
                    assert_shown(h, "Cleanup finished");
                }
                Self::MainMenuOpen => {
                    focus_frame_row(h);
                    h.key_press(Key::F10);
                    h.run();
                    assert_eq!(state(h).dialog.menu(), Some(MenuKind::Main));
                }
            }
        }
    }

    /// Frame 02 (`13:1304`): the design pair, opened on the target against
    /// its counterpart as Findings opens it, once its comparison is ready.
    fn open_design_pair(h: &mut Shell) {
        let pair = design::sample().last_compared.expect("design pair");
        push(
            h,
            UiIntent::OpenCompare {
                target: pair.b,
                counterpart: pair.a,
                return_row: None,
            },
        );
        run_until(h, "Compare's comparison", |s| {
            s.compare.comparison.aligned().is_some()
        });
    }

    /// The view state of frames `13:648`, `37:2466` and `44:3954`: the
    /// design scenario's expanded groups and its focused row,
    /// `Design_Spec_draft.txt` in G004, which enables the toolbar's Compare.
    ///
    /// The expanded set is the fixture's (`DesignSample::expanded_groups`):
    /// G003, G002, G004 and G009, plus G001 and G005, which the reference
    /// PNG also shows expanded; every other group stays collapsed. The row
    /// is focused in Findings' own state with `reveal_focus`, as a keyboard
    /// move leaves it, so the table scrolls it into view. Keyboard focus is
    /// deliberately not given to the table: the frames show the row's fill
    /// without the focus ring or the full-path tooltip that table focus
    /// draws (F12).
    fn focus_frame_row(h: &mut Shell) {
        assert_eq!(state(h).scan.stage, ScanStage::Completed);
        let sample = design::sample();
        let file = sample
            .focused_file
            .expect("the design scenario's focused row");
        let results = state(h).scan.results.clone().expect("design results");
        let group = results.group_of(file).expect("the focused row's group");
        let member = group.member(file).expect("the focused member");
        assert_eq!(member.name, FUZZY_ROW, "the frame focuses {FUZZY_ROW}");

        let expanded: BTreeSet<String> = results
            .groups
            .iter()
            .filter(|g| state(h).findings.expanded.contains(&g.id))
            .map(|g| g.label.clone())
            .collect();
        let frame: BTreeSet<String> = ["G001", "G005", "G003", "G002", "G004", "G009"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        assert_eq!(expanded, frame, "the frame's expanded groups, no others");
        assert_eq!(
            state(h).findings.expanded.len(),
            frame.len(),
            "no expanded group outside these results"
        );

        let findings = &mut state_mut(h).findings;
        findings.focused = Some(RowId::Member {
            group: group.id,
            file,
        });
        findings.reveal_focus = true;
        h.run();
        assert_shown(h, FUZZY_ROW);
        assert_shown(h, &projections::group_label(group));
        // The toolbar's Compare is the rightmost button of that name; the
        // rail's Compare entry shares the name.
        let compare = h
            .query_all_by_role_and_label(Role::Button, "Compare")
            .max_by(|a, b| a.rect().left().total_cmp(&b.rect().left()))
            .expect("the toolbar's Compare");
        assert!(
            !compare.accesskit_node().is_disabled(),
            "Compare is disabled with {FUZZY_ROW} focused"
        );
    }

    /// Frame `16:806`: New scan opened on a stored setup with three
    /// folders, the first protected, and the design fixture's explicit file
    /// types, committed through the same `SetScanSetup` intent Start scan
    /// sends. The empty-list "All plain-text files" state is
    /// `conform_popups`' business, not this frame's.
    ///
    /// The folders are crate-relative (`src`, `tests`, `assets`), resolved
    /// against the package root that cargo and nextest run tests from. The
    /// popup checks each folder's existence when it opens and marks a
    /// missing one in red, so the folders must exist; and the listed path
    /// is drawn verbatim, so an absolute path (`CARGO_MANIFEST_DIR`) would
    /// put the checkout location into the baseline's pixels. The fixture's
    /// own `~/Documents/…` folders exist nowhere and would all show as not
    /// found.
    fn open_seeded_new_scan(h: &mut Shell) {
        const FOLDERS: [(&str, bool); 3] = [("src", true), ("tests", false), ("assets", false)];
        let file_types = design::sample().new_scan.file_types_shown;
        assert!(!file_types.is_empty(), "an explicit file-type list");
        let setup = ScanSetup {
            folders: FOLDERS
                .iter()
                .map(|(path, protect)| ScanFolder {
                    path: PathBuf::from(path),
                    protect: *protect,
                })
                .collect(),
            file_types: file_types.clone(),
        };
        push(h, UiIntent::SetScanSetup(setup.clone()));
        h.run();
        assert_eq!(state(h).preferences.scan_setup, setup);
        open_dialog(h, DialogKind::NewScan);

        for (path, protect) in FOLDERS {
            let shown = new_scan::shown_path(Path::new(path));
            assert_shown(h, &shown);
            let toggled = h
                .get_by_role_and_label(Role::CheckBox, &format!("Protect files in {shown}"))
                .accesskit_node()
                .toggled();
            let expected = if protect {
                Toggled::True
            } else {
                Toggled::False
            };
            assert_eq!(toggled, Some(expected), "Protect files in {shown}");
        }
        assert!(
            h.query_by_label_contains(new_scan::MISSING_FOLDER)
                .is_none(),
            "a seeded folder is shown as not found"
        );
        let summary = file_types
            .iter()
            .map(|t| format!(".{t}"))
            .collect::<Vec<_>>()
            .join("   ");
        assert_shown(h, &summary);
        assert!(
            h.query_by_label(new_scan::ALL_FILE_TYPES).is_none(),
            "the explicit list is shown as {:?}",
            new_scan::ALL_FILE_TYPES
        );
    }

    /// Frame `28:2171`: a scan running after Start scan cleared the queue
    /// and the results it replaces (FR-026).
    ///
    /// Not reached through the real Start scan: the simulated scan runs on
    /// its own threads for a wall time the rig does not control
    /// (`scan_sim::job`, files × a fixed cost), then completes, and its
    /// progress events carry the measured counts and elapsed time, so a
    /// capture could neither hold the running state nor show stable text.
    /// Nor does Start clear the presentation's results: they stay until the
    /// scan's `Completed` event replaces them (`events::install_results`).
    /// So the queue is cleared by Start scan's own command, `ClearForNewScan`,
    /// through the review service, and the rig then drops the results and
    /// applies the design sample's progress.
    fn scanning(h: &mut Shell) {
        assert!(state(h).queue.queued_count() > 0, "the design queue");
        push(
            h,
            UiIntent::Review {
                command: ReviewCommand::ClearForNewScan,
                invoker: None,
            },
        );
        h.run();
        assert_eq!(state(h).queue.queued_count(), 0, "Start's clear ran");

        let sample = design::sample();
        let s = state_mut(h);
        s.scan.results = None;
        s.scan.stage = ScanStage::Running;
        s.scan.progress = Some(sample.scan_progress);
        s.findings.revision += 1;
        // The animated progress bar never settles; a fixed number of
        // frames draws the scanning view.
        h.run_steps(4);
        assert_shown(h, "Scanning…");
        assert!(state(h).scan.results.is_none(), "results cleared");
        let review = h.get_by_role_and_label(Role::Button, "Review cleanup (0)");
        assert!(
            review.accesskit_node().is_disabled(),
            "Review cleanup (0) is enabled"
        );
    }

    /// Frame `16:932`: a queue request on the fuzzy row, which answers
    /// needs-review and opens the popup, once its comparison is ready.
    fn open_unique_text_review(h: &mut Shell) {
        let results = state(h).scan.results.clone().expect("results");
        let member = results
            .groups
            .iter()
            .flat_map(|g| g.members.iter())
            .find(|m| m.name == FUZZY_ROW)
            .expect("the fuzzy row in the design scenario");
        let MemberRole::Member(link) = member.role else {
            panic!("{FUZZY_ROW} is a reference");
        };
        push(
            h,
            UiIntent::Review {
                command: ReviewCommand::RequestQueue {
                    file: member.file,
                    counterpart: link.counterpart,
                    origin: QueueOrigin::Shortcut,
                    reviewed: None,
                },
                invoker: None,
            },
        );
        h.run();
        let dialog = state(h).dialog.dialog().expect("a popup is open");
        let DialogSubject::UniqueText(subject) = dialog.subject else {
            panic!("Review unique text expected, got {:?}", dialog.kind);
        };
        let key = subject.comparison(CompareMode::Original);
        run_until(h, "the popup's comparison", move |s| {
            s.compare.comparison.matches(&key) && s.compare.comparison.aligned().is_some()
        });
    }

    /// Frame `27:1390`. A simulated run ends within the frame it starts, so
    /// its mid-run state cannot be caught from the engine; as in
    /// `tests/cleanup_dialog.rs`, the confirmation is noted as admitted
    /// without reaching the engine and the design sample's running item
    /// states are applied as cleanup events. The engine never ran the plan,
    /// so no event of its own overwrites them.
    fn cleanup_running(h: &mut Shell) {
        open_cleanup(h);
        let s = state_mut(h);
        let plan = s.operation.revision.expect("a plan");
        let items = s.operation.items.clone();
        let request = EngineRequest::ExecuteApprovedPlan(ExecuteApprovedPlan { plan });
        events::note_submitted(s, Origin::Plan(plan), &request);
        for (item, status) in items.iter().zip(&design::sample().cleanup_running) {
            let mut changed = *item;
            changed.status = *status;
            events::apply(
                s,
                EngineEvent::Cleanup {
                    plan,
                    event: CleanupEvent::ItemChanged(changed),
                },
            );
        }
        h.run();
        assert_shown(h, "Moving files to Trash");
        assert_shown(h, "1 of 3 files moved");
    }

    /// Prepares `frame` in `theme` and compares it with its baseline. A
    /// mismatch is recorded in the harness, which panics when dropped at
    /// the end of the test.
    fn shoot(frame: Frame, theme: Theme) {
        let suffix = match theme {
            Theme::Light => "light",
            Theme::Dark => "dark",
        };
        let name = format!("{}-{suffix}", frame.stem());
        let mut h = launch(&name, frame.size(), theme);
        frame.prepare(&mut h);
        h.remove_cursor();
        h.input_mut().time = Some(CAPTURE_TIME);
        // Two frames at the pinned time: the first lays out with it, the
        // second is drawn from that settled layout.
        h.run_steps(2);
        h.snapshot(&name);
    }

    /// Two tests per frame, `<module>::light` and `<module>::dark`, so the
    /// UI-test job can count exactly 32 by name.
    macro_rules! frames {
        ($($module:ident => $frame:ident,)*) => {
            $(
                mod $module {
                    #[test]
                    fn light() {
                        super::shoot(super::Frame::$frame, egui::Theme::Light);
                    }

                    #[test]
                    fn dark() {
                        super::shoot(super::Frame::$frame, egui::Theme::Dark);
                    }
                }
            )*

            const FRAMES: &[Frame] = &[$(Frame::$frame),*];
        };
    }

    frames! {
        findings_13_648 => Findings,
        compare_13_1304 => Compare,
        matching_rules_16_735 => MatchingRules,
        new_scan_16_806 => NewScan,
        review_unique_text_16_932 => ReviewUniqueText,
        review_cleanup_16_886 => ReviewCleanup,
        before_first_scan_28_1399 => BeforeFirstScan,
        scanning_28_2171 => Scanning,
        no_search_matches_30_2370 => NoSearchMatches,
        cleanup_running_27_1390 => CleanupRunning,
        cleanup_finished_27_1533 => CleanupFinished,
        findings_1024x600_29_2214 => FindingsMinimum,
        compare_1024x600_29_3648 => CompareMinimum,
        main_menu_open_44_3954 => MainMenuOpen,
        keyboard_shortcuts_44_3681 => KeyboardShortcuts,
        about_44_3858 => About,
    }

    /// The snapshot set is exactly the review frames that are not dark
    /// variants (C-11: 19 frames, 3 of them dark): every non-dark reference
    /// PNG has a frame here, and every frame here has its PNG.
    #[test]
    fn the_frames_are_the_non_dark_review_frames() {
        let references: BTreeSet<String> = std::fs::read_dir(REFERENCE_DIR)
            .expect("docs/design/reference is readable")
            .map(|entry| entry.expect("a directory entry").path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "png"))
            .filter_map(|path| Some(path.file_stem()?.to_str()?.to_owned()))
            .filter(|stem| !stem.ends_with("-dark"))
            .collect();
        let frames: BTreeSet<String> = FRAMES.iter().map(|f| f.stem().to_owned()).collect();
        assert_eq!(FRAMES.len(), frames.len(), "no frame is listed twice");
        assert_eq!(frames, references);
        assert_eq!(frames.len(), 16, "spec §11.1's 16 non-dark review frames");
    }
}
