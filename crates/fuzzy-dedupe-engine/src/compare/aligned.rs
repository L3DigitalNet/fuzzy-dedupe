// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! The aligned-row model of plan C-04 and the computation that builds it.
//!
//! The comparison service passes the counterpart as side A (the left pane,
//! the evidence anchor) and the target as side B (the right pane, the file
//! Queue B names), so [`AlignedComparison::only_in_b`] is the target's
//! §10.1 Unique count, the value Findings shows on the target's row, Review
//! unique text lists and the review evidence carries. This module itself
//! knows only sides A and B; the orientation is `compare::compute`'s.
//!
//! Two measures live here and must not be confused:
//!
//! - The **rows** come from a `similar` line diff (Patience) and decide only
//!   layout: which lines sit side by side, which are spacers and where the
//!   hunks are.
//! - The **unique lines** come from set membership over line text (§10.1)
//!   and never look at the rows. A moved or repeated line is drawn as
//!   removed and inserted, yet is not unique when its text appears anywhere
//!   in the other file. Rejected: counting `OnlyA` rows, which makes reordered
//!   identical lines unique and turns a harmless reorder into "unique text".
//!
//! Line text excludes the terminator in both modes, so the diff and the
//! counts agree with §10.1 that line-ending differences and a missing final
//! newline change nothing. The panes keep the original bytes, terminators
//! included, for display and whole-file copy (IR-006).

use std::collections::HashSet;
use std::convert::Infallible;
use std::ops::Range;
use std::sync::Arc;

use fuzzy_dedupe_domain::{CompareMode, FileId, IgnoreOptions};
use similar::{Algorithm, DiffOp};

use super::normalize::normalize_line;

/// One source line inside a [`PaneText`], as byte offsets into its text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LineSpan {
    pub start: usize,
    /// End of the line text, before any terminator.
    pub content_end: usize,
    /// End of the terminator; equals `content_end` on a last line without a
    /// final newline.
    pub end: usize,
}

/// A text split into lines. Only LF and CRLF terminate a line; a lone CR is
/// line text. A final line without a newline is still a line, and an empty
/// text has none.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaneText {
    pub text: Arc<str>,
    pub lines: Vec<LineSpan>,
}

impl PaneText {
    #[must_use]
    pub fn split(text: Arc<str>) -> Self {
        let lines = split_lines(&text);
        Self { text, lines }
    }

    /// Line `index` (zero-based) without its terminator.
    #[must_use]
    pub fn line(&self, index: usize) -> Option<&str> {
        let span = self.lines.get(index)?;
        self.text.get(span.start..span.content_end)
    }

    /// Line `index` with its original terminator, if it has one.
    #[must_use]
    pub fn line_with_terminator(&self, index: usize) -> Option<&str> {
        let span = self.lines.get(index)?;
        self.text.get(span.start..span.end)
    }

    #[must_use]
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }
}

fn split_lines(text: &str) -> Vec<LineSpan> {
    let bytes = text.as_bytes();
    let mut lines = Vec::new();
    let mut start = 0;
    while start < bytes.len() {
        let (content_end, end) = match bytes[start..].iter().position(|b| *b == b'\n') {
            Some(offset) => {
                let newline = start + offset;
                let content_end = if newline > start && bytes[newline - 1] == b'\r' {
                    newline - 1
                } else {
                    newline
                };
                (content_end, newline + 1)
            }
            None => (bytes.len(), bytes.len()),
        };
        lines.push(LineSpan {
            start,
            content_end,
            end,
        });
        start = end;
    }
    lines
}

/// One pane: the file, its original text and, in Normalized mode, the
/// normalized text line for line.
///
/// Normalization is per line, so `normalized` always has exactly as many
/// lines as `original`, and line `i` of one is line `i` of the other. That
/// is what keeps source line numbers, and the path back to the original
/// differences (§11.5 rule 8), valid in Normalized mode.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pane {
    pub file: FileId,
    pub original: PaneText,
    pub normalized: Option<PaneText>,
}

impl Pane {
    /// The text the rows and spans refer to: normalized in Normalized mode,
    /// original otherwise.
    #[must_use]
    pub fn shown(&self) -> &PaneText {
        self.normalized.as_ref().unwrap_or(&self.original)
    }
}

/// What a row shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RowKind {
    Equal,
    /// A line of A against a spacer.
    OnlyA,
    /// A spacer against a line of B.
    OnlyB,
    /// A line of A paired with a different line of B.
    Changed(Box<InlineSpans>),
}

/// The differing parts of a Changed row, as byte ranges within each side's
/// shown line text (no terminator). Character granularity, so every range
/// falls on a `char` boundary.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InlineSpans {
    pub a: Vec<Range<usize>>,
    pub b: Vec<Range<usize>>,
}

/// One aligned row. `a` and `b` are zero-based line indices into the
/// panes; the displayed source line number is the index plus one. `None`
/// is an alignment spacer, which has no source line number (IR-002).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub a: Option<usize>,
    pub b: Option<usize>,
    pub kind: RowKind,
}

/// A change: a maximal run of non-Equal rows. Change navigation moves both
/// panes to `rows.start`, the anchor both sides share (§11.5 rule 6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hunk {
    pub rows: Range<usize>,
}

/// One side's §10.1 unique lines: zero-based indices in file order, every
/// occurrence listed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UniqueLines {
    pub lines: Vec<usize>,
    /// Measured on normalized text. Presentation must label such a count as
    /// normalized (§10.1, §11.5 rule 3).
    pub normalized: bool,
}

impl UniqueLines {
    /// The Only-in count.
    #[must_use]
    pub fn count(&self) -> u64 {
        self.lines.len() as u64
    }
}

/// Copy text with the mode it was taken in, so the caller can say when it
/// copied normalized text (IR-006).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CopyText {
    pub text: String,
    pub normalized: bool,
}

/// The aligned comparison of side A with side B: from the comparison
/// service, the counterpart (A) with the target (B).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AlignedComparison {
    pub mode: CompareMode,
    pub a: Pane,
    pub b: Pane,
    pub rows: Vec<Row>,
    pub hunks: Vec<Hunk>,
    pub only_in_a: UniqueLines,
    pub only_in_b: UniqueLines,
}

/// The computation stopped because its request was superseded.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Superseded;

impl AlignedComparison {
    /// Aligns `a` against `b` in `mode`; `ignore` applies only in Normalized
    /// mode. Pure and deterministic; the comparison service runs it on its
    /// worker thread, and tests and benchmarks may call it directly.
    #[must_use]
    pub fn compute(
        a: (FileId, Arc<str>),
        b: (FileId, Arc<str>),
        mode: CompareMode,
        ignore: IgnoreOptions,
    ) -> Self {
        let Ok(model) = Self::build(a, b, mode, ignore, || Ok::<(), Infallible>(()));
        model
    }

    /// [`AlignedComparison::compute`] that polls `superseded` between
    /// phases and gives up as soon as it returns true. The diff itself is
    /// one uninterruptible call, so a superseded request can still run for
    /// the length of one diff; its result is then discarded.
    ///
    /// # Errors
    ///
    /// [`Superseded`] when `superseded` returned true.
    pub fn compute_until(
        a: (FileId, Arc<str>),
        b: (FileId, Arc<str>),
        mode: CompareMode,
        ignore: IgnoreOptions,
        superseded: impl Fn() -> bool,
    ) -> Result<Self, Superseded> {
        Self::build(a, b, mode, ignore, || {
            if superseded() {
                Err(Superseded)
            } else {
                Ok(())
            }
        })
    }

    fn build<E>(
        a: (FileId, Arc<str>),
        b: (FileId, Arc<str>),
        mode: CompareMode,
        ignore: IgnoreOptions,
        check: impl Fn() -> Result<(), E>,
    ) -> Result<Self, E> {
        let a = pane(a, mode, ignore);
        let b = pane(b, mode, ignore);
        check()?;
        let a_lines = lines_of(a.shown());
        let b_lines = lines_of(b.shown());
        let only_in_a = unique(&a_lines, &b_lines, mode);
        let only_in_b = unique(&b_lines, &a_lines, mode);
        check()?;
        // Patience anchors on lines that occur once in each file, so a
        // 100,000-line diff with thousands of scattered changes splits into
        // many tiny gaps. Rejected: Myers, whose worst case grows with the
        // length times the number of edits (here about 100,000 x 20,000).
        let ops = similar::capture_diff_slices(Algorithm::Patience, &a_lines, &b_lines);
        check()?;
        let rows = rows(&ops, &a_lines, &b_lines);
        let hunks = hunks(&rows);
        Ok(Self {
            mode,
            a,
            b,
            rows,
            hunks,
            only_in_a,
            only_in_b,
        })
    }

    /// Copy unique lines for side A, the counterpart in a service result:
    /// the unique lines in file order, each followed by one LF (§10.1), in
    /// the shown text of the mode.
    #[must_use]
    pub fn copy_only_in_a(&self) -> CopyText {
        copy(&self.a, &self.only_in_a)
    }

    /// [`AlignedComparison::copy_only_in_a`] for side B, the target in a
    /// service result: the text Review unique text copies.
    #[must_use]
    pub fn copy_only_in_b(&self) -> CopyText {
        copy(&self.b, &self.only_in_b)
    }
}

fn pane((file, text): (FileId, Arc<str>), mode: CompareMode, ignore: IgnoreOptions) -> Pane {
    let original = PaneText::split(text);
    let normalized = match mode {
        CompareMode::Original => None,
        CompareMode::Normalized => Some(normalize_pane(&original, ignore)),
    };
    Pane {
        file,
        original,
        normalized,
    }
}

fn normalize_pane(original: &PaneText, ignore: IgnoreOptions) -> PaneText {
    let mut text = String::with_capacity(original.text.len());
    let mut lines = Vec::with_capacity(original.lines.len());
    for (index, span) in original.lines.iter().enumerate() {
        let start = text.len();
        text.push_str(&normalize_line(original.line(index).unwrap_or(""), ignore));
        let content_end = text.len();
        if span.end > span.content_end {
            if ignore.line_endings {
                text.push('\n');
            } else {
                text.push_str(
                    original
                        .text
                        .get(span.content_end..span.end)
                        .unwrap_or("\n"),
                );
            }
        }
        lines.push(LineSpan {
            start,
            content_end,
            end: text.len(),
        });
    }
    PaneText {
        text: text.into(),
        lines,
    }
}

fn lines_of(text: &PaneText) -> Vec<&str> {
    (0..text.line_count())
        .map(|i| text.line(i).unwrap_or(""))
        .collect()
}

/// The §10.1 Unique measure of `a` against `b` and of `b` against `a`, in
/// `mode` (`ignore` applies only in Normalized mode), without computing a
/// diff. This is the one measure behind Findings' Unique column, Compare's
/// Only-in counts and Review unique text; [`AlignedComparison`] carries the
/// same values.
#[must_use]
pub fn measure_unique(
    a: &str,
    b: &str,
    mode: CompareMode,
    ignore: IgnoreOptions,
) -> (UniqueLines, UniqueLines) {
    let a = pane((FileId::new(0), Arc::from(a)), mode, ignore);
    let b = pane((FileId::new(0), Arc::from(b)), mode, ignore);
    let (a_lines, b_lines) = (lines_of(a.shown()), lines_of(b.shown()));
    (
        unique(&a_lines, &b_lines, mode),
        unique(&b_lines, &a_lines, mode),
    )
}

/// The §10.1 measure: every line of `x` whose text is absent from `y`.
fn unique(x: &[&str], y: &[&str], mode: CompareMode) -> UniqueLines {
    let present: HashSet<&str> = y.iter().copied().collect();
    UniqueLines {
        lines: x
            .iter()
            .enumerate()
            .filter(|(_, line)| !present.contains(*line))
            .map(|(index, _)| index)
            .collect(),
        normalized: mode == CompareMode::Normalized,
    }
}

fn rows(ops: &[DiffOp], a: &[&str], b: &[&str]) -> Vec<Row> {
    let mut rows = Vec::with_capacity(a.len().max(b.len()));
    for op in ops {
        let (_, old, new) = op.as_tag_tuple();
        // A Replace pairs its lines in order as Changed rows; the longer
        // side's excess becomes one-sided rows after them. Equal, Delete
        // and Insert are the degenerate cases of the same rule.
        let paired = if matches!(op, DiffOp::Equal { .. }) {
            None
        } else {
            Some(old.len().min(new.len()))
        };
        match paired {
            None => rows.extend(old.zip(new).map(|(i, j)| Row {
                a: Some(i),
                b: Some(j),
                kind: RowKind::Equal,
            })),
            Some(paired) => {
                for k in 0..paired {
                    let (i, j) = (old.start + k, new.start + k);
                    rows.push(Row {
                        a: Some(i),
                        b: Some(j),
                        kind: RowKind::Changed(Box::new(inline(a[i], b[j]))),
                    });
                }
                rows.extend(old.skip(paired).map(|i| Row {
                    a: Some(i),
                    b: None,
                    kind: RowKind::OnlyA,
                }));
                rows.extend(new.skip(paired).map(|j| Row {
                    a: None,
                    b: Some(j),
                    kind: RowKind::OnlyB,
                }));
            }
        }
    }
    rows
}

/// Character-level differences of one Changed pair.
fn inline(a: &str, b: &str) -> InlineSpans {
    let (a_chars, a_at) = chars(a);
    let (b_chars, b_at) = chars(b);
    let mut spans = InlineSpans::default();
    for op in similar::capture_diff_slices(Algorithm::Myers, &a_chars, &b_chars) {
        let (_, old, new) = op.as_tag_tuple();
        if matches!(op, DiffOp::Equal { .. }) {
            continue;
        }
        if !old.is_empty() {
            spans.a.push(a_at[old.start]..a_at[old.end]);
        }
        if !new.is_empty() {
            spans.b.push(b_at[new.start]..b_at[new.end]);
        }
    }
    spans
}

/// The characters of `s` and their byte offsets, plus one end offset so
/// that `at[n]` is valid for every exclusive range end `n`.
fn chars(s: &str) -> (Vec<char>, Vec<usize>) {
    let mut at: Vec<usize> = s.char_indices().map(|(i, _)| i).collect();
    at.push(s.len());
    (s.chars().collect(), at)
}

fn hunks(rows: &[Row]) -> Vec<Hunk> {
    let mut hunks = Vec::new();
    let mut start = None;
    for (index, row) in rows.iter().enumerate() {
        match (row.kind == RowKind::Equal, start) {
            (false, None) => start = Some(index),
            (true, Some(s)) => {
                hunks.push(Hunk { rows: s..index });
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        hunks.push(Hunk {
            rows: s..rows.len(),
        });
    }
    hunks
}

fn copy(pane: &Pane, unique: &UniqueLines) -> CopyText {
    let shown = pane.shown();
    let mut text = String::new();
    for index in &unique.lines {
        text.push_str(shown.line(*index).unwrap_or(""));
        text.push('\n');
    }
    CopyText {
        text,
        normalized: unique.normalized,
    }
}
