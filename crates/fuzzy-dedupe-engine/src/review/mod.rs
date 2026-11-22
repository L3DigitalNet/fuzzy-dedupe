// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! Review and queue policy service (plan C-05). Stub until T7.
//!
//! The single authority that evaluates every queue entry point and owns the
//! scan-scoped decision set (plan D-003); presentation's `QueueState` is a
//! read model updated from [`ReviewOutcome`]s. T7 fills this module, adding
//! `policy.rs`, behind the calls [`crate::service::Engine`] makes: `new`,
//! `load_scan`, `apply` and `validation_view`. Those shapes and the command
//! and outcome types below are the C-02 contract and stay fixed.
//!
//! Two engine-side duties frame the service. Before `apply`, the engine
//! resolves a command's `reviewed` request ID to a trusted
//! [`ReviewedPair`] from the comparison service's results, or refuses the
//! command; `apply` receives the resolved pair and never sees an unchecked
//! one. After `apply` and after `load_scan`, the engine publishes
//! `validation_view` for the cleanup run to revalidate against, so the view
//! must describe the state as of the call that just returned.

use std::sync::Arc;

use fuzzy_dedupe_domain::{
    EvidenceUpdate, FileId, GroupId, InvalidatedDecision, MemberDecision, MemberRole, QueueOrigin,
    QueueScope, Refusal, RequestId, ReviewedPair, ScanResults, ValidationView,
};

use crate::service::{EngineModule, NotImplemented};

/// The C-05 operations. Every one runs synchronously on the UI thread.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReviewCommand {
    /// Queue `file` against `counterpart`. From Compare, `counterpart` may
    /// be a member the user reviewed the row against, which then becomes the
    /// row's counterpart (§10.1); elsewhere it is the row's current one.
    /// `reviewed` names the comparison the pair was reviewed in; it is
    /// required when `origin` is `Compare`, and the engine refuses the
    /// command when it is missing or does not match.
    RequestQueue {
        file: FileId,
        counterpart: FileId,
        origin: QueueOrigin,
        reviewed: Option<RequestId>,
    },
    /// The Review unique text acknowledgment for `file` against
    /// `counterpart`. `reviewed` names the comparison when the dialog was
    /// opened from Compare, so the acknowledgment binds to that comparison's
    /// snapshots and Unique count.
    AcknowledgeUniqueText {
        file: FileId,
        counterpart: FileId,
        reviewed: Option<RequestId>,
    },
    /// Compare's Keep both: a Kept decision for the displayed pair, bound to
    /// comparison `reviewed` (spec §11.5). It must not change other members
    /// of the group.
    KeepBoth {
        file: FileId,
        counterpart: FileId,
        reviewed: RequestId,
    },
    RemoveFromQueue {
        file: FileId,
    },
    /// Set (`true`) or clear (`false`) a Kept decision.
    MarkKept {
        file: FileId,
        kept: bool,
    },
    /// Set or clear a per-file Protect decision.
    ProtectFile {
        file: FileId,
        protected: bool,
    },
    MakeReference {
        file: FileId,
    },
    QueueIdenticalCopies {
        scope: QueueScope,
    },
    /// Empties the queue and every decision; folder protection survives.
    ClearForNewScan,
    /// A fixture-injected change to a binding component.
    UpdateEvidence(EvidenceUpdate),
}

/// How a command ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReviewStatus {
    /// Applied with no queue change to report.
    Applied,
    /// This many files entered the queue.
    Queued {
        files: usize,
    },
    /// The request needs Review unique text first; nothing was queued.
    NeedsReview {
        file: FileId,
        counterpart: FileId,
    },
    /// Refused; nothing changed.
    Refused {
        file: FileId,
        refusal: Refusal,
    },
    /// Make this the reference finished; this many other files left the
    /// queue, the count the status bar reports.
    ReferenceChanged {
        unqueued: usize,
    },
    NotImplemented(NotImplemented),
}

/// One row's state after a command, for the presentation read model.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FileReviewState {
    pub file: FileId,
    pub group: GroupId,
    /// The current role, which Make this the reference and Compare review
    /// can change without touching the immutable scan results.
    pub role: MemberRole,
    pub decision: MemberDecision,
}

/// The answer to every review command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewOutcome {
    pub status: ReviewStatus,
    /// Every row whose role or decision changed.
    pub changed: Vec<FileReviewState>,
    /// Every decision the command invalidated (C-05 I-10, PQ-008).
    pub invalidated: Vec<InvalidatedDecision>,
}

impl ReviewOutcome {
    /// A refusal that changed nothing.
    #[must_use]
    pub const fn refused(file: FileId, refusal: Refusal) -> Self {
        Self {
            status: ReviewStatus::Refused { file, refusal },
            changed: Vec::new(),
            invalidated: Vec::new(),
        }
    }

    #[must_use]
    pub const fn not_implemented() -> Self {
        Self {
            status: ReviewStatus::NotImplemented(NotImplemented::new(EngineModule::Review)),
            changed: Vec::new(),
            invalidated: Vec::new(),
        }
    }
}

/// The review service, owned by [`crate::service::Engine`].
#[derive(Debug, Default)]
pub struct ReviewService;

impl ReviewService {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Installs the result set later commands apply to.
    pub fn load_scan(&mut self, _results: Arc<ScanResults>) {}

    /// Applies `command`. `reviewed` is the engine-resolved evidence of the
    /// command's `reviewed` request ID: present exactly when the command
    /// named one and it matched.
    pub fn apply(
        &mut self,
        _command: ReviewCommand,
        _reviewed: Option<ReviewedPair>,
    ) -> ReviewOutcome {
        ReviewOutcome::not_implemented()
    }

    /// The state cleanup revalidates against, or `Ok(None)` before any scan
    /// is loaded. The queue it lists is also what a cleanup plan is built
    /// from.
    ///
    /// # Errors
    ///
    /// [`NotImplemented`] until T7 lands.
    pub fn validation_view(&self) -> Result<Option<ValidationView>, NotImplemented> {
        Err(NotImplemented::new(EngineModule::Review))
    }
}
