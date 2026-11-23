// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! Generated comparison texts (plan T6, REQ-021): the 5 MB pair behind the
//! MS-1 Compare performance target T2, and the small unique-line pairs of
//! spec §17.3 UI check 06.
//!
//! Everything is built in memory (plan D-005) and is a pure function of the
//! seed. The generator uses its own copy of the fixtures' `SplitMix64`
//! rather than importing it, because `fixtures/` belongs to T4 and stays
//! frozen (plan D-004).
//!
//! The large pair carries the edit script it was built from, which is the
//! independent oracle for the diff: every line of either text is distinct
//! from every other line, except a kept line and its copy, so the only
//! longest common subsequence is the kept lines and any correct line diff
//! must reproduce the script exactly. Hunks are separated by at least one
//! kept line, so each hunk is one diff operation and a Replace pairs its
//! lines one to one.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use fuzzy_dedupe_domain::{
    CompareMode, CounterpartBasis, CounterpartLink, Coverage, FileId, Group, GroupId,
    IgnoreOptions, MatchEdge, MatchMode, MatchingRules, Measurement, Member, MemberRole, Permille,
    RulesRevision, ScanFolder, ScanId, ScanResults, ScanSetup, SnapshotId,
};

use super::aligned::measure_unique;
use crate::fixtures::{FileContent, FixtureContents, Scenario, ScenarioKind};

/// The large pair scenario's rules. They match the fixtures' own rules
/// (which `fixtures/` keeps private): fuzzy matching at 80% and Ignore
/// line-ending differences only.
const LARGE_PAIR_RULES: MatchingRules = MatchingRules {
    mode: MatchMode::ExactAndFuzzy,
    minimum_similarity: match Permille::new(800) {
        Some(p) => p,
        None => Permille::FULL,
    },
    ignore: IgnoreOptions {
        whitespace: false,
        line_endings: true,
        letter_case: false,
        punctuation: false,
    },
    revision: RulesRevision::new(1),
};

/// No Ignore option: the Original-mode Unique counts need none.
const IGNORE_NOTHING: IgnoreOptions = IgnoreOptions {
    whitespace: false,
    line_endings: false,
    letter_case: false,
    punctuation: false,
};

/// Lines in each large text.
pub const LARGE_LINES: usize = 100_000;
/// Target size of each large text in bytes: 5 MB, decimal.
pub const LARGE_BYTES: usize = 5_000_000;
/// Target lines replaced one for one; part of both texts' changed lines.
const REPLACED: usize = 8_000;
/// Target lines only in the first text.
const DELETED: usize = 2_000;
/// Target lines only in the second text. Equal to `DELETED`, so both texts
/// have `LARGE_LINES` lines and a tenth of each is changed.
const INSERTED: usize = 2_000;
/// Largest hunk, in lines on its longer side.
const MAX_HUNK: usize = 8;

/// One run of a large pair's edit script, from the first text (A) to the
/// second (B).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EditOp {
    /// Lines copied unchanged.
    Keep(usize),
    /// Lines of A each replaced by one new line of B.
    Replace(usize),
    /// Lines only in A.
    Delete(usize),
    /// Lines only in B.
    Insert(usize),
}

/// The two generated 5 MB texts and the script that turns `a` into `b`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LargePair {
    pub a: Arc<str>,
    pub b: Arc<str>,
    pub script: Vec<EditOp>,
}

impl LargePair {
    /// Lines of A that are not kept: replaced or deleted.
    #[must_use]
    pub fn changed_in_a(&self) -> usize {
        self.script
            .iter()
            .map(|op| match op {
                EditOp::Replace(n) | EditOp::Delete(n) => *n,
                EditOp::Keep(_) | EditOp::Insert(_) => 0,
            })
            .sum()
    }

    /// Lines of B that are not kept: replacements or insertions.
    #[must_use]
    pub fn changed_in_b(&self) -> usize {
        self.script
            .iter()
            .map(|op| match op {
                EditOp::Replace(n) | EditOp::Insert(n) => *n,
                EditOp::Keep(_) | EditOp::Delete(_) => 0,
            })
            .sum()
    }
}

/// A copy of the fixtures' `SplitMix64` (see the module header for why it
/// is a copy). The constants are the published ones; changing them changes
/// every generated text.
struct SplitMix64(u64);

impl SplitMix64 {
    const fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A value in `low..=high`. The modulo bias is below 2^-40 for the
    /// small ranges used here.
    fn between(&mut self, low: usize, high: usize) -> usize {
        let span = (high - low + 1) as u64;
        low + usize::try_from(self.next_u64() % span).unwrap_or(0)
    }
}

const WORDS: [&str; 32] = [
    "review",
    "budget",
    "draft",
    "notes",
    "meeting",
    "release",
    "storage",
    "export",
    "summary",
    "audit",
    "planning",
    "customer",
    "feature",
    "decision",
    "migration",
    "risk",
    "glossary",
    "checklist",
    "quarterly",
    "action",
    "item",
    "open",
    "question",
    "layout",
    "final",
    "acceptance",
    "pass",
    "wording",
    "announcement",
    "interview",
    "design",
    "steps",
];

/// One generated line, without terminator. The suffix makes it distinct:
/// `#n` for the n-th line of A and `~n` for the n-th new line of B, so no
/// new line can equal any line of A.
fn line(rng: &mut SplitMix64, marker: char, id: usize) -> String {
    // Lines of 19 to 58 text bytes average about 50 bytes with suffix and
    // LF, which puts 100,000 lines at the 5 MB target.
    let target = rng.between(19, 58);
    let mut out = String::with_capacity(target + 12);
    while out.len() < target {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(WORDS[rng.between(0, WORDS.len() - 1)]);
    }
    out.push(' ');
    out.push(marker);
    out.push_str(&id.to_string());
    out
}

/// The hunks of a script in order, before kept lines are placed.
fn hunks(rng: &mut SplitMix64) -> Vec<EditOp> {
    let (mut replace, mut delete, mut insert) = (REPLACED, DELETED, INSERTED);
    let mut hunks = Vec::new();
    while replace + delete + insert > 0 {
        // Weighted by what is left, so all three budgets run out together.
        let pick = rng.between(1, replace + delete + insert);
        let size = rng.between(1, MAX_HUNK);
        let hunk = if pick <= replace {
            let n = size.min(replace);
            replace -= n;
            EditOp::Replace(n)
        } else if pick <= replace + delete {
            let n = size.min(delete);
            delete -= n;
            EditOp::Delete(n)
        } else {
            let n = size.min(insert);
            insert -= n;
            EditOp::Insert(n)
        };
        hunks.push(hunk);
    }
    hunks
}

/// The edit script: the hunks with kept runs between them, at least one
/// kept line between neighbours (see the module header).
fn script(rng: &mut SplitMix64) -> Vec<EditOp> {
    let hunks = hunks(rng);
    let kept = LARGE_LINES - REPLACED - DELETED;
    let slots = hunks.len() + 1;
    let spare = kept - (hunks.len() - 1);
    let weights: Vec<usize> = (0..slots).map(|_| rng.between(1, 1000)).collect();
    let total: usize = weights.iter().sum();
    let mut gaps: Vec<usize> = weights.iter().map(|w| spare * w / total).collect();
    let placed: usize = gaps.iter().sum();
    gaps[slots - 1] += spare - placed;
    for gap in &mut gaps[1..slots - 1] {
        *gap += 1;
    }
    let mut script = Vec::with_capacity(slots * 2);
    for (gap, hunk) in gaps.iter().zip(hunks.iter().map(Some).chain([None])) {
        if *gap > 0 {
            script.push(EditOp::Keep(*gap));
        }
        if let Some(hunk) = hunk {
            script.push(*hunk);
        }
    }
    script
}

/// The two 5 MB texts for `seed`, LF-terminated.
#[must_use]
pub fn large_pair(seed: u64) -> LargePair {
    let mut rng = SplitMix64(seed);
    let script = script(&mut rng);
    let mut a = String::with_capacity(LARGE_BYTES + LARGE_BYTES / 10);
    let mut b = String::with_capacity(LARGE_BYTES + LARGE_BYTES / 10);
    let (mut next_a, mut next_b) = (0, 0);
    let push = |out: &mut String, text: &str| {
        out.push_str(text);
        out.push('\n');
    };
    for op in &script {
        match *op {
            EditOp::Keep(n) => {
                for _ in 0..n {
                    let text = line(&mut rng, '#', next_a);
                    next_a += 1;
                    push(&mut a, &text);
                    push(&mut b, &text);
                }
            }
            EditOp::Replace(n) | EditOp::Delete(n) | EditOp::Insert(n) => {
                if !matches!(op, EditOp::Insert(_)) {
                    for _ in 0..n {
                        let text = line(&mut rng, '#', next_a);
                        next_a += 1;
                        push(&mut a, &text);
                    }
                }
                if !matches!(op, EditOp::Delete(_)) {
                    for _ in 0..n {
                        let text = line(&mut rng, '~', next_b);
                        next_b += 1;
                        push(&mut b, &text);
                    }
                }
            }
        }
    }
    LargePair {
        a: a.into(),
        b: b.into(),
        script,
    }
}

/// A scenario holding the large pair as one Similar group, A the reference
/// and B its member, for `Engine::adopt_scenario` and `LoadComparison`.
/// File IDs are 1 (A) and 2 (B); snapshots equal file IDs.
///
/// Its `kind` is [`ScenarioKind::Large`] for want of a dedicated variant:
/// the C-01 enum belongs to T4 and is frozen (plan D-004). Callers identify
/// this scenario by its two files, never by `kind`; `fixtures::build` never
/// returns it.
#[must_use]
pub fn large_pair_scenario(scan: ScanId, seed: u64) -> Scenario {
    let pair = large_pair(seed);
    let (a, b) = (FileId::new(1), FileId::new(2));
    let (unique_a, unique_b) =
        measure_unique(&pair.a, &pair.b, CompareMode::Original, IGNORE_NOTHING);
    // Jaccard resemblance of the distinct lines, rounded down, as the
    // fixtures measure Similar: every kept line is shared, nothing else is.
    let kept = LARGE_LINES - pair.changed_in_a();
    let union = 2 * LARGE_LINES - kept;
    let similarity = u16::try_from(kept * 1000 / union).unwrap_or(1000);
    let measurement = Measurement::Similar(Permille::new(similarity).unwrap_or(Permille::FULL));
    let edge = MatchEdge {
        a,
        b,
        measurement,
        unique_lines_a: unique_a.count(),
        unique_lines_b: unique_b.count(),
    };
    let folder = "/home/fixture/Large texts";
    let member = |file: FileId, name: &str, size: usize, role| Member {
        file,
        name: name.to_owned(),
        folder: folder.to_owned(),
        display_path: format!("{folder}/{name}"),
        size_bytes: size as u64,
        modified: SystemTime::UNIX_EPOCH,
        folder_protected: false,
        snapshot: SnapshotId::new(file.get()),
        role,
    };
    let members = vec![
        member(a, "Large_A.txt", pair.a.len(), MemberRole::Reference),
        member(
            b,
            "Large_B.txt",
            pair.b.len(),
            MemberRole::Member(CounterpartLink {
                counterpart: a,
                basis: CounterpartBasis::Rule,
                measurement: Some(measurement.mirrored()),
                unique_lines: unique_b.count(),
            }),
        ),
    ];
    let mut contents = FixtureContents::default();
    contents.insert(a, FileContent::Bytes(Arc::from(pair.a.as_bytes())));
    contents.insert(b, FileContent::Bytes(Arc::from(pair.b.as_bytes())));
    Scenario {
        kind: ScenarioKind::Large,
        seed,
        results: Arc::new(ScanResults {
            scan,
            setup: ScanSetup {
                folders: vec![ScanFolder {
                    path: PathBuf::from(folder),
                    protect: false,
                }],
                file_types: vec!["txt".to_owned()],
            },
            rules: LARGE_PAIR_RULES,
            groups: vec![Group {
                id: GroupId::new(1),
                label: "G001".to_owned(),
                members,
                edges: vec![edge],
            }],
            issues: Vec::new(),
            coverage: Coverage::Complete,
        }),
        contents,
        seeded_decisions: Vec::new(),
        evidence_changes: Vec::new(),
    }
}
