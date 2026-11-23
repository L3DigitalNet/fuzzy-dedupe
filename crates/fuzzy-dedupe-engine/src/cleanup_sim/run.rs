// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! The cleanup service's two halves: [`Runner`], which the dispatcher calls,
//! and the worker thread it starts, which sends every event and runs every
//! plan.
//!
//! The halves share one [`Shared`] under a mutex. The dispatcher side only
//! records what was asked (a built plan to report, a run to start, a stop, a
//! refusal) and wakes the worker through a one-slot channel, so it never
//! blocks; the worker may block on the bounded event queue, which is the
//! backpressure the facade wants. One worker for all plans keeps each plan's
//! events in order: `PlanBuilt` before any of its item changes, and a
//! superseded run's `RunEnded` before the next plan's events. Rejected: a
//! thread per plan, whose events could interleave out of order, and a
//! command channel, which would grow without bound while the app leaves the
//! event queue full; every slot in `Shared` is bounded instead.
//!
//! Lock order: the board's lock (inside `validate_and_commit`) is never held
//! while `Shared` is taken, and `Shared` is held while `PlanRevisions` is
//! taken, never the reverse. Nothing else nests.
//!
//! No filesystem call exists here: a "move" is the scripted outcome decided
//! inside the board's commit closure, and the moved set is in memory.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use crossbeam_channel::{Receiver, Sender};
use fuzzy_dedupe_domain::{
    CleanupPlan, DecisionBinding, FileId, PlanItem, PlanItemReason, PlanItemStatus, PlanRevision,
    ScanId, ValidationView,
};

use super::{
    CleanupEvent, CleanupScript, JournalEntry, RunReport, ScriptedOutcome, check_transition,
};
use crate::fixtures::Scenario;
use crate::service::{
    ActiveScenario, EngineEvent, EventSink, ModuleContext, PlanRevisions, PublishedValidation,
    ValidationBoard,
};

/// Distinct plans whose refusal may wait for the worker. The cap only bites
/// when the app floods executions of plans it was never given while leaving
/// the event queue full; dropping those refusals then loses nothing the app
/// could act on.
const REFUSAL_BACKLOG: usize = 32;

/// Every critical section on `Shared` is a few field updates that cannot
/// panic half-way, so a poisoned guard still holds consistent data.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Authorization {
    /// Built, not yet executed or withdrawn.
    Fresh,
    /// Its one execution was admitted.
    Executing,
    /// Ran, or a stop withdrew it.
    Ended,
}

/// One plan with what its run needs beyond the published plan.
#[derive(Clone, Debug)]
struct Record {
    plan: Arc<CleanupPlan>,
    /// The approval binding of each item, index for index, as its queue
    /// entry recorded it when the plan was built.
    bindings: Arc<[DecisionBinding]>,
    script: CleanupScript,
    authorization: Authorization,
}

/// State shared by the dispatcher-side [`Runner`] and the worker.
#[derive(Debug, Default)]
struct Shared {
    /// The newest plan: the only one that may execute.
    latest: Option<Record>,
    /// A built plan whose `PlanBuilt` is not sent yet. A newer build
    /// replaces it unsent; presentation discards a superseded plan's events
    /// anyway.
    unreported: Option<Arc<CleanupPlan>>,
    /// The admitted execution, with its plan as admitted.
    go: Option<Record>,
    /// A stop for the plan that is running.
    stop: Option<PlanRevision>,
    /// A plan a stop withdrew before it ran, for its `RunEnded`.
    withdrawn: Option<Arc<CleanupPlan>>,
    refusals: Vec<PlanRevision>,
    /// The scan of `moved`; a build for another scan starts afresh.
    moved_scan: Option<ScanId>,
    /// Every target the simulation moved in `moved_scan`.
    moved: BTreeSet<FileId>,
    /// The service is gone: the worker exits and a run stops.
    closed: bool,
}

/// The dispatcher-side half of the cleanup service.
#[derive(Debug)]
pub(super) struct Runner {
    shared: Arc<Mutex<Shared>>,
    /// `None` when the worker could not be started.
    wake: Option<Sender<()>>,
    events: EventSink,
    active: ActiveScenario,
    revisions: PlanRevisions,
}

impl Runner {
    pub(super) fn new(context: &ModuleContext) -> Self {
        let shared = Arc::new(Mutex::new(Shared::default()));
        let (wake_tx, wake_rx) = crossbeam_channel::bounded(1);
        let worker = Worker {
            shared: Arc::clone(&shared),
            wake: wake_rx,
            events: context.events.clone(),
            validation: context.validation.clone(),
            revisions: context.plan_revisions.clone(),
        };
        let spawned = std::thread::Builder::new()
            .name("cleanup-sim".into())
            .spawn(move || worker.serve())
            .is_ok();
        Self {
            shared,
            wake: spawned.then_some(wake_tx),
            events: context.events.clone(),
            active: context.active.clone(),
            revisions: context.plan_revisions.clone(),
        }
    }

    pub(super) fn build(&self, plan: PlanRevision, validation: &PublishedValidation) {
        if self.wake.is_none() {
            return self.refuse_without_worker(plan);
        }
        let view = &validation.view;
        // The active scenario is the script's source. When it belongs to
        // another scan (a newer scan published before the review service
        // loaded it), the default script and unknown sizes are safe: the
        // run's per-item check then fails on the scan and moves nothing.
        let scenario = self.active.get().filter(|s| s.results.scan == view.scan);
        let moved = {
            let shared = lock(&self.shared);
            if shared.moved_scan == Some(view.scan) {
                shared.moved.clone()
            } else {
                BTreeSet::new()
            }
        };
        let record = build_record(plan, view, scenario.as_deref(), &moved);
        let mut shared = lock(&self.shared);
        shared.unreported = Some(Arc::clone(&record.plan));
        // A run of the older plan sees `latest` change and stops before its
        // next item.
        shared.latest = Some(record);
        drop(shared);
        self.signal();
    }

    pub(super) fn execute(&self, plan: PlanRevision) {
        if self.wake.is_none() {
            return self.refuse_without_worker(plan);
        }
        // Read before `Shared` is taken (lock order). Only the live plan's
        // StopCleanup reaches this service, so a plan that is not live must
        // not start: nothing could stop it.
        let live = self.revisions.active() == Some(plan);
        let mut shared = lock(&self.shared);
        let admitted = match &mut shared.latest {
            Some(record)
                if live
                    && record.plan.revision == plan
                    && record.authorization == Authorization::Fresh =>
            {
                record.authorization = Authorization::Executing;
                Some(record.clone())
            }
            _ => None,
        };
        match admitted {
            Some(record) => shared.go = Some(record),
            None => {
                if !shared.refusals.contains(&plan) && shared.refusals.len() < REFUSAL_BACKLOG {
                    shared.refusals.push(plan);
                }
            }
        }
        drop(shared);
        self.signal();
    }

    pub(super) fn stop(&self, plan: PlanRevision) {
        let mut shared = lock(&self.shared);
        let mut withdrawn = None;
        if let Some(record) = shared.latest.as_mut().filter(|r| r.plan.revision == plan) {
            match record.authorization {
                Authorization::Fresh => {
                    record.authorization = Authorization::Ended;
                    withdrawn = Some(Arc::clone(&record.plan));
                }
                Authorization::Executing => shared.stop = Some(plan),
                Authorization::Ended => {}
            }
        }
        if withdrawn.is_some() {
            shared.withdrawn = withdrawn;
        }
        drop(shared);
        self.signal();
    }

    fn signal(&self) {
        if let Some(wake) = &self.wake {
            // A full slot already means "look at Shared"; a closed one means
            // the worker is gone, and so is the engine.
            let _ = wake.try_send(());
        }
    }

    /// The OS refused the worker thread, so no plan can be reported or run.
    /// Refusing each request keeps the dialog from waiting forever; nothing
    /// is ever moved. Best effort, because the dispatcher must not block.
    fn refuse_without_worker(&self, plan: PlanRevision) {
        let refusal = EngineEvent::Cleanup {
            plan,
            event: CleanupEvent::ExecutionRefused,
        };
        let _ = self.events.try_send(refusal);
    }
}

impl Drop for Runner {
    /// The dispatcher has exited. The worker exits on the closed wake
    /// channel, and a run in progress stops before its next item.
    fn drop(&mut self) {
        lock(&self.shared).closed = true;
        self.wake = None;
    }
}

/// Builds plan `plan` from `view`'s queue, in queue order, leaving out the
/// targets in `moved`. A target the script gives no usable Trash is
/// `Blocked` from the start (the build's `Planned → Blocked`).
fn build_record(
    plan: PlanRevision,
    view: &ValidationView,
    scenario: Option<&Scenario>,
    moved: &BTreeSet<FileId>,
) -> Record {
    let script = scenario
        .map(CleanupScript::for_scenario)
        .unwrap_or_default();
    let queue: Vec<_> = view
        .queue
        .iter()
        .filter(|q| !moved.contains(&q.target))
        .collect();
    let wanted: BTreeSet<FileId> = queue.iter().map(|q| q.target).collect();
    let sizes: BTreeMap<FileId, u64> = scenario
        .map(|s| {
            s.results
                .groups
                .iter()
                .flat_map(|g| &g.members)
                .filter(|m| wanted.contains(&m.file))
                .map(|m| (m.file, m.size_bytes))
                .collect()
        })
        .unwrap_or_default();
    let items = queue
        .iter()
        .map(|q| {
            let blocked = script.outcome(q.target) == ScriptedOutcome::NoTrash;
            PlanItem {
                target: q.target,
                counterpart: q.counterpart,
                size_bytes: sizes.get(&q.target).copied().unwrap_or(0),
                status: if blocked {
                    PlanItemStatus::Blocked
                } else {
                    PlanItemStatus::Planned
                },
                reason: blocked.then_some(PlanItemReason::NoTrash),
            }
        })
        .collect();
    Record {
        plan: Arc::new(CleanupPlan {
            revision: plan,
            scan: view.scan,
            items,
        }),
        bindings: queue.iter().map(|q| q.binding).collect(),
        script,
        authorization: Authorization::Fresh,
    }
}

/// Whether the item bound by `binding` may move now. All three checks run
/// against one view, inside one `validate_and_commit` call.
///
/// `binding_is_current` covers the queue entry, both snapshots, the rules
/// and the scan, but not protection, so protection is checked separately:
/// a folder protection or a valid per-file Protect stops the item even if
/// some review path were ever to leave its approval in place. The explicit
/// queue check repeats what the binding check implies, so that the "still
/// queued" rule does not rest on that function's internals.
///
/// The reason prefers the most specific cause: the target's snapshot, then
/// the counterpart's, then any other change to the approval (unqueued,
/// protected, re-queued against another counterpart, rules or scan).
fn verify(view: &ValidationView, binding: &DecisionBinding) -> Result<(), PlanItemReason> {
    let current = view.binding_is_current(binding);
    let protected = view.is_protected(binding.target);
    let queued = view.queued(binding.target).is_some();
    if current && !protected && queued {
        return Ok(());
    }
    Err(
        if view.snapshots.get(&binding.target) != Some(&binding.target_snapshot) {
            PlanItemReason::TargetChanged
        } else if view.snapshots.get(&binding.counterpart) != Some(&binding.counterpart_snapshot) {
            PlanItemReason::CounterpartChanged
        } else {
            PlanItemReason::EvidenceChanged
        },
    )
}

/// The worker half: owns the event sends and the runs.
struct Worker {
    shared: Arc<Mutex<Shared>>,
    wake: Receiver<()>,
    events: EventSink,
    validation: ValidationBoard,
    revisions: PlanRevisions,
}

/// One batch of work taken from `Shared`.
struct Batch {
    unreported: Option<Arc<CleanupPlan>>,
    withdrawn: Option<Arc<CleanupPlan>>,
    refusals: Vec<PlanRevision>,
    go: Option<Record>,
}

impl Worker {
    fn serve(self) {
        while self.wake.recv().is_ok() {
            if !self.drain() {
                return;
            }
        }
    }

    /// Serves everything recorded so far. Returns `false` once the engine or
    /// the service is gone.
    fn drain(&self) -> bool {
        loop {
            let batch = {
                let mut shared = lock(&self.shared);
                if shared.closed {
                    return false;
                }
                Batch {
                    unreported: shared.unreported.take(),
                    withdrawn: shared.withdrawn.take(),
                    refusals: std::mem::take(&mut shared.refusals),
                    go: shared.go.take(),
                }
            };
            if batch.unreported.is_none()
                && batch.withdrawn.is_none()
                && batch.refusals.is_empty()
                && batch.go.is_none()
            {
                return true;
            }
            // A plan is reported before its withdrawal, its refusals and its
            // run, whichever of them arrived in this batch.
            if let Some(plan) = batch.unreported
                && !self.emit(plan.revision, CleanupEvent::PlanBuilt(Arc::clone(&plan)))
            {
                return false;
            }
            if let Some(plan) = batch.withdrawn {
                let report = RunReport {
                    plan: plan.revision,
                    items: plan.items.clone(),
                    journal: Vec::new(),
                };
                let ended = CleanupEvent::RunEnded {
                    revised: None,
                    report: Arc::new(report),
                };
                if !self.emit(plan.revision, ended) {
                    return false;
                }
            }
            for plan in batch.refusals {
                if !self.emit(plan, CleanupEvent::ExecutionRefused) {
                    return false;
                }
            }
            if let Some(record) = batch.go
                && !Run::new(self, record).execute()
            {
                return false;
            }
        }
    }

    fn emit(&self, plan: PlanRevision, event: CleanupEvent) -> bool {
        self.events.send(EngineEvent::Cleanup { plan, event })
    }
}

/// Why a run cannot continue normally.
enum Abort {
    /// The engine is gone; nothing is left to report to.
    EngineGone,
    /// A bug: the run asked for a transition outside the table. The run
    /// moves nothing more and offers no revised plan, rather than act on an
    /// item whose state it can no longer trust.
    Illegal,
}

/// One execution of one plan, on the worker.
struct Run<'w> {
    worker: &'w Worker,
    revision: PlanRevision,
    scan: ScanId,
    items: Vec<PlanItem>,
    bindings: Arc<[DecisionBinding]>,
    script: CleanupScript,
    journal: Vec<JournalEntry>,
    /// Items left `NeedsReconciliation`, with the simulated truth.
    uncertain: Vec<(usize, bool)>,
}

impl<'w> Run<'w> {
    fn new(worker: &'w Worker, record: Record) -> Self {
        Self {
            worker,
            revision: record.plan.revision,
            scan: record.plan.scan,
            items: record.plan.items.clone(),
            bindings: record.bindings,
            script: record.script,
            journal: Vec::new(),
            uncertain: Vec::new(),
        }
    }

    /// Runs the plan to its end and reports it. Returns `false` once the
    /// engine is gone.
    fn execute(mut self) -> bool {
        let revisable = match self.steps().and_then(|()| self.settle()) {
            Ok(revisable) => revisable,
            Err(Abort::EngineGone) => return false,
            Err(Abort::Illegal) => false,
        };
        self.end(revisable)
    }

    /// Confirms every planned item, then moves them in order until one stops
    /// the run.
    fn steps(&mut self) -> Result<(), Abort> {
        // The run-level check: a plan for a scan the review service no
        // longer holds has nothing to revalidate against. Nothing is
        // confirmed and nothing moves.
        let scan = self.scan;
        let same_scan = self
            .worker
            .validation
            .validate_and_commit(|published| published.view.scan == scan);
        if same_scan.is_none_or(|(_, same)| !same) {
            return Ok(());
        }
        for index in 0..self.items.len() {
            if self.items[index].status == PlanItemStatus::Planned {
                self.set(index, PlanItemStatus::Approved, None)?;
            }
        }
        for index in 0..self.items.len() {
            if self.items[index].status != PlanItemStatus::Approved {
                continue;
            }
            if self.should_stop() {
                return Ok(());
            }
            if !self.step(index)? {
                return Ok(());
            }
        }
        Ok(())
    }

    /// Re-checks and moves one item. Returns whether the run continues.
    fn step(&mut self, index: usize) -> Result<bool, Abort> {
        self.set(index, PlanItemStatus::Validating, None)?;
        let binding = self.bindings[index];
        let script = &self.script;
        // The check and the simulated move happen in this one call, under
        // the board's lock: no review change can fall between them. The
        // outcome is decided here and only reported afterwards, because the
        // closure must not send events.
        let verdict = self
            .worker
            .validation
            .validate_and_commit(|published| {
                verify(&published.view, &binding).map(|()| script.outcome(binding.target))
            })
            .map(|(_, verdict)| verdict);
        let outcome = match verdict {
            // No published view: nothing can authorize the move.
            None => Err(PlanItemReason::EvidenceChanged),
            Some(verdict) => verdict,
        };
        let outcome = match outcome {
            Ok(outcome) => outcome,
            Err(reason) => {
                self.set(index, PlanItemStatus::Stopped, Some(reason))?;
                return Ok(false);
            }
        };
        self.set(index, PlanItemStatus::Executing, None)?;
        match outcome {
            ScriptedOutcome::Moves => {
                self.set(index, PlanItemStatus::Succeeded, None)?;
                self.record_moved(binding.target);
                Ok(true)
            }
            // NoTrash items are blocked at build and never reach a move; a
            // Trash that vanished since would fail the move the same way.
            ScriptedOutcome::MoveFails | ScriptedOutcome::NoTrash => {
                self.set(
                    index,
                    PlanItemStatus::Failed,
                    Some(PlanItemReason::TrashMoveFailed),
                )?;
                Ok(false)
            }
            // The journal no longer knows where the file is, so continuing
            // would act on a filesystem state the run cannot vouch for.
            ScriptedOutcome::Uncertain { moved } => {
                self.set(
                    index,
                    PlanItemStatus::NeedsReconciliation,
                    Some(PlanItemReason::OutcomeUncertain),
                )?;
                self.uncertain.push((index, moved));
                Ok(false)
            }
        }
    }

    /// Simulated reconciliation, then the return of unreached items. Runs
    /// before the revised plan is built, so an item that actually moved is
    /// `Succeeded` and never retried, and one that did not is `Planned` and
    /// in the revised plan. Returns that a revised plan may be offered.
    fn settle(&mut self) -> Result<bool, Abort> {
        for (index, moved) in std::mem::take(&mut self.uncertain) {
            let target = self.items[index].target;
            if moved {
                self.set(index, PlanItemStatus::Succeeded, None)?;
                self.record_moved(target);
            } else {
                self.set(index, PlanItemStatus::Planned, None)?;
            }
            self.script.settle(target);
        }
        for index in 0..self.items.len() {
            if self.items[index].status == PlanItemStatus::Approved {
                self.set(index, PlanItemStatus::Planned, None)?;
            }
        }
        Ok(true)
    }

    /// Ends the authorization, offers the remaining items as a revised plan
    /// when there are any and this plan is still the newest, and reports
    /// the run. Returns `false` once the engine is gone.
    fn end(self, revisable: bool) -> bool {
        let remaining: Vec<usize> = (0..self.items.len())
            .filter(|i| {
                matches!(
                    self.items[*i].status,
                    PlanItemStatus::Planned | PlanItemStatus::Blocked
                )
            })
            .collect();
        let confirmable = remaining
            .iter()
            .any(|i| self.items[*i].status == PlanItemStatus::Planned);
        let mut shared = lock(&self.worker.shared);
        if shared.stop == Some(self.revision) {
            shared.stop = None;
        }
        let closed = shared.closed;
        let current = shared
            .latest
            .as_mut()
            .filter(|r| r.plan.revision == self.revision);
        let revised = match current {
            Some(record) if revisable && confirmable && !closed => {
                // Allocated and activated under `Shared`, so no build or
                // execution can see this plan's record without its revision
                // being live.
                let revision = self.worker.revisions.allocate();
                self.worker.revisions.activate(revision);
                let plan = Arc::new(CleanupPlan {
                    revision,
                    scan: self.scan,
                    items: remaining.iter().map(|i| self.items[*i]).collect(),
                });
                *record = Record {
                    plan: Arc::clone(&plan),
                    bindings: remaining.iter().map(|i| self.bindings[*i]).collect(),
                    script: self.script.clone(),
                    authorization: Authorization::Fresh,
                };
                Some(plan)
            }
            Some(record) => {
                record.authorization = Authorization::Ended;
                None
            }
            None => None,
        };
        drop(shared);
        let report = RunReport {
            plan: self.revision,
            items: self.items,
            journal: self.journal,
        };
        self.worker.emit(
            self.revision,
            CleanupEvent::RunEnded {
                revised,
                report: Arc::new(report),
            },
        )
    }

    /// A stop for this plan, a newer plan, or a closed service ends the run
    /// before its next item.
    fn should_stop(&self) -> bool {
        let shared = lock(&self.worker.shared);
        shared.closed
            || shared.stop == Some(self.revision)
            || shared.latest.as_ref().map(|r| r.plan.revision) != Some(self.revision)
    }

    fn record_moved(&self, target: FileId) {
        let mut shared = lock(&self.worker.shared);
        if shared.moved_scan != Some(self.scan) {
            shared.moved_scan = Some(self.scan);
            shared.moved.clear();
        }
        shared.moved.insert(target);
    }

    /// Makes one item change through the transition table, journals it and
    /// reports it.
    fn set(
        &mut self,
        index: usize,
        to: PlanItemStatus,
        reason: Option<PlanItemReason>,
    ) -> Result<(), Abort> {
        let item = &mut self.items[index];
        check_transition(item.status, to).map_err(|_| Abort::Illegal)?;
        item.status = to;
        item.reason = reason;
        let item = *item;
        self.journal.push(JournalEntry {
            target: item.target,
            status: to,
            reason,
        });
        if self
            .worker
            .emit(self.revision, CleanupEvent::ItemChanged(item))
        {
            Ok(())
        } else {
            Err(Abort::EngineGone)
        }
    }
}

#[cfg(test)]
mod tests {
    //! The per-item check, over hand-built views. Some of these states are
    //! unreachable through the review service (it never leaves a protected
    //! file queued, C-05 I-2), which is exactly why the run checks them on
    //! its own.

    use std::collections::{BTreeMap, BTreeSet};
    use std::sync::Arc;

    use fuzzy_dedupe_domain::{
        DecisionBinding, FileId, PlanItemReason, QueuedTarget, RulesRevision, ScanId, SnapshotId,
        ValidationView,
    };

    use super::verify;

    const TARGET: FileId = FileId::new(2);
    const COUNTERPART: FileId = FileId::new(1);

    fn binding() -> DecisionBinding {
        DecisionBinding {
            scan: ScanId::new(1),
            target: TARGET,
            target_snapshot: SnapshotId::new(20),
            counterpart: COUNTERPART,
            counterpart_snapshot: SnapshotId::new(10),
            rules: RulesRevision::new(1),
        }
    }

    fn view() -> ValidationView {
        ValidationView {
            scan: ScanId::new(1),
            queue: vec![QueuedTarget {
                target: TARGET,
                counterpart: COUNTERPART,
                binding: binding(),
            }],
            protected: BTreeSet::new(),
            snapshots: Arc::new(BTreeMap::from([
                (COUNTERPART, SnapshotId::new(10)),
                (TARGET, SnapshotId::new(20)),
            ])),
            rules: RulesRevision::new(1),
        }
    }

    #[test]
    fn a_current_queued_unprotected_target_may_move() {
        assert_eq!(verify(&view(), &binding()), Ok(()));
    }

    #[test]
    fn a_protected_target_stops_even_with_a_current_binding() {
        let mut view = view();
        view.protected.insert(TARGET);
        assert!(
            view.binding_is_current(&binding()),
            "fixture: binding holds"
        );
        assert_eq!(
            verify(&view, &binding()),
            Err(PlanItemReason::EvidenceChanged)
        );
    }

    #[test]
    fn each_changed_component_stops_with_its_reason() {
        let mut target = view();
        target.snapshots = Arc::new(BTreeMap::from([
            (COUNTERPART, SnapshotId::new(10)),
            (TARGET, SnapshotId::new(21)),
        ]));
        assert_eq!(
            verify(&target, &binding()),
            Err(PlanItemReason::TargetChanged)
        );

        let mut counterpart = view();
        counterpart.snapshots = Arc::new(BTreeMap::from([
            (COUNTERPART, SnapshotId::new(11)),
            (TARGET, SnapshotId::new(20)),
        ]));
        assert_eq!(
            verify(&counterpart, &binding()),
            Err(PlanItemReason::CounterpartChanged)
        );

        let mut unqueued = view();
        unqueued.queue.clear();
        assert_eq!(
            verify(&unqueued, &binding()),
            Err(PlanItemReason::EvidenceChanged)
        );

        // Re-queued against another counterpart with unchanged snapshots.
        let mut requeued = view();
        requeued.queue[0].counterpart = FileId::new(3);
        requeued.queue[0].binding.counterpart = FileId::new(3);
        assert_eq!(
            verify(&requeued, &binding()),
            Err(PlanItemReason::EvidenceChanged)
        );

        let mut rules = view();
        rules.rules = RulesRevision::new(2);
        assert_eq!(
            verify(&rules, &binding()),
            Err(PlanItemReason::EvidenceChanged)
        );
    }
}
