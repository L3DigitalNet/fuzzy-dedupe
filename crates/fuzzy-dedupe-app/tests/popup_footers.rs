// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! PV-T36-001 (plan T36; REQ-032): every popup's action footer stays
//! reachable (spec §11.6), driven headless through `egui_kittest`.
//!
//! Every test sits in the `popup_footers` module and is named
//! `popup_footers_…`, so `-E 'test(popup_footers)'` selects exactly these.
//!
//! The rig runs the app's frame order over T12's public seams, as
//! `tests/cleanup_dialog.rs` does: drain engine events into `events::apply`,
//! poll the services, `dialog_host::begin_frame`, draw an invoking button and
//! the dialog host, then dispatch the frame's intents and their follow-ups.
//! The shell is not drawn: a popup's geometry depends only on the viewport,
//! and the invoking button is what focus returns to.
//!
//! Measurement method (T22's integration leg, commit `b5cf8b9`): the popup
//! frame is the area of egui's top modal layer, which the dialog host's
//! frame fills exactly, border included; a footer control is read through
//! accesskit, whose bounds are the unclipped widget rect. A control scrolled
//! out of the body still reports a rect, so a rect inside the frame's
//! content inset is what makes the pointer click at its centre land on it.
//! Every comparison allows the ±1-unit layout tolerance (A-010).
//!
//! The cases use the largest body a fixture gives each popup: Review unique
//! text on the design pair with 9 unique lines (past its list's 5 visible
//! rows), Review cleanup on the stale scenario with five queued rows and a
//! no-Trash target, New scan with its matching rules expanded, and About on
//! its license text.

mod popup_footers {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    use egui::accesskit::Role;
    use egui::{Event, Key, Modifiers, Rect, Vec2};
    use egui_kittest::Harness;
    use egui_kittest::kittest::{NodeT as _, Queryable};
    use fuzzy_dedupe_app::presentation::commands::{
        self, IntentQueue, Services, UiIntent, comparison_request,
    };
    use fuzzy_dedupe_app::presentation::events;
    use fuzzy_dedupe_app::presentation::projections;
    use fuzzy_dedupe_app::presentation::state::{
        CleanupPhase, DialogKind, DialogSubject, PresentationState, ScanStage, UniqueTextSubject,
    };
    use fuzzy_dedupe_app::ui::dialogs::DialogDraft;
    use fuzzy_dedupe_app::ui::widgets::dialog_host;
    use fuzzy_dedupe_app::ui::{glyphs, theme};
    use fuzzy_dedupe_domain::{
        CompareMode, EvidenceUpdate, FileId, QueueOrigin, QueueScope, SnapshotId,
    };
    use fuzzy_dedupe_engine::cleanup_sim::{CleanupEvent, CleanupScript, ScriptedOutcome};
    use fuzzy_dedupe_engine::fixtures::{self, Scenario, ScenarioKind, SeededDecision};
    use fuzzy_dedupe_engine::service::{
        Engine, EngineConfig, EngineEvent, EngineRequest, ExecuteApprovedPlan, Origin,
        ReviewCommand, ReviewStatus,
    };
    use fuzzy_dedupe_platform::dirs::AppDirs;
    use fuzzy_dedupe_platform::open::{OpenHandle, Opener};
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

    /// Outer bound on any wait for engine events; simulated work takes
    /// microseconds.
    const TIMEOUT: Duration = Duration::from_secs(30);
    const INVOKER: &str = "Open popup";
    /// The largest window the spec draws and the smallest it supports
    /// (spec §11.3).
    const SIZES: [Vec2; 2] = [Vec2::new(1600.0, 960.0), Vec2::new(1024.0, 600.0)];
    /// The layout tolerance of A-010.
    const TOLERANCE: f32 = 1.0;

    // ── Doubles ──────────────────────────────────────────────────────────

    struct NoOpener;

    impl Opener for NoOpener {
        fn open(
            &self,
            _: fuzzy_dedupe_platform::open::Location,
            notify: Box<dyn FnOnce() + Send>,
        ) -> OpenHandle {
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

    /// Directories under the test's own target directory; nothing is
    /// written there, but `Services` needs a resolved set.
    fn dirs(name: &str) -> AppDirs {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after 1970")
            .as_nanos();
        let root: PathBuf = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("popup_footers")
            .join(format!("{name}-{nanos}"));
        AppDirs::resolve(|_| None, &root.join("home"))
    }

    // ── The rig ──────────────────────────────────────────────────────────

    struct Rig {
        state: PresentationState,
        services: Services,
        scenario: Arc<Scenario>,
        /// Frames drain engine events only while set; the running case
        /// clears it to feed a synthetic stream instead, because a
        /// simulated run ends within the frame it starts.
        logic: bool,
        /// The popup the next frame opens from the invoking button.
        open: Option<(DialogKind, DialogSubject)>,
        /// Every `Submit` dispatched, in order.
        submitted: Vec<EngineRequest>,
        /// Unset until the theme is installed: the harness draws a first
        /// frame on build, before the compact font families exist.
        ready: bool,
    }

    impl Rig {
        fn frame(&mut self, ui: &mut egui::Ui) {
            if !self.ready {
                return;
            }
            let ctx = ui.ctx().clone();
            if self.logic {
                for event in self.services.engine.drain_events(256) {
                    events::apply(&mut self.state, event);
                }
                self.services.poll(&mut self.state);
            }
            dialog_host::begin_frame(&ctx, &mut self.state.dialog);
            let mut intents = IntentQueue::default();
            let invoker = ui.button(INVOKER);
            if let Some((kind, subject)) = self.open.take() {
                intents.push(UiIntent::OpenDialog {
                    kind,
                    subject,
                    invoker: Some(invoker.id),
                });
            }
            dialog_host::show(&ctx, &mut self.state, &mut intents);
            let mut batch = intents.drain();
            for _ in 0..4 {
                if batch.is_empty() {
                    break;
                }
                let mut followups = IntentQueue::default();
                for intent in batch {
                    if let UiIntent::Submit(request) = &intent {
                        self.submitted.push(request.clone());
                    }
                    commands::dispatch(
                        intent,
                        &mut self.state,
                        &mut self.services,
                        &ctx,
                        &mut followups,
                    );
                }
                batch = followups.drain();
                ctx.request_repaint();
            }
        }

        fn review(&mut self, command: ReviewCommand) {
            let shown = format!("{command:?}");
            let outcome = self.services.engine.review(command);
            assert!(
                !matches!(outcome.status, ReviewStatus::Refused { .. }),
                "{shown} answered {:?}",
                outcome.status
            );
            events::apply_review(&mut self.state, &outcome);
            self.services.sync_view(&mut self.state);
        }

        /// Queues `file` against `counterpart`, acknowledging unique text
        /// where the policy asks, as a user who reviewed it would.
        fn queue_against(&mut self, file: FileId, counterpart: FileId) {
            let request = ReviewCommand::RequestQueue {
                file,
                counterpart,
                origin: QueueOrigin::Checkbox,
                reviewed: None,
            };
            let outcome = self.services.engine.review(request.clone());
            events::apply_review(&mut self.state, &outcome);
            if let ReviewStatus::NeedsReview { .. } = outcome.status {
                self.review(ReviewCommand::AcknowledgeUniqueText {
                    file,
                    counterpart,
                    reviewed: None,
                });
                self.review(request);
            }
            self.services.sync_view(&mut self.state);
        }

        /// Queues `file` against its rule counterpart.
        fn queue(&mut self, file: FileId) {
            let counterpart = self
                .scenario
                .results
                .group_of(file)
                .and_then(|g| g.member(file))
                .and_then(fuzzy_dedupe_domain::Member::counterpart)
                .expect("a non-reference member");
            self.queue_against(file, counterpart);
        }

        fn file_named(&self, name: &str) -> FileId {
            let found: Vec<FileId> = self
                .scenario
                .results
                .groups
                .iter()
                .flat_map(|g| &g.members)
                .filter(|m| m.name == name)
                .map(|m| m.file)
                .collect();
            assert_eq!(found.len(), 1, "exactly one {name}");
            found[0]
        }

        /// The row's counterpart as the review service recorded it.
        fn counterpart_of(&self, file: FileId) -> FileId {
            let results = &self.scenario.results;
            let group = results.group_of(file).expect("a member");
            let member = group.member(file).expect("a member");
            projections::member_view(group, &self.state.queue, member)
                .counterpart
                .expect("not the reference")
        }
    }

    type Popup = Harness<'static, Rig>;

    /// The rig on `kind` at `size`, adopted and seeded as `app.rs` does it.
    fn launch(name: &str, kind: ScenarioKind, size: Vec2) -> Popup {
        let mut engine = Engine::new(EngineConfig::default()).expect("engine starts");
        let scan = engine.reserve_scan_id();
        let scenario =
            Arc::new(fixtures::build(kind, scan, fixtures::DEFAULT_SEED).expect("fixture"));
        engine
            .adopt_scenario(Arc::clone(&scenario))
            .expect("fresh scan");
        let mut rig = Rig {
            state: PresentationState::default(),
            services: Services::new(
                engine,
                Box::new(NoOpener),
                Box::new(DismissPicker),
                dirs(name),
            ),
            scenario: Arc::clone(&scenario),
            logic: true,
            open: None,
            submitted: Vec::new(),
            ready: false,
        };
        events::install_results(
            &mut rig.state,
            Arc::clone(&scenario.results),
            ScanStage::Completed,
        );
        rig.services.sync_view(&mut rig.state);
        for seed in scenario.seeded_decisions.clone() {
            match seed {
                SeededDecision::Kept(file) => {
                    rig.review(ReviewCommand::MarkKept { file, kept: true });
                }
                SeededDecision::Protect(file) => rig.review(ReviewCommand::ProtectFile {
                    file,
                    protected: true,
                }),
                SeededDecision::MakeReference(file) => {
                    rig.review(ReviewCommand::MakeReference { file });
                }
                SeededDecision::Queue { file, counterpart } => rig.queue_against(file, counterpart),
            }
        }
        let mut harness = Harness::builder()
            .with_size(size)
            .with_max_steps(SETTLE_STEPS)
            .build_ui_state(|ui, rig: &mut Rig| rig.frame(ui), rig);
        theme::install(&harness.ctx, Vec::new());
        glyphs::install(&harness.ctx);
        harness.state_mut().ready = true;
        settle(&mut harness);
        harness
    }

    /// Enough frames for input, dispatch, the frame that draws the result,
    /// glyph registration and egui's scroll animation. Fixed steps rather
    /// than `run()`, which a running cleanup's repaints could exhaust.
    fn settle(h: &mut Popup) {
        for _ in 0..24 {
            h.step();
        }
    }

    /// Steps frames until `done` holds, failing after [`TIMEOUT`].
    #[track_caller]
    fn wait(h: &mut Popup, what: &str, done: impl Fn(&Rig) -> bool) {
        let deadline = Instant::now() + TIMEOUT;
        while !done(h.state()) {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            h.step();
            std::thread::yield_now();
        }
        settle(h);
    }

    fn open(h: &mut Popup, kind: DialogKind, subject: DialogSubject) {
        h.state_mut().open = Some((kind, subject));
        settle(h);
        assert_eq!(open_kind(h), Some(kind), "the popup opened");
    }

    fn open_kind(h: &Popup) -> Option<DialogKind> {
        h.state().state.dialog.dialog().map(|d| d.kind)
    }

    fn draft(h: &Popup) -> Option<DialogDraft> {
        h.state().state.dialog.dialog().map(|d| d.draft.clone())
    }

    fn press(h: &mut Popup, key: Key) {
        for pressed in [true, false] {
            h.input_mut().events.push(Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: Modifiers::NONE,
            });
        }
        settle(h);
    }

    /// The first button whose label starts with `prefix`, for labels that
    /// carry a count ("Queue 4 files", "Move 4 files to Trash").
    fn label_starting(h: &Popup, prefix: &str, suffix: &str) -> String {
        h.query_all_by_role(Role::Button)
            .filter_map(|n| n.accesskit_node().label())
            .find(|l| l.starts_with(prefix) && l.ends_with(suffix) && l.len() > prefix.len())
            .unwrap_or_else(|| panic!("a button {prefix}…{suffix}"))
    }

    /// The painted popup: the area of egui's top modal layer.
    fn popup_rect(h: &Popup) -> Rect {
        h.ctx.memory(|memory| {
            let layer = memory.top_modal_layer().expect("a popup is drawn");
            memory.area_rect(layer.id).expect("the popup's area")
        })
    }

    /// The host's title-row close control, which is also named "Close".
    fn title_close(h: &Popup) -> Rect {
        h.query_all_by_role_and_label(Role::Button, "Close")
            .map(|n| n.rect())
            .min_by(|a, b| a.top().total_cmp(&b.top()))
            .expect("the title-row ×")
    }

    /// The footer control named `label`. Where the title-row × shares the
    /// name (Close), the footer's is the lower of the two.
    fn footer_rect(h: &Popup, label: &str) -> Rect {
        h.query_all_by_role_and_label(Role::Button, label)
            .map(|n| n.rect())
            .max_by(|a, b| a.top().total_cmp(&b.top()))
            .unwrap_or_else(|| panic!("a footer control {label:?}"))
    }

    fn footer_rects(h: &Popup, labels: &[String]) -> Vec<Rect> {
        labels.iter().map(|l| footer_rect(h, l)).collect()
    }

    /// [`footer_rects`], or `None` once a control has been relabelled (a
    /// confirmed run turns Move N files to Trash into the next phase's).
    fn footer_rects_if_present(h: &Popup, labels: &[String]) -> Option<Vec<Rect>> {
        labels
            .iter()
            .all(|l| {
                h.query_all_by_role_and_label(Role::Button, l)
                    .next()
                    .is_some()
            })
            .then(|| footer_rects(h, labels))
    }

    fn enabled(h: &Popup, label: &str) -> bool {
        h.query_all_by_role_and_label(Role::Button, label)
            .max_by(|a, b| a.rect().top().total_cmp(&b.rect().top()))
            .is_some_and(|n| !n.accesskit_node().is_disabled())
    }

    /// Clicks the footer control `label` at its rect's centre, through the
    /// pointer, as a user would.
    fn click_footer(h: &mut Popup, label: &str) {
        let node = h
            .query_all_by_role_and_label(Role::Button, label)
            .max_by(|a, b| a.rect().top().total_cmp(&b.rect().top()))
            .unwrap_or_else(|| panic!("a footer control {label:?}"));
        node.click();
        settle(h);
    }

    // ── The cases ────────────────────────────────────────────────────────

    /// Every popup with an action footer, and Review cleanup in each phase.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Case {
        MatchingRules,
        NewScanExpanded,
        /// `Design_Spec_draft.txt`, 2 unique lines: reachable at the base.
        UniqueTextShort,
        /// `Requirements_v2.txt`, 9 unique lines: unreachable at the base.
        UniqueTextLong,
        QueueIdentical,
        CleanupReview,
        CleanupRunning,
        CleanupStopped,
        CleanupFinished,
        KeyboardShortcuts,
        AboutSummary,
        AboutText,
    }

    const CASES: [Case; 12] = [
        Case::MatchingRules,
        Case::NewScanExpanded,
        Case::UniqueTextShort,
        Case::UniqueTextLong,
        Case::QueueIdentical,
        Case::CleanupReview,
        Case::CleanupRunning,
        Case::CleanupStopped,
        Case::CleanupFinished,
        Case::KeyboardShortcuts,
        Case::AboutSummary,
        Case::AboutText,
    ];

    /// The cases whose footer T22's integration leg measured reachable at
    /// the base (acceptance (4)).
    const BASE_REACHABLE: [Case; 3] = [
        Case::UniqueTextShort,
        Case::QueueIdentical,
        Case::MatchingRules,
    ];

    /// An opened case: the harness and its footer controls' labels in the
    /// footer's own order, which is also its Tab order.
    struct Opened {
        h: Popup,
        footer: Vec<String>,
    }

    fn unique_subject(h: &Popup, name: &str) -> DialogSubject {
        let rig = h.state();
        let target = rig.file_named(name);
        let counterpart = rig.counterpart_of(target);
        let request = comparison_request(&rig.state, target, counterpart, CompareMode::Original)
            .expect("the pair is in the results");
        DialogSubject::UniqueText(UniqueTextSubject {
            scan: request.scan,
            target,
            counterpart,
            target_snapshot: request.target_snapshot,
            counterpart_snapshot: request.counterpart_snapshot,
            rules: request.rules,
            reviewed: None,
            origin: QueueOrigin::RowMenu,
        })
    }

    fn open_unique(h: &mut Popup, name: &str) {
        let subject = unique_subject(h, name);
        let DialogSubject::UniqueText(pair) = subject else {
            unreachable!("built above")
        };
        open(h, DialogKind::UniqueTextReview, subject);
        wait(h, "Review unique text's lines", move |rig| {
            let slot = &rig.state.compare.comparison;
            slot.key.is_some_and(|k| k.target == pair.target) && slot.aligned().is_some()
        });
    }

    /// The stale scenario with every scripted target queued, as
    /// `tests/cleanup_dialog.rs` builds it: five rows, one without Trash.
    /// Returns the target whose snapshot the finished case changes.
    fn stale_queued(h: &mut Popup) -> FileId {
        let script = CleanupScript::for_scenario(&h.state().scenario);
        let one = |outcome| {
            script
                .entries()
                .find(|(_, o)| *o == outcome)
                .map(|(f, _)| f)
                .expect("scripted")
        };
        let fails = one(ScriptedOutcome::MoveFails);
        let no_trash = one(ScriptedOutcome::NoTrash);
        let moved = one(ScriptedOutcome::Uncertain { moved: true });
        let rig = h.state_mut();
        rig.review(ReviewCommand::MarkKept {
            file: fails,
            kept: false,
        });
        rig.review(ReviewCommand::ProtectFile {
            file: no_trash,
            protected: false,
        });
        rig.queue(fails);
        rig.queue(no_trash);
        rig.queue(moved);
        settle(h);
        moved
    }

    fn open_cleanup(h: &mut Popup) {
        open(h, DialogKind::Cleanup, DialogSubject::None);
        wait(h, "the plan", |rig| {
            rig.state.operation.phase == CleanupPhase::Review
        });
    }

    /// Confirms the plan through the accessibility action, which does not
    /// depend on where the footer is, and waits for the run's end.
    fn run_plan(h: &mut Popup) {
        let label = label_starting(h, "Move ", " to Trash");
        h.get_by_role_and_label(Role::Button, &label)
            .click_accesskit();
        settle(h);
        wait(h, "the run's end", |rig| {
            matches!(
                rig.state.operation.phase,
                CleanupPhase::Stopped | CleanupPhase::Finished
            )
        });
    }

    fn open_case(case: Case, size: Vec2) -> Opened {
        let name = format!("{case:?}-{}x{}", size.x, size.y);
        let stale = matches!(
            case,
            Case::CleanupReview | Case::CleanupStopped | Case::CleanupFinished
        );
        let kind = if stale {
            ScenarioKind::Stale
        } else {
            ScenarioKind::Design
        };
        let mut h = launch(&name, kind, size);
        let footer: Vec<String> = match case {
            Case::MatchingRules => {
                open(&mut h, DialogKind::MatchingRules, DialogSubject::None);
                vec!["Save rules".into(), "Cancel".into()]
            }
            Case::NewScanExpanded => {
                open(&mut h, DialogKind::NewScan, DialogSubject::None);
                h.get_by_role_and_label(Role::Button, "Matching rules section")
                    .click_accesskit();
                settle(&mut h);
                vec!["Start scan".into(), "Cancel".into()]
            }
            Case::UniqueTextShort => {
                open_unique(&mut h, "Design_Spec_draft.txt");
                vec!["Cancel".into(), "Queue for cleanup".into()]
            }
            Case::UniqueTextLong => {
                open_unique(&mut h, "Requirements_v2.txt");
                vec!["Cancel".into(), "Queue for cleanup".into()]
            }
            Case::QueueIdentical => {
                open(
                    &mut h,
                    DialogKind::QueueIdentical,
                    DialogSubject::QueueIdentical(QueueScope::AllByteIdenticalGroups),
                );
                vec!["Cancel".into(), label_starting(&h, "Queue ", " files")]
            }
            Case::CleanupReview => {
                stale_queued(&mut h);
                open_cleanup(&mut h);
                vec![label_starting(&h, "Move ", " to Trash"), "Cancel".into()]
            }
            Case::CleanupRunning => {
                open_cleanup(&mut h);
                running(&mut h);
                vec!["Moving…".into(), "Stop".into()]
            }
            Case::CleanupStopped => {
                stale_queued(&mut h);
                open_cleanup(&mut h);
                run_plan(&mut h);
                assert_eq!(h.state().state.operation.phase, CleanupPhase::Stopped);
                vec![
                    label_starting(&h, "Move ", " to Trash"),
                    "Close".into(),
                    "Show journal".into(),
                ]
            }
            Case::CleanupFinished => {
                let changed = stale_queued(&mut h);
                open_cleanup(&mut h);
                // The ERR-003 schedule of `tests/cleanup_dialog.rs`: two
                // stopped runs, a changed snapshot, then the run that
                // finishes, so every outcome stays listed in the body.
                run_plan(&mut h);
                run_plan(&mut h);
                h.state_mut().review(ReviewCommand::UpdateEvidence(
                    EvidenceUpdate::FileSnapshot {
                        file: changed,
                        snapshot: SnapshotId::new(9_000_000),
                    },
                ));
                run_plan(&mut h);
                assert_eq!(h.state().state.operation.phase, CleanupPhase::Finished);
                vec!["Close".into(), "Show journal".into()]
            }
            Case::KeyboardShortcuts => {
                open(&mut h, DialogKind::KeyboardShortcuts, DialogSubject::None);
                vec!["Close".into()]
            }
            Case::AboutSummary => {
                open(&mut h, DialogKind::About, DialogSubject::None);
                vec!["Close".into()]
            }
            Case::AboutText => {
                open(&mut h, DialogKind::About, DialogSubject::None);
                h.get_by_role_and_label(Role::Button, "View license")
                    .click_accesskit();
                settle(&mut h);
                vec!["Close".into()]
            }
        };
        Opened { h, footer }
    }

    /// The running phase, from a synthetic stream as
    /// `tests/cleanup_dialog.rs` drives it: the confirmation is noted as
    /// admitted without reaching the engine, then the design sample's
    /// running statuses arrive.
    fn running(h: &mut Popup) {
        let rig = h.state_mut();
        let plan = rig.state.operation.revision.expect("a plan");
        let items = rig.state.operation.items.clone();
        rig.logic = false;
        let request = EngineRequest::ExecuteApprovedPlan(ExecuteApprovedPlan { plan });
        events::note_submitted(&mut rig.state, Origin::Plan(plan), &request);
        let sample = fixtures::design::sample();
        for (item, status) in items.iter().zip(&sample.cleanup_running) {
            let mut changed = *item;
            changed.status = *status;
            events::apply(
                &mut rig.state,
                EngineEvent::Cleanup {
                    plan,
                    event: CleanupEvent::ItemChanged(changed),
                },
            );
        }
        settle(h);
        assert_eq!(h.state().state.operation.phase, CleanupPhase::Running);
    }

    // ── Geometry ─────────────────────────────────────────────────────────

    /// Where a footer control must lie: the frame less its border and the
    /// 16-unit content inset on every side (spec §11.3).
    fn inset(frame: Rect) -> Rect {
        frame.shrink(theme::BORDER_WIDTH + dialog_host::CONTENT_INSET)
    }

    /// Where the popup draws anything visibly: horizontally its own clip,
    /// the frame less its border; vertically the content area, the frame
    /// less its border and the content inset (T22's `body_clip`).
    fn visible(frame: Rect) -> Rect {
        let inset = theme::BORDER_WIDTH + dialog_host::CONTENT_INSET;
        Rect::from_x_y_ranges(
            frame.shrink(theme::BORDER_WIDTH).x_range(),
            (frame.top() + inset)..=(frame.bottom() - inset),
        )
    }

    fn within(outer: Rect, inner: Rect) -> bool {
        outer.expand(TOLERANCE).contains_rect(inner)
    }

    /// Each footer control's rect against the frame, the content inset and
    /// the visible clip; one table row per control that fails any of them.
    fn misplaced(case: Case, size: Vec2, opened: &Opened, strict: bool) -> Vec<String> {
        let frame = popup_rect(&opened.h);
        let mut rows = Vec::new();
        for label in &opened.footer {
            let button = footer_rect(&opened.h, label);
            let reachable = within(frame, button) && within(visible(frame), button);
            let inside = within(inset(frame), button);
            if !reachable || (strict && !inside) {
                rows.push(format!(
                    "| {} × {} | {case:?} | {label} | frame {frame:?} | button {button:?} | \
                     inset {:?} | visible {:?} | reachable {reachable} | in inset {inside} |",
                    size.x,
                    size.y,
                    inset(frame),
                    visible(frame),
                ));
            }
        }
        rows
    }

    /// Clicks each enabled footer control at its centre on a fresh copy of
    /// the case and reports any that did nothing or moved the footer. A
    /// control acted when it closed the popup, changed the popup's draft
    /// (a prompt, a toggle, a single-flight mark) or sent an engine request.
    /// With `pinned`, a footer that moved on its own click is reported too:
    /// a footer drawn at the end of its body is pushed down by a prompt the
    /// click adds above it.
    fn inert(case: Case, size: Vec2, labels: &[String], pinned: bool) -> Vec<String> {
        let mut rows = Vec::new();
        for label in labels {
            let mut opened = open_case(case, size);
            let h = &mut opened.h;
            if !enabled(h, label) {
                continue;
            }
            let footer = footer_rects(h, &opened.footer);
            let before = draft(h);
            let sent = h.state().submitted.len();
            click_footer(h, label);
            let closed = open_kind(h).is_none();
            let acted = closed || draft(h) != before || h.state().submitted.len() > sent;
            if !acted {
                rows.push(format!(
                    "| {} × {} | {case:?} | {label} | the click at its centre did nothing |",
                    size.x, size.y
                ));
            } else if pinned
                && !closed
                && footer_rects_if_present(h, &opened.footer).is_some_and(|after| after != footer)
            {
                rows.push(format!(
                    "| {} × {} | {case:?} | {label} | the footer moved on its click |",
                    size.x, size.y
                ));
            }
        }
        rows
    }

    #[track_caller]
    fn assert_none(what: &str, rows: &[String]) {
        assert!(rows.is_empty(), "{what}:\n{}", rows.join("\n"));
    }

    // ── Acceptance (1): inside the frame, the inset and the visible clip ──

    #[test]
    fn popup_footers_every_footer_control_lies_inside_the_content_inset() {
        let mut rows = Vec::new();
        for size in SIZES {
            for case in CASES {
                let opened = open_case(case, size);
                rows.extend(misplaced(case, size, &opened, true));
            }
        }
        assert_none(
            "footer controls outside the popup's content inset or visible clip (spec §11.6)",
            &rows,
        );
    }

    #[test]
    fn popup_footers_a_click_at_each_footer_control_centre_activates_it() {
        let mut rows = Vec::new();
        for size in SIZES {
            for case in CASES {
                let labels = open_case(case, size).footer;
                rows.extend(inert(case, size, &labels, true));
            }
        }
        assert_none("footer controls a click does not reach", &rows);
    }

    // ── Acceptance (2): the body scrolls, the footer stays ──────────────

    #[test]
    fn popup_footers_the_body_scrolls_above_a_fixed_footer() {
        // A line near the body's top, and its last line, both in the host's
        // own scroll area rather than a nested list. Scrolled by the
        // accessibility action, as PV-T34-001 does: egui applies a wheel or
        // a ScrollDown action only to the scroll area under the pointer.
        let probes = [
            (
                Case::UniqueTextLong,
                "9 lines exist only in this file.",
                "I reviewed the unique text in this file.",
            ),
            (
                Case::CleanupReview,
                "5 files queued · 1 cannot be moved",
                "No permanent-delete fallback. Space is not reclaimed until Trash is emptied.",
            ),
        ];
        for size in SIZES {
            for (case, top_line, last_line) in probes {
                let mut opened = open_case(case, size);
                let h = &mut opened.h;
                let footer = footer_rects(h, &opened.footer);
                let top = h.get_by_label(top_line).rect().top();
                h.get_by_label(last_line).scroll_to_me();
                settle(h);
                let moved = h.get_by_label(top_line).rect().top();
                assert!(
                    moved < top - TOLERANCE,
                    "{case:?} at {size:?}: the body scrolled ({top} to {moved})"
                );
                assert_eq!(
                    footer_rects(h, &opened.footer),
                    footer,
                    "{case:?} at {size:?}: the footer did not move"
                );
                let frame = popup_rect(h);
                for label in &opened.footer {
                    assert!(
                        within(inset(frame), footer_rect(h, label)),
                        "{case:?} at {size:?}: {label} still inside the inset"
                    );
                }
            }
        }
    }

    // ── Acceptance (3): Tab reaches the body, then the footer ───────────

    /// The controls Tab reaches inside the popup, starting after the
    /// title-row ×, until focus comes back round to it. Each is its label
    /// and whether it is the footer's control of that name.
    fn tab_cycle(h: &mut Popup, footer: &[String]) -> Vec<(String, bool)> {
        let close = title_close(h);
        h.query_all_by_role_and_label(Role::Button, "Close")
            .find(|n| n.rect() == close)
            .expect("the ×")
            .focus();
        settle(h);
        let frame = popup_rect(h);
        let mut cycle = Vec::new();
        for _ in 0..80 {
            press(h, Key::Tab);
            #[expect(
                clippy::redundant_closure_for_method_calls,
                reason = "the suggested `AccessKitNode::is_focused` path is bound to one node \
                          lifetime and fails `query_by`'s higher-ranked bound"
            )]
            let Some(node) = h.query_by(|n| n.is_focused()) else {
                continue;
            };
            let rect = node.rect();
            if rect == close {
                break;
            }
            if !frame.contains_rect(rect) {
                continue;
            }
            let label = node.accesskit_node().label().unwrap_or_default();
            let in_footer = footer.contains(&label) && rect == footer_rect(h, &label);
            cycle.push((label, in_footer));
        }
        cycle
    }

    #[test]
    fn popup_footers_tab_reaches_every_body_control_before_the_footer() {
        for case in CASES {
            let mut opened = open_case(case, SIZES[1]);
            let expected: Vec<String> = opened
                .footer
                .iter()
                .filter(|l| enabled(&opened.h, l))
                .cloned()
                .collect();
            let cycle = tab_cycle(&mut opened.h, &opened.footer);
            let first_footer = cycle.iter().position(|(_, f)| *f);
            let last_body = cycle.iter().rposition(|(_, f)| !*f);
            if let (Some(first), Some(last)) = (first_footer, last_body) {
                assert!(
                    last < first,
                    "{case:?}: every body control before the footer: {cycle:?}"
                );
            }
            let reached: Vec<String> = cycle
                .iter()
                .filter(|(_, f)| *f)
                .map(|(l, _)| l.clone())
                .collect();
            assert_eq!(
                reached, expected,
                "{case:?}: the footer keeps its own control order"
            );
        }
    }

    // ── Acceptance (4): the cases reachable at the base stay so ─────────

    #[test]
    fn popup_footers_the_footers_reachable_at_the_base_stay_reachable() {
        let mut rows = Vec::new();
        for size in SIZES {
            for case in BASE_REACHABLE {
                let opened = open_case(case, size);
                rows.extend(misplaced(case, size, &opened, false));
                rows.extend(inert(case, size, &opened.footer, false));
            }
        }
        assert_none("base-reachable footers no longer reachable", &rows);
    }

    // ── Acceptance (5): Esc, Enter and focus restoration ────────────────

    #[test]
    fn popup_footers_esc_and_enter_keep_their_meaning_and_focus_returns() {
        for case in CASES {
            let mut opened = open_case(case, SIZES[1]);
            let h = &mut opened.h;
            press(h, Key::Escape);
            assert_eq!(open_kind(h), None, "{case:?}: Esc cancels");
            settle(h);
            assert!(
                h.get_by_role_and_label(Role::Button, INVOKER).is_focused(),
                "{case:?}: focus returns to the invoker"
            );
        }

        // Enter never reaches Move N files to Trash, whatever has focus.
        let mut opened = open_case(Case::CleanupReview, SIZES[1]);
        let h = &mut opened.h;
        assert!(
            h.get_by_role_and_label(Role::Button, "Cancel").is_focused(),
            "Review cleanup's initial focus is on Cancel"
        );
        let confirm = opened.footer[0].clone();
        h.get_by_role_and_label(Role::Button, &confirm).focus();
        settle(h);
        let sent = h.state().submitted.len();
        press(h, Key::Enter);
        assert_eq!(open_kind(h), None, "Enter dismisses Review cleanup");
        assert_eq!(h.state().submitted.len(), sent, "Enter moved nothing");
    }
}
