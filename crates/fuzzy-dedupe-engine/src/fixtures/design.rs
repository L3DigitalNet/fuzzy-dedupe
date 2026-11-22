// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! The `design` scenario: the review frames' sample rows, labels, decision
//! states, scan progress and cleanup rows (plan T9). Stub until T9.
//!
//! T9 replaces the body of [`build`] and keeps its signature, which
//! [`super::build`] calls. Literal sample text fits `FileContent::Bytes`;
//! values that would break a C-01 invariant go to the owner instead of into
//! this file. Sample values are fixtures only and set no production default
//! (spec §11.1 Prototype limitation).

use fuzzy_dedupe_domain::ScanId;

use super::Scenario;
use crate::service::{EngineModule, NotImplemented};

/// Builds the design scenario for scan `scan`.
///
/// # Errors
///
/// [`NotImplemented`] until T9 lands.
pub const fn build(_scan: ScanId, _seed: u64) -> Result<Scenario, NotImplemented> {
    Err(NotImplemented::new(EngineModule::DesignFixture))
}
