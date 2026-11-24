// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! PV-T43-001 (plan T43): the Findings table's Unique cells take the
//! caution colour when they show a line count.
//!
//! Frames `13:648`, `29:2214`, `37:2466` and `44:3954` draw a Unique cell
//! with a count ("4 lines") in `amber` and a cell with none ("—") in
//! `muted`. §11.7 gives `amber` the recoverable-caution role, mirroring the
//! owner's gate that Review unique text applies only when Unique > 0.
//!
//! The test carries the `conform_` prefix, so `-E 'test(conform_)'` selects
//! it with the other legs'. The rig is `tests/conform_round3.rs`'s, copied:
//! the app starts on the design scenario through
//! `FuzzyDedupeApp::with_startup`, draws into the full content rect, and
//! takes the theme through the preferences. Colours come from the frame's
//! painted text shapes; the Unique column's span from its painted header.
//!
//! Requirements: fresh preference and log directories under
//! `CARGO_TARGET_TMPDIR`, unique per run, which the app itself may write
//! into.

mod conform_unique {
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use egui::{Rect, Vec2};
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use fuzzy_dedupe_app::app::{AppDeps, Clock, FuzzyDedupeApp};
    use fuzzy_dedupe_app::measure::{NoMeasurement, StartupScenario};
    use fuzzy_dedupe_app::presentation::commands::UiIntent;
    use fuzzy_dedupe_app::presentation::projections;
    use fuzzy_dedupe_app::presentation::state::{ColumnId, PresentationState, ThemeOverride};
    use fuzzy_dedupe_app::ui::glyphs;
    use fuzzy_dedupe_app::ui::theme::{self, Mode, Token};
    use fuzzy_dedupe_app::ui::widgets::grouped_table;
    use fuzzy_dedupe_domain::RelationKind;
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

    const REFERENCE: Vec2 = Vec2::new(1600.0, 960.0);

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
            .join("conform_unique")
            .join(format!("{name}-{nanos}"));
        let state = root.join("state");
        AppDirs::resolve(
            move |var| (var == "XDG_STATE_HOME").then(|| state.clone().into_os_string()),
            &root.join("home"),
        )
    }

    /// The app on the design scenario at 100%.
    fn launch(name: &str) -> Shell {
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
            .with_size(REFERENCE)
            .build_ui_state(
                |ui, slot: &mut Slot| {
                    if let Some(app) = slot.app.as_mut() {
                        app.run_logic(ui.ctx());
                        // kittest's `build_ui_state` hands over a Ui inside
                        // an 8-unit outer margin; eframe gives `App::ui` the
                        // whole client area, so the app draws into a child
                        // spanning the content rect, as `tests/layout.rs`
                        // does.
                        let full = ui.ctx().content_rect();
                        let mut window = ui.new_child(egui::UiBuilder::new().max_rect(full));
                        app.draw(&mut window);
                    }
                },
                Slot { app: None },
            );
        let app = FuzzyDedupeApp::with_startup(
            &harness.ctx,
            deps,
            StartupScenario::Fixture(ScenarioKind::Design),
        )
        .expect("engine starts");
        glyphs::install(&harness.ctx);
        harness.state_mut().app = Some(app);
        harness.run();
        run_until(&mut harness, "the scenario's results", |s| {
            s.scan.results.is_some()
        });
        settle(&mut harness);
        harness
    }

    fn state(h: &Shell) -> &PresentationState {
        &h.state().app.as_ref().expect("app").state
    }

    fn state_mut(h: &mut Shell) -> &mut PresentationState {
        &mut h.state_mut().app.as_mut().expect("app").state
    }

    /// Enough frames for a preference change (theme) to reach egui and the
    /// layout to settle on it.
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

    fn color(h: &Shell, token: Token) -> egui::Color32 {
        let mode = match h.ctx.theme() {
            egui::Theme::Light => Mode::Light,
            egui::Theme::Dark => Mode::Dark,
        };
        theme::palette(mode).color(token)
    }

    /// Every painted text shape, flattened out of nested shape vectors.
    fn texts(h: &Shell) -> Vec<egui::epaint::TextShape> {
        fn walk(shape: &egui::Shape, out: &mut Vec<egui::epaint::TextShape>) {
            match shape {
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, out)),
                egui::Shape::Text(t) => out.push(t.clone()),
                _ => {}
            }
        }
        let mut out = Vec::new();
        for clipped in &h.output().shapes {
            walk(&clipped.shape, &mut out);
        }
        out
    }

    fn text_rect(t: &egui::epaint::TextShape) -> Rect {
        t.galley.rect.translate(t.pos.to_vec2())
    }

    /// The colour a cell's glyphs are painted in: a `RichText` colour lands
    /// on the job's section format, the fallback covering an uncoloured job.
    fn text_color(t: &egui::epaint::TextShape) -> egui::Color32 {
        t.galley
            .job
            .sections
            .first()
            .map_or(t.fallback_color, |s| s.format.color)
    }

    /// Whether `text` is `unique_label`'s count form ("1 line", "1,204
    /// lines"), as opposed to its "—" for no count.
    fn is_count(text: &str) -> bool {
        let Some((n, unit)) = text.split_once(' ') else {
            return false;
        };
        matches!(unit, "line" | "lines")
            && !n.is_empty()
            && n.chars().all(|c| c.is_ascii_digit() || c == ',')
    }

    /// T43: in both themes, on the design scenario with its similar-text
    /// groups expanded and no pointer, each Unique cell showing a line count paints
    /// its glyphs `amber` and each "—" Unique cell paints `muted`.
    ///
    /// The Unique column is bounded by its painted header and the next
    /// header to its right, because "—" also fills the Protected column; a
    /// cell is in the column when its text's left edge falls in that span.
    #[test]
    fn conform_unique_counts_paint_amber_and_dashes_muted() {
        for theme in [ThemeOverride::Light, ThemeOverride::Dark] {
            let mut h = launch("unique_amber");
            push(&mut h, UiIntent::SetTheme(theme));
            // The similar-text groups hold the scenario's counted rows, and
            // their reference rows the "—". Sorted by match type they sit
            // below the fold behind the exact-duplicate groups, so the Match
            // filter brings them to the top.
            state_mut(&mut h).findings.match_filter = Some(RelationKind::SimilarText);
            let similar = projections::kind_label(RelationKind::SimilarText);
            let groups = {
                let s = state(&h);
                s.scan
                    .results
                    .as_ref()
                    .expect("results")
                    .groups
                    .iter()
                    .filter(|g| projections::header_match_label(g, &s.queue) == similar)
                    .map(|g| g.id)
                    .collect()
            };
            state_mut(&mut h).findings.expanded = groups;
            state_mut(&mut h).findings.focused = None;
            settle(&mut h);

            let id = grouped_table::table_id().accesskit_id();
            let table = h.get_by(move |node| node.locate().0 == id).rect();
            let painted = texts(&h);
            // Header texts: the first row band of the table.
            let header_band = table.top() + 31.0;
            let headers: Vec<Rect> = painted
                .iter()
                .filter(|t| table.contains(text_rect(t).center()))
                .filter(|t| text_rect(t).top() < header_band)
                .map(text_rect)
                .collect();
            let title = grouped_table::column_title(ColumnId::Unique);
            let header = painted
                .iter()
                .find(|t| t.galley.text() == title && text_rect(t).top() < header_band)
                .map_or_else(|| panic!("{theme:?}: no painted {title} header"), text_rect);
            let next = headers
                .iter()
                .map(Rect::left)
                .filter(|&x| x > header.right())
                .fold(f32::INFINITY, f32::min);
            // A cell's text starts at its column's padding, as the header's
            // does; the slack covers a cell padded differently from its
            // header by a few units.
            let span = (header.left() - 6.0)..(next - 6.0);

            let (amber, muted) = (color(&h, Token::Amber), color(&h, Token::Muted));
            let cells: Vec<(String, egui::Color32)> = painted
                .iter()
                .filter(|t| {
                    let r = text_rect(t);
                    r.top() >= header_band && table.contains(r.center()) && span.contains(&r.left())
                })
                .map(|t| (t.galley.text().to_owned(), text_color(t)))
                .collect();
            let counts: Vec<_> = cells.iter().filter(|c| is_count(&c.0)).collect();
            let dashes: Vec<_> = cells.iter().filter(|c| c.0 == "—").collect();
            assert!(
                !counts.is_empty() && !dashes.is_empty(),
                "{theme:?}: expected counted and \"—\" Unique cells in view: {cells:?}"
            );
            for (text, painted) in &counts {
                assert_eq!(
                    *painted, amber,
                    "{theme:?}: Unique cell {text:?} painted {painted:?}, expected amber \
                     {amber:?}"
                );
            }
            for (text, painted) in &dashes {
                assert_eq!(
                    *painted, muted,
                    "{theme:?}: Unique cell {text:?} painted {painted:?}, expected muted \
                     {muted:?}"
                );
            }
        }
    }
}
