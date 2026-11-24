// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! PV-T37-001 (plan T37): the §11.3 vertical rhythm of the Findings and
//! Compare stacks, the 340-unit search field, the Findings table's 24-unit
//! bottom margin and Compare's pane geometry, measured on the running app
//! at 1600 × 960 and at the 1024 × 600 minimum window.
//!
//! Every test sits in the `workspace_rhythm` module so
//! `-E 'test(workspace_rhythm)'` selects them; no name contains `layout`,
//! so T22's `-E 'test(layout)'` (PV-T22-001) never selects these. This file
//! measures only the geometry T37 changes; T22's `tests/layout.rs` stays
//! the full C-10 oracle.
//!
//! Method (copied from T22's layout leg, commit `20a97fa`): the app draws
//! into a child spanning the whole content rect, as eframe gives `App::ui`
//! the whole client area, and geometry is read from the accessibility tree
//! (widget bounds) and the frame's painted shapes. Expected values are the
//! spec's and the frames' own numbers, written as literals with their
//! source, never the app's constants, so a drifted constant fails here.
//! Tolerance is ±1 logical unit (A-010).
//!
//! Requirements: fresh preference and log directories under
//! `CARGO_TARGET_TMPDIR`, unique per run, which the app itself may write
//! into.

mod workspace_rhythm {
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use egui::accesskit::Role;
    use egui::{Event, Key, Modifiers, Rect, Vec2};
    use egui_kittest::Harness;
    use egui_kittest::kittest::{NodeT, Queryable};
    use fuzzy_dedupe_app::app::{AppDeps, Clock, FuzzyDedupeApp};
    use fuzzy_dedupe_app::measure::NoMeasurement;
    use fuzzy_dedupe_app::presentation::commands::UiIntent;
    use fuzzy_dedupe_app::presentation::state::{PresentationState, RowId, WorkspaceState};
    use fuzzy_dedupe_app::ui::findings::SEARCH_NAME;
    use fuzzy_dedupe_app::ui::shell::RAIL_NAME;
    use fuzzy_dedupe_app::ui::theme::{self, Mode, Token};
    use fuzzy_dedupe_app::ui::widgets::{diff_view, grouped_table, status};
    use fuzzy_dedupe_domain::CompareMode;
    use fuzzy_dedupe_engine::fixtures::design;
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
            .join("workspace_rhythm")
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
                        // 8-unit outer margin, which would shift and shrink
                        // every panel. eframe gives `App::ui` the whole client
                        // area, so the app draws into a child spanning the
                        // content rect, as it would in the window.
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

    /// Every mismatch of one stack, reported together, so a single run
    /// shows the whole drift instead of only the first offset.
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
        fn finish(self, stack: &str) {
            assert!(
                self.0.is_empty(),
                "{stack}: {} mismatch(es) beyond ±{TOLERANCE} (A-010):\n  {}",
                self.0.len(),
                self.0.join("\n  ")
            );
        }
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

    fn surface(h: &Shell) -> egui::Color32 {
        let mode = match h.ctx.theme() {
            egui::Theme::Light => Mode::Light,
            egui::Theme::Dark => Mode::Dark,
        };
        theme::palette(mode).color(Token::Surface)
    }

    /// The work area: what the rail and the status bar leave (spec §11.3).
    /// It has no node of its own.
    fn work_area(h: &Shell) -> Rect {
        let window = h.ctx.content_rect();
        let rail = h.get_by_label(RAIL_NAME).rect();
        let status = h.get_by_label(status::ACCESSIBLE_NAME).rect();
        Rect::from_min_max(
            egui::pos2(rail.right(), window.top()),
            egui::pos2(window.right(), status.top()),
        )
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

    /// Every node's accessible text (label, else value) with its bounds.
    fn texts(h: &Shell) -> Vec<(String, Rect)> {
        h.query_all_by(|_| true)
            .filter_map(|n| {
                let node = n.accesskit_node();
                node.label()
                    .or_else(|| node.value())
                    .map(|text| (text, n.rect()))
            })
            .collect()
    }

    // ── Findings ─────────────────────────────────────────────────────────

    /// The Findings table's outer rectangle: the bounds of its focus
    /// target, which `grouped_table::show` lays over the whole table.
    fn table_rect(h: &Shell) -> Rect {
        let id = grouped_table::table_id().accesskit_id();
        h.get_by(move |node| node.locate().0 == id).rect()
    }

    /// The search field as drawn: the text edit plus the search glyph at
    /// its trailing end, which together make the §11.3 340-unit field
    /// (frame `13:692`).
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
        let mut c = Checks::default();
        let work = work_area(h);

        // Acceptance (1): toolbar 32 at the 12 inset, gap 8, the 28-high
        // filter row with its chips centred 14 below its top, gap 8, then
        // the table at 88 (frame `13:648`).
        let filter_top = work.top() + 12.0 + 32.0 + 8.0;
        let chip = button(h, "All ");
        c.units(chip.center().y - filter_top, 14.0, "filter chip centre");
        let table = table_rect(h);
        c.units(table.top() - work.top(), 88.0, "table top");
        c.units(
            table.top() - (filter_top + 28.0),
            8.0,
            "gap from the filter row",
        );

        // Acceptance (2): the table grows with the window down to a
        // 24-unit bottom margin (settled reading of §11.3's 1424 × 822).
        c.units(work.bottom() - table.bottom(), 24.0, "table bottom margin");
        let height = if size == REFERENCE { 822.0 } else { 462.0 };
        c.units(table.height(), height, "table height");

        // Acceptance (3): text box plus glyph is 340 (§11.3, frame `13:692`).
        c.units(search_field(h).width(), 340.0, "search field width");
        c.finish(&format!("Findings at {} × {}", size.x, size.y));
    }

    #[test]
    fn workspace_rhythm_findings_stack_at_1600_by_960() {
        let h = launch("findings-1600", REFERENCE);
        check_findings(&h, REFERENCE);
    }

    #[test]
    fn workspace_rhythm_findings_stack_at_1024_by_600() {
        let h = launch("findings-1024", MINIMUM);
        check_findings(&h, MINIMUM);
    }

    // ── Compare ──────────────────────────────────────────────────────────

    /// Compare's two pinned identities, A then B, and the two pane
    /// rectangles below them (the `surface` fill each pane paints around
    /// its Document node).
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
        let fill = surface(h);
        let rects = painted_rects(h);
        let panes = docs.map(|doc| {
            rects
                .iter()
                .filter(|r| r.fill == fill && r.rect.contains_rect(doc))
                .map(|r| r.rect)
                .min_by(|a, b| a.area().total_cmp(&b.area()))
                .expect("a pane around its document")
        });
        // Each identity row is one node named "<side> · <role>: <name>, …"
        // (`projections::identity_name`), found by its side prefix rather
        // than by position, so a gap above the pane is measured instead of
        // making the identity unfindable. egui puts a label's text in the
        // node's value, so both are read.
        let all = texts(h);
        let identities = ["A · ", "B · "].map(|prefix| {
            all.iter()
                .find(|(text, _)| text.starts_with(prefix) && text.contains(": "))
                .unwrap_or_else(|| panic!("the pinned identity {prefix}…"))
                .1
        });
        Paired { identities, panes }
    }

    fn check_compare(h: &Shell, size: Vec2) {
        let mut c = Checks::default();
        let work = work_area(h);

        // Acceptance (4): toolbar 32, evidence 28 and change 28 rows, each
        // followed by an 8-unit gap (frame `13:1304`: rows at 52 and 88,
        // panes at 124). Each row's content is centred in its row.
        let evidence_top = work.top() + 12.0 + 32.0 + 8.0;
        c.units(
            button(h, "Only in A: ").center().y - evidence_top,
            14.0,
            "evidence row centre",
        );
        let change_top = evidence_top + 28.0 + 8.0;
        let previous = button(h, "‹ Previous");
        c.units(previous.center().y - change_top, 14.0, "change row centre");
        // Frame `13:1304` draws ‹ Previous 30 high at y −1 in the 28 row.
        c.units(previous.height(), 30.0, "‹ Previous height");

        let p = paired(h);
        let region = p.region();
        c.units(region.top() - work.top(), 124.0, "paired region top");
        c.units(
            region.top() - (change_top + 28.0),
            8.0,
            "gap from the change row",
        );
        for (side, (identity, pane)) in ["A", "B"].iter().zip(p.identities.iter().zip(&p.panes)) {
            c.units(identity.height(), 54.0, &format!("identity {side} height"));
            c.units(
                pane.top(),
                identity.bottom(),
                &format!("pane {side} text area directly under its identity"),
            );
            c.units(
                work.bottom() - pane.bottom(),
                12.0,
                &format!("pane {side} foot inset"),
            );
        }
        c.units(
            work.bottom() - region.bottom(),
            12.0,
            "paired region foot inset",
        );
        if size == REFERENCE {
            c.units(region.height(), 798.0, "paired region height (§11.3)");
            for (side, pane) in ["A", "B"].iter().zip(&p.panes) {
                c.units(
                    pane.height(),
                    744.0,
                    &format!("pane {side} text area height (§11.3)"),
                );
            }
        }

        // Acceptance (5), first half: no Only-in text inside either pane;
        // each count is drawn once, in its evidence-row menu.
        let model = state(h)
            .compare
            .comparison
            .aligned()
            .expect("ready")
            .clone();
        for (b, count) in [
            (false, model.only_in_a.count()),
            (true, model.only_in_b.count()),
        ] {
            let letter = if b { "B" } else { "A" };
            let named = format!("Only in {letter}: ");
            let shown: Vec<Rect> = texts(h)
                .into_iter()
                .filter(|(text, _)| text.starts_with(&named))
                .map(|(_, rect)| rect)
                .collect();
            c.that(
                shown.len() == 1,
                &format!("Only in {letter} drawn once, found {}", shown.len()),
            );
            c.that(
                shown.iter().all(|r| r.bottom() <= region.top()),
                &format!("Only in {letter} only above the paired region: {shown:?}"),
            );
            let unit = if count == 1 { "line" } else { "lines" };
            c.that(
                texts(h).iter().any(|(text, rect)| {
                    *text == format!("Only in {letter}: {count} {unit} ▾")
                        && within(rect.center().y, evidence_top + 14.0)
                }),
                &format!("the evidence row's Only in {letter} menu names {count} {unit}"),
            );
        }
        for (text, rect) in texts(h) {
            for pane in &p.panes {
                c.that(
                    !(pane.contains_rect(rect) && text.contains("Only in")),
                    &format!("{text:?} is drawn inside a pane"),
                );
            }
        }
        c.finish(&format!("Compare at {} × {}", size.x, size.y));
    }

    #[test]
    fn workspace_rhythm_compare_stack_at_1600_by_960() {
        let mut h = launch("compare-1600", REFERENCE);
        open_compare(&mut h);
        check_compare(&h, REFERENCE);
    }

    #[test]
    fn workspace_rhythm_compare_stack_at_1024_by_600() {
        let mut h = launch("compare-1024", MINIMUM);
        open_compare(&mut h);
        check_compare(&h, MINIMUM);
    }

    // ── Normalized notices (IR-006) ──────────────────────────────────────

    /// The notice texts drawn inside each identity header, A then B, and
    /// whether any notice text is drawn inside either pane.
    fn header_notices(h: &Shell) -> ([Vec<String>; 2], bool) {
        let p = paired(h);
        let all = texts(h);
        let is_notice = |text: &str| {
            text == diff_view::NORMALIZED_NOTICE
                || text == diff_view::copied_notice("A")
                || text == diff_view::copied_notice("B")
        };
        let headers = p.identities.map(|header| {
            all.iter()
                .filter(|(text, rect)| is_notice(text) && header.contains_rect(*rect))
                .map(|(text, _)| text.clone())
                .collect()
        });
        let in_pane = all.iter().any(|(text, rect)| {
            is_notice(text) && p.panes.iter().any(|pane| pane.contains_rect(*rect))
        });
        (headers, in_pane)
    }

    #[test]
    fn workspace_rhythm_normalized_notices_sit_in_the_identity_headers() {
        let mut h = launch("notices", REFERENCE);
        open_compare(&mut h);
        let (headers, _) = header_notices(&h);
        assert_eq!(
            headers,
            [Vec::<String>::new(), Vec::new()],
            "no notice in Original mode"
        );

        h.get_by_role_and_label(Role::Button, "Normalized").click();
        h.run();
        run_until(&mut h, "the Normalized comparison", |s| {
            s.compare
                .comparison
                .aligned()
                .is_some_and(|a| a.mode == CompareMode::Normalized)
        });
        h.run();
        let notice = diff_view::NORMALIZED_NOTICE.to_owned();
        let (headers, in_pane) = header_notices(&h);
        assert_eq!(
            headers,
            [vec![notice.clone()], vec![notice.clone()]],
            "each identity header says the view copies normalized text"
        );
        assert!(!in_pane, "no notice is drawn inside a pane");

        // Select all and copy act on the active pane, A, as egui-winit
        // turns Ctrl+C into the platform's Copy event.
        h.key_press_modifiers(Modifiers::COMMAND, Key::A);
        h.step();
        h.event(Event::Copy);
        h.step();
        h.run();
        let (headers, in_pane) = header_notices(&h);
        assert_eq!(
            headers,
            [vec![diff_view::copied_notice("A")], vec![notice]],
            "A's header says its normalized text was copied"
        );
        assert!(!in_pane, "no notice is drawn inside a pane");
    }
}
