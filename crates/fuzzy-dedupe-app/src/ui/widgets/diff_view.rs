// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! The paired, aligned, read-only diff view (spec §11.5, IR-002, IR-006):
//! two panes over one C-04 aligned-row model, with wrap, linked scroll,
//! change navigation and model-backed per-pane selection and copy. Compare
//! (`ui/compare.rs`, plan T15) hosts it below its change row; [`show`] is
//! the only entry point.
//!
//! What the host owns and what this widget owns:
//!
//! - The host draws the toolbar, evidence and change rows and binds the
//!   Compare chords of spec §11.10 except the four below; it moves between
//!   changes by setting [`CompareState::active_change`] and toggles
//!   `wrap`, `linked_scroll` and `mode`. This widget never writes those.
//! - This widget binds Ctrl+A, copy (egui's `Event::Copy`, and Ctrl+C),
//!   F6 and Shift+arrows, plus plain Up, Down, Page Up and Page Down for
//!   scrolling. It matches modifiers exactly, so Alt+Up and Alt+Down stay
//!   in the input for the host. It writes only `selection` and
//!   `split_ratio`, and puts copied text on egui's clipboard output, the
//!   same output `commands::dispatch` uses for `UiIntent::CopyText`.
//! - The host draws each pane's pinned file identity directly above the
//!   pane (it needs member names and sizes, which `CompareState` does not
//!   carry) and the C-04 Only-in counts, in its evidence-row menus. This
//!   widget draws no caption, so the whole pane is text (frame `13:1304`,
//!   spec §11.3's 744-high text area); [`show`] returns each pane's
//!   Normalized and copy notices for the host to show in that identity.
//!
//! Alignment: both panes draw the same aligned rows with the same row
//! heights, the taller of the pair's wrapped heights, so row `r` has one
//! top edge in both panes. Linked scrolling gives both panes one offset in
//! that shared row space, which pins them to the same row anchor.
//! Rejected: two independent text layouts kept together by applying one
//! raw pixel offset to both, which drifts apart after the first one-sided
//! insertion (§11.5 rule 4); `tests/diff_view.rs` keeps that negative
//! control.
//!
//! Virtualization: only the rows in view plus `OVERSCAN` are laid out.
//! Row tops are a prefix sum cached per comparison, wrap width and zoom
//! (`Layout`); per-line character counts and the row-to-line maps are
//! cached per comparison (`Index`). The comparison's key carries the file
//! snapshots, rules revision and view mode, so together these are the
//! §11.5 rule 5 cache key. Wrapping is by character count on
//! the monospace Document font, which is what makes the tops computable
//! without laying out 100,000 lines. A glyph wider than the font's advance
//! (an emoji or a CJK character) can therefore overflow the wrap width and
//! is clipped at the pane edge; its row height stays correct.
//!
//! Selection: a [`TextPosition`] is an aligned row and a character column
//! in that row's line on the pane's side. A position on a spacer row means
//! the start of the side's next line, and row `rows.len()` means the end
//! of the text, so every position maps to one byte offset in the shown
//! text and a selection is one contiguous byte range. Copying that range
//! returns the original bytes, terminators included, with no line numbers,
//! markers or spacers; in Normalized mode it is the normalized text, and
//! the pane's notice says so.
//!
//! Cross-file contract with `tests/diff_view.rs`: each laid-out row of each
//! pane is one accessibility node with author ID `diff_view:a:<row>` or
//! `diff_view:b:<row>`, labeled with the displayed source line number (no
//! label on a spacer), whose bounds are the row's text cell starting at the
//! first character column. [`only_in_caption`], [`NORMALIZED_NOTICE`] and
//! [`copied_notice`] are the strings the tests look for; the first is
//! drawn by Compare's evidence-row menus, the other two are returned by
//! [`show`].
//!
//! Requirements: `theme::install` has run on the context. The loading and
//! unavailable messages use the compact text styles by name, and egui
//! panics on an unknown style.

use std::ops::Range;
use std::sync::Arc;

use egui::text::{CCursor, LayoutJob, TextFormat};
use egui::{
    Align2, Color32, CursorIcon, Event, EventFilter, Galley, Id, Key, Pos2, Rect, Sense, Stroke,
    StrokeKind, Ui, UiBuilder, Vec2, WidgetInfo,
};
use fuzzy_dedupe_domain::CompareMode;
use fuzzy_dedupe_engine::compare::{
    AlignedComparison, Comparison, ComparisonOutcome, ComparisonRequest, Pane, PaneText, Row,
    RowKind, Unavailable,
};

use crate::presentation::state::{CompareState, PaneSelection, TextPosition};
use crate::ui::theme::{self, CompactStyle, Palette, Token};
use crate::ui::widgets::dialog_host::mode_of;

/// Each pane's notice in Normalized mode (IR-006).
pub const NORMALIZED_NOTICE: &str = "Normalized view: copy gives normalized text";

/// The coloured bar at a changed row's leading edge (frame `25:1082`).
const SIDE_BAR: f32 = 3.0;
/// The line-number gutter's reference width; it grows for six-digit files.
const MIN_GUTTER: f32 = 37.0;
const GUTTER_PAD: f32 = 6.0;
/// The A or B marker column (frame `25:1085`).
const MARKER: f32 = 16.0;
const SCROLLBAR: f32 = 8.0;
const TEXT_PAD: f32 = 4.0;
const PANE_GAP: f32 = 8.0;
/// Rows laid out beyond each edge of the view.
const OVERSCAN: usize = 2;
/// Rows shown above a change navigation lands on, for context.
const CONTEXT_ROWS: usize = 3;
/// Auto-scroll speed while a drag is past the view's edge: units per
/// second for each unit the pointer is beyond the edge.
const AUTO_SCROLL: f32 = 12.0;
/// Split bounds: neither pane narrower than a fifth of the area.
const SPLIT_RANGE: (f32, f32) = (0.2, 0.8);

/// The text of a side's Only-in count, `letter` being "A" or "B", which
/// Compare's "Only in A/B ▾" menus show.
#[must_use]
pub fn only_in_caption(letter: &str, count: u64, normalized: bool) -> String {
    let unit = if count == 1 { "line" } else { "lines" };
    let suffix = if normalized { " (normalized)" } else { "" };
    format!("Only in {letter}: {count} {unit}{suffix}")
}

/// Pane `letter`'s notice after a Normalized-mode copy from it.
#[must_use]
pub fn copied_notice(letter: &str) -> String {
    format!("Copied normalized text from {letter}")
}

/// What each pane's text notice says this frame, side A first, `None`
/// where there is nothing to say (IR-006).
///
/// The widget draws no caption: the host shows each notice in that pane's
/// pinned identity header (`ui/compare.rs`), so the whole pane is text.
pub type PaneNotices = [Option<String>; 2];

/// Draws both panes into the remaining Compare area and returns their
/// notices: [`NORMALIZED_NOTICE`] on each pane in Normalized mode, and
/// [`copied_notice`] instead on the pane a Normalized-mode copy came from,
/// until its selection changes. A copy made this frame is already
/// reflected. Nothing is ready (loading, unavailable, no pair): no notices.
///
/// `input_enabled` is false while a popup or menu is open (spec §11.6):
/// the panes then react to no key, pointer or wheel input.
pub fn show(ui: &mut Ui, compare: &mut CompareState, input_enabled: bool) -> PaneNotices {
    let area = ui.available_rect_before_wrap();
    ui.allocate_rect(area, Sense::hover());
    let Some(result) = current(compare) else {
        message(ui, area, compare);
        return PaneNotices::default();
    };
    let ComparisonOutcome::Ready(aligned) = &result.outcome else {
        message(ui, area, compare);
        return PaneNotices::default();
    };
    let id = Id::unique("fuzzy_dedupe.diff_view");
    let mut memory: Memory = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    memory.adopt(compare, &result);
    let Some(index) = memory.index.clone() else {
        return PaneNotices::default();
    };
    let view = View::new(ui, &index, compare, area);
    memory.relayout(&view, compare.wrap, ui.ctx().pixels_per_point());
    let Some(layout) = memory.layout.clone() else {
        return PaneNotices::default();
    };
    let frame = Frame {
        aligned,
        index: &index,
        layout: &layout,
        view: &view,
    };
    memory.follow_links(compare, &frame);
    let mut copy_from = None;
    if input_enabled {
        copy_from = frame.input(ui, compare, &mut memory);
    }
    memory.clamp(&frame);
    if let Some(side) = copy_from {
        frame.copy(ui, compare, &mut memory, side);
    }
    for side in Side::BOTH {
        frame.paint_pane(ui, compare, &memory, side);
    }
    let notices = Side::BOTH.map(|side| notice(aligned, &memory, side));
    ui.data_mut(|d| d.insert_temp(id, memory));
    notices
}

/// One pane's notice: the copy notice outranks the mode notice, because it
/// is the more specific statement about what the clipboard now holds.
fn notice(aligned: &AlignedComparison, memory: &Memory, side: Side) -> Option<String> {
    if memory.copied_normalized == Some(side.index()) {
        Some(copied_notice(side.letter()))
    } else {
        (aligned.mode == CompareMode::Normalized).then(|| NORMALIZED_NOTICE.to_owned())
    }
}

/// The ready comparison for Compare's pair and mode, or `None` while it is
/// pending, unavailable or for another pair. The slot may briefly hold
/// Review unique text's pair (`ComparisonSlot`); that is never drawn here.
fn current(compare: &CompareState) -> Option<Arc<Comparison>> {
    let slot = &compare.comparison;
    let key = slot.key.as_ref()?;
    let pair = compare.pair?;
    let fits = key.target == pair.target
        && key.counterpart == pair.counterpart
        && key.mode == compare.mode
        && compare
            .target_snapshot
            .is_none_or(|s| s == key.target_snapshot)
        && compare
            .counterpart_snapshot
            .is_none_or(|s| s == key.counterpart_snapshot);
    if !fits {
        return None;
    }
    let result = slot.result.as_ref()?;
    (result.request == *key).then(|| Arc::clone(result))
}

/// States why nothing is drawn, so a blank area is never mistaken for two
/// equal files (§11.5 rule 9). No pair at all draws nothing: the host shows
/// its own disabled state then.
fn message(ui: &mut Ui, area: Rect, compare: &CompareState) {
    if compare.pair.is_none() {
        return;
    }
    let text = match compare.comparison.unavailable() {
        Some(why) if current_key_fits(compare) => unavailable_text(why),
        _ => "Loading the comparison…",
    };
    let palette = theme::palette(mode_of(ui.ctx()));
    ui.scope_builder(UiBuilder::new().max_rect(area), |ui| {
        ui.centered_and_justified(|ui| {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(text)
                        .text_style(CompactStyle::Body.text_style())
                        .color(palette.color(Token::Muted)),
                )
                .selectable(false),
            );
        });
    });
}

fn current_key_fits(compare: &CompareState) -> bool {
    let (Some(key), Some(pair)) = (compare.comparison.key, compare.pair) else {
        return false;
    };
    key.target == pair.target && key.counterpart == pair.counterpart && key.mode == compare.mode
}

const fn unavailable_text(why: Unavailable) -> &'static str {
    match why {
        Unavailable::NoScenario => "No results are loaded, so there is nothing to compare.",
        Unavailable::ScanMismatch { .. } | Unavailable::RulesMismatch { .. } => {
            "The results changed since this pair was opened. Open the comparison again."
        }
        Unavailable::FileMissing(_) => "The text of one of these files is not available.",
        Unavailable::SnapshotMismatch { .. } => {
            "One of these files changed since it was compared. Open the comparison again."
        }
        Unavailable::NotText(_) => "One of these files is not text, so it cannot be shown.",
        Unavailable::WorkerUnavailable => "The comparison could not be started.",
    }
}

// ── Model index ──────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Side {
    A,
    B,
}

impl Side {
    const BOTH: [Self; 2] = [Self::A, Self::B];

    const fn index(self) -> usize {
        match self {
            Self::A => 0,
            Self::B => 1,
        }
    }

    const fn other(self) -> Self {
        match self {
            Self::A => Self::B,
            Self::B => Self::A,
        }
    }

    const fn letter(self) -> &'static str {
        match self {
            Self::A => "A",
            Self::B => "B",
        }
    }

    const fn author(self) -> &'static str {
        match self {
            Self::A => "a",
            Self::B => "b",
        }
    }

    const fn line(self, row: &Row) -> Option<usize> {
        match self {
            Self::A => row.a,
            Self::B => row.b,
        }
    }

    fn pane(self, aligned: &AlignedComparison) -> &Pane {
        match self {
            Self::A => &aligned.a,
            Self::B => &aligned.b,
        }
    }

    const fn tokens(self) -> (Token, Token, Token) {
        match self {
            Self::A => (Token::DiffA, Token::DiffASoft, Token::DiffAStrong),
            Self::B => (Token::DiffB, Token::DiffBSoft, Token::DiffBStrong),
        }
    }
}

/// Per-comparison lookups, built once when a result arrives. Indexed by
/// [`Side::index`].
#[derive(Debug)]
struct Index {
    /// Kept so the cache is tied to this exact result, never to a freed
    /// one at the same address.
    result: Arc<Comparison>,
    /// `prefix[s][r]` is the number of side-`s` lines in `rows[..r]`, so it
    /// is also the line a spacer at row `r` resolves to: lines appear in
    /// file order down the rows.
    prefix: [Vec<u32>; 2],
    /// The row each line sits on.
    row_of_line: [Vec<u32>; 2],
    /// Characters per shown line, terminator excluded.
    chars: [Vec<u32>; 2],
    max_chars: [u32; 2],
    /// Per hunk: its first row and whether it holds lines of A and of B.
    marks: Vec<(usize, bool, bool)>,
}

fn u32_of(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

fn usize_of(n: u32) -> usize {
    usize::try_from(n).unwrap_or(usize::MAX)
}

impl Index {
    fn build(result: Arc<Comparison>, aligned: &AlignedComparison) -> Self {
        let rows = &aligned.rows;
        let mut prefix = [
            Vec::with_capacity(rows.len() + 1),
            Vec::with_capacity(rows.len() + 1),
        ];
        let mut row_of_line = [Vec::new(), Vec::new()];
        let mut chars = [Vec::new(), Vec::new()];
        let mut max_chars = [0, 0];
        for side in Side::BOTH {
            let s = side.index();
            let shown = side.pane(aligned).shown();
            let mut count = 0u32;
            for (r, row) in rows.iter().enumerate() {
                prefix[s].push(count);
                if side.line(row).is_some() {
                    row_of_line[s].push(u32_of(r));
                    count += 1;
                }
            }
            prefix[s].push(count);
            chars[s] = (0..shown.line_count())
                .map(|i| u32_of(shown.line(i).map_or(0, |l| l.chars().count())))
                .collect();
            max_chars[s] = chars[s].iter().copied().max().unwrap_or(0);
        }
        let marks = aligned
            .hunks
            .iter()
            .map(|hunk| {
                let span = &rows[hunk.rows.clone()];
                (
                    hunk.rows.start,
                    span.iter().any(|row| row.a.is_some()),
                    span.iter().any(|row| row.b.is_some()),
                )
            })
            .collect();
        Self {
            result,
            prefix,
            row_of_line,
            chars,
            max_chars,
            marks,
        }
    }

    fn rows(&self) -> usize {
        self.prefix[0].len() - 1
    }

    fn line_count(&self, side: Side) -> usize {
        self.chars[side.index()].len()
    }

    /// The line a position on `row` refers to: the row's own line, or for
    /// a spacer the side's next line. `None` past the last line.
    fn line_at(&self, side: Side, row: usize) -> Option<usize> {
        let line = usize_of(*self.prefix[side.index()].get(row)?);
        (line < self.line_count(side)).then_some(line)
    }

    fn row_of(&self, side: Side, line: usize) -> usize {
        usize_of(self.row_of_line[side.index()][line])
    }

    fn chars_of(&self, side: Side, line: usize) -> usize {
        usize_of(self.chars[side.index()][line])
    }
}

// ── Layout cache ─────────────────────────────────────────────────────────

/// Row tops in aligned space, shared by both panes. Rebuilt when the
/// comparison, the wrap columns or the zoom changes (§11.5 rule 5).
#[derive(Debug)]
struct Layout {
    result: Arc<Comparison>,
    /// Wrap width in characters per pane; `None` with wrap off.
    cols: Option<[usize; 2]>,
    pixels_per_point: f32,
    /// `tops[r]` for `r` in `0..=rows`; `None` when every row is one line
    /// tall and the tops are `r * row height`.
    tops: Option<Vec<f64>>,
    rows: usize,
    row_height: f64,
}

impl Layout {
    fn build(
        index: &Index,
        aligned: &AlignedComparison,
        cols: Option<[usize; 2]>,
        ppp: f32,
    ) -> Self {
        let row_height = f64::from(CompactStyle::Document.line_height());
        let rows = index.rows();
        let tops = cols.map(|cols| {
            let mut tops = Vec::with_capacity(rows + 1);
            let mut top = 0.0;
            for row in &aligned.rows {
                tops.push(top);
                let lines = Side::BOTH
                    .iter()
                    .map(|side| {
                        side.line(row).map_or(1, |line| {
                            index
                                .chars_of(*side, line)
                                .div_ceil(cols[side.index()])
                                .max(1)
                        })
                    })
                    .max()
                    .unwrap_or(1);
                top += row_height * f64_of(lines);
            }
            tops.push(top);
            tops
        });
        Self {
            result: Arc::clone(&index.result),
            cols,
            pixels_per_point: ppp,
            tops,
            rows,
            row_height,
        }
    }

    fn top(&self, row: usize) -> f64 {
        let row = row.min(self.rows);
        self.tops
            .as_ref()
            .map_or_else(|| self.row_height * f64_of(row), |tops| tops[row])
    }

    fn height(&self, row: usize) -> f64 {
        self.top(row + 1) - self.top(row)
    }

    fn total(&self) -> f64 {
        self.top(self.rows)
    }

    /// The row whose span holds `y`, clamped to the last row.
    fn row_at(&self, y: f64) -> usize {
        if self.rows == 0 {
            return 0;
        }
        let row = match &self.tops {
            None => usize_floor(y / self.row_height),
            Some(tops) => tops.partition_point(|top| *top <= y).saturating_sub(1),
        };
        row.min(self.rows - 1)
    }
}

#[expect(
    clippy::cast_precision_loss,
    reason = "row and character counts stay far below 2^52"
)]
const fn f64_of(n: usize) -> f64 {
    n as f64
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to zero first; positions stay far below usize::MAX"
)]
fn usize_floor(x: f64) -> usize {
    x.max(0.0).floor() as usize
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "screen-relative distances, well inside f32's range"
)]
const fn f32_of(x: f64) -> f32 {
    x as f32
}

// ── Widget memory ────────────────────────────────────────────────────────

/// Per-widget state kept in egui's temporary memory: scroll offsets, the
/// active pane, drag state and the two caches. Selection itself lives in
/// [`CompareState::selection`], so it survives a round trip to Findings.
#[derive(Clone, Debug, Default)]
struct Memory {
    key: Option<ComparisonRequest>,
    index: Option<Arc<Index>>,
    layout: Option<Arc<Layout>>,
    /// Vertical offsets in aligned space; equal while scrolling is linked.
    offset: [f64; 2],
    offset_x: [f32; 2],
    active: usize,
    /// The change the panes were last moved to; a different
    /// `active_change` moves them again.
    applied_change: Option<usize>,
    dragging: Option<usize>,
    linked: bool,
    /// The pane last copied from, while its selection is unchanged and the
    /// copy was normalized text.
    copied_normalized: Option<usize>,
}

impl Memory {
    /// Starts over when the comparison key changes (pair, snapshots, rules
    /// or mode): offsets and the applied change belong to the old rows, and
    /// so does Compare's selection, which is cleared. The very first key
    /// this memory sees keeps a selection that still fits the rows, so a
    /// selection survives egui dropping its temporary memory.
    fn adopt(&mut self, compare: &mut CompareState, result: &Arc<Comparison>) {
        let ComparisonOutcome::Ready(aligned) = &result.outcome else {
            return;
        };
        if self.key != Some(result.request) {
            let fits = compare
                .selection
                .iter()
                .flat_map(|s| [s.anchor, s.cursor])
                .flatten()
                .all(|p| p.row <= aligned.rows.len());
            if self.key.is_some() || !fits {
                compare.selection = [PaneSelection::default(); 2];
            }
            *self = Self {
                key: Some(result.request),
                linked: compare.linked_scroll,
                ..Self::default()
            };
        }
        if !self
            .index
            .as_ref()
            .is_some_and(|index| Arc::ptr_eq(&index.result, result))
        {
            self.index = Some(Arc::new(Index::build(Arc::clone(result), aligned)));
            self.layout = None;
        }
    }

    /// Rebuilds the row tops when the wrap width or zoom changed, keeping
    /// each pane's top row where it was.
    fn relayout(&mut self, view: &View, wrap: bool, ppp: f32) {
        let Some(index) = &self.index else {
            return;
        };
        let ComparisonOutcome::Ready(aligned) = &index.result.outcome else {
            return;
        };
        let cols = wrap.then_some(view.cols);
        let fresh = self.layout.as_ref().is_some_and(|layout| {
            Arc::ptr_eq(&layout.result, &index.result)
                && layout.cols == cols
                && (layout.pixels_per_point - ppp).abs() < f32::EPSILON
        });
        if fresh {
            return;
        }
        let layout = Layout::build(index, aligned, cols, ppp);
        if let Some(old) = &self.layout {
            for offset in &mut self.offset {
                let row = old.row_at(*offset);
                let within = (*offset - old.top(row)).min(layout.height(row));
                *offset = layout.top(row) + within.max(0.0);
            }
        }
        self.layout = Some(Arc::new(layout));
    }

    /// Re-links the panes and applies a change the host navigated to.
    fn follow_links(&mut self, compare: &CompareState, frame: &Frame<'_>) {
        if compare.linked_scroll && !self.linked {
            self.offset[1] = self.offset[0];
        }
        self.linked = compare.linked_scroll;
        if self.applied_change != Some(compare.active_change) {
            self.applied_change = Some(compare.active_change);
            if let Some(hunk) = frame.aligned.hunks.get(compare.active_change) {
                // Both panes, linked or not (§11.5 rule 6).
                let top = frame
                    .layout
                    .top(hunk.rows.start.saturating_sub(CONTEXT_ROWS));
                self.offset = [top, top];
            }
        }
    }

    fn scroll(&mut self, side: Side, delta: f64) {
        self.offset[side.index()] += delta;
        if self.linked {
            self.offset[side.other().index()] = self.offset[side.index()];
        }
    }

    fn clamp(&mut self, frame: &Frame<'_>) {
        for side in Side::BOTH {
            let s = side.index();
            let max =
                (frame.layout.total() - f64::from(frame.view.panes[s].body_height())).max(0.0);
            self.offset[s] = self.offset[s].clamp(0.0, max);
            let max_x = frame.view.content_width(frame.index, side, frame.layout)
                - frame.view.panes[s].text_width();
            self.offset_x[s] = self.offset_x[s].clamp(0.0, max_x.max(0.0));
        }
        if self.linked {
            self.offset[1] = self.offset[0];
        }
    }
}

// ── Geometry ─────────────────────────────────────────────────────────────

/// One pane's rectangles this frame.
#[derive(Clone, Copy, Debug)]
struct PaneRects {
    outer: Rect,
    /// The scrolling text region, scrollbar included.
    body: Rect,
    /// Left edge of character column 0 before horizontal scrolling.
    text_left: f32,
    /// Whether a horizontal scrollbar takes the body's bottom strip.
    hbar: bool,
}

impl PaneRects {
    fn text_right(&self) -> f32 {
        self.body.right() - SCROLLBAR - TEXT_PAD
    }

    fn text_width(&self) -> f32 {
        (self.text_right() - self.text_left).max(1.0)
    }

    fn body_height(&self) -> f32 {
        self.body.height() - if self.hbar { SCROLLBAR } else { 0.0 }
    }

    fn view(&self) -> Rect {
        Rect::from_min_max(
            self.body.min,
            Pos2::new(
                self.body.right() - SCROLLBAR,
                self.body.top() + self.body_height(),
            ),
        )
    }

    fn vbar(&self) -> Rect {
        Rect::from_min_max(
            Pos2::new(self.body.right() - SCROLLBAR, self.body.top()),
            Pos2::new(self.body.right(), self.body.top() + self.body_height()),
        )
    }

    fn hbar_rect(&self) -> Rect {
        Rect::from_min_max(
            Pos2::new(self.body.left(), self.body.bottom() - SCROLLBAR),
            Pos2::new(self.body.right() - SCROLLBAR, self.body.bottom()),
        )
    }
}

/// Frame-wide measurements: the split, both panes and font metrics.
#[derive(Debug)]
struct View {
    area: Rect,
    gap: Rect,
    panes: [PaneRects; 2],
    gutter: f32,
    char_width: f32,
    cols: [usize; 2],
}

impl View {
    fn new(ui: &Ui, index: &Index, compare: &CompareState, area: Rect) -> Self {
        let document = CompactStyle::Document.font_id();
        let detail = CompactStyle::PathDetail.font_id();
        let (char_width, digit_width) = ui.fonts_mut(|fonts| {
            (
                fonts.glyph_width(&document, 'x'),
                fonts.glyph_width(&detail, '0'),
            )
        });
        let char_width = char_width.max(1.0);
        let lines = index
            .line_count(Side::A)
            .max(index.line_count(Side::B))
            .max(1);
        let digits = f32::from(u8::try_from(lines.to_string().len()).unwrap_or(u8::MAX));
        let gutter = MIN_GUTTER.max(digits * digit_width + GUTTER_PAD + 2.0);
        let ratio = compare.split_ratio.clamp(SPLIT_RANGE.0, SPLIT_RANGE.1);
        let usable = (area.width() - PANE_GAP).max(2.0);
        let split = area.left() + (usable * ratio).round();
        let gap = Rect::from_min_max(
            Pos2::new(split, area.top()),
            Pos2::new(split + PANE_GAP, area.bottom()),
        );
        let outer = [
            Rect::from_min_max(area.min, Pos2::new(split, area.bottom())),
            Rect::from_min_max(Pos2::new(gap.right(), area.top()), area.max),
        ];
        let mut panes = outer.map(|outer| {
            let body = outer.shrink(theme::BORDER_WIDTH);
            PaneRects {
                outer,
                body,
                text_left: body.left() + SIDE_BAR + gutter + MARKER,
                hbar: false,
            }
        });
        let mut cols = [1, 1];
        for side in Side::BOTH {
            let s = side.index();
            cols[s] = usize_floor(f64::from(panes[s].text_width() / char_width)).max(1);
            if !compare.wrap {
                let widest = f64::from(index.max_chars[s]) * f64::from(char_width);
                panes[s].hbar = widest > f64::from(panes[s].text_width());
            }
        }
        Self {
            area,
            gap,
            panes,
            gutter,
            char_width,
            cols,
        }
    }

    /// Width of the widest line in units, for horizontal scrolling; zero
    /// with wrap on.
    fn content_width(&self, index: &Index, side: Side, layout: &Layout) -> f32 {
        if layout.cols.is_some() {
            return 0.0;
        }
        f32_of(f64::from(index.max_chars[side.index()]) * f64::from(self.char_width))
            + self.char_width
    }
}

// ── One frame ────────────────────────────────────────────────────────────

/// Everything a frame reads, borrowed for its duration.
struct Frame<'a> {
    aligned: &'a AlignedComparison,
    index: &'a Index,
    layout: &'a Layout,
    view: &'a View,
}

/// One input action this widget handles, read from the frame's events.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Copy,
    SelectAll,
    SwitchPane,
    Extend(Key),
    Scroll(Key),
}

fn pane_id(side: Side) -> Id {
    Id::unique(("fuzzy_dedupe.diff_view.pane", side.index()))
}

impl Frame<'_> {
    fn pane(&self, side: Side) -> &PaneRects {
        &self.view.panes[side.index()]
    }

    fn shown(&self, side: Side) -> &PaneText {
        side.pane(self.aligned).shown()
    }

    /// Reads wheel, pointer and keys. Returns the pane to copy from when a
    /// copy was requested this frame.
    fn input(&self, ui: &mut Ui, compare: &mut CompareState, memory: &mut Memory) -> Option<Side> {
        self.wheel(ui, memory);
        self.scrollbars(ui, memory);
        self.split(ui, compare);
        self.pointer(ui, compare, memory);
        self.keys(ui, compare, memory)
    }

    fn wheel(&self, ui: &Ui, memory: &mut Memory) {
        let Some(hover) = ui.input(|i| i.pointer.hover_pos()) else {
            return;
        };
        let Some(side) = Side::BOTH
            .into_iter()
            .find(|side| self.pane(*side).body.contains(hover))
        else {
            return;
        };
        let delta = ui.input(|i| i.smooth_scroll_delta);
        if delta == Vec2::ZERO {
            return;
        }
        memory.scroll(side, -f64::from(delta.y));
        memory.offset_x[side.index()] -= delta.x;
        ui.input_mut(|i| i.smooth_scroll_delta = Vec2::ZERO);
    }

    /// Vertical bars jump to and follow the pointer; the horizontal bar
    /// likewise. Linked scrolling carries a vertical bar to both panes.
    fn scrollbars(&self, ui: &Ui, memory: &mut Memory) {
        for side in Side::BOTH {
            let pane = self.pane(side);
            let s = side.index();
            let vbar = ui.interact(
                pane.vbar(),
                pane_id(side).with("vbar"),
                Sense::click_and_drag(),
            );
            if let (true, Some(pos)) = (
                vbar.is_pointer_button_down_on(),
                vbar.interact_pointer_pos(),
            ) {
                let track = pane.vbar();
                let t = f64::from(((pos.y - track.top()) / track.height()).clamp(0.0, 1.0));
                let view_h = f64::from(pane.body_height());
                memory.offset[s] = t * self.layout.total() - view_h / 2.0;
                if memory.linked {
                    memory.offset[1 - s] = memory.offset[s];
                }
            }
            if pane.hbar {
                let hbar = ui.interact(
                    pane.hbar_rect(),
                    pane_id(side).with("hbar"),
                    Sense::click_and_drag(),
                );
                if let (true, Some(pos)) = (
                    hbar.is_pointer_button_down_on(),
                    hbar.interact_pointer_pos(),
                ) {
                    let track = pane.hbar_rect();
                    let t = ((pos.x - track.left()) / track.width()).clamp(0.0, 1.0);
                    let content = self.view.content_width(self.index, side, self.layout);
                    memory.offset_x[s] = t * content - pane.text_width() / 2.0;
                }
            }
        }
    }

    fn split(&self, ui: &Ui, compare: &mut CompareState) {
        let handle = ui.interact(
            self.view.gap,
            Id::unique("fuzzy_dedupe.diff_view.split"),
            Sense::drag(),
        );
        if handle.hovered() || handle.dragged() {
            ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
        }
        if let (true, Some(pos)) = (handle.dragged(), handle.interact_pointer_pos()) {
            let area = self.view.area;
            let ratio = (pos.x - area.left()) / (area.width() - PANE_GAP).max(1.0);
            compare.split_ratio = ratio.clamp(SPLIT_RANGE.0, SPLIT_RANGE.1);
        }
    }

    /// Press, Shift+press and drag selection. A drag belongs to the pane it
    /// started in: the pointer is clamped into that pane, so moving into
    /// the other pane never extends or starts a selection there.
    fn pointer(&self, ui: &Ui, compare: &mut CompareState, memory: &mut Memory) {
        let (pressed, down, origin, latest, shift, dt) = ui.input(|i| {
            (
                i.pointer.primary_pressed(),
                i.pointer.primary_down(),
                i.pointer.press_origin(),
                i.pointer.latest_pos(),
                i.modifiers.shift,
                i.stable_dt.min(0.1),
            )
        });
        for side in Side::BOTH {
            let view = self.pane(side).view();
            let response = ui.interact(view, pane_id(side), Sense::click_and_drag());
            response.widget_info(|| {
                WidgetInfo::labeled(egui::accesskit::Role::Document, true, side.letter())
            });
            ui.memory_mut(|m| {
                m.set_focus_lock_filter(
                    pane_id(side),
                    EventFilter {
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        ..EventFilter::default()
                    },
                );
            });
            let Some(origin) = origin.filter(|o| pressed && view.contains(*o)) else {
                continue;
            };
            if ui.ctx().layer_id_at(origin) != Some(ui.layer_id()) {
                continue;
            }
            let at = self.hit(ui, memory, side, origin);
            let selection = &mut compare.selection[side.index()];
            if shift {
                selection.anchor = selection.anchor.or(selection.cursor).or(Some(at));
            } else {
                selection.anchor = Some(at);
            }
            selection.cursor = Some(at);
            memory.active = side.index();
            memory.copied_normalized = None;
            memory.dragging = down.then_some(side.index());
            response.request_focus();
        }
        if !down || pressed {
            if !down {
                memory.dragging = None;
            }
            return;
        }
        let (Some(s), Some(pos)) = (memory.dragging, latest) else {
            return;
        };
        let side = Side::BOTH[s];
        let view = self.pane(side).view();
        let clamped = pos.clamp(view.min, view.max - Vec2::splat(0.5));
        let at = self.hit(ui, memory, side, clamped);
        if compare.selection[s].cursor != Some(at) {
            compare.selection[s].cursor = Some(at);
            memory.copied_normalized = None;
        }
        let beyond = if pos.y < view.top() {
            pos.y - view.top()
        } else if pos.y > view.bottom() {
            pos.y - view.bottom()
        } else {
            0.0
        };
        if beyond != 0.0 {
            memory.scroll(side, f64::from(beyond * AUTO_SCROLL * dt));
            ui.ctx().request_repaint();
        }
    }

    /// Takes this widget's keys out of the frame's events, matching
    /// modifiers exactly so the host's Alt chords pass through untouched.
    fn keys(&self, ui: &Ui, compare: &mut CompareState, memory: &mut Memory) -> Option<Side> {
        let focused = ui.memory(egui::Memory::focused);
        let ours = focused.is_none_or(|f| Side::BOTH.iter().any(|side| pane_id(*side) == f));
        if !ours {
            return None;
        }
        let actions = ui.input_mut(|input| {
            let mut actions = Vec::new();
            input.events.retain(|event| {
                let action = match event {
                    Event::Copy => Some(Action::Copy),
                    Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } => {
                        let ctrl = modifiers.ctrl || modifiers.command;
                        match (ctrl, modifiers.shift, modifiers.alt, key) {
                            (true, false, false, Key::C) => Some(Action::Copy),
                            (true, false, false, Key::A) => Some(Action::SelectAll),
                            (false, false, false, Key::F6) => Some(Action::SwitchPane),
                            (
                                false,
                                true,
                                false,
                                Key::ArrowLeft | Key::ArrowRight | Key::ArrowUp | Key::ArrowDown,
                            ) => Some(Action::Extend(*key)),
                            (
                                false,
                                false,
                                false,
                                Key::ArrowUp | Key::ArrowDown | Key::PageUp | Key::PageDown,
                            ) => Some(Action::Scroll(*key)),
                            _ => None,
                        }
                    }
                    _ => None,
                };
                if let Some(action) = action {
                    actions.push(action);
                    false
                } else {
                    true
                }
            });
            actions
        });
        let mut copy = None;
        for action in actions {
            let side = Side::BOTH[memory.active.min(1)];
            match action {
                Action::Copy => copy = Some(side),
                Action::SelectAll => {
                    compare.selection[side.index()] = PaneSelection {
                        anchor: Some(TextPosition::default()),
                        cursor: Some(TextPosition {
                            row: self.index.rows(),
                            column: 0,
                        }),
                    };
                    memory.copied_normalized = None;
                }
                Action::SwitchPane => {
                    memory.active = side.other().index();
                    ui.memory_mut(|m| m.request_focus(pane_id(side.other())));
                }
                Action::Extend(key) => self.extend(compare, memory, side, key),
                Action::Scroll(key) => {
                    let row = self.layout.row_height;
                    let page = (f64::from(self.pane(side).body_height()) - row).max(row);
                    let delta = match key {
                        Key::ArrowUp => -row,
                        Key::ArrowDown => row,
                        Key::PageUp => -page,
                        _ => page,
                    };
                    memory.scroll(side, delta);
                }
            }
        }
        copy
    }

    /// Shift+arrow: moves the cursor of `side`'s selection by one
    /// character or line, keeping its anchor, and scrolls it into view.
    fn extend(&self, compare: &mut CompareState, memory: &mut Memory, side: Side, key: Key) {
        let rows = self.index.rows();
        let selection = &mut compare.selection[side.index()];
        let start = selection.cursor.unwrap_or_else(|| TextPosition {
            row: self.layout.row_at(memory.offset[side.index()]),
            column: 0,
        });
        let cursor = self.canonical(side, start);
        let end = TextPosition {
            row: rows,
            column: 0,
        };
        let at = |line: usize, column: usize| TextPosition {
            row: self.index.row_of(side, line),
            column,
        };
        let line = self
            .index
            .line_at(side, cursor.row)
            .filter(|_| cursor.row < rows);
        let last = self.index.line_count(side).checked_sub(1);
        let moved = match (key, line) {
            (Key::ArrowLeft, None) => last.map_or(cursor, |l| at(l, self.index.chars_of(side, l))),
            (Key::ArrowLeft, Some(l)) if cursor.column > 0 => at(l, cursor.column - 1),
            (Key::ArrowLeft, Some(l)) => l
                .checked_sub(1)
                .map_or(cursor, |p| at(p, self.index.chars_of(side, p))),
            (Key::ArrowRight, Some(l)) if cursor.column < self.index.chars_of(side, l) => {
                at(l, cursor.column + 1)
            }
            (Key::ArrowRight | Key::ArrowDown, Some(l)) if l + 1 < self.index.line_count(side) => {
                let column = if key == Key::ArrowDown {
                    cursor.column.min(self.index.chars_of(side, l + 1))
                } else {
                    0
                };
                at(l + 1, column)
            }
            (Key::ArrowRight | Key::ArrowDown, _) => end,
            (Key::ArrowUp, Some(l)) => l.checked_sub(1).map_or_else(
                || at(l, 0),
                |p| at(p, cursor.column.min(self.index.chars_of(side, p))),
            ),
            (Key::ArrowUp, None) => last.map_or(cursor, |l| at(l, 0)),
            _ => cursor,
        };
        selection.anchor = selection.anchor.or(Some(cursor));
        selection.cursor = Some(moved);
        memory.active = side.index();
        memory.copied_normalized = None;
        let row = moved.row.min(rows.saturating_sub(1));
        let (top, bottom) = (self.layout.top(row), self.layout.top(row + 1));
        let view_h = f64::from(self.pane(side).body_height());
        let offset = memory.offset[side.index()];
        if top < offset {
            memory.scroll(side, top - offset);
        } else if bottom > offset + view_h {
            memory.scroll(side, bottom - offset - view_h);
        }
    }

    /// The same position on the row that holds its line: a spacer position
    /// moves to the next line's start, or to the end of the text.
    fn canonical(&self, side: Side, pos: TextPosition) -> TextPosition {
        let rows = self.index.rows();
        if pos.row >= rows {
            return TextPosition {
                row: rows,
                column: 0,
            };
        }
        if side.line(&self.aligned.rows[pos.row]).is_some() {
            return pos;
        }
        self.index.line_at(side, pos.row).map_or(
            TextPosition {
                row: rows,
                column: 0,
            },
            |line| TextPosition {
                row: self.index.row_of(side, line),
                column: 0,
            },
        )
    }

    /// The position under `pos` in `side`'s pane, as drawn last frame.
    fn hit(&self, ui: &Ui, memory: &Memory, side: Side, pos: Pos2) -> TextPosition {
        let pane = self.pane(side);
        let rows = self.index.rows();
        let y = f64::from(pos.y - pane.body.top()) + memory.offset[side.index()];
        if rows == 0 || y >= self.layout.total() {
            return TextPosition {
                row: rows,
                column: 0,
            };
        }
        let row = self.layout.row_at(y);
        let Some(line) = side.line(&self.aligned.rows[row]) else {
            return TextPosition { row, column: 0 };
        };
        let text = self.shown(side).line(line).unwrap_or("");
        let chunks = chunks(text, self.layout.cols.map(|c| c[side.index()]));
        let visual = usize_floor((y - self.layout.top(row)) / self.layout.row_height);
        let Some((bytes, first_char)) = chunks.get(visual).cloned() else {
            return TextPosition {
                row,
                column: self.index.chars_of(side, line),
            };
        };
        let x = pos.x - pane.text_left + memory.offset_x[side.index()];
        let galley = text_galley(ui, &text[bytes.clone()], Color32::PLACEHOLDER);
        let within = usize::from(
            galley
                .cursor_from_pos(Vec2::new(x, self.view_row_height() / 2.0))
                .index,
        );
        TextPosition {
            row,
            column: first_char + within.min(text[bytes].chars().count()),
        }
    }

    fn view_row_height(&self) -> f32 {
        f32_of(self.layout.row_height)
    }

    /// Byte offset in `side`'s shown text of a selection position.
    fn byte_at(&self, side: Side, pos: TextPosition) -> usize {
        let shown = self.shown(side);
        let Some(line) = self.index.line_at(side, pos.row) else {
            return shown.text.len();
        };
        let span = shown.lines[line];
        if pos.row >= self.aligned.rows.len() || side.line(&self.aligned.rows[pos.row]).is_none() {
            return span.start;
        }
        let text = shown.line(line).unwrap_or("");
        span.start + char_byte(text, pos.column)
    }

    fn selected_bytes(&self, side: Side, selection: PaneSelection) -> Option<Range<usize>> {
        let (Some(anchor), Some(cursor)) = (selection.anchor, selection.cursor) else {
            return None;
        };
        let (a, b) = (self.byte_at(side, anchor), self.byte_at(side, cursor));
        let range = a.min(b)..a.max(b);
        (!range.is_empty()).then_some(range)
    }

    /// Puts the active pane's selection on the clipboard: a byte slice of
    /// the shown text, so terminators are the file's own and nothing drawn
    /// around the text (numbers, markers, spacers) can be included.
    fn copy(&self, ui: &Ui, compare: &CompareState, memory: &mut Memory, side: Side) {
        let Some(range) = self.selected_bytes(side, compare.selection[side.index()]) else {
            return;
        };
        let text = &self.shown(side).text[range];
        ui.ctx().copy_text(text.to_owned());
        memory.copied_normalized =
            (self.aligned.mode == CompareMode::Normalized).then_some(side.index());
    }
}

/// Byte offset of character `column` in `text`, clamped to its end.
fn char_byte(text: &str, column: usize) -> usize {
    text.char_indices()
        .nth(column)
        .map_or(text.len(), |(i, _)| i)
}

/// The visual lines of one source line: byte ranges and the character
/// index each starts at. One chunk with wrap off, and one for an empty
/// line.
fn chunks(text: &str, cols: Option<usize>) -> Vec<(Range<usize>, usize)> {
    let Some(cols) = cols else {
        return vec![(0..text.len(), 0)];
    };
    let mut out = Vec::new();
    let mut start = (0, 0);
    for (count, (byte, _)) in text.char_indices().enumerate() {
        if count > 0 && count % cols == 0 {
            out.push((start.0..byte, start.1));
            start = (byte, count);
        }
    }
    out.push((start.0..text.len(), start.1));
    out
}

fn text_galley(ui: &Ui, text: &str, color: Color32) -> Arc<Galley> {
    let style = CompactStyle::Document;
    let job = LayoutJob::single_section(
        text.to_owned(),
        TextFormat {
            font_id: style.font_id(),
            color,
            line_height: Some(style.line_height()),
            ..TextFormat::default()
        },
    );
    ui.fonts_mut(|fonts| fonts.layout_job(job))
}

// ── Painting ─────────────────────────────────────────────────────────────

impl Frame<'_> {
    fn paint_pane(&self, ui: &mut Ui, compare: &CompareState, memory: &Memory, side: Side) {
        let palette = theme::palette(mode_of(ui.ctx()));
        let pane = *self.pane(side);
        let painter = ui.painter();
        painter.rect(
            pane.outer,
            theme::CORNER_RADIUS,
            palette.color(Token::Surface),
            Stroke::new(theme::BORDER_WIDTH, palette.color(Token::Line)),
            StrokeKind::Inside,
        );
        self.paint_rows(ui, compare, memory, side, palette);
        self.paint_scrollbars(ui, memory, side, palette);
    }

    fn paint_rows(
        &self,
        ui: &Ui,
        compare: &CompareState,
        memory: &Memory,
        side: Side,
        palette: &Palette,
    ) {
        let pane = self.pane(side);
        let s = side.index();
        let view = pane.view();
        let offset = memory.offset[s];
        let rows = self.index.rows();
        if rows == 0 {
            return;
        }
        let first = self.layout.row_at(offset).saturating_sub(OVERSCAN);
        let bottom = offset + f64::from(view.height());
        let selected = self.selected_bytes(side, compare.selection[s]);
        let painter = ui.painter().with_clip_rect(view);
        let mut row = first;
        let mut after = 0;
        while row < rows && after <= OVERSCAN {
            if self.layout.top(row) >= bottom {
                after += 1;
            }
            let top = view.top() + f32_of(self.layout.top(row) - offset);
            let height = f32_of(self.layout.height(row));
            let rect = Rect::from_min_max(
                Pos2::new(pane.body.left(), top),
                Pos2::new(view.right(), top + height),
            );
            self.paint_row(
                ui,
                &painter,
                &RowPaint {
                    side,
                    row,
                    rect,
                    offset_x: memory.offset_x[s],
                    selected: selected.clone(),
                    palette,
                },
            );
            row += 1;
        }
    }

    fn paint_row(&self, ui: &Ui, painter: &egui::Painter, p: &RowPaint<'_>) {
        let row = &self.aligned.rows[p.row];
        let line = p.side.line(row);
        let changed = line.is_some() && row.kind != RowKind::Equal;
        let (strong, soft, _) = p.side.tokens();
        let pane = self.pane(p.side);
        let row_height = self.view_row_height();
        if line.is_none() {
            painter.rect_filled(p.rect, 0.0, p.palette.color(Token::Canvas));
        } else if changed {
            painter.rect_filled(p.rect, 0.0, p.palette.color(soft));
            let bar = Rect::from_min_size(p.rect.min, Vec2::new(SIDE_BAR, p.rect.height()));
            painter.rect_filled(bar, 0.0, p.palette.color(strong));
        }
        let number = line.map(|l| (l + 1).to_string());
        if let Some(number) = &number {
            let detail = CompactStyle::PathDetail;
            let gutter_right = p.rect.left() + SIDE_BAR + self.view.gutter - GUTTER_PAD;
            let y = p.rect.top() + row_height / 2.0;
            let color = p
                .palette
                .color(if changed { Token::Muted } else { Token::Quiet });
            painter.text(
                Pos2::new(gutter_right, y),
                Align2::RIGHT_CENTER,
                number,
                detail.font_id(),
                color,
            );
            if changed {
                let marker = p.rect.left() + SIDE_BAR + self.view.gutter + MARKER / 2.0;
                painter.text(
                    Pos2::new(marker, y),
                    Align2::CENTER_CENTER,
                    p.side.letter(),
                    detail.font_id(),
                    p.palette.color(strong),
                );
            }
        }
        let text_x = pane.text_left - p.offset_x;
        let cell = Rect::from_min_max(
            Pos2::new(text_x, p.rect.top()),
            Pos2::new(p.rect.right().max(text_x + 1.0), p.rect.bottom()),
        );
        register_row(ui, p.side, p.row, cell, number.as_deref());
        if let Some(line) = line {
            self.paint_text(ui, painter, p, line, text_x);
        }
    }

    /// One line's text in visual-line chunks, under its selection band and
    /// its changed-character highlights.
    fn paint_text(
        &self,
        ui: &Ui,
        painter: &egui::Painter,
        p: &RowPaint<'_>,
        line: usize,
        text_x: f32,
    ) {
        let row = &self.aligned.rows[p.row];
        let (_, _, highlight) = p.side.tokens();
        let row_height = self.view_row_height();
        let shown = self.shown(p.side);
        let text = shown.line(line).unwrap_or("");
        let span = shown.lines[line];
        let spans: Vec<Range<usize>> = match &row.kind {
            RowKind::Changed(inline) => match p.side {
                Side::A => inline.a.clone(),
                Side::B => inline.b.clone(),
            },
            _ => Vec::new(),
        };
        // The selection as a byte range within this line; an end past the
        // text means the terminator is selected too.
        let selection = p.selected.as_ref().and_then(|sel| {
            let (start, end) = (sel.start.max(span.start), sel.end.min(span.end));
            (start < end).then(|| (start - span.start, end - span.start))
        });
        let ink = p.palette.color(Token::Ink);
        let mut y = p.rect.top();
        for (bytes, _) in chunks(text, self.layout.cols.map(|c| c[p.side.index()])) {
            let galley = text_galley(ui, &text[bytes.clone()], ink);
            let x_of = |byte: usize| {
                let local = byte.clamp(bytes.start, bytes.end) - bytes.start;
                let chars = text[bytes.start..bytes.start + local].chars().count();
                text_x + galley.pos_from_cursor(CCursor::new(chars)).min.x
            };
            let band = |from: usize, to: usize| {
                Rect::from_min_max(
                    Pos2::new(x_of(from), y),
                    Pos2::new(x_of(to), y + row_height),
                )
            };
            if let Some((from, to)) = selection {
                let newline = bytes.end == text.len() && to > text.len();
                let low = from.clamp(bytes.start, bytes.end);
                let high = to.clamp(low, bytes.end);
                if low < high || (newline && from <= bytes.end) {
                    let mut rect = band(low, high);
                    if newline {
                        rect.max.x += self.view.char_width / 2.0;
                    }
                    painter.rect_filled(rect, 0.0, ui.visuals().selection.bg_fill);
                }
            }
            for range in &spans {
                if range.end > bytes.start && range.start < bytes.end {
                    painter.rect_filled(
                        band(range.start, range.end),
                        2.0,
                        p.palette.color(highlight),
                    );
                }
            }
            painter.galley(Pos2::new(text_x, y), galley, ink);
            y += row_height;
        }
    }

    fn paint_scrollbars(&self, ui: &Ui, memory: &Memory, side: Side, palette: &Palette) {
        let pane = self.pane(side);
        let s = side.index();
        let painter = ui.painter();
        let track = pane.vbar();
        painter.rect_filled(track, 0.0, palette.color(Token::Code));
        let total = self.layout.total().max(1.0);
        let (strong, _, _) = side.tokens();
        let mut last_mark = f32::NEG_INFINITY;
        for (row, has_a, has_b) in &self.index.marks {
            let shows = match side {
                Side::A => *has_a,
                Side::B => *has_b,
            };
            let y = track.top() + f32_of(self.layout.top(*row) / total) * track.height();
            if shows && y - last_mark >= 1.0 {
                painter.rect_filled(
                    Rect::from_min_size(Pos2::new(track.left(), y), Vec2::new(SCROLLBAR, 4.0)),
                    0.0,
                    palette.color(strong),
                );
                last_mark = y;
            }
        }
        let view_h = f64::from(pane.body_height());
        if total > view_h {
            let length = (f32_of(view_h / total) * track.height()).max(20.0);
            let max = (total - view_h).max(1.0);
            let y = track.top() + f32_of(memory.offset[s] / max) * (track.height() - length);
            painter.rect_filled(
                Rect::from_min_size(Pos2::new(track.left() + 2.0, y), Vec2::new(4.0, length)),
                2.0,
                palette.color(Token::Quiet),
            );
        }
        if pane.hbar {
            let bar = pane.hbar_rect();
            painter.rect_filled(bar, 0.0, palette.color(Token::Code));
            let content = self
                .view
                .content_width(self.index, side, self.layout)
                .max(1.0);
            let length = (pane.text_width() / content * bar.width()).clamp(20.0, bar.width());
            let max = (content - pane.text_width()).max(1.0);
            let x = bar.left() + memory.offset_x[s] / max * (bar.width() - length);
            painter.rect_filled(
                Rect::from_min_size(Pos2::new(x, bar.top() + 2.0), Vec2::new(length, 4.0)),
                2.0,
                palette.color(Token::Quiet),
            );
        }
    }
}

/// The row's accessibility node (see the module header's contract).
fn register_row(ui: &Ui, side: Side, row: usize, cell: Rect, number: Option<&str>) {
    let id = pane_id(side).with(("row", row));
    let response = ui.interact(cell, id, Sense::hover());
    response.widget_info(|| match number {
        Some(number) => WidgetInfo::labeled(egui::accesskit::Role::Row, true, number),
        None => WidgetInfo::new(egui::accesskit::Role::Row),
    });
    ui.ctx().accesskit_node_builder(id, |node| {
        node.set_author_id(format!("diff_view:{}:{row}", side.author()));
    });
}

/// One row's paint inputs.
struct RowPaint<'a> {
    side: Side,
    row: usize,
    rect: Rect,
    offset_x: f32,
    selected: Option<Range<usize>>,
    palette: &'a Palette,
}
