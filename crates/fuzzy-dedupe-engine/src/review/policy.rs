// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! The C-05 policy state machine behind [`super::ReviewService`]: one scan's
//! queue, its scan-scoped decisions and the review-time roles (reference and
//! counterparts) that the immutable [`ScanResults`] cannot carry.
//!
//! **One validity rule for every decision (C-05 I-7).** Kept, per-file
//! Protect, unique-text acknowledgments and queue approvals all store the
//! [`DecisionBinding`] they were made under, and a decision is valid exactly
//! while that binding equals [`ReviewState::binding`] for its target now. No
//! command edits a decision because one of its components changed; each
//! command changes only roles or evidence and then runs
//! [`ReviewState::reconcile`], which drops every stale decision and reports
//! it. That one comparison covers a target or counterpart snapshot change, a
//! rules change, and a counterpart change through Make this the reference or
//! a Compare review, so no binding component can be forgotten by one path.
//! Rejected: invalidating per command ("a snapshot change clears X and Y"),
//! which is how a decision keyed by target alone survives a counterpart
//! change.
//!
//! **Locality.** A binding depends only on its target's group (reference,
//! Compare choices, edges), on two members' snapshots and on the rules
//! revision, so a command that touches one group reconciles only that group.
//! Only a rules change and `ClearForNewScan` reach every decided group.
//!
//! **FR-025 is checked, not repaired.** A queue request is refused while its
//! counterpart is queued or while a queued file names it as counterpart, so
//! every queued target's recorded counterpart stays unqueued (I-3). Nothing
//! later can break that without unqueueing the target: a counterpart change
//! changes the target's binding, which reconcile then drops (I-7).
//!
//! **Settled interpretations** (owner review of T7/T31, 2026-11-23), each
//! with the spec text it reads:
//!
//! 1. *A reference's binding names itself as counterpart.* §6 gives the
//!    reference no counterpart, yet Kept and per-file Protect apply to it.
//!    Binding it to itself means that when Make this the reference turns it
//!    into a member (or a member into it), "a change to any one, including a
//!    different counterpart file, invalidates that decision" (C-05 I-7).
//! 2. *An explicit queue request on a Kept row replaces the Kept.* §6 Kept:
//!    "Bulk actions skip kept files, and the user can change the decision";
//!    no refusal label exists for Kept, so only bulk skips it.
//! 3. *A Compare review sets the counterpart* (§10.1: "Reviewing the row in
//!    Compare against another member that stays makes that member its
//!    counterpart"); see `reviewed_decision`.
//! 4. *Stale requests are refused, not re-aimed.* Without evidence a request
//!    must name the row's current counterpart, and evidence must describe
//!    the current snapshots and rules (§11.5 "never show stale results as
//!    current"); both refuse as `EvidenceMismatch`. See `usable_counterpart`.
//! 5. *Queue identical copies is conservative per member* (§11.4: "It queues
//!    only files whose bytes match their reference exactly"); the rule is
//!    the exported forecast in `forecast.rs`, see `queue_identical`.
//! 6. *"Rows that need it show Review first"* (§11.4) until a valid
//!    acknowledgment exists; see `row`. A row needs it exactly when its
//!    match against its current counterpart is not Same bytes and its
//!    Unique count there is above 0 or unknown (owner decision of
//!    2026-11-23): a Similar or containment row with Unique 0 loses no line
//!    when removed, so it shows Unreviewed and queues directly. See
//!    `needs_review` for where the count comes from and when it is unknown.
//! 7. *An already-queued file re-aimed from Compare* runs every check and
//!    replaces its approval; repeating the same counterpart is a no-op.
//!    A re-aim that is not refused commits the new counterpart even when the
//!    request then answers `NeedsReview` (PV-T31-003): the user reviewed the
//!    pair, so §10.1 makes the member the counterpart, and the decisions
//!    bound to the old one are invalidated and listed.
//! 8. *Only decisions that go stale are listed as invalidated* (I-10,
//!    PQ-008). Removals the user asks for (Unqueue, Kept, Protect, Keep both,
//!    a queue request replacing Kept) are not; Make this the reference's
//!    removal of the new reference is, because the approval ended by the
//!    role change rather than by the user's removal of it.
//! 9. *Fresh comparison evidence refreshes an edge-less Compare count.* A
//!    Compare counterpart with no direct edge has only the comparison's
//!    Unique count, which describes the snapshots and rules it was measured
//!    under. Accepted evidence for the same pair replaces it, and once any
//!    of those components changes the count is unknown, not reused.

use std::collections::{BTreeMap, BTreeSet};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use fuzzy_dedupe_domain::{
    CounterpartBasis, CounterpartLink, DecisionBinding, DecisionKind, EvidenceUpdate, FileId,
    Group, GroupId, InvalidatedDecision, Measurement, MemberDecision, MemberRole, QueueOrigin,
    QueueScope, QueuedTarget, Refusal, ReviewedPair, RulesRevision, ScanId, ScanResults,
    SnapshotId, ValidationView, rule_counterpart,
};

use super::forecast::{self, ForecastFacts};
use super::{FileReviewState, ReviewCommand, ReviewOutcome, ReviewStatus};

/// Lookup tables derived once per loaded scan.
#[derive(Debug)]
struct Catalog {
    results: Arc<ScanResults>,
    /// Each file's group index in `results.groups`.
    group_of: BTreeMap<FileId, usize>,
    groups_by_id: BTreeMap<GroupId, usize>,
    /// Folder protection from the scan setup. Never a decision: no command
    /// and no binding change removes a file from this set (C-05 I-9).
    folder_protected: BTreeSet<FileId>,
}

/// The loaded scan, compared by identity. Two states built from the same
/// `load_scan` share the `Arc`; states from different loads never compare
/// equal even when the results are equal, which is the safe direction for
/// the PV-T7-001 enumeration's memo (it may fail to merge two states, never
/// merges two that differ) and avoids hashing 100,000 members per state.
#[derive(Clone, Debug)]
struct Shared(Arc<Catalog>);

impl PartialEq for Shared {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for Shared {}

impl Hash for Shared {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::ptr::hash(Arc::as_ptr(&self.0), state);
    }
}

/// A queued target's approval. The origin is recorded and never consulted
/// (spec §11.4: every entry point gets the same policy); `None` is Queue
/// identical copies, which has no `QueueOrigin`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Approval {
    binding: DecisionBinding,
    origin: Option<QueueOrigin>,
}

/// A counterpart chosen by reviewing in Compare (§10.1). `measured` is the
/// binding of the comparison that supplied `link.unique_lines`, set only
/// when the pair has no direct edge; with an edge the count is the edge's
/// own and `measured` is `None`. See [`ReviewState::unique_now`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Chosen {
    link: CounterpartLink,
    measured: Option<DecisionBinding>,
}

/// One scan's review state. Every map is keyed by file ID, never by row, so
/// decisions follow their files through any sort or filter (FR-017).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct ReviewState {
    catalog: Shared,
    rules: RulesRevision,
    /// Current snapshots, shared with every published view; copied on write
    /// only when a fixture-injected change arrives while a view holds them.
    snapshots: Arc<BTreeMap<FileId, SnapshotId>>,
    /// The current reference of each group, by group index.
    references: Vec<FileId>,
    /// Counterparts chosen by reviewing in Compare (§10.1); every other
    /// member follows `rule_counterpart`. Make this the reference clears its
    /// group's entries.
    chosen: BTreeMap<FileId, Chosen>,
    kept: BTreeMap<FileId, DecisionBinding>,
    protect: BTreeMap<FileId, DecisionBinding>,
    acks: BTreeMap<FileId, DecisionBinding>,
    queue: BTreeMap<FileId, Approval>,
}

impl ReviewState {
    pub(super) fn load(results: Arc<ScanResults>) -> Self {
        let mut group_of = BTreeMap::new();
        let mut groups_by_id = BTreeMap::new();
        let mut folder_protected = BTreeSet::new();
        let mut snapshots = BTreeMap::new();
        let mut references = Vec::with_capacity(results.groups.len());
        let mut chosen = BTreeMap::new();
        for (index, group) in results.groups.iter().enumerate() {
            groups_by_id.insert(group.id, index);
            // A valid group lists its reference first (C-01); `members[0]`
            // is the fallback that keeps a malformed group from panicking.
            let reference = group
                .reference()
                .or_else(|| group.members.first())
                .map(|m| m.file);
            references.push(reference.unwrap_or(FileId::new(0)));
            for m in &group.members {
                group_of.insert(m.file, index);
                snapshots.insert(m.file, m.snapshot);
                if m.folder_protected {
                    folder_protected.insert(m.file);
                }
                if let Some(link) = m
                    .link()
                    .filter(|l| l.basis == CounterpartBasis::ReviewedInCompare)
                {
                    // No comparison binding comes with a loaded choice, so
                    // an edge-less one starts with an unknown count.
                    chosen.insert(
                        m.file,
                        Chosen {
                            link: *link,
                            measured: None,
                        },
                    );
                }
            }
        }
        Self {
            rules: results.rules.revision,
            catalog: Shared(Arc::new(Catalog {
                results,
                group_of,
                groups_by_id,
                folder_protected,
            })),
            snapshots: Arc::new(snapshots),
            references,
            chosen,
            kept: BTreeMap::new(),
            protect: BTreeMap::new(),
            acks: BTreeMap::new(),
            queue: BTreeMap::new(),
        }
    }

    /// The state cleanup revalidates against. Every queue entry's binding
    /// is current by construction (reconcile ran after the last change), so
    /// `ValidationView::binding_is_current` holds for each one until the
    /// next change.
    pub(super) fn view(&self) -> ValidationView {
        let mut protected = self.catalog.0.folder_protected.clone();
        protected.extend(self.protect.keys().copied());
        ValidationView {
            scan: self.scan(),
            queue: self
                .queue
                .iter()
                .map(|(&target, a)| QueuedTarget {
                    target,
                    counterpart: a.binding.counterpart,
                    binding: a.binding,
                })
                .collect(),
            protected,
            snapshots: Arc::clone(&self.snapshots),
            rules: self.rules,
        }
    }

    pub(super) fn apply(
        &mut self,
        command: ReviewCommand,
        reviewed: Option<ReviewedPair>,
    ) -> ReviewOutcome {
        match command {
            ReviewCommand::RequestQueue {
                file,
                counterpart,
                origin,
                ..
            } => self.request_queue(file, counterpart, origin, reviewed),
            ReviewCommand::AcknowledgeUniqueText {
                file, counterpart, ..
            } => self.reviewed_decision(
                file,
                counterpart,
                reviewed,
                false,
                DecisionKind::Acknowledgment,
            ),
            ReviewCommand::KeepBoth {
                file, counterpart, ..
            } => self.reviewed_decision(file, counterpart, reviewed, true, DecisionKind::Kept),
            ReviewCommand::RemoveFromQueue { file } => self.edit(file, |s| {
                s.queue.remove(&file);
            }),
            ReviewCommand::MarkKept { file, kept } => self.edit(file, |s| {
                if kept {
                    // Kept is the decision not to queue, so it replaces a
                    // queue entry; the user removed it, nothing invalidated.
                    s.queue.remove(&file);
                    let binding = s.binding(file);
                    s.kept.insert(file, binding);
                } else {
                    s.kept.remove(&file);
                }
            }),
            ReviewCommand::ProtectFile { file, protected } => self.edit(file, |s| {
                if protected {
                    // I-2: a protected file is never queued.
                    s.queue.remove(&file);
                    let binding = s.binding(file);
                    s.protect.insert(file, binding);
                } else {
                    s.protect.remove(&file);
                }
            }),
            ReviewCommand::MakeReference { file } => self.make_reference(file),
            ReviewCommand::QueueIdenticalCopies { scope } => self.queue_identical(&scope),
            ReviewCommand::ClearForNewScan => self.clear(),
            ReviewCommand::UpdateEvidence(update) => self.update_evidence(update),
        }
    }

    // ── Commands ─────────────────────────────────────────────────────────

    /// Every queue entry point (checkbox, row menu, Space, Compare, Review
    /// unique text) lands here; `origin` is recorded, never used to relax a
    /// rule (spec §11.4, UI check 09).
    ///
    /// Check order: unknown file, reference, protected, the counterpart the
    /// request may use, FR-025 both ways; then, committed, the Compare
    /// choice, already queued (a no-op), review-first and the queue entry.
    /// Every refusal returns before the first mutation. `NeedsReview`
    /// returns after the Compare choice is committed and reconciled, so a
    /// re-aim it interrupts still moves the counterpart and lists what it
    /// invalidated (settled interpretation 7); without a Compare choice it
    /// changes nothing.
    fn request_queue(
        &mut self,
        file: FileId,
        counterpart: FileId,
        origin: QueueOrigin,
        reviewed: Option<ReviewedPair>,
    ) -> ReviewOutcome {
        let Some(group) = self.group_index(file) else {
            return ReviewOutcome::refused(file, Refusal::UnknownFile);
        };
        if self.references[group] == file {
            return ReviewOutcome::refused(file, Refusal::Reference);
        }
        if self.is_protected(file) {
            return ReviewOutcome::refused(file, Refusal::Protected);
        }
        let counterpart = match self.usable_counterpart(
            group,
            file,
            counterpart,
            reviewed.as_ref(),
            origin == QueueOrigin::Compare,
        ) {
            Ok(c) => c,
            Err(refusal) => return ReviewOutcome::refused(file, refusal),
        };
        // A target already queued against `counterpart` passes both FR-025
        // checks (I-3 keeps that counterpart unqueued, and no queued file
        // names a queued one), so running them before the no-op below
        // refuses nothing the no-op would have accepted.
        if self.queue.contains_key(&counterpart) {
            return ReviewOutcome::refused(file, Refusal::CounterpartQueued);
        }
        if self.is_counterpart_of_queued(group, file) {
            return ReviewOutcome::refused(file, Refusal::CounterpartOfQueuedFile);
        }
        self.commit(&BTreeSet::from([group]), |s, invalidated| {
            s.choose_counterpart(group, file, counterpart, reviewed.as_ref());
            // ORDER: reconcile the counterpart change before any explicit
            // edit. Decisions bound to the old counterpart (a Kept, the old
            // approval of a re-aimed target) must be listed as invalidated
            // (I-10); removing the Kept first would erase it unreported.
            // Only a Kept that survived, bound to the unchanged counterpart,
            // is the user's own replacement and goes unlisted. Reconciling
            // before the insert also means the new approval is never
            // reconciled away.
            s.reconcile_group(group, invalidated);
            // Still queued after reconciling: the approval it holds is the
            // one this request would record, so nothing more changes. A
            // re-aim or a refreshed count that made the old approval stale
            // was dropped above and falls through to be decided afresh.
            if s.queue.contains_key(&file) {
                return ReviewStatus::Applied;
            }
            // I-5: the acknowledgment must be bound to exactly the binding
            // the approval will hold, so a stale one (other counterpart,
            // other snapshot, other rules) cannot authorise this request.
            let binding = s.binding(file);
            if s.needs_review(group, file) && s.acks.get(&file) != Some(&binding) {
                return ReviewStatus::NeedsReview { file, counterpart };
            }
            s.kept.remove(&file);
            let binding = s.binding(file);
            s.queue.insert(
                file,
                Approval {
                    binding,
                    origin: Some(origin),
                },
            );
            ReviewStatus::Queued { files: 1 }
        })
    }

    /// Review unique text's acknowledgment (`kind` Acknowledgment) and
    /// Compare's Keep both (`kind` Kept, `required` evidence).
    ///
    /// Reviewing a pair in Compare makes the reviewed member the row's
    /// counterpart (§10.1: "Reviewing the row in Compare against another
    /// member that stays makes that member its counterpart"), which
    /// invalidates whatever was bound to the old one.
    ///
    /// Keep both "records a no-removal (Kept) decision for the displayed
    /// pair and clears pending removal intent for that pair" and "must not
    /// silently change unrelated members" (§11.5): both displayed files are
    /// unqueued and get Kept, in one commit, and no other row changes. A
    /// pair may contain the reference (Compare shows it as A); the
    /// reference's own Kept then binds to itself like any reference
    /// decision, and its row gets no Compare counterpart, because a
    /// reference has none. An acknowledgment on the reference is refused:
    /// the reference is never queued, so there is nothing to acknowledge.
    fn reviewed_decision(
        &mut self,
        file: FileId,
        counterpart: FileId,
        reviewed: Option<ReviewedPair>,
        required: bool,
        kind: DecisionKind,
    ) -> ReviewOutcome {
        let Some(group) = self.group_index(file) else {
            return ReviewOutcome::refused(file, Refusal::UnknownFile);
        };
        let is_reference = self.references[group] == file;
        if is_reference && kind == DecisionKind::Acknowledgment {
            return ReviewOutcome::refused(file, Refusal::Reference);
        }
        let counterpart =
            match self.usable_counterpart(group, file, counterpart, reviewed.as_ref(), required) {
                Ok(c) => c,
                Err(refusal) => return ReviewOutcome::refused(file, refusal),
            };
        self.commit(&BTreeSet::from([group]), |s, invalidated| {
            let moved =
                !is_reference && s.choose_counterpart(group, file, counterpart, reviewed.as_ref());
            if kind == DecisionKind::Acknowledgment && !moved {
                // ORDER: the counterpart is unchanged, so the acknowledgment
                // lands before reconciling. A refreshed count that now needs
                // review must find it, or reconcile would drop the very
                // approval this acknowledgment covers. Nothing is lost
                // unlisted: an older acknowledgment for this row is either
                // stale, and already dropped by the previous command, or
                // bound exactly as this one is.
                let binding = s.binding(file);
                s.acks.insert(file, binding);
                return ReviewStatus::Applied;
            }
            // ORDER: as in `request_queue`, stale decisions are listed
            // before the explicit edits below replace anything.
            s.reconcile_group(group, invalidated);
            if kind == DecisionKind::Kept {
                for f in [file, counterpart] {
                    s.queue.remove(&f);
                    let binding = s.binding(f);
                    s.kept.insert(f, binding);
                }
            } else {
                let binding = s.binding(file);
                s.acks.insert(file, binding);
            }
            ReviewStatus::Applied
        })
    }

    /// A plain decision edit on one file's group.
    fn edit(&mut self, file: FileId, change: impl FnOnce(&mut Self)) -> ReviewOutcome {
        let Some(group) = self.group_index(file) else {
            return ReviewOutcome::refused(file, Refusal::UnknownFile);
        };
        self.commit(&BTreeSet::from([group]), |s, _| {
            change(s);
            ReviewStatus::Applied
        })
    }

    /// Make this the reference, in the C-05 order: (1) take the new
    /// reference out of the queue; (2) recompute every counterpart by the
    /// §10.1 rule, discarding Compare choices; (3) unqueue each other queued
    /// member whose counterpart changed and clear its acknowledgment, which
    /// reconcile does because the counterpart is part of every binding;
    /// (4) report how many others left the queue.
    ///
    /// On the current reference this is a no-op that keeps Compare choices:
    /// the row menu does not offer the item on the reference row (§11.4), so
    /// only a stale command can arrive, and discarding choices for it would
    /// change rows the user did not touch.
    fn make_reference(&mut self, file: FileId) -> ReviewOutcome {
        let Some(group) = self.group_index(file) else {
            return ReviewOutcome::refused(file, Refusal::UnknownFile);
        };
        if self.references[group] == file {
            return Self::unchanged(ReviewStatus::Applied);
        }
        self.commit(&BTreeSet::from([group]), |s, invalidated| {
            if s.queue.remove(&file).is_some() {
                invalidated.push(InvalidatedDecision {
                    file,
                    kind: DecisionKind::QueueApproval,
                });
            }
            s.references[group] = file;
            let members: Vec<FileId> = s.group(group).members.iter().map(|m| m.file).collect();
            for m in &members {
                s.chosen.remove(m);
            }
            let was_queued: Vec<FileId> = members
                .iter()
                .copied()
                .filter(|m| s.queue.contains_key(m))
                .collect();
            s.reconcile_group(group, invalidated);
            let unqueued = was_queued
                .iter()
                .filter(|m| !s.queue.contains_key(m))
                .count();
            ReviewStatus::ReferenceChanged { unqueued }
        })
    }

    /// Queue identical copies (spec §11.4). The eligibility rule is
    /// [`forecast::queue_identical`], evaluated on the state before the
    /// command; exactly the files it returns are queued, each against its
    /// reference, and nothing else changes. The popup states its count and
    /// size from the same function over the presentation read model, so
    /// the two cannot diverge. Skips are silent.
    ///
    /// Rejected: the rule inline here with a copy in the popup. The copy
    /// could not see a Compare re-aim and over-counted by one (plan T33).
    fn queue_identical(&mut self, scope: &QueueScope) -> ReviewOutcome {
        let catalog = Arc::clone(&self.catalog.0);
        let take = forecast::queue_identical(&catalog.results, scope, &*self).files;
        let groups: BTreeSet<usize> = forecast::scope_groups(&catalog.results, scope)
            .map(|(index, _)| index)
            .collect();
        self.commit(&groups, |s, _| {
            for &file in &take {
                let binding = s.binding(file);
                s.queue.insert(
                    file,
                    Approval {
                        binding,
                        origin: None,
                    },
                );
            }
            ReviewStatus::Queued { files: take.len() }
        })
    }

    /// Removes the queue and every decision of every kind, reporting each
    /// (I-9, I-10). Folder protection lives in the catalog and is untouched;
    /// references and Compare choices are roles, not decisions, and the new
    /// scan's `load_scan` replaces them.
    fn clear(&mut self) -> ReviewOutcome {
        let groups = self.decided_groups();
        self.commit(&groups, |s, invalidated| {
            let mut drop_all = |files: Vec<FileId>, kind| {
                invalidated.extend(
                    files
                        .into_iter()
                        .map(|file| InvalidatedDecision { file, kind }),
                );
            };
            drop_all(s.kept.keys().copied().collect(), DecisionKind::Kept);
            drop_all(s.protect.keys().copied().collect(), DecisionKind::Protect);
            drop_all(
                s.acks.keys().copied().collect(),
                DecisionKind::Acknowledgment,
            );
            drop_all(
                s.queue.keys().copied().collect(),
                DecisionKind::QueueApproval,
            );
            s.kept.clear();
            s.protect.clear();
            s.acks.clear();
            s.queue.clear();
            ReviewStatus::Applied
        })
    }

    /// A fixture-injected binding-component change. Repeating the current
    /// value is a no-op, so it can never invalidate anything.
    fn update_evidence(&mut self, update: EvidenceUpdate) -> ReviewOutcome {
        match update {
            EvidenceUpdate::FileSnapshot { file, snapshot } => {
                let Some(group) = self.group_index(file) else {
                    return ReviewOutcome::refused(file, Refusal::UnknownFile);
                };
                if self.snapshots.get(&file) == Some(&snapshot) {
                    return Self::unchanged(ReviewStatus::Applied);
                }
                // A file's snapshot appears only in bindings of its own
                // group (as target or as counterpart), so one group suffices.
                self.commit(&BTreeSet::from([group]), |s, _| {
                    Arc::make_mut(&mut s.snapshots).insert(file, snapshot);
                    ReviewStatus::Applied
                })
            }
            EvidenceUpdate::Rules(rules) => {
                if self.rules == rules {
                    return Self::unchanged(ReviewStatus::Applied);
                }
                // Undecided groups count too when they hold a Compare
                // choice: its measured count goes stale with the rules,
                // which can turn a row to Review first.
                let mut groups = self.decided_groups();
                groups.extend(self.chosen.keys().filter_map(|f| self.group_index(*f)));
                self.commit(&groups, |s, _| {
                    s.rules = rules;
                    ReviewStatus::Applied
                })
            }
        }
    }

    // ── Transaction and reconciliation ───────────────────────────────────

    /// Runs `change` on `groups`, reconciles them, and reports every row of
    /// those groups whose role or decision changed. `groups` must cover
    /// every group `change` can affect; rows elsewhere are not compared.
    fn commit(
        &mut self,
        groups: &BTreeSet<usize>,
        change: impl FnOnce(&mut Self, &mut Vec<InvalidatedDecision>) -> ReviewStatus,
    ) -> ReviewOutcome {
        let before: BTreeMap<FileId, FileReviewState> = self.rows(groups).collect();
        let mut invalidated = Vec::new();
        let status = change(self, &mut invalidated);
        for &group in groups {
            self.reconcile_group(group, &mut invalidated);
        }
        let changed = self
            .rows(groups)
            .filter(|(file, row)| before.get(file) != Some(row))
            .map(|(_, row)| row)
            .collect();
        invalidated.sort_by_key(|d| (d.file, d.kind));
        invalidated.dedup();
        ReviewOutcome {
            status,
            changed,
            invalidated,
        }
    }

    fn unchanged(status: ReviewStatus) -> ReviewOutcome {
        ReviewOutcome {
            status,
            changed: Vec::new(),
            invalidated: Vec::new(),
        }
    }

    /// Drops every decision of `group` whose binding is no longer current
    /// (I-7), and every queue entry whose approval is stale or whose
    /// required acknowledgment is gone, listing each (I-10).
    fn reconcile_group(&mut self, group: usize, invalidated: &mut Vec<InvalidatedDecision>) {
        let catalog = Arc::clone(&self.catalog.0);
        for m in &catalog.results.groups[group].members {
            let file = m.file;
            let current = self.binding(file);
            let mut drop_stale = |map: &mut BTreeMap<FileId, DecisionBinding>, kind| {
                if map.get(&file).is_some_and(|b| *b != current) {
                    map.remove(&file);
                    invalidated.push(InvalidatedDecision { file, kind });
                }
            };
            drop_stale(&mut self.kept, DecisionKind::Kept);
            drop_stale(&mut self.protect, DecisionKind::Protect);
            drop_stale(&mut self.acks, DecisionKind::Acknowledgment);
            if let Some(approval) = self.queue.get(&file) {
                let reviewed_ok =
                    !self.needs_review(group, file) || self.acks.get(&file) == Some(&current);
                if approval.binding != current || !reviewed_ok {
                    self.queue.remove(&file);
                    invalidated.push(InvalidatedDecision {
                        file,
                        kind: DecisionKind::QueueApproval,
                    });
                }
            }
        }
    }

    // ── Queries ──────────────────────────────────────────────────────────

    fn scan(&self) -> ScanId {
        self.catalog.0.results.scan
    }

    fn group(&self, group: usize) -> &Group {
        &self.catalog.0.results.groups[group]
    }

    fn group_index(&self, file: FileId) -> Option<usize> {
        self.catalog.0.group_of.get(&file).copied()
    }

    /// The groups holding any decision: everything a rules change or a
    /// clear can reach.
    fn decided_groups(&self) -> BTreeSet<usize> {
        self.kept
            .keys()
            .chain(self.protect.keys())
            .chain(self.acks.keys())
            .chain(self.queue.keys())
            .filter_map(|f| self.group_index(*f))
            .collect()
    }

    /// The row's current counterpart: `None` for the reference.
    fn counterpart(&self, group: usize, file: FileId) -> Option<FileId> {
        self.link(group, file).map(|l| l.counterpart)
    }

    fn link(&self, group: usize, file: FileId) -> Option<CounterpartLink> {
        let reference = self.references[group];
        if file == reference {
            return None;
        }
        if let Some(chosen) = self.chosen.get(&file) {
            return Some(chosen.link);
        }
        let g = self.group(group);
        let counterpart = rule_counterpart(&g.edges, reference, file)?;
        let edge = g.edge_between(file, counterpart)?;
        Some(CounterpartLink {
            counterpart,
            basis: CounterpartBasis::Rule,
            measurement: edge.measurement_from(file),
            unique_lines: edge.unique_lines_of(file).unwrap_or(0),
        })
    }

    /// The binding a decision on `file` made now would record, and the one
    /// every stored decision on `file` is compared with.
    fn binding(&self, file: FileId) -> DecisionBinding {
        let counterpart = self
            .group_index(file)
            .and_then(|g| self.counterpart(g, file))
            .unwrap_or(file);
        self.binding_against(file, counterpart)
    }

    fn binding_against(&self, file: FileId, counterpart: FileId) -> DecisionBinding {
        let snapshot = |f| {
            self.snapshots
                .get(&f)
                .copied()
                .unwrap_or(SnapshotId::new(0))
        };
        DecisionBinding {
            scan: self.scan(),
            target: file,
            target_snapshot: snapshot(file),
            counterpart,
            counterpart_snapshot: snapshot(counterpart),
            rules: self.rules,
        }
    }

    /// I-5 under the owner decision of 2026-11-23: a member row needs
    /// Review unique text first unless its match with its current
    /// counterpart is Same bytes or its Unique count there is known to be 0.
    /// An unknown count needs review, the safe direction: removing the row
    /// could lose lines nobody has seen.
    fn needs_review(&self, group: usize, file: FileId) -> bool {
        let Some(link) = self.link(group, file) else {
            return false;
        };
        let identical = self
            .group(group)
            .edge_between(file, link.counterpart)
            .is_some_and(|e| matches!(e.measurement, Measurement::Identical(_)));
        !identical && self.unique_now(group, file, link) != Some(0)
    }

    /// The row's Unique count against `link`'s counterpart, if known. With
    /// a direct edge it is the edge's own value, as for every rule link.
    /// Without one (a Compare choice of an unmeasured member) it is the
    /// reviewed comparison's count, valid only while that comparison's
    /// binding is the pair's current one: a snapshot or rules change makes
    /// it unknown until fresh evidence for the pair is accepted.
    fn unique_now(&self, group: usize, file: FileId, link: CounterpartLink) -> Option<u64> {
        if self
            .group(group)
            .edge_between(file, link.counterpart)
            .is_some()
        {
            return Some(link.unique_lines);
        }
        let measured = self.chosen.get(&file)?.measured?;
        (measured == self.binding_against(file, link.counterpart)).then_some(link.unique_lines)
    }

    /// I-2: folder-protected, or holding a per-file Protect, which is valid
    /// whenever it is stored (reconcile drops stale ones).
    fn is_protected(&self, file: FileId) -> bool {
        self.catalog.0.folder_protected.contains(&file) || self.protect.contains_key(&file)
    }

    /// FR-025 "Counterpart of a queued file": some queued member of the
    /// group was measured against `file`. Counterparts never cross groups,
    /// so the group's members are the only candidates.
    fn is_counterpart_of_queued(&self, group: usize, file: FileId) -> bool {
        self.group(group).members.iter().any(|m| {
            self.queue
                .get(&m.file)
                .is_some_and(|a| a.binding.counterpart == file)
        })
    }

    /// The counterpart a decision on `file` may bind to. With comparison
    /// evidence it is the reviewed member, provided the evidence describes
    /// the current observation of the pair; a comparison computed on an
    /// older snapshot or rules revision is not evidence for the files as
    /// they are now. Without evidence it must be the row's current
    /// counterpart, so a stale request from presentation is refused rather
    /// than silently re-aimed.
    fn usable_counterpart(
        &self,
        group: usize,
        file: FileId,
        counterpart: FileId,
        reviewed: Option<&ReviewedPair>,
        required: bool,
    ) -> Result<FileId, Refusal> {
        match reviewed {
            Some(pair) => {
                if pair.scan != self.scan() {
                    return Err(Refusal::EvidenceUnavailable);
                }
                // The engine already matched the pair to the command; this
                // repeats it so the service never trusts a caller-built pair
                // for a different row, and adds the currency check.
                let same_group =
                    counterpart != file && self.group_index(counterpart) == Some(group);
                if !same_group
                    || (pair.target, pair.counterpart) != (file, counterpart)
                    || pair.binding() != self.binding_against(file, counterpart)
                {
                    return Err(Refusal::EvidenceMismatch);
                }
                Ok(counterpart)
            }
            None if required => Err(Refusal::EvidenceUnavailable),
            None if self.counterpart(group, file) == Some(counterpart) => Ok(counterpart),
            None => Err(Refusal::EvidenceMismatch),
        }
    }

    /// Records a Compare review of `file` against `counterpart` and
    /// returns whether the counterpart changed. Callers reconcile
    /// afterwards.
    ///
    /// A new counterpart gets a `ReviewedInCompare` link. Reviewing the
    /// current one again keeps the link, but when the pair has no direct
    /// edge the accepted evidence replaces its Unique count and measured
    /// binding (settled interpretation 9). Rejected: returning early on an
    /// unchanged counterpart, which kept a count measured on older
    /// snapshots after fresh evidence said otherwise.
    fn choose_counterpart(
        &mut self,
        group: usize,
        file: FileId,
        counterpart: FileId,
        reviewed: Option<&ReviewedPair>,
    ) -> bool {
        let edge = self.group(group).edge_between(file, counterpart).copied();
        // `usable_counterpart` accepted `reviewed` only if it describes the
        // pair now, so its binding is the pair's current one.
        let measured = if edge.is_none() {
            reviewed.map(ReviewedPair::binding)
        } else {
            None
        };
        if self.counterpart(group, file) == Some(counterpart) {
            if let (Some(pair), Some(chosen)) = (reviewed, self.chosen.get_mut(&file))
                && edge.is_none()
            {
                chosen.link.unique_lines = pair.unique_lines;
                chosen.measured = measured;
            }
            return false;
        }
        // No direct edge means no score to show (§10.1); the Unique count
        // then comes from the comparison the user reviewed. Without
        // evidence (unreachable: a request without it must name the current
        // counterpart) the count stays unknown through `measured`.
        let link = CounterpartLink {
            counterpart,
            basis: CounterpartBasis::ReviewedInCompare,
            measurement: edge.and_then(|e| e.measurement_from(file)),
            unique_lines: edge
                .and_then(|e| e.unique_lines_of(file))
                .or_else(|| reviewed.map(|p| p.unique_lines))
                .unwrap_or(0),
        };
        self.chosen.insert(file, Chosen { link, measured });
        true
    }

    fn rows<'a>(
        &'a self,
        groups: &'a BTreeSet<usize>,
    ) -> impl Iterator<Item = (FileId, FileReviewState)> + 'a {
        groups.iter().flat_map(move |&group| {
            self.group(group)
                .members
                .iter()
                .map(move |m| (m.file, self.row(group, m.file)))
        })
    }

    fn row(&self, group: usize, file: FileId) -> FileReviewState {
        let link = self.link(group, file);
        let decision = match link {
            None => MemberDecision::Reference,
            Some(_) if self.is_protected(file) => MemberDecision::Protected,
            Some(_) if self.queue.contains_key(&file) => MemberDecision::Queued,
            Some(_) if self.kept.contains_key(&file) => MemberDecision::Kept,
            Some(l) if self.queue.contains_key(&l.counterpart) => MemberDecision::CounterpartQueued,
            // "Rows that need it show Review first" (§11.4) until a valid
            // acknowledgment exists (settled interpretation 6).
            Some(_) if self.needs_review(group, file) && !self.acks.contains_key(&file) => {
                MemberDecision::ReviewFirst
            }
            Some(_) => MemberDecision::Unreviewed,
        };
        FileReviewState {
            file,
            group: self.group(group).id,
            role: link.map_or(MemberRole::Reference, MemberRole::Member),
            decision,
        }
    }
}

/// The service's own answers to the forecast, so `queue_identical` queues
/// what the forecast computes on exactly this state. Presentation's read
/// model implements the same trait from outcomes and the published view.
impl ForecastFacts for ReviewState {
    fn reference(&self, group: &Group) -> Option<FileId> {
        let index = *self.catalog.0.groups_by_id.get(&group.id)?;
        Some(self.references[index])
    }

    fn counterpart(&self, group: &Group, file: FileId) -> Option<FileId> {
        let index = *self.catalog.0.groups_by_id.get(&group.id)?;
        self.counterpart(index, file)
    }

    // Calls the inherent `ReviewState::is_protected`: inherent methods win
    // method resolution over trait methods, so this does not recurse.
    fn is_protected(&self, file: FileId) -> bool {
        self.is_protected(file)
    }

    fn is_kept(&self, file: FileId) -> bool {
        self.kept.contains_key(&file)
    }

    fn queued_counterpart(&self, file: FileId) -> Option<FileId> {
        self.queue.get(&file).map(|a| a.binding.counterpart)
    }
}
