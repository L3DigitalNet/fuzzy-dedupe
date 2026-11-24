// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! PV-T22-001 and PV-T22-004 (plan T22, C-10): the numeric layout and token
//! targets of spec §11.3, §11.4, §11.6 and §11.7, measured on the running
//! application in a headless `egui_kittest` harness at 1600 × 960 and at the
//! 1024 × 600 minimum window.
//!
//! Every test sits in the `layout` module so `-E 'test(layout)'` selects
//! them. The app is built through `FuzzyDedupeApp::new` on the harness's own
//! context, on the design scenario it always starts with; each frame runs
//! `run_logic` then `draw`, as eframe does. No snapshot is rendered and no
//! GPU is needed: geometry comes from the accessibility tree (widget
//! bounds) and from the frame's painted shapes.
//!
//! Oracle. Expected values are the spec's own numbers, written here as
//! literals with their §11 source, never the app's constants: a constant
//! that drifted from the spec must fail here. Token colours are the one
//! exception, because hex colour literals may appear only in `ui/theme.rs`
//! (plan T11 invariant, enforced by `tests/theme_tokens.rs`): they are
//! parsed out of spec §11.7 at run time instead.
//!
//! Tolerance is ±1 logical unit (A-010), for the incidental border offsets
//! §11.3 describes. The 80% rule (NFR-007) is defined on the 1600 × 960
//! reference client area only; at 1024 × 600 the spec's own fixed rows
//! leave Compare's paired region at 438 of 574 units (frame `29:3648`
//! draws 436 of 572), so that size asserts the fill rule instead.
//!
//! Requirements: the workspace's `docs/specs/canonical/fuzzy-dedupe.md`,
//! read at run time for the §11.7 token table; fresh preference and log
//! directories under `CARGO_TARGET_TMPDIR`, unique per run, which the app
//! itself may write into.

mod layout {
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use egui::accesskit::Role;
    use egui::{Color32, Key, Rect, Vec2};
    use egui_kittest::Harness;
    use egui_kittest::kittest::{NodeT, Queryable};
    use fuzzy_dedupe_app::app::{AppDeps, Clock, FuzzyDedupeApp};
    use fuzzy_dedupe_app::measure::NoMeasurement;
    use fuzzy_dedupe_app::presentation::commands::UiIntent;
    use fuzzy_dedupe_app::presentation::state::{
        DialogKind, DialogSubject, PresentationState, RowId, ThemeOverride, WorkspaceState,
    };
    use fuzzy_dedupe_app::ui::findings::SEARCH_NAME;
    use fuzzy_dedupe_app::ui::shell::{RAIL_NAME, SETTINGS_NAME};
    use fuzzy_dedupe_app::ui::theme::{self, Mode, Token};
    use fuzzy_dedupe_app::ui::widgets::{grouped_table, status};
    use fuzzy_dedupe_domain::{IssueKind, MemberRole, QueueOrigin, QueueScope, ScanIssue};
    use fuzzy_dedupe_engine::fixtures::design;
    use fuzzy_dedupe_engine::service::{EngineConfig, ReviewCommand};
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

    const REFERENCE: Vec2 = Vec2::new(1600.0, 960.0);
    const MINIMUM: Vec2 = Vec2::new(1024.0, 600.0);

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
            LogDate::new(2026, 11, 23).expect("valid date")
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
            .join("layout")
            .join(format!("{name}-{nanos}"));
        let state = root.join("state");
        AppDirs::resolve(
            move |var| (var == "XDG_STATE_HOME").then(|| state.clone().into_os_string()),
            &root.join("home"),
        )
    }

    fn launch(name: &str, size: Vec2) -> Shell {
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
                        // 8-unit outer margin, which shifts and shrinks every
                        // panel. eframe gives `App::ui` the whole client area,
                        // so the app draws into a child spanning the content
                        // rect, as it would in the window. (`build_eframe`
                        // would avoid this, but needs kittest's `eframe`
                        // feature, which the manifest does not enable.)
                        let full = ui.ctx().content_rect();
                        let mut window = ui.new_child(egui::UiBuilder::new().max_rect(full));
                        app.draw(&mut window);
                    }
                },
                Slot { app: None },
            );
        let app = FuzzyDedupeApp::new(&harness.ctx, deps).expect("engine starts");
        harness.state_mut().app = Some(app);
        harness.run();
        run_until(&mut harness, "the design scenario's results", |s| {
            s.scan.results.is_some()
        });
        harness.run();
        harness
    }

    fn state(h: &Shell) -> &PresentationState {
        &h.state().app.as_ref().expect("app").state
    }

    fn state_mut(h: &mut Shell) -> &mut PresentationState {
        &mut h.state_mut().app.as_mut().expect("app").state
    }

    fn push(h: &mut Shell, intent: UiIntent) {
        h.state_mut().app.as_mut().expect("app").push_intent(intent);
        h.run();
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

    /// The design pair of frame `13:1304` in Compare, side A the
    /// counterpart and side B the target (T35), drawn once its comparison
    /// is ready.
    fn open_compare(h: &mut Shell) {
        let pair = design::sample().last_compared.expect("design pair");
        let (target, counterpart) = (pair.b, pair.a);
        let group = state(h)
            .scan
            .results
            .as_ref()
            .and_then(|r| r.group_of(target))
            .expect("the target is a member")
            .id;
        push(
            h,
            UiIntent::OpenCompare {
                target,
                counterpart,
                return_row: Some(RowId::Member {
                    group,
                    file: target,
                }),
            },
        );
        run_until(h, "Compare's comparison", move |s| {
            s.workspace == WorkspaceState::Compare
                && s.compare.comparison.aligned().is_some()
                && s.compare
                    .comparison
                    .key
                    .is_some_and(|k| (k.target, k.counterpart) == (target, counterpart))
        });
        h.run();
    }

    // ── Measuring ────────────────────────────────────────────────────────

    fn within(actual: f32, expected: f32) -> bool {
        (actual - expected).abs() <= TOLERANCE
    }

    #[track_caller]
    fn assert_units(actual: f32, expected: f32, what: &str) {
        assert!(
            within(actual, expected),
            "{what}: {actual} units, expected {expected} ±{TOLERANCE} (A-010)"
        );
    }

    /// Every rectangle this frame painted, nested shape lists included.
    fn painted_rects(h: &Shell) -> Vec<egui::epaint::RectShape> {
        fn walk(shape: &egui::Shape, out: &mut Vec<egui::epaint::RectShape>) {
            match shape {
                egui::Shape::Rect(rect) => out.push(rect.clone()),
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

    fn mode_of(h: &Shell) -> Mode {
        match h.ctx.theme() {
            egui::Theme::Light => Mode::Light,
            egui::Theme::Dark => Mode::Dark,
        }
    }

    fn color(h: &Shell, token: Token) -> Color32 {
        theme::palette(mode_of(h)).color(token)
    }

    /// The shell's regions, from their accessibility bounds. The work area
    /// is what the rail and status bar leave (spec §11.3: "Consume
    /// remaining window space"); it has no node of its own.
    struct Regions {
        window: Rect,
        rail: Rect,
        status: Rect,
        work: Rect,
    }

    fn shell_regions(h: &Shell) -> Regions {
        let window = h.ctx.content_rect();
        let rail = h.get_by_label(RAIL_NAME).rect();
        let status = h.get_by_label(status::ACCESSIBLE_NAME).rect();
        let work = Rect::from_min_max(
            egui::pos2(rail.right(), window.top()),
            egui::pos2(window.right(), status.top()),
        );
        Regions {
            window,
            rail,
            status,
            work,
        }
    }

    /// The Findings table's outer rectangle: the bounds of its focus
    /// target, which `grouped_table::show` lays over the whole table.
    fn table_rect(h: &Shell) -> Rect {
        let id = grouped_table::table_id().accesskit_id();
        h.get_by(move |node| node.locate().0 == id).rect()
    }

    /// The header cells, left to right: the `code`-filled rectangles the
    /// table paints along its top edge, inside its 1-unit border.
    fn header_cells(h: &Shell) -> Vec<Rect> {
        let table = table_rect(h);
        let fill = color(h, Token::Code);
        let mut cells: Vec<Rect> = painted_rects(h)
            .into_iter()
            .filter(|r| r.fill == fill)
            .map(|r| r.rect)
            .filter(|r| within(r.top(), table.top() + 1.0) && r.bottom() < table.top() + 60.0)
            .filter(|r| r.left() >= table.left() && r.left() < table.right())
            .collect();
        cells.sort_by(|a, b| a.left().total_cmp(&b.left()));
        cells.dedup_by(|a, b| within(a.left(), b.left()));
        cells
    }

    /// The body rows, top to bottom: the full-width row fills below the
    /// header (group headers on `rail`, members on `surface`, the focused
    /// row on `primarySoft`, context rows on `code`).
    fn body_rows(h: &Shell) -> Vec<Rect> {
        let table = table_rect(h);
        let header_bottom = header_cells(h)
            .iter()
            .map(Rect::bottom)
            .fold(f32::NEG_INFINITY, f32::max);
        let fills =
            [Token::Rail, Token::Surface, Token::PrimarySoft, Token::Code].map(|t| color(h, t));
        let mut rows: Vec<Rect> = painted_rects(h)
            .into_iter()
            .filter(|r| fills.contains(&r.fill))
            .map(|r| r.rect)
            .filter(|r| within(r.left(), table.left() + 1.0) && r.width() > 500.0)
            .filter(|r| r.top() >= header_bottom - 0.5 && r.top() < table.bottom())
            .collect();
        rows.sort_by(|a, b| a.top().total_cmp(&b.top()));
        rows.dedup_by(|a, b| within(a.top(), b.top()));
        rows
    }

    fn button(h: &Shell, prefix: &str) -> Rect {
        h.query_all_by_role(Role::Button)
            .find(|n| {
                n.accesskit_node()
                    .label()
                    .is_some_and(|l| l.starts_with(prefix))
            })
            .unwrap_or_else(|| panic!("a button labeled {prefix:?}…"))
            .rect()
    }

    /// Compare's two pinned identities, A then B (one 54-high label node
    /// each), and the two pane rectangles below them (the `surface` fill
    /// each pane paints around its Document node).
    struct Paired {
        identities: [Rect; 2],
        panes: [Rect; 2],
    }

    impl Paired {
        fn region(&self) -> Rect {
            self.identities
                .iter()
                .chain(&self.panes)
                .fold(Rect::NOTHING, |acc, r| acc.union(*r))
        }
    }

    fn paired(h: &Shell) -> Paired {
        let docs = ["A", "B"].map(|side| h.get_by_role_and_label(Role::Document, side).rect());
        let surface = color(h, Token::Surface);
        let rects = painted_rects(h);
        let panes = docs.map(|doc| {
            rects
                .iter()
                .filter(|r| r.fill == surface && r.rect.contains_rect(doc))
                .map(|r| r.rect)
                .min_by(|a, b| a.area().total_cmp(&b.area()))
                .expect("a pane around its document")
        });
        // Found within 20 units above its pane rather than flush with it, so
        // a gap between the two is measured by `check_compare` instead of
        // making the identity unfindable.
        let identities = panes.map(|pane| {
            h.query_all_by_role(Role::Label)
                .map(|n| n.rect())
                .filter(|r| {
                    r.bottom() <= pane.top() + 1.0
                        && r.bottom() > pane.top() - 20.0
                        && within(r.left(), pane.left())
                })
                .max_by(|a, b| a.height().total_cmp(&b.height()))
                .expect("a pinned identity above the pane")
        });
        Paired { identities, panes }
    }

    // ── PV-T22-001: shell and Findings geometry ──────────────────────────

    /// Every mismatch of one layout, reported together: a single run then
    /// shows the whole drift instead of only the first offset, which is
    /// what a correction task needs.
    #[derive(Default)]
    struct Checks(Vec<String>);

    impl Checks {
        fn units(&mut self, actual: f32, expected: f32, what: &str) {
            if !within(actual, expected) {
                self.0
                    .push(format!("{what}: {actual} units, expected {expected}"));
            }
        }

        fn that(&mut self, holds: bool, what: &str) {
            if !holds {
                self.0.push(what.to_owned());
            }
        }

        #[track_caller]
        fn finish(self, layout: &str) {
            assert!(
                self.0.is_empty(),
                "{layout}: {} mismatch(es) beyond ±{TOLERANCE} (A-010):\n  {}",
                self.0.len(),
                self.0.join("\n  ")
            );
        }
    }

    /// Spec §11.4 reference widths, in default column order.
    const COLUMN_WIDTHS: [(&str, f32); 10] = [
        ("Queue", 48.0),
        ("Group", 104.0),
        ("Match", 240.0),
        ("Unique", 76.0),
        ("File name", 280.0),
        ("Folder", 184.0),
        ("Modified", 120.0),
        ("Size", 72.0),
        ("Protected", 88.0),
        ("Decision", 196.0),
    ];

    /// The columns the collapse order hides below 1280 (spec §11.3, step 2).
    const COLLAPSED: [&str; 3] = ["Modified", "Size", "Protected"];

    /// The shell rows both workspaces share (spec §11.3): the rail, the
    /// 26-high status bar, the work area between them, and the 32-high
    /// toolbar row at the 12-unit inset with 30-high controls centred in it.
    /// `first` is the toolbar's leading control.
    fn check_shell(c: &mut Checks, h: &Shell, size: Vec2, first: Rect) {
        let reference = size == REFERENCE;
        let s = shell_regions(h);
        let (rail_w, work_w, work_h) = if reference {
            (152.0, 1448.0, 934.0)
        } else {
            (48.0, 976.0, 574.0)
        };
        c.units(s.rail.width(), rail_w, "rail width");
        c.units(s.rail.left(), s.window.left(), "rail left edge");
        c.units(s.rail.height(), s.work.height(), "rail height");
        c.units(s.status.height(), 26.0, "status bar height");
        c.units(s.status.width(), s.window.width(), "status bar width");
        c.units(s.status.bottom(), s.window.bottom(), "status bar foot");
        c.units(s.work.width(), work_w, "work area width");
        c.units(s.work.height(), work_h, "work area height");

        let toolbar_top = s.work.top() + 12.0;
        c.units(first.left(), s.work.left() + 12.0, "toolbar left inset");
        c.units(first.height(), 30.0, "leading control height");
        // A 32-high row centres its 30-high controls 16 below its top.
        c.units(first.center().y - toolbar_top, 16.0, "toolbar row centre");
        let settings = h.get_by_label(SETTINGS_NAME).rect();
        c.units(settings.height(), 30.0, "Settings control height");
        c.units(
            settings.right(),
            s.work.right() - 12.0,
            "toolbar right inset",
        );
        c.units(
            settings.center().y - toolbar_top,
            16.0,
            "toolbar row centre (Settings)",
        );
    }

    /// The search field as drawn: the text edit plus the search glyph that
    /// sits at its trailing end, which together make the §11.3 340-unit
    /// field (frame `13:692`).
    fn search_field(h: &Shell) -> Rect {
        let edit = h.get_by_role_and_label(Role::TextInput, SEARCH_NAME).rect();
        let glyph = h
            .query_all_by_role(Role::Label)
            .map(|n| n.rect())
            .find(|r| {
                r.left() >= edit.right() - 1.0
                    && r.left() <= edit.right() + 12.0
                    && within(r.center().y, edit.center().y)
            })
            .expect("the search glyph after the text edit");
        edit.union(glyph)
    }

    fn check_findings(h: &Shell, size: Vec2) {
        let reference = size == REFERENCE;
        let mut c = Checks::default();
        let search = search_field(h);
        check_shell(&mut c, h, size, search);
        let s = shell_regions(h);
        c.units(search.width(), 340.0, "search field width");

        // Toolbar 32, gap 8, then the 28-high filter row with 26-high chips.
        let filter_top = s.work.top() + 12.0 + 32.0 + 8.0;
        let chip = button(h, "All ");
        c.units(chip.height(), 26.0, "filter chip height");
        c.units(chip.center().y - filter_top, 14.0, "filter row centre");

        // Gap 8, then the table across the inset work area.
        let table = table_rect(h);
        c.units(
            table.top() - 8.0 - filter_top,
            28.0,
            "filter row + gap to the table",
        );
        c.units(table.left(), s.work.left() + 12.0, "table left inset");
        c.units(table.width(), s.work.width() - 24.0, "table width");
        if reference {
            c.units(table.width(), 1424.0, "table width (spec §11.3)");
            c.units(table.height(), 822.0, "table height (spec §11.3)");
        } else {
            // No 1024 × 600 table height is specified. The settled reading of
            // §11.3's 1424 × 822 (plan T37) is a table that grows with the
            // window down to a 24-unit bottom margin, which leaves 462 here.
            c.units(
                s.work.bottom() - table.bottom(),
                24.0,
                "table bottom margin (T37)",
            );
            c.units(table.height(), 462.0, "table height (T37)");
        }

        let header = header_cells(h);
        let rows = body_rows(h);
        assert!(!header.is_empty(), "the table header is drawn");
        assert!(rows.len() >= 8, "at least 8 body rows in view: {rows:?}");
        for (cell, (title, _)) in header.iter().zip(&COLUMN_WIDTHS) {
            c.units(cell.height(), 30.0, &format!("{title} header height"));
        }
        c.units(
            rows[0].top(),
            header[0].bottom(),
            "first row under the header",
        );
        for (i, row) in rows.iter().enumerate() {
            c.units(row.height(), 28.0, &format!("body row {i} height"));
        }
        for (i, pair) in rows.windows(2).enumerate() {
            c.units(
                pair[1].top() - pair[0].top(),
                28.0,
                &format!("body row {i} to {} pitch", i + 1),
            );
        }

        let titles: Vec<String> = h
            .query_all_by_role(Role::Label)
            .filter_map(|n| n.accesskit_node().value())
            .collect();
        let shown: Vec<(&str, f32)> = COLUMN_WIDTHS
            .into_iter()
            .filter(|(title, _)| reference || !COLLAPSED.contains(title))
            .collect();
        if reference {
            c.that(header.len() == 10, "all ten columns in view at 1600");
        } else {
            for hidden in COLLAPSED {
                c.that(
                    !titles.iter().any(|t| t == hidden),
                    &format!("{hidden} hides below 1280"),
                );
            }
            c.that(
                header.len() == shown.len(),
                "every collapsed-set column is drawn",
            );
        }
        c.units(
            header[0].left(),
            table.left() + 1.0,
            "Queue column left edge",
        );
        if reference {
            for (cell, (title, width)) in header.iter().zip(&shown) {
                c.units(cell.width(), *width, &format!("{title} column width"));
            }
        } else {
            check_flexible_columns(&mut c, &header, &shown, table);
        }
        for pair in header.windows(2) {
            c.units(pair[1].left(), pair[0].right(), "adjacent column edges");
        }
        c.finish(&format!("Findings at {} × {}", size.x, size.y));
    }

    /// The minimum window's columns (T40 F8): File name and Folder between
    /// their minimums and reference widths, filling the rest of the table;
    /// every other column at its reference width; Decision in the table.
    fn check_flexible_columns(c: &mut Checks, header: &[Rect], shown: &[(&str, f32)], table: Rect) {
        // T40 F8, §11.4: "File name and folder widths are flexible".
        let mut flexible = 0.0;
        for (cell, (title, width)) in header.iter().zip(shown) {
            let minimum = grouped_table::FLEX_MINIMUMS
                .iter()
                .find(|(column, _)| grouped_table::column_title(*column) == *title)
                .map(|(_, minimum)| *minimum);
            if let Some(minimum) = minimum {
                flexible += cell.width();
                c.that(
                    cell.width() >= minimum - 1.0 && cell.width() <= width + 1.0,
                    &format!("{title} width {} within {minimum}..={width}", cell.width()),
                );
            } else {
                c.units(cell.width(), *width, &format!("{title} column width"));
            }
        }
        let fixed: f32 = shown
            .iter()
            .filter(|(title, _)| !["File name", "Folder"].contains(title))
            .map(|(_, width)| width)
            .sum();
        let rest = table.width() - 2.0 - grouped_table::SCROLLBAR_RESERVE - fixed;
        c.units(
            flexible,
            rest,
            "File name + Folder fill the rest of the table",
        );
        let decision = header.last().copied().unwrap_or(Rect::NOTHING);
        c.that(
            table.contains_rect(decision),
            &format!("Decision {decision:?} inside the table {table:?}"),
        );
    }

    #[test]
    fn layout_findings_matches_the_reference_geometry_at_1600_by_960() {
        let h = launch("findings-1600", REFERENCE);
        check_findings(&h, REFERENCE);
    }

    #[test]
    fn layout_findings_matches_the_reference_geometry_at_1024_by_600() {
        let h = launch("findings-1024", MINIMUM);
        check_findings(&h, MINIMUM);
    }

    // ── PV-T22-001: Compare geometry ─────────────────────────────────────

    fn check_compare(h: &Shell, size: Vec2) {
        let reference = size == REFERENCE;
        let mut c = Checks::default();
        check_shell(&mut c, h, size, button(h, "‹ Findings"));
        let s = shell_regions(h);
        let p = paired(h);
        let region = p.region();
        let (pane_w, region_w) = if reference {
            (708.0, 1424.0)
        } else {
            // A-010: 472 for the borderless 1024-unit client area (frame
            // `29:3648` draws 471 inside a 1-unit window outline).
            (472.0, 952.0)
        };
        for (side, (identity, pane)) in ["A", "B"].iter().zip(p.identities.iter().zip(&p.panes)) {
            c.units(pane.width(), pane_w, &format!("pane {side} width"));
            c.units(identity.width(), pane_w, &format!("identity {side} width"));
            c.units(identity.height(), 54.0, &format!("identity {side} height"));
            c.units(
                pane.top(),
                identity.bottom(),
                &format!("pane {side} text area top (directly under its identity)"),
            );
        }
        c.units(
            p.panes[1].left() - p.panes[0].right(),
            8.0,
            "pane separator",
        );
        c.units(region.width(), region_w, "paired region width");
        c.units(
            region.left(),
            s.work.left() + 12.0,
            "paired region left inset",
        );
        // Toolbar 32, evidence 28 and change 28 rows, each followed by an
        // 8-unit gap, above the panes (frame `13:1304`, y = 124).
        c.units(
            region.top() - s.work.top(),
            12.0 + 32.0 + 8.0 + 28.0 + 8.0 + 28.0 + 8.0,
            "paired region top",
        );
        c.units(
            region.bottom(),
            s.work.bottom() - 12.0,
            "paired region foot inset",
        );
        if reference {
            c.units(region.height(), 798.0, "paired region height (spec §11.3)");
            for (side, pane) in ["A", "B"].iter().zip(&p.panes) {
                c.units(
                    pane.height(),
                    744.0,
                    &format!("pane {side} text area height (spec §11.3)"),
                );
            }
        }
        c.finish(&format!("Compare at {} × {}", size.x, size.y));
    }

    #[test]
    fn layout_compare_matches_the_reference_geometry_at_1600_by_960() {
        let mut h = launch("compare-1600", REFERENCE);
        open_compare(&mut h);
        check_compare(&h, REFERENCE);
    }

    #[test]
    fn layout_compare_matches_the_reference_geometry_at_1024_by_600() {
        let mut h = launch("compare-1024", MINIMUM);
        open_compare(&mut h);
        check_compare(&h, MINIMUM);
    }

    // ── PV-T22-001: popup sizes ──────────────────────────────────────────

    /// The painted popup: the area of egui's top modal layer, which the
    /// dialog host's frame fills exactly, its border included.
    fn popup_rect(h: &Shell) -> Rect {
        h.ctx.memory(|memory| {
            let layer = memory.top_modal_layer().expect("a popup is drawn");
            memory.area_rect(layer.id).expect("the popup's area")
        })
    }

    #[track_caller]
    fn assert_popup(h: &Shell, expected: Vec2, what: &str) {
        let size = popup_rect(h).size();
        assert_units(size.x, expected.x, &format!("{what} width"));
        assert_units(size.y, expected.y, &format!("{what} height"));
    }

    fn open_popup(h: &mut Shell, kind: DialogKind) {
        if kind == DialogKind::UniqueTextReview {
            // Review unique text opens from a queue request that needs
            // review: the design scenario's fuzzy row against its
            // counterpart.
            let results = state(h).scan.results.clone().expect("results");
            let member = results
                .groups
                .iter()
                .flat_map(|g| g.members.iter())
                .find(|m| m.name == "Design_Spec_draft.txt")
                .expect("the design scenario's fuzzy row");
            let MemberRole::Member(link) = member.role else {
                panic!("the fuzzy row is a member");
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
            run_until(h, "Review unique text", |s| {
                s.dialog.dialog().map(|d| d.kind) == Some(DialogKind::UniqueTextReview)
            });
        } else {
            let subject = if kind == DialogKind::QueueIdentical {
                DialogSubject::QueueIdentical(QueueScope::AllByteIdenticalGroups)
            } else {
                DialogSubject::None
            };
            push(
                h,
                UiIntent::OpenDialog {
                    kind,
                    subject,
                    invoker: None,
                },
            );
        }
        h.run();
        assert_eq!(state(h).dialog.dialog().map(|d| d.kind), Some(kind));
    }

    fn close_popup(h: &mut Shell) {
        h.key_press(Key::Escape);
        h.run();
        assert!(state(h).dialog.dialog().is_none(), "the popup closed");
    }

    /// Spec §11.6 reference sizes.
    const POPUP_SIZES: [(DialogKind, Vec2); 7] = [
        (DialogKind::MatchingRules, Vec2::new(420.0, 421.0)),
        (DialogKind::NewScan, Vec2::new(640.0, 528.0)),
        (DialogKind::UniqueTextReview, Vec2::new(600.0, 422.0)),
        (DialogKind::KeyboardShortcuts, Vec2::new(960.0, 494.0)),
        (DialogKind::About, Vec2::new(480.0, 387.0)),
        (DialogKind::QueueIdentical, Vec2::new(560.0, 384.0)),
        (DialogKind::Cleanup, Vec2::new(960.0, 423.0)),
    ];

    fn assert_popup_sizes(size: Vec2) {
        let mut h = launch(&format!("popups-{}", size.x), size);
        for (kind, expected) in POPUP_SIZES {
            open_popup(&mut h, kind);
            // Each reference size fits inside both windows less the 16-unit
            // popup inset, so neither size constrains it.
            assert_popup(&h, expected, &format!("{kind:?}"));
            close_popup(&mut h);
        }

        // New scan with matching rules expanded is 778 high (spec §11.6);
        // where that does not fit, it is constrained to the viewport less the
        // 16-unit inset on each side (§11.10, "constrain dialogs to the
        // usable viewport").
        open_popup(&mut h, DialogKind::NewScan);
        h.get_by_role_and_label(Role::Button, "Matching rules section")
            .click();
        h.run();
        let expanded = Vec2::new(640.0, 778.0_f32.min(size.y - 32.0));
        assert_popup(&h, expanded, "New scan, matching rules expanded");
        close_popup(&mut h);
    }

    #[test]
    fn layout_popups_open_at_their_reference_sizes_at_1600_by_960() {
        assert_popup_sizes(REFERENCE);
    }

    #[test]
    fn layout_popups_open_at_their_reference_sizes_at_1024_by_600() {
        assert_popup_sizes(MINIMUM);
    }

    // ── PV-T22-001: token colours ────────────────────────────────────────

    /// The text of spec §11.7, from its heading to the next `###` heading.
    fn spec_section_11_7() -> String {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../docs/specs/canonical/fuzzy-dedupe.md");
        let spec = std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("read {}: {err}", path.display()));
        let start = spec.find("### 11.7 ").expect("spec has a §11.7 heading");
        let rest = &spec[start..];
        let end = rest[4..].find("\n### ").map_or(rest.len(), |i| i + 4);
        rest[..end].to_owned()
    }

    /// The opaque RGB of a §11.7 colour cell such as `#101828 at 6%`.
    fn spec_rgb(cell: &str) -> Color32 {
        let hash = cell
            .find('#')
            .unwrap_or_else(|| panic!("no colour in {cell:?}"));
        let channel = |i: usize| {
            u8::from_str_radix(&cell[hash + 1 + i..hash + 3 + i], 16).expect("hex channel")
        };
        Color32::from_rgb(channel(0), channel(2), channel(4))
    }

    /// Token name → (light, dark), from the §11.7 token table.
    fn spec_tokens() -> BTreeMap<String, (Color32, Color32)> {
        let mut tokens = BTreeMap::new();
        for line in spec_section_11_7().lines().filter(|l| l.starts_with("| `")) {
            let cells: Vec<&str> = line
                .trim()
                .trim_matches('|')
                .split('|')
                .map(str::trim)
                .collect();
            let names = cells[0].split(" / ").map(|n| n.trim_matches('`'));
            let light = cells[1].split(" / ");
            let dark = cells[2].split(" / ");
            for ((name, l), d) in names.zip(light).zip(dark) {
                tokens.insert(name.to_owned(), (spec_rgb(l), spec_rgb(d)));
            }
        }
        // 19 table rows expand to 26 tokens; any other count means the
        // table changed shape and this parser needs review.
        assert_eq!(tokens.len(), 26, "spec §11.7 token count");
        tokens
    }

    #[test]
    fn layout_tokens_match_spec_and_paint_the_shell_and_table() {
        let spec = spec_tokens();
        for token in Token::ALL {
            let (light, dark) = spec
                .get(token.name())
                .unwrap_or_else(|| panic!("{} in spec §11.7", token.name()));
            assert_eq!(
                theme::palette(Mode::Light).color(token),
                *light,
                "{} light",
                token.name()
            );
            assert_eq!(
                theme::palette(Mode::Dark).color(token),
                *dark,
                "{} dark",
                token.name()
            );
        }

        let mut h = launch("tokens", REFERENCE);
        for (choice, mode) in [
            (ThemeOverride::Light, Mode::Light),
            (ThemeOverride::Dark, Mode::Dark),
        ] {
            state_mut(&mut h).preferences.theme = choice;
            h.run();
            assert_eq!(mode_of(&h), mode, "{choice:?} applied");
            let s = shell_regions(&h);
            let table = table_rect(&h);
            let rects = painted_rects(&h);
            let painted = |token: Token, area: Rect| {
                let fill = color(&h, token);
                rects.iter().any(|r| {
                    r.fill == fill
                        && within(r.rect.width(), area.width())
                        && within(r.rect.height(), area.height())
                        && within(r.rect.left(), area.left())
                        && within(r.rect.top(), area.top())
                })
            };
            assert!(painted(Token::Rail, s.rail), "{mode:?}: rail on `rail`");
            assert!(
                painted(Token::Canvas, s.work),
                "{mode:?}: work area on `canvas`"
            );
            assert!(
                painted(Token::Surface, table),
                "{mode:?}: table on `surface`"
            );
            let header = header_cells(&h);
            assert_eq!(header.len(), 10, "{mode:?}: header cells on `code`");
        }
    }

    // ── PV-T22-004: the 80% rule ─────────────────────────────────────────

    /// NFR-007: the main review surface's share of its work-area height.
    fn findings_ratio(h: &Shell) -> f32 {
        table_rect(h).height() / shell_regions(h).work.height()
    }

    fn compare_ratio(h: &Shell) -> f32 {
        paired(h).region().height() / shell_regions(h).work.height()
    }

    #[test]
    fn layout_table_and_paired_region_take_80_percent_of_their_work_areas() {
        let mut h = launch("ratio-1600", REFERENCE);
        let table = findings_ratio(&h);
        assert!(
            table >= 0.80,
            "Findings table at 1600 × 960: {table:.3} of the work area"
        );
        open_compare(&mut h);
        let region = compare_ratio(&h);
        assert!(
            region >= 0.80,
            "Compare paired region at 1600 × 960: {region:.3} of the work area"
        );

        // At the minimum window the Findings table still clears 80%; the
        // paired region fills to the inset (asserted in the geometry test)
        // but the spec's fixed rows above it hold it near 76%, which
        // NFR-007, defined on the reference area, does not cover.
        let h = launch("ratio-1024", MINIMUM);
        let table = findings_ratio(&h);
        assert!(
            table >= 0.80,
            "Findings table at 1024 × 600: {table:.3} of the work area"
        );
    }

    // ── Negative controls ────────────────────────────────────────────────

    /// PV-T22-001's control: a 2-unit row-height change fails. The rows
    /// measured here at 28 would fail the same check against 26 or 30, so
    /// the measurement resolves a 2-unit drift in either direction.
    #[test]
    fn layout_negative_control_a_two_unit_row_change_fails() {
        let h = launch("control-rows", REFERENCE);
        let rows = body_rows(&h);
        assert!(!rows.is_empty(), "rows in view");
        for row in rows {
            assert!(within(row.height(), 28.0), "row is 28: {row:?}");
            for drifted in [26.0, 30.0] {
                assert!(
                    !within(row.height(), drifted),
                    "{drifted} would pass: {row:?}"
                );
            }
        }
    }

    /// PV-T22-004's control: a panel added above the table (here the
    /// discovery-problem list, about 116 units with six issues) pushes the
    /// table below 80% and the ratio check reports it.
    #[test]
    fn layout_negative_control_an_added_panel_breaks_the_80_percent_rule() {
        let mut h = launch("control-ratio", REFERENCE);
        let before = findings_ratio(&h);
        let s = state_mut(&mut h);
        s.scan.issues = IssueKind::ALL
            .into_iter()
            .enumerate()
            .map(|(i, kind)| ScanIssue {
                display_path: format!("~/Documents/unreadable-{i}.txt"),
                kind,
            })
            .collect();
        s.findings.show_issues = true;
        h.run();
        let after = findings_ratio(&h);
        assert!(before >= 0.80, "the unmodified table passes: {before:.3}");
        assert!(after < 0.80, "the added panel fails the rule: {after:.3}");
    }
}
