// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! Review vocabulary shared by the engine review service (plan C-05) and the
//! presentation read model. The policy itself lives in the engine; this
//! module only names its inputs and outcomes.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::ids::{FileId, GroupId, RulesRevision, ScanId, SnapshotId};
use crate::rules::CompareMode;

/// What a decision was valid for when it was made (spec DR-008, C-05 I-7).
/// The decision is valid only while every value equals the current one; a
/// different counterpart file counts as a change. `scan` scopes it: no
/// decision outlives its scan (FR-026).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DecisionBinding {
    pub scan: ScanId,
    pub target: FileId,
    pub target_snapshot: SnapshotId,
    pub counterpart: FileId,
    pub counterpart_snapshot: SnapshotId,
    pub rules: RulesRevision,
}

/// The scan-scoped decision kinds that bind to evidence (C-05 I-7). Folder
/// protection is absent on purpose: it belongs to the scan setup.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DecisionKind {
    Kept,
    /// A per-file Protect from the row menu.
    Protect,
    /// A unique-text acknowledgment from Review unique text.
    Acknowledgment,
    /// The approval a queued target holds.
    QueueApproval,
}

/// One decision an operation invalidated. Every review outcome lists these
/// (C-05 I-10, PQ-008), so presentation can show each row's resulting state
/// without new UI text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct InvalidatedDecision {
    pub file: FileId,
    pub kind: DecisionKind,
}

/// A member row's Decision column state (spec §11.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MemberDecision {
    Reference,
    Unreviewed,
    /// Fuzzy or normalized queueing that still needs Review unique text.
    ReviewFirst,
    Kept,
    Queued,
    Protected,
    /// Blocked by FR-025 because this row's counterpart is queued.
    CounterpartQueued,
}

/// Why a queue request was refused. A refusal never changes state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Refusal {
    /// The group's reference can never be queued (FR-025).
    Reference,
    /// Folder-protected, or holding a valid per-file Protect.
    Protected,
    /// FR-025 "Counterpart queued".
    CounterpartQueued,
    /// FR-025 "Counterpart of a queued file".
    CounterpartOfQueuedFile,
    /// Queue identical copies met a member whose bytes differ from its
    /// reference.
    NotByteIdentical,
    /// The file ID belongs to no group of the current scan, for example
    /// after a new scan replaced the results.
    UnknownFile,
    /// A Compare-originated command named no comparison the engine computed
    /// for this scan, so there is no trusted evidence for the pair.
    EvidenceUnavailable,
    /// The named comparison measured a different pair than the command.
    EvidenceMismatch,
}

/// Which control a queue request came from. Every entry point goes through
/// the same policy (spec §11.4, UI check 09); the origin is recorded, never
/// used to relax a rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum QueueOrigin {
    Checkbox,
    RowMenu,
    /// Space on the focused row.
    Shortcut,
    /// Queue B for cleanup… in Compare.
    Compare,
    /// Queue for cleanup in Review unique text.
    UniqueTextReview,
}

/// The scope of Queue identical copies (spec §11.4).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum QueueScope {
    /// Every byte-identical group.
    AllByteIdenticalGroups,
    /// Only the groups the current filter shows.
    Groups(Vec<GroupId>),
}

/// A fixture-injected evidence change (C-05). Whether a snapshot change
/// affects a decision as its target or as its counterpart depends on the
/// decision's binding, so one variant covers both.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EvidenceUpdate {
    /// `file` now has `snapshot`. Repeating the current value changes
    /// nothing.
    FileSnapshot { file: FileId, snapshot: SnapshotId },
    /// The current matching-rules revision is now this one.
    Rules(RulesRevision),
}

/// One queued cleanup target, as the review service hands it to plan
/// building. The counterpart is the kept copy the Review cleanup dialog
/// shows (spec §10.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct QueuedTarget {
    pub target: FileId,
    pub counterpart: FileId,
    pub binding: DecisionBinding,
}

/// The evidence of one pair the user reviewed in Compare: what Keep both,
/// a Compare queue request and its acknowledgment bind to when the pair may
/// have no direct-match edge (spec §10.1, §11.5).
///
/// Only the engine creates these, from the comparison service's own result
/// for a request ID (C-04), so presentation cannot supply evidence it did not
/// receive. `unique_lines` is the §10.1 measure of `target` against
/// `counterpart` in `mode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ReviewedPair {
    pub scan: ScanId,
    pub target: FileId,
    pub target_snapshot: SnapshotId,
    pub counterpart: FileId,
    pub counterpart_snapshot: SnapshotId,
    pub rules: RulesRevision,
    pub mode: CompareMode,
    pub unique_lines: u64,
}

impl ReviewedPair {
    /// The binding a decision made on this evidence records.
    #[must_use]
    pub const fn binding(&self) -> DecisionBinding {
        DecisionBinding {
            scan: self.scan,
            target: self.target,
            target_snapshot: self.target_snapshot,
            counterpart: self.counterpart,
            counterpart_snapshot: self.counterpart_snapshot,
            rules: self.rules,
        }
    }
}

/// The review state cleanup revalidates against (spec §10.4, ERR-001): an
/// immutable snapshot the review service produces after every command and
/// the engine publishes with a revision. A cleanup run reads the newest one
/// before executing and before each move, so a Protect, an unqueue or a
/// snapshot change made after the plan was built stops the run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidationView {
    pub scan: ScanId,
    /// The current queue, each target with the binding of its approval.
    pub queue: Vec<QueuedTarget>,
    /// Every file no route may queue: folder-protected or holding a valid
    /// per-file Protect.
    pub protected: BTreeSet<FileId>,
    /// Each file's current snapshot. Shared behind an `Arc` so a view per
    /// command does not copy one entry per file of a 100,000-row scan.
    pub snapshots: Arc<BTreeMap<FileId, SnapshotId>>,
    pub rules: RulesRevision,
}

impl ValidationView {
    /// Whether `binding`, the approval a plan item recorded, is still the
    /// current one: the target is still queued, its queue entry's whole
    /// binding equals `binding` (so a target re-queued against another
    /// counterpart fails even with unchanged snapshots), and the scan, the
    /// rules revision and both files' snapshots are still current. A file
    /// missing from `snapshots` counts as changed.
    #[must_use]
    pub fn binding_is_current(&self, binding: &DecisionBinding) -> bool {
        self.queued(binding.target)
            .is_some_and(|q| q.counterpart == binding.counterpart && q.binding == *binding)
            && binding.scan == self.scan
            && binding.rules == self.rules
            && self.snapshots.get(&binding.target) == Some(&binding.target_snapshot)
            && self.snapshots.get(&binding.counterpart) == Some(&binding.counterpart_snapshot)
    }

    #[must_use]
    pub fn is_protected(&self, file: FileId) -> bool {
        self.protected.contains(&file)
    }

    /// The queue entry for `target`, if it is still queued.
    #[must_use]
    pub fn queued(&self, target: FileId) -> Option<&QueuedTarget> {
        self.queue.iter().find(|q| q.target == target)
    }
}
