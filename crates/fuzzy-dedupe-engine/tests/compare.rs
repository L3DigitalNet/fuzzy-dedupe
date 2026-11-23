// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! PV-T6-001 to PV-T6-003 (plan T6): the comparison service, its generated
//! texts and the §10.1 unique-line measure.
//!
//! Every test sits in the `compare` module so the plan's proof filter
//! `-E 'test(compare)'` selects them all.
//!
//! The oracles are independent of the code under test: alignment is checked
//! against the generator's own edit script, never against `similar`; unique
//! lines are recounted here by set membership over lines this file splits
//! itself; and the UI check 06 expectations are written out by hand.

mod compare {
    use std::collections::HashSet;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use fuzzy_dedupe_domain::{
        CompareMode, CounterpartBasis, FileId, IgnoreOptions, MemberRole, QueueOrigin, Refusal,
        RequestId, RulesRevision, ScanId, SnapshotId,
    };
    use fuzzy_dedupe_engine::compare::normalize::{is_punctuation, normalize_line};
    use fuzzy_dedupe_engine::compare::texts::{self, EditOp, LARGE_BYTES, LARGE_LINES, LargePair};
    use fuzzy_dedupe_engine::compare::{
        AlignedComparison, Comparison, ComparisonOutcome, RowKind, Unavailable, measure_unique,
    };
    use fuzzy_dedupe_engine::fixtures::{self, DEFAULT_SEED, Scenario, ScenarioKind};
    use fuzzy_dedupe_engine::review::ReviewCommand;
    use fuzzy_dedupe_engine::service::{
        ComparisonRequest, Engine, EngineConfig, EngineEvent, EngineRequest, Origin, ReviewStatus,
    };
    use proptest::prelude::*;
    use proptest::test_runner::Config as ProptestConfig;
    use unicode_normalization::UnicodeNormalization;

    const NO_IGNORE: IgnoreOptions = IgnoreOptions {
        whitespace: false,
        line_endings: false,
        letter_case: false,
        punctuation: false,
    };

    // ── Shared helpers ───────────────────────────────────────────────────

    /// This file's own line split: LF or CRLF terminate, nothing else.
    fn lines(text: &str) -> Vec<&str> {
        let mut out: Vec<&str> = text
            .split('\n')
            .map(|l| l.strip_suffix('\r').unwrap_or(l))
            .collect();
        if text.is_empty() || text.ends_with('\n') {
            out.pop();
        }
        out
    }

    /// The §10.1 measure by set membership, every occurrence counted.
    fn oracle_unique<'t>(target: &[&'t str], counterpart: &[&str]) -> Vec<&'t str> {
        let present: HashSet<&str> = counterpart.iter().copied().collect();
        target
            .iter()
            .copied()
            .filter(|l| !present.contains(l))
            .collect()
    }

    fn compute(a: &str, b: &str, mode: CompareMode, ignore: IgnoreOptions) -> AlignedComparison {
        AlignedComparison::compute(
            (FileId::new(1), Arc::from(a)),
            (FileId::new(2), Arc::from(b)),
            mode,
            ignore,
        )
    }

    fn adopt(engine: &mut Engine, scenario: Scenario) -> Arc<Scenario> {
        let scenario = Arc::new(scenario);
        engine
            .adopt_scenario(Arc::clone(&scenario))
            .expect("fresh scan ID");
        scenario
    }

    fn fresh_scan(engine: &mut Engine, kind: ScenarioKind) -> Scenario {
        let scan = engine.reserve_scan_id();
        fixtures::build(kind, scan, DEFAULT_SEED).expect("generated scenario")
    }

    fn request_for(scenario: &Scenario, target: FileId, counterpart: FileId) -> ComparisonRequest {
        let snapshot = |file: FileId| {
            scenario
                .results
                .group_of(file)
                .and_then(|g| g.member(file))
                .expect("member")
                .snapshot
        };
        ComparisonRequest {
            scan: scenario.results.scan,
            target,
            counterpart,
            target_snapshot: snapshot(target),
            counterpart_snapshot: snapshot(counterpart),
            rules: scenario.results.rules.revision,
            mode: CompareMode::Original,
        }
    }

    fn submit(engine: &mut Engine, request: ComparisonRequest) -> RequestId {
        match engine.submit(EngineRequest::LoadComparison(request)) {
            Ok(Origin::Request(id)) => id,
            other => panic!("LoadComparison: {other:?}"),
        }
    }

    /// Drains until `request`'s comparison arrives, collecting every event
    /// seen on the way. Generous deadline: debug builds diff 5 MB slowly.
    fn await_comparison(
        engine: &mut Engine,
        request: RequestId,
        seen: &mut Vec<EngineEvent>,
    ) -> Arc<Comparison> {
        let deadline = Instant::now() + Duration::from_mins(2);
        loop {
            for event in engine.drain_events(16) {
                let found = match &event {
                    EngineEvent::Comparison {
                        request: r,
                        comparison,
                    } if *r == request => Some(Arc::clone(comparison)),
                    EngineEvent::NotImplemented { origin, missing }
                        if *origin == Origin::Request(request) =>
                    {
                        panic!("{request:?} answered {missing:?}")
                    }
                    _ => None,
                };
                seen.push(event);
                if let Some(found) = found {
                    return found;
                }
            }
            assert!(Instant::now() < deadline, "no comparison for {request:?}");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn ready(comparison: &Comparison) -> &AlignedComparison {
        match &comparison.outcome {
            ComparisonOutcome::Ready(model) => model,
            ComparisonOutcome::Unavailable(why) => panic!("unavailable: {why:?}"),
        }
    }

    fn evidence_status(
        engine: &mut Engine,
        req: &ComparisonRequest,
        id: RequestId,
    ) -> ReviewStatus {
        engine
            .review(ReviewCommand::AcknowledgeUniqueText {
                file: req.target,
                counterpart: req.counterpart,
                reviewed: Some(id),
            })
            .status
    }

    fn evidence_missing(status: &ReviewStatus) -> bool {
        matches!(
            status,
            ReviewStatus::Refused {
                refusal: Refusal::EvidenceUnavailable,
                ..
            }
        )
    }

    // ── PV-T6-001: generated texts and declared Unique values ────────────

    fn within(value: usize, target: usize, permille: usize) -> bool {
        let tolerance = target * permille / 1000;
        (target - tolerance..=target + tolerance).contains(&value)
    }

    #[test]
    fn compare_large_texts_meet_their_size_line_and_change_budgets() {
        let started = Instant::now();
        let pair = texts::large_pair(DEFAULT_SEED);
        eprintln!("large_pair generation: {:?}", started.elapsed());
        for (side, text, changed) in [
            ("A", &pair.a, pair.changed_in_a()),
            ("B", &pair.b, pair.changed_in_b()),
        ] {
            let count = lines(text).len();
            eprintln!(
                "{side}: {} bytes, {count} lines, {changed} changed",
                text.len()
            );
            assert!(
                within(text.len(), LARGE_BYTES, 50),
                "{side} size {}",
                text.len()
            );
            assert!(within(count, LARGE_LINES, 10), "{side} lines {count}");
            assert!(
                within(changed * 1000, count * 100, 100),
                "{side} changed {changed} of {count}"
            );
        }
        // The changed-line count the script claims is the one the texts
        // have: lines of each side that are absent from the other.
        let (a, b) = (lines(&pair.a), lines(&pair.b));
        assert_eq!(oracle_unique(&a, &b).len(), pair.changed_in_a());
        assert_eq!(oracle_unique(&b, &a).len(), pair.changed_in_b());
    }

    #[test]
    fn compare_large_texts_are_deterministic_per_seed() {
        assert_eq!(texts::large_pair(5), texts::large_pair(5));
        assert_ne!(texts::large_pair(5).a, texts::large_pair(6).a);
    }

    /// The §17.3 UI check 06 pairs as (side A, side B) of a direct
    /// `compute`, with the hand-written Only in A / Only in B lines in file
    /// order. The measure is symmetric in its arguments, so these cases hold
    /// whichever side a service request puts its target on.
    struct Case {
        name: &'static str,
        a: &'static str,
        b: &'static str,
        only_a: &'static [&'static str],
        only_b: &'static [&'static str],
        /// The diff draws at least one change even though no line is unique.
        diff_changes: bool,
    }

    const CASES: &[Case] = &[
        Case {
            name: "reordered identical lines",
            a: "alpha\nbeta\ngamma\ndelta\n",
            b: "delta\ngamma\nalpha\nbeta\n",
            only_a: &[],
            only_b: &[],
            diff_changes: true,
        },
        Case {
            name: "repeated in the file, present once in the counterpart",
            a: "alpha\nalpha\nalpha\nbeta\n",
            b: "alpha\nbeta\n",
            only_a: &[],
            only_b: &[],
            diff_changes: true,
        },
        Case {
            name: "repeated absent line",
            a: "alpha\nomega\nbeta\nomega\nomega\n",
            b: "alpha\nbeta\n",
            only_a: &["omega", "omega", "omega"],
            only_b: &[],
            diff_changes: true,
        },
        Case {
            name: "CRLF input",
            a: "alpha\r\nbeta\r\ngamma\r\n",
            b: "alpha\nbeta\ngamma\n",
            only_a: &[],
            only_b: &[],
            diff_changes: false,
        },
        Case {
            name: "missing final newline",
            a: "alpha\nbeta\ngamma",
            b: "alpha\nbeta\ngamma\n",
            only_a: &[],
            only_b: &[],
            diff_changes: false,
        },
        Case {
            name: "mixed line endings",
            a: "one\r\ntwo\nthree\r\nfour",
            b: "one\ntwo\r\nfive\n",
            only_a: &["three", "four"],
            only_b: &["five"],
            diff_changes: true,
        },
        Case {
            name: "file order of unique lines",
            a: "zeta\nalpha\nmid\nbeta\nzeta\n",
            b: "beta\nalpha\n",
            only_a: &["zeta", "mid", "zeta"],
            only_b: &[],
            diff_changes: true,
        },
        Case {
            // Decomposed "e" + U+0301 against precomposed U+00E9: distinct
            // original text, equal once NFC applies.
            name: "non-NFC input",
            a: "Cafe\u{301} menu\nsoup\n",
            b: "Caf\u{e9} menu\nsoup\n",
            only_a: &["Cafe\u{301} menu"],
            only_b: &["Caf\u{e9} menu"],
            diff_changes: true,
        },
    ];

    #[test]
    fn compare_small_pairs_cover_crlf_mixed_endings_and_non_nfc_input() {
        let crlf = CASES
            .iter()
            .any(|c| c.a.contains("\r\n") && !c.b.contains('\r'));
        let mixed = CASES.iter().any(|c| {
            let text = c.a;
            text.contains("\r\n") && text.replace("\r\n", "").contains('\n')
        });
        let non_nfc = CASES
            .iter()
            .any(|c| c.a.nfc().collect::<String>() != c.a || c.b.nfc().collect::<String>() != c.b);
        assert!(
            crlf && mixed && non_nfc,
            "crlf {crlf}, mixed {mixed}, non-NFC {non_nfc}"
        );
    }

    /// Every (file, counterpart, declared Unique) a scenario asserts: each
    /// member row's link and both ends of each direct edge.
    fn declared(scenario: &Scenario) -> Vec<(FileId, FileId, u64)> {
        let mut out = Vec::new();
        for group in &scenario.results.groups {
            for member in &group.members {
                if let Some(link) = member.link() {
                    out.push((member.file, link.counterpart, link.unique_lines));
                }
            }
            for edge in &group.edges {
                out.push((edge.a, edge.b, edge.unique_lines_a));
                out.push((edge.b, edge.a, edge.unique_lines_b));
            }
        }
        out
    }

    fn text_of(scenario: &Scenario, file: FileId) -> String {
        String::from_utf8(scenario.contents.render(file).expect("buffer")).expect("UTF-8")
    }

    /// The declared values the comparison service's measure disagrees with.
    fn mismatches(scenario: &Scenario) -> Vec<(FileId, FileId, u64, u64)> {
        declared(scenario)
            .into_iter()
            .filter_map(|(file, counterpart, value)| {
                let (target, other) = (text_of(scenario, file), text_of(scenario, counterpart));
                let count = oracle_unique(&lines(&target), &lines(&other)).len() as u64;
                (count != value).then_some((file, counterpart, value, count))
            })
            .collect()
    }

    fn generated_scenarios() -> Vec<Scenario> {
        let mut out: Vec<Scenario> = ScenarioKind::ALL
            .into_iter()
            // Design is T9's; until it lands its builder answers
            // NotImplemented, and once it does it is checked here too.
            .filter_map(|kind| fixtures::build(kind, ScanId::new(3), DEFAULT_SEED).ok())
            .collect();
        out.push(texts::large_pair_scenario(ScanId::new(3), DEFAULT_SEED));
        out
    }

    #[test]
    fn compare_declared_unique_equals_the_computed_count_for_every_fixture_member() {
        for scenario in generated_scenarios() {
            assert_eq!(mismatches(&scenario), Vec::new(), "{:?}", scenario.kind);
        }
    }

    #[test]
    fn compare_a_declared_unique_off_by_one_is_caught() {
        let mut scenario =
            fixtures::build(ScenarioKind::Mixed, ScanId::new(3), DEFAULT_SEED).expect("mixed");
        let results = Arc::make_mut(&mut scenario.results);
        let member = results.groups[0]
            .members
            .iter_mut()
            .find(|m| m.link().is_some())
            .expect("a member row");
        let file = member.file;
        if let fuzzy_dedupe_domain::MemberRole::Member(link) = &mut member.role {
            link.unique_lines += 1;
        }
        let found = mismatches(&scenario);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].0, file);
    }

    #[test]
    fn compare_service_unique_counts_match_the_findings_values() {
        let mut engine = Engine::new(EngineConfig::default()).expect("engine starts");
        for kind in [
            ScenarioKind::Mixed,
            ScenarioKind::Unicode,
            ScenarioKind::Stale,
        ] {
            let scenario = fresh_scan(&mut engine, kind);
            let scenario = adopt(&mut engine, scenario);
            for group in &scenario.results.groups {
                for member in &group.members {
                    let Some(link) = member.link() else { continue };
                    let req = request_for(&scenario, member.file, link.counterpart);
                    let id = submit(&mut engine, req);
                    let got = await_comparison(&mut engine, id, &mut Vec::new());
                    assert_eq!(got.request, req);
                    assert_eq!(
                        ready(&got).only_in_b.count(),
                        link.unique_lines,
                        "{kind:?} {:?}",
                        member.name
                    );
                }
            }
        }
    }

    // ── PV-T6-002: alignment, normalization, supersession, threading ─────

    /// The rows an edit script implies: (kind tag, A index, B index).
    fn expected_rows(script: &[EditOp]) -> Vec<(char, Option<usize>, Option<usize>)> {
        let (mut i, mut j) = (0, 0);
        let mut rows = Vec::new();
        for op in script {
            match *op {
                EditOp::Keep(n) => (0..n).for_each(|k| rows.push(('=', Some(i + k), Some(j + k)))),
                EditOp::Replace(n) => {
                    (0..n).for_each(|k| rows.push(('~', Some(i + k), Some(j + k))));
                }
                EditOp::Delete(n) => (0..n).for_each(|k| rows.push(('-', Some(i + k), None))),
                EditOp::Insert(n) => (0..n).for_each(|k| rows.push(('+', None, Some(j + k)))),
            }
            match *op {
                EditOp::Keep(n) | EditOp::Replace(n) => (i, j) = (i + n, j + n),
                EditOp::Delete(n) => i += n,
                EditOp::Insert(n) => j += n,
            }
        }
        rows
    }

    fn actual_rows(model: &AlignedComparison) -> Vec<(char, Option<usize>, Option<usize>)> {
        model
            .rows
            .iter()
            .map(|row| {
                let tag = match row.kind {
                    RowKind::Equal => '=',
                    RowKind::Changed(_) => '~',
                    RowKind::OnlyA => '-',
                    RowKind::OnlyB => '+',
                };
                (tag, row.a, row.b)
            })
            .collect()
    }

    fn check_alignment(pair: &LargePair, model: &AlignedComparison) {
        assert_eq!(actual_rows(model), expected_rows(&pair.script));
        let hunks = pair
            .script
            .iter()
            .filter(|op| !matches!(op, EditOp::Keep(_)))
            .count();
        assert_eq!(model.hunks.len(), hunks);
        for hunk in &model.hunks {
            assert!(
                model.rows[hunk.rows.clone()]
                    .iter()
                    .all(|r| r.kind != RowKind::Equal)
            );
        }
        // Spacers carry no line number: every one-sided row has exactly one.
        for row in &model.rows {
            match row.kind {
                RowKind::OnlyA => assert!(row.a.is_some() && row.b.is_none()),
                RowKind::OnlyB => assert!(row.a.is_none() && row.b.is_some()),
                _ => assert!(row.a.is_some() && row.b.is_some()),
            }
        }
    }

    #[test]
    fn compare_alignment_of_the_large_pair_equals_the_generator_edit_script() {
        let pair = texts::large_pair(DEFAULT_SEED);
        let started = Instant::now();
        let model = compute(&pair.a, &pair.b, CompareMode::Original, NO_IGNORE);
        eprintln!("large_pair alignment (Original): {:?}", started.elapsed());
        check_alignment(&pair, &model);
        assert_eq!(model.only_in_a.lines.len(), pair.changed_in_a());
        assert_eq!(model.only_in_b.lines.len(), pair.changed_in_b());
        // Inline spans fall on character boundaries of the shown lines.
        for row in &model.rows {
            if let (RowKind::Changed(spans), Some(i), Some(j)) = (&row.kind, row.a, row.b) {
                let (la, lb) = (model.a.shown().line(i), model.b.shown().line(j));
                let (la, lb) = (la.expect("line"), lb.expect("line"));
                assert!(!spans.a.is_empty() || !spans.b.is_empty());
                assert!(spans.a.iter().all(|r| la.get(r.clone()).is_some()));
                assert!(spans.b.iter().all(|r| lb.get(r.clone()).is_some()));
            }
        }
    }

    /// Builds a pair from a script whose lines are all distinct except kept
    /// copies, the same construction the large generator uses.
    fn pair_from(script: Vec<EditOp>) -> LargePair {
        let (mut a, mut b) = (String::new(), String::new());
        let (mut na, mut nb) = (0, 0);
        for op in &script {
            let (from_a, to_b, copy) = match *op {
                EditOp::Keep(n) => (n, 0, true),
                EditOp::Replace(n) => (n, n, false),
                EditOp::Delete(n) => (n, 0, false),
                EditOp::Insert(n) => (0, n, false),
            };
            for _ in 0..from_a {
                let line = format!("old line {na}\n");
                na += 1;
                a.push_str(&line);
                if copy {
                    b.push_str(&line);
                }
            }
            for _ in 0..to_b {
                let line = format!("new line {nb}\n");
                b.push_str(&line);
                nb += 1;
            }
        }
        LargePair {
            a: a.into(),
            b: b.into(),
            script,
        }
    }

    fn script_strategy() -> impl Strategy<Value = Vec<EditOp>> {
        let hunk = prop_oneof![
            (1..5usize).prop_map(EditOp::Replace),
            (1..5usize).prop_map(EditOp::Delete),
            (1..5usize).prop_map(EditOp::Insert),
        ];
        (
            0..4usize,
            prop::collection::vec((hunk, 1..6usize), 0..12),
            0..4usize,
        )
            .prop_map(|(lead, body, trail)| {
                let mut script = Vec::new();
                if lead > 0 {
                    script.push(EditOp::Keep(lead));
                }
                let last = body.len();
                for (k, (hunk, gap)) in body.into_iter().enumerate() {
                    script.push(hunk);
                    if k + 1 < last {
                        script.push(EditOp::Keep(gap));
                    }
                }
                if trail > 0 {
                    script.push(EditOp::Keep(trail));
                }
                script
            })
    }

    /// A line alphabet with decomposed and precomposed accents, ligatures,
    /// case pairs, whitespace and punctuation, so every Ignore option and
    /// NFC have something to act on.
    fn line_strategy() -> impl Strategy<Value = String> {
        prop::collection::vec(
            prop::sample::select(vec![
                'a', 'b', 'A', 'B', 'e', '\u{e9}', '\u{301}', 'u', '\u{308}', '\u{fc}', 'A',
                '\u{c5}', '\u{212b}', '\u{fb01}', '\u{df}', ' ', ' ', '\t', ',', '.', '!', '-',
                '\u{3001}', '\u{2014}',
            ]),
            0..12,
        )
        .prop_map(|chars| chars.into_iter().collect())
    }

    fn ignore_strategy() -> impl Strategy<Value = IgnoreOptions> {
        (any::<bool>(), any::<bool>(), any::<bool>(), any::<bool>()).prop_map(
            |(whitespace, line_endings, letter_case, punctuation)| IgnoreOptions {
                whitespace,
                line_endings,
                letter_case,
                punctuation,
            },
        )
    }

    /// An independent statement of the profile in `normalize.rs`: NFC,
    /// then each enabled option, in the documented order.
    fn oracle_normalize(line: &str, ignore: IgnoreOptions) -> String {
        let mut s: String = line.nfc().collect();
        if ignore.punctuation {
            s = s.chars().filter(|c| !oracle_punctuation(*c)).collect();
        }
        if ignore.letter_case {
            s = s.to_lowercase();
        }
        if ignore.whitespace {
            s = s.split_whitespace().collect::<Vec<_>>().join(" ");
        }
        s
    }

    /// The punctuation class as `normalize.rs` documents it, restated here
    /// as a table of code-point ranges rather than borrowed from the code.
    const PUNCTUATION_RANGES: &[(char, char)] = &[
        ('!', '/'),
        (':', '@'),
        ('[', '`'),
        ('{', '~'),
        ('\u{A1}', '\u{A1}'),
        ('\u{A7}', '\u{A7}'),
        ('\u{AB}', '\u{AB}'),
        ('\u{B6}', '\u{B7}'),
        ('\u{BB}', '\u{BB}'),
        ('\u{BF}', '\u{BF}'),
        ('\u{2010}', '\u{2027}'),
        ('\u{2030}', '\u{205E}'),
        ('\u{3001}', '\u{3003}'),
        ('\u{3008}', '\u{301F}'),
        ('\u{FF01}', '\u{FF0F}'),
        ('\u{FF1A}', '\u{FF20}'),
        ('\u{FF3B}', '\u{FF40}'),
        ('\u{FF5B}', '\u{FF65}'),
    ];

    fn oracle_punctuation(c: char) -> bool {
        PUNCTUATION_RANGES
            .iter()
            .any(|(low, high)| (*low..=*high).contains(&c))
    }

    #[test]
    fn compare_punctuation_class_matches_a_literal_table() {
        // Both ends of every range, plus neighbours just outside them.
        let inside = [
            '!', '/', ':', '@', '[', '`', '{', '~', ',', '.', '\u{A1}', '\u{A7}', '\u{AB}',
            '\u{B6}', '\u{B7}', '\u{BB}', '\u{BF}', '\u{2010}', '\u{2014}', '\u{2027}', '\u{2030}',
            '\u{205E}', '\u{3001}', '\u{3003}', '\u{3008}', '\u{301F}', '\u{FF01}', '\u{FF0F}',
            '\u{FF1A}', '\u{FF20}', '\u{FF3B}', '\u{FF40}', '\u{FF5B}', '\u{FF65}',
        ];
        let outside = [
            'a', 'Z', '0', '9', ' ', '\t', '\u{A0}', '\u{A8}', '\u{B5}', '\u{C0}', '\u{E9}',
            '\u{200F}', '\u{2028}', '\u{202F}', '\u{205F}', '\u{3000}', '\u{3004}', '\u{3020}',
            '\u{FF10}', '\u{FF21}', '\u{FF41}', '\u{FF66}', '\u{65E5}',
        ];
        for c in inside {
            assert!(is_punctuation(c), "{c:?} should be punctuation");
            assert!(oracle_punctuation(c), "table misses {c:?}");
        }
        for c in outside {
            assert!(!is_punctuation(c), "{c:?} should not be punctuation");
            assert!(!oracle_punctuation(c), "table wrongly holds {c:?}");
        }
    }

    fn config() -> ProptestConfig {
        // No failure persistence: the engine's tests write no file (plan
        // D-005), and proptest's default would save regressions beside the
        // sources.
        ProptestConfig {
            failure_persistence: None,
            cases: 256,
            ..ProptestConfig::default()
        }
    }

    proptest! {
        #![proptest_config(config())]

        #[test]
        fn compare_alignment_equals_random_edit_scripts(script in script_strategy()) {
            let pair = pair_from(script);
            let model = compute(&pair.a, &pair.b, CompareMode::Original, NO_IGNORE);
            prop_assert_eq!(actual_rows(&model), expected_rows(&pair.script));
        }

        #[test]
        fn compare_normalize_line_is_nfc_plus_the_enabled_options(
            line in line_strategy(),
            ignore in ignore_strategy(),
        ) {
            prop_assert_eq!(normalize_line(&line, ignore), oracle_normalize(&line, ignore));
            let nothing = IgnoreOptions { line_endings: ignore.line_endings, ..NO_IGNORE };
            prop_assert_eq!(normalize_line(&line, nothing), line.nfc().collect::<String>());
        }

        #[test]
        fn compare_canonically_equivalent_lines_normalize_alike(
            line in line_strategy(),
            ignore in ignore_strategy(),
        ) {
            let nfd: String = line.nfd().collect();
            prop_assert_eq!(normalize_line(&nfd, ignore), normalize_line(&line, ignore));
        }

        #[test]
        fn compare_each_ignore_option_absorbs_its_own_difference(
            words in prop::collection::vec("[a-z]{1,6}", 1..6),
        ) {
            let plain = words.join(" ");
            let spaced = format!("  {}\t", words.join(" \t  "));
            let upper = plain.to_uppercase();
            let punctuated = words.join(", ") + ".";
            let on = |f: fn(&mut IgnoreOptions)| { let mut o = NO_IGNORE; f(&mut o); o };
            let ws = on(|o| o.whitespace = true);
            let case = on(|o| o.letter_case = true);
            let punct = on(|o| { o.punctuation = true; o.whitespace = true; });
            prop_assert_eq!(normalize_line(&spaced, ws), normalize_line(&plain, ws));
            prop_assert_eq!(normalize_line(&upper, case), normalize_line(&plain, case));
            prop_assert_eq!(normalize_line(&punctuated, punct), normalize_line(&plain, punct));
            // And none of them is applied when it is off.
            prop_assert_ne!(normalize_line(&spaced, NO_IGNORE), plain.clone());
            prop_assert_ne!(normalize_line(&upper, NO_IGNORE), plain.clone());
        }

        #[test]
        fn compare_normalized_model_counts_and_rows_use_normalized_lines(
            a in prop::collection::vec(line_strategy(), 0..8),
            b in prop::collection::vec(line_strategy(), 0..8),
            ignore in ignore_strategy(),
            crlf in any::<bool>(),
        ) {
            let end = if crlf { "\r\n" } else { "\n" };
            let (ta, tb) = (a.concat_lines(end), b.concat_lines("\n"));
            let model = compute(&ta, &tb, CompareMode::Normalized, ignore);
            let na: Vec<String> = lines(&ta).iter().map(|l| oracle_normalize(l, ignore)).collect();
            let nb: Vec<String> = lines(&tb).iter().map(|l| oracle_normalize(l, ignore)).collect();
            let shown_a: Vec<&str> =
                (0..model.a.shown().line_count()).filter_map(|i| model.a.shown().line(i)).collect();
            prop_assert_eq!(shown_a, na.iter().map(String::as_str).collect::<Vec<_>>());
            // Original line numbering survives normalization line for line.
            prop_assert_eq!(model.a.original.line_count(), lines(&ta).len());
            let ra: Vec<&str> = na.iter().map(String::as_str).collect();
            let rb: Vec<&str> = nb.iter().map(String::as_str).collect();
            let want: Vec<&str> = oracle_unique(&ra, &rb);
            let got: Vec<&str> = model.only_in_a.lines.iter().map(|i| ra[*i]).collect();
            prop_assert_eq!(got, want);
            prop_assert!(model.only_in_a.normalized && model.only_in_b.normalized);
            // Equal rows pair equal normalized lines.
            for row in &model.rows {
                if let (RowKind::Equal, Some(i), Some(j)) = (&row.kind, row.a, row.b) {
                    prop_assert_eq!(&na[i], &nb[j]);
                }
            }
        }
    }

    trait ConcatLines {
        fn concat_lines(&self, end: &str) -> String;
    }

    impl ConcatLines for Vec<String> {
        fn concat_lines(&self, end: &str) -> String {
            self.iter().flat_map(|l| [l.as_str(), end]).collect()
        }
    }

    /// `req`'s scenario with the target's snapshot or the rules revision
    /// changed, republished under the same scan ID.
    fn republish(engine: &Engine, scenario: &Scenario, edit: impl FnOnce(&mut Scenario)) {
        let mut changed = scenario.clone();
        edit(&mut changed);
        let generation = engine.active().begin();
        assert!(engine.active().publish(generation, Arc::new(changed)));
    }

    #[test]
    fn compare_cache_hits_are_revalidated_against_the_active_scenario() {
        let mut engine = Engine::new(EngineConfig::default()).expect("engine starts");
        let scenario = fresh_scan(&mut engine, ScenarioKind::Mixed);
        let scenario = adopt(&mut engine, scenario);
        let group = &scenario.results.groups[0];
        let req = request_for(&scenario, group.members[1].file, group.members[0].file);
        let warm = |engine: &mut Engine| {
            let id = submit(engine, req);
            ready(&await_comparison(engine, id, &mut Vec::new()));
        };
        let refused = |engine: &mut Engine, why: Unavailable| {
            let id = submit(engine, req);
            let got = await_comparison(engine, id, &mut Vec::new());
            assert_eq!(got.outcome, ComparisonOutcome::Unavailable(why));
            assert!(
                evidence_missing(&evidence_status(engine, &req, id)),
                "{why:?}"
            );
        };

        warm(&mut engine);
        republish(&engine, &scenario, |s| {
            let results = Arc::make_mut(&mut s.results);
            let target = results.groups[0]
                .members
                .iter_mut()
                .find(|m| m.file == req.target)
                .expect("target");
            target.snapshot = SnapshotId::new(55_555);
        });
        refused(
            &mut engine,
            Unavailable::SnapshotMismatch {
                file: req.target,
                requested: req.target_snapshot,
                current: SnapshotId::new(55_555),
            },
        );

        republish(&engine, &scenario, |_| {});
        warm(&mut engine);
        republish(&engine, &scenario, |s| {
            Arc::make_mut(&mut s.results).rules.revision = RulesRevision::new(42);
        });
        refused(
            &mut engine,
            Unavailable::RulesMismatch {
                requested: req.rules,
                current: RulesRevision::new(42),
            },
        );

        republish(&engine, &scenario, |_| {});
        warm(&mut engine);
        let newer = fresh_scan(&mut engine, ScenarioKind::Mixed);
        let newer = adopt(&mut engine, newer);
        refused(
            &mut engine,
            Unavailable::ScanMismatch {
                requested: req.scan,
                current: newer.results.scan,
            },
        );
    }

    #[test]
    fn compare_evidence_is_recorded_for_each_delivered_result() {
        let mut engine = Engine::new(EngineConfig::default()).expect("engine starts");
        let scenario = fresh_scan(&mut engine, ScenarioKind::Mixed);
        let scenario = adopt(&mut engine, scenario);
        let group = &scenario.results.groups[0];
        let req = request_for(&scenario, group.members[1].file, group.members[0].file);
        for mode in [CompareMode::Original, CompareMode::Normalized] {
            let req = ComparisonRequest { mode, ..req };
            let id = submit(&mut engine, req);
            let got = await_comparison(&mut engine, id, &mut Vec::new());
            // The record follows the enqueue: a review in between is
            // refused (fail-closed), so allow it a moment to appear.
            let deadline = Instant::now() + Duration::from_secs(10);
            while evidence_missing(&evidence_status(&mut engine, &req, id)) {
                assert!(Instant::now() < deadline, "{mode:?} never recorded");
                std::thread::sleep(Duration::from_millis(1));
            }
            assert_eq!(ready(&got).mode, mode);
            assert_eq!(
                ready(&got).only_in_b.normalized,
                mode == CompareMode::Normalized
            );
        }
    }

    #[test]
    fn compare_unavailable_requests_record_no_evidence() {
        let mut engine = Engine::new(EngineConfig::default()).expect("engine starts");
        let scenario = fresh_scan(&mut engine, ScenarioKind::Mixed);
        let group = &scenario.results.groups[0];
        let (target, counterpart) = (group.members[1].file, group.members[0].file);
        let good = request_for(&scenario, target, counterpart);

        // Nothing adopted yet.
        let id = submit(&mut engine, good);
        let got = await_comparison(&mut engine, id, &mut Vec::new());
        assert_eq!(
            got.outcome,
            ComparisonOutcome::Unavailable(Unavailable::NoScenario)
        );

        let scenario = adopt(&mut engine, scenario);
        let bad = [
            (
                ComparisonRequest {
                    scan: ScanId::new(999),
                    ..good
                },
                Unavailable::ScanMismatch {
                    requested: ScanId::new(999),
                    current: scenario.results.scan,
                },
            ),
            (
                ComparisonRequest {
                    rules: RulesRevision::new(99),
                    ..good
                },
                Unavailable::RulesMismatch {
                    requested: RulesRevision::new(99),
                    current: scenario.results.rules.revision,
                },
            ),
            (
                ComparisonRequest {
                    target_snapshot: SnapshotId::new(5_000),
                    ..good
                },
                Unavailable::SnapshotMismatch {
                    file: target,
                    requested: SnapshotId::new(5_000),
                    current: good.target_snapshot,
                },
            ),
            (
                ComparisonRequest {
                    counterpart: FileId::new(9_999),
                    ..good
                },
                Unavailable::FileMissing(FileId::new(9_999)),
            ),
        ];
        for (req, why) in bad {
            let id = submit(&mut engine, req);
            let got = await_comparison(&mut engine, id, &mut Vec::new());
            assert_eq!(got.outcome, ComparisonOutcome::Unavailable(why));
            assert!(
                evidence_missing(&evidence_status(&mut engine, &req, id)),
                "{why:?}"
            );
        }
    }

    // ── PV-T6-003: the §10.1 measure on the UI check 06 fixtures ─────────

    fn expected_copy(lines: &[&str]) -> String {
        lines.iter().flat_map(|l| [*l, "\n"]).collect()
    }

    #[test]
    fn compare_ui_check_06_unique_counts_and_copy_text() {
        for case in CASES {
            let model = compute(case.a, case.b, CompareMode::Original, NO_IGNORE);
            let shown = |pane: &fuzzy_dedupe_engine::compare::Pane, idx: &[usize]| -> Vec<String> {
                idx.iter()
                    .map(|i| pane.shown().line(*i).expect("line").to_owned())
                    .collect()
            };
            assert_eq!(
                shown(&model.a, &model.only_in_a.lines),
                case.only_a,
                "{}",
                case.name
            );
            assert_eq!(
                shown(&model.b, &model.only_in_b.lines),
                case.only_b,
                "{}",
                case.name
            );
            assert_eq!(
                model.only_in_a.count(),
                case.only_a.len() as u64,
                "{}",
                case.name
            );
            assert_eq!(
                model.copy_only_in_a().text,
                expected_copy(case.only_a),
                "{}",
                case.name
            );
            assert_eq!(
                model.copy_only_in_b().text,
                expected_copy(case.only_b),
                "{}",
                case.name
            );
            assert!(!model.only_in_a.normalized && !model.copy_only_in_a().normalized);
            assert_eq!(!model.hunks.is_empty(), case.diff_changes, "{}", case.name);
            // The measure is the same one Findings and Review use.
            let (ua, ub) = measure_unique(case.a, case.b, CompareMode::Original, NO_IGNORE);
            assert_eq!((ua, ub), (model.only_in_a.clone(), model.only_in_b.clone()));
        }
    }

    #[test]
    fn compare_line_endings_and_final_newline_are_kept_in_the_panes() {
        let model = compute(
            "one\r\ntwo\nthree",
            "one\ntwo\nthree\n",
            CompareMode::Original,
            NO_IGNORE,
        );
        assert_eq!(&*model.a.original.text, "one\r\ntwo\nthree");
        assert_eq!(model.a.original.line_with_terminator(0), Some("one\r\n"));
        assert_eq!(model.a.original.line_with_terminator(2), Some("three"));
        assert!(model.rows.iter().all(|r| r.kind == RowKind::Equal));
    }

    #[test]
    fn compare_normalized_counts_are_labeled_and_apply_the_rules_first() {
        let case = CASES
            .iter()
            .find(|c| c.name == "non-NFC input")
            .expect("case");
        let model = compute(case.a, case.b, CompareMode::Normalized, NO_IGNORE);
        assert_eq!(model.only_in_a.count(), 0);
        assert_eq!(model.only_in_b.count(), 0);
        assert!(model.only_in_a.normalized && model.only_in_b.normalized);
        assert!(model.copy_only_in_a().normalized);

        let ignore = IgnoreOptions {
            letter_case: true,
            punctuation: true,
            whitespace: true,
            line_endings: true,
        };
        let model = compute(
            "Hello,  World!\r\nkeep\n",
            "hello world\nKEEP\nextra\n",
            CompareMode::Normalized,
            ignore,
        );
        assert_eq!(model.only_in_a.count(), 0);
        assert_eq!(model.copy_only_in_b().text, "extra\n");
        assert_eq!(&*model.a.shown().text, "hello world\nkeep\n");
        // The original text, with its CRLF, stays the path back (§11.5).
        assert_eq!(&*model.a.original.text, "Hello,  World!\r\nkeep\n");
    }

    // ── PV-T35-001: side A is the counterpart, side B the target ─────────

    /// Submits `req` and waits for its ready model, then for the review
    /// evidence the worker records just after delivery. The probe is a Keep
    /// both naming the counterpart as its file, which the evidence route
    /// refuses with `EvidenceMismatch` once the record exists and with
    /// `EvidenceUnavailable` before, so polling mutates no review state.
    fn compare_recorded(
        engine: &mut Engine,
        req: ComparisonRequest,
    ) -> (RequestId, AlignedComparison) {
        let id = submit(engine, req);
        let got = await_comparison(engine, id, &mut Vec::new());
        let model = ready(&got).clone();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let probe = engine.review(ReviewCommand::KeepBoth {
                file: req.counterpart,
                counterpart: req.target,
                reviewed: id,
            });
            if !evidence_missing(&probe.status) {
                break;
            }
            assert!(Instant::now() < deadline, "{id:?} never recorded");
            std::thread::sleep(Duration::from_millis(1));
        }
        (id, model)
    }

    /// Every (target, counterpart, Findings Unique of the target) the
    /// acceptance names: each member row of the mixed, unicode and stale
    /// scenarios against its counterpart, and the design scenario's
    /// frame `13:1304` pair, whose target is `last_compared.b`.
    fn orientation_pairs(scenario: &Scenario) -> Vec<(FileId, FileId, u64)> {
        if scenario.kind == ScenarioKind::Design {
            let pair = fixtures::design::sample()
                .last_compared
                .expect("design pair");
            let group = scenario
                .results
                .groups
                .iter()
                .find(|g| g.id == pair.group)
                .expect("the compared group");
            let link = group.member(pair.b).and_then(|m| m.link()).expect("member");
            assert_eq!(link.counterpart, pair.a, "the draft's counterpart is A");
            return vec![(pair.b, pair.a, link.unique_lines)];
        }
        scenario
            .results
            .groups
            .iter()
            .flat_map(|g| g.members.iter())
            .filter_map(|m| m.link().map(|l| (m.file, l.counterpart, l.unique_lines)))
            .collect()
    }

    /// One requested pair of [`orientation_pairs`] with its ready model.
    struct Oriented {
        why: String,
        target: FileId,
        counterpart: FileId,
        findings_unique: u64,
        target_text: String,
        counterpart_text: String,
        model: AlignedComparison,
    }

    /// Requests every [`orientation_pairs`] pair of the design, mixed,
    /// unicode and stale scenarios through the service, as
    /// `(target, counterpart)`, the way Compare and Review unique text ask.
    fn oriented() -> Vec<Oriented> {
        let mut engine = Engine::new(EngineConfig::default()).expect("engine starts");
        let mut out = Vec::new();
        for kind in [
            ScenarioKind::Design,
            ScenarioKind::Mixed,
            ScenarioKind::Unicode,
            ScenarioKind::Stale,
        ] {
            let scenario = fresh_scan(&mut engine, kind);
            let scenario = adopt(&mut engine, scenario);
            for (target, counterpart, findings_unique) in orientation_pairs(&scenario) {
                let req = request_for(&scenario, target, counterpart);
                let id = submit(&mut engine, req);
                let got = await_comparison(&mut engine, id, &mut Vec::new());
                out.push(Oriented {
                    why: format!("{kind:?} target {target:?} counterpart {counterpart:?}"),
                    target,
                    counterpart,
                    findings_unique,
                    target_text: text_of(&scenario, target),
                    counterpart_text: text_of(&scenario, counterpart),
                    model: ready(&got).clone(),
                });
            }
        }
        // Pairs whose two unique counts differ: on those a swapped reading
        // of the sides fails, on a symmetric pair it would pass unnoticed.
        assert!(
            out.iter()
                .any(|o| o.model.only_in_a.count() != o.model.only_in_b.count()),
            "no pair with unequal unique counts"
        );
        out
    }

    #[test]
    fn compare_orientation_side_a_holds_the_counterpart_and_side_b_the_target() {
        for o in oriented() {
            let m = &o.model;
            assert_eq!((m.a.file, m.b.file), (o.counterpart, o.target), "{}", o.why);
            assert_eq!(&*m.a.original.text, o.counterpart_text, "{}", o.why);
            assert_eq!(&*m.b.original.text, o.target_text, "{}", o.why);
        }
    }

    #[test]
    fn compare_orientation_only_in_b_is_the_targets_findings_unique() {
        for o in oriented() {
            assert_eq!(o.model.only_in_b.count(), o.findings_unique, "{}", o.why);
        }
    }

    #[test]
    fn compare_orientation_design_pair_shows_the_reference_as_side_a() {
        let mut engine = Engine::new(EngineConfig::default()).expect("engine starts");
        let scenario = fresh_scan(&mut engine, ScenarioKind::Design);
        let scenario = adopt(&mut engine, scenario);
        let pair = fixtures::design::sample()
            .last_compared
            .expect("design pair");
        let name = |file: FileId| {
            scenario
                .results
                .group_of(file)
                .and_then(|g| g.member(file))
                .expect("member")
                .name
                .clone()
        };
        let req = request_for(&scenario, pair.b, pair.a);
        let id = submit(&mut engine, req);
        let got = await_comparison(&mut engine, id, &mut Vec::new());
        let model = ready(&got);
        assert_eq!(
            (name(model.a.file), name(model.b.file)),
            (
                "Design_Spec.txt".to_owned(),
                "Design_Spec_draft.txt".to_owned()
            )
        );
        assert_ne!(
            model.only_in_a.count(),
            model.only_in_b.count(),
            "the design pair is asymmetric, so a swapped reading fails"
        );
    }

    /// Records a Compare Keep both of `target` against `counterpart` and
    /// returns the Unique value the review service now holds for the
    /// target's row, with the model the review was bound to.
    fn reaim(
        engine: &mut Engine,
        scenario: &Scenario,
        target: FileId,
        counterpart: FileId,
    ) -> (u64, AlignedComparison) {
        let req = request_for(scenario, target, counterpart);
        let (id, model) = compare_recorded(engine, req);
        let outcome = engine.review(ReviewCommand::KeepBoth {
            file: target,
            counterpart,
            reviewed: id,
        });
        assert_eq!(outcome.status, ReviewStatus::Applied, "{outcome:?}");
        let row = outcome
            .changed
            .iter()
            .find(|row| row.file == target)
            .expect("the re-aimed row changed");
        let MemberRole::Member(link) = row.role else {
            panic!("{:?} is not a member", row.role);
        };
        assert_eq!(link.counterpart, counterpart);
        assert_eq!(link.basis, CounterpartBasis::ReviewedInCompare);
        (link.unique_lines, model)
    }

    #[test]
    fn compare_orientation_reaim_without_a_direct_edge_records_only_in_b() {
        let mut engine = Engine::new(EngineConfig::default()).expect("engine starts");
        let scenario = fresh_scan(&mut engine, ScenarioKind::Mixed);
        let scenario = adopt(&mut engine, scenario);
        // The FR-013 chain: Minutes_final.txt against Minutes.txt has no
        // direct edge, so the recorded Unique value can only come from the
        // comparison's evidence.
        let chain = &scenario.results.groups[1];
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
        let (recorded, model) = reaim(&mut engine, &scenario, target, counterpart);
        assert_eq!(recorded, model.only_in_b.count());
        assert_eq!((model.a.file, model.b.file), (counterpart, target));
    }

    /// Every ordered pair of a member row and another member of its group
    /// with no direct edge, the re-aims whose Unique value only the
    /// comparison can supply. Many are asymmetric (the Minutes chain is
    /// not), so a record taken from side A fails here.
    #[test]
    fn compare_orientation_every_edgeless_reaim_records_only_in_b() {
        let mut engine = Engine::new(EngineConfig::default()).expect("engine starts");
        let mut asymmetric = 0;
        for kind in [
            ScenarioKind::Mixed,
            ScenarioKind::Unicode,
            ScenarioKind::Stale,
            ScenarioKind::Design,
        ] {
            let scenario = fresh_scan(&mut engine, kind);
            let scenario = adopt(&mut engine, scenario);
            let mut edgeless = Vec::new();
            for group in &scenario.results.groups {
                for t in group.members.iter().filter(|m| m.link().is_some()) {
                    for c in &group.members {
                        if c.file != t.file && group.edge_between(t.file, c.file).is_none() {
                            edgeless.push((t.file, c.file));
                        }
                    }
                }
            }
            for (target, counterpart) in edgeless {
                let (recorded, model) = reaim(&mut engine, &scenario, target, counterpart);
                let oracle = oracle_unique(
                    &lines(&text_of(&scenario, target)),
                    &lines(&text_of(&scenario, counterpart)),
                )
                .len() as u64;
                let why = format!("{kind:?} target {target:?} counterpart {counterpart:?}");
                assert_eq!(recorded, oracle, "{why}");
                assert_eq!(recorded, model.only_in_b.count(), "{why}");
                if model.only_in_a.count() != model.only_in_b.count() {
                    asymmetric += 1;
                }
            }
        }
        assert!(asymmetric > 0, "no edge-less pair with unequal counts");
    }

    #[test]
    fn compare_orientation_review_commands_name_the_right_pane_as_their_target() {
        let mut engine = Engine::new(EngineConfig::default()).expect("engine starts");
        let scenario = fresh_scan(&mut engine, ScenarioKind::Design);
        let scenario = adopt(&mut engine, scenario);
        let pair = fixtures::design::sample()
            .last_compared
            .expect("design pair");
        // Named by role, not read from the model's sides: the evidence key
        // is (target, counterpart) whatever the orientation, so this case
        // holds on either side order. Frame `13:1304` draws `pair.a`, the
        // counterpart, in the left pane.
        let (left, right) = (pair.a, pair.b);
        let req = request_for(&scenario, right, left);
        let (id, _) = compare_recorded(&mut engine, req);
        let mismatch = |status: &ReviewStatus| {
            matches!(
                status,
                ReviewStatus::Refused {
                    refusal: Refusal::EvidenceMismatch,
                    ..
                }
            )
        };

        // The left-pane file named as the target is the wrong pair.
        for command in [
            ReviewCommand::RequestQueue {
                file: left,
                counterpart: right,
                origin: QueueOrigin::Compare,
                reviewed: Some(id),
            },
            ReviewCommand::KeepBoth {
                file: left,
                counterpart: right,
                reviewed: id,
            },
        ] {
            let status = engine.review(command).status;
            assert!(mismatch(&status), "{status:?}");
        }
        // Queue B and Keep both naming (target, counterpart) resolve: policy
        // answers them, and the evidence route refuses neither.
        for command in [
            ReviewCommand::RequestQueue {
                file: right,
                counterpart: left,
                origin: QueueOrigin::Compare,
                reviewed: Some(id),
            },
            ReviewCommand::KeepBoth {
                file: right,
                counterpart: left,
                reviewed: id,
            },
        ] {
            let status = engine.review(command).status;
            assert!(
                !mismatch(&status) && !evidence_missing(&status),
                "{status:?}"
            );
        }
    }
}
