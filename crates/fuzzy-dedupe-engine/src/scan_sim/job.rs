// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! One simulated scan: a coordinator thread and one worker per configured
//! core, all owned by this module and never the dispatcher.
//!
//! The coordinator reports `Started`, builds the fixture scenario, runs the
//! workers over its groups while it streams coalesced progress, and ends
//! with `Completed` or, once the workers have acknowledged a cancellation,
//! `Partial`. Workers "read" each group's member files from the in-memory
//! fixture buffers (plan D-005: no filesystem call exists here) and report
//! through shared counters, never through the event queue, so progress stays
//! bounded at one event per [`PROGRESS_INTERVAL`] however many files there
//! are.
//!
//! Busy-work design (plan T5 executor discretion): each file costs a fixed
//! slice of wall time per worker, spent hashing its rendered bytes and then
//! spinning on that hash until the slice ends. Spinning, not sleeping, is
//! what saturates the cores (spec §11.9 target T4 measures input latency
//! under exactly that load). Scaling the slice by the worker count keeps a
//! scan's wall duration near `files × FILE_COST` on any machine and under
//! any contention, so `large` runs for a few seconds everywhere: long enough
//! to watch and to cancel, short enough for a test suite.

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::thread::ScopedJoinHandle;
use std::time::{Duration, Instant};

use crossbeam_channel::RecvTimeoutError;
use fuzzy_dedupe_domain::{
    Coverage, EvidenceUpdate, FileId, Group, ScanId, ScanProgress, ScanResults,
};

use crate::fixtures::{self, FileContent, FixtureContents, Scenario, SeededDecision};
use crate::service::{
    ActiveScenario, EngineEvent, EventSink, NotImplemented, Origin, ScanEvent, ScenarioGeneration,
    StartScan,
};

/// Wall time one file costs the scan as a whole; see the busy-work design
/// in the module header. `large` holds about 80,000 files, about 3 s.
const FILE_COST: Duration = Duration::from_micros(40);

/// How often the coordinator reports progress: the coalescing interval
/// (spec §11.9), about 20 events a second.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(50);

/// How often a coordinator waiting on the scenario build looks at the
/// cancel flag.
const BUILD_POLL: Duration = Duration::from_millis(5);

/// Hash rounds between two looks at the clock and the cancel flag. A round
/// is a few nanoseconds, so a worker notices a cancellation within
/// microseconds, far inside the 1 s acknowledgement bound.
const SPIN_ROUNDS: u32 = 64;

/// One scan's stop signals, shared by the scan service, the coordinator and
/// the workers.
///
/// - `cancel`: stop reading and report the groups found so far as
///   `Partial`. Set by `CancelScan`.
/// - `superseded`: a newer scan started, or the engine is gone; stop and
///   report nothing at all, because no consumer wants this scan's result.
///   Always set together with `cancel`, so workers watch one flag.
#[derive(Debug, Default)]
pub(super) struct Control {
    cancel: AtomicBool,
    superseded: AtomicBool,
}

impl Control {
    pub(super) fn cancel(&self) {
        self.cancel.store(true, Ordering::Release);
    }

    pub(super) fn supersede(&self) {
        self.superseded.store(true, Ordering::Release);
        self.cancel();
    }

    fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Acquire)
    }

    fn is_superseded(&self) -> bool {
        self.superseded.load(Ordering::Acquire)
    }
}

/// Everything one scan needs, moved onto its coordinator thread.
#[derive(Clone, Debug)]
pub(super) struct Job {
    pub(super) scan: ScanId,
    pub(super) generation: ScenarioGeneration,
    pub(super) request: StartScan,
    pub(super) events: EventSink,
    pub(super) active: ActiveScenario,
    pub(super) workers: usize,
    pub(super) control: Arc<Control>,
}

/// What the scenario build produced.
enum Built {
    Scenario(Scenario),
    Missing(NotImplemented),
    Cancelled,
}

/// The counters workers publish and the coordinator reports.
#[derive(Default)]
struct Counters {
    files_read: AtomicU64,
    groups_found: AtomicU64,
    /// Index of the group most recently claimed, `usize::MAX` before any.
    current_group: AtomicUsize,
}

/// One group's work: the member files no earlier group already read. A file
/// in two groups is read, and counted, once.
struct GroupWork<'a> {
    files: Vec<Option<&'a FileContent>>,
}

impl Job {
    /// Runs the scan to its end on the calling thread, which must be a
    /// module-owned thread: it blocks on the event queue (backpressure).
    pub(super) fn run(self) {
        let started = Instant::now();
        if !self.send(ScanEvent::Started) {
            return;
        }
        let scenario = match self.build() {
            Built::Scenario(scenario) => scenario,
            Built::Cancelled => {
                self.finish(started, self.empty_scenario(), &[], false);
                return;
            }
            Built::Missing(missing) => {
                if !self.control.is_superseded() {
                    let _ = self.events.send(EngineEvent::NotImplemented {
                        origin: Origin::Scan(self.scan),
                        missing,
                    });
                }
                return;
            }
        };
        let done = self.read(started, &scenario);
        let complete = done.len() == scenario.results.groups.len();
        self.finish(started, scenario, &done, complete);
    }

    /// Reports a scan whose coordinator thread could not be spawned, from
    /// the dispatcher thread, so non-blocking only. It ends `Partial` with
    /// no group: honest coverage, and the consumer is not left waiting for
    /// a scan that will never report. If the event queue is full the
    /// notices are lost; the app's next `StartScan` recovers.
    pub(super) fn abandon(self) {
        let scenario = self.empty_scenario();
        let results = Arc::clone(&scenario.results);
        if self.active.publish(self.generation, Arc::new(scenario)) {
            for event in [ScanEvent::Started, ScanEvent::Partial(results)] {
                let _ = self.events.try_send(EngineEvent::Scan {
                    scan: self.scan,
                    event,
                });
            }
        }
    }

    fn send(&self, event: ScanEvent) -> bool {
        let sent = self.events.send(EngineEvent::Scan {
            scan: self.scan,
            event,
        });
        if !sent {
            // The engine is gone: stop the workers and report nothing more.
            self.control.supersede();
        }
        sent
    }

    /// Builds the fixture on a helper thread so a cancellation is never held
    /// up by it: building `large` takes a noticeable fraction of a second in
    /// a debug build, and the cancel bound is 1 s. On cancellation the
    /// helper is abandoned and its scenario dropped when it finishes.
    fn build(&self) -> Built {
        if self.control.is_cancelled() {
            return Built::Cancelled;
        }
        let (kind, scan, seed) = (self.request.scenario, self.scan, self.request.seed);
        let (tx, rx) = crossbeam_channel::bounded(1);
        let spawned = std::thread::Builder::new()
            .name("scan-sim-build".into())
            .spawn(move || {
                // The receiver is gone only when the scan was cancelled.
                let _ = tx.send(fixtures::build(kind, scan, seed));
            });
        let built = if spawned.is_ok() {
            loop {
                match rx.recv_timeout(BUILD_POLL) {
                    Ok(built) => break built,
                    Err(RecvTimeoutError::Timeout) if self.control.is_cancelled() => {
                        return Built::Cancelled;
                    }
                    Err(RecvTimeoutError::Timeout) => {}
                    // The helper panicked; nothing is left to report.
                    Err(RecvTimeoutError::Disconnected) => return Built::Cancelled,
                }
            }
        } else {
            fixtures::build(kind, scan, seed)
        };
        match built {
            Ok(scenario) => Built::Scenario(scenario),
            Err(missing) => Built::Missing(missing),
        }
    }

    /// Reads every group with the workers and returns the indices of the
    /// groups read in full. Fewer than all means the scan was cancelled.
    fn read(&self, started: Instant, scenario: &Scenario) -> Vec<usize> {
        let groups = &scenario.results.groups;
        let mut seen = HashSet::new();
        let work: Vec<GroupWork<'_>> = groups
            .iter()
            .map(|group| GroupWork {
                files: group
                    .members
                    .iter()
                    .filter(|m| seen.insert(m.file))
                    .map(|m| scenario.contents.get(m.file))
                    .collect(),
            })
            .collect();
        let counters = Counters {
            current_group: AtomicUsize::new(usize::MAX),
            ..Counters::default()
        };
        let cursor = AtomicUsize::new(0);
        let budget = FILE_COST.saturating_mul(u32::try_from(self.workers).unwrap_or(u32::MAX));
        let worker = || read_groups(&work, &cursor, &counters, budget, &self.control);

        std::thread::scope(|scope| {
            // Each worker holds a clone of `alive`; the coordinator's wait
            // below ends when the last one exits and drops its clone, so it
            // wakes the moment the workers are done instead of at the next
            // progress tick.
            let (alive, finished) = crossbeam_channel::bounded::<()>(0);
            let handles: Vec<ScopedJoinHandle<'_, Vec<usize>>> = (0..self.workers)
                .filter_map(|n| {
                    let alive = alive.clone();
                    std::thread::Builder::new()
                        .name(format!("scan-sim-{n}"))
                        .spawn_scoped(scope, move || {
                            let done = worker();
                            drop(alive);
                            done
                        })
                        .ok()
                })
                .collect();
            drop(alive);
            if handles.is_empty() {
                // No worker could be spawned: read on this thread, without
                // progress, rather than not at all.
                return worker();
            }
            while let Err(RecvTimeoutError::Timeout) = finished.recv_timeout(PROGRESS_INTERVAL) {
                if !self.control.is_superseded() {
                    let _ = self.send(ScanEvent::Progress(
                        self.progress(started, scenario, &counters),
                    ));
                }
            }
            handles
                .into_iter()
                // A panicked worker's groups count as unread, so the result
                // is honestly Partial rather than silently short.
                .flat_map(|handle| handle.join().unwrap_or_default())
                .collect()
        })
    }

    fn progress(&self, started: Instant, scenario: &Scenario, counters: &Counters) -> ScanProgress {
        let groups = &scenario.results.groups;
        let current = counters.current_group.load(Ordering::Relaxed);
        ScanProgress {
            current_folder: self.folder_of(groups.get(current).or_else(|| groups.first())),
            files_read: counters.files_read.load(Ordering::Relaxed),
            groups_found: counters.groups_found.load(Ordering::Relaxed),
            elapsed: started.elapsed(),
        }
    }

    /// Reports the final counts, publishes the scenario and reports the
    /// result, in that order: the comparison service must find the buffers
    /// of every file the result names (C-03 into C-04). Runs only after
    /// every worker has exited, which is the acknowledgement `Partial`
    /// stands for. A superseded scan reports nothing; neither does one whose
    /// publication the generation fence refuses (an adopted scenario or a
    /// newer scan took over).
    fn finish(&self, started: Instant, mut scenario: Scenario, done: &[usize], complete: bool) {
        if self.control.is_superseded() {
            return;
        }
        let mut results =
            Arc::try_unwrap(scenario.results).unwrap_or_else(|shared| (*shared).clone());
        // The scan's immutable copy is the request's, taken when the facade
        // admitted it (spec DR-001, §11.8 `ScanState`), never the fixture's
        // own sample setup.
        results.setup = self.request.setup.clone();
        results.rules = self.request.rules;
        if !complete {
            keep_groups(&mut results, done);
            results.coverage = Coverage::Partial;
            retain_present(
                &mut scenario.seeded_decisions,
                &mut scenario.evidence_changes,
                &results,
            );
        }
        let files_read = results
            .groups
            .iter()
            .flat_map(|g| &g.members)
            .map(|m| m.file)
            .collect::<HashSet<FileId>>()
            .len();
        let final_progress = ScanProgress {
            current_folder: self.folder_of(results.groups.last()),
            files_read: files_read as u64,
            groups_found: results.groups.len() as u64,
            elapsed: started.elapsed(),
        };
        let results = Arc::new(results);
        scenario.results = Arc::clone(&results);
        if !self.send(ScanEvent::Progress(final_progress)) {
            return;
        }
        if self.control.is_superseded() || !self.active.publish(self.generation, Arc::new(scenario))
        {
            return;
        }
        let _ = self.send(if complete {
            ScanEvent::Completed(results)
        } else {
            ScanEvent::Partial(results)
        });
    }

    /// The folder progress names while `group` is read: its reference's
    /// folder, else the first scan folder, so the scanning state never shows
    /// a blank folder line.
    fn folder_of(&self, group: Option<&Group>) -> String {
        group
            .and_then(|group| group.members.first())
            .map(|member| member.folder.clone())
            .or_else(|| {
                self.request
                    .setup
                    .folders
                    .first()
                    .map(|folder| folder.path.display().to_string())
            })
            .unwrap_or_default()
    }

    /// The scenario of a scan cancelled before it read anything.
    fn empty_scenario(&self) -> Scenario {
        Scenario {
            kind: self.request.scenario,
            seed: self.request.seed,
            results: Arc::new(ScanResults {
                scan: self.scan,
                setup: self.request.setup.clone(),
                rules: self.request.rules,
                groups: Vec::new(),
                issues: Vec::new(),
                coverage: Coverage::Partial,
            }),
            contents: FixtureContents::default(),
            seeded_decisions: Vec::new(),
            evidence_changes: Vec::new(),
        }
    }
}

/// One worker's loop: claims groups in order until none is left or the scan
/// is cancelled, and returns the indices of the groups it read in full. A
/// group interrupted by the cancellation is not returned, so a partial
/// result never holds a group whose members were not all read.
fn read_groups(
    work: &[GroupWork<'_>],
    cursor: &AtomicUsize,
    counters: &Counters,
    budget: Duration,
    control: &Control,
) -> Vec<usize> {
    let mut done = Vec::new();
    loop {
        let index = cursor.fetch_add(1, Ordering::Relaxed);
        let Some(group) = work.get(index) else {
            return done;
        };
        counters.current_group.store(index, Ordering::Relaxed);
        for content in &group.files {
            if !read_file(*content, budget, control) {
                return done;
            }
            counters.files_read.fetch_add(1, Ordering::Relaxed);
        }
        counters.groups_found.fetch_add(1, Ordering::Relaxed);
        done.push(index);
    }
}

/// Spends `budget` of wall time on one file; `false` when cancelled first.
fn read_file(content: Option<&FileContent>, budget: Duration, control: &Control) -> bool {
    let deadline = Instant::now() + budget;
    // FNV-1a over the bytes, then the same mix on the hash until the
    // deadline. `black_box` keeps the optimizer from deleting work whose
    // result nobody reads.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    if let Some(content) = content {
        for byte in content.render() {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
        }
    }
    loop {
        if control.is_cancelled() {
            return false;
        }
        if Instant::now() >= deadline {
            std::hint::black_box(hash);
            return true;
        }
        for _ in 0..SPIN_ROUNDS {
            hash = std::hint::black_box((hash ^ (hash >> 29)).wrapping_mul(0x0100_0000_01b3));
        }
    }
}

/// Keeps the groups at `done`, in scenario order.
fn keep_groups(results: &mut ScanResults, done: &[usize]) {
    let done: HashSet<usize> = done.iter().copied().collect();
    let mut index = 0;
    results.groups.retain(|_| {
        let keep = done.contains(&index);
        index += 1;
        keep
    });
}

/// Drops the seeded decisions and scripted evidence changes that name a
/// file the partial result no longer holds, so replaying them can never
/// address a file the review service does not know. Groups are kept or
/// dropped whole, so a `Queue` seed that relies on an earlier
/// `MakeReference` in its group (replay order, see `SeededDecision`) never
/// survives without it.
fn retain_present(
    decisions: &mut Vec<SeededDecision>,
    changes: &mut Vec<EvidenceUpdate>,
    results: &ScanResults,
) {
    let present: HashSet<FileId> = results
        .groups
        .iter()
        .flat_map(|g| &g.members)
        .map(|m| m.file)
        .collect();
    decisions.retain(|decision| match decision {
        SeededDecision::Kept(file)
        | SeededDecision::Protect(file)
        | SeededDecision::MakeReference(file) => present.contains(file),
        SeededDecision::Queue { file, counterpart } => {
            present.contains(file) && present.contains(counterpart)
        }
    });
    changes.retain(|change| match change {
        EvidenceUpdate::FileSnapshot { file, .. } => present.contains(file),
        EvidenceUpdate::Rules(_) => true,
    });
}

#[cfg(test)]
mod tests {
    use fuzzy_dedupe_domain::ScanId;

    use super::*;
    use crate::fixtures::{DEFAULT_SEED, ScenarioKind};

    /// A partial result drops whole groups, so every seed naming a member of
    /// a dropped group must go with it; a kept `MakeReference` seed would
    /// make replay address a file the review service never loaded.
    #[test]
    fn scan_sim_partial_drops_the_seeds_of_dropped_groups() {
        let scenario = fixtures::build(ScenarioKind::Mixed, ScanId::new(1), DEFAULT_SEED)
            .expect("mixed is generated");
        let mut results = (*scenario.results).clone();
        assert!(results.groups.len() >= 2, "mixed has several groups");
        let member = |g: usize, m: usize| results.groups[g].members[m].file;
        let (kept, dropped) = ((member(0, 0), member(0, 1)), (member(1, 0), member(1, 1)));
        let mut decisions = vec![
            SeededDecision::MakeReference(kept.1),
            SeededDecision::MakeReference(dropped.1),
            SeededDecision::Kept(dropped.0),
            SeededDecision::Queue {
                file: kept.0,
                counterpart: kept.1,
            },
            SeededDecision::Queue {
                file: dropped.0,
                counterpart: dropped.1,
            },
        ];
        let mut changes = vec![
            EvidenceUpdate::FileSnapshot {
                file: dropped.0,
                snapshot: results.groups[1].members[0].snapshot,
            },
            EvidenceUpdate::Rules(results.rules.revision),
        ];
        keep_groups(&mut results, &[0]);
        retain_present(&mut decisions, &mut changes, &results);
        assert_eq!(
            decisions,
            vec![
                SeededDecision::MakeReference(kept.1),
                SeededDecision::Queue {
                    file: kept.0,
                    counterpart: kept.1,
                },
            ],
            "only the kept group's seeds survive, in their replay order"
        );
        assert_eq!(changes, vec![EvidenceUpdate::Rules(results.rules.revision)]);
    }
}
