// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! PV-T40-001, leg F (plan T40, items F1 to F12): the Findings toolbar,
//! filter row, table and table-region states conform to the review frames
//! at 1600 × 960 and 1024 × 600, each at zoom 100% and 200%, light and dark.
//!
//! PV-T41-001 adds items 1, 3, 4 and 5 of plan T41: the table's scrollbar
//! at rest, the chips' fit at the narrowest width, the Group column's floor
//! and the Scanning line's folder form.
//!
//! Every test sits in the `conform_findings` module and carries the
//! `conform_` prefix, so `-E 'test(conform_)'` selects them with the other
//! legs'. The rig is `tests/conform_shell.rs`'s, copied: the app is built
//! through `FuzzyDedupeApp::with_startup` on the harness's own context with
//! a fixture scenario, draws into the full content rect as `tests/layout.rs`
//! does, and takes zoom through the preferences. Geometry comes from the
//! accessibility tree; colours, elision and the absence of rules from the
//! frame's painted shapes. Accessible names are written out as literals
//! where the view's constant is newer than the base, so the file also
//! builds on the base for the negative control.
//!
//! Requirements: fresh preference and log directories under
//! `CARGO_TARGET_TMPDIR`, unique per run, which the app itself may write
//! into.

mod conform_findings {
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use egui::accesskit::Role;
    use egui::{Key, Rect, Vec2};
    use egui_kittest::Harness;
    use egui_kittest::kittest::{NodeT, Queryable};
    use fuzzy_dedupe_app::app::{AppDeps, Clock, FuzzyDedupeApp};
    use fuzzy_dedupe_app::measure::{NoMeasurement, StartupScenario};
    use fuzzy_dedupe_app::presentation::commands::UiIntent;
    use fuzzy_dedupe_app::presentation::projections::{self, SORT_ITEMS};
    use fuzzy_dedupe_app::presentation::state::{
        PresentationState, RowId, ScanStage, ThemeOverride,
    };
    use fuzzy_dedupe_app::ui::findings::{ACTIONS_NAME, MEASURED_NOTE, SEARCH_NAME};
    use fuzzy_dedupe_app::ui::glyphs;
    use fuzzy_dedupe_app::ui::shell::WORKSPACE_TOOLBAR_NAME;
    use fuzzy_dedupe_app::ui::theme::{self, Icon, Mode, Token, Zoom};
    use fuzzy_dedupe_app::ui::widgets::grouped_table;
    use fuzzy_dedupe_domain::ScanProgress;
    use fuzzy_dedupe_engine::fixtures::ScenarioKind;
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
            .join("conform_findings")
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
        let mut builder = Harness::builder()
            .with_max_steps(SETTLE_STEPS)
            .with_size(size);
        if kind == ScenarioKind::Unicode {
            // Reads labels and accessibility values, not glyph shapes; EV-001
            // covers rendering on the reference workstation (CI has no CJK font).
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

    fn case(size: Vec2, zoom: u16, theme: ThemeOverride) -> String {
        format!("{}×{} at {zoom}%, {theme:?}", size.x, size.y)
    }

    fn color(h: &Shell, token: Token) -> egui::Color32 {
        let mode = match h.ctx.theme() {
            egui::Theme::Light => Mode::Light,
            egui::Theme::Dark => Mode::Dark,
        };
        theme::palette(mode).color(token)
    }

    /// Every painted shape, flattened.
    fn shapes(h: &Shell) -> Vec<egui::Shape> {
        fn walk(shape: &egui::Shape, out: &mut Vec<egui::Shape>) {
            match shape {
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, out)),
                other => out.push(other.clone()),
            }
        }
        let mut out = Vec::new();
        for clipped in &h.output().shapes {
            walk(&clipped.shape, &mut out);
        }
        out
    }

    fn texts(h: &Shell) -> Vec<egui::epaint::TextShape> {
        shapes(h)
            .into_iter()
            .filter_map(|s| match s {
                egui::Shape::Text(t) => Some(t),
                _ => None,
            })
            .collect()
    }

    fn text_rect(t: &egui::epaint::TextShape) -> Rect {
        t.galley.rect.translate(t.pos.to_vec2())
    }

    fn text_color(t: &egui::epaint::TextShape) -> egui::Color32 {
        t.galley
            .job
            .sections
            .first()
            .map_or(t.fallback_color, |s| s.format.color)
    }

    fn expand_all(h: &mut Shell) {
        let groups = state(h)
            .scan
            .results
            .as_ref()
            .expect("results")
            .groups
            .iter()
            .map(|g| g.id)
            .collect();
        state_mut(h).findings.expanded = groups;
        settle(h);
    }

    fn table(h: &Shell) -> Rect {
        let id = grouped_table::table_id().accesskit_id();
        h.get_by(move |node| node.locate().0 == id).rect()
    }

    // ── F1: the toolbar fits its budget ─────────────────────────────────

    /// The Findings toolbar's own controls: those inside the workspace
    /// budget, with their names.
    fn toolbar_controls(h: &Shell) -> Vec<(String, Rect)> {
        let budget = h
            .get_by_role_and_label(Role::Group, WORKSPACE_TOOLBAR_NAME)
            .rect();
        let mut out: Vec<(String, Rect)> = h
            .query_all_by_role(Role::Button)
            .chain(h.query_all_by_role(Role::TextInput))
            .filter(|n| {
                let r = n.rect();
                r.center().y >= budget.top() && r.center().y <= budget.bottom()
            })
            .filter(|n| n.rect().left() >= budget.left() - TOLERANCE)
            .filter(|n| n.rect().center().x <= budget.right() + 60.0)
            .map(|n| (n.accesskit_node().label().unwrap_or_default(), n.rect()))
            .filter(|(name, _)| {
                name == SEARCH_NAME
                    || name == ACTIONS_NAME
                    || name.starts_with("Sort: ")
                    || name == "Compare"
                    || name == "Clear search"
            })
            .collect();
        out.sort_by(|a, b| a.1.left().total_cmp(&b.1.left()));
        out
    }

    /// Every toolbar control lies inside the workspace budget and the
    /// client area, and no two intersect; at the reference size every
    /// control shows and the search field is 340 (§11.3). Compare leaves
    /// the toolbar only when the full row would squeeze the search field
    /// below its minimum, so nothing moves while the field could shrink.
    #[test]
    fn conform_findings_toolbar_fits_its_budget_at_every_size_and_zoom() {
        for (size, zoom) in COMBOS {
            let mut h = launch_design("toolbar", size, zoom);
            for theme in [ThemeOverride::Light, ThemeOverride::Dark] {
                push(&mut h, UiIntent::SetTheme(theme));
                let what = case(size, zoom, theme);
                let area = client(&h);
                let budget = h
                    .get_by_role_and_label(Role::Group, WORKSPACE_TOOLBAR_NAME)
                    .rect();
                let controls = toolbar_controls(&h);
                let names: Vec<&str> = controls.iter().map(|(n, _)| n.as_str()).collect();
                assert!(names.contains(&SEARCH_NAME), "{what}: no search {names:?}");
                assert!(
                    names.contains(&ACTIONS_NAME),
                    "{what}: no Actions {names:?}"
                );
                for (name, rect) in &controls {
                    assert!(
                        inside(budget, *rect) && inside(area, *rect),
                        "{what}: {name} {rect:?} leaves the budget {budget:?} or {area:?}"
                    );
                }
                for (i, (a, ra)) in controls.iter().enumerate() {
                    for (b, rb) in &controls[i + 1..] {
                        assert!(!overlap(*ra, *rb), "{what}: {a} {ra:?} overlaps {b} {rb:?}");
                    }
                }
                let search = controls
                    .iter()
                    .find(|(n, _)| n == SEARCH_NAME)
                    .map(|(_, r)| *r)
                    .expect("search");
                let all_shown =
                    names.iter().any(|n| n.starts_with("Sort: ")) && names.contains(&"Compare");
                if size.x >= 1600.0 && zoom == 100 {
                    assert!(all_shown, "{what}: an action moved at the reference size");
                    // Text box plus the 24-unit glyph is the 340 field.
                    assert!(
                        (search.width() + 24.0 - 340.0).abs() <= TOLERANCE,
                        "{what}: search {search:?} is not 340 with its glyph"
                    );
                }
                // Compare moves only once the full row would squeeze the
                // search field below its 160 minimum.
                let find = |prefix: &str| {
                    controls
                        .iter()
                        .find(|(n, _)| n.starts_with(prefix))
                        .map(|(_, r)| *r)
                };
                if let (None, Some(actions), Some(sort)) =
                    (find("Compare"), find(ACTIONS_NAME), find("Sort: "))
                {
                    let gap = sort.left() - actions.right();
                    let room =
                        budget.width() - (actions.width() + sort.width() + 100.0 + 3.0 * gap);
                    assert!(
                        room < 160.0 + TOLERANCE,
                        "{what}: Compare moved with {room} left for the search"
                    );
                }
            }
        }
    }

    /// At 1024 × 600 under 200% the budget holds only the search field and
    /// Actions ▾; Compare and Sort move into Actions ▾, which the keyboard
    /// opens and walks: Enter on a moved Sort entry sorts, and the moved
    /// Compare entry is there, disabled until a member row has focus.
    #[test]
    fn conform_findings_overflowed_actions_are_reachable_by_keyboard() {
        let mut h = launch_design("overflow", Vec2::new(1024.0, 600.0), 200);
        let names: Vec<String> = toolbar_controls(&h).into_iter().map(|(n, _)| n).collect();
        assert!(
            !names
                .iter()
                .any(|n| n.starts_with("Sort: ") || n == "Compare"),
            "Sort ▾ and Compare are still in the toolbar: {names:?}"
        );
        h.get_by_role_and_label(Role::Button, ACTIONS_NAME).focus();
        settle(&mut h);
        h.key_press(Key::Enter);
        settle(&mut h);
        let target = format!("Sort: {}", SORT_ITEMS[1].1);
        let mut reached = false;
        for _ in 0..24 {
            if h.query_by_role_and_label(Role::Button, &target)
                .is_some_and(|n| n.is_focused())
            {
                reached = true;
                break;
            }
            h.key_press(Key::Tab);
            settle(&mut h);
        }
        assert!(reached, "Tab never reached \"{target}\" in Actions ▾");
        let menu_compare = h
            .query_all_by_role_and_label(Role::Button, "Compare")
            .any(|n| n.rect().left() > 100.0);
        assert!(menu_compare, "Compare is not in Actions ▾");
        h.key_press(Key::Enter);
        settle(&mut h);
        assert_eq!(
            state(&h).findings.sort,
            SORT_ITEMS[1].0,
            "Enter did not sort"
        );
    }

    // ── F2: the glyph is inside the field, the text centred ─────────────

    /// The field's frame (a 30-high painted rect) holds both the text box
    /// and the search glyph, and the hint text is centred in it.
    #[test]
    fn conform_findings_search_glyph_sits_inside_the_field_and_text_is_centred() {
        for (size, zoom) in COMBOS {
            let h = launch_design("search", size, zoom);
            let what = format!("{}×{} at {zoom}%", size.x, size.y);
            let edit = h.get_by_role_and_label(Role::TextInput, SEARCH_NAME).rect();
            let glyph_text = glyphs::text(&h.ctx, Icon::Search).expect("search glyph");
            let glyph = texts(&h)
                .into_iter()
                .find(|t| t.galley.text() == glyph_text)
                .map(|t| text_rect(&t))
                .expect("the search glyph is painted");
            let frame = shapes(&h)
                .into_iter()
                .filter_map(|s| match s {
                    egui::Shape::Rect(r) => Some(r.rect),
                    _ => None,
                })
                .filter(|r| (r.height() - 30.0).abs() <= TOLERANCE)
                .find(|r| inside(*r, edit) && inside(*r, glyph));
            assert!(
                frame.is_some(),
                "{what}: no 30-high field frame holds the text box {edit:?} and glyph {glyph:?}"
            );
            let hint = texts(&h)
                .into_iter()
                .find(|t| t.galley.text() == SEARCH_NAME)
                .map(|t| text_rect(&t))
                .expect("the hint is painted");
            assert!(
                (hint.center().y - edit.center().y).abs() <= TOLERANCE,
                "{what}: hint centre {} against field centre {}",
                hint.center().y,
                edit.center().y
            );
        }
    }

    // ── F3, F4: chips and the caption ───────────────────────────────────

    fn chips(h: &Shell) -> Vec<Rect> {
        let names = [
            "All",
            "Same bytes",
            "Same text",
            "Similar text",
            "Containment",
        ];
        let mut out: Vec<Rect> = h
            .query_all_by_role(Role::Button)
            .filter(|n| {
                let label = n.accesskit_node().label().unwrap_or_default();
                names.iter().any(|p| {
                    label == *p
                        || label.strip_prefix(p).is_some_and(|rest| {
                            rest.starts_with(' ')
                                && rest[1..].chars().all(|c| c.is_ascii_digit() || c == ',')
                        })
                })
            })
            .map(|n| n.rect())
            .collect();
        out.sort_by(|a, b| a.left().total_cmp(&b.left()));
        out
    }

    /// Chips are 26 high, 6 apart, and pad their label by 6 on each side
    /// (frames `13:648`, `30:2370`).
    #[test]
    fn conform_findings_chips_have_the_frame_padding_and_spacing() {
        let h = launch_design("chips", Vec2::new(1600.0, 960.0), 100);
        let chips = chips(&h);
        assert_eq!(chips.len(), 5, "five chips: {chips:?}");
        for chip in &chips {
            assert!(
                (chip.height() - 26.0).abs() <= TOLERANCE,
                "chip {chip:?} not 26 high"
            );
        }
        for pair in chips.windows(2) {
            let gap = pair[1].left() - pair[0].right();
            assert!((gap - 6.0).abs() <= TOLERANCE, "chip gap {gap}, frame 6");
        }
        let label = texts(&h)
            .into_iter()
            .filter(|t| t.galley.text().starts_with("Same bytes"))
            .map(|t| text_rect(&t))
            .find(|r| inside(chips[1], *r))
            .expect("the Same bytes label in its chip");
        let pad = label.left() - chips[1].left();
        assert!(
            (pad - 6.0).abs() <= TOLERANCE,
            "chip padding {pad}, frame 6"
        );
    }

    /// The statement never overprints a chip: it shows whole, inside the
    /// client area, or not at all; visible chips never overlap each other.
    #[test]
    fn conform_findings_filter_caption_never_overprints_the_chips() {
        for (size, zoom) in COMBOS {
            let mut h = launch_design("caption", size, zoom);
            for theme in [ThemeOverride::Light, ThemeOverride::Dark] {
                push(&mut h, UiIntent::SetTheme(theme));
                let what = case(size, zoom, theme);
                let area = client(&h);
                let chips = chips(&h);
                assert_eq!(chips.len(), 5, "{what}: chips {chips:?}");
                for pair in chips.windows(2) {
                    assert!(!overlap(pair[0], pair[1]), "{what}: chips overlap {pair:?}");
                }
                if let Some(caption) = h.query_by_label(MEASURED_NOTE) {
                    let caption = caption.rect();
                    assert!(
                        inside(area, caption),
                        "{what}: caption {caption:?} off {area:?}"
                    );
                    for chip in &chips {
                        assert!(
                            !overlap(*chip, caption),
                            "{what}: caption {caption:?} overprints chip {chip:?}"
                        );
                    }
                    let painted = texts(&h)
                        .into_iter()
                        .find(|t| t.galley.text() == MEASURED_NOTE)
                        .expect("caption painted");
                    assert!(!painted.galley.elided, "{what}: caption elided");
                }
            }
        }
        // The reference size keeps it (frame `13:648`).
        let h = launch_design("caption_ref", Vec2::new(1600.0, 960.0), 100);
        assert!(
            h.query_by_label(MEASURED_NOTE).is_some(),
            "caption hidden at 1600"
        );
    }

    // ── F5: the progress track ──────────────────────────────────────────

    /// Frame `28:2171`: a visible 360 × 6 track in the `line` token, with a
    /// moving `primary` segment, while a scan runs.
    #[test]
    fn conform_findings_progress_track_is_visible_while_scanning() {
        let mut h = launch_design("progress", Vec2::new(1600.0, 960.0), 100);
        state_mut(&mut h).scan.stage = ScanStage::Running;
        // The segment animates, so the harness never settles: step only.
        for _ in 0..6 {
            h.step();
        }
        let track = h.get_by_label("Scan progress").rect();
        assert!(
            (track.width() - 360.0).abs() <= TOLERANCE && (track.height() - 6.0).abs() <= TOLERANCE,
            "track {track:?}, frame 360 × 6"
        );
        let rects: Vec<egui::epaint::RectShape> = shapes(&h)
            .into_iter()
            .filter_map(|s| match s {
                egui::Shape::Rect(r) => Some(r),
                _ => None,
            })
            .collect();
        let line = color(&h, Token::Line);
        let primary = color(&h, Token::Primary);
        assert!(
            rects
                .iter()
                .any(|r| r.fill == line && inside(r.rect, track) && inside(track, r.rect)),
            "no line-coloured track painted at {track:?}"
        );
        assert!(
            rects
                .iter()
                .any(|r| r.fill == primary && inside(track, r.rect) && r.rect.width() > 1.0),
            "no primary segment painted on the track"
        );
    }

    // ── F6: the header row on empty states ──────────────────────────────

    /// Frame `30:2370`: with no matching rows the table's header row still
    /// shows across the region's top.
    #[test]
    fn conform_findings_empty_states_keep_the_header_row() {
        let mut h = launch_design("empty", Vec2::new(1600.0, 960.0), 100);
        state_mut(&mut h).findings.search = "zz-no-such-file-zz".to_owned();
        settle(&mut h);
        assert!(
            h.query_by_label("Clear search and filters").is_some(),
            "not the 01e state"
        );
        for title in ["Queue", "Group", "Match", "File name", "Folder", "Decision"] {
            assert!(
                h.query_all_by_role(Role::Label)
                    .any(|n| n.accesskit_node().value().as_deref() == Some(title)),
                "header title {title} missing on the empty state"
            );
        }
    }

    // ── F7: ⋯ on every member row ───────────────────────────────────────

    #[test]
    fn conform_findings_every_member_row_shows_its_row_menu_button() {
        let mut h = launch_design("dots", Vec2::new(1600.0, 960.0), 100);
        expand_all(&mut h);
        assert_eq!(state(&h).findings.focused, None);
        let buttons = h
            .query_all_by_role_and_label(Role::Button, "Row menu")
            .count();
        let members = h.query_all_by_role(Role::CheckBox).count();
        assert!(
            buttons >= members && buttons >= 8,
            "{buttons} row-menu buttons for {members} checkboxes with no row focused"
        );
    }

    // ── F8: flexible File name and Folder ───────────────────────────────

    /// The Decision column stays on canvas at 1600 × 960 (100%, 200%) and
    /// 1024 × 600 at 100%. 1024 × 600 at 200% (a 440-point table) is not
    /// asserted here: no readable set of the always-shown columns fits, so
    /// the table scrolls sideways there by design (T40 ruling; spec line
    /// 869 forbids clipping, not scrolling), and T26 exercises it by hand.
    /// The Folder text shortens visually at 1024 × 600 while its value
    /// stays whole.
    #[test]
    fn conform_findings_decision_stays_on_canvas_and_folder_shortens_visually() {
        for (size, zoom) in COMBOS {
            if size.x < 1600.0 && zoom == 200 {
                // Scrolls sideways by design; see the doc comment.
                continue;
            }
            let mut h = launch_design("columns", size, zoom);
            expand_all(&mut h);
            let what = format!("{}×{} at {zoom}%", size.x, size.y);
            let table = table(&h);
            let decision = h
                .query_all_by_role(Role::Label)
                .filter(|n| n.accesskit_node().value().as_deref() == Some("Decision"))
                .map(|n| n.rect())
                .min_by(|a, b| a.top().total_cmp(&b.top()))
                .unwrap_or_else(|| panic!("{what}: no Decision header"));
            assert!(
                inside(table, decision) && inside(client(&h), decision),
                "{what}: Decision header {decision:?} off the table {table:?}"
            );
        }
        let mut h = launch_design("folder", Vec2::new(1024.0, 600.0), 100);
        expand_all(&mut h);
        let shortened = texts(&h)
            .into_iter()
            .any(|t| t.galley.text().starts_with("~/…/") || t.galley.text().starts_with("/…/"));
        assert!(shortened, "no Folder cell shortened to ~/…/ at 1024 × 600");
        let results = state(&h).scan.results.clone().expect("results");
        let folders: Vec<String> = results
            .groups
            .iter()
            .flat_map(|g| g.members.iter())
            .map(|m| fuzzy_dedupe_app::presentation::projections::folder_label(&m.folder))
            .collect();
        assert!(
            h.query_all_by_role(Role::Label)
                .filter_map(|n| n.accesskit_node().value())
                .any(|v| folders.contains(&v) && v.len() > 12),
            "no Folder cell keeps its whole value"
        );
    }

    // ── F9, F10, F11: Protected, Queue, rules, reference row ────────────

    #[test]
    fn conform_findings_table_cells_follow_the_frame() {
        for theme in [ThemeOverride::Light, ThemeOverride::Dark] {
            let mut h = launch_design("cells", Vec2::new(1600.0, 960.0), 100);
            push(&mut h, UiIntent::SetTheme(theme));
            expand_all(&mut h);
            let table = table(&h);
            let painted = texts(&h);
            let green = color(&h, Token::Green);
            // F9: "Protected" values (below the header) in the green token.
            let values: Vec<_> = painted
                .iter()
                .filter(|t| {
                    t.galley.text() == "Protected" && text_rect(t).top() > table.top() + 31.0
                })
                .collect();
            assert!(!values.is_empty(), "{theme:?}: no Protected value in view");
            assert!(
                values.iter().all(|t| text_color(t) == green),
                "{theme:?}: a Protected value is not green"
            );
            // F10: the Queue header shows whole.
            let queue = painted
                .iter()
                .find(|t| t.galley.text() == "Queue" && text_rect(t).top() < table.top() + 31.0)
                .expect("Queue header painted");
            assert!(
                !queue.galley.elided,
                "{theme:?}: the Queue header is elided"
            );
            // F11: no vertical rules inside the body.
            let rules = shapes(&h)
                .into_iter()
                .filter_map(|s| match s {
                    egui::Shape::LineSegment { points, stroke } => Some((points, stroke)),
                    _ => None,
                })
                .filter(|(p, stroke)| {
                    (p[0].x - p[1].x).abs() < 0.5
                        && (p[1].y - p[0].y).abs() > 60.0
                        && stroke.width > 0.0
                        && stroke.color.a() > 0
                        && table.contains(p[0])
                        && p[0].x > table.left() + 2.0
                        && p[0].x < table.right() - 16.0
                })
                .count();
            assert_eq!(rules, 0, "{theme:?}: {rules} vertical cell rules");
            // F11: the reference row's anchor.
            let anchor = glyphs::text(&h.ctx, Icon::Reference).expect("anchor glyph");
            assert!(
                painted.iter().any(|t| t.galley.text() == anchor),
                "{theme:?}: no reference anchor painted"
            );
        }
    }

    // ── F12: the full path on keyboard focus ────────────────────────────

    /// The `unicode` scenario's long path shows whole once its row has
    /// keyboard focus, reached by Tab and the arrow keys alone.
    #[test]
    fn conform_findings_keyboard_focus_shows_the_full_path() {
        for (size, zoom) in [
            (Vec2::new(1600.0, 960.0), 100),
            (Vec2::new(1024.0, 600.0), 200),
        ] {
            let mut h = launch("full_path", ScenarioKind::Unicode, size, zoom);
            let what = format!("{}×{} at {zoom}%", size.x, size.y);
            expand_all(&mut h);
            let results = state(&h).scan.results.clone().expect("results");
            let (group, member) = results
                .groups
                .iter()
                .find_map(|g| {
                    g.members
                        .iter()
                        .find(|m| m.name.starts_with("非常に長い"))
                        .map(|m| (g.id, m.clone()))
                })
                .expect("the long-path member");
            let target = RowId::Member {
                group,
                file: member.file,
            };
            let mut on_table = false;
            for _ in 0..40 {
                h.key_press(Key::Tab);
                settle(&mut h);
                if h.ctx.memory(|m| m.has_focus(grouped_table::table_id())) {
                    on_table = true;
                    break;
                }
            }
            assert!(on_table, "{what}: Tab never reached the table");
            for _ in 0..20 {
                if state(&h).findings.focused == Some(target) {
                    break;
                }
                h.key_press(Key::ArrowDown);
                settle(&mut h);
            }
            assert_eq!(
                state(&h).findings.focused,
                Some(target),
                "{what}: row not reached"
            );
            let node = h.get_by_role_and_label(Role::Tooltip, "Full path");
            assert_eq!(
                node.accesskit_node().value().as_deref(),
                Some(member.display_path.as_str()),
                "{what}: the full path shown"
            );
            assert!(
                inside(client(&h), node.rect()),
                "{what}: path {:?} off canvas",
                node.rect()
            );
            assert!(
                texts(&h)
                    .iter()
                    .any(|t| t.galley.text() == member.display_path && !t.galley.elided),
                "{what}: the whole path {} is not painted",
                member.display_path
            );
        }
    }

    // ── Round 2: D1, Q7, Q9 ─────────────────────────────────────────────

    /// The colour a text shape is drawn in, after any fade its painter
    /// applied (a disabled `Ui` multiplies the galley's vertex colours, not
    /// the section format).
    fn drawn_color(t: &egui::epaint::TextShape) -> Option<egui::Color32> {
        t.galley
            .rows
            .iter()
            .flat_map(|row| row.row.visuals.mesh.vertices.iter())
            .map(|v| v.color)
            .next()
    }

    /// The search field's painted 30-high frame, which holds the text box.
    fn search_frame(h: &Shell) -> Rect {
        let edit = h.get_by_role_and_label(Role::TextInput, SEARCH_NAME).rect();
        shapes(h)
            .into_iter()
            .filter_map(|s| match s {
                egui::Shape::Rect(r) => Some(r.rect),
                _ => None,
            })
            .filter(|r| (r.height() - 30.0).abs() <= TOLERANCE)
            .find(|r| inside(*r, edit))
            .expect("the search field's frame")
    }

    /// D1: placeholder and typed text start 6 units inside the field's left
    /// border (frame `13:648`), at every size and zoom. Pins the round-2
    /// finding of text against the border (x = 164, text from 165).
    #[test]
    fn conform_findings_search_text_starts_six_inside_the_field() {
        for (size, zoom) in COMBOS {
            let mut h = launch_design("search_inset", size, zoom);
            let what = format!("{}×{} at {zoom}%", size.x, size.y);
            let frame = search_frame(&h);
            let hint = texts(&h)
                .into_iter()
                .find(|t| t.galley.text() == SEARCH_NAME)
                .map(|t| text_rect(&t))
                .expect("the hint is painted");
            let inset = hint.left() - frame.left();
            assert!(
                (inset - 6.0).abs() <= TOLERANCE,
                "{what}: hint starts {inset} inside the field, frame 13:648 has 6"
            );

            state_mut(&mut h).findings.search = "Plan".to_owned();
            settle(&mut h);
            let frame = search_frame(&h);
            let typed = texts(&h)
                .into_iter()
                .map(|t| text_rect(&t))
                .zip(texts(&h).into_iter().map(|t| t.galley.text().to_owned()))
                .find(|(r, text)| text == "Plan" && inside(frame, *r))
                .map(|(r, _)| r)
                .expect("the typed text is painted in the field");
            let inset = typed.left() - frame.left();
            assert!(
                (inset - 6.0).abs() <= TOLERANCE,
                "{what}: typed text starts {inset} inside the field, frame 13:648 has 6"
            );
        }
    }

    /// Q7: before the first scan (frame `28:1399`) no result exists, so the
    /// filter chips are disabled, their labels in the muted token, and the
    /// caption is drawn faded; with results both are live again. The rail's
    /// Menu stays enabled (the frame's muted Menu is decorative).
    #[test]
    fn conform_findings_chips_and_caption_are_muted_before_the_first_scan() {
        let mut h = launch_design("before_scan", Vec2::new(1600.0, 960.0), 100);
        let caption = |h: &Shell| {
            texts(h)
                .into_iter()
                .find(|t| t.galley.text() == MEASURED_NOTE)
                .and_then(|t| drawn_color(&t))
                .expect("the caption is painted")
        };
        assert!(
            h.query_all_by_role(Role::Button)
                .filter(|n| chips(&h).contains(&n.rect()))
                .all(|n| !n.accesskit_node().is_disabled()),
            "with results a chip is disabled"
        );
        assert_eq!(caption(&h).a(), 255, "with results the caption is faded");

        // The before-first-scan state, as `tests/snapshots.rs` reaches it:
        // a fresh presentation state that keeps only the preferences.
        let preferences = state(&h).preferences.clone();
        *state_mut(&mut h) = PresentationState {
            preferences,
            ..PresentationState::default()
        };
        settle(&mut h);
        assert!(state(&h).scan.results.is_none());

        let chip_rects = chips(&h);
        assert_eq!(chip_rects.len(), 5, "five chips: {chip_rects:?}");
        let chip_nodes: Vec<_> = h
            .query_all_by_role(Role::Button)
            .filter(|n| chip_rects.contains(&n.rect()))
            .collect();
        assert_eq!(chip_nodes.len(), 5);
        for node in &chip_nodes {
            assert!(
                node.accesskit_node().is_disabled(),
                "chip {:?} is enabled before the first scan",
                node.accesskit_node().label()
            );
        }
        let muted = color(&h, Token::Muted);
        for name in ["Same bytes", "Same text", "Similar text", "Containment"] {
            let label = texts(&h)
                .into_iter()
                .find(|t| t.galley.text() == name)
                .unwrap_or_else(|| panic!("chip {name} painted"));
            assert_eq!(
                text_color(&label),
                muted,
                "chip {name} not in the muted token"
            );
            assert!(
                drawn_color(&label).is_some_and(|c| c.a() < 255),
                "chip {name} is not drawn disabled"
            );
        }
        assert!(
            caption(&h).a() < 255,
            "the caption is not muted before the first scan"
        );
        let menu = h.get_by_role_and_label(Role::Button, "Menu");
        assert!(
            !menu.accesskit_node().is_disabled(),
            "the rail's Menu is disabled"
        );
    }

    /// Q9 (frame `13:648`): a member that needs review ("Review first")
    /// shows its queue checkbox outlined in the amber token on the amber
    /// soft fill; a protected member shows no queue checkbox, and the lock
    /// glyph in place of its file icon.
    #[test]
    fn conform_findings_review_first_and_protected_rows_follow_the_frame() {
        use fuzzy_dedupe_app::presentation::projections::{decision_label, member_view};
        for theme in [ThemeOverride::Light, ThemeOverride::Dark] {
            let mut h = launch_design("row_marks", Vec2::new(1600.0, 960.0), 100);
            push(&mut h, UiIntent::SetTheme(theme));
            let results = state(&h).scan.results.clone().expect("results");
            let queue = state(&h).queue.clone();
            let mut protected = Vec::new();
            let mut review = Vec::new();
            for group in &results.groups {
                for member in &group.members {
                    let view = member_view(group, &queue, member);
                    if view.counterpart.is_none() {
                        continue;
                    }
                    if member.folder_protected || queue.is_protected(member.file) {
                        protected.push((group.id, member.name.clone()));
                    } else if decision_label(member, &view, &queue) == "Review first" {
                        review.push((group.id, member.name.clone()));
                    }
                }
            }
            // Expand only the groups holding a protected member, and search
            // for its name's stem, so frame `13:648`'s G002 (protected
            // Requirements_v2, review-first Requirements_draft) is in view
            // rather than below the dozens of groups sorted before it.
            let expanded: std::collections::BTreeSet<_> =
                protected.iter().map(|(group, _)| *group).collect();
            review.retain(|(group, _)| expanded.contains(group));
            assert!(!protected.is_empty() && !review.is_empty());
            state_mut(&mut h).findings.expanded = expanded;
            state_mut(&mut h).findings.search = "Requirements".to_owned();
            settle(&mut h);

            let painted = texts(&h);
            let name_rect = |name: &str| {
                painted
                    .iter()
                    .find(|t| t.galley.text() == name)
                    .map(text_rect)
            };
            let lock = glyphs::text(&h.ctx, Icon::Protected).expect("lock glyph");
            let document = glyphs::text(&h.ctx, Icon::Document).expect("document glyph");
            let mut seen_protected = 0;
            for (_, name) in &protected {
                let Some(row) = name_rect(name) else { continue };
                seen_protected += 1;
                assert!(
                    h.query_by_role_and_label(Role::CheckBox, &format!("Queue {name}"))
                        .is_none(),
                    "{theme:?}: protected {name} shows a queue checkbox"
                );
                let icon_in_row = |glyph: &str| {
                    painted.iter().any(|t| {
                        let r = text_rect(t);
                        t.galley.text() == glyph
                            && (r.center().y - row.center().y).abs() <= 3.0
                            && r.right() <= row.left() + TOLERANCE
                    })
                };
                assert!(icon_in_row(&lock), "{theme:?}: no lock beside {name}");
                assert!(
                    !icon_in_row(&document),
                    "{theme:?}: {name} still shows the file icon"
                );
            }
            assert!(
                seen_protected > 0,
                "{theme:?}: no protected row in view: {protected:?} among {:?}",
                painted
                    .iter()
                    .map(|t| t.galley.text().to_owned())
                    .collect::<Vec<_>>()
            );

            let amber = color(&h, Token::Amber);
            let amber_soft = color(&h, Token::AmberSoft);
            let rects: Vec<_> = shapes(&h)
                .into_iter()
                .filter_map(|s| match s {
                    egui::Shape::Rect(r) => Some(r),
                    _ => None,
                })
                .collect();
            let mut seen_review = 0;
            for (_, name) in &review {
                let label = format!("Queue {name}");
                let Some(boxed) = h
                    .query_by_role_and_label(Role::CheckBox, &label)
                    .map(|node| node.rect())
                else {
                    continue;
                };
                seen_review += 1;
                assert!(
                    rects.iter().any(|r| {
                        (r.rect.min - boxed.min).length() <= TOLERANCE
                            && (r.rect.max - boxed.max).length() <= TOLERANCE
                            && r.stroke.color == amber
                            && r.fill == amber_soft
                    }),
                    "{theme:?}: {name}'s queue checkbox is not amber-outlined"
                );
            }
            assert!(seen_review > 0, "{theme:?}: no review-first row in view");
        }
    }

    // ── T41: the round-2 review deviations ──────────────────────────────

    /// T41 item 1, frame `13:648`: the table's vertical scrollbar shows at
    /// rest, with no pointer over the table, as a `quiet` thumb inside the
    /// reserved space right of the columns. Every group is expanded so the
    /// rows overflow the body and there is something to scroll.
    #[test]
    fn conform_findings_table_scrollbar_thumb_shows_without_hover() {
        let mut h = launch_design("scrollbar", Vec2::new(1600.0, 960.0), 100);
        expand_all(&mut h);
        for theme in [ThemeOverride::Light, ThemeOverride::Dark] {
            push(&mut h, UiIntent::SetTheme(theme));
            h.remove_cursor();
            settle(&mut h);
            let table = table(&h);
            let reserve = Rect::from_min_max(
                egui::pos2(
                    table.right() - grouped_table::SCROLLBAR_RESERVE,
                    table.top(),
                ),
                table.max,
            );
            let quiet = color(&h, Token::Quiet);
            let in_reserve: Vec<egui::epaint::RectShape> = shapes(&h)
                .into_iter()
                .filter_map(|s| match s {
                    egui::Shape::Rect(r) => Some(r),
                    _ => None,
                })
                .filter(|r| inside(reserve, r.rect) && r.rect.height() >= 12.0)
                .collect();
            assert!(
                in_reserve
                    .iter()
                    .any(|r| r.fill == quiet && r.rect.width() >= 4.0),
                "{theme:?}: no quiet scrollbar thumb at rest in {reserve:?}; painted there: {:?}",
                in_reserve
                    .iter()
                    .map(|r| (r.rect, r.fill))
                    .collect::<Vec<_>>()
            );
        }
    }

    /// T41 item 3, at rest: at 1024 × 600 under 200% (512 points) the
    /// chips narrow their padding to fit the row, so every chip lies at or
    /// before the 12-unit work-area inset and the row starts at the
    /// workspace's left edge, unscrolled.
    #[test]
    fn conform_findings_chips_fit_before_the_inset_at_the_narrowest_width() {
        let mut h = launch_design("chip-inset", Vec2::new(1024.0, 600.0), 200);
        for theme in [ThemeOverride::Light, ThemeOverride::Dark] {
            push(&mut h, UiIntent::SetTheme(theme));
            h.remove_cursor();
            settle(&mut h);
            let edge = client(&h).right() - 12.0;
            let budget = h
                .get_by_role_and_label(Role::Group, WORKSPACE_TOOLBAR_NAME)
                .rect();
            let chips = chips(&h);
            assert_eq!(chips.len(), 5, "{theme:?}: chips {chips:?}");
            assert!(
                (chips[0].left() - budget.left()).abs() <= TOLERANCE,
                "{theme:?}: the chip row is scrolled: first chip {:?}, workspace left {}",
                chips[0],
                budget.left()
            );
            for chip in &chips {
                assert!(
                    chip.right() <= edge + TOLERANCE,
                    "{theme:?}: chip {chip:?} runs past the inset at {edge}"
                );
            }
        }
    }

    /// T41 item 3, at the reference size: every chip keeps the frame's
    /// 6-unit padding on both sides of its label (frame `13:648`), so the
    /// narrow padding never applies where the row fits.
    #[test]
    fn conform_findings_chips_keep_frame_padding_where_the_row_fits() {
        let h = launch_design("chip-pad", Vec2::new(1600.0, 960.0), 100);
        let chips = chips(&h);
        assert_eq!(chips.len(), 5, "five chips: {chips:?}");
        let labels: Vec<Rect> = texts(&h).into_iter().map(|t| text_rect(&t)).collect();
        for chip in &chips {
            let label = labels
                .iter()
                .find(|r| inside(*chip, **r) && r.width() > 4.0)
                .unwrap_or_else(|| panic!("no label in chip {chip:?}"));
            let (left, right) = (label.left() - chip.left(), chip.right() - label.right());
            assert!(
                (left - 6.0).abs() <= TOLERANCE && (right - 6.0).abs() <= TOLERANCE,
                "chip {chip:?} pads its label {left} and {right}, frame 6"
            );
        }
    }

    /// T41 item 4: at 1600 × 960 under 200% the second shrink stage never
    /// takes Group below its widest label, so every group label in view
    /// shows whole ("G002 (3)" and "G004 (3)" no longer both read "G00…").
    #[test]
    fn conform_findings_group_labels_show_whole_under_the_second_shrink_stage() {
        let h = launch_design("group-floor", Vec2::new(1600.0, 960.0), 200);
        let labels: Vec<String> = state(&h)
            .scan
            .results
            .as_ref()
            .expect("results")
            .groups
            .iter()
            .map(projections::group_label)
            .collect();
        let painted: Vec<(String, bool)> = texts(&h)
            .into_iter()
            .filter(|t| labels.iter().any(|l| l == t.galley.text()))
            .map(|t| (t.galley.text().to_owned(), t.galley.elided))
            .collect();
        assert!(!painted.is_empty(), "no group label in view");
        let elided: Vec<&String> = painted
            .iter()
            .filter(|(_, elided)| *elided)
            .map(|(l, _)| l)
            .collect();
        assert!(
            elided.is_empty(),
            "group labels elided: {elided:?} (of {})",
            painted.len()
        );
    }

    /// T41 item 5, frame `28:2171`: the Scanning line shows the folder in
    /// the Folder column's display form, trailing "/" included, while the
    /// engine's progress keeps its own string.
    #[test]
    fn conform_findings_scanning_line_keeps_the_trailing_slash() {
        let mut h = launch_design("scanning-slash", Vec2::new(1600.0, 960.0), 100);
        let s = state_mut(&mut h);
        s.scan.stage = ScanStage::Running;
        s.scan.progress = Some(ScanProgress {
            current_folder: "~/Documents/Archive".to_owned(),
            files_read: 8214,
            groups_found: 61,
            elapsed: std::time::Duration::from_secs(42),
        });
        // The segment animates, so the harness never settles: step only.
        for _ in 0..6 {
            h.step();
        }
        assert!(
            h.query_by_label("Reading files in ~/Documents/Archive/")
                .is_some(),
            "the Scanning line drops the trailing \"/\": {:?}",
            h.query_all_by_label_contains("Reading files in")
                .map(|n| n.accesskit_node().label().unwrap_or_default())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            state(&h)
                .scan
                .progress
                .as_ref()
                .map(|p| p.current_folder.as_str()),
            Some("~/Documents/Archive"),
            "the progress string changed"
        );
    }
}
