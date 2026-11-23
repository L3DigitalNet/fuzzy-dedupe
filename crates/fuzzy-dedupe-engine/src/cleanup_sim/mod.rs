// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! Simulated cleanup run (plan C-06, spec §10.4, §11.6).
//!
//! [`CleanupService`] sits behind the calls the dispatcher in
//! [`crate::service`] makes: `new`, `build`, `execute` and `stop`. It acts on
//! file IDs only, with outcomes scripted per fixture ([`CleanupScript`]); no
//! filesystem call exists in this module or in `run.rs` (plan D-005). The
//! dispatcher-side calls only record state and wake one worker thread the
//! service owns (`run.rs`), which sends every event and runs every plan, so
//! a plan's events keep their order and the dispatcher never blocks.
//!
//! Revalidation (spec §10.4, ERR-001): `build` receives the scan-checked
//! validation view the plan is built from, and the plan items keep each
//! target's approval binding from its queue entry. Before executing, and
//! for each simulated move, the run calls
//! `ModuleContext::validation.validate_and_commit` and, inside that one
//! call, checks the item and records the move's outcome: it stops before a
//! target that is protected or whose recorded binding is no longer the
//! current queued approval (`ValidationView::binding_is_current`, which
//! compares the whole binding, counterpart included, and the current
//! snapshots and rules). Checking and committing in separate calls would let
//! a review change fall between them. Keep the commit closure short and do
//! not send events from inside it.
//!
//! Plan revisions: a revised plan after a stop takes its revision from
//! `ModuleContext::plan_revisions.allocate()` and is made live with
//! `activate` before it is reported, so it can never collide with a plan the
//! facade builds and a `StopCleanup` for it is delivered.
//!
//! Authorization rules (C-06 "second execution with one authorization
//! refused"): a plan revision executes at most once, and only while it is
//! both the service's newest plan and the facade's live plan. A plan that is
//! not live could not be stopped, because the facade drops a `StopCleanup`
//! for any other revision, so it must never start. A stop ends the
//! authorization whether or not the run had begun, and a newer plan
//! supersedes an older one: a run still in progress stops before its next
//! item and reports no revised plan.
//!
//! The run is not paced: each simulated move completes as soon as the event
//! queue takes its events, and the bounded event queue's backpressure is
//! the only pacing. A run never waits on a clock, so no test needs one.

mod run;

use std::collections::BTreeMap;
use std::sync::Arc;

use fuzzy_dedupe_domain::{
    CleanupPlan, FileId, PlanItem, PlanItemReason, PlanItemStatus, PlanRevision,
};

use crate::fixtures::{Scenario, ScenarioKind};
use crate::service::{ModuleContext, NotImplemented, PublishedValidation};

/// What a cleanup run reports, carried by `EngineEvent::Cleanup`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CleanupEvent {
    /// The plan was built; blocked items are already marked.
    PlanBuilt(Arc<CleanupPlan>),
    /// One item changed state, in the order the run made the changes.
    ItemChanged(PlanItem),
    /// The run ended, or a stop withdrew a plan before it ran. After a stop,
    /// a change, a failure or an uncertain outcome, `revised` holds the
    /// remaining items under a new revision for re-confirmation; it is
    /// `None` when no item is left to confirm, or when a newer plan
    /// superseded this one. `report` backs the finished summary and Show
    /// journal.
    RunEnded {
        revised: Option<Arc<CleanupPlan>>,
        report: Arc<RunReport>,
    },
    /// A second execution of one authorization, or one of an unknown, ended
    /// or superseded plan. Nothing was changed.
    ExecutionRefused,
}

/// A fixture's scripted Trash behavior for one target (executor discretion
/// of plan T8). The default is a successful move.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ScriptedOutcome {
    /// Trash is available and the move succeeds.
    #[default]
    Moves,
    /// No usable Trash: the item is `Blocked` when the plan is built
    /// (ERR-004) and never runs.
    NoTrash,
    /// The Trash move fails: `Failed` after the attempt, and the run stops
    /// (ERR-003).
    MoveFails,
    /// The outcome is uncertain (ERR-002): `NeedsReconciliation`, and the run
    /// stops. `moved` is the simulated truth that reconciliation finds.
    Uncertain { moved: bool },
}

/// The scripted outcome of each target of one scenario; a target that is
/// absent moves successfully.
///
/// Derived by [`CleanupScript::for_scenario`], a pure function of the
/// scenario, so the running app, the UI checks and the tests all see the
/// same script and tests choose their targets from it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CleanupScript(BTreeMap<FileId, ScriptedOutcome>);

impl CleanupScript {
    /// The script `scenario` defines.
    ///
    /// Only [`ScenarioKind::Stale`] scripts anything: it exists for the
    /// cleanup stale-evidence paths and backs UI check 11, so it carries
    /// every non-default outcome. Its non-reference members, in result order
    /// (groups, then members), take [`STALE_SEQUENCE`] in turn. Read against
    /// the stale fixture's seeds: the seeded queued similar file moves (and
    /// is the one the first evidence change stops), the seeded Kept file
    /// fails, the seeded Protected file has no Trash, and the two other
    /// copies of the exact group are uncertain, first not moved and then
    /// moved. [`ScenarioKind::Design`] moves everything, because its
    /// finished frame shows 3 moved; every other scenario uses the default.
    #[must_use]
    pub fn for_scenario(scenario: &Scenario) -> Self {
        match scenario.kind {
            ScenarioKind::Stale => Self(
                scenario
                    .results
                    .groups
                    .iter()
                    .flat_map(|g| &g.members)
                    .filter(|m| !m.is_reference())
                    .map(|m| m.file)
                    .zip(STALE_SEQUENCE)
                    .filter(|(_, outcome)| *outcome != ScriptedOutcome::Moves)
                    .collect(),
            ),
            ScenarioKind::Large
            | ScenarioKind::Mixed
            | ScenarioKind::Issues
            | ScenarioKind::Empty
            | ScenarioKind::Unicode
            | ScenarioKind::Design => Self::default(),
        }
    }

    /// The outcome a move of `file` has.
    #[must_use]
    pub fn outcome(&self, file: FileId) -> ScriptedOutcome {
        self.0.get(&file).copied().unwrap_or_default()
    }

    /// Every target with a non-default outcome, in file-ID order.
    pub fn entries(&self) -> impl Iterator<Item = (FileId, ScriptedOutcome)> + '_ {
        self.0.iter().map(|(file, outcome)| (*file, *outcome))
    }

    /// Clears `file`'s scripted outcome once reconciliation settled it. The
    /// scripted uncertainty models one interrupted move; without this, a
    /// reconciled file that had not moved would turn uncertain again on
    /// every retry and the targeted retry could never finish.
    fn settle(&mut self, file: FileId) {
        self.0.remove(&file);
    }
}

/// The stale scenario's outcomes, assigned in order to its non-reference
/// members (see [`CleanupScript::for_scenario`]).
pub const STALE_SEQUENCE: [ScriptedOutcome; 5] = [
    ScriptedOutcome::Moves,
    ScriptedOutcome::MoveFails,
    ScriptedOutcome::NoTrash,
    ScriptedOutcome::Uncertain { moved: false },
    ScriptedOutcome::Uncertain { moved: true },
];

/// The Review cleanup dialog's Status column (spec §10.4 table, §11.6).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DialogStatus {
    Ready,
    BlockedNoTrash,
    Waiting,
    /// Both `validating` and `executing`.
    Moving,
    Moved,
    Failed,
    StoppedChanged,
    NeedsCheck,
}

impl DialogStatus {
    #[must_use]
    pub const fn of(status: PlanItemStatus) -> Self {
        match status {
            PlanItemStatus::Planned => Self::Ready,
            PlanItemStatus::Blocked => Self::BlockedNoTrash,
            PlanItemStatus::Approved => Self::Waiting,
            PlanItemStatus::Validating | PlanItemStatus::Executing => Self::Moving,
            PlanItemStatus::Succeeded => Self::Moved,
            PlanItemStatus::Failed => Self::Failed,
            PlanItemStatus::Stopped => Self::StoppedChanged,
            PlanItemStatus::NeedsReconciliation => Self::NeedsCheck,
        }
    }

    /// The label the dialog shows. "Failed" and "Needs check" are the
    /// spec's wording; their Figma labels are not drawn yet.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Ready => "Ready",
            Self::BlockedNoTrash => "Blocked: no Trash",
            Self::Waiting => "Waiting",
            Self::Moving => "Moving…",
            Self::Moved => "Moved",
            Self::Failed => "Failed",
            Self::StoppedChanged => "Stopped: changed",
            Self::NeedsCheck => "Needs check",
        }
    }
}

/// Every legal C-06 item transition. Everything else is illegal, notably
/// any exit from `Succeeded`, `Failed`, `Stopped` or `Blocked`, so a moved
/// item is never replayed and a changed or failed one never re-enters a
/// plan; and any route to `Executing` that skips `Validating`. There is no
/// `skipped` state to reach.
pub const TRANSITIONS: [(PlanItemStatus, PlanItemStatus); 11] = [
    // Build: no usable Trash (ERR-004).
    (PlanItemStatus::Planned, PlanItemStatus::Blocked),
    // Confirmation.
    (PlanItemStatus::Planned, PlanItemStatus::Approved),
    (PlanItemStatus::Approved, PlanItemStatus::Validating),
    // The run ended before this item: it returns to the revised plan.
    (PlanItemStatus::Approved, PlanItemStatus::Planned),
    (PlanItemStatus::Validating, PlanItemStatus::Executing),
    // A stale precondition stops the run before the move (ERR-001).
    (PlanItemStatus::Validating, PlanItemStatus::Stopped),
    (PlanItemStatus::Executing, PlanItemStatus::Succeeded),
    // ERR-003.
    (PlanItemStatus::Executing, PlanItemStatus::Failed),
    // ERR-002.
    (
        PlanItemStatus::Executing,
        PlanItemStatus::NeedsReconciliation,
    ),
    // Reconciliation found the file moved, or not moved.
    (
        PlanItemStatus::NeedsReconciliation,
        PlanItemStatus::Succeeded,
    ),
    (PlanItemStatus::NeedsReconciliation, PlanItemStatus::Planned),
];

/// A transition outside [`TRANSITIONS`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct IllegalTransition {
    pub from: PlanItemStatus,
    pub to: PlanItemStatus,
}

/// Accepts exactly the transitions in [`TRANSITIONS`]. The run makes every
/// item change through this check.
///
/// # Errors
///
/// [`IllegalTransition`] for any other pair, including a repeat of the
/// current state.
pub fn check_transition(from: PlanItemStatus, to: PlanItemStatus) -> Result<(), IllegalTransition> {
    if TRANSITIONS.contains(&(from, to)) {
        Ok(())
    } else {
        Err(IllegalTransition { from, to })
    }
}

/// One state change in a run's in-memory journal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct JournalEntry {
    pub target: FileId,
    pub status: PlanItemStatus,
    pub reason: Option<PlanItemReason>,
}

/// The record of one ended run: every item in its final state, and the
/// per-item journal of every change the run made, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunReport {
    pub plan: PlanRevision,
    pub items: Vec<PlanItem>,
    pub journal: Vec<JournalEntry>,
}

/// The finished summary's counts ("N moved · N failed"). Every item of the
/// run's plan is counted exactly once, so the fields add up to its length.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct RunSummary {
    pub moved: usize,
    /// Trash failures (ERR-003), never counted as moved or as stopped.
    pub failed: usize,
    /// Stopped before the move because something changed.
    pub stopped: usize,
    pub blocked: usize,
    /// Back in `Planned`: unreached, or reconciled as not moved.
    pub returned: usize,
}

impl RunReport {
    #[must_use]
    pub fn summary(&self) -> RunSummary {
        let mut summary = RunSummary::default();
        for item in &self.items {
            let slot = match item.status {
                PlanItemStatus::Succeeded => &mut summary.moved,
                PlanItemStatus::Failed => &mut summary.failed,
                PlanItemStatus::Stopped => &mut summary.stopped,
                PlanItemStatus::Blocked => &mut summary.blocked,
                // A run never ends with an item mid-move or unreconciled;
                // were it to, the item has not moved, and it is counted as
                // returned rather than dropped from the sum.
                PlanItemStatus::Planned
                | PlanItemStatus::Approved
                | PlanItemStatus::Validating
                | PlanItemStatus::Executing
                | PlanItemStatus::NeedsReconciliation => &mut summary.returned,
            };
            *slot += 1;
        }
        summary
    }
}

/// The count "Move N files to Trash" confirms: the plan's `Planned` items.
/// Blocked items are excluded (ERR-004).
#[must_use]
pub fn confirmed_count(plan: &CleanupPlan) -> usize {
    plan.items
        .iter()
        .filter(|i| i.status == PlanItemStatus::Planned)
        .count()
}

/// The cleanup service, owned by the dispatcher thread.
#[derive(Debug)]
pub struct CleanupService {
    runner: run::Runner,
}

impl CleanupService {
    /// Starts the service's worker thread.
    #[must_use]
    pub fn new(context: &ModuleContext) -> Self {
        Self {
            runner: run::Runner::new(context),
        }
    }

    /// Builds plan `plan` from `validation`: its view's scan and queue, as the
    /// engine checked and published them when the request was submitted.
    /// `PlanBuilt` follows from the worker. Targets the simulation already
    /// moved in this scan are left out: they no longer exist in the
    /// simulated filesystem, so a plan built from the unchanged queue cannot
    /// replay them.
    ///
    /// # Errors
    ///
    /// None; the `Result` is the C-02 shape.
    #[expect(
        clippy::needless_pass_by_value,
        reason = "C-02 shape: the dispatcher hands the published validation over by value"
    )]
    pub fn build(
        &mut self,
        plan: PlanRevision,
        validation: PublishedValidation,
    ) -> Result<(), NotImplemented> {
        self.runner.build(plan, &validation);
        Ok(())
    }

    /// Executes plan `plan`, at most once per revision, and returns at once;
    /// the run proceeds on the worker. Any other execution is answered with
    /// `ExecutionRefused`.
    ///
    /// # Errors
    ///
    /// None; the `Result` is the C-02 shape.
    pub fn execute(&mut self, plan: PlanRevision) -> Result<(), NotImplemented> {
        self.runner.execute(plan);
        Ok(())
    }

    /// Stops the run of `plan` before its next item, or withdraws `plan` if
    /// it has not run; either way its authorization ends.
    ///
    /// # Errors
    ///
    /// None; the `Result` is the C-02 shape.
    pub fn stop(&mut self, plan: PlanRevision) -> Result<(), NotImplemented> {
        self.runner.stop(plan);
        Ok(())
    }
}
