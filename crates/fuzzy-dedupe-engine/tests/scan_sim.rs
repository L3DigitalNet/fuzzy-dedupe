// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! PV-T5-001 (plan T5): the simulated scan job behind `StartScan` and
//! `CancelScan` (C-03).
//!
//! Every test sits in the `scan_sim` module so the plan's proof filter
//! `-E 'test(scan_sim)'` selects them all; nextest's `test()` predicate
//! matches the test path, not the binary name.
//!
//! All timing uses the real clock through the public facade, as the app
//! sees it: a latency here includes the dispatcher hop, the workers'
//! acknowledgement, the partial result's assembly and the event queue. The
//! engines run with the default config, one scan worker per available core,
//! so a scan of `large` saturates the machine while it is cancelled.

mod scan_sim {
    use std::collections::BTreeSet;
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    use fuzzy_dedupe_domain::{
        Coverage, GroupId, MatchingRules, RulesRevision, ScanFolder, ScanId, ScanProgress,
        ScanResults, ScanSetup,
    };
    use fuzzy_dedupe_engine::fixtures::{self, DEFAULT_SEED, ScenarioKind};
    use fuzzy_dedupe_engine::service::{
        Engine, EngineConfig, EngineEvent, EngineRequest, Origin, ScanEvent, StartScan,
    };

    /// Generous outer bound for one whole scan of `large`, so a hung job
    /// fails the test instead of hanging the suite.
    const SCAN_TIMEOUT: Duration = Duration::from_mins(1);

    /// The C-03 and spec §11.9 bound on cancel acknowledgement.
    const CANCEL_BOUND: Duration = Duration::from_secs(1);

    fn engine() -> Engine {
        Engine::new(EngineConfig::default()).expect("engine starts")
    }

    fn setup() -> ScanSetup {
        ScanSetup {
            folders: vec![
                ScanFolder {
                    path: PathBuf::from("/home/user/Documents"),
                    protect: true,
                },
                ScanFolder {
                    path: PathBuf::from("/home/user/Downloads"),
                    protect: false,
                },
            ],
            file_types: vec!["txt".into(), "md".into()],
        }
    }

    /// The fixture's rules under a revision no fixture uses, so a result
    /// that carried the fixture's own copy would be told apart.
    fn rules() -> MatchingRules {
        let mut rules = fixtures::build(ScenarioKind::Mixed, ScanId::new(1), DEFAULT_SEED)
            .expect("mixed is generated")
            .results
            .rules;
        rules.revision = RulesRevision::new(4242);
        rules
    }

    fn request(kind: ScenarioKind) -> StartScan {
        StartScan {
            setup: setup(),
            rules: rules(),
            scenario: kind,
            seed: DEFAULT_SEED,
        }
    }

    fn start(engine: &mut Engine, request: StartScan) -> ScanId {
        match engine.submit(EngineRequest::StartScan(request)) {
            Ok(Origin::Scan(scan)) => scan,
            other => panic!("StartScan admitted as {other:?}"),
        }
    }

    fn cancel(engine: &mut Engine, scan: ScanId) {
        assert_eq!(
            engine.submit(EngineRequest::CancelScan(scan)),
            Ok(Origin::Scan(scan)),
            "a cancellation is always admitted"
        );
    }

    /// One drained event with the moment the consumer saw it.
    struct Seen {
        at: Instant,
        event: EngineEvent,
    }

    /// Drains `engine` until `stop` accepts an event or `timeout` passes, and
    /// returns everything drained, the accepted event last. A not-implemented
    /// notice for a scan fails at once: it is the stub's answer, and the
    /// readable RED reason.
    fn drain_until(
        engine: &mut Engine,
        timeout: Duration,
        mut stop: impl FnMut(&EngineEvent) -> bool,
    ) -> Vec<Seen> {
        let deadline = Instant::now() + timeout;
        let mut seen = Vec::new();
        loop {
            for event in engine.drain_events(256) {
                if let EngineEvent::NotImplemented {
                    origin: Origin::Scan(scan),
                    missing,
                } = &event
                {
                    panic!("scan {scan:?} answered not implemented: {missing:?}");
                }
                let done = stop(&event);
                seen.push(Seen {
                    at: Instant::now(),
                    event,
                });
                if done {
                    return seen;
                }
            }
            assert!(
                Instant::now() < deadline,
                "no accepted event within {timeout:?}; drained {} events",
                seen.len()
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn is_terminal(event: &EngineEvent, scan: ScanId) -> bool {
        matches!(
            event,
            EngineEvent::Scan {
                scan: s,
                event: ScanEvent::Completed(_) | ScanEvent::Partial(_),
            } if *s == scan
        )
    }

    fn events_of(seen: &[Seen], scan: ScanId) -> Vec<&ScanEvent> {
        seen.iter()
            .filter_map(|s| match &s.event {
                EngineEvent::Scan { scan: s, event } if *s == scan => Some(event),
                _ => None,
            })
            .collect()
    }

    fn progress_of<'a>(events: &[&'a ScanEvent]) -> Vec<&'a ScanProgress> {
        events
            .iter()
            .filter_map(|e| match e {
                ScanEvent::Progress(p) => Some(p),
                _ => None,
            })
            .collect()
    }

    fn group_ids(results: &ScanResults) -> BTreeSet<GroupId> {
        results.groups.iter().map(|g| g.id).collect()
    }

    /// The lifecycle shape of C-03: `Started` first, then only `Progress`,
    /// then exactly one terminal event, last.
    fn assert_lifecycle(events: &[&ScanEvent]) {
        assert!(
            matches!(events.first(), Some(ScanEvent::Started)),
            "Started comes first: {:?}",
            events.first()
        );
        let (last, middle) = events.split_last().expect("events");
        assert!(
            matches!(last, ScanEvent::Completed(_) | ScanEvent::Partial(_)),
            "the terminal event comes last"
        );
        assert!(
            middle[1..]
                .iter()
                .all(|e| matches!(e, ScanEvent::Progress(_))),
            "only progress between Started and the result"
        );
    }

    #[test]
    fn scan_sim_progress_is_monotonic_and_completes_with_the_scenario() {
        let mut engine = engine();
        let scan = start(&mut engine, request(ScenarioKind::Large));
        let seen = drain_until(&mut engine, SCAN_TIMEOUT, |e| is_terminal(e, scan));
        let events = events_of(&seen, scan);
        assert_lifecycle(&events);

        let progress = progress_of(&events);
        assert!(
            progress.len() >= 2,
            "a large scan streams progress, got {} reports",
            progress.len()
        );
        for pair in progress.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            assert!(
                b.files_read >= a.files_read,
                "files read fell: {a:?} -> {b:?}"
            );
            assert!(
                b.groups_found >= a.groups_found,
                "groups fell: {a:?} -> {b:?}"
            );
            assert!(b.elapsed >= a.elapsed, "elapsed fell: {a:?} -> {b:?}");
        }
        assert!(
            progress
                .windows(2)
                .any(|p| p[1].files_read > p[0].files_read),
            "the counts actually advance during the scan"
        );
        assert!(
            progress.iter().all(|p| !p.current_folder.is_empty()),
            "every report names the folder being read"
        );

        let Some(ScanEvent::Completed(results)) = events.last() else {
            panic!("an uncancelled scan completes: {:?}", events.last());
        };
        let expected = fixtures::build(ScenarioKind::Large, scan, DEFAULT_SEED).expect("large");
        assert_eq!(results.scan, scan);
        assert_eq!(results.coverage, expected.results.coverage);
        assert_eq!(
            results.groups, expected.results.groups,
            "the whole scenario"
        );
        let last = progress.last().expect("progress");
        assert_eq!(
            last.groups_found,
            results.groups.len() as u64,
            "the final report counts every group"
        );

        let published = engine.active().get().expect("the scenario is published");
        assert_eq!(published.results.scan, scan, "published before completion");
        assert_eq!(published.contents, expected.contents);
    }

    #[test]
    fn scan_sim_cancel_is_acknowledged_within_one_second_under_saturation() {
        let mut engine = engine();
        let full = fixtures::build(ScenarioKind::Large, ScanId::new(1), DEFAULT_SEED)
            .expect("large")
            .results;
        let all_groups = group_ids(&full);
        let mut worst = Duration::ZERO;
        for run in 1..=20 {
            let scan = start(&mut engine, request(ScenarioKind::Large));
            // Cancel only once the workers are reading: the claim is about a
            // saturated job, not one still building its scenario.
            let before = drain_until(&mut engine, SCAN_TIMEOUT, |e| {
                matches!(e, EngineEvent::Scan { scan: s, event: ScanEvent::Progress(p) }
                    if *s == scan && p.files_read > 0)
            });
            assert!(
                !events_of(&before, scan)
                    .iter()
                    .any(|e| matches!(e, ScanEvent::Completed(_) | ScanEvent::Partial(_))),
                "run {run}: the scan ended before it could be cancelled"
            );
            let cancelled_at = Instant::now();
            cancel(&mut engine, scan);
            let after = drain_until(&mut engine, SCAN_TIMEOUT, |e| is_terminal(e, scan));
            let acknowledged = after.last().expect("terminal").at;
            let latency = acknowledged - cancelled_at;
            worst = worst.max(latency);
            assert!(
                latency <= CANCEL_BOUND,
                "run {run}: cancel acknowledged after {latency:?}"
            );

            let mut events = events_of(&before, scan);
            events.extend(events_of(&after, scan));
            assert_lifecycle(&events);
            let Some(ScanEvent::Partial(results)) = events.last() else {
                panic!(
                    "run {run}: a cancelled scan ends Partial: {:?}",
                    events.last()
                );
            };
            assert_eq!(results.scan, scan);
            assert_eq!(results.coverage, Coverage::Partial, "run {run}");
            let kept = group_ids(results);
            assert!(
                kept.is_subset(&all_groups),
                "run {run}: partial groups are real"
            );
            assert!(
                kept.len() < all_groups.len(),
                "run {run}: a cancelled scan is missing groups"
            );
            assert_eq!(results.setup, setup(), "run {run}: the setup copy is kept");
        }
        // Reported with `--no-capture`; the assertion above is the gate.
        eprintln!("scan_sim cancel acknowledgement, worst of 20 runs: {worst:?}");
    }

    #[test]
    fn scan_sim_cancel_sent_with_the_start_is_honoured() {
        // The dispatcher takes pending cancellations before queued jobs, so a
        // cancel submitted right behind its start may reach the scan service
        // first. Either order must end Partial.
        let mut engine = engine();
        for run in 1..=20 {
            let scan = start(&mut engine, request(ScenarioKind::Large));
            cancel(&mut engine, scan);
            let seen = drain_until(&mut engine, SCAN_TIMEOUT, |e| is_terminal(e, scan));
            let events = events_of(&seen, scan);
            assert_lifecycle(&events);
            assert!(
                matches!(events.last(), Some(ScanEvent::Partial(r)) if r.coverage == Coverage::Partial),
                "run {run}: {:?}",
                events.last()
            );
        }
    }

    #[test]
    fn scan_sim_superseded_scan_events_are_tagged_and_never_merge() {
        let mut engine = engine();
        let old = start(&mut engine, request(ScenarioKind::Large));
        drain_until(
            &mut engine,
            SCAN_TIMEOUT,
            |e| matches!(e, EngineEvent::Scan { scan, event: ScanEvent::Progress(_) } if *scan == old),
        );
        // Leave the queue undrained while the old scan keeps reporting, so
        // its events are still waiting when the new scan becomes current.
        std::thread::sleep(Duration::from_millis(300));
        let new = start(&mut engine, request(ScenarioKind::Mixed));
        assert_ne!(old, new, "scan IDs are fresh");

        let seen = drain_until(&mut engine, SCAN_TIMEOUT, |e| is_terminal(e, new));
        // Linger to catch anything the old job might still emit.
        let deadline = Instant::now() + Duration::from_millis(500);
        let mut late = seen;
        while Instant::now() < deadline {
            late.extend(engine.drain_events(256).into_iter().map(|event| Seen {
                at: Instant::now(),
                event,
            }));
            std::thread::sleep(Duration::from_millis(5));
        }

        let stale = events_of(&late, old);
        assert!(
            !stale.is_empty(),
            "the old scan's queued events arrive after the new scan started"
        );
        assert!(
            late.iter()
                .all(|s| matches!(s.event.origin(), Origin::Scan(id) if id == old || id == new)),
            "every event is tagged with the scan that produced it"
        );
        assert!(
            !stale
                .iter()
                .any(|e| matches!(e, ScanEvent::Completed(_) | ScanEvent::Partial(_))),
            "the superseded scan reports no result"
        );

        let current = events_of(&late, new);
        assert_lifecycle(&current);
        let Some(ScanEvent::Completed(results)) = current.last() else {
            panic!("the new scan completes: {:?}", current.last());
        };
        let mixed = fixtures::build(ScenarioKind::Mixed, new, DEFAULT_SEED).expect("mixed");
        assert_eq!(results.scan, new);
        assert_eq!(
            results.groups, mixed.results.groups,
            "no old group merged in"
        );
        let published = engine.active().get().expect("published");
        assert_eq!(published.results.scan, new, "the old scan never published");
    }

    #[test]
    fn scan_sim_stored_setup_copy_is_the_setup_at_start() {
        let mut engine = engine();
        let mut caller_setup = setup();
        let mut caller_rules = rules();
        let at_start = (caller_setup.clone(), caller_rules);
        let scan = start(
            &mut engine,
            StartScan {
                setup: caller_setup.clone(),
                rules: caller_rules,
                scenario: ScenarioKind::Mixed,
                seed: DEFAULT_SEED,
            },
        );
        // The caller keeps editing its own draft while the scan runs.
        caller_setup.folders.push(ScanFolder {
            path: PathBuf::from("/home/user/Later"),
            protect: false,
        });
        caller_setup.folders[0].protect = false;
        caller_setup.file_types.clear();
        caller_rules.revision = RulesRevision::new(9999);

        let seen = drain_until(&mut engine, SCAN_TIMEOUT, |e| is_terminal(e, scan));
        let events = events_of(&seen, scan);
        let Some(ScanEvent::Completed(results)) = events.last() else {
            panic!("the scan completes: {:?}", events.last());
        };
        assert_eq!(
            (&results.setup, results.rules),
            (&at_start.0, at_start.1),
            "the result keeps the setup and rules the scan started with"
        );
        assert_ne!(results.setup, caller_setup);
        assert_ne!(results.rules, caller_rules);
        let published = engine.active().get().expect("published");
        assert_eq!(published.results.setup, at_start.0);
    }
}
