// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! Deterministic fixture scenarios (plan T4, REQ-020): the result sets MS-1
//! runs on instead of a real scan.
//!
//! A scenario is a C-01 [`ScanResults`] plus the in-memory "files" behind it
//! (plan D-005: no fixture file is ever written or read). The same kind,
//! scan ID and seed always build an equal scenario; the generator uses a
//! hand-rolled `SplitMix64` because no RNG crate is approved (spec §8.3).
//!
//! Every generated scenario satisfies the C-01 invariants listed at the
//! domain crate root, and the engine's `fixtures_contract` suite proves them
//! from the rendered bytes. The generator keeps the evidence honest by
//! construction: each file's content is derived from another's by an edit
//! that matches the edge's relation (copy for Same bytes, a changed line
//! ending for Same text, one replaced line for Similar, a slice or an
//! extension for Containment), and every measurement and Unique count is
//! computed from that content rather than declared.
//!
//! Scenario selection for developers is by name ([`ScenarioKind::from_name`])
//! or kind; [`crate::service::Engine::adopt_scenario`] installs one without
//! running the simulated scan.

pub mod design;
mod scenarios;

use std::collections::BTreeMap;
use std::sync::Arc;

use fuzzy_dedupe_domain::{EvidenceUpdate, FileId, ScanId, ScanResults};

use crate::service::NotImplemented;

/// The seed every caller uses unless it wants a different data set.
pub const DEFAULT_SEED: u64 = 0x00F0_22D0_DEDE_0001;

/// The fixture scenarios.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ScenarioKind {
    /// Exactly 100,000 flattened rows with every group expanded, holding
    /// every relation type, Mixed groups, protected, kept-eligible and
    /// counterpart-chained members.
    Large,
    /// Small groups for FR-013: an identical pair only similar to a
    /// protected reference, a containment pair that excludes the reference,
    /// a chain, and a member whose best direct match is not the reference.
    Mixed,
    /// A complete scan with one discovery problem of every kind (frame 01c).
    Issues,
    /// A scan that found no relationship.
    Empty,
    /// Japanese, Arabic, emoji and decomposed-accent names, a long path and
    /// mixed line endings (MS-1 exit item 7).
    Unicode,
    /// Seeded decisions plus scripted evidence changes that invalidate them,
    /// for the review and cleanup stale-evidence paths (C-05, ERR-001).
    Stale,
    /// The review frames' sample data. Stub until T9.
    Design,
}

impl ScenarioKind {
    pub const ALL: [Self; 7] = [
        Self::Large,
        Self::Mixed,
        Self::Issues,
        Self::Empty,
        Self::Unicode,
        Self::Stale,
        Self::Design,
    ];

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Large => "large",
            Self::Mixed => "mixed",
            Self::Issues => "issues",
            Self::Empty => "empty",
            Self::Unicode => "unicode",
            Self::Stale => "stale",
            Self::Design => "design",
        }
    }

    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.name() == name)
    }
}

/// A review decision a scenario starts with, which the app or a test replays
/// through review commands. Seeds never encode a state C-05 forbids.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SeededDecision {
    Kept(FileId),
    /// A per-file Protect.
    Protect(FileId),
    /// Queued against its current counterpart, acknowledged where review is
    /// required.
    Queue {
        file: FileId,
        counterpart: FileId,
    },
}

/// One built scenario.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scenario {
    pub kind: ScenarioKind,
    pub seed: u64,
    pub results: Arc<ScanResults>,
    pub contents: FixtureContents,
    pub seeded_decisions: Vec<SeededDecision>,
    /// Changes to inject, in order, with `ReviewCommand::UpdateEvidence`.
    /// Empty except in `stale`.
    pub evidence_changes: Vec<EvidenceUpdate>,
}

/// Builds `kind` for scan `scan`.
///
/// # Errors
///
/// [`NotImplemented`] for [`ScenarioKind::Design`] until T9 lands.
pub fn build(kind: ScenarioKind, scan: ScanId, seed: u64) -> Result<Scenario, NotImplemented> {
    match kind {
        ScenarioKind::Large => Ok(scenarios::large(scan, seed)),
        ScenarioKind::Mixed => Ok(scenarios::mixed(scan, seed)),
        ScenarioKind::Issues => Ok(scenarios::issues(scan, seed)),
        ScenarioKind::Empty => Ok(scenarios::empty(scan, seed)),
        ScenarioKind::Unicode => Ok(scenarios::unicode(scan, seed)),
        ScenarioKind::Stale => Ok(scenarios::stale(scan, seed)),
        ScenarioKind::Design => design::build(scan, seed),
    }
}

/// Line terminators of a generated file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LineEnding {
    Lf,
    Crlf,
    /// LF after even-numbered lines, CRLF after odd-numbered ones.
    Mixed,
}

impl LineEnding {
    const fn bytes(self, line: usize) -> &'static [u8] {
        match self {
            Self::Crlf => b"\r\n",
            Self::Mixed if !line.is_multiple_of(2) => b"\r\n",
            Self::Lf | Self::Mixed => b"\n",
        }
    }
}

/// The vocabulary a generated line is drawn from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TextScript {
    Latin,
    /// Japanese, Arabic, emoji and accented text.
    Unicode,
}

/// One fixture file's content.
///
/// `Lines` is compact: a line is a token rendered on demand, so the 100,000
/// rows of `large` cost a few bytes per line rather than a buffer per file.
/// Rendering is injective (each line ends in its token number), which is
/// what makes token equality and §10.1 line-text equality the same thing.
/// `Bytes` holds a literal buffer for hand-written samples such as T9's.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum FileContent {
    Lines {
        tokens: Vec<u32>,
        script: TextScript,
        ending: LineEnding,
        final_newline: bool,
    },
    Bytes(Arc<[u8]>),
}

impl FileContent {
    /// The file's bytes.
    #[must_use]
    pub fn render(&self) -> Vec<u8> {
        match self {
            Self::Bytes(bytes) => bytes.to_vec(),
            Self::Lines {
                tokens,
                script,
                ending,
                final_newline,
            } => {
                let mut out = Vec::with_capacity(self.byte_len());
                for (i, token) in tokens.iter().enumerate() {
                    push_line(&mut out, *script, *token);
                    if i + 1 < tokens.len() || *final_newline {
                        out.extend_from_slice(ending.bytes(i));
                    }
                }
                out
            }
        }
    }

    /// `render().len()` without rendering.
    #[must_use]
    pub fn byte_len(&self) -> usize {
        match self {
            Self::Bytes(bytes) => bytes.len(),
            Self::Lines {
                tokens,
                script,
                ending,
                final_newline,
            } => tokens
                .iter()
                .enumerate()
                .map(|(i, token)| {
                    let terminator = if i + 1 < tokens.len() || *final_newline {
                        ending.bytes(i).len()
                    } else {
                        0
                    };
                    line_len(*script, *token) + terminator
                })
                .sum(),
        }
    }
}

/// The in-memory buffers of one scenario, by file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FixtureContents(BTreeMap<FileId, FileContent>);

impl FixtureContents {
    #[must_use]
    pub fn get(&self, file: FileId) -> Option<&FileContent> {
        self.0.get(&file)
    }

    /// The file's bytes, or `None` for a file the scenario does not hold.
    #[must_use]
    pub fn render(&self, file: FileId) -> Option<Vec<u8>> {
        self.0.get(&file).map(FileContent::render)
    }

    pub fn insert(&mut self, file: FileId, content: FileContent) {
        self.0.insert(file, content);
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

const LATIN_PHRASES: [&str; 12] = [
    "Budget figures for the quarterly review",
    "Action items agreed at the planning meeting",
    "Requirements for the export feature",
    "Notes on the customer interview",
    "Draft wording for the release announcement",
    "Open questions about the storage layout",
    "Summary of the accessibility audit",
    "Steps to reproduce the reported crash",
    "Decisions recorded by the design review",
    "Risks identified during the migration",
    "Glossary entry for normalized text",
    "Checklist for the final acceptance pass",
];

const UNICODE_PHRASES: [&str; 6] = [
    "議事録：次回の打ち合わせの議題",
    "تقرير الاجتماع الأسبوعي",
    "📝 Notizen zur Überprüfung ✅",
    "Résumé des décisions prises",
    "要件定義書の改訂履歴 🔖",
    "ملاحظات حول المراجعة النهائية",
];

fn phrase(script: TextScript, token: u32) -> &'static str {
    // Knuth's multiplicative hash spreads consecutive tokens over the table;
    // the token number appended after the phrase keeps lines distinct.
    let mixed = token.wrapping_mul(2_654_435_761) >> 16;
    match script {
        TextScript::Latin => LATIN_PHRASES[mixed as usize % LATIN_PHRASES.len()],
        TextScript::Unicode => UNICODE_PHRASES[mixed as usize % UNICODE_PHRASES.len()],
    }
}

fn digits(mut n: u32) -> usize {
    let mut count = 1;
    while n >= 10 {
        n /= 10;
        count += 1;
    }
    count
}

/// `"<phrase> #<token>"`: no line terminator inside, and injective in the
/// token, which the generator's token-based Unique counts rely on.
fn push_line(out: &mut Vec<u8>, script: TextScript, token: u32) {
    out.extend_from_slice(phrase(script, token).as_bytes());
    out.extend_from_slice(b" #");
    out.extend_from_slice(token.to_string().as_bytes());
}

fn line_len(script: TextScript, token: u32) -> usize {
    phrase(script, token).len() + 2 + digits(token)
}

/// `SplitMix64` (Steele, Lea and Flood, 2014): tiny, fast, and fully
/// determined by its seed, which is all a fixture generator needs.
#[derive(Clone, Debug)]
pub(crate) struct SplitMix64(u64);

impl SplitMix64 {
    pub(crate) const fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub(crate) const fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform enough in `0..bound` for fixtures; `bound` must be non-zero.
    pub(crate) fn below(&mut self, bound: usize) -> usize {
        // The modulo bias is below 2^-50 for every bound used here.
        usize::try_from(self.next_u64() % bound as u64).unwrap_or(0)
    }

    /// A value in `low..=high`.
    pub(crate) fn between(&mut self, low: usize, high: usize) -> usize {
        low + self.below(high - low + 1)
    }

    pub(crate) fn chance(&mut self, numerator: usize, denominator: usize) -> bool {
        self.below(denominator) < numerator
    }
}
