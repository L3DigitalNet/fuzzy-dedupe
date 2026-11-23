// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! PV-T9-001 (plan T9, REQ-038): the `design` scenario reproduces the
//! values the review frames show.
//!
//! The expected values below are literal data transcribed from Figma file
//! `d5tA6ap1tH4cTDaJiwve89` (text through the read-only Figma tools) and
//! checked against the PNGs in `docs/design/reference/`; where the two
//! disagreed the PNG won. Each block names its frame and node. The test
//! projects the same values from the scenario: it replays the seeded
//! decisions in order with a projection written here, then renders each cell
//! with this file's own reading of spec §11.4. Nothing here calls the review
//! service (T7), so the comparison holds whatever T7's internals become.
//!
//! The C-01 invariant checks are copied from `fixtures_contract.rs`, which
//! keeps its helpers private and checks only T4's six scenarios. The copy
//! keeps their independence: its counterpart and Unique oracles are
//! recomputed from edges and rendered bytes, not from the domain crate.
//!
//! Every test sits in the `design_scenario` module so the proof filter
//! `-E 'test(design_scenario)'` selects them all.

mod design_scenario {
    use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

    use fuzzy_dedupe_domain::{
        ContainmentDirection, Coverage, FileId, Group, GroupId, MatchEdge, MatchMode, Measurement,
        Member, MemberRole, PlanItemStatus, RelationKind, ScanId, rule_counterpart,
    };
    use fuzzy_dedupe_engine::fixtures::design::{self, DesignSample};
    use fuzzy_dedupe_engine::fixtures::{
        self, DEFAULT_SEED, Scenario, ScenarioKind, SeededDecision,
    };

    const SCAN: ScanId = ScanId::new(7);

    fn build() -> Scenario {
        fixtures::build(ScenarioKind::Design, SCAN, DEFAULT_SEED)
            .unwrap_or_else(|missing| panic!("design scenario: {missing:?}"))
    }

    // ── C-01 invariants (copied from fixtures_contract.rs) ───────────────

    fn strength(measurement: Measurement) -> (u8, u16) {
        match measurement {
            Measurement::Identical(_) => (3, 1000),
            Measurement::SameText => (2, 1000),
            Measurement::Similar(p) => (1, p.get()),
            Measurement::Containment { coverage, .. } => (0, coverage.get()),
        }
    }

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

    fn unique_lines(target: &[u8], counterpart: &[u8]) -> u64 {
        let theirs: HashSet<&[u8]> = lines(counterpart).into_iter().collect();
        lines(target)
            .iter()
            .filter(|l| !theirs.contains(*l))
            .count() as u64
    }

    struct Rendered(HashMap<FileId, Vec<u8>>);

    impl Rendered {
        fn of(scenario: &Scenario) -> Self {
            let files = scenario.results.groups.iter().flat_map(|g| &g.members);
            Self(
                files
                    .map(|m| {
                        let bytes = scenario
                            .contents
                            .render(m.file)
                            .unwrap_or_else(|| panic!("no in-memory buffer for {:?}", m.file));
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

    #[test]
    fn every_c01_invariant_holds() {
        check_invariants(&build());
    }

    fn check_invariants(scenario: &Scenario) {
        let results = &scenario.results;
        let rendered = Rendered::of(scenario);
        assert_eq!(results.scan, SCAN, "results carry the scan ID");
        match results.coverage {
            Coverage::Empty => assert!(results.groups.is_empty(), "Empty has groups"),
            Coverage::Complete | Coverage::Partial => {
                assert!(!results.groups.is_empty(), "use Empty for no groups");
            }
        }

        let mut files = HashSet::new();
        let mut group_ids = HashSet::new();
        for (index, group) in results.groups.iter().enumerate() {
            // Exhaustive destructuring: a group-wide score field fails to
            // compile here (§10.1).
            let Group {
                id,
                label,
                members,
                edges,
            } = group;
            let at = label.clone();
            assert!(group_ids.insert(*id), "{at}: duplicate group ID");
            assert_eq!(*label, format!("G{:03}", index + 1), "{at}: stable label");
            assert!(members.len() >= 2, "{at}: a group has at least two members");
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

    fn check_seeds(scenario: &Scenario) {
        let by_id = members_by_id(scenario);
        let known = |file: &FileId| {
            by_id
                .get(file)
                .unwrap_or_else(|| panic!("seed names unknown {file:?}"))
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
                    let current = rereferenced
                        .get(&group.id)
                        .copied()
                        .unwrap_or(group.members[0].file);
                    assert_ne!(
                        *f, current,
                        "{}: MakeReference of the reference",
                        group.label
                    );
                    rereferenced.insert(group.id, *f);
                }
                SeededDecision::Queue { file, counterpart } => {
                    let (group, m) = known(file);
                    let (counterpart_group, _) = known(counterpart);
                    assert_eq!(group.id, counterpart_group.id, "seed counterpart elsewhere");
                    assert!(!m.folder_protected, "seed queues a protected file");
                    if rereferenced.contains_key(&group.id) {
                        continue;
                    }
                    assert!(!m.is_reference(), "seed queues a reference");
                    assert_eq!(m.counterpart(), Some(*counterpart), "seed counterpart");
                    assert!(!queued.contains(counterpart), "counterpart queued");
                }
            }
        }
        for group in &scenario.results.groups {
            assert!(
                group.members.iter().any(|m| !queued.contains(&m.file)),
                "{}: fully queued",
                group.label
            );
        }
    }

    // ── Review-state projection ──────────────────────────────────────────

    /// The review state the seeds leave, replayed in listed order by this
    /// file's reading of spec §10.1 "Make this the reference" and C-05 I-7:
    /// a new reference leaves the queue, every counterpart is recomputed by
    /// the §10.1 rule, and a queued, Kept or per-file Protect decision whose
    /// counterpart changed is dropped. No Compare choice is seeded, so none
    /// is discarded.
    struct Review<'a> {
        scenario: &'a Scenario,
        reference: HashMap<GroupId, FileId>,
        /// Queued target to the counterpart it was queued against, and the
        /// queue order, which the cleanup plan follows.
        queued: HashMap<FileId, FileId>,
        queue_order: Vec<FileId>,
        kept: HashMap<FileId, FileId>,
        protected: HashMap<FileId, FileId>,
    }

    impl<'a> Review<'a> {
        fn replay(scenario: &'a Scenario) -> Self {
            let mut review = Self {
                scenario,
                reference: scenario
                    .results
                    .groups
                    .iter()
                    .map(|g| (g.id, g.members[0].file))
                    .collect(),
                queued: HashMap::new(),
                queue_order: Vec::new(),
                kept: HashMap::new(),
                protected: HashMap::new(),
            };
            for seed in &scenario.seeded_decisions {
                match *seed {
                    SeededDecision::MakeReference(file) => review.make_reference(file),
                    SeededDecision::Queue { file, counterpart } => {
                        assert_eq!(
                            review.counterpart(file),
                            Some(counterpart),
                            "a Queue seed names its counterpart under the replayed reference"
                        );
                        review.queued.insert(file, counterpart);
                        review.queue_order.push(file);
                    }
                    SeededDecision::Kept(file) => {
                        let counterpart = review.counterpart(file).expect("Kept a reference");
                        review.kept.insert(file, counterpart);
                    }
                    SeededDecision::Protect(file) => {
                        let counterpart = review.counterpart(file).expect("Protect a reference");
                        review.protected.insert(file, counterpart);
                    }
                }
            }
            review
        }

        fn group_of(&self, file: FileId) -> &'a Group {
            self.scenario
                .results
                .group_of(file)
                .unwrap_or_else(|| panic!("{file:?} is in no group"))
        }

        fn member(&self, file: FileId) -> &'a Member {
            self.group_of(file).member(file).expect("member")
        }

        fn reference_of(&self, group: &Group) -> FileId {
            self.reference[&group.id]
        }

        fn counterpart(&self, file: FileId) -> Option<FileId> {
            let group = self.group_of(file);
            let reference = self.reference_of(group);
            if file == reference {
                return None;
            }
            rule_counterpart(&group.edges, reference, file)
        }

        fn edge(&self, file: FileId) -> Option<&'a MatchEdge> {
            let counterpart = self.counterpart(file)?;
            self.group_of(file).edge_between(file, counterpart)
        }

        fn measurement(&self, file: FileId) -> Option<Measurement> {
            self.edge(file).and_then(|e| e.measurement_from(file))
        }

        fn unique(&self, file: FileId) -> u64 {
            self.edge(file)
                .and_then(|e| e.unique_lines_of(file))
                .unwrap_or(0)
        }

        fn make_reference(&mut self, file: FileId) {
            let group = self.group_of(file);
            let before: HashMap<FileId, Option<FileId>> = group
                .members
                .iter()
                .map(|m| (m.file, self.counterpart(m.file)))
                .collect();
            self.reference.insert(group.id, file);
            self.queued.remove(&file);
            for m in &group.members {
                if self.counterpart(m.file) != before[&m.file] {
                    self.queued.remove(&m.file);
                    self.kept.remove(&m.file);
                    self.protected.remove(&m.file);
                }
            }
            self.queue_order.retain(|f| self.queued.contains_key(f));
        }

        fn is_protected(&self, file: FileId) -> bool {
            self.member(file).folder_protected || self.protected.contains_key(&file)
        }

        /// Reference first, then the strongest match, then file ID (§11.4).
        fn rows(&self, group: &'a Group) -> Vec<&'a Member> {
            let reference = self.reference_of(group);
            let mut rows: Vec<&Member> = group.members.iter().collect();
            rows.sort_by(|x, y| {
                (y.file == reference)
                    .cmp(&(x.file == reference))
                    .then_with(
                        || match (self.measurement(x.file), self.measurement(y.file)) {
                            (Some(mx), Some(my)) => my.cmp_strength(&mx),
                            _ => std::cmp::Ordering::Equal,
                        },
                    )
                    .then_with(|| x.file.cmp(&y.file))
            });
            rows
        }

        fn row_kinds(&self, group: &Group) -> BTreeSet<RelationKind> {
            group
                .members
                .iter()
                .filter_map(|m| self.measurement(m.file).map(|x| x.relation()))
                .collect()
        }

        // ── Cell text, by this file's reading of spec §11.4 ──────────────

        /// §11.4 Member-row labels: "reference" when the counterpart is the
        /// current reference, otherwise the counterpart's file name.
        fn match_label(&self, file: FileId) -> String {
            let Some(measurement) = self.measurement(file) else {
                return String::new();
            };
            let counterpart = self.counterpart(file).expect("counterpart");
            let is_reference = counterpart == self.reference_of(self.group_of(file));
            let name = if is_reference {
                "reference".to_owned()
            } else {
                self.member(counterpart).name.clone()
            };
            match measurement {
                Measurement::Identical(_) => format!("Identical to {name}"),
                Measurement::SameText => format!("Same text as {name}"),
                Measurement::Similar(p) => format!("{} similar to {name}", percent(p.get())),
                Measurement::Containment {
                    coverage,
                    direction: ContainmentDirection::InsideCounterpart,
                } => format!("{} inside {name}", percent(coverage.get())),
                Measurement::Containment {
                    direction: ContainmentDirection::ContainsCounterpart,
                    ..
                } => {
                    if is_reference {
                        "Contains the reference".to_owned()
                    } else {
                        format!("Contains {name}")
                    }
                }
            }
        }

        /// §11.4 Unique: "4 lines", or "—" when there are none.
        fn unique_label(&self, file: FileId) -> String {
            match self.unique(file) {
                0 => "—".to_owned(),
                1 => "1 line".to_owned(),
                n => format!("{n} lines"),
            }
        }

        /// Whether queueing the row first opens Review unique text. Spec
        /// §11.4: "Fuzzy or normalized-text removal intent goes through the
        /// same review policy … Rows that need it show “Review first”", and
        /// Space opens "Review unique text first when the row needs review".
        /// The frames show the reading used here: a non-identical row needs
        /// review when it has unique lines (`Requirements_draft.txt`, 4 lines,
        /// Review first), and one without any shows Unreviewed
        /// (`Requirements_final.txt` and `Guide_excerpt.txt`, "—").
        fn needs_review(&self, file: FileId) -> bool {
            self.measurement(file)
                .is_some_and(|m| m.relation() != RelationKind::SameBytes)
                && self.unique(file) > 0
        }

        /// §11.4 Decision: Reference, Unreviewed, Review first, Kept, Queued,
        /// Protected, or Counterpart queued. Seeded and structural states
        /// take precedence over the derived review state.
        fn decision_label(&self, file: FileId) -> &'static str {
            if file == self.reference_of(self.group_of(file)) {
                "Reference"
            } else if self.queued.contains_key(&file) {
                "Queued"
            } else if self.is_protected(file) {
                "Protected"
            } else if self.kept.contains_key(&file) {
                "Kept"
            } else if self
                .counterpart(file)
                .is_some_and(|c| self.queued.contains_key(&c))
            {
                "Counterpart queued"
            } else if self.needs_review(file) {
                "Review first"
            } else {
                "Unreviewed"
            }
        }

        fn header_decision(&self, group: &Group, sample: &DesignSample) -> String {
            let queued = group
                .members
                .iter()
                .filter(|m| self.queued.contains_key(&m.file))
                .count();
            if queued > 0 {
                format!("{queued} queued")
            } else if sample.last_compared.is_some_and(|p| p.group == group.id) {
                "Last compared".to_owned()
            } else {
                String::new()
            }
        }

        fn header_kind(&self, group: &Group) -> &'static str {
            let kinds = self.row_kinds(group);
            if kinds.len() > 1 {
                return "Mixed";
            }
            kinds.first().map_or("", |k| kind_name(*k))
        }
    }

    const fn kind_name(kind: RelationKind) -> &'static str {
        match kind {
            RelationKind::SameBytes => "Same bytes",
            RelationKind::SameText => "Same text",
            RelationKind::SimilarText => "Similar text",
            RelationKind::Containment => "Containment",
        }
    }

    /// One decimal of a percentage from per-mille, "100%" when whole.
    fn percent(permille: u16) -> String {
        if permille.is_multiple_of(10) {
            format!("{}%", permille / 10)
        } else {
            format!("{}.{}%", permille / 10, permille % 10)
        }
    }

    /// §11.4 Size: binary units, one decimal, rounded to nearest.
    fn kib(bytes: u64) -> String {
        let tenths = (bytes * 10 + 512) / 1024;
        format!("{}.{} KiB", tenths / 10, tenths % 10)
    }

    fn thousands(n: u64) -> String {
        let digits = n.to_string();
        let mut out = String::new();
        for (i, c) in digits.chars().enumerate() {
            if i > 0 && (digits.len() - i).is_multiple_of(3) {
                out.push(',');
            }
            out.push(c);
        }
        out
    }

    /// UTC calendar date of a modification time, by Hinnant's
    /// `civil_from_days`.
    fn date(time: std::time::SystemTime) -> String {
        let secs = time
            .duration_since(std::time::UNIX_EPOCH)
            .expect("after 1970")
            .as_secs();
        let z = i64::try_from(secs / 86_400).expect("days") + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = doy - (153 * mp + 2) / 5 + 1;
        let month = if mp < 10 { mp + 3 } else { mp - 9 };
        let year = yoe + era * 400 + i64::from(month <= 2);
        format!("{year:04}-{month:02}-{day:02}")
    }

    /// The Folder column shortens only the visual form (§11.4): a frame
    /// value with "…" matches a stored folder that keeps the text on both
    /// sides of the ellipsis and hides at least one character between them.
    /// The UI's display form ends folders in "/".
    fn folder_matches(frame: &str, folder: &str) -> bool {
        let shown = format!("{folder}/");
        match frame.split_once('…') {
            None => shown == frame,
            Some((head, tail)) => {
                shown.len() > head.len() + tail.len()
                    && shown.starts_with(head)
                    && shown.ends_with(tail)
            }
        }
    }

    fn by_name<'a>(scenario: &'a Scenario, name: &str) -> &'a Member {
        let found: Vec<&Member> = scenario
            .results
            .groups
            .iter()
            .flat_map(|g| &g.members)
            .filter(|m| m.name == name)
            .collect();
        assert_eq!(found.len(), 1, "exactly one file named {name}");
        found[0]
    }

    fn group_by_label<'a>(scenario: &'a Scenario, label: &str) -> &'a Group {
        scenario
            .results
            .groups
            .iter()
            .find(|g| g.label == label)
            .unwrap_or_else(|| panic!("no group {label}"))
    }

    // ── Frame 01 · Findings `13:648` ─────────────────────────────────────

    struct Row {
        matched: &'static str,
        unique: &'static str,
        name: &'static str,
        folder: &'static str,
        modified: &'static str,
        size: &'static str,
        protected: &'static str,
        decision: &'static str,
    }

    struct Header {
        group: &'static str,
        kind: &'static str,
        title: &'static str,
        decision: &'static str,
        /// `None` for a collapsed group.
        rows: Option<&'static [Row]>,
    }

    const fn row(cells: [&'static str; 8]) -> Row {
        let [
            matched,
            unique,
            name,
            folder,
            modified,
            size,
            protected,
            decision,
        ] = cells;
        Row {
            matched,
            unique,
            name,
            folder,
            modified,
            size,
            protected,
            decision,
        }
    }

    /// Frame 01 `13:648`, table body `13:781`, in frame order. The group
    /// rows are instances `24:973` (G001) to `24:1603` (G009's last file).
    /// The frame's own group order is not asserted: it is the UI's sort
    /// (T13), and it cannot be the top of a Match-type sort when 42 groups
    /// are Same bytes (recorded for T24).
    #[rustfmt::skip]
    const FINDINGS: &[Header] = &[
        // `24:973`, `24:990`, `24:1021`, `24:1052`.
        Header {
            group: "G001 (3)",
            kind: "Same bytes",
            title: "Project plan",
            decision: "2 queued",
            rows: Some(&[
                row(["", "—", "Project_Plan.txt", "~/Documents/Projects/", "2026-03-14", "12.8 KiB", "Protected", "Reference"]),
                row(["Identical to reference", "—", "Project_Plan_copy.txt", "~/Documents/Archive/", "2026-03-14", "12.8 KiB", "—", "Queued"]),
                row(["Identical to reference", "—", "Project_Plan (2).txt", "~/Backups/Documents/", "2026-03-14", "12.8 KiB", "—", "Queued"]),
            ]),
        },
        // `24:1397`, `24:1414`, `24:1445`.
        Header {
            group: "G005 (2)",
            kind: "Same bytes",
            title: "Release notes",
            decision: "1 queued",
            rows: Some(&[
                row(["", "—", "Release_Notes.txt", "~/Documents/Release/", "2026-07-11", "22.4 KiB", "—", "Reference"]),
                row(["Identical to reference", "—", "Release_Notes_copy.txt", "~/Documents/Release/Old/", "2026-07-11", "22.4 KiB", "—", "Queued"]),
            ]),
        },
        // `24:1493`, collapsed.
        Header {
            group: "G007 (2)",
            kind: "Same bytes",
            title: "Installation notes",
            decision: "",
            rows: None,
        },
        // `24:1631`, collapsed.
        Header {
            group: "G010 (2)",
            kind: "Same bytes",
            title: "Configuration reference",
            decision: "",
            rows: None,
        },
        // `24:1217`, `24:1234`, `24:1265`.
        Header {
            group: "G003 (2)",
            kind: "Same text",
            title: "Meeting notes",
            decision: "",
            rows: Some(&[
                row(["", "—", "Meeting_Notes.md", "~/Documents/Meetings/", "2026-05-06", "6.2 KiB", "—", "Reference"]),
                row(["Same text as reference", "—", "Meeting_Notes_copy.md", "~/Documents/…/2024-05/", "2026-05-06", "6.3 KiB", "—", "Kept"]),
            ]),
        },
        // `24:1083`, `24:1100`, `24:1131`, `24:1159`, `24:1189`.
        Header {
            group: "G002 (4)",
            kind: "Similar text",
            title: "Requirements",
            decision: "",
            rows: Some(&[
                row(["", "—", "Requirements.txt", "~/Documents/Specs/", "2026-06-02", "18.4 KiB", "—", "Reference"]),
                row(["98.7% similar to reference", "4 lines", "Requirements_draft.txt", "~/Documents/Specs/Old/", "2026-05-20", "18.1 KiB", "—", "Review first"]),
                row(["97.1% similar to reference", "9 lines", "Requirements_v2.txt", "~/Documents/…/Specs/", "2026-05-28", "18.9 KiB", "Protected", "Protected"]),
                row(["95.8% similar to Requirements_v2.txt", "—", "Requirements_final.txt", "~/Documents/…/2024/", "2024-11-08", "17.2 KiB", "—", "Unreviewed"]),
            ]),
        },
        // `24:1293`, `24:1310`, `24:1341`, `24:1369`.
        Header {
            group: "G004 (3)",
            kind: "Similar text",
            title: "Design specification",
            decision: "Last compared",
            rows: Some(&[
                row(["", "—", "Design_Spec.txt", "~/Documents/Design/", "2026-05-02", "12.8 KiB", "—", "Reference"]),
                row(["97.9% similar to reference", "2 lines", "Design_Spec_draft.txt", "~/Documents/…/Drafts/", "2026-04-18", "12.6 KiB", "—", "Review first"]),
                row(["96.4% similar to reference", "5 lines", "Design_Spec_rev.txt", "~/Documents/…/Archive/", "2026-04-27", "13.0 KiB", "—", "Review first"]),
            ]),
        },
        // `24:1476`, collapsed.
        Header {
            group: "G006 (3)",
            kind: "Similar text",
            title: "User guide",
            decision: "",
            rows: None,
        },
        // `24:1510`, collapsed.
        Header {
            group: "G008 (3)",
            kind: "Similar text",
            title: "Support procedures",
            decision: "",
            rows: None,
        },
        // `24:1527`, `24:1544`, `24:1575`, `24:1603`.
        Header {
            group: "G009 (3)",
            kind: "Containment",
            title: "Guide excerpt",
            decision: "",
            rows: Some(&[
                row(["", "—", "Guide.txt", "~/Documents/Guides/", "2026-02-10", "48.0 KiB", "—", "Reference"]),
                row(["100% inside reference", "—", "Guide_excerpt.txt", "~/Documents/…/Extracts/", "2026-02-12", "9.6 KiB", "—", "Unreviewed"]),
                row(["Contains the reference", "41 lines", "Guide_extended.txt", "~/Downloads/", "2026-08-30", "55.1 KiB", "—", "Review first"]),
            ]),
        },
    ];

    #[test]
    fn findings_rows_equal_frame_13_648() {
        let scenario = build();
        let sample = design::sample();
        let review = Review::replay(&scenario);
        let mut expanded = BTreeSet::new();
        for header in FINDINGS {
            let label = header.group.split_once(' ').expect("label").0;
            let group = group_by_label(&scenario, label);
            assert_eq!(
                format!("{} ({})", group.label, group.members.len()),
                header.group
            );
            assert_eq!(review.header_kind(group), header.kind, "{label} Match");
            assert_eq!(
                sample.group_titles.get(&group.id).map(String::as_str),
                Some(header.title),
                "{label} title"
            );
            assert_eq!(
                review.header_decision(group, &sample),
                header.decision,
                "{label} Decision"
            );
            let Some(rows) = header.rows else {
                continue;
            };
            expanded.insert(group.id);
            let projected = review.rows(group);
            let names: Vec<&str> = projected.iter().map(|m| m.name.as_str()).collect();
            let expected: Vec<&str> = rows.iter().map(|r| r.name).collect();
            assert_eq!(names, expected, "{label} row order");
            for (m, r) in projected.iter().zip(rows) {
                let at = format!("{label} {}", r.name);
                assert_eq!(review.match_label(m.file), r.matched, "{at} Match");
                assert_eq!(review.unique_label(m.file), r.unique, "{at} Unique");
                assert!(
                    folder_matches(r.folder, &m.folder),
                    "{at} Folder {}",
                    m.folder
                );
                assert_eq!(date(m.modified), r.modified, "{at} Modified");
                assert_eq!(kib(m.size_bytes), r.size, "{at} Size");
                // The size sits in the lower half of its displayed tenth, so
                // a truncating formatter shows the same value.
                assert_eq!(
                    m.size_bytes * 10 / 1024,
                    (m.size_bytes * 10 + 512) / 1024,
                    "{at}: size {} straddles a rounding boundary",
                    m.size_bytes
                );
                let protection = if review.is_protected(m.file) {
                    "Protected"
                } else {
                    "—"
                };
                assert_eq!(protection, r.protected, "{at} Protected");
                assert_eq!(review.decision_label(m.file), r.decision, "{at} Decision");
            }
        }
        let shown: BTreeSet<GroupId> = sample.expanded_groups.iter().copied().collect();
        assert_eq!(shown, expanded, "expanded groups");
        // The highlighted row `24:1341`.
        assert_eq!(
            sample.focused_file,
            Some(by_name(&scenario, "Design_Spec_draft.txt").file)
        );
    }

    /// Frame 01 `13:648`: relationship filters `13:729` to `13:755` and the
    /// status bar `13:1300` (PNG text, which adds "finished 14:32" to the
    /// layer name `13:1301`); frame 01e `30:2370`: search "guide v9".
    #[test]
    fn filter_counts_and_status_bar_equal_frames_13_648_and_30_2370() {
        let scenario = build();
        let sample = design::sample();
        let review = Review::replay(&scenario);
        let groups = &scenario.results.groups;
        let count = |kind: RelationKind| {
            groups
                .iter()
                .filter(|g| review.row_kinds(g).contains(&kind))
                .count()
        };
        let filters = [
            format!("All {}", groups.len()),
            format!("Same bytes {}", count(RelationKind::SameBytes)),
            format!("Same text {}", count(RelationKind::SameText)),
            format!("Similar text {}", count(RelationKind::SimilarText)),
            format!("Containment {}", count(RelationKind::Containment)),
        ];
        assert_eq!(
            filters,
            [
                "All 84",
                "Same bytes 42",
                "Same text 16",
                "Similar text 20",
                "Containment 6"
            ]
        );

        let matched: usize = groups.iter().map(|g| g.members.len()).sum();
        let status = [
            format!("{} text files scanned", thousands(sample.files_scanned)),
            format!("{} groups", groups.len()),
            format!("{matched} matched files"),
            format!(
                "finished {}:{:02}",
                sample.finished_at.hour, sample.finished_at.minute
            ),
        ];
        assert_eq!(
            status,
            [
                "12,648 text files scanned",
                "84 groups",
                "236 matched files",
                "finished 14:32"
            ]
        );
        assert_eq!(format!("{} queued", review.queued.len()), "3 queued");
        // Toolbar `13:717`.
        assert_eq!(
            format!("Review cleanup ({})", review.queued.len()),
            "Review cleanup (3)"
        );

        // 01e `30:2370`: no file name or folder contains the search.
        assert_eq!(sample.search_without_matches, "guide v9");
        let needle = sample.search_without_matches.to_lowercase();
        let hits = groups
            .iter()
            .flat_map(|g| &g.members)
            .filter(|m| {
                m.name.to_lowercase().contains(&needle) || m.folder.to_lowercase().contains(&needle)
            })
            .count();
        assert_eq!(hits, 0, "frame 01e shows no match for “guide v9”");
        assert_eq!(
            format!("{} groups are hidden", groups.len()),
            "84 groups are hidden"
        );
        assert_eq!(
            format!("{0} queued, {0} outside the search", review.queued.len()),
            "3 queued, 3 outside the search"
        );
    }

    // ── Frame 02 · Compare `13:1304` and popup Review unique text `16:932`

    /// Document A `13:1418`, lines `25:1025` to `25:1296`. Figma joins the
    /// highlighted words with no-break spaces and draws an empty line as one
    /// space; both are drawing artifacts, transcribed as plain spaces and
    /// empty lines.
    const COMPARE_A: [&str; 34] = [
        "Design Specification",
        "",
        "1. Overview",
        "This document describes the design for the document",
        "deduplication solution, including core functionality,",
        "data processing, and user experience.",
        "",
        "The system identifies duplicate and near-duplicate",
        "files across one or more folders.",
        "",
        "2. Functional Requirements",
        "2.1 Scan and identify duplicates",
        "The system scans selected locations, compares file content,",
        "and groups files that are exact or similar matches.",
        "",
        "2.2 Review and take action",
        "Users can review matched files, compare differences, and",
        "choose which files to retain or queue for cleanup.",
        "",
        "3. Non-Functional Requirements",
        "The solution is fast, reliable, and easy to use.",
        "",
        "3.1 Local processing",
        "Documents remain on the local filesystem.",
        "Scans never modify the source files.",
        "",
        "3.2 File safety",
        "Protected references cannot be queued for removal.",
        "Changed files must be rescanned before cleanup.",
        "",
        "4. Supported formats",
        "Plain text, Markdown, source and configuration files.",
        "",
        "End of specification.",
    ];

    /// Document B `13:1532`, lines `25:1304` to `25:1573`; the alignment
    /// spacer `25:1370` is not a line.
    const COMPARE_B: [&str; 33] = [
        "Design Specification",
        "",
        "1. Overview",
        "This document describes the design for the document",
        "deduplication solution, including core functionality,",
        "data processing, and user experience.",
        "",
        "The system finds near-duplicates within the selected scope.",
        "",
        "2. Functional Requirements",
        "2.1 Scan and identify duplicates",
        "The system scans selected locations, compares file content,",
        "and groups files that are exact or similar matches.",
        "",
        "2.2 Review and take action",
        "Users can review matched files, compare differences, and",
        "select the version to retain.",
        "",
        "3. Non-Functional Requirements",
        "The solution is fast, reliable, and easy to use.",
        "",
        "3.1 Local processing",
        "Documents remain on the local filesystem.",
        "Scans never modify the source files.",
        "",
        "3.2 File safety",
        "Protected references cannot be queued for removal.",
        "Changed files must be rescanned before cleanup.",
        "",
        "4. Supported formats",
        "Plain text, Markdown, source and configuration files.",
        "",
        "End of specification.",
    ];

    fn text_lines(bytes: &[u8]) -> Vec<String> {
        lines(bytes)
            .into_iter()
            .map(|l| String::from_utf8(l.to_vec()).expect("UTF-8 sample"))
            .collect()
    }

    #[test]
    fn compare_pair_equals_frame_13_1304() {
        let scenario = build();
        let sample = design::sample();
        let review = Review::replay(&scenario);
        let pair = sample.last_compared.expect("a compared pair");
        let (a, b) = (review.member(pair.a), review.member(pair.b));
        let group = review.group_of(pair.a);
        assert_eq!(group.id, pair.group);

        // Toolbar `13:1354`, `13:1355`; status bar `13:1638`.
        assert_eq!(group.label, "G004");
        assert_eq!(format!("{} files", group.members.len()), "3 files");
        assert_eq!(
            format!("{}  ↔  {}", a.name, b.name),
            "Design_Spec.txt  ↔  Design_Spec_draft.txt"
        );
        // Identities `13:1415`, `13:1529`; side labels `13:1417`, `13:1531`.
        let identity = |m: &Member| {
            format!(
                "{} · {} · modified {}",
                m.display_path,
                kib(m.size_bytes),
                date(m.modified)
            )
        };
        assert_eq!(
            identity(a),
            "~/Documents/Design/Design_Spec.txt · 12.8 KiB · modified 2026-05-02"
        );
        assert_eq!(
            identity(b),
            "~/Documents/Design/Drafts/Design_Spec_draft.txt · 12.6 KiB · modified 2026-04-18"
        );
        assert_eq!(review.decision_label(a.file), "Reference", "A · Reference");
        assert_eq!(
            review.decision_label(b.file),
            "Review first",
            "B · Review first"
        );
        assert_eq!(review.counterpart(b.file), Some(a.file));

        // Evidence `13:1373` to `13:1376`.
        let measurement = review.measurement(b.file).expect("measured");
        assert_eq!(kind_name(measurement.relation()), "Similar text");
        let Measurement::Similar(p) = measurement else {
            panic!("not Similar")
        };
        assert_eq!(format!("{} similar", percent(p.get())), "97.9% similar");
        let bytes_a = scenario.contents.render(a.file).expect("A");
        let bytes_b = scenario.contents.render(b.file).expect("B");
        assert_eq!(
            format!("Only in A: {} lines", unique_lines(&bytes_a, &bytes_b)),
            "Only in A: 3 lines"
        );
        assert_eq!(
            format!("Only in B: {} lines", unique_lines(&bytes_b, &bytes_a)),
            "Only in B: 2 lines"
        );

        // Visible lines, verbatim; the panes scroll past them.
        let (lines_a, lines_b) = (text_lines(&bytes_a), text_lines(&bytes_b));
        assert!(lines_a.len() > COMPARE_A.len() && lines_b.len() > COMPARE_B.len());
        assert_eq!(lines_a[..COMPARE_A.len()], COMPARE_A);
        assert_eq!(lines_b[..COMPARE_B.len()], COMPARE_B);

        // "Change 1 of 2" `13:1380`: the line diff of the whole files, with
        // line terminators set aside as §10.1 does, has exactly two hunks.
        let ops = similar::capture_diff_slices(similar::Algorithm::Myers, &lines_a, &lines_b);
        let mut hunks = 0;
        let mut in_change = false;
        for op in &ops {
            let changed = !matches!(op, similar::DiffOp::Equal { .. });
            if changed && !in_change {
                hunks += 1;
            }
            in_change = changed;
        }
        assert_eq!(format!("Change 1 of {hunks}"), "Change 1 of 2");
    }

    /// Popup Review unique text `16:932`.
    #[test]
    fn review_unique_text_equals_frame_16_932() {
        let scenario = build();
        let sample = design::sample();
        let pair = sample.last_compared.expect("a compared pair");
        let review = Review::replay(&scenario);
        let (target, counterpart) = (review.member(pair.b), review.member(pair.a));
        assert_eq!(review.counterpart(target.file), Some(counterpart.file));
        assert_eq!(
            [
                target.name.as_str(),
                &format!("{}/", target.folder),
                counterpart.name.as_str(),
                &format!("{}/", counterpart.folder),
            ],
            [
                "Design_Spec_draft.txt",
                "~/Documents/Design/Drafts/",
                "Design_Spec.txt",
                "~/Documents/Design/"
            ]
        );
        let bytes_t = scenario.contents.render(target.file).expect("target");
        let bytes_c = scenario
            .contents
            .render(counterpart.file)
            .expect("counterpart");
        let theirs: HashSet<String> = text_lines(&bytes_c).into_iter().collect();
        let only: Vec<(usize, String)> = text_lines(&bytes_t)
            .into_iter()
            .enumerate()
            .filter(|(_, l)| !theirs.contains(l))
            .map(|(i, l)| (i + 1, l))
            .collect();
        assert_eq!(
            format!("{} lines exist only in this file.", only.len()),
            "2 lines exist only in this file."
        );
        assert_eq!(
            only,
            [
                (
                    8,
                    "The system finds near-duplicates within the selected scope.".to_owned()
                ),
                (17, "select the version to retain.".to_owned()),
            ]
        );
    }

    // ── Cleanup popups `16:886`, `27:1390`, `27:1533` ────────────────────

    struct CleanupRow {
        file: &'static str,
        folder: &'static str,
        matched: &'static str,
        kept: &'static str,
        kept_folder: &'static str,
        size: &'static str,
    }

    /// Removal plan rows, identical in all three popups (`27:1493`,
    /// `27:1507`, `27:1521` in the running frame).
    const CLEANUP: [CleanupRow; 3] = [
        CleanupRow {
            file: "Project_Plan_copy.txt",
            folder: "~/Documents/Archive/",
            matched: "Identical",
            kept: "Project_Plan.txt",
            kept_folder: "~/Documents/Projects/ · protected",
            size: "12.8 KiB",
        },
        CleanupRow {
            file: "Project_Plan (2).txt",
            folder: "~/Backups/Documents/",
            matched: "Identical",
            kept: "Project_Plan.txt",
            kept_folder: "~/Documents/Projects/ · protected",
            size: "12.8 KiB",
        },
        CleanupRow {
            file: "Release_Notes_copy.txt",
            folder: "~/Documents/Release/Old/",
            matched: "Identical",
            kept: "Release_Notes.txt",
            kept_folder: "~/Documents/Release/",
            size: "22.4 KiB",
        },
    ];

    /// §10.4 states as the cleanup popups label them.
    const fn status_label(status: PlanItemStatus) -> &'static str {
        match status {
            PlanItemStatus::Planned => "Ready",
            PlanItemStatus::Approved => "Waiting",
            PlanItemStatus::Validating => "Checking…",
            PlanItemStatus::Executing => "Moving…",
            PlanItemStatus::Succeeded => "Moved",
            PlanItemStatus::Failed => "Failed",
            PlanItemStatus::Stopped => "Stopped",
            PlanItemStatus::Blocked => "Blocked",
            PlanItemStatus::NeedsReconciliation => "Needs checking",
        }
    }

    #[test]
    fn cleanup_rows_equal_frames_16_886_27_1390_27_1533() {
        let scenario = build();
        let sample = design::sample();
        let review = Review::replay(&scenario);
        let items = &sample.cleanup_items;
        let targets: Vec<FileId> = items.iter().map(|i| i.target).collect();
        assert_eq!(targets, review.queue_order, "plan follows the queue");
        assert_eq!(items.len(), CLEANUP.len());
        for (item, r) in items.iter().zip(&CLEANUP) {
            let (m, kept) = (review.member(item.target), review.member(item.counterpart));
            assert_eq!(review.queued.get(&item.target), Some(&item.counterpart));
            assert_eq!(item.size_bytes, m.size_bytes);
            let matched = match review.measurement(m.file) {
                Some(Measurement::Identical(_)) => "Identical",
                other => panic!("{}: {other:?}", m.name),
            };
            let kept_folder = if review.is_protected(kept.file) {
                format!("{}/ · protected", kept.folder)
            } else {
                format!("{}/", kept.folder)
            };
            assert_eq!(
                [
                    m.name.as_str(),
                    &format!("{}/", m.folder),
                    matched,
                    kept.name.as_str(),
                    &kept_folder,
                    &kib(item.size_bytes),
                ],
                [r.file, r.folder, r.matched, r.kept, r.kept_folder, r.size],
            );
        }

        // Review cleanup `16:886`.
        let statuses = |list: &[PlanItemStatus]| -> Vec<&str> {
            list.iter().copied().map(status_label).collect()
        };
        let planned: Vec<PlanItemStatus> = items.iter().map(|i| i.status).collect();
        assert_eq!(statuses(&planned), ["Ready", "Ready", "Ready"]);
        let kept_copies: BTreeSet<FileId> = items.iter().map(|i| i.counterpart).collect();
        assert!(items.iter().all(|i| matches!(
            review.measurement(i.target),
            Some(Measurement::Identical(_))
        )));
        assert_eq!(
            format!(
                "{} files queued · all byte-identical · {} kept copies",
                items.len(),
                kept_copies.len()
            ),
            "3 files queued · all byte-identical · 2 kept copies"
        );
        let total: u64 = items.iter().map(|i| i.size_bytes).sum();
        assert_eq!(
            format!("Destination: system Trash · {} logical size", kib(total)),
            "Destination: system Trash · 48.0 KiB logical size"
        );
        assert_eq!(
            format!("Move {} files to Trash", items.len()),
            "Move 3 files to Trash"
        );

        // Running `27:1390`.
        assert_eq!(
            statuses(&sample.cleanup_running),
            ["Moved", "Moving…", "Waiting"]
        );
        let moved = |list: &[PlanItemStatus]| {
            list.iter()
                .filter(|s| **s == PlanItemStatus::Succeeded)
                .count()
        };
        assert_eq!(
            format!(
                "{} of {} files moved",
                moved(&sample.cleanup_running),
                items.len()
            ),
            "1 of 3 files moved"
        );

        // Finished `27:1533`.
        assert_eq!(
            statuses(&sample.cleanup_finished),
            ["Moved", "Moved", "Moved"]
        );
        let failed = sample
            .cleanup_finished
            .iter()
            .filter(|s| **s == PlanItemStatus::Failed)
            .count();
        assert_eq!(
            format!(
                "{} moved · {failed} failed",
                moved(&sample.cleanup_finished)
            ),
            "3 moved · 0 failed"
        );
    }

    // ── 01b · Findings — scanning `28:2171` ──────────────────────────────

    #[test]
    fn scan_progress_equals_frame_28_2171() {
        let sample = design::sample();
        let p = &sample.scan_progress;
        let elapsed = p.elapsed.as_secs();
        assert_eq!(
            [
                format!("Reading files in {}/", p.current_folder),
                format!(
                    "{} of about {} files",
                    thousands(p.files_read),
                    thousands(sample.scan_estimated_files)
                ),
                format!("{} groups so far", p.groups_found),
                format!("{}:{:02} elapsed", elapsed / 60, elapsed % 60),
            ],
            [
                "Reading files in ~/Documents/Archive/",
                "8,214 of about 12,600 files",
                "61 groups so far",
                "0:42 elapsed"
            ]
        );
        // The progress is a scan of the same setup: its folder is a stored
        // scan folder.
        let scenario = build();
        assert!(
            scenario
                .results
                .setup
                .folders
                .iter()
                .any(|f| f.path.to_string_lossy() == p.current_folder)
        );
    }

    // ── Popups New scan `16:806` and Matching rules `16:735` ─────────────

    #[test]
    fn new_scan_and_matching_rules_equal_frames_16_806_and_16_735() {
        let scenario = build();
        let sample = design::sample();
        let review = Review::replay(&scenario);
        let folders: Vec<(String, bool)> = sample
            .new_scan
            .folders
            .iter()
            .map(|f| (format!("{}/", f.path.to_string_lossy()), f.protect))
            .collect();
        assert_eq!(
            folders,
            [
                ("~/Documents/Projects/".to_owned(), true),
                ("~/Documents/Archive/".to_owned(), false),
                ("~/Downloads/".to_owned(), false),
            ]
        );
        let types: Vec<String> = sample
            .new_scan
            .file_types_shown
            .iter()
            .map(|t| format!(".{t}"))
            .collect();
        assert_eq!(types, [".txt", ".md", ".rst", ".log", ".csv", ".json"]);
        assert_eq!(
            format!("+{} more", sample.new_scan.more_file_types),
            "+12 more"
        );
        assert_eq!(
            format!(
                "{} queued files will be cleared from the cleanup queue.",
                review.queued.len()
            ),
            "3 queued files will be cleared from the cleanup queue."
        );

        // The stored setup's folder protection agrees with the popup's for
        // the folder both list (Project_Plan.txt shows Protected).
        let projects = scenario
            .results
            .setup
            .folders
            .iter()
            .find(|f| f.path.to_string_lossy() == "~/Documents/Projects")
            .expect("stored setup lists ~/Documents/Projects");
        assert!(projects.protect);

        let rules = scenario.results.rules;
        assert_eq!(rules.mode, MatchMode::ExactAndFuzzy, "Exact + fuzzy text");
        let p = rules.minimum_similarity.get();
        assert_eq!(format!("{}.{} %", p / 10, p % 10), "95.0 %");
        assert_eq!(
            [
                rules.ignore.whitespace,
                rules.ignore.line_endings,
                rules.ignore.letter_case,
                rules.ignore.punctuation
            ],
            [true, true, false, false]
        );
    }

    // ── Determinism ──────────────────────────────────────────────────────

    #[test]
    fn the_scenario_is_deterministic_and_seed_independent() {
        let first = build();
        assert_eq!(first, build());
        let other =
            fixtures::build(ScenarioKind::Design, SCAN, DEFAULT_SEED ^ 0xFFFF).expect("design");
        assert_eq!(
            first.results, other.results,
            "frame data must not vary by seed"
        );
        assert_eq!(first.contents, other.contents);
        assert_eq!(first.seeded_decisions, other.seeded_decisions);
        assert_eq!(design::sample(), design::sample());
    }
}
