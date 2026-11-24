// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! PV-T42-001 (plan T42, items 1 to 3): the three deviations T24's round-3
//! design review found after T41.
//!
//! 1. Queued member rows are filled `code` across the whole row (frames
//!    `13:648`, `29:2214`, `37:2466`), the Queue cell keeping its redSoft.
//! 2. Matching rules' Match mode control is the 30-unit control height
//!    with its label centred (frame `16:735`).
//! 3. Review cleanup's Kept copy cell never elides the "· protected"
//!    marker at 1600 × 960 and 200%, where the cell is narrowest.
//!
//! Every test carries the `conform_` prefix, so `-E 'test(conform_)'`
//! selects them with the other legs'. The rig is `tests/conform_findings.rs`'s,
//! copied: the app starts on the design scenario through
//! `FuzzyDedupeApp::with_startup`, draws into the full content rect, and
//! takes zoom and theme through the preferences. Colours and elision come
//! from the frame's painted shapes, geometry from the accessibility tree.
//!
//! Requirements: fresh preference and log directories under
//! `CARGO_TARGET_TMPDIR`, unique per run, which the app itself may write
//! into.

mod conform_round3 {
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use egui::accesskit::{Role, Toggled};
    use egui::{Pos2, Rect, Vec2};
    use egui_kittest::Harness;
    use egui_kittest::kittest::{NodeT, Queryable};
    use fuzzy_dedupe_app::app::{AppDeps, Clock, FuzzyDedupeApp};
    use fuzzy_dedupe_app::measure::{NoMeasurement, StartupScenario};
    use fuzzy_dedupe_app::presentation::commands::UiIntent;
    use fuzzy_dedupe_app::presentation::state::{
        CleanupPhase, DialogKind, DialogSubject, PresentationState, ThemeOverride,
    };
    use fuzzy_dedupe_app::ui::glyphs;
    use fuzzy_dedupe_app::ui::theme::{self, Mode, Token, Zoom};
    use fuzzy_dedupe_app::ui::widgets::grouped_table;
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
            .join("conform_round3")
            .join(format!("{name}-{nanos}"));
        let state = root.join("state");
        AppDirs::resolve(
            move |var| (var == "XDG_STATE_HOME").then(|| state.clone().into_os_string()),
            &root.join("home"),
        )
    }

    /// The app on the design scenario, whose seeds queue two G001 members
    /// against `Project_Plan.txt` and protect the kept copy's folder.
    fn launch(name: &str, size: Vec2, zoom: u16) -> Shell {
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
        state_mut(&mut harness).preferences.zoom =
            Zoom::from_percent(zoom).expect("on the zoom grid");
        settle(&mut harness);
        harness
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

    fn color(h: &Shell, token: Token) -> egui::Color32 {
        let mode = match h.ctx.theme() {
            egui::Theme::Light => Mode::Light,
            egui::Theme::Dark => Mode::Dark,
        };
        theme::palette(mode).color(token)
    }

    /// Every painted shape, flattened, with the clip rect it was drawn in.
    fn shapes(h: &Shell) -> Vec<(Rect, egui::Shape)> {
        fn walk(clip: Rect, shape: &egui::Shape, out: &mut Vec<(Rect, egui::Shape)>) {
            match shape {
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(clip, s, out)),
                other => out.push((clip, other.clone())),
            }
        }
        let mut out = Vec::new();
        for clipped in &h.output().shapes {
            walk(clipped.clip_rect, &clipped.shape, &mut out);
        }
        out
    }

    // ── Item 1: queued rows are tinted `code` ───────────────────────────

    /// The fills of the row-wide rectangles painted under `at`, in paint
    /// order. Row-wide means at least half the table's width and no taller
    /// than a row, which leaves out the Queue cell's own redSoft fill, the
    /// table's background and the checkbox square.
    fn row_fills(h: &Shell, table: Rect, at: Pos2) -> Vec<egui::Color32> {
        shapes(h)
            .into_iter()
            .filter_map(|(_, s)| match s {
                egui::Shape::Rect(r) => Some(r),
                _ => None,
            })
            .filter(|r| {
                r.rect.contains(at)
                    && r.rect.width() >= table.width() / 2.0
                    && r.rect.height() <= 40.0
            })
            .map(|r| r.fill)
            .collect()
    }

    /// T42 item 1: in both themes, with no pointer and no row focused, every
    /// visible queued member row is filled `code` and every unqueued member
    /// row `surface`, and the queued row's Queue cell stays redSoft. A
    /// member row is told by its Queue checkbox, checked when queued; the
    /// design scenario seeds G001's two copies as queued.
    #[test]
    fn conform_round3_queued_member_rows_are_filled_code() {
        for theme in [ThemeOverride::Light, ThemeOverride::Dark] {
            let mut h = launch("queued_tint", REFERENCE, 100);
            push(&mut h, UiIntent::SetTheme(theme));
            let groups = state(&h)
                .scan
                .results
                .as_ref()
                .expect("results")
                .groups
                .iter()
                .map(|g| g.id)
                .collect();
            state_mut(&mut h).findings.expanded = groups;
            state_mut(&mut h).findings.focused = None;
            settle(&mut h);

            let id = grouped_table::table_id().accesskit_id();
            let table = h.get_by(move |node| node.locate().0 == id).rect();
            let (code, surface, red_soft) = (
                color(&h, Token::Code),
                color(&h, Token::Surface),
                color(&h, Token::RedSoft),
            );
            let boxes: Vec<(String, bool, Rect)> = h
                .query_all_by_role(Role::CheckBox)
                .filter(|n| table.contains(n.rect().center()))
                .map(|n| {
                    let node = n.accesskit_node();
                    (
                        node.label().unwrap_or_default(),
                        node.toggled() == Some(Toggled::True),
                        n.rect(),
                    )
                })
                .collect();
            assert!(
                boxes.iter().any(|b| b.1) && boxes.iter().any(|b| !b.1),
                "{theme:?}: expected queued and unqueued rows in view: {boxes:?}"
            );
            for (name, queued, rect) in &boxes {
                // A point in the Folder column's span, right of the Queue
                // cell, on the row's centre line.
                let at = Pos2::new(table.center().x, rect.center().y);
                let fills = row_fills(&h, table, at);
                let want = if *queued { code } else { surface };
                assert_eq!(
                    fills.last(),
                    Some(&want),
                    "{theme:?}: {name} (queued: {queued}) row fills {fills:?}, \
                     expected {want:?}"
                );
                if *queued {
                    let cell = shapes(&h).into_iter().any(|(_, s)| {
                        matches!(s, egui::Shape::Rect(r)
                            if r.fill == red_soft
                                && r.rect.contains(rect.center())
                                && r.rect.width() < table.width() / 2.0)
                    });
                    assert!(cell, "{theme:?}: {name}'s Queue cell is not redSoft");
                }
            }
        }
    }

    // ── Item 2: the Match mode control is 30 high ───────────────────────

    /// T42 item 2, frame `16:735`: the Match mode control is the 30-unit
    /// control height at 100%, with its selected label centred in it.
    #[test]
    fn conform_round3_match_mode_control_is_30_high() {
        let mut h = launch("match_mode", REFERENCE, 100);
        push(
            &mut h,
            UiIntent::OpenDialog {
                kind: DialogKind::MatchingRules,
                subject: DialogSubject::None,
                invoker: None,
            },
        );
        let combo = h.get_by_role_and_label(Role::ComboBox, "Match mode").rect();
        assert!(
            (combo.height() - 30.0).abs() <= TOLERANCE,
            "Match mode control {} high, expected 30 ±{TOLERANCE} (frame 16:735)",
            combo.height()
        );
        let label = shapes(&h)
            .into_iter()
            .filter_map(|(_, s)| match s {
                egui::Shape::Text(t) => Some(t),
                _ => None,
            })
            .map(|t| t.galley.rect.translate(t.pos.to_vec2()))
            .find(|r| combo.contains(r.center()) && r.width() > 20.0)
            .expect("the selected mode's label inside the control");
        assert!(
            (label.center().y - combo.center().y).abs() <= TOLERANCE,
            "label centred at {}, control centre {}",
            label.center().y,
            combo.center().y
        );
    }

    // ── Item 3: the protected marker is never elided ────────────────────

    /// The glyphs a text shape actually draws: an elided galley keeps its
    /// whole source text in `job.text`, so the rows are what shows.
    fn drawn(t: &egui::epaint::TextShape) -> String {
        t.galley
            .rows
            .iter()
            .flat_map(|row| row.row.glyphs.iter().map(|g| g.chr))
            .collect()
    }

    /// T42 item 3 (Q-R3-1): at 1600 × 960 and 200% (800 × 480 points), the
    /// Review cleanup plan's first kept copy, `Project_Plan.txt` in the
    /// protected ~/Documents/Projects/, shows its whole "· protected"
    /// marker inside the table's clip, however much of the path is cut.
    #[test]
    fn conform_round3_protected_marker_shows_whole_at_200_percent() {
        let mut h = launch("protected_marker", REFERENCE, 200);
        push(
            &mut h,
            UiIntent::OpenDialog {
                kind: DialogKind::Cleanup,
                subject: DialogSubject::None,
                invoker: None,
            },
        );
        run_until(&mut h, "the cleanup plan", |s| {
            s.operation.phase == CleanupPhase::Review
        });
        settle(&mut h);

        let row = h
            .query_all_by_role(Role::Row)
            .next()
            .expect("the plan's first row")
            .rect();
        let painted: Vec<(Rect, String, Rect)> = shapes(&h)
            .into_iter()
            .filter_map(|(clip, s)| match s {
                egui::Shape::Text(t) => {
                    Some((clip, drawn(&t), t.galley.rect.translate(t.pos.to_vec2())))
                }
                _ => None,
            })
            .filter(|(_, _, r)| row.contains(r.center()))
            .collect();
        let marker = painted
            .iter()
            .find(|(_, text, _)| text.contains("· protected"));
        let Some((clip, _, rect)) = marker else {
            panic!(
                "no whole \"· protected\" marker in the first row: {:?}",
                painted.iter().map(|p| &p.1).collect::<Vec<_>>()
            );
        };
        assert!(
            clip.expand(TOLERANCE).contains_rect(*rect),
            "the marker at {rect:?} is cut by its clip {clip:?}"
        );
    }
}
