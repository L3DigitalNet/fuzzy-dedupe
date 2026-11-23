// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! The Queue identical copies eligibility forecast (plan T33): which files
//! the C-05 `QueueIdenticalCopies` command queues, as a pure read-only
//! function of review facts.
//!
//! One rule, two evaluators. The review service evaluates it on its own
//! pre-command state and queues exactly the files it returns
//! (`ReviewState::queue_identical` in `policy.rs`); the Queue identical
//! copies popup evaluates it over the presentation read model to state the
//! count and size beforehand. Because both call this function, the popup's
//! "Queue N files" cannot drift from what the service then queues, provided
//! the read model's [`ForecastFacts`] answer as the service's do. The rule
//! must therefore not grow a term that reads anything outside
//! [`ForecastFacts`] and the immutable results: the read model mirrors
//! nothing else.
//!
//! This extends C-05 with no new operation, outcome or state (plan T33).

use std::collections::BTreeSet;

use fuzzy_dedupe_domain::{
    FileId, Group, GroupId, Measurement, QueueScope, ScanResults, Verification,
};

/// The review facts the forecast reads, as of one moment.
///
/// Implemented by the review service's state and by presentation's queue
/// read model. Each answer must agree with the service's own wherever
/// [`queue_identical`] can tell them apart: an implementation that
/// approximates one fact makes the popup's count wrong.
pub trait ForecastFacts {
    /// The group's current reference, which Make this the reference can
    /// move away from the scan's.
    fn reference(&self, group: &Group) -> Option<FileId>;
    /// `file`'s current counterpart in `group`: `None` for the reference.
    /// A Compare review can make it a member other than the reference.
    fn counterpart(&self, group: &Group, file: FileId) -> Option<FileId>;
    /// Folder-protected or holding a per-file Protect (C-05 I-2).
    fn is_protected(&self, file: FileId) -> bool;
    /// Holding a Kept decision.
    fn is_kept(&self, file: FileId) -> bool;
    /// The counterpart `file` is queued against, or `None` when unqueued.
    fn queued_counterpart(&self, file: FileId) -> Option<FileId>;
}

/// What Queue identical copies queues in one scope.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IdenticalForecast {
    /// The files it queues, in result order (group, then member), each once.
    pub files: Vec<FileId>,
    /// Groups those files belong to.
    pub groups: usize,
    /// The files' total size in bytes.
    pub bytes: u64,
    /// Groups in scope holding any byte-verified exact copy of their
    /// current reference, queueable or not: the count each scope's radio
    /// states.
    pub identical_groups: usize,
}

/// The groups `scope` covers, with their index in `results.groups`, in
/// result order. Group IDs the results do not hold are ignored.
pub fn scope_groups<'a>(
    results: &'a ScanResults,
    scope: &QueueScope,
) -> impl Iterator<Item = (usize, &'a Group)> + 'a {
    let wanted: Option<BTreeSet<GroupId>> = match scope {
        QueueScope::AllByteIdenticalGroups => None,
        QueueScope::Groups(ids) => Some(ids.iter().copied().collect()),
    };
    results
        .groups
        .iter()
        .enumerate()
        .filter(move |(_, g)| wanted.as_ref().is_none_or(|w| w.contains(&g.id)))
}

/// What Queue identical copies queues over `scope` (spec §11.4): per member,
/// only one whose counterpart is its group's reference and whose edge to it
/// is Same bytes with bytes compared equal. A hash match alone is not
/// "bytes match their reference exactly". Reference, protected, kept and
/// already queued members are skipped, and so is any member FR-025 would
/// refuse: the counterpart of a queued file, or one whose reference is
/// queued.
///
/// Evaluated once on the state before the command. Taking a file never
/// changes another file's eligibility within the same command: every file
/// taken is queued against its reference, and the reference is skipped, so
/// no taken file can become another one's queued counterpart.
pub fn queue_identical<F: ForecastFacts + ?Sized>(
    results: &ScanResults,
    scope: &QueueScope,
    facts: &F,
) -> IdenticalForecast {
    let mut out = IdenticalForecast::default();
    // A file listed in two groups is taken once, as the service's queue
    // holds one entry per file.
    let mut taken = BTreeSet::new();
    for (_, group) in scope_groups(results, scope) {
        let Some(reference) = facts.reference(group) else {
            continue;
        };
        let exact = |file: FileId| {
            group.edge_between(file, reference).is_some_and(|e| {
                e.measurement == Measurement::Identical(Verification::ByteVerified)
            })
        };
        // FR-025 "Counterpart of a queued file": counterparts never cross
        // groups, so the group's own queued members are the only ones that
        // can name a member.
        let named: BTreeSet<FileId> = group
            .members
            .iter()
            .filter_map(|m| facts.queued_counterpart(m.file))
            .collect();
        let reference_queued = facts.queued_counterpart(reference).is_some();
        let mut in_group = 0;
        let mut identical = false;
        for m in &group.members {
            let file = m.file;
            if file == reference || !exact(file) {
                continue;
            }
            identical = true;
            let eligible = !facts.is_protected(file)
                && !facts.is_kept(file)
                && facts.queued_counterpart(file).is_none()
                && !reference_queued
                && facts.counterpart(group, file) == Some(reference)
                && !named.contains(&file);
            if eligible && taken.insert(file) {
                out.files.push(file);
                out.bytes += m.size_bytes;
                in_group += 1;
            }
        }
        out.groups += usize::from(in_group > 0);
        out.identical_groups += usize::from(identical);
    }
    out
}
