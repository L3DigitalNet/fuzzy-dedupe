// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! PV-T40-001, leg C (plan T40, items C1 to C6): Compare's toolbar, change
//! row, identity header and the Original | Normalized switch conform to the
//! review frames at 1600 × 960 and 1024 × 600, each at zoom 100% and 200%,
//! light and dark; the identity header gives keyboard access to the full
//! path and Copy path; Wrap starts on unless a stored preference says
//! otherwise.
//!
//! Every test sits in the `conform_compare` module and carries the
//! `conform_` prefix, so `-E 'test(conform_)'` selects them with the other
//! legs'. The rig is `tests/conform_shell.rs`'s, copied because test files
//! share no module: the app is built through `FuzzyDedupeApp::with_startup`
//! on the harness's own context and draws into the full content rect, as
//! `tests/layout.rs` does; zoom is set through the preferences, which the
//! app applies with `theme::apply_zoom`. At 200% a 1600 × 960 window gives
//! the UI 800 × 480 points and a 1024 × 600 one 512 × 300. Geometry comes
//! from the accessibility tree; colours and painted text positions from the
//! frame's shapes.
//!
//! Requirements: fresh preference and log directories under
//! `CARGO_TARGET_TMPDIR`, unique per run, which the app itself may write
//! into.

mod conform_compare {
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use egui::accesskit::{Role, Toggled};
    use egui::{Key, Modifiers, OutputCommand, Rect, Vec2};
    use egui_kittest::Harness;
    use egui_kittest::kittest::{NodeT, Queryable};
    use fuzzy_dedupe_app::app::{AppDeps, Clock, FuzzyDedupeApp};
    use fuzzy_dedupe_app::measure::{NoMeasurement, StartupScenario};
    use fuzzy_dedupe_app::presentation::commands::UiIntent;
    use fuzzy_dedupe_app::presentation::state::{
        CompareState, PreferencesState, PresentationState, ThemeOverride,
    };
    use fuzzy_dedupe_app::ui::compare::{
        BACK_NAME, KEEP_BOTH_NAME, LINKED_SCROLL_NAME, NEXT_CANDIDATE_NAME, NEXT_CHANGE_NAME,
        NORMALIZED_NAME, ORIGINAL_NAME, PREVIOUS_CANDIDATE_NAME, PREVIOUS_CHANGE_NAME,
        QUEUE_B_NAME, WRAP_NAME,
    };
    use fuzzy_dedupe_app::ui::glyphs;
    use fuzzy_dedupe_app::ui::shell::{SETTINGS_NAME, WORKSPACE_TOOLBAR_NAME};
    use fuzzy_dedupe_app::ui::theme::{self, Token, Zoom};
    use fuzzy_dedupe_app::ui::widgets::dialog_host::mode_of;
    use fuzzy_dedupe_domain::FileId;
    use fuzzy_dedupe_engine::fixtures::{ScenarioKind, design};
    use fuzzy_dedupe_engine::service::EngineConfig;
    use fuzzy_dedupe_platform::dirs::AppDirs;
    use fuzzy_dedupe_platform::fonts::SystemFontLookup;
    use fuzzy_dedupe_platform::log_retention::LogDate;
    use fuzzy_dedupe_platform::open::{Location, OpenHandle, Opener};
    use fuzzy_dedupe_platform::picker::{FolderPicker, PickHandle, PickOutcome, PickRequest};

    /// Step bound for every `Harness::run` here, in place of kittest's
    /// default of 4: a result from another thread may arrive part-way
    /// through a run and its new widgets ask for two more frames each.
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

    /// The overflow button's accessible name, spelled out rather than
    /// imported so the file compiles on the base it is a negative control
    /// for (`compare::MORE_ACTIONS_NAME`).
    const MORE_ACTIONS: &str = "More actions";

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

    type App = Harness<'static, Slot>;

    /// A fresh home under the test's own target directory, unique per run
    /// because tests may not delete what an earlier run left behind.
    fn dirs_for(name: &str) -> AppDirs {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after 1970")
            .as_nanos();
        let root: PathBuf = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("conform_compare")
            .join(format!("{name}-{nanos}"));
        let state = root.join("state");
        AppDirs::resolve(
            move |var| (var == "XDG_STATE_HOME").then(|| state.clone().into_os_string()),
            &root.join("home"),
        )
    }

    fn launch(name: &str, kind: ScenarioKind, size: Vec2, zoom: u16) -> App {
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
        let mut builder = Harness::builder()
            .with_max_steps(SETTLE_STEPS)
            .with_size(size);
        // The `unicode` checks read labels, accessibility values and the
        // clipboard, not glyph shapes. A CI runner's system fonts have no
        // CJK face, so kittest's default missing-glyph policy would abort
        // the run; rendering those names is EV-001's job (T21).
        if matches!(kind, ScenarioKind::Unicode) {
            builder = builder.allow_missing_glyphs();
        }
        let mut harness = builder.build_ui_state(
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

    fn state(h: &App) -> &PresentationState {
        &h.state().app.as_ref().expect("app").state
    }

    fn state_mut(h: &mut App) -> &mut PresentationState {
        &mut h.state_mut().app.as_mut().expect("app").state
    }

    /// Enough frames for a preference change (zoom, theme) to reach egui
    /// and the layout to settle on it.
    fn settle(h: &mut App) {
        for _ in 0..6 {
            h.step();
        }
        h.run();
    }

    fn push(h: &mut App, intent: UiIntent) {
        h.state_mut().app.as_mut().expect("app").push_intent(intent);
        settle(h);
    }

    /// Runs frames until `done` holds, for engine work on worker threads.
    fn run_until(h: &mut App, what: &str, done: impl Fn(&PresentationState) -> bool) {
        for _ in 0..500 {
            h.step();
            if done(state(h)) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("timed out waiting for {what}");
    }

    fn open_pair(h: &mut App, target: FileId, counterpart: FileId) {
        push(
            h,
            UiIntent::OpenCompare {
                target,
                counterpart,
                return_row: None,
            },
        );
        run_until(h, "the pair's comparison", move |s| {
            s.compare
                .comparison
                .aligned()
                .is_some_and(|a| a.b.file == target)
        });
        settle(h);
    }

    /// The design pair (frame `13:1304`): the draft against its reference.
    fn open_design_pair(h: &mut App) {
        let pair = design::sample().last_compared.expect("design pair");
        open_pair(h, pair.b, pair.a);
    }

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

    fn case(size: Vec2, zoom: u16, theme: ThemeOverride) -> String {
        format!("{}×{} at {zoom}%, {theme:?}", size.x, size.y)
    }

    fn name_of<'n>(node: &impl NodeT<'n>) -> String {
        let node = node.accesskit_node();
        node.label().or_else(|| node.value()).unwrap_or_default()
    }

    /// Every labelled control or text node whose centre lies in `region`.
    fn nodes_in(h: &App, region: Rect) -> Vec<(String, Rect)> {
        let mut out = Vec::new();
        for role in [Role::Button, Role::Label, Role::CheckBox] {
            for node in h.query_all_by_role(role) {
                let rect = node.rect();
                if region.contains(rect.center()) {
                    out.push((name_of(&node), rect));
                }
            }
        }
        out
    }

    fn assert_apart(what: &str, items: &[(String, Rect)]) {
        for (i, (a, ra)) in items.iter().enumerate() {
            for (b, rb) in &items[i + 1..] {
                assert!(
                    !overlap(*ra, *rb),
                    "{what}: \"{a}\" {ra:?} overlaps \"{b}\" {rb:?}"
                );
            }
        }
    }

    /// The two pinned identity rows, A then B.
    fn identity_rects(h: &App) -> [Rect; 2] {
        ["A · ", "B · "].map(|prefix| {
            h.query_all_by_role(Role::Label)
                .find(|n| name_of(n).starts_with(prefix) && name_of(n).contains(": "))
                .unwrap_or_else(|| panic!("an identity row named {prefix}…"))
                .rect()
        })
    }

    /// Every painted text shape: its string, colour and screen rect.
    fn painted(h: &App) -> Vec<(String, egui::Color32, Rect)> {
        fn walk(shape: &egui::Shape, out: &mut Vec<(String, egui::Color32, Rect)>) {
            match shape {
                egui::Shape::Text(text) => {
                    let color = text
                        .galley
                        .job
                        .sections
                        .first()
                        .map_or(text.fallback_color, |s| s.format.color);
                    let color = if color == egui::Color32::PLACEHOLDER {
                        text.fallback_color
                    } else {
                        color
                    };
                    out.push((
                        text.galley.text().to_owned(),
                        color,
                        text.galley.rect.translate(text.pos.to_vec2()),
                    ));
                }
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

    // ── C1: the toolbar fits its budget ─────────────────────────────────

    /// Every control and label of Compare's toolbar lies inside the budget
    /// the shell hands it (and so clear of the shared items) and inside the
    /// client area, and none overlaps another. ‹ Findings always stays.
    /// Whatever of the stepper, Keep both and Queue B is not in the toolbar
    /// is in the More actions menu, which opens from the keyboard with its
    /// first item focused.
    #[test]
    fn conform_compare_toolbar_fits_its_budget_and_overflows_into_more_actions() {
        let actions = [
            PREVIOUS_CANDIDATE_NAME,
            NEXT_CANDIDATE_NAME,
            KEEP_BOTH_NAME,
            QUEUE_B_NAME,
        ];
        let mut overflowed_somewhere = false;
        for (size, zoom) in COMBOS {
            let mut h = launch("toolbar", ScenarioKind::Design, size, zoom);
            open_design_pair(&mut h);
            for theme in [ThemeOverride::Light, ThemeOverride::Dark] {
                push(&mut h, UiIntent::SetTheme(theme));
                let what = case(size, zoom, theme);
                let area = client(&h);
                let budget = h
                    .get_by_role_and_label(Role::Group, WORKSPACE_TOOLBAR_NAME)
                    .rect();
                // Tooltips and menus are not open, so everything centred in
                // the toolbar's band is the toolbar's.
                // The shell's shared items are leg S's; conform_shell proves
                // the budget ends before them, so the workspace's items
                // staying in the budget keeps them apart.
                let band = Rect::from_x_y_ranges(budget.left()..=area.right(), budget.y_range());
                let items: Vec<(String, Rect)> = nodes_in(&h, band)
                    .into_iter()
                    .filter(|(name, _)| {
                        name != SETTINGS_NAME && !name.starts_with("Review cleanup")
                    })
                    .collect();
                assert!(
                    items.iter().any(|(name, _)| name == BACK_NAME),
                    "{what}: ‹ Findings is not in the toolbar: {items:?}"
                );
                for (name, rect) in &items {
                    assert!(
                        inside(budget, *rect) && inside(area, *rect),
                        "{what}: \"{name}\" {rect:?} leaves the budget {budget:?}"
                    );
                }
                assert_apart(&what, &items);

                let shown = |name: &str| items.iter().any(|(n, _)| n == name);
                let missing: Vec<&str> = actions.into_iter().filter(|a| !shown(a)).collect();
                if missing.is_empty() {
                    assert!(
                        !shown(MORE_ACTIONS),
                        "{what}: More actions with nothing overflowed"
                    );
                    continue;
                }
                overflowed_somewhere = true;
                assert!(
                    shown(MORE_ACTIONS),
                    "{what}: {missing:?} left the toolbar with no More actions"
                );
                h.get_by_role_and_label(Role::Button, MORE_ACTIONS).focus();
                settle(&mut h);
                h.key_press(Key::Enter);
                settle(&mut h);
                for name in &missing {
                    let item = h.get_by_role_and_label(Role::Button, name);
                    assert!(
                        inside(area, item.rect()),
                        "{what}: menu item \"{name}\" {:?} off {area:?}",
                        item.rect()
                    );
                }
                assert!(
                    h.get_by_role_and_label(Role::Button, missing[0])
                        .accesskit_node()
                        .is_focused()
                        || h.query_all_by_role(Role::Button)
                            .any(|n| missing.contains(&name_of(&n).as_str())
                                && n.accesskit_node().is_focused()),
                    "{what}: the menu's first item has no keyboard focus"
                );
                h.key_press(Key::Escape);
                settle(&mut h);
                assert!(
                    h.query_by_role_and_label(Role::Button, missing[0])
                        .is_none_or(|n| inside(budget, n.rect())),
                    "{what}: Esc left the menu open"
                );
            }
        }
        assert!(
            overflowed_somewhere,
            "no combination overflowed; the 1024 × 600 at 200% budget cannot hold every action"
        );
    }

    /// An overflowed action works from the keyboard: at 1024 × 600 and
    /// 200%, opening More actions and pressing Enter on Next candidate
    /// re-aims Compare at the second candidate.
    #[test]
    fn conform_compare_an_overflowed_action_runs_from_the_keyboard() {
        let mut h = launch(
            "overflow_keys",
            ScenarioKind::Design,
            Vec2::new(1024.0, 600.0),
            200,
        );
        open_design_pair(&mut h);
        let before = state(&h).compare.candidate_index;
        assert_eq!(before, 0, "the design pair is the first candidate");
        let budget = h
            .get_by_role_and_label(Role::Group, WORKSPACE_TOOLBAR_NAME)
            .rect();
        let in_toolbar = h
            .query_by_role_and_label(Role::Button, NEXT_CANDIDATE_NAME)
            .is_some_and(|n| inside(budget, n.rect()));
        if in_toolbar {
            h.get_by_role_and_label(Role::Button, NEXT_CANDIDATE_NAME)
                .focus();
            settle(&mut h);
            h.key_press(Key::Enter);
        } else {
            h.get_by_role_and_label(Role::Button, MORE_ACTIONS).focus();
            settle(&mut h);
            h.key_press(Key::Enter);
            settle(&mut h);
            h.get_by_role_and_label(Role::Button, NEXT_CANDIDATE_NAME)
                .focus();
            settle(&mut h);
            h.key_press(Key::Enter);
        }
        run_until(&mut h, "the second candidate", |s| {
            s.compare.candidate_index == 1
        });
        // Keep both is also reachable: in the toolbar or in the menu.
        let budget = h
            .get_by_role_and_label(Role::Group, WORKSPACE_TOOLBAR_NAME)
            .rect();
        let keep_in_toolbar = h
            .query_by_role_and_label(Role::Button, KEEP_BOTH_NAME)
            .is_some_and(|n| inside(budget, n.rect()));
        if !keep_in_toolbar {
            h.get_by_role_and_label(Role::Button, MORE_ACTIONS).focus();
            settle(&mut h);
            h.key_press(Key::Enter);
            settle(&mut h);
            h.get_by_role_and_label(Role::Button, KEEP_BOTH_NAME);
        }
    }

    // ── C2 and the identity header: the change row is width-checked ────

    /// Change position, ‹ Previous, Next ›, Wrap, Linked scroll and both
    /// switch segments lie inside the client area and the work area the
    /// identity header spans, and none overlaps another; the two identity
    /// rows lie inside the client area and do not overlap.
    #[test]
    fn conform_compare_change_row_and_identity_header_never_overlap_or_leave_the_canvas() {
        for (size, zoom) in COMBOS {
            let mut h = launch("change_row", ScenarioKind::Design, size, zoom);
            open_design_pair(&mut h);
            for theme in [ThemeOverride::Light, ThemeOverride::Dark] {
                push(&mut h, UiIntent::SetTheme(theme));
                let what = case(size, zoom, theme);
                let area = client(&h);
                let [a, b] = identity_rects(&h);
                assert!(
                    inside(area, a) && inside(area, b),
                    "{what}: identities {a:?} {b:?}"
                );
                assert!(!overlap(a, b), "{what}: identities overlap");
                let work = Rect::from_x_y_ranges(a.left()..=b.right(), area.y_range());
                let mut items: Vec<(String, Rect)> = Vec::new();
                let change = h
                    .query_all_by_role(Role::Label)
                    .find(|n| {
                        let name = name_of(n);
                        name.starts_with("Change ") || name == "No changes"
                    })
                    .expect("the change position");
                items.push((name_of(&change), change.rect()));
                for (role, name) in [
                    (Role::Button, PREVIOUS_CHANGE_NAME),
                    (Role::Button, NEXT_CHANGE_NAME),
                    (Role::CheckBox, WRAP_NAME),
                    (Role::CheckBox, LINKED_SCROLL_NAME),
                    (Role::Button, ORIGINAL_NAME),
                    (Role::Button, NORMALIZED_NAME),
                ] {
                    items.push((name.to_owned(), h.get_by_role_and_label(role, name).rect()));
                }
                for (name, rect) in &items {
                    assert!(
                        inside(area, *rect) && inside(work, *rect),
                        "{what}: \"{name}\" {rect:?} leaves the work area {work:?}"
                    );
                    assert!(
                        rect.bottom() <= a.top() + TOLERANCE,
                        "{what}: \"{name}\" {rect:?} runs into the identity header at {}",
                        a.top()
                    );
                }
                assert_apart(&what, &items);
            }
        }
    }

    // ── C3: the B role is amber ─────────────────────────────────────────

    /// Frame `13:1304` (and `37:2548` in dark) draws "B · Review first" in
    /// the amber token and "A · Reference" in ink.
    #[test]
    fn conform_compare_b_review_first_role_uses_the_amber_token() {
        let mut h = launch("role", ScenarioKind::Design, Vec2::new(1600.0, 960.0), 100);
        open_design_pair(&mut h);
        for theme in [ThemeOverride::Light, ThemeOverride::Dark] {
            push(&mut h, UiIntent::SetTheme(theme));
            let palette = theme::palette(mode_of(&h.ctx));
            let texts = painted(&h);
            let color_of = |text: &str| {
                texts.iter().find(|(t, _, _)| t == text).map_or_else(
                    || panic!("{theme:?}: \"{text}\" not painted"),
                    |(_, c, _)| *c,
                )
            };
            assert_eq!(
                color_of("B · Review first"),
                palette.color(Token::Amber),
                "{theme:?}: the B role is not amber"
            );
            assert_eq!(
                color_of("A · Reference"),
                palette.color(Token::Ink),
                "{theme:?}: the A role is not ink"
            );
        }
    }

    // ── C4: "Normalized" is not clipped ─────────────────────────────────

    /// Each switch segment's painted label sits inside its segment with at
    /// least 4 units either side, so neither label meets the switch's
    /// border.
    #[test]
    fn conform_compare_switch_segments_hold_their_labels_with_padding() {
        for (size, zoom) in COMBOS {
            let mut h = launch("switch", ScenarioKind::Design, size, zoom);
            open_design_pair(&mut h);
            let what = format!("{}×{} at {zoom}%", size.x, size.y);
            let texts = painted(&h);
            for name in [ORIGINAL_NAME, NORMALIZED_NAME] {
                let segment = h.get_by_role_and_label(Role::Button, name).rect();
                let (_, _, text) = texts
                    .iter()
                    .find(|(t, _, r)| t == name && segment.contains(r.center()))
                    .unwrap_or_else(|| panic!("{what}: \"{name}\" not painted in its segment"));
                let left = text.left() - segment.left();
                let right = segment.right() - text.right();
                assert!(
                    left >= 4.0 && right >= 4.0,
                    "{what}: \"{name}\" {text:?} has {left} and {right} of padding in {segment:?}"
                );
            }
        }
    }

    // ── C5: full path and Copy path from the keyboard ───────────────────

    /// The `unicode` scenario's deep path, compared against its group
    /// partner: Tab alone reaches the B identity row, which then shows the
    /// complete path, and Ctrl+Shift+C puts exactly that path on the
    /// clipboard. No pointer event is sent.
    #[test]
    fn conform_compare_identity_shows_the_full_path_on_keyboard_focus_and_copies_it() {
        let mut h = launch(
            "full_path",
            ScenarioKind::Unicode,
            Vec2::new(1024.0, 600.0),
            100,
        );
        let results = state(&h).scan.results.clone().expect("results");
        let (target, counterpart, path) = results
            .groups
            .iter()
            .find_map(|g| {
                let deep = g
                    .members
                    .iter()
                    .max_by_key(|m| m.display_path.chars().count())?;
                let other = g.members.iter().find(|m| m.file != deep.file)?;
                deep.display_path
                    .contains("長いフォルダー名の例")
                    .then(|| (deep.file, other.file, deep.display_path.clone()))
            })
            .expect("the unicode scenario's long path");
        open_pair(&mut h, target, counterpart);
        assert!(
            h.query_by_role_and_label(Role::Label, &path).is_none(),
            "the full path shows before any focus"
        );

        let identity = |h: &App| {
            h.query_all_by_role(Role::Label)
                .find(|n| name_of(n).starts_with("B · "))
                .is_some_and(|n| n.accesskit_node().is_focused())
        };
        let mut reached = false;
        for _ in 0..60 {
            h.key_press(Key::Tab);
            h.run();
            if identity(&h) {
                reached = true;
                break;
            }
        }
        assert!(reached, "Tab never focused the B identity row");
        settle(&mut h);
        let shown = h
            .query_all_by_role(Role::Label)
            .any(|n| name_of(&n) == path);
        assert!(
            shown,
            "keyboard focus does not show the full path \"{path}\""
        );

        // One event per step: kittest runs a pass per queued event and keeps
        // only the last pass's output, so a whole `key_press_modifiers`
        // sequence would overwrite the pass that copied.
        let mut copied = Vec::new();
        for pressed in [true, false] {
            h.event(egui::Event::Key {
                key: Key::C,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: Modifiers::CTRL | Modifiers::SHIFT,
            });
            h.step();
            copied.extend(
                h.output()
                    .platform_output
                    .commands
                    .iter()
                    .filter_map(|c| match c {
                        OutputCommand::CopyText(text) => Some(text.clone()),
                        _ => None,
                    }),
            );
        }
        assert_eq!(copied, [path], "Copy path did not copy the full path");
    }

    // ── C6: Wrap defaults on; a stored preference wins ──────────────────

    #[test]
    fn conform_compare_wrap_defaults_on_and_a_stored_preference_wins() {
        assert!(PreferencesState::default().compare_wrap);
        assert!(CompareState::default().wrap);

        let mut h = launch(
            "wrap_default",
            ScenarioKind::Design,
            Vec2::new(1600.0, 960.0),
            100,
        );
        open_design_pair(&mut h);
        assert!(state(&h).compare.wrap, "a fresh Compare does not wrap");
        let toggled = h
            .get_by_role_and_label(Role::CheckBox, WRAP_NAME)
            .accesskit_node()
            .toggled();
        assert_eq!(toggled, Some(Toggled::True), "Wrap is not shown checked");

        // A stored `wrap = false`, as `preferences::load` would have read
        // it, reaches Compare before it first draws.
        let mut h = launch(
            "wrap_stored",
            ScenarioKind::Design,
            Vec2::new(1600.0, 960.0),
            100,
        );
        state_mut(&mut h).preferences.compare_wrap = false;
        open_design_pair(&mut h);
        assert!(!state(&h).compare.wrap, "the stored preference lost");
        let toggled = h
            .get_by_role_and_label(Role::CheckBox, WRAP_NAME)
            .accesskit_node()
            .toggled();
        assert_eq!(toggled, Some(Toggled::False));
    }

    // ── Q5: wrapped diff text breaks between words ──────────────────────

    /// The painted visual lines of each laid-out diff row, top to bottom,
    /// per pane: the text shapes painted at the left edge of the row's
    /// accessibility node (`diff_view:<a|b>:<row>`, which starts at the text
    /// column), so the gutter's numbers and markers and the identity header
    /// over an overscan row are not taken for its text.
    fn wrapped_rows(h: &App) -> Vec<(String, Vec<String>)> {
        let texts = painted(h);
        h.query_all_by(|n| {
            n.author_id()
                .is_some_and(|id| id.starts_with("diff_view:a:") || id.starts_with("diff_view:b:"))
        })
        .map(|n| {
            let cell = n.rect();
            let mut lines: Vec<(f32, String)> = texts
                .iter()
                .filter(|(_, _, r)| {
                    (r.left() - cell.left()).abs() <= TOLERANCE
                        && r.center().y > cell.top()
                        && r.center().y < cell.bottom()
                })
                .map(|(t, _, r)| (r.top(), t.clone()))
                .collect();
            lines.sort_by(|a, b| a.0.total_cmp(&b.0));
            (
                n.accesskit_node()
                    .author_id()
                    .unwrap_or_default()
                    .to_owned(),
                lines.into_iter().map(|(_, t)| t).collect(),
            )
        })
        .collect()
    }

    /// Frame `29:3648` wraps the diff text between words at 1024 × 600
    /// ("functionality" stays whole); a break falls inside a word only when
    /// that word alone is wider than the line. Pins the T40 round-2 review
    /// finding "function/ality".
    #[test]
    fn conform_compare_wrapped_diff_text_breaks_between_words() {
        for (size, zoom) in [
            (Vec2::new(1024.0, 600.0), 100),
            (Vec2::new(1024.0, 600.0), 200),
            (Vec2::new(1600.0, 960.0), 200),
        ] {
            let mut h = launch("word_wrap", ScenarioKind::Design, size, zoom);
            open_design_pair(&mut h);
            assert!(state(&h).compare.wrap, "Compare does not wrap");
            let what = format!("{}×{} at {zoom}%", size.x, size.y);
            let rows = wrapped_rows(&h);
            let wrapped: Vec<_> = rows.iter().filter(|(_, l)| l.len() > 1).collect();
            assert!(!wrapped.is_empty(), "{what}: no diff row wraps");
            for (row, lines) in wrapped {
                for pair in lines.windows(2) {
                    let (before, after) = (&pair[0], &pair[1]);
                    let mid_word = before.chars().last().is_some_and(char::is_alphanumeric)
                        && after.chars().next().is_some_and(char::is_alphanumeric);
                    // A visual line with no break opportunity is one word
                    // wider than the line, which may break anywhere.
                    let one_word = !before.trim_start().contains(char::is_whitespace);
                    assert!(
                        !mid_word || one_word,
                        "{what}: {row} breaks mid-word: {before:?} / {after:?}"
                    );
                }
            }
        }
    }
}
