// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! Simulated cleanup run (plan C-06). Stub until T8.
//!
//! T8 fills this module, adding `run.rs`, behind the calls the dispatcher in
//! [`crate::service`] makes: `new`, `build`, `execute` and `stop`. It acts on
//! file IDs only, with scripted outcomes; no filesystem call may exist here.
//! [`CleanupEvent`] is T8's to refine as C-06 settles.
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

use std::sync::Arc;

use fuzzy_dedupe_domain::{CleanupPlan, PlanItem, PlanRevision};

use crate::service::{EngineModule, ModuleContext, NotImplemented, PublishedValidation};

/// What a cleanup run reports, carried by `EngineEvent::Cleanup`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CleanupEvent {
    /// The plan was built; blocked items are already marked.
    PlanBuilt(Arc<CleanupPlan>),
    /// One item changed state.
    ItemChanged(PlanItem),
    /// The run ended. After a stop or a failure, `revised` holds the
    /// remaining items under a new revision for re-confirmation.
    RunEnded { revised: Option<Arc<CleanupPlan>> },
    /// A second execution of one authorization, or one of an unknown plan.
    ExecutionRefused,
}

/// The cleanup service, owned by the dispatcher thread.
#[derive(Debug)]
pub struct CleanupService;

impl CleanupService {
    #[must_use]
    pub fn new(_context: &ModuleContext) -> Self {
        Self
    }

    /// Builds plan `plan` from `validation`: its view's scan and queue, as the
    /// engine checked and published them when the request was submitted.
    ///
    /// # Errors
    ///
    /// [`NotImplemented`] until T8 lands.
    pub fn build(
        &mut self,
        _plan: PlanRevision,
        _validation: PublishedValidation,
    ) -> Result<(), NotImplemented> {
        Err(NotImplemented::new(EngineModule::CleanupSim))
    }

    /// Executes plan `plan`, at most once per revision. Must return
    /// promptly; the run proceeds on a worker.
    ///
    /// # Errors
    ///
    /// [`NotImplemented`] until T8 lands.
    pub fn execute(&mut self, _plan: PlanRevision) -> Result<(), NotImplemented> {
        Err(NotImplemented::new(EngineModule::CleanupSim))
    }

    /// Stops the run of `plan` before its next item.
    ///
    /// # Errors
    ///
    /// [`NotImplemented`] until T8 lands.
    pub fn stop(&mut self, _plan: PlanRevision) -> Result<(), NotImplemented> {
        Err(NotImplemented::new(EngineModule::CleanupSim))
    }
}
