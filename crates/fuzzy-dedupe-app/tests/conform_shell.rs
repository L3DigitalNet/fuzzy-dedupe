// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! PV-T40-001, leg S (plan T40, items S1 to S6): the shell's toolbar
//! composition, its shared right-hand items, the rail's Compare entry, the
//! main menu's item pitch and the status bar conform to the review frames at
//! 1600 × 960 and 1024 × 600, each at zoom 100% and 200%, light and dark.
//!
//! Every test sits in the `conform_shell` module and carries the `conform_`
//! prefix, so `-E 'test(conform_)'` selects them with the other legs'. The
//! app is built through `FuzzyDedupeApp::with_startup` on the harness's own
//! context and draws into the full content rect, as `tests/layout.rs` does;
//! zoom is set through the preferences, which the app applies with
//! `theme::apply_zoom`. At 200% a 1600 × 960 window gives the UI 800 × 480
//! points and a 1024 × 600 one 512 × 300. Geometry comes from the
//! accessibility tree, and the visible Review cleanup label from the frame's
//! painted text, because its accessible name stays the full label.
//!
//! Requirements: fresh preference and log directories under
//! `CARGO_TARGET_TMPDIR`, unique per run, which the app itself may write
//! into.

mod conform_shell {
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use egui::accesskit::Role;
    use egui::{Key, Rect, Vec2};
    use egui_kittest::Harness;
    use egui_kittest::kittest::{NodeT, Queryable};
    use fuzzy_dedupe_app::app::{AppDeps, Clock, FuzzyDedupeApp};
    use fuzzy_dedupe_app::measure::{NoMeasurement, StartupScenario};
    use fuzzy_dedupe_app::presentation::commands::IntentQueue;
    use fuzzy_dedupe_app::presentation::commands::UiIntent;
    use fuzzy_dedupe_app::presentation::projections;
    use fuzzy_dedupe_app::presentation::state::{
        PresentationState, RowId, Severity, StatusMessage, ThemeOverride, WorkspaceState,
    };
    use fuzzy_dedupe_app::ui::compare::MORE_ACTIONS_NAME;
    use fuzzy_dedupe_app::ui::glyphs;
    use fuzzy_dedupe_app::ui::shell::{
        self, MAIN_MENU_ENTRIES, SETTINGS_NAME, WORKSPACE_TOOLBAR_NAME,
    };
    use fuzzy_dedupe_app::ui::theme::{self, Zoom};
    use fuzzy_dedupe_app::ui::widgets::status;
    use fuzzy_dedupe_domain::FileId;
    use fuzzy_dedupe_engine::fixtures::{ScenarioKind, design};
    use fuzzy_dedupe_engine::service::EngineConfig;
    use fuzzy_dedupe_platform::dirs::AppDirs;
    use fuzzy_dedupe_platform::fonts::SystemFontLookup;
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

    /// A-010: numeric layout assertions allow ±1 logical unit.
    const TOLERANCE: f32 = 1.0;

    /// The four size and zoom combinations T40's outcome names.
    const COMBOS: [(Vec2, u16); 4] = [
        (Vec2::new(1600.0, 960.0), 100),
        (Vec2::new(1600.0, 960.0), 200),
        (Vec2::new(1024.0, 600.0), 100),
        (Vec2::new(1024.0, 600.0), 200),
    ];

    // ── Doubles ──────────────────────────────────────────────────────────

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
            LogDate::new(2026, 11, 24).expect("valid date")
        }
    }

    // ── Harness ──────────────────────────────────────────────────────────

    struct Slot {
        app: Option<FuzzyDedupeApp>,
    }

    type Shell = Harness<'static, Slot>;

    /// A fresh home under the test's own target directory, unique per run
    /// because tests may not delete what an earlier run left behind.
    fn dirs_for(name: &str) -> AppDirs {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after 1970")
            .as_nanos();
        let root: PathBuf = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("conform_shell")
            .join(format!("{name}-{nanos}"));
        let state = root.join("state");
        AppDirs::resolve(
            move |var| (var == "XDG_STATE_HOME").then(|| state.clone().into_os_string()),
            &root.join("home"),
        )
    }

    fn launch(name: &str, kind: ScenarioKind, size: Vec2, zoom: u16) -> Shell {
        let deps = AppDeps {
            dirs: dirs_for(name),
            opener: Box::new(NoOpener),
            picker: Box::new(DismissPicker),
            fonts: Box::new(SystemFontLookup),
            clock: Box::new(FixedClock),
            log_expiry: Box::new(|_| {}),
            engine: EngineConfig::default(),
            measure: Box::new(NoMeasurement),
        };
        let mut harness = Harness::builder()
            .with_max_steps(SETTLE_STEPS)
            .with_size(size)
            .build_ui_state(
                |ui, slot: &mut Slot| {
                    if let Some(app) = slot.app.as_mut() {
                        app.run_logic(ui.ctx());
                        // kittest's `build_ui_state` hands over a Ui inside an
                        // 8-unit outer margin; eframe gives `App::ui` the whole
                        // client area, so the app draws into a child spanning
                        // the content rect, as `tests/layout.rs` does.
                        let full = ui.ctx().content_rect();
                        let mut window = ui.new_child(egui::UiBuilder::new().max_rect(full));
                        app.draw(&mut window);
                    }
                },
                Slot { app: None },
            );
        let app = FuzzyDedupeApp::with_startup(&harness.ctx, deps, StartupScenario::Fixture(kind))
            .expect("engine starts");
        glyphs::install(&harness.ctx);
        harness.state_mut().app = Some(app);
        harness.run();
        run_until(&mut harness, "the scenario's results", |s| {
            s.scan.results.is_some()
        });
        state_mut(&mut harness).preferences.zoom =
            Zoom::from_percent(zoom).expect("on the zoom grid");
        settle(&mut harness);
        harness
    }

    fn launch_design(name: &str, size: Vec2, zoom: u16) -> Shell {
        launch(name, ScenarioKind::Design, size, zoom)
    }

    fn state(h: &Shell) -> &PresentationState {
        &h.state().app.as_ref().expect("app").state
    }

    fn state_mut(h: &mut Shell) -> &mut PresentationState {
        &mut h.state_mut().app.as_mut().expect("app").state
    }

    /// Enough frames for a preference change (zoom, theme) to reach egui
    /// and the layout to settle on it.
    fn settle(h: &mut Shell) {
        for _ in 0..6 {
            h.step();
        }
        h.run();
    }

    fn push(h: &mut Shell, intent: UiIntent) {
        h.state_mut().app.as_mut().expect("app").push_intent(intent);
        settle(h);
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

    /// The design pair (frame `13:1304`): the draft against its reference.
    fn design_pair() -> (FileId, FileId) {
        let pair = design::sample().last_compared.expect("design pair");
        (pair.b, pair.a)
    }

    fn open_design_pair(h: &mut Shell) {
        let (target, counterpart) = design_pair();
        push(
            h,
            UiIntent::OpenCompare {
                target,
                counterpart,
                return_row: None,
            },
        );
        run_until(h, "the design pair's comparison", move |s| {
            s.compare
                .comparison
                .aligned()
                .is_some_and(|a| a.b.file == target)
        });
        settle(h);
    }

    /// The client area in points, which zoom shrinks.
    fn client<S>(h: &Harness<'_, S>) -> Rect {
        h.ctx.content_rect()
    }

    fn inside(outer: Rect, inner: Rect) -> bool {
        outer.expand(TOLERANCE).contains_rect(inner)
    }

    /// Whether `a` and `b` share area beyond the ±1 tolerance.
    fn overlap(a: Rect, b: Rect) -> bool {
        let both = a.intersect(b);
        both.width() > TOLERANCE && both.height() > TOLERANCE
    }

    fn case(size: Vec2, zoom: u16, theme: ThemeOverride, workspace: WorkspaceState) -> String {
        format!("{}×{} at {zoom}%, {theme:?}, {workspace:?}", size.x, size.y)
    }

    /// The Review cleanup button: its accessible name is always the full
    /// "Review cleanup (N)", whatever the visible label.
    fn review_button<S>(h: &Harness<'_, S>, queued: usize) -> Rect {
        h.get_by_role_and_label(Role::Button, &format!("Review cleanup ({queued})"))
            .rect()
    }

    fn painted_texts(h: &Shell) -> Vec<String> {
        fn walk(shape: &egui::Shape, out: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(text) => out.push(text.galley.text().to_owned()),
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, out)),
                _ => {}
            }
        }
        let mut out = Vec::new();
        for clipped in &h.output().shapes {
            walk(&clipped.shape, &mut out);
        }
        out
    }

    /// The rail's Compare entry: the leftmost button of that name (the
    /// Findings toolbar's Compare action has the same name).
    fn rail_compare_disabled(h: &Shell) -> bool {
        h.query_all_by_role_and_label(Role::Button, "Compare")
            .min_by(|a, b| a.rect().left().total_cmp(&b.rect().left()))
            .expect("the rail's Compare entry")
            .accesskit_node()
            .is_disabled()
    }

    /// The text labels drawn inside the status bar, left to right.
    fn status_labels(h: &Shell) -> Vec<(String, Rect)> {
        let bar = h.get_by_label(status::ACCESSIBLE_NAME).rect();
        let mut labels: Vec<(String, Rect)> = h
            .query_all_by_role(Role::Label)
            .filter(|node| bar.contains(node.rect().center()))
            .map(|node| {
                (
                    {
                        let node = node.accesskit_node();
                        node.label().or_else(|| node.value()).unwrap_or_default()
                    },
                    node.rect(),
                )
            })
            .collect();
        labels.sort_by(|a, b| a.1.left().total_cmp(&b.1.left()));
        labels
    }

    // ── S1: shared items first, the workspace gets the rest ─────────────

    /// The gear and Review cleanup lie inside the client area, never
    /// overlap, and the region handed to the workspace toolbar ends before
    /// they begin. The label shortens to "Cleanup (N)" only when the
    /// workspace's toolbar cannot fit beside the full label (Q6).
    #[test]
    fn conform_shell_shared_items_fit_and_the_workspace_budget_ends_before_them() {
        for (size, zoom) in COMBOS {
            let mut h = launch_design("shared", size, zoom);
            for workspace in [WorkspaceState::Findings, WorkspaceState::Compare] {
                if workspace == WorkspaceState::Compare {
                    open_design_pair(&mut h);
                }
                for theme in [ThemeOverride::Light, ThemeOverride::Dark] {
                    push(&mut h, UiIntent::SetTheme(theme));
                    let what = case(size, zoom, theme, workspace);
                    let area = client(&h);
                    let gear = h.get_by_role_and_label(Role::Button, SETTINGS_NAME).rect();
                    let review = review_button(&h, 3);
                    assert!(inside(area, gear), "{what}: gear {gear:?} off {area:?}");
                    assert!(
                        inside(area, review),
                        "{what}: Review cleanup {review:?} off {area:?}"
                    );
                    assert!(
                        !overlap(gear, review),
                        "{what}: gear {gear:?} overlaps Review cleanup {review:?}"
                    );
                    assert!(
                        (gear.height() - 30.0).abs() <= TOLERANCE
                            && (review.height() - 30.0).abs() <= TOLERANCE,
                        "{what}: shared controls are not 30 high"
                    );

                    let budget = h
                        .get_by_role_and_label(Role::Group, WORKSPACE_TOOLBAR_NAME)
                        .rect();
                    let shared_left = gear.left().min(review.left());
                    assert!(
                        budget.right() <= shared_left + TOLERANCE,
                        "{what}: workspace budget {budget:?} runs into the shared items at {shared_left}"
                    );

                    // Q6: the label shortens only when the workspace
                    // cannot fit its items at their minimum beside the full
                    // label (`shell::toolbar_row`). Frame `29:2214` keeps it
                    // in Findings at 1024 × 600, where search narrows
                    // instead. Elsewhere whichever fits wins, so the rule's
                    // consequence is asserted: with the full label showing,
                    // nothing of the workspace's toolbar has moved into its
                    // overflow menu.
                    let texts = painted_texts(&h);
                    let long = texts.iter().any(|t| t == "Review cleanup (3)");
                    let short = texts.iter().any(|t| t == "Cleanup (3)");
                    assert!(
                        long != short,
                        "{what}: full label painted {long}, short label painted {short}"
                    );
                    if zoom == 100 && (size.x >= 1600.0 || workspace == WorkspaceState::Findings) {
                        assert!(long, "{what}: the frames keep the full label");
                    }
                    if long {
                        let overflowed = match workspace {
                            WorkspaceState::Findings => {
                                h.query_all_by_role_and_label(Role::Button, "Compare")
                                    .count()
                                    < 2
                            }
                            WorkspaceState::Compare => h
                                .query_by_role_and_label(Role::Button, MORE_ACTIONS_NAME)
                                .is_some(),
                        };
                        assert!(
                            !overflowed,
                            "{what}: the full label shows while the workspace overflows"
                        );
                    }
                }
            }
        }
    }

    /// What the stub workspace toolbar saw and took.
    #[derive(Default)]
    struct Stub {
        /// False until the fonts are installed: the builder runs a first
        /// frame before that, and laying out text then panics.
        ready: bool,
        row: Option<Rect>,
        taken: Option<Rect>,
    }

    /// The S1 contract with legs F and C: a workspace toolbar that takes
    /// its whole `ui.available_width()` ends before the shared items begin,
    /// and starts at the row's left edge, so that width is the row less the
    /// shared items. The stub stands in for the Findings and Compare
    /// toolbars, whose own fit their legs prove; it is placed where the
    /// shell places the row (rail, then the 12-unit work-area inset).
    #[test]
    fn conform_shell_a_workspace_toolbar_filling_its_budget_stops_before_the_shared_items() {
        for (size, zoom) in COMBOS {
            let mut h = Harness::builder()
                .with_max_steps(SETTLE_STEPS)
                .with_size(size)
                .build_ui_state(
                    |ui, stub: &mut Stub| {
                        if !stub.ready {
                            return;
                        }
                        let full = ui.ctx().content_rect();
                        let rail = shell::rail_width(full.width(), false);
                        let work = Rect::from_min_max(
                            egui::pos2(full.left() + rail, full.top()),
                            full.max,
                        )
                        .shrink(shell::WORK_INSET);
                        let mut window = ui.new_child(egui::UiBuilder::new().max_rect(work));
                        window.spacing_mut().item_spacing.y = 0.0;
                        let mut intents = IntentQueue::default();
                        shell::toolbar_row(
                            &mut window,
                            "Review cleanup (3)",
                            3,
                            // No workspace minimum: the stub takes any budget.
                            0.0,
                            &mut intents,
                            |ui, _| {
                                stub.row = Some(ui.max_rect());
                                let (taken, _) = ui.allocate_exact_size(
                                    Vec2::new(ui.available_width(), shell::CONTROL_HEIGHT),
                                    egui::Sense::hover(),
                                );
                                stub.taken = Some(taken);
                            },
                        );
                    },
                    Stub::default(),
                );
            theme::install(&h.ctx, Vec::new());
            glyphs::install(&h.ctx);
            theme::apply_zoom(&h.ctx, Zoom::from_percent(zoom).expect("on the zoom grid"));
            h.state_mut().ready = true;
            for _ in 0..4 {
                h.step();
            }
            h.run();

            let what = format!("{}×{} at {zoom}%", size.x, size.y);
            let area = client(&h);
            let rail = shell::rail_width(area.width(), false);
            let taken = h.state().taken.expect("the stub drew");
            let gear = h.get_by_role_and_label(Role::Button, SETTINGS_NAME).rect();
            let review = review_button(&h, 3);
            let shared_left = gear.left().min(review.left());
            assert!(
                taken.right() <= shared_left + TOLERANCE,
                "{what}: a full-budget toolbar {taken:?} runs into the shared items at {shared_left}"
            );
            assert!(
                (taken.left() - (area.left() + rail + shell::WORK_INSET)).abs() <= TOLERANCE,
                "{what}: the budget {taken:?} does not start at the row's left edge"
            );
            assert!(
                taken.width() > 0.0 && inside(area, gear) && inside(area, review),
                "{what}: budget {taken:?}, gear {gear:?}, Review cleanup {review:?} in {area:?}"
            );
            let budget = h
                .get_by_role_and_label(Role::Group, WORKSPACE_TOOLBAR_NAME)
                .rect();
            assert!(
                (budget.width() - taken.width()).abs() <= TOLERANCE,
                "{what}: the toolbar's region {budget:?} is not its budget {taken:?}"
            );
            assert!(h.state().row.is_some());
        }
    }

    // ── D2: the toolbar's last gap is 8 like every other ────────────────

    /// The workspace toolbar's last control sits one 8-unit toolbar gap
    /// before Review cleanup, the gap every other neighbouring pair of
    /// toolbar controls has (§11.3). Pins the T40 round-2 finding of a 4-unit
    /// gap (x = 1413–1417 at 1600 wide). At 100% only: at 200% the
    /// workspace's trailing control may have moved into its overflow menu,
    /// which leaves a stretch, not a gap.
    #[test]
    fn conform_shell_the_last_workspace_control_sits_one_gap_before_review_cleanup() {
        for size in [Vec2::new(1600.0, 960.0), Vec2::new(1024.0, 600.0)] {
            let mut h = launch_design("last_gap", size, 100);
            for workspace in [WorkspaceState::Findings, WorkspaceState::Compare] {
                if workspace == WorkspaceState::Compare {
                    open_design_pair(&mut h);
                }
                let what = case(size, 100, ThemeOverride::Light, workspace);
                let review = review_button(&h, 3);
                let gear = h.get_by_role_and_label(Role::Button, SETTINGS_NAME).rect();
                assert!(
                    (gear.width() - 30.0).abs() <= TOLERANCE,
                    "{what}: the gear {gear:?} is not 30 wide"
                );
                assert!(
                    (gear.left() - review.right() - 8.0).abs() <= TOLERANCE,
                    "{what}: Review cleanup {review:?} is not 8 before the gear {gear:?}"
                );
                let budget = h
                    .get_by_role_and_label(Role::Group, WORKSPACE_TOOLBAR_NAME)
                    .rect();
                let last = h
                    .query_all_by_role(Role::Button)
                    .map(|n| n.rect())
                    .filter(|r| {
                        r.center().y > budget.top()
                            && r.center().y < budget.bottom()
                            && r.left() >= budget.left() - TOLERANCE
                            && r.right() <= review.left() + TOLERANCE
                            && *r != review
                    })
                    .max_by(|a, b| a.right().total_cmp(&b.right()))
                    .unwrap_or_else(|| panic!("{what}: no workspace control"));
                let gap = review.left() - last.right();
                assert!(
                    (gap - 8.0).abs() <= TOLERANCE,
                    "{what}: last control {last:?} sits {gap} before Review cleanup {review:?}"
                );
            }
        }
    }

    // ── S2: Review cleanup (0) is disabled ──────────────────────────────

    #[test]
    fn conform_shell_review_cleanup_is_disabled_when_nothing_is_queued() {
        let h = launch(
            "review_empty",
            ScenarioKind::Empty,
            Vec2::new(1600.0, 960.0),
            100,
        );
        assert_eq!(state(&h).queue.queued_count(), 0);
        let review = h.get_by_role_and_label(Role::Button, "Review cleanup (0)");
        assert!(
            review.accesskit_node().is_disabled(),
            "Review cleanup (0) is enabled (frame 28:1399 disables it)"
        );

        let h = launch_design("review_queued", Vec2::new(1600.0, 960.0), 100);
        let review = h.get_by_role_and_label(Role::Button, "Review cleanup (3)");
        assert!(!review.accesskit_node().is_disabled());
    }

    // ── S3: the rail's Compare needs a pair ─────────────────────────────

    /// Spec §11.2: the rail's Compare shows the focused row against its
    /// counterpart, or the last valid pair, and is disabled when neither
    /// exists.
    #[test]
    fn conform_shell_rail_compare_is_disabled_until_a_pair_exists() {
        let mut h = launch_design("rail_compare", Vec2::new(1600.0, 960.0), 100);
        assert_eq!(state(&h).compare.pair, None);
        assert_eq!(state(&h).findings.focused, None);
        assert!(
            rail_compare_disabled(&h),
            "the rail's Compare is enabled with no pair"
        );

        // A focused member with a counterpart is a pair.
        let (target, _) = design_pair();
        let results = state(&h).scan.results.clone().expect("results");
        let group = results.group_of(target).expect("the draft's group").id;
        state_mut(&mut h).findings.focused = Some(RowId::Member {
            group,
            file: target,
        });
        settle(&mut h);
        assert!(!rail_compare_disabled(&h), "focused member: disabled");

        // A group row has no counterpart, and there is no last pair yet.
        state_mut(&mut h).findings.focused = Some(RowId::Group(group));
        settle(&mut h);
        assert!(
            rail_compare_disabled(&h),
            "group row, no last pair: enabled"
        );

        // Once a pair was shown, the last valid pair keeps it enabled.
        open_design_pair(&mut h);
        push(&mut h, UiIntent::SwitchWorkspace(WorkspaceState::Findings));
        state_mut(&mut h).findings.focused = Some(RowId::Group(group));
        settle(&mut h);
        assert!(!rail_compare_disabled(&h), "last valid pair: disabled");
    }

    // ── S4: main-menu pitch ─────────────────────────────────────────────

    /// Frame `44:3954`: Keyboard shortcuts…, Open log folder and About sit
    /// 28 apart, egui's item spacing not stacked on the 28-high rows.
    #[test]
    fn conform_shell_main_menu_items_sit_at_a_28_unit_pitch() {
        let mut h = launch_design("menu_pitch", Vec2::new(1600.0, 960.0), 100);
        h.key_press(Key::F10);
        settle(&mut h);
        let tops: Vec<f32> = MAIN_MENU_ENTRIES[2..5]
            .iter()
            .map(|entry| h.get_by_role_and_label(Role::Button, entry).rect().top())
            .collect();
        for pair in tops.windows(2) {
            let pitch = pair[1] - pair[0];
            assert!(
                (pitch - 28.0).abs() <= TOLERANCE,
                "main-menu pitch {pitch}, frame 44:3954 draws 28"
            );
        }
    }

    // ── S5: one status line that never overprints ───────────────────────

    /// The status bar is one 26-high line inside the client area; its left
    /// and right texts never intersect and never leave the bar, in both
    /// workspaces, with the counts and with a long status message.
    #[test]
    fn conform_shell_status_bar_is_one_line_that_never_overprints() {
        for (size, zoom) in COMBOS {
            let mut h = launch_design("status", size, zoom);
            for workspace in [WorkspaceState::Findings, WorkspaceState::Compare] {
                if workspace == WorkspaceState::Compare {
                    open_design_pair(&mut h);
                }
                for theme in [ThemeOverride::Light, ThemeOverride::Dark] {
                    push(&mut h, UiIntent::SetTheme(theme));
                    for message in [false, true] {
                        state_mut(&mut h).status = message.then(|| StatusMessage {
                            severity: Severity::Warning,
                            text: "The comparison changed; compare again before queueing. \
                                   Three files were unqueued because their counterpart changed."
                                .to_owned(),
                        });
                        settle(&mut h);
                        let what = format!(
                            "{}, {}",
                            case(size, zoom, theme, workspace),
                            if message { "message" } else { "counts" }
                        );
                        let area = client(&h);
                        let bar = h.get_by_label(status::ACCESSIBLE_NAME).rect();
                        assert!(inside(area, bar), "{what}: bar {bar:?} off {area:?}");
                        assert!(
                            (bar.height() - 26.0).abs() <= TOLERANCE,
                            "{what}: bar is {} high",
                            bar.height()
                        );
                        let labels = status_labels(&h);
                        assert_eq!(labels.len(), 2, "{what}: status labels {labels:?}");
                        for (text, rect) in &labels {
                            assert!(
                                inside(bar, *rect),
                                "{what}: \"{text}\" at {rect:?} leaves the bar {bar:?}"
                            );
                            assert!(
                                rect.height() <= 26.0,
                                "{what}: \"{text}\" wraps to {} high",
                                rect.height()
                            );
                        }
                        let ((left, l), (right, r)) = (&labels[0], &labels[1]);
                        assert!(
                            !overlap(*l, *r) && l.right() <= r.left() + TOLERANCE,
                            "{what}: \"{left}\" {l:?} overprints \"{right}\" {r:?}"
                        );
                    }
                }
            }
        }
    }

    // ── S6: the Compare status line ─────────────────────────────────────

    /// Frame `13:1304`: "G004 · 3 files · A is the comparison reference" on
    /// the left, "Read-only · Unique text on both sides · 3 queued" on the
    /// right. The group, its size, the unique-text summary and the queue
    /// count come from state, not from the frame's sample values.
    #[test]
    fn conform_shell_compare_status_line_states_the_pair() {
        let mut h = launch_design("compare_status", Vec2::new(1600.0, 960.0), 100);
        let findings = projections::status_line(state(&h));
        open_design_pair(&mut h);

        let s = state(&h);
        let results = s.scan.results.clone().expect("results");
        let pair = s.compare.pair.expect("the pair");
        let group = results
            .groups
            .iter()
            .find(|g| g.id == pair.group)
            .expect("the pair's group");
        let aligned = s.compare.comparison.aligned().expect("ready");
        let unique = match (aligned.only_in_a.count() > 0, aligned.only_in_b.count() > 0) {
            (true, true) => "Unique text on both sides",
            (true, false) => "Unique text only in A",
            (false, true) => "Unique text only in B",
            (false, false) => "No unique text",
        };
        let sep = "   ·   ";
        let expected = projections::StatusLine {
            left: format!(
                "{} · {} files{sep}A is the comparison reference",
                group.label,
                group.members.len()
            ),
            right: format!(
                "Read-only{sep}{unique}{sep}{} queued",
                s.queue.queued_count()
            ),
        };
        assert_eq!(projections::status_line(s), expected);
        // The design fixture is frame 13:1304's pair.
        assert_eq!(
            expected.left,
            "G004 · 3 files   ·   A is the comparison reference"
        );
        assert_eq!(
            expected.right,
            "Read-only   ·   Unique text on both sides   ·   3 queued"
        );
        h.get_by_label(&expected.left);
        h.get_by_label(&expected.right);

        // Findings keeps its own line.
        push(&mut h, UiIntent::SwitchWorkspace(WorkspaceState::Findings));
        assert_eq!(projections::status_line(state(&h)), findings);
    }
}
