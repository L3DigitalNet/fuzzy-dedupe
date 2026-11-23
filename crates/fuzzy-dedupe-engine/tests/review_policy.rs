// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! PV-T31-001 to PV-T31-004 (plan T31, superseding T7): the C-05 review and
//! queue policy.
//!
//! Every test sits in the `review_policy` module so the plan's proof filter
//! `-E 'test(review_policy)'` selects them all; nextest's `test()` predicate
//! matches the test path, not the binary name. The exhaustive tier at the
//! plan's bound lives in `review_policy::enumeration`, ignored here and run
//! by the release-mode step of `.github/workflows/rust.yml`; the debug gate
//! runs a smaller exhaustive bound plus seeded depth-24 walks.
//!
//! The oracles are independent of the policy code. The enumeration and the
//! walks check every step against `Oracle`, a separate model of the
//! decisions, their bindings, queue eligibility and the expected
//! invalidation lists, written from the spec and plan; counterparts are
//! recomputed with this file's own strength order. No oracle calls the
//! production `ValidationView::binding_is_current` or the `MatchEdge` and
//! `Group` edge helpers; edges are read from their raw fields.
//!
//! The enumeration and the walks spread their shapes over every available
//! core with scoped threads (see `parallel_map`); they need no thread or
//! RNG crate, and their counts do not depend on the core count.
//!
//! The service is driven directly, without the engine: `apply` takes the
//! engine-resolved `ReviewedPair`, so these tests construct that evidence
//! themselves from the current state, as the comparison service would.

mod review_policy {
    use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};
    use std::fmt;
    use std::num::NonZeroUsize;
    use std::panic::{self, AssertUnwindSafe};
    use std::rc::Rc;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::thread;
    use std::time::{Duration, Instant, SystemTime};

    use fuzzy_dedupe_domain::{
        CompareMode, CounterpartBasis, CounterpartLink, Coverage, DecisionBinding, DecisionKind,
        EvidenceUpdate, FileId, Group, GroupId, IgnoreOptions, InvalidatedDecision, MatchEdge,
        MatchMode, MatchingRules, Measurement, Member, MemberDecision, MemberRole, Permille,
        QueueOrigin, QueueScope, QueuedTarget, Refusal, ReviewedPair, RulesRevision, ScanFolder,
        ScanId, ScanResults, ScanSetup, SnapshotId, ValidationView, Verification,
    };
    use fuzzy_dedupe_engine::fixtures::{self, DEFAULT_SEED, ScenarioKind, SeededDecision};
    use fuzzy_dedupe_engine::review::{
        FileReviewState, ReviewCommand, ReviewOutcome, ReviewService, ReviewStatus,
    };

    const SCAN: ScanId = ScanId::new(1);
    const RULES: RulesRevision = RulesRevision::new(1);
    /// Added to a file ID to give it a changed snapshot; initial snapshots
    /// equal the file ID, so the two never collide in these small shapes.
    const TOUCHED: u64 = 100;

    // ── Building result sets ─────────────────────────────────────────────

    fn id(raw: u64) -> FileId {
        FileId::new(raw)
    }

    fn permille(value: u16) -> Permille {
        Permille::new(value).expect("per-mille in range")
    }

    const BYTES: Measurement = Measurement::Identical(Verification::ByteVerified);

    fn similar(value: u16) -> Measurement {
        Measurement::Similar(permille(value))
    }

    /// One group to build: files `files[0]` (the reference) and the rest,
    /// joined by `edges` (both ends in `files`). Each `(file, other)` in
    /// `no_unique` gives `file` 0 unique lines against `other`, the §10.1
    /// value that lets a non-identical row skip Review unique text.
    #[derive(Clone, Debug)]
    struct GroupSpec {
        group: u64,
        files: Vec<u64>,
        edges: Vec<(u64, u64, Measurement)>,
        folder_protected: Vec<u64>,
        no_unique: Vec<(u64, u64)>,
    }

    fn edge(a: u64, b: u64, measurement: Measurement) -> MatchEdge {
        MatchEdge {
            a: id(a),
            b: id(b),
            measurement,
            // Distinct per direction so a link built from the wrong end of
            // an edge shows up as a wrong Unique value.
            unique_lines_a: a,
            unique_lines_b: b + 10,
        }
    }

    /// The edge joining `x` and `y`, found from the raw endpoint fields.
    /// This and [`seen_from`] stand in for the production `MatchEdge` and
    /// `Group` helpers, so that a bug in those cannot hide from the oracle.
    fn edge_of(edges: &[MatchEdge], x: FileId, y: FileId) -> Option<&MatchEdge> {
        edges
            .iter()
            .find(|e| (e.a, e.b) == (x, y) || (e.a, e.b) == (y, x))
    }

    /// `e` as `file` sees it: the measurement, with a containment's
    /// direction flipped from the `b` end, and `file`'s own unique lines.
    fn seen_from(e: &MatchEdge, file: FileId) -> (Measurement, u64) {
        use fuzzy_dedupe_domain::ContainmentDirection::{ContainsCounterpart, InsideCounterpart};
        if e.a == file {
            return (e.measurement, e.unique_lines_a);
        }
        assert_eq!(e.b, file, "{file:?} is not an endpoint of {e:?}");
        let flipped = match e.measurement {
            Measurement::Containment {
                coverage,
                direction,
            } => Measurement::Containment {
                coverage,
                direction: if direction == InsideCounterpart {
                    ContainsCounterpart
                } else {
                    InsideCounterpart
                },
            },
            other => other,
        };
        (flipped, e.unique_lines_b)
    }

    /// This file's own strength order (spec §6, owner decision of
    /// 2026-11-22): match type, then per-mille.
    fn strength(m: Measurement) -> (u8, u16) {
        match m {
            Measurement::Identical(_) => (3, 1000),
            Measurement::SameText => (2, 1000),
            Measurement::Similar(p) => (1, p.get()),
            Measurement::Containment { coverage, .. } => (0, coverage.get()),
        }
    }

    /// The §10.1 rule, recomputed independently: the reference when
    /// adjacent, else the strongest neighbour, ties to the lower file ID.
    fn oracle_rule(edges: &[MatchEdge], reference: FileId, file: FileId) -> Option<FileId> {
        let mut best: Option<((u8, u16), FileId)> = None;
        for e in edges {
            let other = if e.a == file {
                e.b
            } else if e.b == file {
                e.a
            } else {
                continue;
            };
            if other == reference {
                return Some(reference);
            }
            let s = strength(e.measurement);
            let better = match best {
                None => true,
                Some((held, held_file)) => s > held || (s == held && other < held_file),
            };
            if better {
                best = Some((s, other));
            }
        }
        best.map(|(_, f)| f)
    }

    fn build_group(spec: &GroupSpec) -> Group {
        let edges: Vec<MatchEdge> = spec
            .edges
            .iter()
            .map(|&(a, b, m)| {
                let mut e = edge(a, b, m);
                if spec.no_unique.contains(&(a, b)) {
                    e.unique_lines_a = 0;
                }
                if spec.no_unique.contains(&(b, a)) {
                    e.unique_lines_b = 0;
                }
                e
            })
            .collect();
        let reference = id(spec.files[0]);
        let members = spec
            .files
            .iter()
            .map(|&raw| {
                let file = id(raw);
                let role = if file == reference {
                    MemberRole::Reference
                } else {
                    let counterpart =
                        oracle_rule(&edges, reference, file).expect("connected group");
                    let e =
                        edge_of(&edges, file, counterpart).expect("rule counterpart has an edge");
                    let (measurement, unique_lines) = seen_from(e, file);
                    MemberRole::Member(CounterpartLink {
                        counterpart,
                        basis: CounterpartBasis::Rule,
                        measurement: Some(measurement),
                        unique_lines,
                    })
                };
                Member {
                    file,
                    name: format!("file{raw}.txt"),
                    folder: "/fixture".into(),
                    display_path: format!("/fixture/file{raw}.txt"),
                    size_bytes: 100,
                    modified: SystemTime::UNIX_EPOCH + Duration::from_secs(raw),
                    folder_protected: spec.folder_protected.contains(&raw),
                    snapshot: SnapshotId::new(raw),
                    role,
                }
            })
            .collect();
        Group {
            id: GroupId::new(spec.group),
            label: format!("G{:03}", spec.group),
            members,
            edges,
        }
    }

    fn results_of(specs: &[GroupSpec]) -> Arc<ScanResults> {
        Arc::new(ScanResults {
            scan: SCAN,
            setup: ScanSetup {
                folders: vec![ScanFolder {
                    path: "/fixture".into(),
                    protect: false,
                }],
                file_types: vec!["txt".into()],
            },
            rules: MatchingRules {
                mode: MatchMode::ExactAndFuzzy,
                minimum_similarity: permille(800),
                ignore: IgnoreOptions {
                    whitespace: false,
                    line_endings: false,
                    letter_case: false,
                    punctuation: false,
                },
                revision: RULES,
            },
            groups: specs.iter().map(build_group).collect(),
            issues: Vec::new(),
            coverage: Coverage::Complete,
        })
    }

    fn single(files: &[u64], edges: &[(u64, u64, Measurement)]) -> Arc<ScanResults> {
        results_of(&[GroupSpec {
            group: 1,
            files: files.to_vec(),
            edges: edges.to_vec(),
            folder_protected: Vec::new(),
            no_unique: Vec::new(),
        }])
    }

    fn loaded(results: &Arc<ScanResults>) -> ReviewService {
        let mut service = ReviewService::new();
        service.load_scan(Arc::clone(results));
        service
    }

    fn view(service: &ReviewService) -> ValidationView {
        service
            .validation_view()
            .expect("review service implemented")
            .expect("a scan is loaded")
    }

    fn queued(service: &ReviewService) -> BTreeSet<FileId> {
        view(service).queue.iter().map(|q| q.target).collect()
    }

    // ── Commands ─────────────────────────────────────────────────────────

    fn queue(file: u64, counterpart: u64) -> ReviewCommand {
        ReviewCommand::RequestQueue {
            file: id(file),
            counterpart: id(counterpart),
            origin: QueueOrigin::Checkbox,
            reviewed: None,
        }
    }

    fn ack(file: u64, counterpart: u64) -> ReviewCommand {
        ReviewCommand::AcknowledgeUniqueText {
            file: id(file),
            counterpart: id(counterpart),
            reviewed: None,
        }
    }

    /// The comparison evidence the engine would resolve for `target` against
    /// `counterpart` on the current view.
    fn pair(service: &ReviewService, target: FileId, counterpart: FileId) -> ReviewedPair {
        let v = view(service);
        ReviewedPair {
            scan: v.scan,
            target,
            target_snapshot: v.snapshots[&target],
            counterpart,
            counterpart_snapshot: v.snapshots[&counterpart],
            rules: v.rules,
            mode: CompareMode::Original,
            unique_lines: 2,
        }
    }

    /// Runs `command` with no comparison evidence.
    fn run(service: &mut ReviewService, command: ReviewCommand) -> ReviewOutcome {
        service.apply(command, None)
    }

    /// Runs a Compare-originated command with evidence for its pair.
    fn run_reviewed(service: &mut ReviewService, command: ReviewCommand) -> ReviewOutcome {
        run_measured(service, command, CompareMode::Original, 2)
    }

    /// [`run_reviewed`] with evidence measured in `mode` at `unique` lines.
    fn run_measured(
        service: &mut ReviewService,
        command: ReviewCommand,
        mode: CompareMode,
        unique: u64,
    ) -> ReviewOutcome {
        let (file, counterpart) = match &command {
            ReviewCommand::RequestQueue {
                file, counterpart, ..
            }
            | ReviewCommand::AcknowledgeUniqueText {
                file, counterpart, ..
            }
            | ReviewCommand::KeepBoth {
                file, counterpart, ..
            } => (*file, *counterpart),
            other => panic!("not a reviewed command: {other:?}"),
        };
        let evidence = ReviewedPair {
            mode,
            unique_lines: unique,
            ..pair(service, file, counterpart)
        };
        service.apply(command, Some(evidence))
    }

    fn compare_queue(file: u64, counterpart: u64) -> ReviewCommand {
        ReviewCommand::RequestQueue {
            file: id(file),
            counterpart: id(counterpart),
            origin: QueueOrigin::Compare,
            reviewed: Some(fuzzy_dedupe_domain::RequestId::new(1)),
        }
    }

    fn compare_ack(file: u64, counterpart: u64) -> ReviewCommand {
        ReviewCommand::AcknowledgeUniqueText {
            file: id(file),
            counterpart: id(counterpart),
            reviewed: Some(fuzzy_dedupe_domain::RequestId::new(1)),
        }
    }

    fn keep_both(file: u64, counterpart: u64) -> ReviewCommand {
        ReviewCommand::KeepBoth {
            file: id(file),
            counterpart: id(counterpart),
            reviewed: fuzzy_dedupe_domain::RequestId::new(1),
        }
    }

    fn request_from(origin: QueueOrigin, file: u64, counterpart: u64) -> ReviewCommand {
        ReviewCommand::RequestQueue {
            file: id(file),
            counterpart: id(counterpart),
            origin,
            reviewed: (origin == QueueOrigin::Compare)
                .then_some(fuzzy_dedupe_domain::RequestId::new(1)),
        }
    }

    /// Runs `command`, with comparison evidence exactly when it names a
    /// comparison, as the engine would.
    fn run_any(service: &mut ReviewService, command: ReviewCommand) -> ReviewOutcome {
        let reviewed = matches!(
            command,
            ReviewCommand::RequestQueue {
                reviewed: Some(_),
                ..
            } | ReviewCommand::AcknowledgeUniqueText {
                reviewed: Some(_),
                ..
            } | ReviewCommand::KeepBoth { .. }
        );
        if reviewed {
            run_reviewed(service, command)
        } else {
            run(service, command)
        }
    }

    const ALL_ORIGINS: [QueueOrigin; 5] = [
        QueueOrigin::Checkbox,
        QueueOrigin::RowMenu,
        QueueOrigin::Shortcut,
        QueueOrigin::Compare,
        QueueOrigin::UniqueTextReview,
    ];

    fn touch(file: u64) -> ReviewCommand {
        ReviewCommand::UpdateEvidence(EvidenceUpdate::FileSnapshot {
            file: id(file),
            snapshot: SnapshotId::new(file + TOUCHED),
        })
    }

    fn rules(raw: u64) -> ReviewCommand {
        ReviewCommand::UpdateEvidence(EvidenceUpdate::Rules(RulesRevision::new(raw)))
    }

    fn inv(file: u64, kind: DecisionKind) -> InvalidatedDecision {
        InvalidatedDecision {
            file: id(file),
            kind,
        }
    }

    fn decision_of(outcome: &ReviewOutcome, file: u64) -> Option<MemberDecision> {
        outcome
            .changed
            .iter()
            .find(|r| r.file == id(file))
            .map(|r| r.decision)
    }

    fn refused(status: ReviewStatus, file: u64, refusal: Refusal) {
        assert_eq!(
            status,
            ReviewStatus::Refused {
                file: id(file),
                refusal
            }
        );
    }

    // ── PV-T31-001: the independent oracle ───────────────────────────────

    /// A binding as this oracle records it. The scan is the fixed `SCAN`,
    /// so it is left out and added back by [`Bind::domain`].
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    struct Bind {
        target: FileId,
        target_snapshot: SnapshotId,
        counterpart: FileId,
        counterpart_snapshot: SnapshotId,
        rules: RulesRevision,
    }

    impl Bind {
        const fn domain(self) -> DecisionBinding {
            DecisionBinding {
                scan: SCAN,
                target: self.target,
                target_snapshot: self.target_snapshot,
                counterpart: self.counterpart,
                counterpart_snapshot: self.counterpart_snapshot,
                rules: self.rules,
            }
        }
    }

    /// The comparison evidence a generated Compare command carries. Two
    /// variants, so that both modes and a zero and a nonzero Unique count
    /// reach the policy: evidence only decides a row's Unique count when
    /// the pair has no direct edge, and there 0 against 2 is exactly the
    /// owner rule's review boundary.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    enum Ev {
        Original2,
        Normalized0,
    }

    impl Ev {
        const ALL: [Self; 2] = [Self::Original2, Self::Normalized0];

        const fn mode(self) -> CompareMode {
            match self {
                Self::Original2 => CompareMode::Original,
                Self::Normalized0 => CompareMode::Normalized,
            }
        }

        const fn unique(self) -> u64 {
            match self {
                Self::Original2 => 2,
                Self::Normalized0 => 0,
            }
        }
    }

    /// A Compare choice as the oracle holds it: the link, and for a pair
    /// with no direct edge the binding its Unique count was measured under,
    /// since that count is evidence only while the binding is current.
    type Choice = (CounterpartLink, Option<Bind>);

    /// The oracle: an independent model of everything C-05 says the review
    /// state is, written from the spec and the plan rather than from
    /// `policy.rs`. It predicts each command's exact status, invalidated
    /// list, changed rows and resulting view, and the tests compare all four
    /// with the service after every step. It never calls the production
    /// `ValidationView::binding_is_current` or the `MatchEdge` and `Group`
    /// edge helpers; binding currency is its own comparison against its own
    /// snapshots, counterparts and rules, and edges are read from their raw
    /// fields by [`edge_of`] and [`seen_from`].
    ///
    /// The rules it encodes, each from its source:
    /// - a decision is valid only while its binding of (target, its
    ///   snapshot, counterpart, its snapshot, rules) equals the current one,
    ///   and a change to any component invalidates it (C-05 I-7, DR-008);
    /// - a reference's decisions bind to itself as counterpart (settled
    ///   interpretation 1);
    /// - FR-025's two refusals, and Compare choices that become the
    ///   counterpart (§10.1);
    /// - review first (owner decision of 2026-11-23): a row needs Review
    ///   unique text exactly when its match is not Same bytes and its
    ///   Unique count against its current counterpart is above 0, or is not
    ///   known because the comparison it came from is stale;
    /// - a Compare re-aim that is not refused commits the new counterpart,
    ///   and its invalidations, even when the request then needs review
    ///   (PV-T31-003);
    /// - fresh comparison evidence for an unchanged edge-less counterpart
    ///   refreshes its Unique count;
    /// - Keep both keeps both displayed files (§11.5);
    /// - Make this the reference in the C-05 order;
    /// - Queue identical copies: counterpart is the reference, Same bytes
    ///   byte-verified, not kept, protected, queued or blocked (§11.4).
    #[derive(Clone, Debug, PartialEq, Eq, Hash)]
    struct Oracle {
        reference: FileId,
        chosen: BTreeMap<FileId, Choice>,
        snapshots: BTreeMap<FileId, SnapshotId>,
        rules: RulesRevision,
        kept: BTreeMap<FileId, Bind>,
        protect: BTreeMap<FileId, Bind>,
        acks: BTreeMap<FileId, Bind>,
        queue: BTreeMap<FileId, Bind>,
    }

    /// What the oracle expects one command to produce.
    struct Expected {
        status: ReviewStatus,
        invalidated: Vec<InvalidatedDecision>,
    }

    impl Oracle {
        fn new(group: &Group) -> Self {
            Self {
                reference: group.members[0].file,
                chosen: BTreeMap::new(),
                snapshots: group.members.iter().map(|m| (m.file, m.snapshot)).collect(),
                rules: RULES,
                kept: BTreeMap::new(),
                protect: BTreeMap::new(),
                acks: BTreeMap::new(),
                queue: BTreeMap::new(),
            }
        }

        fn link(&self, group: &Group, file: FileId) -> Option<CounterpartLink> {
            if file == self.reference {
                return None;
            }
            if let Some((link, _)) = self.chosen.get(&file) {
                return Some(*link);
            }
            let counterpart = oracle_rule(&group.edges, self.reference, file)?;
            let (measurement, unique_lines) =
                seen_from(edge_of(&group.edges, file, counterpart)?, file);
            Some(CounterpartLink {
                counterpart,
                basis: CounterpartBasis::Rule,
                measurement: Some(measurement),
                unique_lines,
            })
        }

        fn counterpart(&self, group: &Group, file: FileId) -> Option<FileId> {
            self.link(group, file).map(|l| l.counterpart)
        }

        fn against(&self, file: FileId, counterpart: FileId) -> Bind {
            Bind {
                target: file,
                target_snapshot: self.snapshots[&file],
                counterpart,
                counterpart_snapshot: self.snapshots[&counterpart],
                rules: self.rules,
            }
        }

        fn bind(&self, group: &Group, file: FileId) -> Bind {
            self.against(file, self.counterpart(group, file).unwrap_or(file))
        }

        /// The row's Unique count, if known: an edge's own value, or a
        /// Compare measurement still bound to the current observation.
        fn unique(&self, group: &Group, file: FileId) -> Option<u64> {
            let link = self.link(group, file)?;
            if edge_of(&group.edges, file, link.counterpart).is_some() {
                return Some(link.unique_lines);
            }
            let measured = self.chosen.get(&file).and_then(|(_, m)| *m)?;
            (measured == self.against(file, link.counterpart)).then_some(link.unique_lines)
        }

        /// The owner's review rule (see the type's rule list): never for the
        /// reference or a Same bytes row, otherwise unless the Unique count
        /// is known to be 0.
        fn needs_review(&self, group: &Group, file: FileId) -> bool {
            let Some(counterpart) = self.counterpart(group, file) else {
                return false;
            };
            let identical = edge_of(&group.edges, file, counterpart)
                .is_some_and(|e| matches!(e.measurement, Measurement::Identical(_)));
            !identical && self.unique(group, file) != Some(0)
        }

        fn protected(&self, group: &Group, file: FileId) -> bool {
            self.protect.contains_key(&file)
                || group
                    .members
                    .iter()
                    .any(|m| m.file == file && m.folder_protected)
        }

        /// Records a Compare review of `file` against `counterpart` under
        /// evidence `ev`, as §10.1 makes the reviewed member the counterpart.
        /// A direct edge supplies the measurement and Unique count; without
        /// one the Unique count is the comparison's, measured under the
        /// current binding. Reviewing the current counterpart again keeps
        /// the link and, for an edge-less pair, takes the fresh count.
        /// Returns whether the counterpart changed.
        fn choose(&mut self, group: &Group, file: FileId, counterpart: FileId, ev: Ev) -> bool {
            let e = edge_of(&group.edges, file, counterpart);
            let measured = self.against(file, counterpart);
            if self.counterpart(group, file) == Some(counterpart) {
                if e.is_none() {
                    let (link, at) = self.chosen.get_mut(&file).expect("edge-less is chosen");
                    link.unique_lines = ev.unique();
                    *at = Some(measured);
                }
                return false;
            }
            let link = CounterpartLink {
                counterpart,
                basis: CounterpartBasis::ReviewedInCompare,
                measurement: e.map(|e| seen_from(e, file).0),
                unique_lines: e.map_or(ev.unique(), |e| seen_from(e, file).1),
            };
            self.chosen
                .insert(file, (link, e.is_none().then_some(measured)));
            true
        }

        /// I-7 and I-10: every decision whose binding is no longer current
        /// goes, and so does an approval whose row needs review and holds
        /// no current acknowledgment.
        fn reconcile(&mut self, group: &Group, out: &mut Vec<InvalidatedDecision>) {
            for m in &group.members {
                let file = m.file;
                let now = self.bind(group, file);
                for (map, kind) in [
                    (&mut self.kept, DecisionKind::Kept),
                    (&mut self.protect, DecisionKind::Protect),
                    (&mut self.acks, DecisionKind::Acknowledgment),
                ] {
                    if map.get(&file).is_some_and(|b| *b != now) {
                        map.remove(&file);
                        out.push(InvalidatedDecision { file, kind });
                    }
                }
                if let Some(b) = self.queue.get(&file).copied() {
                    let acknowledged =
                        !self.needs_review(group, file) || self.acks.get(&file) == Some(&now);
                    if b != now || !acknowledged {
                        self.queue.remove(&file);
                        out.push(InvalidatedDecision {
                            file,
                            kind: DecisionKind::QueueApproval,
                        });
                    }
                }
            }
        }

        /// A queue request against `counterpart`. `compare` carries the
        /// evidence of a Compare request, whose counterpart may differ from
        /// the current one.
        fn queue_request(
            &mut self,
            group: &Group,
            file: FileId,
            counterpart: FileId,
            compare: Option<Ev>,
            out: &mut Vec<InvalidatedDecision>,
        ) -> ReviewStatus {
            let refuse = |refusal| ReviewStatus::Refused { file, refusal };
            if file == self.reference {
                return refuse(Refusal::Reference);
            }
            if self.protected(group, file) {
                return refuse(Refusal::Protected);
            }
            if compare.is_none() && self.counterpart(group, file) != Some(counterpart) {
                return refuse(Refusal::EvidenceMismatch);
            }
            // A target already queued against `counterpart` cannot trip
            // either FR-025 check (I-3 keeps its counterpart unqueued, and
            // no queued file names a queued file), so checking them first
            // refuses nothing the no-op below would have accepted.
            if self.queue.contains_key(&counterpart) {
                return refuse(Refusal::CounterpartQueued);
            }
            if self.queue.values().any(|b| b.counterpart == file) {
                return refuse(Refusal::CounterpartOfQueuedFile);
            }
            if let Some(ev) = compare {
                self.choose(group, file, counterpart, ev);
                self.reconcile(group, out);
            }
            if self.queue.contains_key(&file) {
                return ReviewStatus::Applied;
            }
            if self.needs_review(group, file)
                && self.acks.get(&file) != Some(&self.bind(group, file))
            {
                return ReviewStatus::NeedsReview { file, counterpart };
            }
            self.kept.remove(&file);
            let b = self.bind(group, file);
            self.queue.insert(file, b);
            ReviewStatus::Queued { files: 1 }
        }

        #[expect(
            clippy::too_many_lines,
            reason = "one arm per C-05 operation, kept in one match so the oracle reads as the \
                      operation table it encodes"
        )]
        fn apply(&mut self, group: &Group, op: Op) -> Expected {
            let mut out = Vec::new();
            let applied = ReviewStatus::Applied;
            let status = match op {
                Op::Queue(f) => {
                    let c = self.counterpart(group, f).unwrap_or(f);
                    self.queue_request(group, f, c, None, &mut out)
                }
                Op::CompareQueue(f, c, ev) => self.queue_request(group, f, c, Some(ev), &mut out),
                Op::Ack(f) | Op::CompareAck(f, _, _) if f == self.reference => {
                    ReviewStatus::Refused {
                        file: f,
                        refusal: Refusal::Reference,
                    }
                }
                Op::Ack(f) => {
                    let b = self.bind(group, f);
                    self.acks.insert(f, b);
                    applied
                }
                Op::CompareAck(f, c, ev) => {
                    // An acknowledgment of the unchanged counterpart lands
                    // before reconciling, so a refreshed count that now
                    // needs review keeps the approval it acknowledges; one
                    // that moves the counterpart reconciles first, so the
                    // decisions bound to the old counterpart are listed.
                    if self.choose(group, f, c, ev) {
                        self.reconcile(group, &mut out);
                    }
                    let b = self.bind(group, f);
                    self.acks.insert(f, b);
                    applied
                }
                Op::KeepBoth(f, c, ev) => {
                    if f != self.reference {
                        self.choose(group, f, c, ev);
                    }
                    self.reconcile(group, &mut out);
                    for x in [f, c] {
                        self.queue.remove(&x);
                        let b = self.bind(group, x);
                        self.kept.insert(x, b);
                    }
                    applied
                }
                Op::Unqueue(f) => {
                    self.queue.remove(&f);
                    applied
                }
                Op::Kept(f, true) => {
                    self.queue.remove(&f);
                    let b = self.bind(group, f);
                    self.kept.insert(f, b);
                    applied
                }
                Op::Kept(f, false) => {
                    self.kept.remove(&f);
                    applied
                }
                Op::Protect(f, true) => {
                    self.queue.remove(&f);
                    let b = self.bind(group, f);
                    self.protect.insert(f, b);
                    applied
                }
                Op::Protect(f, false) => {
                    self.protect.remove(&f);
                    applied
                }
                Op::MakeReference(f) if f == self.reference => applied,
                Op::MakeReference(f) => {
                    if self.queue.remove(&f).is_some() {
                        out.push(inv(f.get(), DecisionKind::QueueApproval));
                    }
                    self.reference = f;
                    self.chosen.clear();
                    let before: Vec<FileId> = self.queue.keys().copied().collect();
                    self.reconcile(group, &mut out);
                    let unqueued = before
                        .iter()
                        .filter(|x| !self.queue.contains_key(x))
                        .count();
                    ReviewStatus::ReferenceChanged { unqueued }
                }
                Op::QueueIdentical => {
                    let mut files = 0;
                    let r = self.reference;
                    for m in &group.members {
                        let f = m.file;
                        let eligible = f != r
                            && !self.protected(group, f)
                            && !self.kept.contains_key(&f)
                            && !self.queue.contains_key(&f)
                            && self.counterpart(group, f) == Some(r)
                            && edge_of(&group.edges, f, r).is_some_and(|e| e.measurement == BYTES)
                            && !self.queue.values().any(|b| b.counterpart == f);
                        if eligible {
                            let b = self.bind(group, f);
                            self.queue.insert(f, b);
                            files += 1;
                        }
                    }
                    ReviewStatus::Queued { files }
                }
                Op::Touch(f) => {
                    let toggled = toggled_snapshot(self.snapshots[&f], f);
                    self.snapshots.insert(f, toggled);
                    applied
                }
                Op::ToggleRules => {
                    self.rules = toggled_rules(self.rules);
                    applied
                }
                Op::Clear => {
                    for (map, kind) in [
                        (&mut self.kept, DecisionKind::Kept),
                        (&mut self.protect, DecisionKind::Protect),
                        (&mut self.acks, DecisionKind::Acknowledgment),
                        (&mut self.queue, DecisionKind::QueueApproval),
                    ] {
                        out.extend(map.keys().map(|&file| InvalidatedDecision { file, kind }));
                        map.clear();
                    }
                    applied
                }
            };
            if !matches!(status, ReviewStatus::Refused { .. }) {
                self.reconcile(group, &mut out);
            }
            out.sort_by_key(|d| (d.file, d.kind));
            out.dedup();
            Expected {
                status,
                invalidated: out,
            }
        }

        /// The §11.4 Decision column, in this oracle's own precedence:
        /// Reference, Protected, Queued, Kept, Counterpart queued, Review
        /// first (needs review and holds no acknowledgment), Unreviewed.
        fn row(&self, group: &Group, file: FileId) -> FileReviewState {
            let link = self.link(group, file);
            let decision = match link {
                None => MemberDecision::Reference,
                Some(_) if self.protected(group, file) => MemberDecision::Protected,
                Some(_) if self.queue.contains_key(&file) => MemberDecision::Queued,
                Some(_) if self.kept.contains_key(&file) => MemberDecision::Kept,
                Some(l) if self.queue.contains_key(&l.counterpart) => {
                    MemberDecision::CounterpartQueued
                }
                Some(_) if self.needs_review(group, file) && !self.acks.contains_key(&file) => {
                    MemberDecision::ReviewFirst
                }
                Some(_) => MemberDecision::Unreviewed,
            };
            FileReviewState {
                file,
                group: group.id,
                role: link.map_or(MemberRole::Reference, MemberRole::Member),
                decision,
            }
        }

        fn rows(&self, group: &Group) -> Vec<FileReviewState> {
            group
                .members
                .iter()
                .map(|m| self.row(group, m.file))
                .collect()
        }

        /// The view this state must publish.
        fn view(&self, group: &Group) -> ValidationView {
            ValidationView {
                scan: SCAN,
                queue: self
                    .queue
                    .iter()
                    .map(|(&target, b)| QueuedTarget {
                        target,
                        counterpart: b.counterpart,
                        binding: b.domain(),
                    })
                    .collect(),
                protected: group
                    .members
                    .iter()
                    .filter(|m| m.folder_protected)
                    .map(|m| m.file)
                    .chain(self.protect.keys().copied())
                    .collect(),
                snapshots: Arc::new(self.snapshots.clone()),
                rules: self.rules,
            }
        }

        /// C-05 I-1 to I-5 and I-7 over the oracle's own state, which the
        /// step has already shown equal to the service's view.
        fn check_invariants(&self, group: &Group, trace: Trace<'_>) {
            for (&target, b) in &self.queue {
                assert_ne!(target, self.reference, "{trace:?}: I-1");
                assert!(!self.protected(group, target), "{trace:?}: I-2");
                assert!(!self.queue.contains_key(&b.counterpart), "{trace:?}: I-3");
                assert_eq!(
                    *b,
                    self.bind(group, target),
                    "{trace:?}: I-7 approval current"
                );
                if self.needs_review(group, target) {
                    assert_eq!(self.acks.get(&target), Some(b), "{trace:?}: I-5");
                }
            }
            assert!(self.queue.len() < group.members.len(), "{trace:?}: I-4");
        }
    }

    const fn toggled_snapshot(now: SnapshotId, file: FileId) -> SnapshotId {
        if now.get() == file.get() {
            SnapshotId::new(file.get() + TOUCHED)
        } else {
            SnapshotId::new(file.get())
        }
    }

    const fn toggled_rules(now: RulesRevision) -> RulesRevision {
        if now.get() == RULES.get() {
            RulesRevision::new(2)
        } else {
            RULES
        }
    }

    /// One enumerated operation. Snapshot and rules changes toggle between
    /// two values; because a stale decision is dropped the moment it goes
    /// stale, only equality of these IDs matters, so a toggle stands for
    /// every change.
    #[derive(Clone, Copy, Debug)]
    enum Op {
        Queue(FileId),
        CompareQueue(FileId, FileId, Ev),
        Ack(FileId),
        CompareAck(FileId, FileId, Ev),
        KeepBoth(FileId, FileId, Ev),
        Unqueue(FileId),
        Kept(FileId, bool),
        Protect(FileId, bool),
        MakeReference(FileId),
        QueueIdentical,
        Touch(FileId),
        ToggleRules,
        Clear,
    }

    /// Every C-05 operation on every file and ordered pair, each Compare
    /// operation once per [`Ev`]: 168 operations at 5 members. Non-Compare
    /// queue origins are represented by `Checkbox`: the policy records the
    /// origin and never reads it (the origin matrix test covers all five).
    fn alphabet(files: &[FileId]) -> Vec<Op> {
        let mut ops = vec![Op::QueueIdentical, Op::ToggleRules, Op::Clear];
        for &f in files {
            ops.extend([
                Op::Queue(f),
                Op::Ack(f),
                Op::Unqueue(f),
                Op::Kept(f, true),
                Op::Kept(f, false),
                Op::Protect(f, true),
                Op::Protect(f, false),
                Op::MakeReference(f),
                Op::Touch(f),
            ]);
            for &c in files {
                if c != f {
                    for ev in Ev::ALL {
                        ops.extend([
                            Op::CompareQueue(f, c, ev),
                            Op::CompareAck(f, c, ev),
                            Op::KeepBoth(f, c, ev),
                        ]);
                    }
                }
            }
        }
        ops
    }

    /// The command `op` stands for, and its comparison evidence (built from
    /// the oracle's snapshots, which the previous step proved equal to the
    /// service's).
    fn command(oracle: &Oracle, group: &Group, op: Op) -> (ReviewCommand, Option<ReviewedPair>) {
        let evidence = |f: FileId, c: FileId, ev: Ev| ReviewedPair {
            scan: SCAN,
            target: f,
            target_snapshot: oracle.snapshots[&f],
            counterpart: c,
            counterpart_snapshot: oracle.snapshots[&c],
            rules: oracle.rules,
            mode: ev.mode(),
            unique_lines: ev.unique(),
        };
        let request = fuzzy_dedupe_domain::RequestId::new(1);
        match op {
            Op::Queue(f) => (
                ReviewCommand::RequestQueue {
                    file: f,
                    counterpart: oracle.counterpart(group, f).unwrap_or(f),
                    origin: QueueOrigin::Checkbox,
                    reviewed: None,
                },
                None,
            ),
            Op::CompareQueue(f, c, ev) => (
                ReviewCommand::RequestQueue {
                    file: f,
                    counterpart: c,
                    origin: QueueOrigin::Compare,
                    reviewed: Some(request),
                },
                Some(evidence(f, c, ev)),
            ),
            Op::Ack(f) => (
                ReviewCommand::AcknowledgeUniqueText {
                    file: f,
                    counterpart: oracle.counterpart(group, f).unwrap_or(f),
                    reviewed: None,
                },
                None,
            ),
            Op::CompareAck(f, c, ev) => (
                ReviewCommand::AcknowledgeUniqueText {
                    file: f,
                    counterpart: c,
                    reviewed: Some(request),
                },
                Some(evidence(f, c, ev)),
            ),
            Op::KeepBoth(f, c, ev) => (
                ReviewCommand::KeepBoth {
                    file: f,
                    counterpart: c,
                    reviewed: request,
                },
                Some(evidence(f, c, ev)),
            ),
            Op::Unqueue(f) => (ReviewCommand::RemoveFromQueue { file: f }, None),
            Op::Kept(f, kept) => (ReviewCommand::MarkKept { file: f, kept }, None),
            Op::Protect(f, protected) => (ReviewCommand::ProtectFile { file: f, protected }, None),
            Op::MakeReference(f) => (ReviewCommand::MakeReference { file: f }, None),
            Op::QueueIdentical => (
                ReviewCommand::QueueIdenticalCopies {
                    scope: QueueScope::AllByteIdenticalGroups,
                },
                None,
            ),
            Op::Touch(f) => (
                ReviewCommand::UpdateEvidence(EvidenceUpdate::FileSnapshot {
                    file: f,
                    snapshot: toggled_snapshot(oracle.snapshots[&f], f),
                }),
                None,
            ),
            Op::ToggleRules => (
                ReviewCommand::UpdateEvidence(EvidenceUpdate::Rules(toggled_rules(oracle.rules))),
                None,
            ),
            Op::Clear => (ReviewCommand::ClearForNewScan, None),
        }
    }

    /// Where a checked step sits, for failure messages: `at` names the
    /// shape and the tier depth or walk, `ops` the sequence so far. Every
    /// assertion in [`step`] prints it with `{trace:?}`, so a failure on any
    /// worker thread is reproducible from its message alone.
    #[derive(Clone, Copy)]
    struct Trace<'a> {
        at: &'a str,
        ops: &'a [Op],
    }

    impl fmt::Debug for Trace<'_> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(
                f,
                "{}, sequence of {}: {:?}",
                self.at,
                self.ops.len(),
                self.ops
            )
        }
    }

    /// Applies `op` to the service and the oracle and asserts the service
    /// produced exactly what the oracle predicts: status, invalidated list,
    /// changed rows and the published view; then C-05 I-1 to I-5 and I-7.
    /// A refusal must also leave the service equal to its prior state, and
    /// so must a review prompt that the oracle says changed nothing; one
    /// that committed a Compare re-aim is held to the oracle instead.
    fn step(
        service: &mut ReviewService,
        oracle: &mut Oracle,
        group: &Group,
        op: Op,
        trace: Trace<'_>,
    ) {
        let (cmd, evidence) = command(oracle, group, op);
        let rows_before = oracle.rows(group);
        let prior = service.clone();
        let oracle_prior = oracle.clone();
        let outcome = service.apply(cmd, evidence);
        let expected = oracle.apply(group, op);
        assert_eq!(outcome.status, expected.status, "{trace:?}");
        assert_eq!(outcome.invalidated, expected.invalidated, "{trace:?}");
        let rows_after = oracle.rows(group);
        let expected_changed: Vec<FileReviewState> = rows_after
            .iter()
            .zip(&rows_before)
            .filter(|(a, b)| a != b)
            .map(|(a, _)| *a)
            .collect();
        let mut changed = outcome.changed.clone();
        changed.sort_by_key(|r| r.file);
        assert_eq!(changed, expected_changed, "{trace:?}: changed rows");
        let unchanged = match outcome.status {
            ReviewStatus::Refused { .. } => true,
            ReviewStatus::NeedsReview { .. } => *oracle == oracle_prior,
            _ => false,
        };
        if unchanged {
            assert_eq!(*service, prior, "{trace:?}: a refusal changed state");
        }
        assert_eq!(
            view(service),
            oracle.view(group),
            "{trace:?}: published view"
        );
        oracle.check_invariants(group, trace);
    }

    /// Every connected labelled graph on `n` vertices, as edge lists over
    /// file IDs `1..=n`.
    fn connected_graphs(n: u64) -> Vec<Vec<(u64, u64)>> {
        let pairs: Vec<(u64, u64)> = (1..=n)
            .flat_map(|a| ((a + 1)..=n).map(move |b| (a, b)))
            .collect();
        let mut graphs = Vec::new();
        for mask in 0u32..(1 << pairs.len()) {
            let edges: Vec<(u64, u64)> = pairs
                .iter()
                .enumerate()
                .filter(|(i, _)| mask & (1 << i) != 0)
                .map(|(_, p)| *p)
                .collect();
            let mut reached = BTreeSet::from([1u64]);
            let mut frontier = vec![1u64];
            while let Some(v) = frontier.pop() {
                for &(a, b) in &edges {
                    for (x, y) in [(a, b), (b, a)] {
                        if x == v && reached.insert(y) {
                            frontier.push(y);
                        }
                    }
                }
            }
            if reached.len() as u64 == n {
                graphs.push(edges);
            }
        }
        graphs
    }

    /// One explored state, shared by the memo and the BFS frontier.
    type Node = Rc<(ReviewService, Oracle)>;

    /// Breadth-first over every operation sequence up to `depth`, from one
    /// loaded shape. Returns the number of distinct states visited and of
    /// transitions checked.
    ///
    /// Memoisation is the only pruning, and it is sound: `apply` is a
    /// deterministic function of the service state and the command, every
    /// check in [`step`] is a function of the (state, oracle, operation)
    /// triple, and the search is level by level, so a state is first
    /// expanded at the smallest depth it occurs at. Any sequence reaching it
    /// later has no more remaining depth than that first expansion did, so
    /// every transition such a sequence could take was already checked. The
    /// key includes the oracle, and service equality is exact over its
    /// decision state, so two states are merged only when every future
    /// answer is identical.
    fn explore(shape: &Shape, depth: usize) -> (usize, usize) {
        let results = &shape.results;
        let at = format!("{}, tier depth {depth}", shape.name);
        let group = results.groups[0].clone();
        let files: Vec<FileId> = group.members.iter().map(|m| m.file).collect();
        let ops = alphabet(&files);
        let start = Rc::new((loaded(results), Oracle::new(&group)));
        assert_eq!(view(&start.0), start.1.view(&group), "{at}: initial view");
        // Memory, not time, is what limits this search when every core
        // runs a shape at once: the memo holds every distinct state. Each
        // state is allocated once and shared by the memo and the frontier;
        // a state at the last level is never expanded, so it enters only
        // the memo, which still dedupes and counts it.
        let mut seen: HashSet<Node> = HashSet::from([Rc::clone(&start)]);
        let mut frontier: VecDeque<(Node, Vec<Op>)> = VecDeque::new();
        if depth > 0 {
            frontier.push_back((start, Vec::new()));
        }
        let mut transitions = 0;
        while let Some((node, trace)) = frontier.pop_front() {
            let (service, oracle) = &*node;
            for &op in &ops {
                transitions += 1;
                let mut longer = trace.clone();
                longer.push(op);
                let (mut s, mut o) = (service.clone(), oracle.clone());
                let trace = Trace {
                    at: &at,
                    ops: &longer,
                };
                step(&mut s, &mut o, &group, op, trace);
                let successor = Rc::new((s, o));
                if seen.insert(Rc::clone(&successor)) && longer.len() < depth {
                    frontier.push_back((successor, longer));
                }
            }
        }
        (seen.len(), transitions)
    }

    /// One enumerated group shape. `name` identifies it in failure
    /// messages; `ordinal` is its position among the shapes of its size,
    /// which the seeded walks also mix into their seeds.
    struct Shape {
        results: Arc<ScanResults>,
        members: usize,
        ordinal: usize,
        name: String,
    }

    /// The group shapes of PV-T31-001: every connected labelled graph on 1
    /// to 5 files, file 1 the initial reference, in four edge-kind families:
    ///
    /// - all Same bytes, so every counterpart choice off the reference is a
    ///   file-ID tie and Queue identical copies has work;
    /// - all Similar with a distinct per-mille per edge, so best-match
    ///   choice is by score; on every even-numbered edge the higher file ID
    ///   has 0 unique lines, so rows against the reference include both
    ///   review-first and review-free Similar rows;
    /// - alternating Same bytes and Similar by edge order, so groups mix
    ///   review-free and review-first rows; on Similar edges numbered 1
    ///   modulo 4 the lower file ID has 0 unique lines, so a row measured
    ///   from that end skips review;
    /// - all Same bytes with the highest-numbered file folder-protected.
    ///
    /// Labelled rather than up-to-isomorphism graphs, because the file-ID
    /// tie break makes relabelling a non-symmetry; no symmetry pruning is
    /// applied.
    fn shapes(max_members: u64) -> Vec<Shape> {
        let mut out = Vec::new();
        for n in 1..=max_members {
            let mut ordinal = 0;
            for graph in connected_graphs(n) {
                for family in 0..4 {
                    let edges = graph
                        .iter()
                        .enumerate()
                        .map(|(i, &(a, b))| {
                            let fuzzy = match family {
                                1 => true,
                                2 => i % 2 == 1,
                                _ => false,
                            };
                            let index = u16::try_from(i).expect("at most 10 edges");
                            let m = if fuzzy {
                                similar(990 - 10 * index)
                            } else {
                                BYTES
                            };
                            (a, b, m)
                        })
                        .collect();
                    let results = results_of(&[GroupSpec {
                        group: 1,
                        files: (1..=n).collect(),
                        edges,
                        folder_protected: if family == 3 && n > 1 {
                            vec![n]
                        } else {
                            vec![]
                        },
                        no_unique: graph
                            .iter()
                            .enumerate()
                            .filter_map(|(i, &(a, b))| match (family, i % 4) {
                                (1, 0 | 2) => Some((b, a)),
                                (2, 1) => Some((a, b)),
                                _ => None,
                            })
                            .collect(),
                    }]);
                    out.push(Shape {
                        results,
                        members: usize::try_from(n).expect("at most 5 members"),
                        ordinal,
                        name: format!(
                            "shape {n}-member #{ordinal} (family {family}, edges {graph:?})"
                        ),
                    });
                    ordinal += 1;
                }
            }
        }
        out
    }

    // ── Parallel enumeration ─────────────────────────────────────────────

    /// The enumeration's unit of work, as (index into the bounds, index
    /// into the shapes): every shape with a bound's member count, once per
    /// bound that names that count. The shapes are independent (each
    /// `explore` builds its own service, oracle and memo), so jobs share
    /// nothing and their order cannot change any count.
    fn tier_jobs(bounds: &[(usize, usize)], all: &[Shape]) -> Vec<(usize, usize)> {
        bounds
            .iter()
            .enumerate()
            .flat_map(|(tier, &(members, _))| {
                all.iter()
                    .enumerate()
                    .filter(move |(_, s)| s.members == members)
                    .map(move |(shape, _)| (tier, shape))
            })
            .collect()
    }

    /// Threads for the enumeration and the walks: every core, at most
    /// [`MAX_WORKERS`].
    fn workers() -> usize {
        thread::available_parallelism()
            .map_or(1, NonZeroUsize::get)
            .min(MAX_WORKERS)
    }

    /// The thread cap is a memory bound, not a CPU one. Each thread holds
    /// one shape's whole memo; at the release bound that peaked at 19.3 GB
    /// resident over 16 threads on the rexec worker (largest shape 302,336
    /// states), and 40 uncapped threads exhausted its 64 GB and took the
    /// worker down. A machine with less than about 1.5 GB per core up to
    /// this cap can still run out; the counts never depend on the cap.
    const MAX_WORKERS: usize = 16;

    /// Runs `job` on every index in `0..count` across `workers` scoped
    /// threads and returns each index with its result, in index order.
    ///
    /// Threads claim the next index from one shared counter rather than a
    /// fixed slice each: shape costs vary by orders of magnitude within a
    /// tier, and static slices leave cores idle behind the slowest one.
    /// Each index is claimed by exactly one `fetch_add`, so it runs exactly
    /// once; `partition_covers_every_shape_exactly_once` holds that.
    ///
    /// A panicking job stops every thread from claiming more work, then
    /// its panic is resumed on the caller, so the test fails with the
    /// job's own message (the default hook has already printed it with the
    /// shape and sequence) instead of grinding on through the remaining
    /// shapes for minutes.
    fn parallel_map<R: Send>(
        workers: usize,
        count: usize,
        job: impl Fn(usize) -> R + Sync,
    ) -> Vec<(usize, R)> {
        let next = AtomicUsize::new(0);
        let failed = AtomicBool::new(false);
        let mut done: Vec<(usize, R)> = thread::scope(|scope| {
            let handles: Vec<_> = (0..workers.max(1))
                .map(|_| {
                    scope.spawn(|| {
                        let mut mine = Vec::new();
                        while !failed.load(Ordering::Relaxed) {
                            let index = next.fetch_add(1, Ordering::Relaxed);
                            if index >= count {
                                break;
                            }
                            match panic::catch_unwind(AssertUnwindSafe(|| job(index))) {
                                Ok(result) => mine.push((index, result)),
                                Err(payload) => {
                                    failed.store(true, Ordering::Relaxed);
                                    return Err(payload);
                                }
                            }
                        }
                        Ok(mine)
                    })
                })
                .collect();
            let mut all = Vec::new();
            let mut first_panic = None;
            for handle in handles {
                match handle
                    .join()
                    .expect("job panics are caught inside the worker")
                {
                    Ok(mine) => all.extend(mine),
                    Err(payload) => {
                        first_panic.get_or_insert(payload);
                    }
                }
            }
            if let Some(payload) = first_panic {
                panic::resume_unwind(payload);
            }
            all
        });
        done.sort_by_key(|&(index, _)| index);
        done
    }

    /// Explores every shape at every bound in parallel and prints, per
    /// bound, the totals, which are sums and so independent of scheduling,
    /// plus the summed per-shape time. Wall time is printed once, for the
    /// whole run.
    fn enumerate(bounds: &[(usize, usize)], label: &str) {
        let started = Instant::now();
        let all = shapes(5);
        let jobs = tier_jobs(bounds, &all);
        let threads = workers();
        let results = parallel_map(threads, jobs.len(), |j| {
            let (tier, shape) = jobs[j];
            let clock = Instant::now();
            let (states, transitions) = explore(&all[shape], bounds[tier].1);
            (states, transitions, clock.elapsed())
        });
        assert_eq!(results.len(), jobs.len(), "{label}: every job reported");
        for (tier, &(members, depth)) in bounds.iter().enumerate() {
            let (mut states, mut transitions, mut count, mut largest) = (0, 0, 0, 0);
            let mut busy = Duration::ZERO;
            for (j, (s, t, d)) in &results {
                if jobs[*j].0 == tier {
                    states += s;
                    transitions += t;
                    busy += *d;
                    count += 1;
                    largest = largest.max(*s);
                }
            }
            // `largest` sizes memory: a thread's peak is one shape's memo.
            eprintln!(
                "{label}: {members} members, depth {depth}: {count} shapes, {states} states \
                 (largest shape {largest}), {transitions} transitions, {busy:?} summed over shapes"
            );
        }
        eprintln!(
            "{label}: total {:?} wall on {threads} threads",
            started.elapsed()
        );
    }

    /// The exhaustive bound of the debug test gate, as (members, depth).
    /// The plan's bound is [`RELEASE_BOUNDS`], which needs a release build;
    /// this smaller one keeps every shape size in the ordinary gate.
    const BOUNDS: [(usize, usize); 5] = [(1, 6), (2, 6), (3, 4), (4, 2), (5, 1)];

    /// PV-T31-001's exhaustive tier at the plan's binding bound: every
    /// shape at these (members, depth) pairs, in the release-mode CI step.
    ///
    /// With the two-variant Compare evidence the 4-member depth-4 tier is
    /// 27.8 million states and 337 million transitions, about 1,280 s of
    /// thread time, so `enumerate` spreads the shapes over up to
    /// [`MAX_WORKERS`] cores: 103 s of test wall time on 16 threads of the
    /// rexec worker. A 4-core runner needs several minutes for the test
    /// alone, beyond the step's 4-minute target. Depth beyond these bounds
    /// comes from the seeded walks: release-build measurements with the
    /// earlier, smaller alphabet already put 5 members at depth 3 near 16
    /// minutes of single-core time and depth 6 near two weeks with tens of
    /// GB per shape.
    const RELEASE_BOUNDS: [(usize, usize); 5] = [(1, 6), (2, 6), (3, 6), (4, 4), (5, 2)];

    /// The debug-gate exhaustive tier (see [`BOUNDS`]).
    #[test]
    fn invariants_hold_for_every_shape_and_sequence() {
        enumerate(&BOUNDS, "debug exhaustive");
    }

    /// The parallel enumeration drops and repeats no shape: the shape set
    /// has the known size, both bound tables give each shape exactly one
    /// job under its own member count, and the scheduler runs each index
    /// exactly once for any worker count, including more workers than jobs
    /// and no jobs at all.
    #[test]
    fn partition_covers_every_shape_exactly_once() {
        let all = shapes(5);
        // Connected labelled graphs on 1 to 5 vertices (OEIS A001187), an
        // independent count against which `connected_graphs` is checked;
        // each graph appears in 4 edge-kind families.
        for (members, graphs) in [(1, 1), (2, 1), (3, 4), (4, 38), (5, 728)] {
            let count = all.iter().filter(|s| s.members == members).count();
            assert_eq!(count, 4 * graphs, "{members}-member shapes");
        }
        for bounds in [&BOUNDS[..], &RELEASE_BOUNDS[..]] {
            let jobs = tier_jobs(bounds, &all);
            let mut per_shape = vec![0; all.len()];
            for &(tier, shape) in &jobs {
                assert_eq!(all[shape].members, bounds[tier].0, "{}", all[shape].name);
                per_shape[shape] += 1;
            }
            for (shape, n) in per_shape.iter().enumerate() {
                assert_eq!(*n, 1, "{bounds:?}: {} has {n} jobs", all[shape].name);
            }
            for threads in [1, 2, 3, 7, 64] {
                for count in [0, 1, 5, jobs.len()] {
                    let hits: Vec<AtomicUsize> = (0..count).map(|_| AtomicUsize::new(0)).collect();
                    let results = parallel_map(threads, count, |i| {
                        hits[i].fetch_add(1, Ordering::Relaxed);
                        i
                    });
                    let indices: Vec<usize> = results
                        .iter()
                        .map(|&(i, r)| {
                            assert_eq!(i, r, "a result is filed under its own index");
                            i
                        })
                        .collect();
                    assert_eq!(
                        indices,
                        (0..count).collect::<Vec<_>>(),
                        "{threads} threads, {count} jobs: results"
                    );
                    for (i, h) in hits.iter().enumerate() {
                        assert_eq!(
                            h.load(Ordering::Relaxed),
                            1,
                            "{threads} threads, {count} jobs: job {i} runs"
                        );
                    }
                }
            }
        }
    }

    /// PV-T31-001's exhaustive tier at the plan's bound. Every test here is
    /// ignored in the debug gate and run by exactly one release-mode step
    /// in `.github/workflows/rust.yml` (`test` job, "Run the review-policy
    /// enumeration (release)"), whose filter is
    /// `test(review_policy::enumeration)`; renaming this module breaks that
    /// step's filter, which then fails on finding no test.
    mod enumeration {
        use super::{RELEASE_BOUNDS, enumerate};

        /// I-1 to I-5 and I-7 plus the oracle's exact per-step agreement,
        /// over every shape and sequence within [`RELEASE_BOUNDS`].
        #[test]
        #[ignore = "release-only: run by the rust.yml test-job step \"Run the review-policy \
                    enumeration (release)\""]
        fn every_shape_and_sequence_within_the_release_bound() {
            enumerate(&RELEASE_BOUNDS, "release exhaustive");
        }
    }

    /// `SplitMix64`, hand-rolled because no RNG crate is approved (D-011).
    struct SplitMix64(u64);

    impl SplitMix64 {
        const fn next(&mut self) -> u64 {
            self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^ (z >> 31)
        }
    }

    const WALK_SEED: u64 = 0x00F0_22D0_7310_0001;
    const WALKS_PER_SHAPE: u64 = 50;
    const WALK_DEPTH: usize = 24;

    /// PV-T31-001 deep tier: on every 5-member shape, 50 seeded walks of 24
    /// uniformly chosen operations, each step checked against the oracle.
    /// Fixed seeds make every failure reproducible from its trace. Shapes
    /// run in parallel, as in `enumerate`; a walk's seed depends only on
    /// its shape's ordinal and the walk number, never on scheduling.
    #[test]
    fn seeded_walks_over_every_five_member_shape() {
        let started = Instant::now();
        let all = shapes(5);
        let five_member: Vec<&Shape> = all.iter().filter(|s| s.members == 5).collect();
        let threads = workers();
        let per_shape = parallel_map(threads, five_member.len(), |index| {
            let shape = five_member[index];
            let results = &shape.results;
            let group = &results.groups[0];
            let files: Vec<FileId> = group.members.iter().map(|m| m.file).collect();
            let ops = alphabet(&files);
            let len = u64::try_from(ops.len()).expect("small alphabet");
            let ordinal = u64::try_from(shape.ordinal).expect("shape ordinal");
            let mut steps = 0usize;
            for walk in 0..WALKS_PER_SHAPE {
                let at = format!("{}, walk {walk} of depth {WALK_DEPTH}", shape.name);
                let mut rng = SplitMix64(WALK_SEED ^ (ordinal << 16) ^ walk);
                let mut service = loaded(results);
                let mut oracle = Oracle::new(group);
                let mut trace = Vec::with_capacity(WALK_DEPTH);
                for _ in 0..WALK_DEPTH {
                    let pick = usize::try_from(rng.next() % len).expect("index");
                    let op = ops[pick];
                    trace.push(op);
                    let t = Trace {
                        at: &at,
                        ops: &trace,
                    };
                    step(&mut service, &mut oracle, group, op, t);
                    steps += 1;
                }
            }
            steps
        });
        assert_eq!(per_shape.len(), five_member.len(), "every shape walked");
        let steps: usize = per_shape.iter().map(|(_, s)| s).sum();
        eprintln!(
            "seeded walks: {} shapes, {steps} steps in {:?} wall on {threads} threads",
            five_member.len(),
            started.elapsed()
        );
    }

    /// FR-025 in both queue orders on a chain R - A - B, where B is measured
    /// against A: whichever of A and B is queued first, the other is
    /// refused with its own label and the queue is unchanged.
    #[test]
    fn fr025_refuses_both_queue_orders_with_their_labels() {
        let results = single(&[1, 2, 3], &[(1, 2, BYTES), (2, 3, BYTES)]);

        // Counterpart first: B's counterpart A is queued.
        let mut service = loaded(&results);
        assert_eq!(
            run(&mut service, queue(2, 1)).status,
            ReviewStatus::Queued { files: 1 }
        );
        let before = service.clone();
        refused(
            run(&mut service, queue(3, 2)).status,
            3,
            Refusal::CounterpartQueued,
        );
        assert_eq!(service, before, "refusal changed nothing");
        assert_eq!(queued(&service), BTreeSet::from([id(2)]));

        // Dependent first: A is what queued B was measured against.
        let mut service = loaded(&results);
        assert_eq!(
            run(&mut service, queue(3, 2)).status,
            ReviewStatus::Queued { files: 1 }
        );
        let before = service.clone();
        refused(
            run(&mut service, queue(2, 1)).status,
            2,
            Refusal::CounterpartOfQueuedFile,
        );
        assert_eq!(service, before);
        assert_eq!(queued(&service), BTreeSet::from([id(3)]));

        // Reviewing B in Compare against the reference, which stays, lifts
        // the block (spec §10.1). There is no B-R edge, so it needs review.
        let results = single(&[1, 2, 3], &[(1, 2, BYTES), (2, 3, BYTES)]);
        let mut service = loaded(&results);
        run(&mut service, queue(2, 1));
        assert_eq!(
            run_reviewed(&mut service, compare_queue(3, 1)).status,
            ReviewStatus::NeedsReview {
                file: id(3),
                counterpart: id(1)
            }
        );
        assert_eq!(
            run_reviewed(&mut service, compare_ack(3, 1)).status,
            ReviewStatus::Applied
        );
        assert_eq!(
            run(&mut service, queue(3, 1)).status,
            ReviewStatus::Queued { files: 1 }
        );
        assert_eq!(queued(&service), BTreeSet::from([id(2), id(3)]));
    }

    // ── Negative controls (plan §7) ──────────────────────────────────────

    /// Queue attempts on reference, protected, kept, counterpart-queued and
    /// unacknowledged rows, through every entry point.
    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "one control per refused row kind, per entry point, kept in one script so each \
                  control runs against the state the previous one left"
    )]
    fn negative_controls_for_queue_attempts() {
        // R=1; 2 identical to R; 3 similar to R; 4 folder-protected
        // identical; 5 measured against 2 only.
        let results = results_of(&[GroupSpec {
            group: 1,
            files: vec![1, 2, 3, 4, 5],
            edges: vec![
                (1, 2, BYTES),
                (1, 3, similar(950)),
                (1, 4, BYTES),
                (2, 5, BYTES),
            ],
            folder_protected: vec![4],
            no_unique: Vec::new(),
        }]);
        let origins = [
            QueueOrigin::Checkbox,
            QueueOrigin::RowMenu,
            QueueOrigin::Shortcut,
            QueueOrigin::UniqueTextReview,
        ];
        for origin in origins {
            let mut service = loaded(&results);
            let request = |file, counterpart| ReviewCommand::RequestQueue {
                file: id(file),
                counterpart: id(counterpart),
                origin,
                reviewed: None,
            };
            let start = service.clone();
            refused(
                run(&mut service, request(1, 1)).status,
                1,
                Refusal::Reference,
            );
            refused(
                run(&mut service, request(4, 1)).status,
                4,
                Refusal::Protected,
            );
            // Unacknowledged fuzzy row: review first, nothing queued.
            assert_eq!(
                run(&mut service, request(3, 1)).status,
                ReviewStatus::NeedsReview {
                    file: id(3),
                    counterpart: id(1)
                }
            );
            assert_eq!(service, start, "{origin:?}: nothing changed");

            // Per-file Protect blocks every route, including Compare.
            run(
                &mut service,
                ReviewCommand::ProtectFile {
                    file: id(2),
                    protected: true,
                },
            );
            refused(
                run(&mut service, request(2, 1)).status,
                2,
                Refusal::Protected,
            );
            refused(
                run_reviewed(&mut service, compare_queue(2, 1)).status,
                2,
                Refusal::Protected,
            );
            run(
                &mut service,
                ReviewCommand::ProtectFile {
                    file: id(2),
                    protected: false,
                },
            );

            // Counterpart queued.
            assert_eq!(
                run(&mut service, request(2, 1)).status,
                ReviewStatus::Queued { files: 1 }
            );
            refused(
                run(&mut service, request(5, 2)).status,
                5,
                Refusal::CounterpartQueued,
            );

            // A stale counterpart (not the row's current one) without
            // comparison evidence is refused rather than trusted.
            refused(
                run(&mut service, request(5, 1)).status,
                5,
                Refusal::EvidenceMismatch,
            );
        }

        // Kept rows: bulk skips them; an explicit request changes the
        // decision (spec §6: "the user can change the decision").
        let mut service = loaded(&results);
        run(
            &mut service,
            ReviewCommand::MarkKept {
                file: id(2),
                kept: true,
            },
        );
        assert_eq!(
            run(
                &mut service,
                ReviewCommand::QueueIdenticalCopies {
                    scope: QueueScope::AllByteIdenticalGroups
                }
            )
            .status,
            ReviewStatus::Queued { files: 0 },
            "2 kept, 3 similar, 4 protected, 5 not against the reference"
        );
        let outcome = run(&mut service, queue(2, 1));
        assert_eq!(outcome.status, ReviewStatus::Queued { files: 1 });
        assert_eq!(decision_of(&outcome, 2), Some(MemberDecision::Queued));
    }

    /// A Compare-originated command without trusted evidence never reaches
    /// a decision, and evidence for an older observation is refused.
    #[test]
    fn compare_commands_need_current_evidence() {
        let results = single(&[1, 2], &[(1, 2, similar(900))]);
        let mut service = loaded(&results);
        let start = service.clone();
        refused(
            service.apply(compare_queue(2, 1), None).status,
            2,
            Refusal::EvidenceUnavailable,
        );
        let mut stale = pair(&service, id(2), id(1));
        stale.target_snapshot = SnapshotId::new(77);
        refused(
            service.apply(compare_ack(2, 1), Some(stale)).status,
            2,
            Refusal::EvidenceMismatch,
        );
        assert_eq!(service, start);
    }

    // ── PV-T7-002: Kept, Protected, Queue identical copies ───────────────

    /// Kept and Protected are keyed by file ID: re-ordering the results'
    /// groups and members (what sorting and filtering do to rows) does not
    /// move them to another file.
    #[test]
    fn kept_and_protected_follow_file_ids() {
        let specs = vec![
            GroupSpec {
                group: 1,
                files: vec![1, 2, 3],
                edges: vec![(1, 2, BYTES), (1, 3, BYTES)],
                folder_protected: vec![],
                no_unique: Vec::new(),
            },
            GroupSpec {
                group: 2,
                files: vec![4, 5, 6],
                edges: vec![(4, 5, similar(900)), (4, 6, BYTES)],
                folder_protected: vec![],
                no_unique: Vec::new(),
            },
        ];
        let script = |service: &mut ReviewService| {
            let mut states: BTreeMap<FileId, FileReviewState> = BTreeMap::new();
            for command in [
                ReviewCommand::MarkKept {
                    file: id(3),
                    kept: true,
                },
                ReviewCommand::ProtectFile {
                    file: id(5),
                    protected: true,
                },
                queue(2, 1),
                queue(6, 4),
            ] {
                for row in run(service, command).changed {
                    states.insert(row.file, row);
                }
            }
            (states, view(service))
        };
        let mut reordered = specs.clone();
        reordered.reverse();
        for spec in &mut reordered {
            spec.files[1..].reverse();
        }
        let (a_rows, a_view) = script(&mut loaded(&results_of(&specs)));
        let (b_rows, b_view) = script(&mut loaded(&results_of(&reordered)));
        assert_eq!(a_rows, b_rows);
        assert_eq!(a_view.queue, b_view.queue);
        assert_eq!(a_view.protected, b_view.protected);
        assert_eq!(a_rows[&id(3)].decision, MemberDecision::Kept);
        assert_eq!(a_rows[&id(5)].decision, MemberDecision::Protected);
        assert_eq!(a_view.protected, BTreeSet::from([id(5)]));
    }

    /// No operation queues a protected file, and Protect on a queued file
    /// takes it out of the queue.
    #[test]
    fn protect_unqueues_and_blocks_every_route() {
        let results = single(&[1, 2, 3], &[(1, 2, BYTES), (1, 3, BYTES)]);
        let mut service = loaded(&results);
        run(&mut service, queue(2, 1));
        let outcome = run(
            &mut service,
            ReviewCommand::ProtectFile {
                file: id(2),
                protected: true,
            },
        );
        assert_eq!(decision_of(&outcome, 2), Some(MemberDecision::Protected));
        assert!(queued(&service).is_empty());
        assert_eq!(
            run(
                &mut service,
                ReviewCommand::QueueIdenticalCopies {
                    scope: QueueScope::AllByteIdenticalGroups
                }
            )
            .status,
            ReviewStatus::Queued { files: 1 },
            "only 3"
        );
        assert_eq!(queued(&service), BTreeSet::from([id(3)]));
        refused(
            run_reviewed(&mut service, compare_queue(2, 1)).status,
            2,
            Refusal::Protected,
        );
    }

    /// Queue identical copies skips kept, protected, reference and
    /// non-byte-identical members, and respects its scope.
    #[test]
    fn queue_identical_copies_skips_ineligible_members() {
        let hash_candidate = Measurement::Identical(Verification::HashCandidate);
        let results = results_of(&[
            GroupSpec {
                group: 1,
                // 2 kept, 3 protected per file, 4 folder-protected, 5 similar,
                // 6 hash candidate only, 7 eligible, 8 via 7 only.
                files: vec![1, 2, 3, 4, 5, 6, 7, 8],
                edges: vec![
                    (1, 2, BYTES),
                    (1, 3, BYTES),
                    (1, 4, BYTES),
                    (1, 5, similar(900)),
                    (1, 6, hash_candidate),
                    (1, 7, BYTES),
                    (7, 8, BYTES),
                ],
                folder_protected: vec![4],
                no_unique: Vec::new(),
            },
            GroupSpec {
                group: 2,
                files: vec![11, 12],
                edges: vec![(11, 12, BYTES)],
                folder_protected: vec![],
                no_unique: Vec::new(),
            },
        ]);
        let mut service = loaded(&results);
        run(
            &mut service,
            ReviewCommand::MarkKept {
                file: id(2),
                kept: true,
            },
        );
        run(
            &mut service,
            ReviewCommand::ProtectFile {
                file: id(3),
                protected: true,
            },
        );
        let outcome = run(
            &mut service,
            ReviewCommand::QueueIdenticalCopies {
                scope: QueueScope::Groups(vec![GroupId::new(1)]),
            },
        );
        assert_eq!(outcome.status, ReviewStatus::Queued { files: 1 });
        assert_eq!(queued(&service), BTreeSet::from([id(7)]));
        let outcome = run(
            &mut service,
            ReviewCommand::QueueIdenticalCopies {
                scope: QueueScope::AllByteIdenticalGroups,
            },
        );
        assert_eq!(outcome.status, ReviewStatus::Queued { files: 1 });
        assert_eq!(queued(&service), BTreeSet::from([id(7), id(12)]));
        for f in [1, 2, 3, 4, 5, 6, 8] {
            assert!(view(&service).queued(id(f)).is_none(), "{f} skipped");
        }
    }

    // ── PV-T7-003: decision bindings ─────────────────────────────────────

    /// The decision under test, made on file 2 (counterpart 1) of a
    /// three-file group where file 3 is an unrelated member.
    #[derive(Clone, Copy, Debug)]
    enum Kind {
        Kept,
        Protect,
        Acknowledgment,
    }

    fn binding_fixture() -> Arc<ScanResults> {
        single(&[1, 2, 3], &[(1, 2, similar(900)), (1, 3, similar(880))])
    }

    fn make(service: &mut ReviewService, kind: Kind) {
        let command = match kind {
            Kind::Kept => ReviewCommand::MarkKept {
                file: id(2),
                kept: true,
            },
            Kind::Protect => ReviewCommand::ProtectFile {
                file: id(2),
                protected: true,
            },
            Kind::Acknowledgment => ack(2, 1),
        };
        assert_eq!(run(service, command).status, ReviewStatus::Applied);
    }

    const fn domain_kind(kind: Kind) -> DecisionKind {
        match kind {
            Kind::Kept => DecisionKind::Kept,
            Kind::Protect => DecisionKind::Protect,
            Kind::Acknowledgment => DecisionKind::Acknowledgment,
        }
    }

    /// Whether the decision is still in force, observed through behaviour:
    /// Kept and Protect through the row's decision, an acknowledgment
    /// through whether queueing still needs review.
    fn holds(service: &ReviewService, kind: Kind) -> bool {
        let mut probe = service.clone();
        match kind {
            Kind::Kept => {
                // Bulk skips kept files, but file 2 is similar, so probe the
                // decision by unsetting and checking it was set.
                let outcome = run(
                    &mut probe,
                    ReviewCommand::MarkKept {
                        file: id(2),
                        kept: false,
                    },
                );
                decision_of(&outcome, 2).is_some()
            }
            Kind::Protect => view(service).is_protected(id(2)),
            Kind::Acknowledgment => {
                run(&mut probe, queue(2, 1)).status == ReviewStatus::Queued { files: 1 }
            }
        }
    }

    /// For each decision kind and each binding component, a change of that
    /// component alone invalidates exactly that decision, and is reported.
    #[test]
    fn each_component_change_invalidates_each_decision_kind() {
        let results = binding_fixture();
        let components: [(&str, ReviewCommand); 3] = [
            ("target snapshot", touch(2)),
            ("counterpart snapshot", touch(1)),
            ("rules revision", rules(2)),
        ];
        for kind in [Kind::Kept, Kind::Protect, Kind::Acknowledgment] {
            for (name, change) in &components {
                let mut service = loaded(&results);
                make(&mut service, kind);
                assert!(holds(&service, kind), "{kind:?} made");
                let outcome = run(&mut service, change.clone());
                assert_eq!(outcome.status, ReviewStatus::Applied);
                assert_eq!(
                    outcome.invalidated,
                    vec![inv(2, domain_kind(kind))],
                    "{kind:?} / {name}"
                );
                assert!(!holds(&service, kind), "{kind:?} / {name}: still holds");
            }
        }
    }

    /// Unchanged-binding controls: repeating the current values, or
    /// changing only another member, leaves every decision intact.
    #[test]
    fn unchanged_bindings_keep_every_decision() {
        let results = binding_fixture();
        for kind in [Kind::Kept, Kind::Protect, Kind::Acknowledgment] {
            let mut service = loaded(&results);
            make(&mut service, kind);
            for change in [
                ReviewCommand::UpdateEvidence(EvidenceUpdate::FileSnapshot {
                    file: id(2),
                    snapshot: SnapshotId::new(2),
                }),
                ReviewCommand::UpdateEvidence(EvidenceUpdate::FileSnapshot {
                    file: id(1),
                    snapshot: SnapshotId::new(1),
                }),
                rules(1),
                touch(3),
            ] {
                let outcome = run(&mut service, change.clone());
                assert!(outcome.invalidated.is_empty(), "{kind:?} {change:?}");
                assert!(holds(&service, kind), "{kind:?} {change:?}");
            }
        }
    }

    /// A counterpart change through Make this the reference or a Compare
    /// review invalidates the decisions bound to the old counterpart.
    #[test]
    fn counterpart_changes_invalidate_decisions_bound_to_the_old_one() {
        // 2 measured against 1; 3 also adjacent to 2. Make 3 the reference:
        // 2's counterpart becomes 3. A Compare review of 2 against 3 does
        // the same without a reference change.
        let results = single(
            &[1, 2, 3],
            &[
                (1, 2, similar(900)),
                (1, 3, similar(880)),
                (2, 3, similar(870)),
            ],
        );
        for kind in [Kind::Kept, Kind::Protect, Kind::Acknowledgment] {
            let mut service = loaded(&results);
            make(&mut service, kind);
            let outcome = run(&mut service, ReviewCommand::MakeReference { file: id(3) });
            assert!(
                outcome.invalidated.contains(&inv(2, domain_kind(kind))),
                "{kind:?}: Make this the reference"
            );

            let mut service = loaded(&results);
            make(&mut service, kind);
            let outcome = run_reviewed(&mut service, compare_ack(2, 3));
            assert!(
                outcome.invalidated.contains(&inv(2, domain_kind(kind))),
                "{kind:?}: Compare review"
            );
            assert!(
                !outcome.changed.iter().any(|r| r.file != id(2)),
                "a Compare review changes only its own row"
            );
        }
    }

    /// A queued target whose acknowledgment or approval is invalidated
    /// leaves the queue, and both are reported.
    #[test]
    fn invalidated_approvals_leave_the_queue() {
        let results = binding_fixture();
        for change in [touch(2), touch(1), rules(2)] {
            let mut service = loaded(&results);
            run(&mut service, ack(2, 1));
            assert_eq!(
                run(&mut service, queue(2, 1)).status,
                ReviewStatus::Queued { files: 1 }
            );
            let outcome = run(&mut service, change.clone());
            assert_eq!(
                outcome.invalidated,
                vec![
                    inv(2, DecisionKind::Acknowledgment),
                    inv(2, DecisionKind::QueueApproval)
                ],
                "{change:?}"
            );
            assert!(queued(&service).is_empty());
            assert_eq!(decision_of(&outcome, 2), Some(MemberDecision::ReviewFirst));
        }
        // An identical row holds an approval without an acknowledgment.
        let results = single(&[1, 2], &[(1, 2, BYTES)]);
        let mut service = loaded(&results);
        run(&mut service, queue(2, 1));
        let outcome = run(&mut service, touch(1));
        assert_eq!(
            outcome.invalidated,
            vec![inv(2, DecisionKind::QueueApproval)]
        );
        assert!(queued(&service).is_empty());
    }

    /// `ClearForNewScan` empties every decision of every kind; folder
    /// protection survives it and every binding change.
    #[test]
    fn clear_for_new_scan_empties_decisions_and_keeps_folder_protection() {
        let results = results_of(&[GroupSpec {
            group: 1,
            files: vec![1, 2, 3, 4, 5],
            edges: vec![
                (1, 2, similar(900)),
                (1, 3, BYTES),
                (1, 4, BYTES),
                (1, 5, BYTES),
            ],
            folder_protected: vec![5],
            no_unique: Vec::new(),
        }]);
        let mut service = loaded(&results);
        run(&mut service, ack(2, 1));
        run(&mut service, queue(2, 1));
        run(
            &mut service,
            ReviewCommand::MarkKept {
                file: id(3),
                kept: true,
            },
        );
        run(
            &mut service,
            ReviewCommand::ProtectFile {
                file: id(4),
                protected: true,
            },
        );
        for change in [touch(5), touch(1), rules(2)] {
            run(&mut service, change);
            assert!(view(&service).is_protected(id(5)), "folder protection");
        }
        // Rebuild the decisions after the rules change, then clear.
        run(&mut service, ack(2, 1));
        run(&mut service, queue(2, 1));
        run(
            &mut service,
            ReviewCommand::MarkKept {
                file: id(3),
                kept: true,
            },
        );
        run(
            &mut service,
            ReviewCommand::ProtectFile {
                file: id(4),
                protected: true,
            },
        );
        let outcome = run(&mut service, ReviewCommand::ClearForNewScan);
        assert_eq!(outcome.status, ReviewStatus::Applied);
        assert_eq!(
            outcome.invalidated,
            vec![
                inv(2, DecisionKind::Acknowledgment),
                inv(2, DecisionKind::QueueApproval),
                inv(3, DecisionKind::Kept),
                inv(4, DecisionKind::Protect),
            ]
        );
        let v = view(&service);
        assert!(v.queue.is_empty());
        assert_eq!(v.protected, BTreeSet::from([id(5)]));
        assert_eq!(
            run(&mut service, queue(2, 1)).status,
            ReviewStatus::NeedsReview {
                file: id(2),
                counterpart: id(1)
            },
            "the acknowledgment is gone"
        );
        refused(run(&mut service, queue(5, 1)).status, 5, Refusal::Protected);
    }

    /// The `stale` fixture's seeds replayed through commands, then its
    /// scripted evidence changes, each reported as it invalidates.
    #[test]
    fn stale_fixture_changes_invalidate_its_seeds() {
        let scenario =
            fixtures::build(ScenarioKind::Stale, SCAN, DEFAULT_SEED).expect("stale is generated");
        let mut service = loaded(&scenario.results);
        for seed in &scenario.seeded_decisions {
            // An exhaustive match with no wildcard: a seed kind added later
            // fails to compile here until this replay handles it.
            let outcome = match *seed {
                SeededDecision::Kept(file) => {
                    run(&mut service, ReviewCommand::MarkKept { file, kept: true })
                }
                SeededDecision::Protect(file) => run(
                    &mut service,
                    ReviewCommand::ProtectFile {
                        file,
                        protected: true,
                    },
                ),
                SeededDecision::Queue { file, counterpart } => {
                    run(
                        &mut service,
                        ReviewCommand::AcknowledgeUniqueText {
                            file,
                            counterpart,
                            reviewed: None,
                        },
                    );
                    run(
                        &mut service,
                        ReviewCommand::RequestQueue {
                            file,
                            counterpart,
                            origin: QueueOrigin::Checkbox,
                            reviewed: None,
                        },
                    )
                }
                SeededDecision::MakeReference(file) => {
                    run(&mut service, ReviewCommand::MakeReference { file })
                }
            };
            assert!(
                !matches!(outcome.status, ReviewStatus::Refused { .. }),
                "{seed:?}: {outcome:?}"
            );
        }
        assert_eq!(view(&service).queue.len(), 2);
        let mut reported = 0;
        for change in &scenario.evidence_changes {
            let before = view(&service);
            let outcome = run(&mut service, ReviewCommand::UpdateEvidence(*change));
            let after = view(&service);
            for q in &before.queue {
                if after.queued(q.target).is_none() {
                    assert!(outcome.invalidated.contains(&InvalidatedDecision {
                        file: q.target,
                        kind: DecisionKind::QueueApproval
                    }));
                }
            }
            for q in &after.queue {
                assert!(after.binding_is_current(&q.binding));
            }
            reported += outcome.invalidated.len();
        }
        assert!(reported > 0);
        assert!(
            view(&service).queue.is_empty(),
            "the rules change ends every approval"
        );
    }

    // ── Review findings (T31) ────────────────────────────────────────────

    /// Codex finding 1: Keep both records Kept for both displayed files and
    /// clears pending removal intent for the pair (spec §11.5), here with
    /// the counterpart queued beforehand.
    #[test]
    fn keep_both_keeps_both_displayed_files() {
        // 4 and 5 are unrelated members: 4 queued, 5 Kept. Keep both on
        // (2, 3) must leave both exactly as they were (spec §11.5: "must
        // not silently change unrelated members").
        let results = single(
            &[1, 2, 3, 4, 5],
            &[
                (1, 2, BYTES),
                (1, 3, BYTES),
                (2, 3, BYTES),
                (1, 4, BYTES),
                (1, 5, BYTES),
            ],
        );
        let mut service = loaded(&results);
        assert_eq!(
            run(&mut service, queue(3, 1)).status,
            ReviewStatus::Queued { files: 1 }
        );
        run(&mut service, queue(4, 1));
        run(
            &mut service,
            ReviewCommand::MarkKept {
                file: id(5),
                kept: true,
            },
        );
        let four = view(&service).queued(id(4)).copied();
        let outcome = run_reviewed(&mut service, keep_both(2, 3));
        assert_eq!(outcome.status, ReviewStatus::Applied);
        assert_eq!(
            queued(&service),
            BTreeSet::from([id(4)]),
            "the pair's removal intent is cleared"
        );
        assert_eq!(
            view(&service).queued(id(4)).copied(),
            four,
            "4's approval untouched"
        );
        assert!(
            outcome
                .changed
                .iter()
                .all(|r| r.file == id(2) || r.file == id(3)),
            "only the displayed pair's rows change"
        );
        let mut probe = service.clone();
        let unkept = run(
            &mut probe,
            ReviewCommand::MarkKept {
                file: id(5),
                kept: false,
            },
        );
        assert_eq!(
            decision_of(&unkept, 5),
            Some(MemberDecision::Unreviewed),
            "5 still Kept"
        );
        assert_eq!(decision_of(&outcome, 2), Some(MemberDecision::Kept));
        assert_eq!(decision_of(&outcome, 3), Some(MemberDecision::Kept));
        assert!(
            outcome.invalidated.is_empty(),
            "the user removed 3, nothing went stale"
        );
        assert_eq!(
            run(
                &mut service,
                ReviewCommand::QueueIdenticalCopies {
                    scope: QueueScope::AllByteIdenticalGroups
                }
            )
            .status,
            ReviewStatus::Queued { files: 0 },
            "bulk skips both kept files"
        );
    }

    /// Keep both on a pair that contains the reference, in either order: the
    /// member gets Kept and bulk skips it; the reference's own Kept binds to
    /// itself.
    #[test]
    fn keep_both_accepts_pairs_with_the_reference() {
        let results = single(&[1, 2, 3], &[(1, 2, BYTES), (1, 3, BYTES)]);
        let mut service = loaded(&results);
        let outcome = run_reviewed(&mut service, keep_both(2, 1));
        assert_eq!(outcome.status, ReviewStatus::Applied);
        assert_eq!(decision_of(&outcome, 2), Some(MemberDecision::Kept));
        let outcome = run_reviewed(&mut service, keep_both(1, 3));
        assert_eq!(outcome.status, ReviewStatus::Applied);
        assert_eq!(decision_of(&outcome, 3), Some(MemberDecision::Kept));
        assert_eq!(
            run(
                &mut service,
                ReviewCommand::QueueIdenticalCopies {
                    scope: QueueScope::AllByteIdenticalGroups
                }
            )
            .status,
            ReviewStatus::Queued { files: 0 }
        );
        // The reference's self-bound Kept goes stale when it stops being the
        // reference.
        let outcome = run(&mut service, ReviewCommand::MakeReference { file: id(2) });
        assert!(outcome.invalidated.contains(&inv(1, DecisionKind::Kept)));
    }

    /// Codex finding 2: a Compare request on an already-queued file that
    /// names a different, unqueued counterpart replaces the approval, and
    /// the old approval is reported.
    #[test]
    fn compare_reaims_an_already_queued_target() {
        let results = single(&[1, 2, 3], &[(1, 2, BYTES), (1, 3, BYTES), (2, 3, BYTES)]);
        let mut service = loaded(&results);
        run(&mut service, queue(2, 1));
        let outcome = run_reviewed(&mut service, compare_queue(2, 3));
        assert_eq!(outcome.status, ReviewStatus::Queued { files: 1 });
        assert_eq!(
            outcome.invalidated,
            vec![inv(2, DecisionKind::QueueApproval)]
        );
        let v = view(&service);
        assert_eq!(v.queued(id(2)).map(|q| q.counterpart), Some(id(3)));
        refused(
            run(&mut service, queue(3, 1)).status,
            3,
            Refusal::CounterpartOfQueuedFile,
        );
        // Repeating the same reviewed counterpart is the unchanged no-op.
        let before = service.clone();
        assert_eq!(
            run_reviewed(&mut service, compare_queue(2, 3)).status,
            ReviewStatus::Applied
        );
        assert_eq!(service, before);
    }

    /// Finding 2, queued counterpart: the re-aim is refused with "Counterpart
    /// queued" and the original approval stays.
    #[test]
    fn compare_reaim_onto_a_queued_counterpart_is_refused() {
        let results = single(
            &[1, 2, 3, 4],
            &[(1, 2, BYTES), (1, 3, BYTES), (2, 3, BYTES), (1, 4, BYTES)],
        );
        let mut service = loaded(&results);
        run(&mut service, queue(3, 1));
        run(&mut service, queue(2, 1));
        let before = service.clone();
        let outcome = run_reviewed(&mut service, compare_queue(2, 3));
        refused(outcome.status, 2, Refusal::CounterpartQueued);
        assert_eq!(service, before);
        assert_eq!(
            view(&service).queued(id(2)).map(|q| q.counterpart),
            Some(id(1))
        );
    }

    fn row_of(outcome: &ReviewOutcome, file: u64) -> Option<FileReviewState> {
        outcome.changed.iter().find(|r| r.file == id(file)).copied()
    }

    fn link_of(outcome: &ReviewOutcome, file: u64) -> CounterpartLink {
        match row_of(outcome, file).map(|r| r.role) {
            Some(MemberRole::Member(link)) => link,
            other => panic!("file {file} has no changed member row: {other:?}"),
        }
    }

    /// R with Similar edges to B and C, and no B - C edge, so a Compare
    /// choice of C for B takes its Unique count from the comparison.
    fn edge_less_choice() -> Arc<ScanResults> {
        single(&[1, 2, 3], &[(1, 2, similar(900)), (1, 3, similar(880))])
    }

    /// Review item 1 (PV-T31-003): a Compare re-aim that needs review still
    /// commits the new counterpart, and the approval and acknowledgment
    /// bound to the old one are invalidated and listed.
    #[test]
    fn compare_reaim_needing_review_commits_the_new_counterpart() {
        let mut service = loaded(&edge_less_choice());
        run(&mut service, ack(2, 1));
        assert_eq!(
            run(&mut service, queue(2, 1)).status,
            ReviewStatus::Queued { files: 1 }
        );
        let outcome = run_reviewed(&mut service, compare_queue(2, 3));
        assert_eq!(
            outcome.status,
            ReviewStatus::NeedsReview {
                file: id(2),
                counterpart: id(3)
            }
        );
        assert_eq!(
            outcome.invalidated,
            vec![
                inv(2, DecisionKind::Acknowledgment),
                inv(2, DecisionKind::QueueApproval)
            ]
        );
        assert!(queued(&service).is_empty());
        assert_eq!(link_of(&outcome, 2).counterpart, id(3));
        assert_eq!(decision_of(&outcome, 2), Some(MemberDecision::ReviewFirst));
        run_reviewed(&mut service, compare_ack(2, 3));
        assert_eq!(
            run_reviewed(&mut service, compare_queue(2, 3)).status,
            ReviewStatus::Queued { files: 1 }
        );
    }

    /// Review item 2: fresh evidence for an unchanged edge-less Compare
    /// counterpart replaces the Unique count the link shows.
    #[test]
    fn fresh_evidence_refreshes_an_edge_less_unique_count() {
        let mut service = loaded(&edge_less_choice());
        let first = run_measured(&mut service, compare_ack(2, 3), CompareMode::Original, 2);
        assert_eq!(link_of(&first, 2).unique_lines, 2);
        let touched = run(&mut service, touch(2));
        assert_eq!(
            touched.invalidated,
            vec![inv(2, DecisionKind::Acknowledgment)]
        );
        let fresh = run_measured(&mut service, compare_ack(2, 3), CompareMode::Original, 9);
        let link = link_of(&fresh, 2);
        assert_eq!((link.counterpart, link.unique_lines), (id(3), 9));
    }

    /// Review item 2 with the owner's review rule: an edge-less count of 0
    /// waives review only while the comparison it came from is current. A
    /// snapshot change of either file leaves the count unknown, which
    /// needs review again until fresh evidence arrives.
    #[test]
    fn a_stale_compare_count_needs_review_again() {
        let mut service = loaded(&edge_less_choice());
        assert_eq!(
            run_measured(
                &mut service,
                compare_queue(2, 3),
                CompareMode::Normalized,
                0
            )
            .status,
            ReviewStatus::Queued { files: 1 }
        );
        let touched = run(&mut service, touch(3));
        assert_eq!(
            touched.invalidated,
            vec![inv(2, DecisionKind::QueueApproval)]
        );
        assert_eq!(decision_of(&touched, 2), Some(MemberDecision::ReviewFirst));
        assert_eq!(
            run(&mut service, queue(2, 3)).status,
            ReviewStatus::NeedsReview {
                file: id(2),
                counterpart: id(3)
            }
        );
        assert_eq!(
            run_measured(&mut service, compare_queue(2, 3), CompareMode::Original, 0).status,
            ReviewStatus::Queued { files: 1 }
        );
    }

    /// Owner decision of 2026-11-23 (review item 4): a Similar or
    /// Containment row with 0 unique lines against its counterpart shows
    /// Unreviewed and queues without an acknowledgment; the same rows with
    /// unique lines need review.
    #[test]
    fn unique_zero_rows_queue_without_review() {
        let inside = Measurement::Containment {
            coverage: permille(1000),
            direction: fuzzy_dedupe_domain::ContainmentDirection::InsideCounterpart,
        };
        let spec = |no_unique: Vec<(u64, u64)>| {
            results_of(&[GroupSpec {
                group: 1,
                files: vec![1, 2, 3],
                edges: vec![(1, 2, similar(958)), (1, 3, inside)],
                folder_protected: Vec::new(),
                no_unique,
            }])
        };

        let mut service = loaded(&spec(vec![(2, 1), (3, 1)]));
        for file in [2, 3] {
            let outcome = run(&mut service, queue(file, 1));
            assert_eq!(outcome.status, ReviewStatus::Queued { files: 1 }, "{file}");
            assert!(outcome.invalidated.is_empty());
        }
        let unqueued = run(&mut service, ReviewCommand::RemoveFromQueue { file: id(2) });
        assert_eq!(decision_of(&unqueued, 2), Some(MemberDecision::Unreviewed));

        let mut service = loaded(&spec(Vec::new()));
        for file in [2, 3] {
            assert_eq!(
                run(&mut service, queue(file, 1)).status,
                ReviewStatus::NeedsReview {
                    file: id(file),
                    counterpart: id(1)
                },
                "{file}"
            );
        }
        let acked = run(&mut service, ack(2, 1));
        assert_eq!(decision_of(&acked, 2), Some(MemberDecision::Unreviewed));
    }

    /// Owner decision, third control: an approval made without an
    /// acknowledgment, because its Unique count was 0, ends when the count
    /// against the row's counterpart rises above 0. A counterpart change
    /// ends it through the binding; a refreshed count for the same
    /// counterpart leaves the binding equal, so the review check in
    /// reconcile ends it instead. An acknowledgment of the new count keeps
    /// the approval.
    #[test]
    fn a_count_rising_above_zero_ends_an_unacknowledged_approval() {
        let results = results_of(&[GroupSpec {
            group: 1,
            files: vec![1, 2, 3],
            edges: vec![
                (1, 2, similar(900)),
                (1, 3, similar(880)),
                (2, 3, similar(850)),
            ],
            folder_protected: Vec::new(),
            no_unique: vec![(2, 1)],
        }]);
        let mut service = loaded(&results);
        assert_eq!(
            run(&mut service, queue(2, 1)).status,
            ReviewStatus::Queued { files: 1 }
        );
        let reaimed = run_reviewed(&mut service, compare_queue(2, 3));
        assert_eq!(
            reaimed.status,
            ReviewStatus::NeedsReview {
                file: id(2),
                counterpart: id(3)
            }
        );
        assert_eq!(
            reaimed.invalidated,
            vec![inv(2, DecisionKind::QueueApproval)]
        );

        let mut service = loaded(&edge_less_choice());
        run_measured(
            &mut service,
            compare_queue(2, 3),
            CompareMode::Normalized,
            0,
        );
        let binding = view(&service).queued(id(2)).expect("queued").binding;
        let raised = run_measured(&mut service, compare_queue(2, 3), CompareMode::Original, 2);
        assert_eq!(
            raised.status,
            ReviewStatus::NeedsReview {
                file: id(2),
                counterpart: id(3)
            }
        );
        assert_eq!(
            raised.invalidated,
            vec![inv(2, DecisionKind::QueueApproval)]
        );
        assert_eq!(view(&service).queued(id(2)), None);

        let mut service = loaded(&edge_less_choice());
        run_measured(
            &mut service,
            compare_queue(2, 3),
            CompareMode::Normalized,
            0,
        );
        let acked = run_measured(&mut service, compare_ack(2, 3), CompareMode::Original, 2);
        assert!(acked.invalidated.is_empty(), "{acked:?}");
        assert_eq!(
            view(&service).queued(id(2)).map(|q| q.binding),
            Some(binding),
            "the acknowledgment covers the raised count"
        );
    }

    /// Codex finding 3: R, A, B. A is Kept against R, then queued from
    /// Compare against B. The Kept bound to R went stale through the
    /// counterpart change and is listed; with an unchanged counterpart the
    /// Kept is replaced by the user's request and not listed.
    #[test]
    fn compare_counterpart_change_reports_the_stale_kept() {
        let results = single(&[1, 2, 3], &[(1, 2, BYTES), (1, 3, BYTES), (2, 3, BYTES)]);
        let mut service = loaded(&results);
        run(
            &mut service,
            ReviewCommand::MarkKept {
                file: id(2),
                kept: true,
            },
        );
        let outcome = run_reviewed(&mut service, compare_queue(2, 3));
        assert_eq!(outcome.status, ReviewStatus::Queued { files: 1 });
        assert_eq!(outcome.invalidated, vec![inv(2, DecisionKind::Kept)]);

        let mut service = loaded(&results);
        run(
            &mut service,
            ReviewCommand::MarkKept {
                file: id(2),
                kept: true,
            },
        );
        let outcome = run(&mut service, queue(2, 1));
        assert_eq!(outcome.status, ReviewStatus::Queued { files: 1 });
        assert!(outcome.invalidated.is_empty());
    }

    /// The reverse-FR-025 matrix: in both queue orders, through all five
    /// origins, the second request is refused with its label and changes
    /// nothing.
    #[test]
    fn fr025_label_matrix_across_every_origin() {
        let results = single(&[1, 2, 3], &[(1, 2, BYTES), (2, 3, BYTES)]);
        for origin in ALL_ORIGINS {
            let mut service = loaded(&results);
            assert_eq!(
                run_any(&mut service, request_from(origin, 2, 1)).status,
                ReviewStatus::Queued { files: 1 },
                "{origin:?}"
            );
            let before = service.clone();
            let outcome = run_any(&mut service, request_from(origin, 3, 2));
            refused(outcome.status, 3, Refusal::CounterpartQueued);
            assert!(outcome.changed.is_empty() && outcome.invalidated.is_empty());
            assert_eq!(service, before, "{origin:?}");

            let mut service = loaded(&results);
            assert_eq!(
                run_any(&mut service, request_from(origin, 3, 2)).status,
                ReviewStatus::Queued { files: 1 },
                "{origin:?}"
            );
            let before = service.clone();
            let outcome = run_any(&mut service, request_from(origin, 2, 1));
            refused(outcome.status, 2, Refusal::CounterpartOfQueuedFile);
            assert!(outcome.changed.is_empty() && outcome.invalidated.is_empty());
            assert_eq!(service, before, "{origin:?}");
        }
    }

    /// Same text and Containment rows need Review unique text too (spec
    /// §11.4: "Fuzzy or normalized-text removal intent goes through the same
    /// review policy"), through every origin.
    #[test]
    fn same_text_and_containment_rows_need_review_first() {
        let containment = Measurement::Containment {
            coverage: permille(1000),
            direction: fuzzy_dedupe_domain::ContainmentDirection::InsideCounterpart,
        };
        let results = single(
            &[1, 2, 3],
            &[(1, 2, Measurement::SameText), (1, 3, containment)],
        );
        for origin in ALL_ORIGINS {
            let mut service = loaded(&results);
            let start = service.clone();
            for file in [2, 3] {
                assert_eq!(
                    run_any(&mut service, request_from(origin, file, 1)).status,
                    ReviewStatus::NeedsReview {
                        file: id(file),
                        counterpart: id(1)
                    },
                    "{origin:?} {file}"
                );
            }
            assert_eq!(service, start);
            run(&mut service, ack(2, 1));
            assert_eq!(
                run_any(&mut service, request_from(origin, 2, 1)).status,
                ReviewStatus::Queued { files: 1 }
            );
        }
    }

    /// Comparison evidence computed on a stale counterpart snapshot or a
    /// stale rules revision is refused for every reviewed command.
    #[test]
    fn stale_counterpart_snapshot_and_rules_evidence_are_refused() {
        let results = single(&[1, 2], &[(1, 2, similar(900))]);
        let mut service = loaded(&results);
        let start = service.clone();
        let mut stale_counterpart = pair(&service, id(2), id(1));
        stale_counterpart.counterpart_snapshot = SnapshotId::new(77);
        let mut stale_rules = pair(&service, id(2), id(1));
        stale_rules.rules = RulesRevision::new(9);
        for evidence in [stale_counterpart, stale_rules] {
            for command in [compare_queue(2, 1), compare_ack(2, 1), keep_both(2, 1)] {
                refused(
                    service.apply(command.clone(), Some(evidence)).status,
                    2,
                    Refusal::EvidenceMismatch,
                );
            }
        }
        assert_eq!(service, start);
    }

    // ── PV-T7-004: Make this the reference ───────────────────────────────

    /// R=1; 2 and 3 adjacent to R; 4 adjacent to 2 and 5; 5 adjacent to 4
    /// and, more strongly, 3; 6 adjacent only to 3 but reviewed in Compare
    /// against 1. With 4, 5 and 6 queued, Make 4 the reference: 4 leaves the
    /// queue, 5's counterpart moves 3 -> 4 (adjacent to the new reference),
    /// 6's Compare choice is discarded for the rule's 3, and 3 keeps 1 (it
    /// is not adjacent to 4, and Same bytes to 1 ties Same bytes to 6, which
    /// the lower file ID wins).
    #[test]
    fn make_reference_follows_the_section_10_1_order() {
        let results = single(
            &[1, 2, 3, 4, 5, 6],
            &[
                (1, 2, BYTES),
                (1, 3, BYTES),
                (2, 4, BYTES),
                (4, 5, similar(900)),
                (3, 5, similar(950)),
                (3, 6, BYTES),
            ],
        );
        let mut service = loaded(&results);
        // 6 is reviewed against 1 in Compare (no edge, so acknowledged).
        run_reviewed(&mut service, compare_ack(6, 1));
        assert_eq!(
            run(&mut service, queue(6, 1)).status,
            ReviewStatus::Queued { files: 1 }
        );
        assert_eq!(
            run(&mut service, queue(4, 2)).status,
            ReviewStatus::Queued { files: 1 }
        );
        run(&mut service, ack(5, 3));
        assert_eq!(
            run(&mut service, queue(5, 3)).status,
            ReviewStatus::Queued { files: 1 }
        );
        assert_eq!(queued(&service), BTreeSet::from([id(4), id(5), id(6)]));

        let outcome = run(&mut service, ReviewCommand::MakeReference { file: id(4) });
        // The new reference is not counted; 5 and 6 are.
        assert_eq!(
            outcome.status,
            ReviewStatus::ReferenceChanged { unqueued: 2 }
        );
        assert!(queued(&service).is_empty());
        assert!(
            outcome
                .invalidated
                .contains(&inv(5, DecisionKind::Acknowledgment)),
            "acknowledgment cleared"
        );
        for f in [4, 5, 6] {
            assert!(
                outcome
                    .invalidated
                    .contains(&inv(f, DecisionKind::QueueApproval))
            );
        }
        let role = |f: u64| {
            outcome
                .changed
                .iter()
                .find(|r| r.file == id(f))
                .map(|r| r.role)
        };
        assert_eq!(role(4), Some(MemberRole::Reference));
        let counterpart_of = |f: u64| match role(f) {
            Some(MemberRole::Member(link)) => Some((link.counterpart, link.basis)),
            _ => None,
        };
        assert_eq!(counterpart_of(5), Some((id(4), CounterpartBasis::Rule)));
        assert_eq!(counterpart_of(6), Some((id(3), CounterpartBasis::Rule)));
        assert_eq!(counterpart_of(2), Some((id(4), CounterpartBasis::Rule)));
        assert!(role(3).is_none(), "3's counterpart is unchanged");
    }

    /// Members whose counterpart does not change keep their queue state and
    /// acknowledgment; only the changed ones are counted.
    #[test]
    fn make_reference_leaves_unchanged_counterparts_untouched() {
        // R=1; 2 and 3 adjacent to R; 4 adjacent to 3 only; 5 adjacent to 1
        // and 2. After Make 2 the reference, 3 is not adjacent to 2 and its
        // best direct match is still 1, so its counterpart file is unchanged
        // even though 1 stopped being the reference.
        let results = single(
            &[1, 2, 3, 4, 5],
            &[
                (1, 2, BYTES),
                (1, 3, BYTES),
                (3, 4, similar(900)),
                (1, 5, similar(910)),
                (2, 5, similar(920)),
            ],
        );
        let mut service = loaded(&results);
        // 3 queued against 1 (identical); 4 acknowledged against 3 but not
        // queued (3 is queued); 5 queued against 1 after acknowledgment.
        run(&mut service, queue(3, 1));
        run(&mut service, ack(4, 3));
        run(&mut service, ack(5, 1));
        assert_eq!(
            run(&mut service, queue(5, 1)).status,
            ReviewStatus::Queued { files: 1 }
        );
        run(&mut service, queue(2, 1));
        assert_eq!(queued(&service), BTreeSet::from([id(2), id(3), id(5)]));
        let three_before = view(&service).queued(id(3)).copied();

        let outcome = run(&mut service, ReviewCommand::MakeReference { file: id(2) });
        // 2 leaves as the new reference; 5's counterpart becomes 2 (adjacent)
        // so it is unqueued; 3 keeps 1 and stays queued.
        assert_eq!(
            outcome.status,
            ReviewStatus::ReferenceChanged { unqueued: 1 }
        );
        assert_eq!(queued(&service), BTreeSet::from([id(3)]));
        assert_eq!(view(&service).queued(id(3)).copied(), three_before);
        assert!(
            outcome
                .invalidated
                .contains(&inv(5, DecisionKind::Acknowledgment))
        );
        assert!(
            !outcome
                .invalidated
                .iter()
                .any(|d| d.file == id(3) || d.file == id(4))
        );
        assert!(
            !outcome.changed.iter().any(|r| r.file == id(3)),
            "3 is untouched"
        );
        // 4's acknowledgment against 3 survived: once 3 is unqueued, 4 can
        // be queued without reviewing again.
        run(&mut service, ReviewCommand::RemoveFromQueue { file: id(3) });
        assert_eq!(
            run(&mut service, queue(4, 3)).status,
            ReviewStatus::Queued { files: 1 }
        );
    }
}
