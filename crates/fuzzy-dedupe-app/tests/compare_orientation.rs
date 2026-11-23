// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! PV-T35-001 (plan T35), app half: Compare's side A is the counterpart and
//! side B the target, and every app reader of the sides follows.
//!
//! Every test sits in the `compare_orientation` module so
//! `-E 'test(compare_orientation)'` selects them. They run the whole app on
//! the design scenario headless through `egui_kittest`, as
//! `tests/review_popups.rs` does: `run_logic` then `draw` per frame, with
//! test doubles for every platform adapter.
//!
//! `ui/compare.rs` is still T15's stub, so the Compare workspace draws no
//! panes. The diff widget is therefore drawn alone, from the Compare state
//! the app built, exactly as T15 will hand it over.
//!
//! The design pair is frame `13:1304`'s: `Design_Spec.txt` (the reference,
//! "A · Reference") on the left and `Design_Spec_draft.txt` (the target,
//! "B · Review first") on the right. Its two unique counts differ, so every
//! assertion here fails when the sides are swapped. Expected counts come
//! from the scan results' edge, never from the comparison under test.

mod compare_orientation {
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use egui::accesskit::Role;
    use egui::{OutputCommand, Vec2};
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use fuzzy_dedupe_app::app::{AppDeps, Clock, FuzzyDedupeApp};
    use fuzzy_dedupe_app::measure::NoMeasurement;
    use fuzzy_dedupe_app::presentation::commands::UiIntent;
    use fuzzy_dedupe_app::presentation::state::{
        CompareState, DialogKind, DialogSubject, PresentationState,
    };
    use fuzzy_dedupe_app::ui::theme;
    use fuzzy_dedupe_app::ui::widgets::diff_view;
    use fuzzy_dedupe_domain::{CompareMode, FileId, QueueOrigin, ScanResults};
    use fuzzy_dedupe_engine::compare::AlignedComparison;
    use fuzzy_dedupe_engine::fixtures::design::{self, ComparedPair};
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

    /// A fresh home under the test's own target directory, unique per run
    /// because tests may not delete what an earlier run left behind.
    fn dirs_for(name: &str) -> AppDirs {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after 1970")
            .as_nanos();
        let root: PathBuf = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("compare_orientation")
            .join(format!("{name}-{nanos}"));
        let state = root.join("state");
        AppDirs::resolve(
            move |var| (var == "XDG_STATE_HOME").then(|| state.clone().into_os_string()),
            &root.join("home"),
        )
    }

    // ── The whole app on the design scenario ─────────────────────────────

    type Shell = Harness<'static, Option<FuzzyDedupeApp>>;

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
            .with_size(Vec2::new(1600.0, 960.0))
            .build_ui_state(
                |ui, app: &mut Option<FuzzyDedupeApp>| {
                    if let Some(app) = app.as_mut() {
                        app.run_logic(ui.ctx());
                        app.draw(ui);
                    }
                },
                None,
            );
        let app = FuzzyDedupeApp::new(&harness.ctx, deps).expect("engine starts");
        *harness.state_mut() = Some(app);
        harness.run();
        harness
    }

    fn state(h: &Shell) -> &PresentationState {
        &h.state().as_ref().expect("app").state
    }

    fn push(h: &mut Shell, intent: UiIntent) {
        h.state_mut().as_mut().expect("app").push_intent(intent);
    }

    /// Runs frames until `done` holds, for engine work on worker threads.
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

    fn aligned(h: &Shell) -> AlignedComparison {
        state(h)
            .compare
            .comparison
            .aligned()
            .expect("comparison ready")
            .clone()
    }

    fn results(h: &Shell) -> std::sync::Arc<ScanResults> {
        state(h).scan.results.clone().expect("results")
    }

    fn name_of(results: &ScanResults, file: FileId) -> String {
        results
            .group_of(file)
            .and_then(|g| g.member(file))
            .expect("member")
            .name
            .clone()
    }

    /// The design pair and each side's §10.1 Unique count against the
    /// other, from the scan results' edge: `(pair, unique of A, unique of B)`.
    fn design_pair(h: &Shell) -> (ComparedPair, u64, u64) {
        let pair = design::sample().last_compared.expect("design pair");
        let results = results(h);
        assert_eq!(name_of(&results, pair.a), "Design_Spec.txt");
        assert_eq!(name_of(&results, pair.b), "Design_Spec_draft.txt");
        let edge = results
            .group_of(pair.a)
            .and_then(|g| g.edge_between(pair.a, pair.b))
            .copied()
            .expect("the design pair has a direct edge");
        let of = |file| edge.unique_lines_of(file).expect("an end of the edge");
        let (a, b) = (of(pair.a), of(pair.b));
        assert_ne!(a, b, "the design pair's unique counts differ");
        (pair, a, b)
    }

    // ── Compare and the diff widget ──────────────────────────────────────

    /// The diff widget alone, given Compare's state, with the shipped fonts
    /// and styles. The theme is installed after the harness's first frame,
    /// because the shipped text styles do not exist before
    /// `theme::install`; until then the closure draws nothing.
    fn widget(compare: CompareState) -> Harness<'static, (CompareState, bool)> {
        let mut harness = Harness::builder()
            .with_max_steps(SETTLE_STEPS)
            .with_size(Vec2::new(1600.0, 800.0))
            .build_ui_state(
                |ui, (compare, themed): &mut (CompareState, bool)| {
                    if *themed {
                        diff_view::show(ui, compare, true);
                    }
                },
                (compare, false),
            );
        theme::install(&harness.ctx, Vec::new());
        harness.state_mut().1 = true;
        harness.run();
        harness
    }

    #[test]
    fn compare_orientation_app_design_pair_draws_the_reference_as_a_on_the_left() {
        let mut h = launch("design-pair");
        let (pair, unique_a, unique_b) = design_pair(&h);
        // Compare is opened on the target against its counterpart, as
        // Findings and Review unique text open it.
        push(
            &mut h,
            UiIntent::OpenCompare {
                target: pair.b,
                counterpart: pair.a,
                return_row: None,
            },
        );
        run_until(&mut h, "Compare's comparison", |s| {
            s.compare.comparison.aligned().is_some()
        });
        let model = aligned(&h);
        let results = results(&h);
        assert_eq!(
            (
                name_of(&results, model.a.file),
                name_of(&results, model.b.file)
            ),
            (
                "Design_Spec.txt".to_owned(),
                "Design_Spec_draft.txt".to_owned()
            ),
            "side A is the reference, side B the draft"
        );

        let view = widget(state(&h).compare.clone());
        let caption_a = diff_view::only_in_caption("A", unique_a, false);
        let caption_b = diff_view::only_in_caption("B", unique_b, false);
        let left = view.get_by_label(&caption_a).rect();
        let right = view.get_by_label(&caption_b).rect();
        assert!(
            left.right() <= right.left(),
            "the A caption ({left:?}) is left of the B caption ({right:?})"
        );
    }

    // ── Review unique text ───────────────────────────────────────────────

    /// Opens Review unique text from a Space request on the design draft
    /// against its counterpart, the reference, and waits for its comparison.
    fn open_review(name: &str) -> (Shell, u64) {
        let mut h = launch(name);
        let (pair, _, unique_b) = design_pair(&h);
        push(
            &mut h,
            UiIntent::Review {
                command: ReviewCommand::RequestQueue {
                    file: pair.b,
                    counterpart: pair.a,
                    origin: QueueOrigin::Shortcut,
                    reviewed: None,
                },
                invoker: None,
            },
        );
        h.run();
        let dialog = state(&h).dialog.dialog().expect("a popup is open");
        assert_eq!(dialog.kind, DialogKind::UniqueTextReview);
        let DialogSubject::UniqueText(subject) = dialog.subject else {
            panic!("Review unique text about {:?}", dialog.subject);
        };
        assert_eq!((subject.target, subject.counterpart), (pair.b, pair.a));
        let key = subject.comparison(CompareMode::Original);
        run_until(&mut h, "the popup's comparison", move |s| {
            let slot = &s.compare.comparison;
            slot.matches(&key) && slot.aligned().is_some()
        });
        (h, unique_b)
    }

    #[test]
    fn compare_orientation_review_unique_text_lists_side_b() {
        let (h, findings_unique) = open_review("review-lines");
        let model = aligned(&h);
        let count = model.only_in_b.count();
        let headline = if count == 1 {
            "1 line exists only in this file.".to_owned()
        } else {
            format!("{count} lines exist only in this file.")
        };
        h.get_by_label(&headline);
        for &line in &model.only_in_b.lines {
            let text = model.b.original.line(line).expect("line");
            h.get_by_label(&format!("Line {}: {text}", line + 1));
        }
        assert_eq!(count, findings_unique, "one measure, C-04");
    }

    #[test]
    fn compare_orientation_review_unique_text_copies_side_b() {
        let (mut h, _) = open_review("review-copy");
        let model = aligned(&h);
        let expected = model.copy_only_in_b().text;
        assert_ne!(expected, model.copy_only_in_a().text);
        h.get_by_role_and_label(Role::Button, "Copy unique lines")
            .click();
        h.step();
        let copied: Vec<&String> = h
            .output()
            .platform_output
            .commands
            .iter()
            .filter_map(|c| match c {
                OutputCommand::CopyText(text) => Some(text),
                _ => None,
            })
            .collect();
        assert_eq!(copied, vec![&expected]);
    }
}
