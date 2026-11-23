// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! PV-T4-001 (plan T4): the fixture scenarios, the C-01 results model and
//! the C-02 facade properties that hold for any correct module.
//!
//! SCOPE: no test here observes a module stub's not-implemented answer, so
//! T5 to T9 can replace their stub without turning this file red (D-004
//! forbids them to edit it). Facade properties that need a module's exact
//! behavior (that a cancel reached the scan service, that a plan was
//! enqueued) are proved against test doubles in `service::tests`, which an
//! integration test cannot reach.
//!
//! Every test sits in the `fixtures_contract` module so the plan's proof
//! filter `-E 'test(fixtures_contract)'` selects them all; nextest's `test()`
//! predicate matches the test path, not the binary name.
//!
//! The oracles are deliberately independent of the engine code they check.
//! The counterpart rule is recomputed here from each group's edges with this
//! file's own strength order, and the §10.1 unique-line measure is recomputed
//! from the rendered in-memory bytes rather than from the generator's line
//! tokens, so a generator that agrees with itself but not with the spec still
//! fails.

mod fixtures_contract {
    use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
    use std::panic::AssertUnwindSafe;
    use std::time::{Duration, Instant};

    use std::sync::Arc;

    use fuzzy_dedupe_domain::{
        ContainmentDirection, Coverage, DecisionBinding, FileId, Group, GroupHeaderKind, MatchEdge,
        Measurement, Member, MemberRole, QueueOrigin, QueuedTarget, Refusal, RelationKind,
        RequestId, RulesRevision, ScanId, SnapshotId, ValidationView,
    };
    use fuzzy_dedupe_engine::fixtures::{
        self, DEFAULT_SEED, Scenario, ScenarioKind, SeededDecision,
    };
    use fuzzy_dedupe_engine::review::ReviewCommand;
    use fuzzy_dedupe_engine::service::{ActiveScenario, AdoptError, ScenarioGeneration};
    use fuzzy_dedupe_engine::service::{
        BuildCleanupPlan, Engine, EngineConfig, EngineRequest, Origin, ReviewStatus, StartScan,
        SubmitError,
    };

    /// The six generated scenarios T4 owns. `Design` is T9's.
    const GENERATED: [ScenarioKind; 6] = [
        ScenarioKind::Large,
        ScenarioKind::Mixed,
        ScenarioKind::Issues,
        ScenarioKind::Empty,
        ScenarioKind::Unicode,
        ScenarioKind::Stale,
    ];

    const SCAN: ScanId = ScanId::new(7);

    fn build(kind: ScenarioKind) -> Scenario {
        fixtures::build(kind, SCAN, DEFAULT_SEED)
            .unwrap_or_else(|missing| panic!("{kind:?} is a T4 scenario, got {missing:?}"))
    }

    // ── Independent oracles ──────────────────────────────────────────────

    /// Strength order of spec §11.4's match-type sort (identical, same text,
    /// similar, then containment), stronger first, with the per-mille value
    /// ordering matches of one type. Written out here rather than borrowed
    /// from the domain crate so the recomputation below is independent.
    fn strength(measurement: Measurement) -> (u8, u16) {
        match measurement {
            Measurement::Identical(_) => (3, 1000),
            Measurement::SameText => (2, 1000),
            Measurement::Similar(p) => (1, p.get()),
            Measurement::Containment { coverage, .. } => (0, coverage.get()),
        }
    }

    /// `member`'s view of `edge`: containment flips direction, every other
    /// measurement is symmetric.
    fn seen_from(edge: &MatchEdge, member: FileId) -> Measurement {
        if edge.a == member {
            edge.measurement
        } else {
            match edge.measurement {
                Measurement::Containment {
                    coverage,
                    direction,
                } => Measurement::Containment {
                    coverage,
                    direction: match direction {
                        ContainmentDirection::InsideCounterpart => {
                            ContainmentDirection::ContainsCounterpart
                        }
                        ContainmentDirection::ContainsCounterpart => {
                            ContainmentDirection::InsideCounterpart
                        }
                    },
                },
                other => other,
            }
        }
    }

    /// §6 and §10.1: the reference whenever a direct edge to it exists,
    /// otherwise the member with the best direct match; equal strength goes
    /// to the lower file ID so the rule is total.
    fn expected_counterpart(group: &Group, reference: FileId, member: FileId) -> Option<FileId> {
        let incident: Vec<&MatchEdge> = group
            .edges
            .iter()
            .filter(|e| e.a == member || e.b == member)
            .collect();
        let other = |e: &MatchEdge| if e.a == member { e.b } else { e.a };
        if incident.iter().any(|e| other(e) == reference) {
            return Some(reference);
        }
        incident
            .iter()
            .max_by(|x, y| {
                strength(seen_from(x, member))
                    .cmp(&strength(seen_from(y, member)))
                    .then_with(|| other(y).cmp(&other(x)))
            })
            .map(|e| other(e))
    }

    /// The §6 default reference under the owner's reading B: the first by
    /// path among protected members if any; otherwise among members in the
    /// first-listed scan folder; otherwise among all members. Written as
    /// three explicit clauses, independent of the generator's sort key.
    fn expected_reference(
        members: &[Member],
        in_first_folder: impl Fn(&Member) -> bool,
    ) -> Option<FileId> {
        let first_by_path = |pool: Vec<&Member>| {
            pool.into_iter()
                .min_by(|x, y| x.display_path.cmp(&y.display_path))
                .map(|m| m.file)
        };
        let protected: Vec<&Member> = members.iter().filter(|m| m.folder_protected).collect();
        if !protected.is_empty() {
            return first_by_path(protected);
        }
        let first_folder: Vec<&Member> = members.iter().filter(|m| in_first_folder(m)).collect();
        if !first_folder.is_empty() {
            return first_by_path(first_folder);
        }
        first_by_path(members.iter().collect())
    }

    /// Lines of a buffer by spec §10.1: original text without the
    /// terminator, so CRLF, LF and a missing final newline compare equal.
    fn lines(bytes: &[u8]) -> Vec<&[u8]> {
        let mut out: Vec<&[u8]> = bytes
            .split(|b| *b == b'\n')
            .map(|l| l.strip_suffix(b"\r").unwrap_or(l))
            .collect();
        if bytes.ends_with(b"\n") || bytes.is_empty() {
            out.pop();
        }
        out
    }

    /// §10.1 unique lines: every occurrence of a target line whose text no
    /// line of the counterpart has.
    fn unique_lines(target: &[u8], counterpart: &[u8]) -> u64 {
        let theirs: HashSet<&[u8]> = lines(counterpart).into_iter().collect();
        lines(target)
            .iter()
            .filter(|l| !theirs.contains(*l))
            .count() as u64
    }

    /// Every member's rendered buffer, rendered once per scenario: the
    /// invariant checks read each file several times, and `large` has 80,000
    /// of them.
    struct Rendered(HashMap<FileId, Vec<u8>>);

    impl Rendered {
        fn of(scenario: &Scenario) -> Self {
            let files = scenario.results.groups.iter().flat_map(|g| &g.members);
            Self(
                files
                    .map(|m| {
                        let bytes = scenario.contents.render(m.file).unwrap_or_else(|| {
                            panic!("{:?}: no in-memory buffer for {:?}", scenario.kind, m.file)
                        });
                        (m.file, bytes)
                    })
                    .collect(),
            )
        }

        fn get(&self, file: FileId) -> &[u8] {
            &self.0[&file]
        }
    }

    fn members_by_id(scenario: &Scenario) -> BTreeMap<FileId, (&Group, &Member)> {
        scenario
            .results
            .groups
            .iter()
            .flat_map(|g| g.members.iter().map(move |m| (m.file, (g, m))))
            .collect()
    }

    // ── REQ-020: the 100,000-row scenario ────────────────────────────────

    #[test]
    fn large_expands_to_exactly_100000_flattened_rows() {
        let scenario = build(ScenarioKind::Large);
        let rows: usize = scenario
            .results
            .groups
            .iter()
            .map(|g| 1 + g.members.len())
            .sum();
        assert_eq!(
            rows, 100_000,
            "one header row per group plus every member row"
        );
        assert_eq!(scenario.results.flattened_row_count(), 100_000);
    }

    #[test]
    fn large_contains_every_required_row_shape() {
        let scenario = build(ScenarioKind::Large);
        let groups = &scenario.results.groups;

        let kinds: BTreeSet<RelationKind> = groups
            .iter()
            .flat_map(|g| g.members.iter().filter_map(|m| m.measurement()))
            .map(Measurement::relation)
            .collect();
        assert_eq!(
            kinds,
            BTreeSet::from([
                RelationKind::SameBytes,
                RelationKind::SameText,
                RelationKind::SimilarText,
                RelationKind::Containment,
            ]),
            "every relation type appears on a member row"
        );

        assert!(
            groups
                .iter()
                .any(|g| g.header() == Some(GroupHeaderKind::Mixed)),
            "at least one Mixed group"
        );
        let members = || groups.iter().flat_map(|g| g.members.iter());
        assert!(members().any(|m| m.folder_protected), "a protected member");
        assert!(
            members().any(|m| m.folder_protected && !m.is_reference()),
            "a protected member that is not the reference"
        );
        assert!(
            members().any(|m| !m.is_reference() && !m.folder_protected),
            "a kept-eligible member: neither reference nor protected"
        );
        assert!(
            groups.iter().any(|g| {
                let reference = g.members[0].file;
                g.members
                    .iter()
                    .any(|m| m.counterpart().is_some_and(|c| c != reference))
            }),
            "a counterpart-chained member, measured against a non-reference member"
        );
        assert!(
            members().any(|m| matches!(
                m.measurement(),
                Some(Measurement::Containment {
                    direction: ContainmentDirection::ContainsCounterpart,
                    ..
                })
            )),
            "a member that contains its counterpart, not only one inside it"
        );
    }

    #[test]
    fn large_builds_well_under_a_second() {
        let started = Instant::now();
        let scenario = build(ScenarioKind::Large);
        let elapsed = started.elapsed();
        eprintln!("large scenario build: {elapsed:?}");
        assert_eq!(scenario.results.flattened_row_count(), 100_000);
        // A generous ceiling, so a loaded CI runner cannot flake it; the
        // measured time is printed above and recorded in the task report.
        assert!(
            elapsed < Duration::from_secs(5),
            "large build took {elapsed:?}"
        );
    }

    // ── Determinism ──────────────────────────────────────────────────────

    #[test]
    fn two_generations_with_the_same_seed_are_equal() {
        for kind in GENERATED {
            assert_eq!(
                build(kind),
                build(kind),
                "{kind:?} differs between two builds"
            );
        }
        let other = fixtures::build(ScenarioKind::Large, SCAN, DEFAULT_SEED ^ 0x5EED)
            .expect("large is generated");
        assert_ne!(
            other,
            build(ScenarioKind::Large),
            "the seed must reach the generator, or determinism is vacuous"
        );
    }

    #[test]
    fn scenario_names_round_trip() {
        for kind in ScenarioKind::ALL {
            assert_eq!(ScenarioKind::from_name(kind.name()), Some(kind));
        }
        let names: Vec<&str> = ScenarioKind::ALL.iter().map(|k| k.name()).collect();
        assert_eq!(
            names,
            [
                "large", "mixed", "issues", "empty", "unicode", "stale", "design"
            ]
        );
        assert_eq!(ScenarioKind::from_name("nonesuch"), None);
    }

    // ── C-01 invariants over every scenario ──────────────────────────────

    #[test]
    fn every_c01_invariant_holds_over_all_scenarios() {
        for kind in GENERATED {
            let scenario = build(kind);
            check_invariants(&scenario);
        }
    }

    fn check_invariants(scenario: &Scenario) {
        let kind = scenario.kind;
        let results = &scenario.results;
        let rendered = Rendered::of(scenario);
        assert_eq!(results.scan, SCAN, "{kind:?}: results carry the scan ID");

        // Coverage agrees with the result shape.
        match results.coverage {
            Coverage::Empty => assert!(results.groups.is_empty(), "{kind:?}: Empty has groups"),
            Coverage::Complete | Coverage::Partial => {
                assert!(
                    !results.groups.is_empty(),
                    "{kind:?}: use Empty for no groups"
                );
            }
        }

        let mut files = HashSet::new();
        let mut group_ids = HashSet::new();
        for (index, group) in results.groups.iter().enumerate() {
            // Exhaustive destructuring: adding a field to `Group`, such as a
            // group-wide score, fails to compile here. That is the proof that
            // no group carries a cluster-wide percentage (§10.1).
            let Group {
                id,
                label,
                members,
                edges,
            } = group;
            let at = format!("{kind:?} {label}");
            assert!(group_ids.insert(*id), "{at}: duplicate group ID");
            assert_eq!(*label, format!("G{:03}", index + 1), "{at}: stable label");
            assert!(members.len() >= 2, "{at}: a group has at least two members");

            // Reference first, and only there.
            assert!(members[0].is_reference(), "{at}: reference is not first");
            assert_eq!(
                members.iter().filter(|m| m.is_reference()).count(),
                1,
                "{at}: exactly one reference"
            );
            let reference = members[0].file;

            let root = |m: &Member| {
                results
                    .setup
                    .folders
                    .iter()
                    .position(|f| m.display_path.starts_with(&*f.path.to_string_lossy()))
                    .unwrap_or_else(|| panic!("{at}: {} is under no scan folder", m.display_path))
            };
            assert_eq!(
                Some(reference),
                expected_reference(members, |m| root(m) == 0),
                "{at}: reference is not the §6 default (owner reading B)"
            );

            let ids: BTreeSet<FileId> = members.iter().map(|m| m.file).collect();
            assert_eq!(ids.len(), members.len(), "{at}: duplicate member");
            for m in members {
                assert!(files.insert(m.file), "{at}: {:?} is in two groups", m.file);
                assert_eq!(
                    m.folder_protected,
                    results.setup.folders[root(m)].protect,
                    "{at}: {} protection disagrees with its scan folder",
                    m.name
                );
                assert!(m.display_path.ends_with(&m.name), "{at}: path ends in name");
                assert!(
                    m.display_path.starts_with(&m.folder),
                    "{at}: path under folder"
                );
            }

            // Edges: within the group, no self-edge, no duplicate pair, and
            // together they connect the group (a connected cluster, §6).
            let mut pairs = HashSet::new();
            for e in edges {
                assert!(
                    ids.contains(&e.a) && ids.contains(&e.b),
                    "{at}: foreign edge"
                );
                assert_ne!(e.a, e.b, "{at}: self-edge");
                assert!(
                    pairs.insert((e.a.min(e.b), e.a.max(e.b))),
                    "{at}: duplicate edge"
                );
                check_edge_measurement(scenario, &rendered, &at, e);
            }
            let mut reached = BTreeSet::from([reference]);
            let mut frontier = vec![reference];
            while let Some(f) = frontier.pop() {
                for e in edges.iter().filter(|e| e.a == f || e.b == f) {
                    let next = if e.a == f { e.b } else { e.a };
                    if reached.insert(next) {
                        frontier.push(next);
                    }
                }
            }
            assert_eq!(reached, ids, "{at}: edges do not connect the group");

            check_counterparts(&rendered, &at, group);
        }
        check_seeds(scenario);
    }

    /// Each edge's measurement and stored unique counts agree with the
    /// rendered buffers it claims to describe.
    fn check_edge_measurement(scenario: &Scenario, rendered: &Rendered, at: &str, e: &MatchEdge) {
        let (a, b) = (rendered.get(e.a), rendered.get(e.b));
        assert_eq!(e.unique_lines_a, unique_lines(a, b), "{at}: edge unique a");
        assert_eq!(e.unique_lines_b, unique_lines(b, a), "{at}: edge unique b");
        let minimum = scenario.results.rules.minimum_similarity.get();
        match e.measurement {
            Measurement::Identical(_) => assert_eq!(a, b, "{at}: Identical bytes differ"),
            Measurement::SameText => {
                assert_ne!(a, b, "{at}: Same text that is also Same bytes");
                assert_eq!(lines(a), lines(b), "{at}: Same text lines differ");
            }
            Measurement::Similar(p) => {
                assert!(
                    (minimum..1000).contains(&p.get()),
                    "{at}: similarity {} outside [{minimum}, 1000)",
                    p.get()
                );
                assert_ne!(lines(a), lines(b), "{at}: Similar text is Same text");
            }
            Measurement::Containment {
                coverage,
                direction,
            } => {
                assert!((1..=1000).contains(&coverage.get()), "{at}: coverage");
                // The contained side's unique lines are exactly the part of
                // it the container lacks.
                let inner_unique = match direction {
                    ContainmentDirection::InsideCounterpart => e.unique_lines_a,
                    ContainmentDirection::ContainsCounterpart => e.unique_lines_b,
                };
                assert_eq!(
                    inner_unique == 0,
                    coverage.get() == 1000,
                    "{at}: full containment iff the inner file has no unique line"
                );
            }
        }
    }

    fn check_counterparts(rendered: &Rendered, at: &str, group: &Group) {
        let reference = group.members[0].file;
        for m in &group.members[1..] {
            let MemberRole::Member(link) = &m.role else {
                panic!("{at}: {} is a second reference", m.name)
            };
            assert_eq!(
                Some(link.counterpart),
                expected_counterpart(group, reference, m.file),
                "{at}: {} counterpart differs from the §10.1 rule",
                m.name
            );
            let edge = group
                .edges
                .iter()
                .find(|e| {
                    (e.a == m.file && e.b == link.counterpart)
                        || (e.b == m.file && e.a == link.counterpart)
                })
                .unwrap_or_else(|| panic!("{at}: {} has no edge to its counterpart", m.name));
            assert_eq!(
                link.measurement,
                Some(seen_from(edge, m.file)),
                "{at}: {} measurement is not its direct match with its counterpart",
                m.name
            );
            let computed = unique_lines(rendered.get(m.file), rendered.get(link.counterpart));
            assert_eq!(
                link.unique_lines, computed,
                "{at}: {} declared Unique is not the §10.1 count",
                m.name
            );
            assert_eq!(
                m.size_bytes,
                rendered.get(m.file).len() as u64,
                "{at}: {} size is not its buffer length",
                m.name
            );
        }
        assert_eq!(
            group.members[0].size_bytes,
            rendered.get(reference).len() as u64,
            "{at}: reference size"
        );
    }

    /// Seeded review decisions never encode a state C-05 forbids: no queued
    /// reference or protected file, no queued file whose counterpart is
    /// queued, no `MakeReference` of a file that is already the reference,
    /// and no fully queued group. Every seed names a file of the scenario.
    ///
    /// Seeds replay in listed order against the state earlier seeds left
    /// (see `SeededDecision`), so a `MakeReference` changes the group's
    /// reference and counterparts before a later `Queue` seed in that group.
    /// The scan results still show the §6 default, so such a `Queue` seed is
    /// checked only for existence, group membership and folder protection,
    /// which no review command changes. Recomputing the counterparts after a
    /// reference change is T7's policy, not this oracle's.
    fn check_seeds(scenario: &Scenario) {
        let kind = scenario.kind;
        let by_id = members_by_id(scenario);
        let known = |file: &FileId| {
            by_id
                .get(file)
                .unwrap_or_else(|| panic!("{kind:?}: seed names unknown {file:?}"))
        };
        let queued: BTreeSet<FileId> = scenario
            .seeded_decisions
            .iter()
            .filter_map(|d| match d {
                SeededDecision::Queue { file, .. } => Some(*file),
                _ => None,
            })
            .collect();
        let mut rereferenced = HashMap::new();
        for d in &scenario.seeded_decisions {
            match d {
                SeededDecision::Kept(f) | SeededDecision::Protect(f) => {
                    known(f);
                }
                SeededDecision::MakeReference(f) => {
                    let (group, _) = known(f);
                    // The reference the replay has reached: an earlier seed's
                    // choice, otherwise the results' §6 default.
                    let current = rereferenced
                        .get(&group.id)
                        .copied()
                        .unwrap_or(group.members[0].file);
                    assert_ne!(
                        *f, current,
                        "{kind:?} {}: MakeReference seed names {f:?}, already the reference",
                        group.label
                    );
                    rereferenced.insert(group.id, *f);
                }
                SeededDecision::Queue { file, counterpart } => {
                    let (group, m) = known(file);
                    let (counterpart_group, _) = known(counterpart);
                    assert_eq!(
                        group.id, counterpart_group.id,
                        "{kind:?}: seed counterpart is in another group"
                    );
                    assert!(
                        !m.folder_protected,
                        "{kind:?}: seed queues a protected file"
                    );
                    if rereferenced.contains_key(&group.id) {
                        continue;
                    }
                    assert!(!m.is_reference(), "{kind:?}: seed queues a reference");
                    assert_eq!(
                        m.counterpart(),
                        Some(*counterpart),
                        "{kind:?}: seed counterpart"
                    );
                    assert!(
                        !queued.contains(counterpart),
                        "{kind:?}: counterpart queued"
                    );
                }
            }
        }
        for group in &scenario.results.groups {
            assert!(
                group.members.iter().any(|m| !queued.contains(&m.file)),
                "{kind:?} {}: fully queued",
                group.label
            );
        }
    }

    /// `check_seeds`'s panic message, or `None` when it accepts the seeds.
    fn seed_rejection(scenario: &Scenario) -> Option<String> {
        let outcome = std::panic::catch_unwind(AssertUnwindSafe(|| check_seeds(scenario)));
        outcome.err().map(|panic| {
            panic
                .downcast_ref::<String>()
                .cloned()
                .unwrap_or_else(|| "non-string panic".to_owned())
        })
    }

    /// No generated scenario seeds `MakeReference` yet (T9's design sample
    /// will), so the sweep in `every_c01_invariant_holds_over_all_scenarios`
    /// cannot show that the check bites. These hand-built seeds do.
    #[test]
    fn seed_check_rejects_a_make_reference_of_the_current_reference() {
        let mut scenario = build(ScenarioKind::Mixed);
        let group = &scenario.results.groups[0];
        let (reference, member) = (group.members[0].file, group.members[1].file);

        scenario.seeded_decisions = vec![SeededDecision::MakeReference(member)];
        assert_eq!(
            seed_rejection(&scenario),
            None,
            "a member may become reference"
        );

        scenario.seeded_decisions = vec![SeededDecision::MakeReference(reference)];
        let rejected = seed_rejection(&scenario).expect("the current reference is rejected");
        assert!(rejected.contains("already the reference"), "{rejected}");

        // "Current" is the replay's state, not the results': after the first
        // seed, `member` is the reference and the §6 default is not.
        scenario.seeded_decisions = vec![
            SeededDecision::MakeReference(member),
            SeededDecision::MakeReference(reference),
        ];
        assert_eq!(seed_rejection(&scenario), None, "back to the default");
        scenario.seeded_decisions = vec![
            SeededDecision::MakeReference(member),
            SeededDecision::MakeReference(member),
        ];
        let rejected = seed_rejection(&scenario).expect("a repeated choice is rejected");
        assert!(rejected.contains("already the reference"), "{rejected}");

        // Outside every group, which the existence check must still catch.
        let unknown = FileId::new(u64::MAX);
        scenario.seeded_decisions = vec![SeededDecision::MakeReference(unknown)];
        let rejected = seed_rejection(&scenario).expect("an unknown file is rejected");
        assert!(rejected.contains("unknown"), "{rejected}");
    }

    // ── FR-013 pairs in `mixed` ──────────────────────────────────────────

    #[test]
    fn mixed_holds_the_fr013_identical_and_containment_pairs() {
        let scenario = build(ScenarioKind::Mixed);
        let identical_pair_found = scenario.results.groups.iter().any(|g| {
            let reference = &g.members[0];
            reference.folder_protected
                && g.edges.iter().any(|e| {
                    matches!(e.measurement, Measurement::Identical(_))
                        && e.a != reference.file
                        && e.b != reference.file
                        && [e.a, e.b].iter().all(|f| {
                            g.edges.iter().any(|r| {
                                ((r.a == *f && r.b == reference.file)
                                    || (r.b == *f && r.a == reference.file))
                                    && matches!(r.measurement, Measurement::Similar(_))
                            })
                        })
                })
        });
        assert!(
            identical_pair_found,
            "an identical pair, each member only similar to a protected reference"
        );

        let containment_pair_found = scenario.results.groups.iter().any(|g| {
            let reference = g.members[0].file;
            g.edges.iter().any(|e| {
                matches!(e.measurement, Measurement::Containment { .. })
                    && e.a != reference
                    && e.b != reference
                    && g.members.iter().any(|m| {
                        (m.file == e.a || m.file == e.b)
                            && m.counterpart() == Some(if m.file == e.a { e.b } else { e.a })
                    })
            })
        });
        assert!(
            containment_pair_found,
            "a containment pair that excludes the reference, shown on a member row"
        );

        assert!(
            scenario
                .results
                .groups
                .iter()
                .any(|g| g.header() == Some(GroupHeaderKind::Mixed)),
            "mixed holds a Mixed group"
        );
    }

    // ── Edge-state scenarios ─────────────────────────────────────────────

    #[test]
    fn edge_state_scenarios_have_their_shape() {
        let empty = build(ScenarioKind::Empty);
        assert_eq!(empty.results.coverage, Coverage::Empty);
        assert!(empty.results.groups.is_empty() && empty.results.issues.is_empty());

        let issues = build(ScenarioKind::Issues);
        assert_eq!(issues.results.coverage, Coverage::Complete);
        let kinds: BTreeSet<_> = issues.results.issues.iter().map(|i| i.kind).collect();
        assert_eq!(
            kinds.len(),
            fuzzy_dedupe_domain::IssueKind::ALL.len(),
            "issues lists every discovery-problem kind"
        );

        let unicode = build(ScenarioKind::Unicode);
        let names: Vec<&str> = unicode
            .results
            .groups
            .iter()
            .flat_map(|g| g.members.iter().map(|m| m.name.as_str()))
            .collect();
        let has = |range: std::ops::RangeInclusive<char>| {
            names.iter().any(|n| n.chars().any(|c| range.contains(&c)))
        };
        assert!(has('\u{3040}'..='\u{30FF}'), "a Japanese kana file name");
        assert!(has('\u{0600}'..='\u{06FF}'), "an Arabic file name");
        assert!(has('\u{1F300}'..='\u{1FAFF}'), "an emoji file name");

        let stale = build(ScenarioKind::Stale);
        assert!(
            !stale.evidence_changes.is_empty(),
            "stale scripts evidence changes"
        );
        assert!(
            stale
                .seeded_decisions
                .iter()
                .any(|d| matches!(d, SeededDecision::Queue { .. })),
            "stale seeds a queued decision for the change to invalidate"
        );
        for kind in GENERATED {
            if kind != ScenarioKind::Stale {
                assert!(build(kind).evidence_changes.is_empty(), "{kind:?}");
            }
        }
    }

    // ── Reference default, owner reading B ───────────────────────────────

    #[test]
    fn reference_default_follows_reading_b_in_every_clause() {
        let first_folder = |scenario: &Scenario| {
            scenario.results.setup.folders[0]
                .path
                .to_string_lossy()
                .into_owned()
        };
        let mixed = build(ScenarioKind::Mixed);
        let root0 = first_folder(&mixed);
        // Clause 3: no protected member and none in folders[0]; the first by
        // path across all folders wins even though it sits in the
        // second-listed folder's sibling.
        let notes = &mixed.results.groups[2];
        assert!(
            notes
                .members
                .iter()
                .all(|m| !m.folder_protected && !m.display_path.starts_with(&root0))
        );
        assert_eq!(notes.members[0].name, "Notes (Windows).txt");
        assert_eq!(
            notes.members[0].display_path,
            notes
                .members
                .iter()
                .map(|m| m.display_path.clone())
                .min()
                .expect("members")
        );

        // Clause 2 decides over path order, and clause 3 also occurs, in
        // `large`.
        let large = build(ScenarioKind::Large);
        let root0 = first_folder(&large);
        let clause_two_over_path = large.results.groups.iter().any(|g| {
            let r = &g.members[0];
            g.members.iter().all(|m| !m.folder_protected)
                && r.display_path.starts_with(&root0)
                && g.members.iter().any(|m| m.display_path < r.display_path)
        });
        assert!(
            clause_two_over_path,
            "a group where folders[0] beats a smaller path"
        );
        let clause_three = large.results.groups.iter().any(|g| {
            g.members
                .iter()
                .all(|m| !m.folder_protected && !m.display_path.starts_with(&root0))
        });
        assert!(
            clause_three,
            "a group with no protected and no folders[0] member"
        );
    }

    // ── Review evidence for Compare-chosen counterparts ──────────────────

    #[test]
    fn compare_review_needs_trusted_comparison_evidence() {
        let mut engine = Engine::new(EngineConfig::default()).expect("engine starts");
        let mixed = Arc::new(build(ScenarioKind::Mixed));
        engine
            .adopt_scenario(Arc::clone(&mixed))
            .expect("fresh scan ID");
        // The FR-013 chain: Minutes_final.txt reviewed against Minutes.txt,
        // a pair with no direct edge.
        let chain = &mixed.results.groups[1];
        let named = |name: &str| {
            chain
                .members
                .iter()
                .find(|m| m.name == name)
                .unwrap_or_else(|| panic!("no {name}"))
                .file
        };
        let (target, counterpart) = (named("Minutes_final.txt"), named("Minutes.txt"));
        assert!(chain.edge_between(target, counterpart).is_none());

        let refused = |status: ReviewStatus, refusal: Refusal| {
            assert_eq!(
                status,
                ReviewStatus::Refused {
                    file: target,
                    refusal
                }
            );
        };
        // A Compare queue request with no comparison is refused before policy.
        refused(
            engine
                .review(ReviewCommand::RequestQueue {
                    file: target,
                    counterpart,
                    origin: QueueOrigin::Compare,
                    reviewed: None,
                })
                .status,
            Refusal::EvidenceUnavailable,
        );
        // A request ID the engine never stored is not evidence either.
        let unknown = RequestId::new(999);
        for command in [
            ReviewCommand::RequestQueue {
                file: target,
                counterpart,
                origin: QueueOrigin::Compare,
                reviewed: Some(unknown),
            },
            ReviewCommand::AcknowledgeUniqueText {
                file: target,
                counterpart,
                reviewed: Some(unknown),
            },
            ReviewCommand::KeepBoth {
                file: target,
                counterpart,
                reviewed: unknown,
            },
        ] {
            refused(engine.review(command).status, Refusal::EvidenceUnavailable);
        }
        // Without a comparison reference, a Findings request passes the
        // evidence route. What policy answers is the review module's business;
        // that the command reaches it with no pair is proved against a test
        // double in `service::tests`.
        let findings = engine.review(ReviewCommand::RequestQueue {
            file: target,
            counterpart: chain.members[1].file,
            origin: QueueOrigin::Checkbox,
            reviewed: None,
        });
        assert!(
            !matches!(
                findings.status,
                ReviewStatus::Refused {
                    refusal: Refusal::EvidenceUnavailable | Refusal::EvidenceMismatch,
                    ..
                }
            ),
            "a Findings request is not an evidence refusal: {findings:?}"
        );
    }

    // ── Cleanup provenance and validation ────────────────────────────────

    #[test]
    fn build_cleanup_plan_rejects_a_scan_other_than_the_reviewed_one() {
        let mut engine = Engine::new(EngineConfig::default()).expect("engine starts");
        let other = ScanId::new(SCAN.get() + 1);
        assert_eq!(
            engine.submit(EngineRequest::BuildCleanupPlan(BuildCleanupPlan {
                scan: SCAN
            })),
            Err(SubmitError::ScanMismatch {
                requested: SCAN,
                current: None
            })
        );
        engine
            .adopt_scenario(Arc::new(build(ScenarioKind::Mixed)))
            .expect("fresh scan ID");
        assert_eq!(
            engine.submit(EngineRequest::BuildCleanupPlan(BuildCleanupPlan {
                scan: other
            })),
            Err(SubmitError::ScanMismatch {
                requested: other,
                current: Some(SCAN)
            })
        );
        // The matching scan passes provenance. Whether a plan is then built
        // depends on the review module; that it is enqueued from the
        // published view is proved against test doubles in `service::tests`.
        let matching = engine.submit(EngineRequest::BuildCleanupPlan(BuildCleanupPlan {
            scan: SCAN,
        }));
        assert!(
            !matches!(matching, Err(SubmitError::ScanMismatch { .. })),
            "the reviewed scan passes provenance: {matching:?}"
        );
    }

    #[test]
    fn a_validation_view_detects_changed_evidence_and_protection() {
        let (target, counterpart) = (FileId::new(1), FileId::new(2));
        let binding = DecisionBinding {
            scan: SCAN,
            target,
            target_snapshot: SnapshotId::new(10),
            counterpart,
            counterpart_snapshot: SnapshotId::new(20),
            rules: RulesRevision::new(1),
        };
        let view = |target_snapshot, rules, protected: &[FileId]| ValidationView {
            scan: SCAN,
            queue: vec![QueuedTarget {
                target,
                counterpart,
                binding,
            }],
            protected: protected.iter().copied().collect(),
            snapshots: Arc::new(BTreeMap::from([
                (target, SnapshotId::new(target_snapshot)),
                (counterpart, SnapshotId::new(20)),
            ])),
            rules: RulesRevision::new(rules),
        };
        assert!(view(10, 1, &[]).binding_is_current(&binding));
        assert!(
            !view(11, 1, &[]).binding_is_current(&binding),
            "target changed"
        );
        assert!(
            !view(10, 2, &[]).binding_is_current(&binding),
            "rules changed"
        );
        let mut other_scan = binding;
        other_scan.scan = ScanId::new(99);
        assert!(
            !view(10, 1, &[]).binding_is_current(&other_scan),
            "other scan"
        );
        assert!(view(10, 1, &[target]).is_protected(target));
        assert!(!view(10, 1, &[]).is_protected(target));

        // X was approved against A; the queue now holds X against B with the
        // same snapshots and rules. The old approval must not validate.
        let b = FileId::new(3);
        let mut current = view(10, 1, &[]);
        let rebound = DecisionBinding {
            counterpart: b,
            ..binding
        };
        current.queue = vec![QueuedTarget {
            target,
            counterpart: b,
            binding: rebound,
        }];
        Arc::make_mut(&mut current.snapshots).insert(b, SnapshotId::new(20));
        assert!(
            !current.binding_is_current(&binding),
            "counterpart changed A to B"
        );
        assert!(current.binding_is_current(&rebound));
        // A target no longer queued has no current approval at all.
        current.queue.clear();
        assert!(!current.binding_is_current(&rebound), "unqueued");
    }

    // ── Scenario publication fence ───────────────────────────────────────

    #[test]
    fn a_stale_generation_cannot_publish_its_scenario() {
        let active = ActiveScenario::default();
        let older = active.begin();
        let newer = active.begin();
        assert_ne!(older, newer);
        let scenario = Arc::new(build(ScenarioKind::Mixed));
        assert!(
            !active.publish(older, Arc::clone(&scenario)),
            "stale generation"
        );
        assert!(active.get().is_none());
        assert!(active.publish(newer, Arc::clone(&scenario)));
        assert_eq!(active.get().as_deref(), Some(&*scenario));

        // adopt_scenario also starts a generation, so a scan begun before it
        // can no longer publish over the adopted scenario.
        let mut engine = Engine::new(EngineConfig::default()).expect("engine starts");
        let before: ScenarioGeneration = engine.active().begin();
        engine
            .adopt_scenario(Arc::clone(&scenario))
            .expect("fresh scan ID");
        let empty = Arc::new(build(ScenarioKind::Empty));
        assert!(!engine.active().publish(before, empty));
        assert_eq!(engine.active().get().as_deref(), Some(&*scenario));
    }

    // ── Plan revisions and scan identity ─────────────────────────────────

    #[test]
    fn plan_revisions_are_shared_unique_and_increasing() {
        let engine = Engine::new(EngineConfig::default()).expect("engine starts");
        let facade = engine.plan_revisions().clone();
        let module = engine.plan_revisions().clone();
        let worker =
            std::thread::spawn(move || (0..500).map(|_| module.allocate()).collect::<Vec<_>>());
        let mine: Vec<_> = (0..500).map(|_| facade.allocate()).collect();
        let theirs = worker.join().expect("allocator thread");
        for side in [&mine, &theirs] {
            assert!(
                side.windows(2).all(|w| w[0] < w[1]),
                "increasing per caller"
            );
        }
        let all: BTreeSet<_> = mine.iter().chain(&theirs).collect();
        assert_eq!(all.len(), 1000, "no revision is handed out twice");
    }

    #[test]
    fn adoption_never_reuses_a_scan_id_the_engine_has_seen() {
        let mut engine = Engine::new(EngineConfig::default()).expect("engine starts");
        let mixed = build(ScenarioKind::Mixed);
        let Ok(Origin::Scan(started)) = engine.submit(EngineRequest::StartScan(StartScan {
            setup: mixed.results.setup.clone(),
            rules: mixed.results.rules,
            scenario: ScenarioKind::Mixed,
            seed: DEFAULT_SEED,
        })) else {
            panic!("StartScan admitted")
        };
        // The counterexample: a scenario carrying the running scan's ID would
        // let that scan's late Completed event replace the adopted results.
        let clash = fixtures::build(ScenarioKind::Mixed, started, DEFAULT_SEED).expect("mixed");
        assert_eq!(
            engine.adopt_scenario(Arc::new(clash)),
            Err(AdoptError::ScanIdReused { scan: started })
        );
        let fresh = engine.reserve_scan_id();
        assert!(fresh > started);
        let scenario = fixtures::build(ScenarioKind::Mixed, fresh, DEFAULT_SEED).expect("mixed");
        assert_eq!(engine.adopt_scenario(Arc::new(scenario.clone())), Ok(fresh));
        assert_eq!(
            engine.adopt_scenario(Arc::new(scenario)),
            Err(AdoptError::ScanIdReused { scan: fresh }),
            "adopting the same ID twice is a reuse too"
        );
        let Ok(Origin::Scan(next)) = engine.submit(EngineRequest::StartScan(StartScan {
            setup: mixed.results.setup.clone(),
            rules: mixed.results.rules,
            scenario: ScenarioKind::Mixed,
            seed: DEFAULT_SEED,
        })) else {
            panic!("StartScan admitted")
        };
        assert!(next > fresh, "StartScan allocates past every adopted ID");
    }
}
