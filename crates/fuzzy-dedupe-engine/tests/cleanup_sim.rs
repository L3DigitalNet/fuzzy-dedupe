// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! PV-T8-001 (plan T8, REQ-025): the simulated cleanup run behind
//! `BuildCleanupPlan`, `ExecuteApprovedPlan` and `StopCleanup` (C-06).
//!
//! Every test sits in the `cleanup_sim` module so the proof filter
//! `-E 'test(cleanup_sim)'` selects them all; nextest's `test()` predicate
//! matches the test path, not the binary name.
//!
//! Everything goes through the public facade as the app drives it: a
//! scenario is adopted, its seeded decisions are replayed through
//! `Engine::review`, and every review change a test makes between build and
//! execution reaches the run only through the validation board. Tests
//! synchronise on the event stream alone, never on a sleep. Targets with a
//! scripted outcome are chosen through `CleanupScript::for_scenario`, the
//! same pure function the service reads, so no test hard-codes a file ID.

mod cleanup_sim {
    use std::collections::{BTreeMap, BTreeSet};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use fuzzy_dedupe_domain::{
        CleanupPlan, EvidenceUpdate, FileId, PlanItem, PlanItemReason, PlanItemStatus,
        PlanRevision, QueueOrigin, ScanId, SnapshotId,
    };
    use fuzzy_dedupe_engine::cleanup_sim::{
        CleanupEvent, CleanupScript, DialogStatus, RunReport, ScriptedOutcome, TRANSITIONS,
        check_transition, confirmed_count,
    };
    use fuzzy_dedupe_engine::fixtures::{
        self, DEFAULT_SEED, Scenario, ScenarioKind, SeededDecision,
    };
    use fuzzy_dedupe_engine::service::{
        BuildCleanupPlan, Engine, EngineConfig, EngineEvent, EngineRequest, ExecuteApprovedPlan,
        Origin, ReviewCommand, ReviewStatus,
    };

    /// Outer bound on any wait, so a hung run fails instead of hanging the
    /// suite. Runs are simulated and finish in microseconds.
    const TIMEOUT: Duration = Duration::from_secs(30);

    use PlanItemStatus::{
        Approved, Blocked, Executing, Failed, NeedsReconciliation, Planned, Stopped, Succeeded,
        Validating,
    };

    const ALL_STATUSES: [PlanItemStatus; 9] = [
        Planned,
        Blocked,
        Approved,
        Validating,
        Executing,
        Succeeded,
        Failed,
        Stopped,
        NeedsReconciliation,
    ];

    /// The §10.4 Dialog status column, transcribed from the spec table as an
    /// oracle independent of `DialogStatus::label`.
    fn spec_label(status: PlanItemStatus) -> &'static str {
        match status {
            Planned => "Ready",
            Blocked => "Blocked: no Trash",
            Approved => "Waiting",
            Validating | Executing => "Moving…",
            Succeeded => "Moved",
            Failed => "Failed",
            Stopped => "Stopped: changed",
            NeedsReconciliation => "Needs check",
        }
    }

    /// One run as the app sees it: every item change, then the end.
    #[derive(Debug)]
    struct Run {
        changes: Vec<PlanItem>,
        revised: Option<Arc<CleanupPlan>>,
        report: Arc<RunReport>,
        refused: usize,
    }

    impl Run {
        fn statuses_of(&self, target: FileId) -> Vec<PlanItemStatus> {
            self.changes
                .iter()
                .filter(|c| c.target == target)
                .map(|c| c.status)
                .collect()
        }

        fn last(&self, target: FileId) -> Option<PlanItem> {
            self.changes
                .iter()
                .rev()
                .find(|c| c.target == target)
                .copied()
        }

        /// Whether the run attempted anything on `target`.
        fn touched(&self, target: FileId) -> bool {
            self.changes
                .iter()
                .any(|c| c.target == target && c.status == Validating)
        }
    }

    struct Fixture {
        engine: Engine,
        scan: ScanId,
        scenario: Arc<Scenario>,
        script: CleanupScript,
    }

    fn adopt(kind: ScenarioKind) -> Fixture {
        adopt_with(kind, EngineConfig::default())
    }

    fn adopt_with(kind: ScenarioKind, config: EngineConfig) -> Fixture {
        let mut engine = Engine::new(config).expect("engine starts");
        let scan = engine.reserve_scan_id();
        let scenario = Arc::new(fixtures::build(kind, scan, DEFAULT_SEED).expect("fixture builds"));
        engine
            .adopt_scenario(Arc::clone(&scenario))
            .expect("fresh scan ID");
        let script = CleanupScript::for_scenario(&scenario);
        let mut fixture = Fixture {
            engine,
            scan,
            scenario,
            script,
        };
        fixture.replay_seeds();
        fixture
    }

    impl Fixture {
        fn replay_seeds(&mut self) {
            for seed in self.scenario.seeded_decisions.clone() {
                match seed {
                    SeededDecision::Kept(file) => {
                        self.review(ReviewCommand::MarkKept { file, kept: true });
                    }
                    SeededDecision::Protect(file) => {
                        self.review(ReviewCommand::ProtectFile {
                            file,
                            protected: true,
                        });
                    }
                    SeededDecision::Queue { file, counterpart } => {
                        self.queue_against(file, counterpart);
                    }
                    SeededDecision::MakeReference(file) => {
                        self.review(ReviewCommand::MakeReference { file });
                    }
                }
            }
        }

        fn review(&mut self, command: ReviewCommand) {
            let shown = format!("{command:?}");
            let outcome = self.engine.review(command);
            assert!(
                !matches!(
                    outcome.status,
                    ReviewStatus::Refused { .. } | ReviewStatus::NotImplemented(_)
                ),
                "{shown} answered {:?}",
                outcome.status
            );
        }

        /// Queues through the checkbox route, acknowledging unique text
        /// where the policy asks for it, as a user would.
        fn queue_against(&mut self, file: FileId, counterpart: FileId) {
            let request = ReviewCommand::RequestQueue {
                file,
                counterpart,
                origin: QueueOrigin::Checkbox,
                reviewed: None,
            };
            match self.engine.review(request.clone()).status {
                ReviewStatus::Queued { .. } => {}
                ReviewStatus::NeedsReview { .. } => {
                    self.review(ReviewCommand::AcknowledgeUniqueText {
                        file,
                        counterpart,
                        reviewed: None,
                    });
                    let again = self.engine.review(request).status;
                    assert!(
                        matches!(again, ReviewStatus::Queued { .. }),
                        "queue {file:?} after acknowledgment answered {again:?}"
                    );
                }
                other => panic!("queue {file:?} against {counterpart:?} answered {other:?}"),
            }
        }

        fn queue(&mut self, file: FileId) {
            let counterpart = self
                .scenario
                .results
                .group_of(file)
                .and_then(|g| g.member(file))
                .and_then(fuzzy_dedupe_domain::Member::counterpart)
                .expect("a non-reference member has a counterpart");
            self.queue_against(file, counterpart);
        }

        /// The one file the script gives `outcome`.
        fn scripted(&self, outcome: ScriptedOutcome) -> FileId {
            let files: Vec<FileId> = self
                .script
                .entries()
                .filter(|(_, o)| *o == outcome)
                .map(|(f, _)| f)
                .collect();
            assert_eq!(files.len(), 1, "stale scripts {outcome:?} exactly once");
            files[0]
        }

        fn submit_build(&mut self) -> PlanRevision {
            match self
                .engine
                .submit(EngineRequest::BuildCleanupPlan(BuildCleanupPlan {
                    scan: self.scan,
                })) {
                Ok(Origin::Plan(plan)) => plan,
                other => panic!("BuildCleanupPlan admitted as {other:?}"),
            }
        }

        fn submit_execute(&mut self, plan: PlanRevision) {
            assert_eq!(
                self.engine
                    .submit(EngineRequest::ExecuteApprovedPlan(ExecuteApprovedPlan {
                        plan
                    })),
                Ok(Origin::Plan(plan))
            );
        }

        fn build(&mut self) -> Arc<CleanupPlan> {
            let plan = self.submit_build();
            let mut built = None;
            self.drain_until(plan, |event| {
                if let CleanupEvent::PlanBuilt(p) = event {
                    built = Some(Arc::clone(p));
                    true
                } else {
                    false
                }
            });
            let built = built.expect("PlanBuilt");
            assert_eq!(built.revision, plan);
            assert_eq!(built.scan, self.scan);
            built
        }

        /// Executes `plan` and collects its events until its run ends.
        fn execute(&mut self, plan: PlanRevision) -> Run {
            self.submit_execute(plan);
            self.collect_run(plan)
        }

        fn collect_run(&mut self, plan: PlanRevision) -> Run {
            let mut changes = Vec::new();
            let mut ended = None;
            let mut refused = 0;
            self.drain_until(plan, |event| match event {
                CleanupEvent::ItemChanged(item) => {
                    changes.push(*item);
                    false
                }
                CleanupEvent::RunEnded { revised, report } => {
                    ended = Some((revised.clone(), Arc::clone(report)));
                    true
                }
                CleanupEvent::ExecutionRefused => {
                    refused += 1;
                    false
                }
                CleanupEvent::PlanBuilt(_) => false,
            });
            let (revised, report) = ended.expect("RunEnded");
            Run {
                changes,
                revised,
                report,
                refused,
            }
        }

        /// Drains until `accept` takes one of `plan`'s cleanup events. A
        /// not-implemented notice for the plan fails at once: it is the
        /// stub's answer and the readable RED reason.
        fn drain_until(
            &mut self,
            plan: PlanRevision,
            mut accept: impl FnMut(&CleanupEvent) -> bool,
        ) {
            let deadline = Instant::now() + TIMEOUT;
            loop {
                for event in self.engine.drain_events(1) {
                    match event {
                        EngineEvent::NotImplemented {
                            origin: Origin::Plan(p),
                            missing,
                        } if p == plan => {
                            panic!("plan {p:?} answered not implemented: {missing:?}")
                        }
                        EngineEvent::Cleanup { plan: p, event } if p == plan && accept(&event) => {
                            return;
                        }
                        _ => {}
                    }
                }
                assert!(
                    Instant::now() < deadline,
                    "no accepted event for {plan:?} within {TIMEOUT:?}"
                );
                std::thread::yield_now();
            }
        }

        /// Drains until one `ExecutionRefused` for `plan` arrives.
        fn expect_refused(&mut self, plan: PlanRevision) {
            self.drain_until(plan, |event| {
                assert!(
                    !matches!(event, CleanupEvent::ItemChanged(_)),
                    "a refused execution changed an item: {event:?}"
                );
                matches!(event, CleanupEvent::ExecutionRefused)
            });
        }
    }

    fn targets(plan: &CleanupPlan) -> Vec<FileId> {
        plan.items.iter().map(|i| i.target).collect()
    }

    /// The (from, to) pairs a run made, starting from the built plan's
    /// statuses; a `Blocked` item in the built plan is the build's own
    /// `Planned → Blocked`.
    fn observed(built: &CleanupPlan, run: Option<&Run>) -> BTreeSet<(u8, u8)> {
        let mut seen = BTreeSet::new();
        let mut current: BTreeMap<FileId, PlanItemStatus> = BTreeMap::new();
        for item in &built.items {
            if item.status == Blocked {
                seen.insert(key(Planned, Blocked));
            }
            current.insert(item.target, item.status);
        }
        for change in run.map(|r| r.changes.as_slice()).unwrap_or_default() {
            let from = current
                .insert(change.target, change.status)
                .expect("known target");
            assert!(
                check_transition(from, change.status).is_ok(),
                "the run made the illegal transition {from:?} → {:?}",
                change.status
            );
            seen.insert(key(from, change.status));
        }
        seen
    }

    fn key(from: PlanItemStatus, to: PlanItemStatus) -> (u8, u8) {
        let index = |s| {
            u8::try_from(ALL_STATUSES.iter().position(|x| *x == s).expect("status")).expect("small")
        };
        (index(from), index(to))
    }

    // ── Fixture script ───────────────────────────────────────────────────

    #[test]
    fn the_script_is_a_pure_function_of_the_scenario() {
        let scan = ScanId::new(3);
        for kind in ScenarioKind::ALL {
            let a = fixtures::build(kind, scan, DEFAULT_SEED).expect("builds");
            let b = fixtures::build(kind, scan, DEFAULT_SEED).expect("builds");
            let script = CleanupScript::for_scenario(&a);
            assert_eq!(script, CleanupScript::for_scenario(&b), "{kind:?}");
            if kind == ScenarioKind::Stale {
                let outcomes: BTreeSet<_> = script.entries().map(|(_, o)| o).collect();
                for wanted in [
                    ScriptedOutcome::NoTrash,
                    ScriptedOutcome::MoveFails,
                    ScriptedOutcome::Uncertain { moved: true },
                    ScriptedOutcome::Uncertain { moved: false },
                ] {
                    assert!(outcomes.contains(&wanted), "stale lacks {wanted:?}");
                }
                // The reading `CleanupScript::for_scenario` documents against
                // the stale seeds: the Kept file fails, the Protected file has
                // no Trash, and the seeded queued similar file moves.
                for seed in &a.seeded_decisions {
                    match *seed {
                        SeededDecision::Kept(file) => {
                            assert_eq!(script.outcome(file), ScriptedOutcome::MoveFails);
                        }
                        SeededDecision::Protect(file) => {
                            assert_eq!(script.outcome(file), ScriptedOutcome::NoTrash);
                        }
                        SeededDecision::Queue { file, .. } => assert!(matches!(
                            script.outcome(file),
                            ScriptedOutcome::Moves | ScriptedOutcome::Uncertain { moved: false }
                        )),
                        SeededDecision::MakeReference(_) => {}
                    }
                }
                for (file, _) in script.entries() {
                    let member = a.results.group_of(file).and_then(|g| g.member(file));
                    assert!(
                        member.is_some_and(|m| !m.is_reference()),
                        "stale scripts {file:?}, which is no queueable member"
                    );
                }
            } else {
                // Design's finished frame shows every move succeeding; every
                // other scenario uses the default.
                assert_eq!(script.entries().count(), 0, "{kind:?} scripts outcomes");
            }
        }
    }

    // ── Transition table ─────────────────────────────────────────────────

    #[test]
    fn an_illegal_transition_is_rejected() {
        let legal: BTreeSet<_> = TRANSITIONS.iter().map(|(f, t)| key(*f, *t)).collect();
        for from in ALL_STATUSES {
            for to in ALL_STATUSES {
                let result = check_transition(from, to);
                if legal.contains(&key(from, to)) {
                    assert!(result.is_ok(), "{from:?} → {to:?} is legal");
                } else {
                    let refused = result.expect_err("illegal transition accepted");
                    assert_eq!((refused.from, refused.to), (from, to));
                }
            }
        }
        // The C-06 safety rules, by name: nothing leaves a final state, a
        // stopped or failed item never re-enters a plan, and no item skips
        // its re-check.
        for (from, to) in [
            (Succeeded, Planned),
            (Succeeded, Approved),
            (Stopped, Planned),
            (Failed, Planned),
            (Failed, Approved),
            (Blocked, Approved),
            (Planned, Executing),
            (Approved, Executing),
            (Approved, Succeeded),
            (Validating, Succeeded),
        ] {
            assert!(check_transition(from, to).is_err(), "{from:?} → {to:?}");
        }
        for status in ALL_STATUSES {
            assert_eq!(DialogStatus::of(status).label(), spec_label(status));
        }
    }

    #[test]
    fn every_legal_transition_occurs_with_its_dialog_status() {
        let mut seen = BTreeSet::new();
        let mut labels = BTreeSet::new();
        let mut note = |built: &CleanupPlan, run: Option<&Run>| {
            seen.extend(observed(built, run));
            for item in built
                .items
                .iter()
                .chain(run.map(|r| r.changes.as_slice()).unwrap_or_default())
            {
                assert_eq!(
                    DialogStatus::of(item.status).label(),
                    spec_label(item.status)
                );
                labels.insert(spec_label(item.status));
            }
        };

        // Moved, failed, blocked and unreached-returned, in one stale run.
        let mut stale = adopt(ScenarioKind::Stale);
        let fails = stale.scripted(ScriptedOutcome::MoveFails);
        let blocked = stale.scripted(ScriptedOutcome::NoTrash);
        stale.review(ReviewCommand::MarkKept {
            file: fails,
            kept: false,
        });
        stale.review(ReviewCommand::ProtectFile {
            file: blocked,
            protected: false,
        });
        stale.queue(fails);
        stale.queue(blocked);
        let built = stale.build();
        let run = stale.execute(built.revision);
        note(&built, Some(&run));
        // The failure stopped the run with an unreached item behind it, so
        // its retry covers the uncertain-not-moved path.
        let revised = run.revised.clone().expect("revised plan after a failure");
        let retry = stale.execute(revised.revision);
        note(&revised, Some(&retry));

        // Uncertain, actually moved. The seeded uncertain-not-moved file is
        // unqueued, because it sits earlier in the plan and would stop the
        // run before this one.
        let mut moved = adopt(ScenarioKind::Stale);
        let earlier = moved.scripted(ScriptedOutcome::Uncertain { moved: false });
        moved.review(ReviewCommand::RemoveFromQueue { file: earlier });
        moved.queue(moved.scripted(ScriptedOutcome::Uncertain { moved: true }));
        let built = moved.build();
        let run = moved.execute(built.revision);
        note(&built, Some(&run));

        // A changed target.
        let mut design = adopt(ScenarioKind::Design);
        let built = design.build();
        let changed = built.items[1].target;
        design.review(ReviewCommand::UpdateEvidence(
            EvidenceUpdate::FileSnapshot {
                file: changed,
                snapshot: SnapshotId::new(9_000_000),
            },
        ));
        let run = design.execute(built.revision);
        note(&built, Some(&run));

        let legal: BTreeSet<_> = TRANSITIONS.iter().map(|(f, t)| key(*f, *t)).collect();
        assert_eq!(
            seen, legal,
            "every legal C-06 transition occurs, and only those"
        );
        let all: BTreeSet<_> = ALL_STATUSES.into_iter().map(spec_label).collect();
        assert_eq!(labels, all, "every dialog status is shown");
    }

    // ── One confirmation, one execution ──────────────────────────────────

    #[test]
    fn a_second_execute_of_one_authorization_is_refused() {
        let mut design = adopt(ScenarioKind::Design);
        let built = design.build();
        assert_eq!(built.items.len(), 3, "design queues three files");
        // The second submission arrives once the run has visibly started,
        // so a service that admitted it would run the plan a second time
        // rather than coalesce the two.
        design.submit_execute(built.revision);
        let mut first = None;
        design.drain_until(built.revision, |event| {
            if let CleanupEvent::ItemChanged(item) = event {
                first = Some(*item);
                true
            } else {
                false
            }
        });
        design.submit_execute(built.revision);
        let mut run = design.collect_run(built.revision);
        run.changes.insert(0, first.expect("the run started"));
        if run.refused == 0 {
            design.expect_refused(built.revision);
            run.refused = 1;
        }
        assert_eq!(run.refused, 1, "the second submission is refused once");
        for target in targets(&built) {
            let moves = run
                .changes
                .iter()
                .filter(|c| c.target == target && c.status == Executing)
                .count();
            assert_eq!(moves, 1, "{target:?} moved exactly once");
        }
        let summary = run.report.summary();
        assert_eq!(
            (summary.moved, summary.failed),
            (3, 0),
            "the frame shows 3 moved"
        );
        assert!(run.revised.is_none(), "a completed run leaves no plan");

        // After the run, the same authorization stays spent.
        design.submit_execute(built.revision);
        design.expect_refused(built.revision);
        // A plan the engine never issued is refused too.
        let unknown = PlanRevision::new(9_999);
        design.submit_execute(unknown);
        design.expect_refused(unknown);
    }

    // ── ERR-001 and ERR-003: stop before a change, stop after a failure ──

    #[test]
    fn a_changed_later_target_stops_the_run_before_it() {
        let mut design = adopt(ScenarioKind::Design);
        let built = design.build();
        let [a, b, c] = <[FileId; 3]>::try_from(targets(&built)).expect("three targets");
        // The change reaches the run only through review and the board.
        design.review(ReviewCommand::UpdateEvidence(
            EvidenceUpdate::FileSnapshot {
                file: b,
                snapshot: SnapshotId::new(9_000_000),
            },
        ));
        let run = design.execute(built.revision);

        assert_eq!(run.last(a).map(|i| i.status), Some(Succeeded));
        assert_eq!(run.statuses_of(b), [Approved, Validating, Stopped]);
        let stopped = run.last(b).expect("b changed");
        assert_eq!(stopped.reason, Some(PlanItemReason::TargetChanged));
        assert_eq!(DialogStatus::of(stopped.status).label(), "Stopped: changed");
        assert_eq!(
            run.statuses_of(c),
            [Approved, Planned],
            "c is never reached"
        );

        let revised = run.revised.clone().expect("revised plan");
        assert!(revised.revision > built.revision, "a new authorization");
        assert_eq!(targets(&revised), [c], "b left the plan, a stays moved");
        assert!(revised.items.iter().all(|i| i.status == Planned));
        assert_eq!(confirmed_count(&revised), 1);
        let summary = run.report.summary();
        assert_eq!(
            (
                summary.moved,
                summary.failed,
                summary.stopped,
                summary.returned
            ),
            (1, 0, 1, 1)
        );
        assert!(
            run.report
                .journal
                .iter()
                .any(|e| e.target == b && e.reason == Some(PlanItemReason::TargetChanged)),
            "the journal records the stop with its reason"
        );

        // The old authorization ended; the revised plan runs under its own.
        design.submit_execute(built.revision);
        design.expect_refused(built.revision);
        let retry = design.execute(revised.revision);
        assert_eq!(retry.last(c).map(|i| i.status), Some(Succeeded));
        assert!(
            !retry.touched(a) && !retry.touched(b),
            "no replay of a or b"
        );
    }

    #[test]
    fn a_counterpart_change_or_an_unqueue_stops_the_run_too() {
        // The stale fixture's first evidence change hits the seeded queued
        // file's counterpart.
        let mut stale = adopt(ScenarioKind::Stale);
        let built = stale.build();
        let change = stale.scenario.evidence_changes[0];
        stale.review(ReviewCommand::UpdateEvidence(change));
        let run = stale.execute(built.revision);
        let stopped: Vec<PlanItem> = run
            .changes
            .iter()
            .filter(|c| c.status == Stopped)
            .copied()
            .collect();
        assert_eq!(stopped.len(), 1, "{:?}", run.changes);
        assert_eq!(stopped[0].reason, Some(PlanItemReason::CounterpartChanged));

        let mut design = adopt(ScenarioKind::Design);
        let built = design.build();
        let first = built.items[0].target;
        design.review(ReviewCommand::RemoveFromQueue { file: first });
        let run = design.execute(built.revision);
        assert_eq!(run.statuses_of(first), [Approved, Validating, Stopped]);
        assert_eq!(
            run.last(first).and_then(|i| i.reason),
            Some(PlanItemReason::EvidenceChanged)
        );
        assert!(
            run.changes.iter().all(|c| c.status != Succeeded),
            "nothing moves past the stop"
        );
    }

    #[test]
    fn a_trash_failure_stops_the_run_after_the_failed_move() {
        let mut stale = adopt(ScenarioKind::Stale);
        let fails = stale.scripted(ScriptedOutcome::MoveFails);
        stale.review(ReviewCommand::MarkKept {
            file: fails,
            kept: false,
        });
        stale.queue(fails);
        let built = stale.build();
        let order = targets(&built);
        let at = order.iter().position(|t| *t == fails).expect("queued");
        assert!(
            at > 0 && at + 1 < order.len(),
            "fixture shape: the failure sits mid-plan in {order:?}"
        );
        let run = stale.execute(built.revision);

        for moved in &order[..at] {
            assert_eq!(run.last(*moved).map(|i| i.status), Some(Succeeded));
        }
        // Failed after the attempt, never "Stopped: changed".
        assert_eq!(
            run.statuses_of(fails),
            [Approved, Validating, Executing, Failed]
        );
        let failed = run.last(fails).expect("changed");
        assert_eq!(failed.reason, Some(PlanItemReason::TrashMoveFailed));
        assert_eq!(DialogStatus::of(failed.status).label(), "Failed");
        for rest in &order[at + 1..] {
            assert_eq!(run.statuses_of(*rest), [Approved, Planned]);
        }

        let summary = run.report.summary();
        assert_eq!(summary.moved, at, "the failed file is not counted as moved");
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.stopped, 0, "a failure is not a change");
        assert_eq!(
            summary.moved + summary.failed + summary.stopped + summary.blocked + summary.returned,
            built.items.len()
        );
        let report_item = run
            .report
            .items
            .iter()
            .find(|i| i.target == fails)
            .expect("in the report");
        assert_eq!(report_item.status, Failed);

        let revised = run.revised.clone().expect("the rest needs confirmation");
        assert_eq!(
            targets(&revised),
            order[at + 1..],
            "the failed file left the plan"
        );
    }

    // ── ERR-004 ──────────────────────────────────────────────────────────

    #[test]
    fn a_blocked_item_is_excluded_from_the_confirmed_count() {
        let mut stale = adopt(ScenarioKind::Stale);
        let blocked = stale.scripted(ScriptedOutcome::NoTrash);
        stale.review(ReviewCommand::ProtectFile {
            file: blocked,
            protected: false,
        });
        stale.queue(blocked);
        let built = stale.build();
        let item = built
            .items
            .iter()
            .find(|i| i.target == blocked)
            .expect("in the plan");
        assert_eq!(item.status, Blocked);
        assert_eq!(item.reason, Some(PlanItemReason::NoTrash));
        assert_eq!(DialogStatus::of(item.status).label(), "Blocked: no Trash");
        assert_eq!(confirmed_count(&built), built.items.len() - 1);

        let run = stale.execute(built.revision);
        assert!(
            run.statuses_of(blocked).is_empty(),
            "never approved or moved"
        );
        assert_eq!(run.report.summary().blocked, 1);
    }

    // ── ERR-002 and the targeted retry ───────────────────────────────────

    #[test]
    fn a_retry_after_reconciliation_re_executes_no_succeeded_item() {
        let mut stale = adopt(ScenarioKind::Stale);
        let not_moved = stale.scripted(ScriptedOutcome::Uncertain { moved: false });
        let moved = stale.scripted(ScriptedOutcome::Uncertain { moved: true });
        stale.queue(moved);
        let built = stale.build();
        let order = targets(&built);
        assert!(
            order.len() == 3 && order[1] == not_moved && order[2] == moved,
            "fixture shape: [default, uncertain-not-moved, uncertain-moved], got {order:?}"
        );
        let first = order[0];

        let run = stale.execute(built.revision);
        assert_eq!(run.last(first).map(|i| i.status), Some(Succeeded));
        // Uncertain, then reconciled from the simulated truth: not moved.
        assert_eq!(
            run.statuses_of(not_moved),
            [
                Approved,
                Validating,
                Executing,
                NeedsReconciliation,
                Planned
            ]
        );
        assert!(run.report.journal.iter().any(|e| e.target == not_moved
            && e.status == NeedsReconciliation
            && e.reason == Some(PlanItemReason::OutcomeUncertain)));
        let revised = run.revised.clone().expect("retry plan");
        assert_eq!(targets(&revised), [not_moved, moved]);

        let retry = stale.execute(revised.revision);
        assert!(!retry.touched(first), "the moved item is not replayed");
        assert_eq!(retry.last(not_moved).map(|i| i.status), Some(Succeeded));
        // Uncertain, reconciled as actually moved: Succeeded, never retried.
        assert_eq!(
            retry.statuses_of(moved),
            [
                Approved,
                Validating,
                Executing,
                NeedsReconciliation,
                Succeeded
            ]
        );
        assert!(retry.revised.is_none(), "nothing is left to retry");
        assert_eq!(retry.report.summary().moved, 2);

        // A fresh plan from the unchanged queue holds none of the moved files.
        let fresh = stale.build();
        assert!(
            fresh.items.is_empty(),
            "moved files re-entered a plan: {:?}",
            fresh.items
        );
    }

    // ── StopCleanup ──────────────────────────────────────────────────────

    #[test]
    fn a_stop_ends_the_authorization_and_returns_unreached_items() {
        // One-slot event queue: the run blocks on each event until the test
        // drains it, so the stop lands while the run is still in progress.
        let config = EngineConfig {
            event_capacity: 1,
            ..EngineConfig::default()
        };
        let mut design = adopt_with(ScenarioKind::Design, config);
        let built = design.build();
        design.submit_execute(built.revision);
        let mut first = None;
        design.drain_until(built.revision, |event| {
            if let CleanupEvent::ItemChanged(item) = event {
                first = Some(*item);
                true
            } else {
                false
            }
        });
        assert_eq!(first.map(|i| i.status), Some(Approved), "the run started");
        assert_eq!(
            design
                .engine
                .submit(EngineRequest::StopCleanup(built.revision)),
            Ok(Origin::Plan(built.revision))
        );
        let mut run = design.collect_run(built.revision);
        run.changes.insert(0, first.expect("first change"));

        let summary = run.report.summary();
        assert_eq!(
            summary.moved + summary.failed + summary.stopped + summary.blocked + summary.returned,
            built.items.len(),
            "counts add up"
        );
        assert_eq!((summary.failed, summary.stopped), (0, 0));
        let moved: Vec<FileId> = run
            .report
            .items
            .iter()
            .filter(|i| i.status == Succeeded)
            .map(|i| i.target)
            .collect();
        let returned: Vec<FileId> = run
            .report
            .items
            .iter()
            .filter(|i| i.status == Planned)
            .map(|i| i.target)
            .collect();
        assert_eq!(moved.len() + returned.len(), built.items.len());
        match &run.revised {
            Some(revised) => {
                assert_eq!(targets(revised), returned, "unreached items return");
                assert!(revised.items.iter().all(|i| i.status == Planned));
            }
            None => assert!(returned.is_empty(), "items left without a plan"),
        }

        // The stop ended this authorization.
        design.submit_execute(built.revision);
        design.expect_refused(built.revision);
    }
}
