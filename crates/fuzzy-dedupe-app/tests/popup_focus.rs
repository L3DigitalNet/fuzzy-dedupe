// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! PV-T38-001 (plan T38; REQ-032, NFR-005): a popup body control that has
//! keyboard focus lies inside the body's visible clip (spec §11.6), driven
//! headless through `egui_kittest`.
//!
//! Every test sits in the `popup_focus` module and is named `popup_focus_…`,
//! so `-E 'test(popup_focus)'` selects exactly these.
//!
//! The rig is PV-T36-001's (`tests/popup_footers.rs`), copied because test
//! files share no module: T12's public seams in the app's frame order, an
//! invoking button, and the cases with the largest body a fixture gives each
//! popup. Its measurement method carries over: the popup frame is egui's top
//! modal layer, a control's rect is its unclipped accesskit bounds, and
//! every comparison allows the ±1-unit tolerance of A-010.
//!
//! The body's visible clip is the host's scroll area: from the title row
//! down to the pinned footer slot less its gap (`dialog_host::FOOTER_HEIGHT`,
//! `dialog_host::FOOTER_GAP`). The defect T22 measured (commit `f3aceb1`)
//! was Review unique text's acknowledgment checkbox focused at y 649 to 669
//! of a frame ending at 691, under the footer band, where neither the focus
//! ring nor a click could reach it.
//!
//! "Inside the clip" cannot hold for a control taller than the clip, About's
//! license text; for it the test asks that the clip show it as far as room
//! allows (`seen`).

mod popup_focus {
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
    use fuzzy_dedupe_app::ui::dialogs::{DialogDraft, unique_text_review};
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
            .join("popup_focus")
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
        press_with(h, key, Modifiers::NONE);
    }

    fn press_with(h: &mut Popup, key: Key, modifiers: Modifiers) {
        for pressed in [true, false] {
            h.input_mut().events.push(Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers,
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

    /// The body's visible clip: below the title row, above the pinned
    /// footer slot and its gap, across the popup's own clip. A body control
    /// outside it is drawn but clipped away, under the footer band or the
    /// title row.
    fn body_clip(h: &Popup) -> Rect {
        let frame = popup_rect(h);
        let bottom = inset(frame).bottom() - dialog_host::FOOTER_HEIGHT - dialog_host::FOOTER_GAP;
        Rect::from_x_y_ranges(
            frame.shrink(theme::BORDER_WIDTH).x_range(),
            title_close(h).bottom()..=bottom,
        )
    }

    fn within(outer: Rect, inner: Rect) -> bool {
        outer.expand(TOLERANCE).contains_rect(inner)
    }

    /// Whether a focused control is in view: entirely inside `clip`, or,
    /// for one taller than `clip` can ever hold (About's license text, one
    /// read-only field some 25 000 units tall in its own scroll area),
    /// shown from where it enters the clip down to the clip's bottom edge,
    /// as much of it as the clip has room for.
    fn seen(clip: Rect, rect: Rect) -> bool {
        if rect.height() <= clip.height() {
            return within(clip, rect);
        }
        let shown = clip.intersect(rect);
        shown.is_positive() && shown.bottom() >= clip.bottom() - TOLERANCE
    }

    #[track_caller]
    fn assert_none(what: &str, rows: &[String]) {
        assert!(rows.is_empty(), "{what}:\n{}", rows.join("\n"));
    }

    /// The control with keyboard focus: its label and unclipped rect.
    fn focused(h: &Popup) -> Option<(String, Rect)> {
        #[expect(
            clippy::redundant_closure_for_method_calls,
            reason = "the suggested `AccessKitNode::is_focused` path is bound to one node \
                      lifetime and fails `query_by`'s higher-ranked bound"
        )]
        h.query_by(|n| n.is_focused())
            .map(|n| (n.accesskit_node().label().unwrap_or_default(), n.rect()))
    }

    fn acknowledged(h: &Popup) -> bool {
        matches!(draft(h), Some(DialogDraft::UniqueTextReview(d)) if d.acknowledged)
    }

    fn unique_target(h: &Popup) -> FileId {
        match h.state().state.dialog.dialog().map(|d| &d.subject) {
            Some(DialogSubject::UniqueText(pair)) => pair.target,
            other => panic!("Review unique text is open, not {other:?}"),
        }
    }

    fn prompt_shown(h: &Popup) -> bool {
        h.query_by_label(unique_text_review::PROMPT).is_some()
    }

    /// The acknowledgment checkbox after Queue for cleanup without it: one
    /// table row when it lacks focus or lies outside the body's clip.
    fn checkbox_misplaced(case: Case, size: Vec2, via: &str, h: &Popup) -> Option<String> {
        let node = h.get_by_role_and_label(Role::CheckBox, unique_text_review::ACKNOWLEDGMENT);
        let rect = node.rect();
        let clip = body_clip(h);
        let focus = node.is_focused();
        (!focus || !within(clip, rect)).then(|| {
            format!(
                "| {} × {} | {case:?} | {via} | checkbox y {} to {} | clip y {} to {} | \
                 focused {focus} |",
                size.x,
                size.y,
                rect.top(),
                rect.bottom(),
                clip.top(),
                clip.bottom(),
            )
        })
    }

    const UNIQUE_CASES: [Case; 2] = [Case::UniqueTextLong, Case::UniqueTextShort];

    // ── Acceptance (1): the checkbox the popup focuses is visible ───────

    #[test]
    fn popup_focus_queue_without_the_acknowledgment_reveals_the_focused_checkbox() {
        let mut rows = Vec::new();
        for size in SIZES {
            for case in UNIQUE_CASES {
                let mut opened = open_case(case, size);
                let h = &mut opened.h;
                let target = unique_target(h);
                click_footer(h, "Queue for cleanup");
                assert!(prompt_shown(h), "{case:?} at {size:?}: the prompt shows");
                assert_eq!(open_kind(h), Some(DialogKind::UniqueTextReview));
                if let Some(row) = checkbox_misplaced(case, size, "click", h) {
                    rows.push(row);
                    continue;
                }
                h.get_by_role_and_label(Role::CheckBox, unique_text_review::ACKNOWLEDGMENT)
                    .click();
                settle(h);
                assert!(
                    acknowledged(h),
                    "{case:?} at {size:?}: a click at the checkbox centre ticks it"
                );
                click_footer(h, "Queue for cleanup");
                assert_eq!(open_kind(h), None, "{case:?} at {size:?}: the popup closed");
                assert!(!prompt_shown(h), "{case:?} at {size:?}: no prompt remains");
                // The review service's answer is authoritative (spec
                // §11.6), and the fixture's seeded decisions may make it
                // refuse the pair; a queued file or a refusal naming the
                // target both prove the request reached it.
                let queue = &h.state().state.queue;
                assert!(
                    queue.is_queued(target)
                        || queue.last_refusal.is_some_and(|(file, _)| file == target),
                    "{case:?} at {size:?}: the queue request reached the review service"
                );
            }
        }
        assert_none(
            "the focused acknowledgment checkbox outside the body's visible clip (NFR-005)",
            &rows,
        );
    }

    /// Enter runs the same Queue for cleanup before the popup draws, so its
    /// focus move waits a frame (the draft's `focus_checkbox`); it must
    /// reveal the checkbox as the click does.
    #[test]
    fn popup_focus_enter_without_the_acknowledgment_reveals_the_focused_checkbox() {
        let mut rows = Vec::new();
        for size in SIZES {
            for case in UNIQUE_CASES {
                let mut opened = open_case(case, size);
                let h = &mut opened.h;
                press(h, Key::Enter);
                assert!(prompt_shown(h), "{case:?} at {size:?}: the prompt shows");
                rows.extend(checkbox_misplaced(case, size, "Enter", h));
            }
        }
        assert_none(
            "the focused acknowledgment checkbox outside the body's visible clip (NFR-005)",
            &rows,
        );
    }

    /// Revealing focus happens once, when focus arrives, and never fights
    /// the user: with the checkbox focused, scrolling the body back to its
    /// top leaves it there. A rule that re-scrolled every frame would snap
    /// the body back under the user's wheel.
    #[test]
    fn popup_focus_scrolling_away_from_the_focused_control_is_not_undone() {
        for size in SIZES {
            let mut opened = open_case(Case::UniqueTextLong, size);
            let h = &mut opened.h;
            click_footer(h, "Queue for cleanup");
            let top_line = "9 lines exist only in this file.";
            h.get_by_label(top_line).scroll_to_me();
            settle(h);
            let line = h.get_by_label(top_line).rect();
            assert!(
                within(body_clip(h), line),
                "{size:?}: the body stays where the user scrolled it ({line:?})"
            );
            assert!(
                h.get_by_role_and_label(Role::CheckBox, unique_text_review::ACKNOWLEDGMENT)
                    .is_focused(),
                "{size:?}: the checkbox keeps focus"
            );
            // Queue for cleanup again moves focus there again, so it is
            // revealed again, though the checkbox never lost focus.
            press(h, Key::Enter);
            let rows: Vec<String> =
                checkbox_misplaced(Case::UniqueTextLong, size, "Enter again", h)
                    .into_iter()
                    .collect();
            assert_none(
                "the checkbox focused again is outside the body's clip",
                &rows,
            );
        }
    }

    // ── Acceptance (2): Tab and Shift+Tab keep every control visible ────

    /// Walks focus from the title-row × round to it again with Tab, or
    /// Shift+Tab when `back`, and reports every focused control outside
    /// where it must be seen: a body control inside the body's clip, a
    /// footer control inside the visible content area. The footer controls
    /// are checked at every step too, as PV-T36-001 requires them.
    fn walk(case: Case, size: Vec2, opened: &mut Opened, back: bool) -> Vec<String> {
        let direction = if back { "Shift+Tab" } else { "Tab" };
        let modifiers = if back {
            Modifiers::SHIFT
        } else {
            Modifiers::NONE
        };
        let h = &mut opened.h;
        let close = title_close(h);
        h.query_all_by_role_and_label(Role::Button, "Close")
            .find(|n| n.rect() == close)
            .expect("the ×")
            .focus();
        settle(h);
        let mut rows = Vec::new();
        for _ in 0..80 {
            press_with(h, Key::Tab, modifiers);
            let Some((label, rect)) = focused(h) else {
                rows.push(format!(
                    "| {} × {} | {case:?} | {direction} | focus left the popup |",
                    size.x, size.y
                ));
                break;
            };
            if rect == close {
                return rows;
            }
            let frame = popup_rect(h);
            let in_footer = opened.footer.contains(&label) && rect == footer_rect(h, &label);
            let bound = if in_footer {
                visible(frame)
            } else {
                body_clip(h)
            };
            if !seen(bound, rect) {
                rows.push(format!(
                    "| {} × {} | {case:?} | {direction} | {label:?} focused at y {} to {} | \
                     must lie in y {} to {} |",
                    size.x,
                    size.y,
                    rect.top(),
                    rect.bottom(),
                    bound.top(),
                    bound.bottom(),
                ));
            }
            for footer in &opened.footer {
                let button = footer_rect(h, footer);
                if !within(visible(frame), button) {
                    rows.push(format!(
                        "| {} × {} | {case:?} | {direction} at {label:?} | footer {footer} at \
                         {button:?} left the visible area |",
                        size.x, size.y
                    ));
                }
            }
        }
        rows.push(format!(
            "| {} × {} | {case:?} | {direction} | focus never came back to the × |",
            size.x, size.y
        ));
        rows
    }

    #[test]
    fn popup_focus_tab_and_shift_tab_keep_every_focused_control_visible() {
        let mut rows = Vec::new();
        for size in SIZES {
            for case in CASES {
                let mut opened = open_case(case, size);
                rows.extend(walk(case, size, &mut opened, false));
                rows.extend(walk(case, size, &mut opened, true));
            }
        }
        assert_none(
            "focused popup controls outside where they can be seen (NFR-005)",
            &rows,
        );
    }

    // ── Acceptance (3): Esc, Enter, initial focus and focus restoration ─

    #[test]
    fn popup_focus_esc_enter_and_initial_focus_are_unchanged() {
        for size in SIZES {
            for case in CASES {
                let mut opened = open_case(case, size);
                let h = &mut opened.h;
                press(h, Key::Escape);
                assert_eq!(open_kind(h), None, "{case:?} at {size:?}: Esc cancels");
                settle(h);
                assert!(
                    h.get_by_role_and_label(Role::Button, INVOKER).is_focused(),
                    "{case:?} at {size:?}: focus returns to the invoker"
                );
            }

            let opened = open_case(Case::CleanupReview, size);
            assert!(
                opened
                    .h
                    .get_by_role_and_label(Role::Button, "Cancel")
                    .is_focused(),
                "{size:?}: Review cleanup's initial focus is on Cancel"
            );

            // Enter is Review unique text's Queue for cleanup: unacknowledged,
            // it keeps the popup open and sends nothing.
            let mut opened = open_case(Case::UniqueTextLong, size);
            let h = &mut opened.h;
            let target = unique_target(h);
            press(h, Key::Enter);
            assert_eq!(open_kind(h), Some(DialogKind::UniqueTextReview));
            assert!(!h.state().state.queue.is_queued(target), "nothing queued");
        }
    }
}
