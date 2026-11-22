// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! Simulated scan job (plan C-03). Stub until T5.
//!
//! T5 fills this module, adding `job.rs`, behind the calls the dispatcher in
//! [`crate::service`] makes: `new`, `start` and `cancel`. Those shapes are
//! the C-02 contract and stay fixed. The job builds its scenario with
//! [`crate::fixtures::build`] on a worker and publishes it through
//! `ModuleContext::active` with the generation `start` received, before
//! reporting completion. A `false` from that publication means a newer scan
//! or an adopted scenario superseded this one: the job then reports no
//! completion. It emits `EngineEvent::Scan` events from its own threads
//! through `ModuleContext::events` and performs no filesystem traversal.

use fuzzy_dedupe_domain::ScanId;

use crate::service::{EngineModule, ModuleContext, NotImplemented, ScenarioGeneration, StartScan};

/// The scan job service, owned by the dispatcher thread.
#[derive(Debug)]
pub struct ScanService;

impl ScanService {
    #[must_use]
    pub fn new(_context: &ModuleContext) -> Self {
        Self
    }

    /// Starts scan `scan`. Must return promptly; the work runs on worker
    /// threads.
    ///
    /// # Errors
    ///
    /// [`NotImplemented`] until T5 lands.
    pub fn start(
        &mut self,
        _scan: ScanId,
        _generation: ScenarioGeneration,
        _request: StartScan,
    ) -> Result<(), NotImplemented> {
        Err(NotImplemented::new(EngineModule::ScanSim))
    }

    /// Requests cancellation of `scan`; `Partial` follows once the workers
    /// acknowledge it. Repeated cancellations of one scan may arrive
    /// coalesced into one call, and a cancellation of a finished scan is a
    /// no-op. Pending cancellations are taken before queued jobs, so a cancel
    /// can arrive before the `start` of the same scan; remember it and start
    /// that scan already cancelled.
    ///
    /// # Errors
    ///
    /// [`NotImplemented`] until T5 lands.
    pub fn cancel(&mut self, _scan: ScanId) -> Result<(), NotImplemented> {
        Err(NotImplemented::new(EngineModule::ScanSim))
    }
}
