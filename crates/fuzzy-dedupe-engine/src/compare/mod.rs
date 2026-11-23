// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! Comparison service (plan C-04): `LoadComparison` in, one
//! [`EngineEvent::Comparison`] per current request out.
//!
//! The dispatcher in [`crate::service`] calls `new` once and `load` per
//! request. Those shapes and [`ComparisonRequest`] are the C-02 contract
//! and stay fixed. Text comes from the active scenario's in-memory buffers
//! ([`ActiveScenario`]), never from a file (plan D-005).
//!
//! | Module         | Holds                                                  |
//! | -------------- | ------------------------------------------------------ |
//! | [`aligned`]    | the aligned-row model and the §10.1 unique-line measure |
//! | [`normalize`]  | the Normalized view's line profile                     |
//! | [`texts`]      | the generated 5 MB pair and its scenario               |
//!
//! Threading: `load` only parks the request in a one-request slot and
//! returns, because it runs on the dispatcher thread, which must never wait
//! on a diff. One worker thread the service owns takes the slot, computes
//! and delivers. A newer `load` overwrites a waiting request, so a burst of
//! requests costs at most the one diff in progress plus the newest.
//!
//! Supersession: request IDs only grow, and the newest one `load` saw is
//! the current one. A request that stops being current is abandoned at the
//! next phase boundary of its computation and is never recorded or
//! delivered.
//!
//! Review evidence: once a ready result's event is in the queue, the worker
//! records a `ReviewedPair` (both snapshots, the rules revision, the mode
//! and the target's §10.1 Unique count) under the request ID with
//! `ModuleContext::comparisons`. The engine binds a Compare-originated
//! review command only to that record, so it must never exist for a result
//! that was not delivered while current; `Worker::deliver` explains the
//! short fail-closed window this ordering leaves.
//!
//! Freshness: a result is checked against the active scenario (scan, rules
//! revision, both snapshots) before the cache is used and again at
//! publication, so neither a cache hit nor a slow computation can publish
//! a comparison of text the active scenario no longer holds.

pub mod aligned;
pub mod normalize;
pub mod texts;

use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use crossbeam_channel::TrySendError;
use fuzzy_dedupe_domain::{
    CompareMode, FileId, RequestId, ReviewedPair, RulesRevision, ScanId, SnapshotId,
};

pub use aligned::{
    AlignedComparison, CopyText, Hunk, InlineSpans, LineSpan, Pane, PaneText, Row, RowKind,
    UniqueLines, measure_unique,
};

use crate::fixtures::Scenario;
use crate::service::{
    ActiveScenario, ComparisonEvidence, EngineEvent, EventSink, ModuleContext, NotImplemented,
};

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

/// One delivered comparison: the request it answers and its outcome.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Comparison {
    pub request: ComparisonRequest,
    pub outcome: ComparisonOutcome,
}

/// What a comparison produced.
#[derive(Debug, PartialEq, Eq)]
pub enum ComparisonOutcome {
    /// Side A is the target, side B the counterpart.
    Ready(Box<AlignedComparison>),
    /// Nothing can be shown. Presentation states the reason and never
    /// shows a blank pane as if the files were equal (§11.5 rule 9).
    Unavailable(Unavailable),
}

/// Why a comparison could not be computed. Each case fails closed: no
/// evidence is recorded for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Unavailable {
    /// No scenario is active yet.
    NoScenario,
    /// The request names a scan other than the active one.
    ScanMismatch { requested: ScanId, current: ScanId },
    /// The active scan has no such file, or holds no buffer for it.
    FileMissing(FileId),
    /// The request names an observation of `file` other than the one the
    /// active scenario's buffer is. Computing anyway would record evidence
    /// for a snapshot whose text was never compared.
    SnapshotMismatch {
        file: FileId,
        requested: SnapshotId,
        current: SnapshotId,
    },
    /// The request names rules other than the scan's own; Normalized mode
    /// would apply the wrong Ignore options.
    RulesMismatch {
        requested: RulesRevision,
        current: RulesRevision,
    },
    /// The buffer is not valid UTF-8, so no text can be shown for it.
    NotText(FileId),
    /// The worker thread could not be started.
    WorkerUnavailable,
}

/// Recent results the worker keeps, keyed by request. Toggling Original
/// and Normalized, or stepping back to the previous candidate, then costs
/// no second diff.
const CACHE_SLOTS: usize = 4;

/// Undelivered `WorkerUnavailable` answers kept while the app drains
/// nothing; beyond it the oldest is dropped, so a stalled app cannot grow
/// the service without bound.
const NOTICE_LIMIT: usize = 64;

/// How often a worker whose event did not fit retries the event queue.
const DELIVERY_RETRY: Duration = Duration::from_millis(5);

/// A poisoned lock still holds consistent data: each critical section is a
/// few assignments that cannot panic half-way.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[derive(Debug, Default)]
struct Slot {
    /// The newest request `load` saw: the only current one.
    latest: Option<RequestId>,
    /// A request waiting for the worker. A newer `load` replaces it.
    waiting: Option<(RequestId, ComparisonRequest)>,
    /// Set when the service is dropped; the worker stops at its next look.
    closed: bool,
}

#[derive(Debug, Default)]
struct Shared {
    slot: Mutex<Slot>,
    wake: Condvar,
}

impl Shared {
    fn is_current(&self, request: RequestId) -> bool {
        let slot = lock(&self.slot);
        !slot.closed && slot.latest == Some(request)
    }
}

/// Points where tests observe or steer the service. Compiled to nothing
/// outside `cfg(test)`; see `seam` below.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    /// `load` admitted the request (dispatcher thread, no lock held).
    Loaded,
    /// The worker is about to deliver (no lock held).
    BeforeDeliver,
    /// The result event entered the event queue.
    Enqueued,
    /// The event queue was full; the worker will retry.
    QueueFull,
    /// The worker is about to be spawned; a `false` answer makes the spawn
    /// fail.
    Spawn,
}

#[cfg(test)]
mod seam {
    use std::sync::{Arc, Mutex};

    use fuzzy_dedupe_domain::{RequestId, ScanId};

    use super::{Stage, lock};

    type Hook = Arc<dyn Fn(Stage, ScanId, RequestId) -> bool + Send + Sync>;

    /// One process-wide hook. Tests that install one hold `SERIAL` and
    /// filter on a scan ID no other test uses, because unit tests may share
    /// the process under `cargo test`.
    static HOOK: Mutex<Option<Hook>> = Mutex::new(None);
    pub static SERIAL: Mutex<()> = Mutex::new(());

    pub fn fire(stage: Stage, scan: ScanId, request: RequestId) -> bool {
        // Cloned out so a hook that blocks never holds `HOOK`.
        let hook = lock(&HOOK).clone();
        hook.is_none_or(|hook| hook(stage, scan, request))
    }

    pub fn install(hook: Option<Hook>) {
        *lock(&HOOK) = hook;
    }
}

#[cfg(test)]
use seam::fire;

#[cfg(not(test))]
#[inline]
const fn fire(_stage: Stage, _scan: ScanId, _request: RequestId) -> bool {
    true
}

/// The comparison service, owned by the dispatcher thread.
#[derive(Debug)]
pub struct CompareService {
    shared: Arc<Shared>,
    context: ModuleContext,
    /// Spawned by the first `load`, and retried by every later `load` while
    /// `None`. Never joined: a worker in the middle of a 5 MB diff would
    /// hold up engine shutdown, and it exits by itself once it sees
    /// `closed`.
    worker: Option<JoinHandle<()>>,
    /// `WorkerUnavailable` answers the event queue had no room for, oldest
    /// first. Each `load` delivers what fits before admitting its own
    /// request, so a request accepted while no worker ran is answered by
    /// the next `load` at the latest; until then the failure stays here
    /// rather than vanishing. At most `NOTICE_LIMIT`, oldest dropped first.
    notices: VecDeque<EngineEvent>,
}

impl CompareService {
    #[must_use]
    pub fn new(context: &ModuleContext) -> Self {
        Self {
            shared: Arc::new(Shared::default()),
            context: context.clone(),
            worker: None,
            notices: VecDeque::new(),
        }
    }

    fn spawn(&mut self, comparison: &ComparisonRequest, request: RequestId) {
        let worker = Worker {
            shared: Arc::clone(&self.shared),
            events: self.context.events.clone(),
            active: self.context.active.clone(),
            evidence: self.context.comparisons.clone(),
            cache: VecDeque::new(),
        };
        if !fire(Stage::Spawn, comparison.scan, request) {
            return;
        }
        self.worker = std::thread::Builder::new()
            .name("engine-compare".into())
            .spawn(move || worker.run())
            .ok();
    }

    /// Starts computing `comparison` for request `request`, superseding any
    /// earlier request. Returns at once; the result arrives as an
    /// [`EngineEvent::Comparison`] for `request`, unless a later request
    /// supersedes it first.
    ///
    /// # Errors
    ///
    /// Never: every request is implemented. The `Result` is the C-02 shape.
    pub fn load(
        &mut self,
        request: RequestId,
        comparison: ComparisonRequest,
    ) -> Result<(), NotImplemented> {
        if self.worker.is_none() {
            self.spawn(&comparison, request);
        }
        // Earlier answers go first, so the app sees them in request order.
        self.flush_notices();
        let mut slot = lock(&self.shared.slot);
        slot.latest = Some(request);
        if self.worker.is_some() {
            slot.waiting = Some((request, comparison));
            self.shared.wake.notify_one();
            drop(slot);
        } else {
            drop(slot);
            self.notices.push_back(event(
                request,
                Arc::new(Comparison {
                    request: comparison,
                    outcome: ComparisonOutcome::Unavailable(Unavailable::WorkerUnavailable),
                }),
            ));
            while self.notices.len() > NOTICE_LIMIT {
                self.notices.pop_front();
            }
            self.flush_notices();
        }
        fire(Stage::Loaded, comparison.scan, request);
        Ok(())
    }
}

impl CompareService {
    /// Delivers waiting notices with `try_send` (the dispatcher must not
    /// block) until the queue is full or the engine is gone.
    fn flush_notices(&mut self) {
        while let Some(notice) = self.notices.pop_front() {
            match self.context.events.try_send(notice) {
                Ok(()) => {}
                Err(TrySendError::Full(notice)) => {
                    self.notices.push_front(notice);
                    return;
                }
                Err(TrySendError::Disconnected(_)) => {
                    self.notices.clear();
                    return;
                }
            }
        }
    }
}

impl Drop for CompareService {
    fn drop(&mut self) {
        lock(&self.shared.slot).closed = true;
        self.shared.wake.notify_all();
    }
}

const fn event(request: RequestId, comparison: Arc<Comparison>) -> EngineEvent {
    EngineEvent::Comparison {
        request,
        comparison,
    }
}

struct Worker {
    shared: Arc<Shared>,
    events: EventSink,
    active: ActiveScenario,
    evidence: ComparisonEvidence,
    cache: VecDeque<Arc<Comparison>>,
}

impl Worker {
    fn run(mut self) {
        while let Some((request, comparison)) = self.next() {
            let Some(result) = self.result(request, comparison) else {
                continue;
            };
            if !self.deliver(request, &result) {
                return;
            }
        }
    }

    /// Waits for a request; `None` once the service is gone.
    fn next(&self) -> Option<(RequestId, ComparisonRequest)> {
        let mut slot = lock(&self.shared.slot);
        loop {
            if slot.closed {
                return None;
            }
            if let Some(job) = slot.waiting.take() {
                return Some(job);
            }
            slot = self
                .shared
                .wake
                .wait(slot)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// The comparison for `request`, from the cache or computed; `None`
    /// when it was superseded on the way.
    fn result(
        &mut self,
        request: RequestId,
        comparison: ComparisonRequest,
    ) -> Option<Arc<Comparison>> {
        let unavailable = |why| {
            Some(Arc::new(Comparison {
                request: comparison,
                outcome: ComparisonOutcome::Unavailable(why),
            }))
        };
        let Some(scenario) = self.active.get() else {
            return unavailable(Unavailable::NoScenario);
        };
        // Validated before the cache is consulted: a cached result is only
        // as current as the scenario it was computed from.
        if let Err(why) = validate(&scenario, &comparison) {
            return unavailable(why);
        }
        if let Some(cached) = self.cache.iter().find(|c| c.request == comparison) {
            return Some(Arc::clone(cached));
        }
        let shared = Arc::clone(&self.shared);
        let outcome = match compute(&scenario, comparison, || !shared.is_current(request)) {
            Ok(model) => ComparisonOutcome::Ready(Box::new(model)),
            Err(Failure::Unavailable(why)) => ComparisonOutcome::Unavailable(why),
            Err(Failure::Superseded) => return None,
        };
        let result = Arc::new(Comparison {
            request: comparison,
            outcome,
        });
        // Only ready results are cached: an unavailable one may become
        // available once a scan publishes its scenario.
        if matches!(result.outcome, ComparisonOutcome::Ready(_)) {
            self.cache.push_back(Arc::clone(&result));
            while self.cache.len() > CACHE_SLOTS {
                self.cache.pop_front();
            }
        }
        Some(result)
    }

    /// Sends the event and records its evidence, but only while `request`
    /// is current and the active scenario still matches it. Returns
    /// `false` once the engine is gone.
    ///
    /// One critical section under the slot lock, which `load` also takes,
    /// holds the currency check, the scenario fence, the enqueue and then
    /// the evidence record, so no newer request is admitted in between.
    /// Evidence is recorded only after `try_send` succeeded: a result that
    /// never reached the queue (full, then superseded; or the engine gone)
    /// leaves no record a review could bind to. The cost is a window of a
    /// few instructions after the enqueue in which the app could already
    /// drain the event and a review naming it is refused as
    /// `EvidenceUnavailable`. That is the chosen trade-off: it fails
    /// closed. Rejected: recording first, which on a full queue followed by
    /// a supersession leaves trusted evidence for a result nobody saw, and
    /// `ComparisonEvidence` (T4's) has no way to withdraw a record.
    ///
    /// The fence re-reads the active scenario: if a scan or snapshot change
    /// was published while the result was computed or waiting, a ready
    /// result goes out as `Unavailable` instead, with no evidence.
    ///
    /// A full queue is retried after releasing the lock, because holding it
    /// would block the dispatcher's next `load`. Rejected: the blocking
    /// `EventSink::send`, which outside the lock can deliver a request
    /// superseded while it waited and inside it stalls the dispatcher.
    fn deliver(&self, request: RequestId, result: &Arc<Comparison>) -> bool {
        let scan = result.request.scan;
        fire(Stage::BeforeDeliver, scan, request);
        loop {
            {
                let slot = lock(&self.shared.slot);
                if slot.closed || slot.latest != Some(request) {
                    return true;
                }
                let (comparison, pair) = self.fenced(result);
                match self.events.try_send(event(request, comparison)) {
                    Ok(()) => {
                        if let Some(pair) = pair {
                            self.evidence.record(request, pair);
                        }
                        drop(slot);
                        fire(Stage::Enqueued, scan, request);
                        return true;
                    }
                    Err(TrySendError::Disconnected(_)) => return false,
                    Err(TrySendError::Full(_)) => {}
                }
            }
            fire(Stage::QueueFull, scan, request);
            std::thread::sleep(DELIVERY_RETRY);
        }
    }

    /// `result` as it may be published now, with the evidence it carries.
    fn fenced(&self, result: &Arc<Comparison>) -> (Arc<Comparison>, Option<ReviewedPair>) {
        let ComparisonOutcome::Ready(model) = &result.outcome else {
            return (Arc::clone(result), None);
        };
        let now = self
            .active
            .get()
            .map_or(Err(Unavailable::NoScenario), |scenario| {
                validate(&scenario, &result.request)
            });
        match now {
            Ok(()) => (
                Arc::clone(result),
                Some(reviewed_pair(&result.request, model)),
            ),
            Err(why) => (
                Arc::new(Comparison {
                    request: result.request,
                    outcome: ComparisonOutcome::Unavailable(why),
                }),
                None,
            ),
        }
    }
}

const fn reviewed_pair(request: &ComparisonRequest, model: &AlignedComparison) -> ReviewedPair {
    ReviewedPair {
        scan: request.scan,
        target: request.target,
        target_snapshot: request.target_snapshot,
        counterpart: request.counterpart,
        counterpart_snapshot: request.counterpart_snapshot,
        rules: request.rules,
        mode: request.mode,
        unique_lines: model.only_in_a.lines.len() as u64,
    }
}

enum Failure {
    Unavailable(Unavailable),
    Superseded,
}

impl From<Unavailable> for Failure {
    fn from(why: Unavailable) -> Self {
        Self::Unavailable(why)
    }
}

/// Whether `scenario` is the observation `request` names: its scan, its
/// rules revision, and both members at the requested snapshots. The worker
/// checks this before using its cache and again just before publishing.
fn validate(scenario: &Scenario, request: &ComparisonRequest) -> Result<(), Unavailable> {
    let results = &scenario.results;
    if request.scan != results.scan {
        return Err(Unavailable::ScanMismatch {
            requested: request.scan,
            current: results.scan,
        });
    }
    if request.rules != results.rules.revision {
        return Err(Unavailable::RulesMismatch {
            requested: request.rules,
            current: results.rules.revision,
        });
    }
    for (file, snapshot) in [
        (request.target, request.target_snapshot),
        (request.counterpart, request.counterpart_snapshot),
    ] {
        let member = results
            .group_of(file)
            .and_then(|group| group.member(file))
            .ok_or(Unavailable::FileMissing(file))?;
        if member.snapshot != snapshot {
            return Err(Unavailable::SnapshotMismatch {
                file,
                requested: snapshot,
                current: member.snapshot,
            });
        }
    }
    Ok(())
}

/// Validates `request` against the active scenario and computes it.
fn compute(
    scenario: &Scenario,
    request: ComparisonRequest,
    superseded: impl Fn() -> bool,
) -> Result<AlignedComparison, Failure> {
    let results = &scenario.results;
    validate(scenario, &request)?;
    let target = text(scenario, request.target)?;
    let counterpart = text(scenario, request.counterpart)?;
    AlignedComparison::compute_until(
        (request.target, target),
        (request.counterpart, counterpart),
        request.mode,
        results.rules.ignore,
        superseded,
    )
    .map_err(|aligned::Superseded| Failure::Superseded)
}

/// The text of `file` from the scenario's buffers; [`validate`] has
/// already matched its snapshot.
fn text(scenario: &Scenario, file: FileId) -> Result<Arc<str>, Unavailable> {
    let bytes = scenario
        .contents
        .render(file)
        .ok_or(Unavailable::FileMissing(file))?;
    String::from_utf8(bytes)
        .map(Arc::from)
        .map_err(|_| Unavailable::NotText(file))
}

/// Interleavings the integration tests cannot force: each test pins the
/// worker or the dispatcher at a [`Stage`] through the `seam` hook, so the
/// order under test is fixed rather than left to the scheduler.
#[cfg(test)]
mod tests {
    use std::sync::mpsc::{Receiver, Sender, channel};
    use std::sync::{Arc, Mutex, MutexGuard};
    use std::time::{Duration, Instant};

    use fuzzy_dedupe_domain::{CompareMode, Refusal, RequestId, ScanId, SnapshotId};

    use super::seam::{SERIAL, install};
    use super::{Comparison, ComparisonOutcome, ComparisonRequest, Stage, Unavailable, lock};
    use crate::fixtures::{self, DEFAULT_SEED, Scenario, ScenarioKind};
    use crate::review::{ReviewCommand, ReviewStatus};
    use crate::service::{Engine, EngineConfig, EngineEvent, EngineRequest, Origin};

    type Seen = (Stage, RequestId);

    /// Holds `SERIAL` and clears the hook when the test ends, even on panic.
    struct Hooked {
        _serial: MutexGuard<'static, ()>,
    }

    impl Drop for Hooked {
        fn drop(&mut self) {
            install(None);
        }
    }

    /// Installs a hook for `scan` that reports every stage to the returned
    /// receiver and answers with `steer`.
    fn hook(
        scan: ScanId,
        steer: impl Fn(Stage, RequestId) -> bool + Send + Sync + 'static,
    ) -> (Hooked, Receiver<Seen>) {
        let guard = Hooked {
            _serial: lock(&SERIAL),
        };
        let (tx, rx) = channel::<Seen>();
        let tx = Mutex::new(tx);
        install(Some(Arc::new(move |stage, s, request| {
            if s != scan {
                return true;
            }
            let _ = lock(&tx).send((stage, request));
            steer(stage, request)
        })));
        (guard, rx)
    }

    fn wait_for(rx: &Receiver<Seen>, want: Seen) {
        let deadline = Instant::now() + Duration::from_mins(1);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match rx.recv_timeout(left) {
                Ok(seen) if seen == want => return,
                Ok(_) => {}
                Err(e) => panic!("never saw {want:?}: {e}"),
            }
        }
    }

    fn setup(scan: u64, config: EngineConfig) -> (Engine, Arc<Scenario>, ComparisonRequest) {
        let mut engine = Engine::new(config).expect("engine starts");
        let scenario = Arc::new(
            fixtures::build(ScenarioKind::Mixed, ScanId::new(scan), DEFAULT_SEED).expect("mixed"),
        );
        engine
            .adopt_scenario(Arc::clone(&scenario))
            .expect("fresh scan ID");
        let group = &scenario.results.groups[0];
        let (target, reference) = (&group.members[1], &group.members[0]);
        let request = ComparisonRequest {
            scan: scenario.results.scan,
            target: target.file,
            counterpart: reference.file,
            target_snapshot: target.snapshot,
            counterpart_snapshot: reference.snapshot,
            rules: scenario.results.rules.revision,
            mode: CompareMode::Original,
        };
        (engine, scenario, request)
    }

    fn submit(engine: &mut Engine, request: ComparisonRequest) -> RequestId {
        match engine.submit(EngineRequest::LoadComparison(request)) {
            Ok(Origin::Request(id)) => id,
            other => panic!("LoadComparison: {other:?}"),
        }
    }

    /// Drains until `id`'s comparison arrives; every event seen is kept.
    fn await_result(
        engine: &mut Engine,
        id: RequestId,
        seen: &mut Vec<EngineEvent>,
    ) -> Arc<Comparison> {
        let deadline = Instant::now() + Duration::from_mins(1);
        loop {
            for event in engine.drain_events(16) {
                let hit = match &event {
                    EngineEvent::Comparison {
                        request,
                        comparison,
                    } if *request == id => Some(Arc::clone(comparison)),
                    _ => None,
                };
                seen.push(event);
                if let Some(hit) = hit {
                    return hit;
                }
            }
            assert!(Instant::now() < deadline, "no comparison for {id:?}");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn evidence_missing(engine: &mut Engine, req: &ComparisonRequest, id: RequestId) -> bool {
        matches!(
            engine
                .review(ReviewCommand::AcknowledgeUniqueText {
                    file: req.target,
                    counterpart: req.counterpart,
                    reviewed: Some(id),
                })
                .status,
            ReviewStatus::Refused {
                refusal: Refusal::EvidenceUnavailable,
                ..
            }
        )
    }

    /// A pause point: the hook blocks the thread that reaches `(stage, id)`
    /// until the returned sender is used or dropped.
    fn pause_at(stage: Stage, id: RequestId) -> (impl Fn(Stage, RequestId) -> bool, Sender<()>) {
        let (release, wait) = channel::<()>();
        let wait = Mutex::new(wait);
        let steer = move |s, r| {
            if (s, r) == (stage, id) {
                let _ = lock(&wait).recv();
            }
            true
        };
        (steer, release)
    }

    #[test]
    fn compare_a_request_superseded_before_publication_is_dropped() {
        let (mut engine, _, req) = setup(9_104, EngineConfig::default());
        let normalized = ComparisonRequest {
            mode: CompareMode::Normalized,
            ..req
        };
        // Request IDs are allocated from 1 by a fresh engine.
        let (steer, release) = pause_at(Stage::BeforeDeliver, RequestId::new(1));
        let (_hook, rx) = hook(req.scan, steer);
        let first = submit(&mut engine, req);
        assert_eq!(first, RequestId::new(1));
        // The worker holds `first`'s finished result at the pause point,
        // while the dispatcher thread admits the second request: so the
        // computation runs off the dispatcher thread.
        wait_for(&rx, (Stage::BeforeDeliver, first));
        let second = submit(&mut engine, normalized);
        wait_for(&rx, (Stage::Loaded, second));
        drop(release);
        let mut seen = Vec::new();
        let got = await_result(&mut engine, second, &mut seen);
        assert_eq!(got.request, normalized);
        assert!(seen.iter().all(|e| e.origin() != Origin::Request(first)));
        assert!(evidence_missing(&mut engine, &req, first));
        assert!(!evidence_missing(&mut engine, &normalized, second));
    }

    #[test]
    fn compare_no_evidence_survives_a_full_queue_and_a_supersession() {
        let (mut engine, _, req) = setup(
            9_101,
            EngineConfig {
                event_capacity: 1,
                ..EngineConfig::default()
            },
        );
        let (_hook, rx) = hook(req.scan, |_, _| true);
        // `filler`'s result occupies the only event slot; nothing drains.
        let filler = submit(&mut engine, req);
        wait_for(&rx, (Stage::Enqueued, filler));
        let normalized = ComparisonRequest {
            mode: CompareMode::Normalized,
            ..req
        };
        let stuck = submit(&mut engine, normalized);
        wait_for(&rx, (Stage::QueueFull, stuck));
        assert!(
            evidence_missing(&mut engine, &normalized, stuck),
            "evidence for a result that never reached the queue"
        );
        let newest = submit(&mut engine, req);
        // The worker leaves `stuck` and now retries `newest` instead.
        wait_for(&rx, (Stage::QueueFull, newest));
        assert!(evidence_missing(&mut engine, &normalized, stuck));
        assert!(evidence_missing(&mut engine, &req, newest));

        let mut seen = Vec::new();
        await_result(&mut engine, newest, &mut seen);
        assert!(seen.iter().all(|e| e.origin() != Origin::Request(stuck)));
        assert!(evidence_missing(&mut engine, &normalized, stuck));
        // `newest` was enqueued while current; its record follows the
        // enqueue (the documented fail-closed window), so allow a moment.
        let deadline = Instant::now() + Duration::from_secs(10);
        while evidence_missing(&mut engine, &req, newest) {
            assert!(Instant::now() < deadline, "newest never recorded");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn compare_delivery_is_fenced_against_a_scenario_change() {
        let (mut engine, scenario, req) = setup(9_102, EngineConfig::default());
        let (steer, release) = pause_at(Stage::BeforeDeliver, RequestId::new(1));
        let (_hook, rx) = hook(req.scan, steer);
        let id = submit(&mut engine, req);
        wait_for(&rx, (Stage::BeforeDeliver, id));
        // The target changes while its finished result waits to publish.
        let mut changed = (*scenario).clone();
        let results = Arc::make_mut(&mut changed.results);
        let member = results.groups[0]
            .members
            .iter_mut()
            .find(|m| m.file == req.target)
            .expect("target");
        member.snapshot = SnapshotId::new(77_777);
        let generation = engine.active().begin();
        assert!(engine.active().publish(generation, Arc::new(changed)));
        drop(release);
        let got = await_result(&mut engine, id, &mut Vec::new());
        assert_eq!(
            got.outcome,
            ComparisonOutcome::Unavailable(Unavailable::SnapshotMismatch {
                file: req.target,
                requested: req.target_snapshot,
                current: SnapshotId::new(77_777),
            })
        );
        assert!(evidence_missing(&mut engine, &req, id));
    }

    #[test]
    fn compare_every_request_is_answered_when_the_worker_cannot_start() {
        let (mut engine, _, req) = setup(
            9_103,
            EngineConfig {
                event_capacity: 1,
                ..EngineConfig::default()
            },
        );
        // The first two spawn attempts fail, later ones succeed.
        let attempts = Arc::new(Mutex::new(0_u32));
        let counter = Arc::clone(&attempts);
        let (_hook, _rx) = hook(req.scan, move |stage, _| {
            if stage != Stage::Spawn {
                return true;
            }
            let mut n = lock(&counter);
            *n += 1;
            *n > 2
        });
        let first = submit(&mut engine, req);
        let second = submit(&mut engine, req);
        // `first`'s notice fills the queue; `second`'s must wait, not vanish.
        let mut seen = Vec::new();
        let got = await_result(&mut engine, first, &mut seen);
        assert_eq!(
            got.outcome,
            ComparisonOutcome::Unavailable(Unavailable::WorkerUnavailable)
        );
        // The next load delivers the waiting notice and starts the worker.
        let third = submit(&mut engine, req);
        let got = await_result(&mut engine, second, &mut seen);
        assert_eq!(
            got.outcome,
            ComparisonOutcome::Unavailable(Unavailable::WorkerUnavailable)
        );
        let got = await_result(&mut engine, third, &mut seen);
        assert!(matches!(got.outcome, ComparisonOutcome::Ready(_)));
        assert_eq!(*lock(&attempts), 3);
    }
}
