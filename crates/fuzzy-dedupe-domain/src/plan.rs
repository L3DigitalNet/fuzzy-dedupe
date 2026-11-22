// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! Cleanup plan items and their states (spec §10.4). Plans refer to files by
//! ID only; a rendered path is never the authority for an action (§11.8).

use crate::ids::{FileId, PlanRevision, ScanId};

/// The §10.4 item states. There is deliberately no `Skipped`: cleanup never
/// passes over a changed file silently.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PlanItemStatus {
    /// In the plan, not yet confirmed.
    Planned,
    /// Cannot run, found when the plan was built; excluded from the
    /// confirmed count.
    Blocked,
    /// Confirmed and waiting for its turn.
    Approved,
    /// Re-checked just before the move.
    Validating,
    /// Moving.
    Executing,
    /// Moved to the system Trash.
    Succeeded,
    /// The Trash move failed; the run stopped after it.
    Failed,
    /// The file, its counterpart or its evidence changed; the run stopped
    /// before it and it left the plan.
    Stopped,
    /// The journal and the (simulated) filesystem disagree.
    NeedsReconciliation,
}

/// Why an item is blocked, stopped, failed or uncertain. Each such item is
/// named with its reason (spec §11.6).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PlanItemReason {
    /// ERR-004: the target's volume has no usable Trash.
    NoTrash,
    /// ERR-001: the target no longer matches its approved snapshot.
    TargetChanged,
    /// ERR-001: the kept counterpart no longer matches its snapshot.
    CounterpartChanged,
    /// The required review evidence changed.
    EvidenceChanged,
    /// ERR-003: the Trash move failed.
    TrashMoveFailed,
    /// ERR-002: the outcome after an interruption is uncertain.
    OutcomeUncertain,
}

/// One target in a cleanup plan.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PlanItem {
    pub target: FileId,
    /// The kept copy shown in the Kept copy column.
    pub counterpart: FileId,
    pub size_bytes: u64,
    pub status: PlanItemStatus,
    /// Present for `Blocked`, `Stopped`, `Failed` and `NeedsReconciliation`.
    pub reason: Option<PlanItemReason>,
}

/// An engine-issued, immutable cleanup plan. A revised plan after a stop is
/// a new value with a new revision.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CleanupPlan {
    pub revision: PlanRevision,
    pub scan: ScanId,
    pub items: Vec<PlanItem>,
}
