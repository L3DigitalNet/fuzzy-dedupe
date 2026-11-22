// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! Comparison service (plan C-04). Stub until T6.
//!
//! T6 fills this module, adding `texts.rs`, `aligned.rs` and `normalize.rs`,
//! behind the calls the dispatcher in [`crate::service`] makes: `new` and
//! `load`. Those shapes and [`ComparisonRequest`] are the C-02 contract and
//! stay fixed; [`Comparison`] is a placeholder whose fields T6 defines. Text
//! comes from the active scenario's in-memory buffers, never from a file.
//!
//! Review evidence: for each result it delivers, the service also records a
//! `ReviewedPair` (both snapshots, the rules revision, the mode and the
//! §10.1 Unique count) under the request ID with
//! `ModuleContext::comparisons.record`. That record is the only evidence a
//! Compare-originated review command can bind to, so record it before the
//! `EngineEvent::Comparison` that lets the user act on the result.

use fuzzy_dedupe_domain::{CompareMode, FileId, RequestId, RulesRevision, ScanId, SnapshotId};

use crate::service::{EngineModule, ModuleContext, NotImplemented};

/// One comparison to compute. The four key fields (both snapshots, the
/// rules revision and the mode) identify a cached result.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ComparisonRequest {
    pub scan: ScanId,
    pub target: FileId,
    pub counterpart: FileId,
    pub target_snapshot: SnapshotId,
    pub counterpart_snapshot: SnapshotId,
    pub rules: RulesRevision,
    pub mode: CompareMode,
}

/// The aligned-row model of C-04. Placeholder: T6 defines its fields.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Comparison {}

/// The comparison service, owned by the dispatcher thread.
#[derive(Debug)]
pub struct CompareService;

impl CompareService {
    #[must_use]
    pub fn new(_context: &ModuleContext) -> Self {
        Self
    }

    /// Starts computing `comparison` for request `request`, superseding any
    /// earlier request. Must return promptly.
    ///
    /// # Errors
    ///
    /// [`NotImplemented`] until T6 lands.
    pub fn load(
        &mut self,
        _request: RequestId,
        _comparison: ComparisonRequest,
    ) -> Result<(), NotImplemented> {
        Err(NotImplemented::new(EngineModule::Compare))
    }
}
