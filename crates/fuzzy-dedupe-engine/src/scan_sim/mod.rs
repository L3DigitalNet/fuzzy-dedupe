// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! Simulated scan job (plan C-03): `Started`, then coalesced `Progress`,
//! then `Completed` or, after a cancellation, `Partial`.
//!
//! [`ScanService`] is the dispatcher-side half, behind the calls the
//! dispatcher in [`crate::service`] makes: `new`, `start` and `cancel`.
//! Those shapes are the C-02 contract and stay fixed. Each `start` spawns
//! one coordinator thread (`job.rs`) and returns at once; the coordinator
//! builds the scenario with [`crate::fixtures::build`], saturates
//! `ModuleContext::worker_threads` workers over it, and emits
//! `EngineEvent::Scan` events through `ModuleContext::events`. It publishes
//! the scenario through `ModuleContext::active` with the generation `start`
//! received, before reporting the result. A `false` from that publication
//! means a newer scan or an adopted scenario superseded this one: the job
//! then reports no result. No filesystem traversal happens anywhere here
//! (plan D-005): the files read are the fixture's in-memory buffers.
//!
//! Lifecycle rules the service enforces, because the facade admits a
//! `CancelScan` only for the live scan and drops every other one:
//!
//! - Starting a scan supersedes the one still running. The service stops it
//!   itself, and it then reports nothing more, so results of two scan IDs
//!   never merge; its events already queued stay tagged with its own ID for
//!   presentation to discard.
//! - A cancel may reach the service before the `start` of the same scan
//!   (the dispatcher takes controls before jobs). Scan IDs only grow, so a
//!   cancel for an ID above every started one is remembered and that scan
//!   starts already cancelled; one at or below it is for a finished or
//!   superseded scan and is a no-op.

mod job;

use std::sync::Arc;

use fuzzy_dedupe_domain::ScanId;

use crate::service::{
    ActiveScenario, EventSink, ModuleContext, NotImplemented, ScenarioGeneration, StartScan,
};

use self::job::{Control, Job};

/// The scan that a cancel can still affect.
#[derive(Debug)]
struct Running {
    scan: ScanId,
    control: Arc<Control>,
}

/// The scan job service, owned by the dispatcher thread.
#[derive(Debug)]
pub struct ScanService {
    events: EventSink,
    active: ActiveScenario,
    workers: usize,
    running: Option<Running>,
    /// The highest scan ID started.
    newest: Option<ScanId>,
    /// A cancel that arrived before its scan's `start`.
    early_cancel: Option<ScanId>,
}

impl ScanService {
    #[must_use]
    pub fn new(context: &ModuleContext) -> Self {
        Self {
            events: context.events.clone(),
            active: context.active.clone(),
            workers: context.worker_threads.max(1),
            running: None,
            newest: None,
            early_cancel: None,
        }
    }

    /// Starts scan `scan`, superseding any scan still running, and returns
    /// at once; the work runs on threads this module owns.
    ///
    /// # Errors
    ///
    /// None in practice: the signature is the C-02 shape. A scenario whose
    /// fixture is still a stub is reported asynchronously as
    /// `EngineEvent::NotImplemented` for the scan's origin, because only the
    /// job's own build discovers it.
    pub fn start(
        &mut self,
        scan: ScanId,
        generation: ScenarioGeneration,
        request: StartScan,
    ) -> Result<(), NotImplemented> {
        if let Some(old) = self.running.take() {
            old.control.supersede();
        }
        let control = Arc::new(Control::default());
        if self.early_cancel.take() == Some(scan) {
            control.cancel();
        }
        self.newest = Some(scan);
        self.running = Some(Running {
            scan,
            control: Arc::clone(&control),
        });
        let job = Job {
            scan,
            generation,
            request,
            events: self.events.clone(),
            active: self.active.clone(),
            workers: self.workers,
            control,
        };
        let fallback = job.clone();
        if std::thread::Builder::new()
            .name(format!("scan-sim-{}", scan.get()))
            .spawn(move || job.run())
            .is_err()
        {
            fallback.abandon();
        }
        Ok(())
    }

    /// Requests cancellation of `scan`; `Partial` follows once the workers
    /// acknowledge it. Repeated cancellations of one scan may arrive
    /// coalesced into one call, and a cancellation of a finished scan is a
    /// no-op. A cancel for a scan not started yet is remembered (see the
    /// module header).
    ///
    /// # Errors
    ///
    /// None in practice: the signature is the C-02 shape.
    pub fn cancel(&mut self, scan: ScanId) -> Result<(), NotImplemented> {
        match &self.running {
            Some(running) if running.scan == scan => running.control.cancel(),
            _ if self.newest.is_some_and(|newest| scan <= newest) => {}
            _ => self.early_cancel = Some(scan),
        }
        Ok(())
    }
}

impl Drop for ScanService {
    /// The dispatcher has exited, so the engine is gone: stop the running
    /// scan without a result instead of letting it saturate the cores for
    /// seconds more. Its threads are not joined; they exit within
    /// microseconds of seeing the flag.
    fn drop(&mut self) {
        if let Some(running) = &self.running {
            running.control.supersede();
        }
    }
}
