// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! PV-T18-001 to PV-T18-003 (plan T18; REQ-024, REQ-040, REQ-041): the
//! Review cleanup dialog over the simulated cleanup, driven headless through
//! `egui_kittest`.
//!
//! Every test sits in the `cleanup_dialog` module so `-E
//! 'test(cleanup_dialog)'` selects them all.
//!
//! The rig runs the app's frame order over T12's public seams, not
//! `FuzzyDedupeApp`, because the app always adopts the design scenario and
//! the ERR-003 schedule needs the stale one: drain engine events into
//! `events::apply`, poll the services, `dialog_host::begin_frame`, draw an
//! invoking button and the dialog host, then dispatch the frame's intents
//! and their follow-ups through `commands::dispatch`. Engine work goes
//! through the real `Engine` facade on fixture scenarios. Only the running
//! state uses a synthetic `CleanupEvent` stream, because a simulated run
//! ends within the frame it starts and its mid-run states cannot be caught
//! deterministically.
//!
//! The rig records every dispatched `Submit`, so "submits once" is counted
//! at the dispatch boundary rather than inferred from the engine's refusal
//! of a second execution.

mod cleanup_dialog {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    use egui::accesskit::Role;
    use egui::{Event, Key, Modifiers, Vec2};
    use egui_kittest::Harness;
    use egui_kittest::kittest::{NodeT as _, Queryable};
    use fuzzy_dedupe_app::presentation::commands::{self, IntentQueue, Services, UiIntent};
    use fuzzy_dedupe_app::presentation::events;
    use fuzzy_dedupe_app::presentation::state::{
        CleanupPhase, DialogKind, DialogSubject, PresentationState, ScanStage,
    };
    use fuzzy_dedupe_app::ui::theme;
    use fuzzy_dedupe_app::ui::widgets::dialog_host;
    use fuzzy_dedupe_domain::{
        EvidenceUpdate, FileId, PlanItemStatus, PlanRevision, QueueOrigin, SnapshotId,
    };
    use fuzzy_dedupe_engine::cleanup_sim::{
        CleanupEvent, CleanupScript, DialogStatus, RunReport, ScriptedOutcome,
    };
    use fuzzy_dedupe_engine::fixtures::{self, Scenario, ScenarioKind, SeededDecision};
    use fuzzy_dedupe_engine::service::{
        Engine, EngineConfig, EngineEvent, EngineRequest, ExecuteApprovedPlan, Origin,
        ReviewCommand, ReviewStatus,
    };
    use fuzzy_dedupe_platform::dirs::AppDirs;
    use fuzzy_dedupe_platform::open::{OpenHandle, Opener};
    use fuzzy_dedupe_platform::picker::{FolderPicker, PickHandle, PickOutcome, PickRequest};

    /// Outer bound on any wait for engine events; simulated runs take
    /// microseconds.
    const TIMEOUT: Duration = Duration::from_secs(30);
    const INVOKER: &str = "Open cleanup";

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
            .join("cleanup_dialog")
            .join(format!("{name}-{nanos}"));
        AppDirs::resolve(|_| None, &root.join("home"))
    }

    // ── The rig ──────────────────────────────────────────────────────────

    struct Rig {
        state: PresentationState,
        services: Services,
        scenario: Arc<Scenario>,
        /// Frames drain engine events only while set; the running-state test
        /// clears it to feed a synthetic stream instead.
        logic: bool,
        /// Every `Submit` dispatched, in order.
        submitted: Vec<EngineRequest>,
        invoker: Option<egui::Id>,
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
            self.invoker = Some(invoker.id);
            if invoker.clicked() {
                intents.push(UiIntent::OpenDialog {
                    kind: DialogKind::Cleanup,
                    subject: DialogSubject::None,
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

        /// Queues `file` against its rule counterpart, acknowledging unique
        /// text where the policy asks, as a user would.
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

        /// Queues `file` against `counterpart`. The seeds name their own
        /// counterpart: after a seeded `MakeReference` the stored results'
        /// rule counterpart is no longer the current one.
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

        fn name(&self, file: FileId) -> String {
            self.member(file).name.clone()
        }

        fn path(&self, file: FileId) -> String {
            self.member(file).display_path.clone()
        }

        fn member(&self, file: FileId) -> &fuzzy_dedupe_domain::Member {
            self.scenario
                .results
                .group_of(file)
                .and_then(|g| g.member(file))
                .expect("in the results")
        }

        fn executions(&self) -> Vec<PlanRevision> {
            self.submitted
                .iter()
                .filter_map(|r| match r {
                    EngineRequest::ExecuteApprovedPlan(ExecuteApprovedPlan { plan }) => Some(*plan),
                    _ => None,
                })
                .collect()
        }
    }

    type Dialog = Harness<'static, Rig>;

    /// The rig on `kind`, adopted and seeded as `app.rs` does it.
    fn launch(name: &str, kind: ScenarioKind) -> Dialog {
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
            submitted: Vec::new(),
            invoker: None,
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
            .with_size(Vec2::new(1600.0, 960.0))
            .build_ui_state(|ui, rig: &mut Rig| rig.frame(ui), rig);
        theme::install(&harness.ctx, Vec::new());
        harness.state_mut().ready = true;
        harness.run();
        harness
    }

    /// Steps frames until `done` holds, failing after [`TIMEOUT`].
    fn wait(h: &mut Dialog, what: &str, done: impl Fn(&Rig) -> bool) {
        let deadline = Instant::now() + TIMEOUT;
        while !done(h.state()) {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            h.step();
            std::thread::yield_now();
        }
        // Let the frame that first sees the new state draw it, and settle
        // any focus request it made.
        h.run();
    }

    fn phase(h: &Dialog) -> CleanupPhase {
        h.state().state.operation.phase
    }

    fn open_cleanup(h: &mut Dialog) {
        h.get_by_role_and_label(Role::Button, INVOKER).click();
        h.run();
        wait(h, "the plan", |rig| {
            rig.state.operation.phase == CleanupPhase::Review
        });
    }

    fn dialog_open(h: &Dialog) -> bool {
        h.state().state.dialog.dialog().is_some()
    }

    fn has_label(h: &Dialog, text: &str) -> bool {
        h.query_all_by_label(text).next().is_some()
    }

    fn assert_label(h: &Dialog, text: &str) {
        if !has_label(h, text) {
            let labels: Vec<String> = h
                .query_all_by(|_| true)
                .filter_map(|node| {
                    let node = node.accesskit_node();
                    node.label().or_else(|| node.value())
                })
                .collect();
            panic!("no node labelled {text:?}; labels: {labels:#?}");
        }
    }

    fn count_label(h: &Dialog, text: &str) -> usize {
        h.query_all_by_label(text).count()
    }

    fn button_enabled(h: &Dialog, label: &str) -> bool {
        !h.get_by_role_and_label(Role::Button, label)
            .accesskit_node()
            .is_disabled()
    }

    /// FR-021, WH-007: no control in any state offers permanent deletion.
    /// The destination note "No permanent-delete fallback" is text, not a
    /// control, so only buttons and other actionable roles are checked.
    fn assert_no_permanent_delete(h: &Dialog) {
        let offending: Vec<String> = h
            .query_all_by(|node| {
                matches!(
                    node.role(),
                    Role::Button | Role::Link | Role::MenuItem | Role::CheckBox
                )
            })
            .filter_map(|node| node.accesskit_node().label())
            .filter(|label| {
                let lower = label.to_lowercase();
                lower.contains("delete") || lower.contains("permanent") || lower.contains("empty")
            })
            .collect();
        assert!(offending.is_empty(), "deletion controls: {offending:?}");
    }

    /// Presses a footer button through its accessibility action. The stale
    /// plan's dialog is taller than the 423-unit popup, so its footer sits
    /// in the host's scroll area; a pointer click depends on the scroll
    /// offset, which egui animates over wall-clock frames, and was flaky
    /// under load. The accessibility action reaches the same `clicked()`.
    fn click_in_view(h: &mut Dialog, label: &str) {
        h.get_by_role_and_label(Role::Button, label)
            .click_accesskit();
        h.run();
    }

    fn press(h: &mut Dialog, key: Key) {
        for pressed in [true, false] {
            h.input_mut().events.push(Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: Modifiers::NONE,
            });
        }
        h.run();
    }

    // ── PV-T18-001 and PV-T18-002: review ───────────────────────────────

    #[test]
    fn cleanup_dialog_review_lists_targets_kept_copies_and_destination() {
        let mut h = launch("review", ScenarioKind::Design);
        open_cleanup(&mut h);
        let sample = fixtures::design::sample();
        let rig = h.state();
        let names: Vec<(String, String)> = sample
            .cleanup_items
            .iter()
            .map(|i| (rig.name(i.target), rig.name(i.counterpart)))
            .collect();

        assert_label(&h, "Review cleanup");
        assert_label(&h, "3 files queued · all byte-identical · 2 kept copies");
        for (target, kept) in &names {
            assert_label(&h, target);
            assert!(has_label(&h, kept), "kept copy {kept} listed");
        }
        assert_eq!(count_label(&h, "Ready"), 3, "every target is Ready");
        assert_eq!(count_label(&h, "~/Documents/Projects/ · protected"), 2);
        assert_label(&h, "Destination: system Trash · 48.0 KiB logical size");
        assert_label(
            &h,
            "No permanent-delete fallback. Space is not reclaimed until Trash is emptied.",
        );
        assert_label(
            &h,
            "If a file or its kept copy changed since the scan, cleanup stops before it and asks \
             you to confirm the rest.",
        );
        assert_label(&h, "Nothing has been removed.");
        assert!(button_enabled(&h, "Move 3 files to Trash"));
        assert!(
            h.get_by_role_and_label(Role::Button, "Cancel").is_focused(),
            "Cancel has the initial focus"
        );
        assert_no_permanent_delete(&h);
        assert!(h.state().executions().is_empty(), "nothing ran on open");
    }

    #[test]
    fn cleanup_dialog_enter_never_confirms_and_cancel_returns_focus() {
        let mut h = launch("enter", ScenarioKind::Design);
        open_cleanup(&mut h);
        press(&mut h, Key::Enter);
        assert!(!dialog_open(&h), "Enter dismisses the popup");
        h.run();
        assert!(h.state().executions().is_empty(), "Enter moved nothing");
        assert!(
            h.get_by_role_and_label(Role::Button, INVOKER).is_focused(),
            "focus returns to the invoking control"
        );

        open_cleanup(&mut h);
        // With the confirm button focused, Enter still only dismisses.
        h.get_by_role_and_label(Role::Button, "Move 3 files to Trash")
            .focus();
        h.run();
        press(&mut h, Key::Enter);
        assert!(!dialog_open(&h));
        assert!(h.state().executions().is_empty(), "Enter moved nothing");

        open_cleanup(&mut h);
        h.get_by_role_and_label(Role::Button, "Cancel").click();
        h.run();
        h.run();
        assert!(!dialog_open(&h));
        assert!(h.state().executions().is_empty());
        assert!(h.get_by_role_and_label(Role::Button, INVOKER).is_focused());
    }

    #[test]
    fn cleanup_dialog_double_click_submits_once_and_finishes() {
        let mut h = launch("double", ScenarioKind::Design);
        open_cleanup(&mut h);
        let plan = h.state().state.operation.revision.expect("a plan");
        let confirm = h.get_by_role_and_label(Role::Button, "Move 3 files to Trash");
        confirm.click();
        confirm.click();
        h.step();
        // A third press on the next frame lands on the disabled button.
        if let Some(again) = h.query_by_role_and_label(Role::Button, "Move 3 files to Trash") {
            assert!(
                again.accesskit_node().is_disabled(),
                "disabled from submission"
            );
            again.click();
        }
        h.step();
        wait(&mut h, "the run's end", |rig| {
            rig.state.operation.phase == CleanupPhase::Finished
        });
        assert_eq!(h.state().executions(), [plan], "one submission");

        assert_label(&h, "Cleanup finished");
        assert_label(&h, "3 moved · 0 failed");
        assert_eq!(count_label(&h, "Moved"), 3);
        assert_label(&h, "All 3 files were moved to the system Trash.");
        assert_label(&h, "Moved files can be restored from the system Trash.");
        assert_label(&h, "3 files moved to Trash.");
        assert!(button_enabled(&h, "Show journal"));
        // The host's close control is also labelled "Close".
        assert_eq!(
            h.query_all_by_role_and_label(Role::Button, "Close").count(),
            2,
            "the footer's Close beside the title row's"
        );
        assert!(
            h.query_by_role_and_label(Role::Button, "Move 3 files to Trash")
                .is_none(),
            "nothing left to confirm"
        );
        assert_no_permanent_delete(&h);
    }

    // ── PV-T18-001: running, from a synthetic stream ────────────────────

    #[test]
    fn cleanup_dialog_running_state_shows_progress_and_stop() {
        let mut h = launch("running", ScenarioKind::Design);
        open_cleanup(&mut h);
        let plan = h.state().state.operation.revision.expect("a plan");
        let items = h.state().state.operation.items.clone();
        // From here the test owns the event stream: the confirmation is
        // noted as admitted without reaching the engine.
        let rig = h.state_mut();
        rig.logic = false;
        let request = EngineRequest::ExecuteApprovedPlan(ExecuteApprovedPlan { plan });
        events::note_submitted(&mut rig.state, Origin::Plan(plan), &request);
        let sample = fixtures::design::sample();
        for (item, status) in items.iter().zip(&sample.cleanup_running) {
            let mut changed = *item;
            changed.status = *status;
            let event = EngineEvent::Cleanup {
                plan,
                event: CleanupEvent::ItemChanged(changed),
            };
            events::apply(&mut rig.state, event);
        }
        h.run();

        assert_label(&h, "Moving files to Trash");
        assert_label(&h, "1 of 3 files moved");
        assert_label(&h, "Moved");
        assert_eq!(
            count_label(&h, "Moving…"),
            2,
            "the row and the disabled button"
        );
        assert_label(&h, "Waiting");
        assert_label(&h, "1 file moved so far.");
        assert_label(
            &h,
            "Each file and its kept copy are checked again just before the move.",
        );
        assert_label(&h, "Stopping leaves the remaining files in place.");
        assert!(
            !button_enabled(&h, "Moving…"),
            "repeated submission disabled"
        );
        assert_no_permanent_delete(&h);

        h.get_by_role_and_label(Role::Button, "Stop").click();
        h.run();
        assert!(
            h.state()
                .submitted
                .contains(&EngineRequest::StopCleanup(plan)),
            "Stop sends StopCleanup"
        );
        assert_eq!(phase(&h), CleanupPhase::Stopping);
        assert_label(&h, "Moving files to Trash");
        assert!(!button_enabled(&h, "Stop"), "one stop");

        // The run ends with a Trash failure on its last item: the failure
        // counts as failed, never as moved.
        let mut report = items.clone();
        report[0].status = PlanItemStatus::Succeeded;
        report[1].status = PlanItemStatus::Succeeded;
        report[2].status = PlanItemStatus::Failed;
        report[2].reason = Some(fuzzy_dedupe_domain::PlanItemReason::TrashMoveFailed);
        let ended = EngineEvent::Cleanup {
            plan,
            event: CleanupEvent::RunEnded {
                revised: None,
                report: Arc::new(RunReport {
                    plan,
                    items: report,
                    journal: Vec::new(),
                }),
            },
        };
        events::apply(&mut h.state_mut().state, ended);
        h.run();
        assert_label(&h, "Cleanup finished");
        assert_label(&h, "2 moved · 1 failed");
        assert_label(&h, "Failed");
        assert_label(&h, "2 of 3 files were moved to the system Trash.");
        assert_no_permanent_delete(&h);
    }

    // ── PV-T18-002: blocked before confirmation ─────────────────────────

    /// The stale scenario with every scripted target queued: in plan order
    /// the seeded queued file (moves), the failing file, the file without
    /// Trash, then the two uncertain copies (not moved, then moved).
    fn stale_queued(name: &str) -> (Dialog, StaleFiles) {
        let mut h = launch(name, ScenarioKind::Stale);
        let script = CleanupScript::for_scenario(&h.state().scenario);
        let one = |outcome| {
            let files: Vec<FileId> = script
                .entries()
                .filter(|(_, o)| *o == outcome)
                .map(|(f, _)| f)
                .collect();
            assert_eq!(files.len(), 1, "{outcome:?} scripted once");
            files[0]
        };
        let files = StaleFiles {
            fails: one(ScriptedOutcome::MoveFails),
            no_trash: one(ScriptedOutcome::NoTrash),
            not_moved: one(ScriptedOutcome::Uncertain { moved: false }),
            moved: one(ScriptedOutcome::Uncertain { moved: true }),
        };
        let rig = h.state_mut();
        rig.review(ReviewCommand::MarkKept {
            file: files.fails,
            kept: false,
        });
        rig.review(ReviewCommand::ProtectFile {
            file: files.no_trash,
            protected: false,
        });
        rig.queue(files.fails);
        rig.queue(files.no_trash);
        rig.queue(files.moved);
        h.run();
        (h, files)
    }

    struct StaleFiles {
        fails: FileId,
        no_trash: FileId,
        not_moved: FileId,
        moved: FileId,
    }

    #[test]
    fn cleanup_dialog_blocked_target_is_shown_and_excluded_before_confirmation() {
        let (mut h, files) = stale_queued("blocked");
        open_cleanup(&mut h);
        let op = &h.state().state.operation;
        assert_eq!(op.items.len(), 5, "five targets queued");
        let blocked = op
            .items
            .iter()
            .find(|i| i.target == files.no_trash)
            .expect("in the plan");
        assert_eq!(blocked.status, PlanItemStatus::Blocked);

        assert_label(&h, "Blocked: no Trash");
        assert_eq!(count_label(&h, "Ready"), 4);
        assert_label(&h, "5 files queued · 1 cannot be moved");
        // Excluded from the count the confirmation names.
        assert!(button_enabled(&h, "Move 4 files to Trash"));
        assert!(
            h.query_by_role_and_label(Role::Button, "Move 5 files to Trash")
                .is_none()
        );
        let folder = format!("{}/", h.state().member(files.no_trash).folder);
        assert_label(
            &h,
            &format!("{folder} is on a volume without a Trash folder."),
        );
        assert_label(
            &h,
            "Fuzzy DeDupe never deletes permanently. This file will be left in place.",
        );
        assert_label(&h, "Nothing has been removed.");
        assert!(h.get_by_role_and_label(Role::Button, "Cancel").is_focused());
        assert_no_permanent_delete(&h);
    }

    // ── PV-T18-003: the ERR-003 schedule ────────────────────────────────

    fn confirm(h: &mut Dialog, label: &str) {
        let before = h.state().executions().len();
        assert!(button_enabled(h, label), "{label} is offered");
        click_in_view(h, label);
        assert_eq!(
            h.state().executions().len(),
            before + 1,
            "{label} submitted once"
        );
        wait(h, "the run's end", |rig| {
            matches!(
                rig.state.operation.phase,
                CleanupPhase::Stopped | CleanupPhase::Finished
            )
        });
    }

    /// Each file's display path, captured up front so the assertions can
    /// name files while the harness is borrowed mutably.
    fn paths(h: &Dialog, files: &[FileId]) -> Box<dyn Fn(FileId) -> String> {
        let paths: Vec<(FileId, String)> = files.iter().map(|f| (*f, h.state().path(*f))).collect();
        Box::new(move |file| {
            paths
                .iter()
                .find(|(f, _)| *f == file)
                .map(|(_, p)| p.clone())
                .expect("a planned file")
        })
    }

    #[test]
    fn cleanup_dialog_err003_schedule_keeps_every_outcome_visible() {
        let (mut h, files) = stale_queued("err003");
        open_cleanup(&mut h);
        let order: Vec<FileId> = h
            .state()
            .state
            .operation
            .items
            .iter()
            .map(|i| i.target)
            .collect();
        let first = order[0];
        assert_eq!(
            order,
            [
                first,
                files.fails,
                files.no_trash,
                files.not_moved,
                files.moved
            ],
            "fixture shape"
        );

        // Run 1: the first file moves, then the Trash move fails (ERR-003).
        confirm(&mut h, "Move 4 files to Trash");
        assert_eq!(phase(&h), CleanupPhase::Stopped);
        assert_label(&h, "Cleanup stopped");
        assert_label(&h, "1 moved · 1 failed · 2 left to confirm · 1 blocked");
        assert_label(&h, "Failed");
        assert_label(&h, "Moved");
        assert_label(&h, "Blocked: no Trash");
        let path = paths(&h, &order);
        assert_label(
            &h,
            &format!(
                "{}: Failed, the move to Trash failed. It was taken out of the plan.",
                path(files.fails)
            ),
        );
        let revised = &h.state().state.operation;
        assert!(!revised.authorized, "the revised plan is unconfirmed");
        assert!(
            revised.items.iter().all(|i| i.target != files.fails),
            "the failed file left the plan"
        );
        // The revised plan waits for its own confirmation: frames pass and
        // nothing runs.
        let executions = h.state().executions().len();
        for _ in 0..5 {
            h.step();
        }
        assert_eq!(h.state().executions().len(), executions);
        assert_eq!(phase(&h), CleanupPhase::Stopped);
        assert_no_permanent_delete(&h);

        // Run 2: the first uncertain copy is reconciled as not moved; the
        // run stops and returns it to the plan.
        confirm(&mut h, "Move 2 files to Trash");
        assert_eq!(phase(&h), CleanupPhase::Stopped);
        assert_label(
            &h,
            &format!(
                "{}: Needs check, the outcome of the move was uncertain; it was found not moved \
                 and returned to the plan.",
                path(files.not_moved)
            ),
        );
        assert_label(&h, "Failed");

        // The second uncertain copy changes before its turn.
        h.state_mut().review(ReviewCommand::UpdateEvidence(
            EvidenceUpdate::FileSnapshot {
                file: files.moved,
                snapshot: SnapshotId::new(9_000_000),
            },
        ));

        // Run 3, the retry: the reconciled copy moves, the changed one stops
        // the run before its move (ERR-001).
        confirm(&mut h, "Move 2 files to Trash");
        assert_eq!(
            phase(&h),
            CleanupPhase::Finished,
            "only the blocked file is left"
        );
        assert_label(&h, "Cleanup finished");
        assert_label(&h, "2 moved · 1 failed");
        assert_label(&h, "Stopped: changed");
        assert_label(&h, "Failed");
        assert_label(&h, "Blocked: no Trash");
        assert_eq!(count_label(&h, "Moved"), 2);
        assert_label(
            &h,
            &format!(
                "{}: Changed since review, the file changed since review. It was taken out of \
                 the plan.",
                path(files.moved)
            ),
        );
        assert_label(
            &h,
            &format!(
                "{}: Failed, the move to Trash failed. It was taken out of the plan.",
                path(files.fails)
            ),
        );

        let executions = h.state().executions();
        assert_eq!(executions.len(), 3, "three confirmations, three runs");

        show_journal_keeps_every_outcome(&mut h, &path, first, &files);
    }

    /// Show journal after the ERR-003 schedule: every outcome of every run
    /// is listed, and no run after the first reached the file it moved.
    fn show_journal_keeps_every_outcome(
        h: &mut Dialog,
        path: &dyn Fn(FileId) -> String,
        first: FileId,
        files: &StaleFiles,
    ) {
        // Show journal: every outcome stays visible.
        click_in_view(h, "Show journal");
        let journal = |file: FileId, status: PlanItemStatus, reason: Option<&str>| {
            let mut line = format!("{} — {}", path(file), DialogStatus::of(status).label());
            if let Some(reason) = reason {
                line.push_str(" — ");
                line.push_str(reason);
            }
            line
        };
        assert_label(h, "Run 1");
        assert_label(h, "Run 3");
        assert_eq!(
            count_label(h, &journal(first, PlanItemStatus::Succeeded, None)),
            1,
            "the first file moved once and was never replayed"
        );
        // No retry re-executed a moved item: the first file was confirmed
        // ("Waiting") in run 1 only, so no later run reached it.
        assert_eq!(
            count_label(h, &journal(first, PlanItemStatus::Approved, None)),
            1,
            "attempted in one run only"
        );
        assert_label(
            h,
            &journal(
                files.fails,
                PlanItemStatus::Failed,
                Some("the move to Trash failed"),
            ),
        );
        assert_label(
            h,
            &journal(
                files.moved,
                PlanItemStatus::Stopped,
                Some("the file changed since review"),
            ),
        );
        assert_label(
            h,
            &journal(
                files.not_moved,
                PlanItemStatus::NeedsReconciliation,
                Some("the outcome of the move was uncertain"),
            ),
        );
        assert_eq!(
            count_label(
                h,
                &journal(files.not_moved, PlanItemStatus::Succeeded, None)
            ),
            1,
            "the reconciled copy moved in the retry"
        );
        assert!(button_enabled(h, "Hide journal"));
        assert_no_permanent_delete(h);
    }

    // ── The title hook ───────────────────────────────────────────────────

    #[test]
    fn cleanup_dialog_title_hook_keeps_other_popup_titles() {
        let mut h = launch("titles", ScenarioKind::Design);
        for kind in [
            DialogKind::MatchingRules,
            DialogKind::NewScan,
            DialogKind::KeyboardShortcuts,
            DialogKind::About,
        ] {
            let rig = h.state_mut();
            let mut intents = IntentQueue::default();
            dialog_host::open(
                &mut rig.state,
                kind,
                DialogSubject::None,
                None,
                &mut intents,
            )
            .expect("opens");
            h.run();
            assert!(
                has_label(&h, kind.title()),
                "{kind:?} shows {:?}",
                kind.title()
            );
            dialog_host::close(&mut h.state_mut().state.dialog);
            h.run();
        }
    }
}
