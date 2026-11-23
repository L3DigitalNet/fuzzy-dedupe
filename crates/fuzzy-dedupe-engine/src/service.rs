// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! The engine facade (plan C-02): typed requests in, revisioned events out.
//!
//! Two kinds of call cross it (plan D-008, spec §11.8):
//!
//! - **Review commands** are synchronous, in-memory calls on [`Engine::review`],
//!   made by `app.rs` after the UI pass. They are cheap and have no I/O to
//!   hide, so routing them through a worker would only add ordering hazards.
//! - **Jobs** (`StartScan`, `LoadComparison`, `BuildCleanupPlan`,
//!   `ExecuteApprovedPlan`) go through [`Engine::submit`] into a bounded
//!   crossbeam queue. A full queue answers [`SubmitError::Busy`] at once, so
//!   the UI thread never blocks. Cancellation (`CancelScan`, `StopCleanup`)
//!   bypasses that queue and is always admitted (see [`Engine::submit`]).
//!
//! One dispatcher thread takes pending cancellations first, then jobs, and
//! hands each to its module service. The app drains events in bounded
//! batches with [`Engine::drain_events`]. Every event carries the [`Origin`]
//! that started it, so presentation discards events whose origin is no longer
//! current.
//!
//! Shared state the engine owns and the modules read or write, all reached
//! through [`ModuleContext`] so T5 to T8 never edit this file:
//!
//! | State                  | Written by                    | Read by                 |
//! | ---------------------- | ----------------------------- | ----------------------- |
//! | [`ActiveScenario`]     | scan job, `adopt_scenario`    | comparison service      |
//! | [`ComparisonEvidence`] | comparison service (T6)       | [`Engine::review`]      |
//! | [`ValidationBoard`]    | engine, after each review call | cleanup run (T8)       |
//! | [`PlanRevisions`]      | facade, cleanup service (T8)  | facade (stop liveness)  |
//!
//! Contract for module services (`scan_sim`, `compare`, `cleanup_sim`): each
//! handler runs on the dispatcher thread and must return promptly; long work
//! runs on threads the module owns. A handler on the dispatcher thread may
//! emit only with [`EventSink::try_send`]; module-owned threads may use the
//! blocking [`EventSink::send`]. A blocked dispatcher would delay every later
//! job and every cancellation.
//!
//! No GUI or OS type crosses this facade, and there is no path-based delete
//! callback (§11.8): cleanup acts only on engine-issued plan revisions.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender, TryRecvError, TrySendError, select_biased};
use fuzzy_dedupe_domain::{
    MatchingRules, PlanRevision, QueueOrigin, Refusal, RequestId, ReviewedPair, ScanId,
    ScanProgress, ScanResults, ScanSetup, ValidationView,
};

use crate::cleanup_sim::{CleanupEvent, CleanupService};
use crate::compare::{CompareService, Comparison};
use crate::fixtures::{Scenario, ScenarioKind};
use crate::review::ReviewService;
use crate::scan_sim::ScanService;

pub use crate::compare::ComparisonRequest;
pub use crate::review::{FileReviewState, ReviewCommand, ReviewOutcome, ReviewStatus};

/// The engine part that has no implementation yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EngineModule {
    ScanSim,
    Compare,
    Review,
    CleanupSim,
    DesignFixture,
}

/// The typed outcome of a stub: the request was understood and refused
/// because its module is not implemented. Stubs return this and never panic.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NotImplemented {
    pub module: EngineModule,
}

impl NotImplemented {
    #[must_use]
    pub const fn new(module: EngineModule) -> Self {
        Self { module }
    }
}

/// Queue sizes and worker count.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EngineConfig {
    /// Job requests waiting for the dispatcher. Values below 1 are raised to
    /// 1, because a zero-capacity crossbeam channel is a rendezvous channel
    /// whose `try_send` fails whenever the dispatcher is busy.
    pub request_capacity: usize,
    /// Events waiting for the app. Module workers block when it is full,
    /// which is the backpressure that keeps a slow frame from buffering a
    /// whole scan; the dispatcher itself never blocks on it.
    pub event_capacity: usize,
    /// Threads a job may saturate (the simulated scan uses one per core).
    pub worker_threads: usize,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            request_capacity: 64,
            event_capacity: 4096,
            worker_threads: std::thread::available_parallelism().map_or(1, usize::from),
        }
    }
}

/// Starts a simulated scan of a fixture scenario (C-03). The engine assigns
/// the scan ID and keeps these values as the scan's immutable copy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartScan {
    pub setup: ScanSetup,
    pub rules: MatchingRules,
    pub scenario: ScenarioKind,
    pub seed: u64,
}

/// Builds a cleanup plan from the review service's current queue (C-06).
/// The targets come from the review service's published validation view,
/// the single queue authority (plan D-003), never from presentation state;
/// `scan` must be the scan the review service holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildCleanupPlan {
    pub scan: ScanId,
}

/// Runs a built plan. The plan revision is the authorization: each revision
/// executes at most once, and a revised plan gets a new revision.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExecuteApprovedPlan {
    pub plan: PlanRevision,
}

/// Every asynchronous request the facade accepts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EngineRequest {
    StartScan(StartScan),
    CancelScan(ScanId),
    LoadComparison(ComparisonRequest),
    BuildCleanupPlan(BuildCleanupPlan),
    ExecuteApprovedPlan(ExecuteApprovedPlan),
    StopCleanup(PlanRevision),
}

/// The ID an event belongs to: the scan, request or plan that started it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Origin {
    Scan(ScanId),
    Request(RequestId),
    Plan(PlanRevision),
}

/// Why [`Engine::submit`] admitted nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SubmitError {
    /// The request queue is full; retry on a later frame.
    Busy,
    /// A synchronous precondition belongs to an unimplemented module.
    NotImplemented(NotImplemented),
    /// `BuildCleanupPlan` named a scan other than the one the review
    /// service holds (`current`), so its queue cannot be that scan's.
    ScanMismatch {
        requested: ScanId,
        current: Option<ScanId>,
    },
    /// The dispatcher has stopped; no later request can succeed.
    Stopped,
}

/// Why [`Engine::adopt_scenario`] refused a scenario.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AdoptError {
    /// The scenario's scan ID is not fresh for this engine.
    ScanIdReused { scan: ScanId },
}

/// The simulated scan's lifecycle (C-03): `Started`, any number of
/// `Progress`, then `Completed` or, after a cancellation, `Partial`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScanEvent {
    Started,
    Progress(ScanProgress),
    Completed(Arc<ScanResults>),
    /// Emitted only after the workers acknowledged a cancellation.
    Partial(Arc<ScanResults>),
}

/// Everything the engine reports to the app.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EngineEvent {
    Scan {
        scan: ScanId,
        event: ScanEvent,
    },
    Comparison {
        request: RequestId,
        comparison: Arc<Comparison>,
    },
    Cleanup {
        plan: PlanRevision,
        event: CleanupEvent,
    },
    /// A job reached a module that is not implemented yet.
    NotImplemented {
        origin: Origin,
        missing: NotImplemented,
    },
}

impl EngineEvent {
    #[must_use]
    pub const fn origin(&self) -> Origin {
        match self {
            Self::Scan { scan, .. } => Origin::Scan(*scan),
            Self::Comparison { request, .. } => Origin::Request(*request),
            Self::Cleanup { plan, .. } => Origin::Plan(*plan),
            Self::NotImplemented { origin, .. } => *origin,
        }
    }
}

/// The module side of the bounded event queue.
#[derive(Clone, Debug)]
pub struct EventSink(Sender<EngineEvent>);

impl EventSink {
    /// Blocks while the queue is full (backpressure). Returns `false` once
    /// the engine is dropped, which tells a worker to stop. Never call this
    /// on the dispatcher thread; use [`EventSink::try_send`] there.
    #[must_use = "a false return means the engine is gone and the worker should stop"]
    pub fn send(&self, event: EngineEvent) -> bool {
        self.0.send(event).is_ok()
    }

    /// Never blocks.
    ///
    /// # Errors
    ///
    /// Hands the event back when the queue is full or the engine is gone.
    pub fn try_send(&self, event: EngineEvent) -> Result<(), TrySendError<EngineEvent>> {
        self.0.try_send(event)
    }
}

/// A poisoned lock still holds consistent data here: every critical section
/// below is a single assignment or collection update that cannot panic
/// half-way, so recovering the guard is safe and keeps the engine panic-free.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// One claim on publishing the active scenario. Allocated per `StartScan`
/// and per `adopt_scenario`; later generations supersede earlier ones.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ScenarioGeneration(u64);

#[derive(Debug, Default)]
struct ActiveInner {
    /// The highest generation handed out, so each one is unique.
    allocated: u64,
    /// The newest generation that is allowed to publish.
    current: u64,
    scenario: Option<Arc<Scenario>>,
}

/// The scenario whose in-memory buffers back the current results, shared
/// between the scan job that builds it and the comparison service that reads
/// it. Holding the buffers here keeps file contents out of the domain model.
///
/// Publication is fenced by [`ScenarioGeneration`]: a slow scan worker that
/// finishes after a newer `StartScan` or `adopt_scenario` cannot overwrite
/// the newer scenario, because its generation is no longer current. Without
/// the fence, Compare could show the texts of a superseded scan against the
/// current results.
#[derive(Clone, Debug, Default)]
pub struct ActiveScenario(Arc<Mutex<ActiveInner>>);

impl ActiveScenario {
    /// Allocates a generation and makes it current at once, superseding
    /// every earlier one.
    #[must_use = "only the returned generation may publish"]
    pub fn begin(&self) -> ScenarioGeneration {
        let generation = self.reserve();
        self.activate(generation);
        generation
    }

    /// Allocates a generation without superseding anything yet, so a
    /// `StartScan` refused as Busy leaves the running scan's claim intact.
    fn reserve(&self) -> ScenarioGeneration {
        let mut inner = lock(&self.0);
        inner.allocated += 1;
        ScenarioGeneration(inner.allocated)
    }

    /// Supersedes every generation older than `generation`.
    fn activate(&self, generation: ScenarioGeneration) {
        let mut inner = lock(&self.0);
        inner.current = inner.current.max(generation.0);
    }

    /// Publishes `scenario` if `generation` is not older than the current
    /// one, and reports whether it took effect. A worker may publish before
    /// the engine has activated its generation, so "not older" rather than
    /// "equal" is the test.
    #[must_use = "a false return means the scan was superseded and must not report completion"]
    pub fn publish(&self, generation: ScenarioGeneration, scenario: Arc<Scenario>) -> bool {
        let mut inner = lock(&self.0);
        if generation.0 < inner.current {
            return false;
        }
        inner.current = generation.0;
        inner.scenario = Some(scenario);
        true
    }

    #[must_use]
    pub fn get(&self) -> Option<Arc<Scenario>> {
        lock(&self.0).scenario.clone()
    }
}

/// Comparison results kept as review evidence, at most this many. A Compare
/// review refers to the comparison on screen, so only recent requests need
/// to resolve; older ones fail closed as `EvidenceUnavailable`.
const EVIDENCE_SLOTS: usize = 64;

/// The engine-trusted store of reviewed-pair evidence, keyed by the request
/// that computed it (C-04 into C-05).
///
/// Rejected: letting the caller pass a [`ReviewedPair`] in the review
/// command, which would let presentation code invent or reuse evidence. The
/// app names a `RequestId`; [`Engine::review`] looks it up here and refuses
/// a command whose pair, or whose scan, differs. The comparison service (T6)
/// records each result's pair with [`ComparisonEvidence::record`] through
/// `ModuleContext::comparisons`; the app has no handle on this store.
#[derive(Clone, Debug, Default)]
pub struct ComparisonEvidence(Arc<Mutex<VecDeque<(RequestId, ReviewedPair)>>>);

impl ComparisonEvidence {
    /// Records the evidence of `request`, replacing an earlier record for the
    /// same request and evicting the oldest beyond the slot limit.
    pub fn record(&self, request: RequestId, pair: ReviewedPair) {
        let mut slots = lock(&self.0);
        slots.retain(|(id, _)| *id != request);
        slots.push_back((request, pair));
        while slots.len() > EVIDENCE_SLOTS {
            slots.pop_front();
        }
    }

    #[must_use]
    pub fn get(&self, request: RequestId) -> Option<ReviewedPair> {
        lock(&self.0)
            .iter()
            .find(|(id, _)| *id == request)
            .map(|(_, pair)| *pair)
    }
}

/// One published validation view with the revision the engine gave it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublishedValidation {
    /// Increases with every publication, so a reader can tell whether review
    /// state changed since it last looked.
    pub revision: u64,
    pub view: Arc<ValidationView>,
}

#[derive(Debug, Default)]
struct BoardInner {
    revision: u64,
    current: Option<PublishedValidation>,
}

/// The newest review state cleanup must revalidate against (spec §10.4,
/// ERR-001). The engine republishes it after every review command and every
/// scan load, from `ReviewService::validation_view`.
///
/// Ordering guarantee: the engine applies each review command and publishes
/// the view that results under this board's lock, in one critical section.
/// A cleanup item is validated and committed inside
/// [`ValidationBoard::validate_and_commit`], under the same lock. So for any
/// item, every review change happened either wholly before its validation
/// (and the item saw the new view) or wholly after its commit; none can fall
/// between them. Rejected: reading [`ValidationBoard::current`] and then
/// committing, which leaves a window in which the user protects or unqueues
/// the file after the check but before the simulated move.
#[derive(Clone, Debug, Default)]
pub struct ValidationBoard(Arc<Mutex<BoardInner>>);

impl ValidationBoard {
    /// A snapshot for display or diagnostics. Never use it to authorize a
    /// move; that is [`ValidationBoard::validate_and_commit`]'s job.
    #[must_use]
    pub fn current(&self) -> Option<PublishedValidation> {
        lock(&self.0).current.clone()
    }

    /// Runs `commit` against the current view while no review change can be
    /// applied or published, and returns the view's revision with `commit`'s
    /// result; `None` when no view is published, which a run must treat as
    /// "stop". `commit` holds the lock that the UI thread's review calls
    /// wait for: it must be short, must not block (no `EventSink::send`), and
    /// must not touch this board again.
    pub fn validate_and_commit<R>(
        &self,
        commit: impl FnOnce(&PublishedValidation) -> R,
    ) -> Option<(u64, R)> {
        let inner = lock(&self.0);
        let published = inner.current.as_ref()?;
        Some((published.revision, commit(published)))
    }

    /// Applies a review change and publishes its resulting view in one
    /// critical section (the ordering guarantee above). `change` returns
    /// its own result and the view to publish: `Some(None)` withdraws the
    /// view, `None` leaves the board untouched (a stub review service).
    fn transact<R>(&self, change: impl FnOnce() -> (R, Option<Option<ValidationView>>)) -> R {
        let mut inner = lock(&self.0);
        let (result, view) = change();
        if let Some(view) = view {
            inner.revision += 1;
            inner.current = view.map(|view| PublishedValidation {
                revision: inner.revision,
                view: Arc::new(view),
            });
        }
        result
    }

    /// Replaces the view (`None` withdraws it) and returns what was
    /// published.
    fn publish(&self, view: Option<ValidationView>) -> Option<PublishedValidation> {
        let mut inner = lock(&self.0);
        inner.revision += 1;
        inner.current = view.map(|view| PublishedValidation {
            revision: inner.revision,
            view: Arc::new(view),
        });
        inner.current.clone()
    }
}

#[derive(Debug, Default)]
struct PlanInner {
    allocated: u64,
    /// The plan whose run a stop may still affect: the newest one activated
    /// and not yet ended.
    active: Option<PlanRevision>,
}

/// The one source of plan revisions, shared by the facade (`BuildCleanupPlan`)
/// and the cleanup service (a revised plan after a stop), so no revision is
/// ever handed out twice. Without one allocator, T8's revised plan and the
/// next built plan could share a revision, and with it an authorization.
#[derive(Clone, Debug, Default)]
pub struct PlanRevisions(Arc<Mutex<PlanInner>>);

impl PlanRevisions {
    /// A fresh revision, greater than every earlier one.
    #[must_use]
    pub fn allocate(&self) -> PlanRevision {
        let mut inner = lock(&self.0);
        inner.allocated += 1;
        PlanRevision::new(inner.allocated)
    }

    /// Makes `plan` the live plan, superseding older ones: only the live
    /// plan's `StopCleanup` reaches the cleanup service. The facade calls it
    /// once a built plan is admitted; T8 calls it for a revised plan before
    /// reporting it.
    pub fn activate(&self, plan: PlanRevision) {
        let mut inner = lock(&self.0);
        if inner.active.is_none_or(|live| live < plan) {
            inner.active = Some(plan);
        }
    }

    /// Ends `plan`'s liveness if it is still the live plan.
    pub fn retire(&self, plan: PlanRevision) {
        let mut inner = lock(&self.0);
        if inner.active == Some(plan) {
            inner.active = None;
        }
    }

    #[must_use]
    pub fn active(&self) -> Option<PlanRevision> {
        lock(&self.0).active
    }
}

/// What every module service receives when the engine starts.
#[derive(Clone, Debug)]
pub struct ModuleContext {
    pub events: EventSink,
    pub active: ActiveScenario,
    pub comparisons: ComparisonEvidence,
    pub validation: ValidationBoard,
    pub plan_revisions: PlanRevisions,
    pub worker_threads: usize,
}

/// Cancellations and stops admitted for the dispatcher, one slot per kind.
///
/// Only a live operation is ever held: the scan that is running (the newest
/// started, until its completion is drained) and the live plan
/// ([`PlanRevisions::active`]). A cancel or stop for any other ID is
/// acknowledged and dropped at admission, because there is nothing left for
/// it to stop. With one live operation per kind, one slot per kind suffices
/// and no live control can ever be displaced by unrelated ones. Rejected: a
/// capped set evicting the oldest ID, which let a burst of stale or invented
/// IDs push the live cancellation out.
#[derive(Debug, Default)]
struct PendingControl {
    cancel: Option<ScanId>,
    stop: Option<PlanRevision>,
}

impl PendingControl {
    fn len(&self) -> usize {
        usize::from(self.cancel.is_some()) + usize::from(self.stop.is_some())
    }
}

/// A job after the facade assigned its ID.
#[derive(Debug)]
enum Job {
    StartScan {
        scan: ScanId,
        generation: ScenarioGeneration,
        request: StartScan,
    },
    LoadComparison {
        request: RequestId,
        comparison: ComparisonRequest,
    },
    BuildCleanupPlan {
        plan: PlanRevision,
        validation: PublishedValidation,
    },
    ExecuteApprovedPlan(PlanRevision),
}

/// The application's handle on the engine. Owned by `app.rs` on the UI
/// thread; dropping it stops the dispatcher.
#[derive(Debug)]
pub struct Engine {
    // Options only so `Drop` can release them before joining the thread.
    jobs: Option<Sender<Job>>,
    /// Capacity-one wake for the dispatcher; a full slot already means "look
    /// at the pending controls", so a failed `try_send` loses nothing.
    wake: Option<Sender<()>>,
    events: Option<Receiver<EngineEvent>>,
    dispatcher: Option<JoinHandle<()>>,
    control: Arc<Mutex<PendingControl>>,
    active: ActiveScenario,
    evidence: ComparisonEvidence,
    validation: ValidationBoard,
    plan_revisions: PlanRevisions,
    review: Review,
    next_scan: u64,
    /// The highest scan ID started or adopted. `adopt_scenario` refuses any
    /// ID at or below it, so a late event of an earlier scan can never be
    /// mistaken for the current one's.
    highest_seen_scan: u64,
    next_request: u64,
    /// The scan a `CancelScan` can still affect: set when a scan is admitted,
    /// cleared when its completion is drained or a scenario is adopted.
    live_scan: Option<ScanId>,
    /// The newest scan started or adopted. Only its completion is installed
    /// into the review service; a superseded scan's late result is passed on
    /// tagged with its own ID for presentation to discard.
    current_scan: Option<ScanId>,
    /// The scan whose results the review service holds.
    review_scan: Option<ScanId>,
}

impl Engine {
    /// Starts the dispatcher thread.
    ///
    /// # Errors
    ///
    /// Returns the operating system's error when the thread cannot be
    /// spawned.
    pub fn new(config: EngineConfig) -> std::io::Result<Self> {
        Self::with_modules(config, Services::new)
    }

    /// [`Engine::new`] with the dispatcher's module services built by
    /// `modules`, on the dispatcher thread, from the engine's context. The
    /// seam lets `tests` run the facade over test doubles, so its properties
    /// rest on deterministic fakes rather than on what a module stub or its
    /// replacement happens to answer.
    fn with_modules<M: Modules>(
        config: EngineConfig,
        modules: impl FnOnce(&ModuleContext) -> M + Send + 'static,
    ) -> std::io::Result<Self> {
        let (jobs_tx, jobs_rx) = crossbeam_channel::bounded(config.request_capacity.max(1));
        let (wake_tx, wake_rx) = crossbeam_channel::bounded(1);
        let (events_tx, events_rx) = crossbeam_channel::bounded(config.event_capacity.max(1));
        let control = Arc::new(Mutex::new(PendingControl::default()));
        let context = ModuleContext {
            events: EventSink(events_tx),
            active: ActiveScenario::default(),
            comparisons: ComparisonEvidence::default(),
            validation: ValidationBoard::default(),
            plan_revisions: PlanRevisions::default(),
            worker_threads: config.worker_threads.max(1),
        };
        let (active, evidence, validation, plan_revisions) = (
            context.active.clone(),
            context.comparisons.clone(),
            context.validation.clone(),
            context.plan_revisions.clone(),
        );
        let dispatcher = Dispatcher {
            context,
            jobs: jobs_rx,
            wake: wake_rx,
            control: Arc::clone(&control),
            backlog: VecDeque::new(),
        };
        let dispatcher = std::thread::Builder::new()
            .name("engine-dispatch".into())
            .spawn(move || dispatcher.run(modules))?;
        Ok(Self {
            jobs: Some(jobs_tx),
            wake: Some(wake_tx),
            events: Some(events_rx),
            dispatcher: Some(dispatcher),
            control,
            active,
            evidence,
            validation,
            plan_revisions,
            review: Review::new(),
            next_scan: 1,
            highest_seen_scan: 0,
            next_request: 1,
            live_scan: None,
            current_scan: None,
            review_scan: None,
        })
    }

    /// Admits one asynchronous request without blocking and returns the ID
    /// its events will carry.
    ///
    /// Cancellation never enters the job queue: `CancelScan` and
    /// `StopCleanup` set a one-slot mark and wake the dispatcher, so they are
    /// admitted even when the job queue and the event queue are both full,
    /// and repeated clicks on Stop cost nothing. A cancel or stop for an ID
    /// that is not live (unknown, finished or superseded) is acknowledged
    /// with `Ok` and has no effect.
    /// Rejected: an unbounded control channel, which a stuck consumer would
    /// let grow without limit, and a bounded one, which could refuse the very
    /// request that unblocks the engine.
    ///
    /// # Errors
    ///
    /// [`SubmitError::Busy`] when the job queue is full (never for a
    /// cancellation); [`SubmitError::ScanMismatch`] when `BuildCleanupPlan`
    /// names a scan the review service does not hold;
    /// [`SubmitError::NotImplemented`] when a synchronous precondition's
    /// module is a stub; [`SubmitError::Stopped`] when the dispatcher has
    /// exited.
    pub fn submit(&mut self, request: EngineRequest) -> Result<Origin, SubmitError> {
        match request {
            EngineRequest::StartScan(request) => {
                let scan = ScanId::new(self.next_scan);
                let generation = self.active.reserve();
                self.enqueue(Job::StartScan {
                    scan,
                    generation,
                    request,
                })?;
                self.active.activate(generation);
                self.next_scan += 1;
                self.highest_seen_scan = scan.get();
                self.current_scan = Some(scan);
                self.live_scan = Some(scan);
                Ok(Origin::Scan(scan))
            }
            EngineRequest::CancelScan(scan) => {
                if self.live_scan == Some(scan) {
                    self.signal(|pending| pending.cancel = Some(scan))?;
                }
                Ok(Origin::Scan(scan))
            }
            EngineRequest::LoadComparison(comparison) => {
                let request = RequestId::new(self.next_request);
                self.enqueue(Job::LoadComparison {
                    request,
                    comparison,
                })?;
                self.next_request += 1;
                Ok(Origin::Request(request))
            }
            EngineRequest::BuildCleanupPlan(BuildCleanupPlan { scan }) => {
                let validation = self.plan_basis(scan)?;
                let plan = self.plan_revisions.allocate();
                self.enqueue(Job::BuildCleanupPlan { plan, validation })?;
                self.plan_revisions.activate(plan);
                Ok(Origin::Plan(plan))
            }
            EngineRequest::ExecuteApprovedPlan(ExecuteApprovedPlan { plan }) => {
                self.enqueue(Job::ExecuteApprovedPlan(plan))?;
                Ok(Origin::Plan(plan))
            }
            EngineRequest::StopCleanup(plan) => {
                if self.plan_revisions.active() == Some(plan) {
                    self.signal(|pending| pending.stop = Some(plan))?;
                }
                Ok(Origin::Plan(plan))
            }
        }
    }

    /// Applies one review command synchronously (C-05), then republishes the
    /// validation view.
    ///
    /// A command that names a comparison (`reviewed`) is resolved against
    /// the engine's evidence store first. It is refused, before any policy
    /// runs, with `EvidenceUnavailable` when the request is unknown or from
    /// another scan, or when a Compare queue request or Keep both names none,
    /// and with `EvidenceMismatch` when the stored pair is not the command's
    /// pair.
    pub fn review(&mut self, command: ReviewCommand) -> ReviewOutcome {
        let reviewed = match self.resolve_evidence(&command) {
            Ok(reviewed) => reviewed,
            Err(refusal) => return refusal,
        };
        let review = &mut self.review;
        self.validation.transact(|| {
            let outcome = review.apply(command, reviewed);
            (outcome, review.validation_view().ok())
        })
    }

    /// Takes at most `max` waiting events without blocking.
    ///
    /// A `Completed` or `Partial` result of the current scan is installed
    /// into the review service before it is returned, so a review command
    /// issued after this call already sees the new results.
    pub fn drain_events(&mut self, max: usize) -> Vec<EngineEvent> {
        let Some(events) = &self.events else {
            return Vec::new();
        };
        let mut batch = Vec::new();
        while batch.len() < max {
            match events.try_recv() {
                Ok(event) => batch.push(event),
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
            }
        }
        for event in &batch {
            if let EngineEvent::Scan {
                scan,
                event: ScanEvent::Completed(results) | ScanEvent::Partial(results),
            } = event
                && Some(*scan) == self.current_scan
            {
                if self.live_scan == Some(*scan) {
                    self.live_scan = None;
                }
                self.load_review(Arc::clone(results));
            }
            if let EngineEvent::Cleanup {
                plan,
                event: CleanupEvent::RunEnded { .. },
            } = event
            {
                self.plan_revisions.retire(*plan);
            }
        }
        batch
    }

    /// Makes `scenario` the current result set without running a scan: the
    /// developer and snapshot-test path to a fixture scenario. It starts a
    /// new scenario generation, so a scan still running cannot publish over
    /// it, and that scan is no longer live. Returns the scenario's scan ID,
    /// which becomes current.
    ///
    /// # Errors
    ///
    /// [`AdoptError::ScanIdReused`] when the scenario's scan ID is not above
    /// every ID this engine has started or adopted. A reused ID would let a
    /// late `Completed` of the earlier scan replace the adopted results;
    /// build the scenario with [`Engine::reserve_scan_id`] instead.
    pub fn adopt_scenario(&mut self, scenario: Arc<Scenario>) -> Result<ScanId, AdoptError> {
        let scan = scenario.results.scan;
        if scan.get() <= self.highest_seen_scan {
            return Err(AdoptError::ScanIdReused { scan });
        }
        self.highest_seen_scan = scan.get();
        self.next_scan = self.next_scan.max(scan.get().saturating_add(1));
        self.current_scan = Some(scan);
        self.live_scan = None;
        self.load_review(Arc::clone(&scenario.results));
        let generation = self.active.begin();
        // Cannot fail: no generation newer than the one just begun exists.
        let _ = self.active.publish(generation, scenario);
        Ok(scan)
    }

    /// A scan ID no scan has used, for building a scenario to adopt. It stays
    /// adoptable until a later scan is started or adopted.
    pub fn reserve_scan_id(&mut self) -> ScanId {
        let scan = ScanId::new(self.next_scan);
        self.next_scan += 1;
        scan
    }

    /// The shared plan-revision allocator.
    #[must_use]
    pub const fn plan_revisions(&self) -> &PlanRevisions {
        &self.plan_revisions
    }

    /// The scenario publication fence, for scan jobs and tests.
    #[must_use]
    pub const fn active(&self) -> &ActiveScenario {
        &self.active
    }

    /// The newest published validation view.
    #[must_use]
    pub const fn validation(&self) -> &ValidationBoard {
        &self.validation
    }

    /// Cancellations and stops admitted but not yet taken by the dispatcher.
    #[must_use]
    pub fn pending_controls(&self) -> usize {
        lock(&self.control).len()
    }

    /// Installs results and publishes the resulting view in one critical
    /// section. A stub review service publishes nothing; any real answer,
    /// including "no scan loaded", replaces the board's view.
    fn load_review(&mut self, results: Arc<ScanResults>) {
        self.review_scan = Some(results.scan);
        let review = &mut self.review;
        self.validation.transact(|| {
            review.load_scan(results);
            ((), review.validation_view().ok())
        });
    }

    /// The scan-checked review state a new plan is built from.
    fn plan_basis(&self, scan: ScanId) -> Result<PublishedValidation, SubmitError> {
        let mismatch = |current| SubmitError::ScanMismatch {
            requested: scan,
            current,
        };
        if self.review_scan != Some(scan) {
            return Err(mismatch(self.review_scan));
        }
        let view = self
            .review
            .validation_view()
            .map_err(SubmitError::NotImplemented)?
            .ok_or_else(|| mismatch(None))?;
        if view.scan != scan {
            return Err(mismatch(Some(view.scan)));
        }
        self.validation
            .publish(Some(view))
            .ok_or_else(|| mismatch(None))
    }

    fn resolve_evidence(
        &self,
        command: &ReviewCommand,
    ) -> Result<Option<ReviewedPair>, ReviewOutcome> {
        let (file, counterpart, request, required) = match command {
            ReviewCommand::RequestQueue {
                file,
                counterpart,
                origin,
                reviewed,
            } => (
                *file,
                *counterpart,
                *reviewed,
                *origin == QueueOrigin::Compare,
            ),
            ReviewCommand::AcknowledgeUniqueText {
                file,
                counterpart,
                reviewed,
            } => (*file, *counterpart, *reviewed, false),
            ReviewCommand::KeepBoth {
                file,
                counterpart,
                reviewed,
            } => (*file, *counterpart, Some(*reviewed), true),
            _ => return Ok(None),
        };
        let refuse = |refusal| Err(ReviewOutcome::refused(file, refusal));
        let Some(request) = request else {
            return if required {
                refuse(Refusal::EvidenceUnavailable)
            } else {
                Ok(None)
            };
        };
        let Some(pair) = self
            .evidence
            .get(request)
            .filter(|pair| Some(pair.scan) == self.review_scan)
        else {
            return refuse(Refusal::EvidenceUnavailable);
        };
        if (pair.target, pair.counterpart) != (file, counterpart) {
            return refuse(Refusal::EvidenceMismatch);
        }
        Ok(Some(pair))
    }

    fn enqueue(&self, job: Job) -> Result<(), SubmitError> {
        let jobs = self.jobs.as_ref().ok_or(SubmitError::Stopped)?;
        jobs.try_send(job).map_err(|e| match e {
            TrySendError::Full(_) => SubmitError::Busy,
            TrySendError::Disconnected(_) => SubmitError::Stopped,
        })
    }

    fn signal(&self, mark: impl FnOnce(&mut PendingControl)) -> Result<(), SubmitError> {
        let wake = self.wake.as_ref().ok_or(SubmitError::Stopped)?;
        mark(&mut lock(&self.control));
        match wake.try_send(()) {
            Ok(()) | Err(TrySendError::Full(())) => Ok(()),
            Err(TrySendError::Disconnected(())) => Err(SubmitError::Stopped),
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        // Dropping the job and wake senders ends the dispatcher's loop, which
        // never blocks on the event queue, so the join cannot hang.
        self.events = None;
        self.jobs = None;
        self.wake = None;
        if let Some(dispatcher) = self.dispatcher.take() {
            // A panicked dispatcher has nothing left to clean up.
            let _ = dispatcher.join();
        }
    }
}

/// Refusal notices waiting for room in the event queue. The backlog persists
/// across passes and coalesces by origin. Job notices are only added below
/// this limit, because at it the dispatcher stops taking jobs (backpressure
/// reaches `submit` as `Busy`); control notices can add at most one per
/// control slot. So the backlog never exceeds `NOTICE_BACKLOG + 2`, however
/// long the app leaves the event queue full.
const NOTICE_BACKLOG: usize = 32;

/// How often a dispatcher with undelivered notices retries the event queue.
const NOTICE_RETRY: Duration = Duration::from_millis(5);

struct Dispatcher {
    context: ModuleContext,
    jobs: Receiver<Job>,
    wake: Receiver<()>,
    control: Arc<Mutex<PendingControl>>,
    /// The dispatcher's own notices, delivered with `try_send` only. Repeats
    /// for one origin coalesce.
    backlog: VecDeque<EngineEvent>,
}

/// The review service [`Engine`] drives. Unit tests swap in a switch that
/// forwards to [`ReviewService`] by default and can hold a test double
/// instead; it has the same three calls. Rejected: a trait object, which
/// would put dynamic dispatch on the production path and tie `Engine`'s
/// `Send` and `Sync` to the bounds written on it rather than to T7's type.
#[cfg(not(test))]
type Review = ReviewService;
#[cfg(test)]
type Review = tests::doubles::ReviewSwitch;

/// The dispatcher's view of the module services: one call per job and per
/// control, with the C-02 shapes of `scan_sim`, `compare` and `cleanup_sim`.
/// Private, so it is a test seam and not a contract the modules implement;
/// [`Services`] forwards to them.
trait Modules {
    fn start_scan(
        &mut self,
        scan: ScanId,
        generation: ScenarioGeneration,
        request: StartScan,
    ) -> Result<(), NotImplemented>;
    fn cancel_scan(&mut self, scan: ScanId) -> Result<(), NotImplemented>;
    fn load_comparison(
        &mut self,
        request: RequestId,
        comparison: ComparisonRequest,
    ) -> Result<(), NotImplemented>;
    fn build_plan(
        &mut self,
        plan: PlanRevision,
        validation: PublishedValidation,
    ) -> Result<(), NotImplemented>;
    fn execute_plan(&mut self, plan: PlanRevision) -> Result<(), NotImplemented>;
    fn stop_plan(&mut self, plan: PlanRevision) -> Result<(), NotImplemented>;
}

/// The module services the dispatcher drives.
struct Services {
    scan: ScanService,
    compare: CompareService,
    cleanup: CleanupService,
}

impl Services {
    fn new(context: &ModuleContext) -> Self {
        Self {
            scan: ScanService::new(context),
            compare: CompareService::new(context),
            cleanup: CleanupService::new(context),
        }
    }
}

impl Modules for Services {
    fn start_scan(
        &mut self,
        scan: ScanId,
        generation: ScenarioGeneration,
        request: StartScan,
    ) -> Result<(), NotImplemented> {
        self.scan.start(scan, generation, request)
    }

    fn cancel_scan(&mut self, scan: ScanId) -> Result<(), NotImplemented> {
        self.scan.cancel(scan)
    }

    fn load_comparison(
        &mut self,
        request: RequestId,
        comparison: ComparisonRequest,
    ) -> Result<(), NotImplemented> {
        self.compare.load(request, comparison)
    }

    fn build_plan(
        &mut self,
        plan: PlanRevision,
        validation: PublishedValidation,
    ) -> Result<(), NotImplemented> {
        self.cleanup.build(plan, validation)
    }

    fn execute_plan(&mut self, plan: PlanRevision) -> Result<(), NotImplemented> {
        self.cleanup.execute(plan)
    }

    fn stop_plan(&mut self, plan: PlanRevision) -> Result<(), NotImplemented> {
        self.cleanup.stop(plan)
    }
}

/// What one wait of the dispatcher produced.
enum Wake {
    Control,
    Job(Job),
    Retry,
    Closed,
}

impl Dispatcher {
    fn run<M: Modules>(mut self, modules: impl FnOnce(&ModuleContext) -> M) {
        let mut services = modules(&self.context);
        while self.pass(&mut services, true) {}
    }

    /// One dispatcher pass, which never blocks on the event queue.
    ///
    /// Fairness rule: a pass first takes every pending control, which is at
    /// most one cancel and one stop (the control budget, bounded by the
    /// one-slot `PendingControl`), then services at most one ready job
    /// before it may wait. However fast cancellations are replenished, each
    /// pass still takes a job, so jobs cannot starve; and a cancellation
    /// waits at most one job handler, plus `NOTICE_RETRY` when the event
    /// queue is full, before it is taken. With `block` false the pass never
    /// waits (tests drive passes one at a time). Returns `false` once the
    /// engine is gone.
    fn pass(&mut self, services: &mut impl Modules, block: bool) -> bool {
        if !self.flush() {
            return false;
        }
        let pending = std::mem::take(&mut *lock(&self.control));
        if let Some(id) = pending.cancel {
            let result = services.cancel_scan(id);
            self.note(Origin::Scan(id), result);
        }
        if let Some(plan) = pending.stop {
            let result = services.stop_plan(plan);
            self.note(Origin::Plan(plan), result);
        }
        if self.backlog.len() < NOTICE_BACKLOG {
            match self.jobs.try_recv() {
                Ok(job) => {
                    self.handle(services, job);
                    return true;
                }
                Err(TryRecvError::Disconnected) => return false,
                Err(TryRecvError::Empty) => {}
            }
        }
        if !block {
            return true;
        }
        match self.wait() {
            Wake::Closed => false,
            Wake::Control | Wake::Retry => true,
            Wake::Job(job) => {
                self.handle(services, job);
                true
            }
        }
    }

    fn handle(&mut self, services: &mut impl Modules, job: Job) {
        match job {
            Job::StartScan {
                scan,
                generation,
                request,
            } => {
                let result = services.start_scan(scan, generation, request);
                self.note(Origin::Scan(scan), result);
            }
            Job::LoadComparison {
                request,
                comparison,
            } => {
                let result = services.load_comparison(request, comparison);
                self.note(Origin::Request(request), result);
            }
            Job::BuildCleanupPlan { plan, validation } => {
                let result = services.build_plan(plan, validation);
                self.note(Origin::Plan(plan), result);
            }
            Job::ExecuteApprovedPlan(plan) => {
                let result = services.execute_plan(plan);
                self.note(Origin::Plan(plan), result);
            }
        }
    }

    fn wait(&self) -> Wake {
        let wake = |m: Result<(), _>| {
            if m.is_ok() {
                Wake::Control
            } else {
                Wake::Closed
            }
        };
        let job = |m: Result<Job, _>| m.map_or(Wake::Closed, Wake::Job);
        if self.backlog.len() >= NOTICE_BACKLOG {
            select_biased! {
                recv(self.wake) -> m => wake(m),
                default(NOTICE_RETRY) => Wake::Retry,
            }
        } else if self.backlog.is_empty() {
            select_biased! {
                recv(self.wake) -> m => wake(m),
                recv(self.jobs) -> m => job(m),
            }
        } else {
            select_biased! {
                recv(self.wake) -> m => wake(m),
                recv(self.jobs) -> m => job(m),
                default(NOTICE_RETRY) => Wake::Retry,
            }
        }
    }

    fn note(&mut self, origin: Origin, result: Result<(), NotImplemented>) {
        let Err(missing) = result else {
            return;
        };
        if !self.backlog.iter().any(|e| e.origin() == origin) {
            self.backlog
                .push_back(EngineEvent::NotImplemented { origin, missing });
        }
    }

    /// Delivers queued notices until the event queue is full. Returns
    /// `false` once the engine is gone.
    fn flush(&mut self) -> bool {
        while let Some(event) = self.backlog.pop_front() {
            match self.context.events.try_send(event) {
                Ok(()) => {}
                Err(TrySendError::Full(event)) => {
                    self.backlog.push_front(event);
                    return true;
                }
                Err(TrySendError::Disconnected(_)) => return false,
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    //! Facade properties proved over test doubles (`doubles`), never over a
    //! module stub's answers: T5 to T8 replace the stubs and must not turn
    //! these tests red, and D-004 forbids them to edit this file.

    use std::collections::BTreeSet;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Instant;

    use fuzzy_dedupe_domain::{CompareMode, FileId, RulesRevision, SnapshotId};

    use self::doubles::{Call, CallLog, FakeModules, FakeReview, ReviewSwitch, ViewScan};
    use super::*;
    use crate::fixtures::{self, DEFAULT_SEED};

    /// Deterministic stand-ins for the module services and the review
    /// service, reached through the private `Modules` and `Review` seams.
    pub(super) mod doubles {
        use std::collections::BTreeSet;
        use std::time::{Duration, Instant};

        use fuzzy_dedupe_domain::RulesRevision;

        use super::super::*;

        /// One call the dispatcher made, in order.
        #[derive(Clone, Debug, PartialEq, Eq)]
        pub enum Call {
            Start(ScanId),
            Cancel(ScanId),
            Load(RequestId),
            Build(PlanRevision, PublishedValidation),
            Execute(PlanRevision),
            Stop(PlanRevision),
        }

        /// The calls a [`FakeModules`] received, shared with the test thread
        /// while the fake lives on the dispatcher thread.
        #[derive(Clone, Debug, Default)]
        pub struct CallLog(Arc<Mutex<Vec<Call>>>);

        impl CallLog {
            pub fn calls(&self) -> Vec<Call> {
                lock(&self.0).clone()
            }

            /// Waits up to `within` for a call matching `wanted`.
            pub fn wait_for(&self, within: Duration, wanted: impl Fn(&Call) -> bool) -> bool {
                let deadline = Instant::now() + within;
                while Instant::now() < deadline {
                    if lock(&self.0).iter().any(&wanted) {
                        return true;
                    }
                    std::thread::yield_now();
                }
                false
            }
        }

        /// Records every call and answers each with `answer`. `Err` makes it
        /// refuse like a stub does, which is the only way to drive the
        /// dispatcher's notice backlog; `Ok` makes it behave like a module
        /// whose work runs elsewhere and emits nothing on the dispatcher.
        pub struct FakeModules {
            pub log: CallLog,
            pub answer: Result<(), NotImplemented>,
        }

        impl FakeModules {
            fn record(&self, call: Call) -> Result<(), NotImplemented> {
                lock(&self.log.0).push(call);
                self.answer
            }
        }

        impl Modules for FakeModules {
            fn start_scan(
                &mut self,
                scan: ScanId,
                _generation: ScenarioGeneration,
                _request: StartScan,
            ) -> Result<(), NotImplemented> {
                self.record(Call::Start(scan))
            }

            fn cancel_scan(&mut self, scan: ScanId) -> Result<(), NotImplemented> {
                self.record(Call::Cancel(scan))
            }

            fn load_comparison(
                &mut self,
                request: RequestId,
                _comparison: ComparisonRequest,
            ) -> Result<(), NotImplemented> {
                self.record(Call::Load(request))
            }

            fn build_plan(
                &mut self,
                plan: PlanRevision,
                validation: PublishedValidation,
            ) -> Result<(), NotImplemented> {
                self.record(Call::Build(plan, validation))
            }

            fn execute_plan(&mut self, plan: PlanRevision) -> Result<(), NotImplemented> {
                self.record(Call::Execute(plan))
            }

            fn stop_plan(&mut self, plan: PlanRevision) -> Result<(), NotImplemented> {
                self.record(Call::Stop(plan))
            }
        }

        /// A review double: it records what `Engine::review` handed to
        /// policy and publishes an empty view of the loaded scan, like a
        /// correct review service with no decisions.
        #[derive(Debug, Default)]
        pub struct FakeReview {
            pub loaded: Option<ScanId>,
            pub applied: Vec<(ReviewCommand, Option<ReviewedPair>)>,
            pub view: ViewScan,
        }

        /// Which scan the published view claims. The non-default answers
        /// stand for review state that disagrees with the facade.
        #[derive(Clone, Copy, Debug, Default)]
        pub enum ViewScan {
            #[default]
            Loaded,
            Other(ScanId),
            Withdrawn,
        }

        /// The engine's `Review` in test builds: the real service unless a
        /// test installs a [`FakeReview`].
        #[derive(Debug)]
        pub enum ReviewSwitch {
            Real(ReviewService),
            Fake(Arc<Mutex<FakeReview>>),
        }

        impl ReviewSwitch {
            pub const fn new() -> Self {
                Self::Real(ReviewService::new())
            }

            pub fn load_scan(&mut self, results: Arc<ScanResults>) {
                match self {
                    Self::Real(review) => review.load_scan(results),
                    Self::Fake(fake) => lock(fake).loaded = Some(results.scan),
                }
            }

            pub fn apply(
                &mut self,
                command: ReviewCommand,
                reviewed: Option<ReviewedPair>,
            ) -> ReviewOutcome {
                match self {
                    Self::Real(review) => review.apply(command, reviewed),
                    Self::Fake(fake) => {
                        lock(fake).applied.push((command, reviewed));
                        ReviewOutcome {
                            status: ReviewStatus::Applied,
                            changed: Vec::new(),
                            invalidated: Vec::new(),
                        }
                    }
                }
            }

            pub fn validation_view(&self) -> Result<Option<ValidationView>, NotImplemented> {
                match self {
                    Self::Real(review) => review.validation_view(),
                    Self::Fake(fake) => {
                        let fake = lock(fake);
                        let scan = match fake.view {
                            ViewScan::Loaded => fake.loaded,
                            ViewScan::Other(scan) => Some(scan),
                            ViewScan::Withdrawn => None,
                        };
                        Ok(scan.map(|scan| ValidationView {
                            scan,
                            queue: Vec::new(),
                            protected: BTreeSet::new(),
                            snapshots: Arc::default(),
                            rules: RulesRevision::new(1),
                        }))
                    }
                }
            }
        }
    }

    /// Long enough for a loaded CI runner; a passing test never waits it.
    const PATIENCE: Duration = Duration::from_secs(10);

    /// An engine over [`FakeModules`] answering `answer`, and its call log.
    fn engine(config: EngineConfig, answer: Result<(), NotImplemented>) -> (Engine, CallLog) {
        let log = CallLog::default();
        let modules = FakeModules {
            log: log.clone(),
            answer,
        };
        let engine = Engine::with_modules(config, move |_| modules).expect("engine starts");
        (engine, log)
    }

    fn fake_review(engine: &mut Engine) -> Arc<Mutex<FakeReview>> {
        let fake = Arc::default();
        engine.review = ReviewSwitch::Fake(Arc::clone(&fake));
        fake
    }

    /// The stub answer the backlog tests need from a double.
    const REFUSE: Result<(), NotImplemented> = Err(NotImplemented::new(EngineModule::ScanSim));

    fn start_scan() -> EngineRequest {
        let mixed =
            fixtures::build(ScenarioKind::Mixed, ScanId::new(1), DEFAULT_SEED).expect("mixed");
        EngineRequest::StartScan(StartScan {
            setup: mixed.results.setup.clone(),
            rules: mixed.results.rules,
            scenario: ScenarioKind::Mixed,
            seed: DEFAULT_SEED,
        })
    }

    fn adopt_mixed(engine: &mut Engine, scan: ScanId) -> ScanId {
        let scenario = fixtures::build(ScenarioKind::Mixed, scan, DEFAULT_SEED).expect("mixed");
        engine
            .adopt_scenario(Arc::new(scenario))
            .expect("fresh scan ID")
    }

    fn pair(scan: ScanId, target: FileId, counterpart: FileId) -> ReviewedPair {
        ReviewedPair {
            scan,
            target,
            target_snapshot: SnapshotId::new(target.get()),
            counterpart,
            counterpart_snapshot: SnapshotId::new(counterpart.get()),
            rules: RulesRevision::new(1),
            mode: CompareMode::Original,
            unique_lines: 3,
        }
    }

    /// The trusted route end to end: evidence the comparison service stored
    /// reaches policy as the resolved pair; a mismatched pair, another
    /// scan's evidence or a Compare request naming none is refused before
    /// policy runs at all.
    #[test]
    fn review_resolves_only_stored_matching_evidence() {
        let (mut engine, _) = engine(EngineConfig::default(), Ok(()));
        let review = fake_review(&mut engine);
        let scan = adopt_mixed(&mut engine, ScanId::new(3));
        let (a, b, c) = (FileId::new(1), FileId::new(2), FileId::new(3));
        engine.evidence.record(RequestId::new(1), pair(scan, a, b));
        engine
            .evidence
            .record(RequestId::new(2), pair(ScanId::new(99), a, b));

        let keep = |counterpart, reviewed| ReviewCommand::KeepBoth {
            file: a,
            counterpart,
            reviewed: RequestId::new(reviewed),
        };
        assert_eq!(engine.review(keep(b, 1)).status, ReviewStatus::Applied);
        let refused = |refusal| ReviewStatus::Refused { file: a, refusal };
        assert_eq!(
            engine.review(keep(c, 1)).status,
            refused(Refusal::EvidenceMismatch)
        );
        assert_eq!(
            engine.review(keep(b, 2)).status,
            refused(Refusal::EvidenceUnavailable)
        );
        let queue = |origin| ReviewCommand::RequestQueue {
            file: a,
            counterpart: b,
            origin,
            reviewed: None,
        };
        assert_eq!(
            engine.review(queue(QueueOrigin::Compare)).status,
            refused(Refusal::EvidenceUnavailable)
        );
        assert_eq!(
            engine.review(queue(QueueOrigin::Checkbox)).status,
            ReviewStatus::Applied
        );

        assert_eq!(
            lock(&review).applied,
            [
                (keep(b, 1), Some(pair(scan, a, b))),
                (queue(QueueOrigin::Checkbox), None),
            ],
            "only the matching pair and the Findings request reached policy"
        );
    }

    /// Provenance is checked before anything is enqueued: no refused
    /// request reaches the cleanup service or spends a plan revision, and
    /// the admitted one carries the view the engine published.
    #[test]
    fn build_cleanup_plan_rejects_a_mismatched_scan_before_enqueueing() {
        let (mut engine, log) = engine(EngineConfig::default(), Ok(()));
        let review = fake_review(&mut engine);
        let build = |scan| EngineRequest::BuildCleanupPlan(BuildCleanupPlan { scan });
        let mismatch = |requested, current| Err(SubmitError::ScanMismatch { requested, current });
        let (scan, other) = (ScanId::new(3), ScanId::new(4));

        assert_eq!(engine.submit(build(scan)), mismatch(scan, None));
        adopt_mixed(&mut engine, scan);
        assert_eq!(engine.submit(build(other)), mismatch(other, Some(scan)));
        lock(&review).view = ViewScan::Other(other);
        assert_eq!(engine.submit(build(scan)), mismatch(scan, Some(other)));
        // The engine's own record of the loaded scan decides even when the
        // review state claims the requested one.
        assert_eq!(engine.submit(build(other)), mismatch(other, Some(scan)));
        lock(&review).view = ViewScan::Withdrawn;
        assert_eq!(engine.submit(build(scan)), mismatch(scan, None));

        lock(&review).view = ViewScan::Loaded;
        let Ok(Origin::Plan(plan)) = engine.submit(build(scan)) else {
            panic!("the reviewed scan's plan is admitted")
        };
        // Revision 1: the four refusals allocated none.
        assert_eq!(plan, PlanRevision::new(1));
        assert_eq!(engine.plan_revisions().active(), Some(plan));
        assert!(log.wait_for(PATIENCE, |c| matches!(c, Call::Build(..))));
        let published = engine.validation().current().expect("the plan's view");
        assert_eq!(published.view.scan, scan);
        assert_eq!(
            log.calls(),
            [Call::Build(plan, published)],
            "only the admitted plan reached the cleanup service"
        );
    }

    #[test]
    fn a_full_request_queue_is_busy_and_cancellation_is_still_admitted() {
        // One-slot queues and no draining: the refusing double fills the
        // dispatcher's notice backlog, it stops taking jobs, the request
        // queue fills behind it, and the next submit must answer Busy
        // instead of blocking the caller.
        let (mut engine, log) = engine(
            EngineConfig {
                request_capacity: 1,
                event_capacity: 1,
                ..EngineConfig::default()
            },
            REFUSE,
        );
        let deadline = Instant::now() + PATIENCE;
        let mut last = None;
        loop {
            match engine.submit(start_scan()) {
                Ok(Origin::Scan(id)) => last = Some(id),
                Err(SubmitError::Busy) => break,
                other => panic!("unexpected submit result {other:?}"),
            }
            assert!(Instant::now() < deadline, "the request queue never filled");
        }
        let scan = last.expect("at least one scan was admitted");
        assert_eq!(
            engine.submit(EngineRequest::CancelScan(scan)),
            Ok(Origin::Scan(scan)),
            "cancellation is admitted while the request queue is full"
        );
        assert!(
            log.wait_for(PATIENCE, |c| *c == Call::Cancel(scan)),
            "the admitted cancellation reached the scan service"
        );
    }

    #[test]
    fn cancellation_is_processed_while_the_event_queue_stays_full() {
        let (mut engine, log) = engine(
            EngineConfig {
                request_capacity: 1,
                event_capacity: 1,
                ..EngineConfig::default()
            },
            REFUSE,
        );
        let deadline = Instant::now() + PATIENCE;
        let mut last = None;
        while engine
            .submit(start_scan())
            .map(|origin| last = Some(origin))
            .is_ok()
        {
            assert!(Instant::now() < deadline, "the queues never filled");
        }
        let Some(Origin::Scan(scan)) = last else {
            panic!("no scan admitted")
        };
        // Busy can come before the dispatcher has taken the admitted job and
        // queued its refusal, so wait for the queue to fill rather than
        // assume it; nothing drains it afterwards.
        let events = engine.events.clone().expect("engine running");
        while !events.is_full() {
            assert!(Instant::now() < deadline, "the event queue never filled");
            std::thread::yield_now();
        }
        for _ in 0..100 {
            assert_eq!(
                engine.submit(EngineRequest::CancelScan(scan)),
                Ok(Origin::Scan(scan)),
                "cancellation is always admitted"
            );
        }
        assert!(engine.pending_controls() <= 1, "repeated cancels coalesce");
        // The slot, not the log, is the deadline's measure: the dispatcher
        // may hand over an early cancel while later ones re-arm the slot.
        let taken_by = Instant::now() + Duration::from_secs(1);
        while engine.pending_controls() > 0 {
            assert!(
                Instant::now() < taken_by,
                "the dispatcher did not take the cancellation within 1 s"
            );
            std::thread::yield_now();
        }
        assert!(
            log.wait_for(PATIENCE, |c| *c == Call::Cancel(scan)),
            "the taken cancellation reached the scan service"
        );
        assert!(events.is_full(), "the event queue stayed full throughout");
    }

    #[test]
    fn a_live_cancellation_survives_a_flood_of_unknown_ones() {
        let settle = Duration::from_millis(300);
        let (mut engine, log) = engine(EngineConfig::default(), Ok(()));
        let Ok(Origin::Scan(live)) = engine.submit(start_scan()) else {
            panic!("StartScan admitted")
        };
        assert!(log.wait_for(PATIENCE, |c| *c == Call::Start(live)));
        assert_eq!(
            engine.submit(EngineRequest::CancelScan(live)),
            Ok(Origin::Scan(live))
        );
        for unknown in 1000..1020 {
            let id = ScanId::new(unknown);
            assert_eq!(
                engine.submit(EngineRequest::CancelScan(id)),
                Ok(Origin::Scan(id)),
                "unknown IDs are acknowledged harmlessly"
            );
        }
        assert!(engine.pending_controls() <= 1, "only the live scan is held");
        assert!(log.wait_for(PATIENCE, |c| *c == Call::Cancel(live)));
        std::thread::sleep(settle);
        let cancels: Vec<Call> = log
            .calls()
            .into_iter()
            .filter(|c| matches!(c, Call::Cancel(_)))
            .collect();
        assert_eq!(
            cancels,
            [Call::Cancel(live)],
            "no unknown cancel reached it"
        );

        // The same for StopCleanup, with a plan made live through the shared
        // allocator, as T8 does for a revised plan.
        let plan = engine.plan_revisions().allocate();
        engine.plan_revisions().activate(plan);
        assert_eq!(
            engine.submit(EngineRequest::StopCleanup(plan)),
            Ok(Origin::Plan(plan))
        );
        for unknown in 1000..1020 {
            let id = PlanRevision::new(unknown);
            assert_eq!(
                engine.submit(EngineRequest::StopCleanup(id)),
                Ok(Origin::Plan(id))
            );
        }
        assert!(log.wait_for(PATIENCE, |c| *c == Call::Stop(plan)));
        std::thread::sleep(settle);
        let stops: Vec<Call> = log
            .calls()
            .into_iter()
            .filter(|c| matches!(c, Call::Stop(_)))
            .collect();
        assert_eq!(stops, [Call::Stop(plan)], "only the live stop reached it");
    }

    #[test]
    fn evidence_store_is_bounded_and_latest_wins() {
        let store = ComparisonEvidence::default();
        let scan = ScanId::new(1);
        for id in 0..(EVIDENCE_SLOTS as u64 + 5) {
            store.record(
                RequestId::new(id),
                pair(scan, FileId::new(id), FileId::new(0)),
            );
        }
        assert!(store.get(RequestId::new(0)).is_none(), "oldest evicted");
        assert!(store.get(RequestId::new(EVIDENCE_SLOTS as u64)).is_some());
        let replaced = pair(scan, FileId::new(7), FileId::new(8));
        store.record(RequestId::new(10), replaced);
        assert_eq!(store.get(RequestId::new(10)), Some(replaced));
        assert_eq!(lock(&store.0).len(), EVIDENCE_SLOTS);
    }

    #[test]
    fn validation_board_revisions_increase_and_can_withdraw() {
        let board = ValidationBoard::default();
        let view = ValidationView {
            scan: ScanId::new(1),
            queue: Vec::new(),
            protected: BTreeSet::new(),
            snapshots: Arc::default(),
            rules: RulesRevision::new(1),
        };
        let first = board.publish(Some(view.clone())).expect("published");
        let second = board.publish(Some(view)).expect("published");
        assert!(second.revision > first.revision);
        assert_eq!(board.current(), Some(second));
        assert!(board.publish(None).is_none());
        assert!(board.current().is_none());
    }

    #[test]
    fn a_review_change_never_interleaves_with_a_validated_commit() {
        // `applied` stands for review state the policy mutates in two steps
        // (a change, then the view that describes it). Every commit must see
        // a view that matches the state exactly: old or new, never a mix.
        let board = ValidationBoard::default();
        let applied = Arc::new(AtomicU64::new(0));
        let view = |rules: u64| ValidationView {
            scan: ScanId::new(1),
            queue: Vec::new(),
            protected: BTreeSet::new(),
            snapshots: Arc::default(),
            rules: RulesRevision::new(rules),
        };
        board.publish(Some(view(0)));
        let (writer_board, writer_state) = (board.clone(), Arc::clone(&applied));
        let writer = std::thread::spawn(move || {
            for _ in 0..2_000 {
                writer_board.transact(|| {
                    let now = writer_state.fetch_add(1, Ordering::SeqCst) + 1;
                    std::thread::yield_now();
                    ((), Some(Some(view(now))))
                });
            }
        });
        let mut commits = 0;
        while !writer.is_finished() || commits == 0 {
            let seen = board.validate_and_commit(|published| {
                (published.view.rules.get(), applied.load(Ordering::SeqCst))
            });
            let (_, (published, state)) = seen.expect("a view is published");
            assert_eq!(published, state, "commit saw a half-applied review change");
            commits += 1;
        }
        writer.join().expect("writer");
    }

    fn dispatcher(
        event_capacity: usize,
        answer: Result<(), NotImplemented>,
    ) -> (Dispatcher, FakeModules, Sender<Job>, Receiver<EngineEvent>) {
        let (jobs_tx, jobs) = crossbeam_channel::bounded(128);
        let (_, wake) = crossbeam_channel::bounded(1);
        let (events_tx, events) = crossbeam_channel::bounded(event_capacity);
        let context = ModuleContext {
            events: EventSink(events_tx),
            active: ActiveScenario::default(),
            comparisons: ComparisonEvidence::default(),
            validation: ValidationBoard::default(),
            plan_revisions: PlanRevisions::default(),
            worker_threads: 1,
        };
        let modules = FakeModules {
            log: CallLog::default(),
            answer,
        };
        let dispatcher = Dispatcher {
            context,
            jobs,
            wake,
            control: Arc::default(),
            backlog: VecDeque::new(),
        };
        (dispatcher, modules, jobs_tx, events)
    }

    fn comparison_job(id: u64) -> Job {
        Job::LoadComparison {
            request: RequestId::new(id),
            comparison: ComparisonRequest {
                scan: ScanId::new(1),
                target: FileId::new(1),
                counterpart: FileId::new(2),
                target_snapshot: SnapshotId::new(1),
                counterpart_snapshot: SnapshotId::new(2),
                rules: RulesRevision::new(1),
                mode: CompareMode::Original,
            },
        }
    }

    #[test]
    fn replenished_controls_do_not_starve_jobs() {
        let (mut d, mut modules, jobs, _events) = dispatcher(1024, Ok(()));
        for id in 0..10 {
            jobs.send(comparison_job(id)).expect("queued");
        }
        for _ in 0..10 {
            lock(&d.control).cancel = Some(ScanId::new(1));
            lock(&d.control).stop = Some(PlanRevision::new(1));
            assert!(d.pass(&mut modules, false));
        }
        assert!(
            jobs.is_empty(),
            "every pass serviced one job despite controls"
        );
        let calls = modules.log.calls();
        let count = |wanted: fn(&Call) -> bool| calls.iter().filter(|c| wanted(c)).count();
        assert_eq!(count(|c| matches!(c, Call::Load(_))), 10);
        assert_eq!(count(|c| matches!(c, Call::Cancel(_))), 10, "controls too");
        assert_eq!(count(|c| matches!(c, Call::Stop(_))), 10);
    }

    #[test]
    fn notices_stay_bounded_across_passes_while_events_are_full() {
        let (mut d, mut modules, jobs, events) = dispatcher(1, REFUSE);
        for id in 0..100 {
            jobs.send(comparison_job(id)).expect("queued");
        }
        for _ in 0..200 {
            lock(&d.control).cancel = Some(ScanId::new(1));
            assert!(d.pass(&mut modules, false));
            assert!(
                d.backlog.len() <= NOTICE_BACKLOG + 2,
                "backlog {}",
                d.backlog.len()
            );
        }
        assert_eq!(events.len(), 1, "the app drained nothing");
        assert!(
            !jobs.is_empty(),
            "jobs stop being taken at the limit: backpressure"
        );
        assert!(
            d.backlog
                .iter()
                .any(|e| e.origin() == Origin::Scan(ScanId::new(1))),
            "the control notice is kept, coalesced"
        );
    }
}
