// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! Engine events and review outcomes into presentation state (plan C-12).
//!
//! Every engine event carries the origin that started it: a scan, a
//! comparison request or a plan revision (C-02). [`apply`] installs an event
//! only when its origin is the one the matching state owner holds as
//! current; anything else is a late answer to a superseded request and is
//! discarded, so a stale response can never replace the active view (spec
//! §11.8). `app.rs` calls [`apply`] for each event of a bounded batch.
//!
//! Which origin is current is set when a request is admitted
//! ([`note_submitted`]) and, for cleanup, moved on by the run itself: a
//! `RunEnded` with a revised plan makes the revised revision current.

use std::sync::Arc;

use fuzzy_dedupe_domain::{ScanResults, ValidationView};
use fuzzy_dedupe_engine::cleanup_sim::CleanupEvent;
use fuzzy_dedupe_engine::service::{
    EngineEvent, EngineRequest, Origin, ReviewOutcome, ReviewStatus, ScanEvent,
};

use crate::presentation::projections;
use crate::presentation::state::{
    CleanupPhase, ComparisonSlot, IntendedAction, PresentationState, QueueEntry, QueueState,
    ScanStage, Severity, StatusMessage,
};

/// What [`apply`] did with an event.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Applied {
    Installed,
    /// The event's origin is not current; state is unchanged.
    DiscardedStale,
}

/// Whether `origin` is the current scan, comparison or plan.
#[must_use]
pub fn is_current(state: &PresentationState, origin: Origin) -> bool {
    match origin {
        Origin::Scan(scan) => state.scan.scan == Some(scan),
        Origin::Request(request) => state.compare.comparison.request == Some(request),
        Origin::Plan(plan) => state.operation.revision == Some(plan),
    }
}

fn status(state: &mut PresentationState, severity: Severity, text: impl Into<String>) {
    state.status = Some(StatusMessage {
        severity,
        text: text.into(),
    });
}

/// Installs one engine event, or discards it when its origin is stale.
pub fn apply(state: &mut PresentationState, event: EngineEvent) -> Applied {
    if !is_current(state, event.origin()) {
        tracing::debug!(origin = ?event.origin(), "discarding a stale engine event");
        return Applied::DiscardedStale;
    }
    match event {
        EngineEvent::Scan { event, .. } => match event {
            ScanEvent::Started => state.scan.stage = ScanStage::Running,
            ScanEvent::Progress(progress) => {
                state.scan.files_scanned = progress.files_read;
                state.scan.progress = Some(progress);
            }
            ScanEvent::Completed(results) => install_results(state, results, ScanStage::Completed),
            ScanEvent::Partial(results) => install_results(state, results, ScanStage::Partial),
        },
        EngineEvent::Comparison { comparison, .. } => {
            state.compare.comparison.result = Some(comparison);
        }
        EngineEvent::Cleanup { event, .. } => apply_cleanup(state, event),
        EngineEvent::NotImplemented { origin, missing } => {
            tracing::info!(?origin, ?missing, "engine module not implemented");
            status(
                state,
                Severity::Warning,
                "That action is not available yet.",
            );
        }
    }
    Applied::Installed
}

/// One C-06 event for the current plan.
fn apply_cleanup(state: &mut PresentationState, event: CleanupEvent) {
    let operation = &mut state.operation;
    match event {
        CleanupEvent::PlanBuilt(plan) => {
            operation.items.clone_from(&plan.items);
            operation.plan = Some(plan);
            operation.phase = CleanupPhase::Review;
            operation.authorized = false;
        }
        CleanupEvent::ItemChanged(item) => {
            if let Some(slot) = operation.items.iter_mut().find(|i| i.target == item.target) {
                *slot = item;
            }
        }
        CleanupEvent::RunEnded { revised, report } => {
            operation.report = Some(report);
            operation.authorized = false;
            match revised {
                // The remaining items under a new revision: it becomes the
                // current origin and needs confirming again (spec §11.6
                // Stopped).
                Some(plan) => {
                    operation.revision = Some(plan.revision);
                    operation.items.clone_from(&plan.items);
                    operation.plan = Some(plan);
                    operation.phase = CleanupPhase::Stopped;
                }
                None => operation.phase = CleanupPhase::Finished,
            }
        }
        CleanupEvent::ExecutionRefused => {
            operation.authorized = false;
            if operation.phase == CleanupPhase::Running {
                operation.phase = CleanupPhase::Review;
            }
            status(
                state,
                Severity::Warning,
                "That plan was already run or replaced; nothing was moved.",
            );
        }
    }
}

/// Makes `results` the current scan's results, as a completed or adopted
/// scan does. A new result set starts with an empty queue (FR-026).
pub fn install_results(
    state: &mut PresentationState,
    results: Arc<ScanResults>,
    outcome: ScanStage,
) {
    state.scan.scan = Some(results.scan);
    state.scan.setup = Some(results.setup.clone());
    state.scan.rules = Some(results.rules);
    state.scan.issues.clone_from(&results.issues);
    state.scan.stage = outcome;
    state.scan.progress = None;
    state.scan.results = Some(results);
    state.queue = QueueState::default();
    state.findings.scan = state.scan.scan;
    state.findings.revision += 1;
}

/// Records an admitted request's origin as current, so its events apply
/// and every earlier origin of the same kind becomes stale.
pub fn note_submitted(state: &mut PresentationState, origin: Origin, request: &EngineRequest) {
    match (origin, request) {
        (Origin::Scan(scan), EngineRequest::StartScan(start)) => {
            state.scan.scan = Some(scan);
            state.scan.setup = Some(start.setup.clone());
            state.scan.rules = Some(start.rules);
            state.scan.stage = ScanStage::Running;
            state.scan.progress = None;
            state.scan.errors.clear();
        }
        (Origin::Scan(scan), EngineRequest::CancelScan(_)) => {
            if state.scan.scan == Some(scan) && state.scan.stage == ScanStage::Running {
                state.scan.stage = ScanStage::Stopping;
            }
        }
        (Origin::Request(id), EngineRequest::LoadComparison(key)) => {
            state.compare.comparison = ComparisonSlot {
                request: Some(id),
                key: Some(*key),
                result: None,
            };
        }
        (Origin::Plan(plan), EngineRequest::BuildCleanupPlan(_)) => {
            let operation = &mut state.operation;
            operation.revision = Some(plan);
            operation.plan = None;
            operation.items.clear();
            operation.report = None;
            operation.authorized = false;
            operation.phase = CleanupPhase::Building;
        }
        (Origin::Plan(plan), EngineRequest::ExecuteApprovedPlan(_)) => {
            if state.operation.revision == Some(plan) {
                state.operation.authorized = true;
                state.operation.phase = CleanupPhase::Running;
            }
        }
        (Origin::Plan(plan), EngineRequest::StopCleanup(_)) => {
            if state.operation.revision == Some(plan)
                && state.operation.phase == CleanupPhase::Running
            {
                state.operation.phase = CleanupPhase::Stopping;
            }
        }
        (origin, request) => {
            tracing::warn!(
                ?origin,
                ?request,
                "admitted request with an unexpected origin"
            );
        }
    }
}

/// Mirrors a review outcome into the queue read model (plan D-003): the
/// engine decided, presentation only records it. Queue membership itself
/// comes from the published view ([`sync_view`]), the single authority.
pub fn apply_review(state: &mut PresentationState, outcome: &ReviewOutcome) {
    for row in &outcome.changed {
        state.queue.decisions.insert(row.file, row.decision);
    }
    match outcome.status {
        ReviewStatus::Refused { file, refusal } => {
            state.queue.last_refusal = Some((file, refusal));
            status(state, Severity::Warning, projections::refusal_text(refusal));
        }
        ReviewStatus::ReferenceChanged { unqueued } if unqueued > 0 => {
            let files = if unqueued == 1 { "file" } else { "files" };
            status(
                state,
                Severity::Info,
                format!("Reference changed; {unqueued} {files} left the queue."),
            );
        }
        _ => {}
    }
    state.findings.revision += 1;
}

/// Replaces the queue read model with the engine's published view: the
/// queue with each target's binding, and the current snapshots and rules.
pub fn sync_view(state: &mut PresentationState, view: Arc<ValidationView>) {
    state.queue.entries = view
        .queue
        .iter()
        .map(|queued| {
            (
                queued.target,
                QueueEntry {
                    counterpart: queued.counterpart,
                    action: IntendedAction::MoveToTrash,
                    acknowledgment: Some(queued.binding),
                },
            )
        })
        .collect();
    state.queue.view = Some(view);
}
