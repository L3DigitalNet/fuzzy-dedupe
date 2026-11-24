// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! The Compare workspace (spec §11.2, §11.5; frames `13:1304`, `29:3648`,
//! `37:2548`): the toolbar's own items, the evidence row with its Only-in
//! menus, the change row, the pinned file identities, and the diff widget
//! (`widgets/diff_view.rs`) below them. [`toolbar`] and [`show`] are the
//! entry points; the shell draws the shared trailing items (Review cleanup
//! (N), Settings) after [`toolbar`].
//!
//! Sides. Side A is the counterpart, the evidence anchor in the left pane,
//! and side B the target, the file Queue B for cleanup… names (C-04 as T35
//! oriented it). Queue B and Keep both therefore send the pair as the
//! comparison key holds it, `(target, counterpart)`, with the request ID of
//! the comparison on screen, which is the only evidence the engine binds a
//! Compare decision to (C-05).
//!
//! The return context. Compare never writes a Findings field: the seven
//! IR-001 properties live in `FindingsState`, which stays untouched while
//! Compare shows, and the table applies its own recorded scroll offset on
//! the first frame it draws again. A write from here would land one frame
//! late, because the table's scroll area is animated (T33). Returning is
//! only a workspace switch.
//!
//! The comparison slot. One slot serves Compare and Review unique text
//! (`state::ComparisonSlot`). Compare pins the request its pair was last
//! answered for (`CompareState::pinned`) and, whenever it draws with no
//! overlay open and the slot no longer holds that request in the current
//! mode, submits it again; the engine answers from its cache. The panes
//! therefore always show the evidence they were opened on, and the stale
//! marker says when the published view has moved past it.
//!
//! Keys (§11.10, Compare scope). This view binds Esc, Alt+Up/Down,
//! Alt+Left/Right, Alt+Z and Alt+L, with exact modifiers, and only while
//! no popup or menu is open. The diff widget binds F6, Ctrl+A, Ctrl+C and
//! the plain scrolling keys; nothing here touches them.
//!
//! Menus. "Only in A/B ▾" open through the dialog host's single overlay
//! slot (`MenuKind::OnlyIn`); this view draws the menu the slot names and
//! keeps only whether its first item has taken focus. Every item first
//! closes the overlay, then sends its own intent.
//!
//! A request the engine does not admit (Busy) is retried at most every
//! `RETRY_SECONDS` of egui time, never every frame (see `resubmit`).
//!
//! Interpretations recorded for review:
//! - §11.5 and the frames give no wording for the no-pair state; the title
//!   and explanation are [`NO_PAIR_TITLE`] and [`NO_PAIR_BODY`].
//! - The rail and Ctrl+2 (§11.2) are routed in `commands.rs`
//!   (`switch_to_compare`): the focused member row against its recorded
//!   counterpart, else the last valid pair, which this view shows, else
//!   the no-pair state drawn here.
//! - Below the reference width, what does not fit is dropped: the
//!   toolbar's group label and pair, the evidence row's reminder, and the
//!   identity rows' size and date, as frame `29:3648` does (the plan
//!   leaves the mechanics to the executor).
//! - While the evidence is stale, Keep both and Queue B are disabled until
//!   Compare again re-opens the pair on the current snapshots: a decision
//!   must bind to evidence the published view still holds (C-05 I-7).
//!
//! Requirements: `theme::install` has run on the context (the compact text
//! styles are looked up by name, and egui panics on an unknown style).

use std::sync::Arc;

use egui::text::{LayoutJob, TextFormat};
use egui::{
    Align, Align2, Color32, Event, Id, Key, Layout, Rect, Response, RichText, Sense, Stroke,
    StrokeKind, Ui, UiBuilder, Vec2,
};
use fuzzy_dedupe_domain::{CompareMode, QueueOrigin, RelationKind, RequestId};
use fuzzy_dedupe_engine::compare::{
    AlignedComparison, Comparison, ComparisonOutcome, ComparisonRequest,
};
use fuzzy_dedupe_engine::service::{EngineRequest, ReviewCommand};

use crate::presentation::commands::{IntentQueue, UiIntent};
use crate::presentation::projections::{self, CompareView, PaneIdentity};
use crate::presentation::state::{
    CompareState, DialogKind, DialogSubject, MenuKind, UniqueTextSubject, WorkspaceState,
};
use crate::ui::WorkspaceCx;
use crate::ui::dialogs::matching_rules::check_box;
use crate::ui::shell::{CONTROL_HEIGHT, GAP};
use crate::ui::theme::{self, CompactStyle, Icon, Palette, Token};
use crate::ui::widgets::dialog_host::mode_of;
use crate::ui::widgets::diff_view;

/// Accessible names and visible strings of Compare's controls (spec §11.5,
/// frame `13:1304`).
pub const BACK_NAME: &str = "‹ Findings";
pub const KEEP_BOTH_NAME: &str = "Keep both";
pub const QUEUE_B_NAME: &str = "Queue B for cleanup…";
pub const PREVIOUS_CANDIDATE_NAME: &str = "Previous candidate";
pub const NEXT_CANDIDATE_NAME: &str = "Next candidate";
pub const PREVIOUS_CHANGE_NAME: &str = "‹ Previous";
pub const NEXT_CHANGE_NAME: &str = "Next ›";
pub const WRAP_NAME: &str = "Wrap";
pub const LINKED_SCROLL_NAME: &str = "Linked scroll";
pub const ORIGINAL_NAME: &str = "Original";
pub const NORMALIZED_NAME: &str = "Normalized";
pub const READ_ONLY_NOTE: &str = "Read-only · similarity is not deletion safety";
pub const STALE_BADGE: &str = "Stale evidence";
pub const COMPARE_AGAIN_NAME: &str = "Compare again";
pub const NO_PAIR_TITLE: &str = "No pair to compare";
pub const NO_PAIR_BODY: &str = "Compare shows a file beside the file it is measured against. In Findings, focus a file and press Enter or choose Compare.";
/// The Only-in menus' items after the first, which names its side.
pub const COPY_LINES_NAME: &str = "Copy those lines";
pub const REVIEW_UNIQUE_NAME: &str = "Review unique text…";

/// Evidence and change row height (frame `13:1372`, `13:1379`).
const ROW_HEIGHT: f32 = 28.0;
/// The pinned file identity's height (frame `13:1410`).
const IDENTITY_HEIGHT: f32 = 54.0;
/// Gap between the panes, matching `diff_view`'s own split.
const PANE_GAP: f32 = 8.0;
/// `diff_view`'s split bounds; the identity rows follow the same split.
const SPLIT_RANGE: (f32, f32) = (0.2, 0.8);
const MENU_WIDTH: f32 = 260.0;
const MENU_ITEM_HEIGHT: f32 = 28.0;

/// "Jump to first line only in A".
#[must_use]
pub fn jump_item_name(b: bool) -> String {
    format!("Jump to first line only in {}", letter(b))
}

/// The Only-in menu's accessible name: the pane caption's text and "▾".
#[must_use]
pub fn only_in_name(b: bool, count: u64, normalized: bool) -> String {
    format!(
        "{} ▾",
        diff_view::only_in_caption(letter(b), count, normalized)
    )
}

const fn letter(b: bool) -> &'static str {
    if b { "B" } else { "A" }
}

// ── Icon glyphs (moved to one registry by T34) ───────────────────────────

// Private-use code points for the icons Compare draws as tinted text
// glyphs. Cross-file contract: the shell holds U+E100–U+E103,
// `grouped_table` U+E110–U+E116, `matching_rules` U+E130–U+E132 and
// `queue_identical` U+E170–U+E171; egui keys a glyph rasterizer by its
// text, so a code point registered twice draws whichever icon came last.
const GLYPHS: [(Icon, char); 2] = [(Icon::Document, '\u{E150}'), (Icon::Expanded, '\u{E151}')];

fn glyphs_flag() -> Id {
    Id::unique("fd.compare.glyphs")
}

/// Registers Compare's glyphs once per context. A glyph is usable only from
/// the next pass, and laying out a code point no font or rasterizer draws
/// panics, so [`glyph_text`] draws nothing until then. An SVG that fails
/// to parse is logged and never drawn.
fn ensure_glyphs(ctx: &egui::Context) {
    if ctx
        .data(|d| d.get_temp::<(u64, Vec<char>)>(glyphs_flag()))
        .is_some()
    {
        return;
    }
    let mut loaded = Vec::new();
    for (icon, ch) in GLYPHS {
        match egui_extras::SvgGlyph::from_bytes(icon.bytes()) {
            Ok(svg) => {
                ctx.add_glyph_rasterizer(
                    svg.with_color(false)
                        .with_baseline_offset(-0.2)
                        .into_rasterizer(ch.to_string()),
                );
                loaded.push(ch);
            }
            Err(err) => tracing::warn!(icon = icon.lucide_name(), %err, "icon glyph not loaded"),
        }
    }
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| d.insert_temp(glyphs_flag(), (pass, loaded)));
    ctx.request_repaint();
}

/// `icon` as text in the current text colour, or "" while it is not usable.
fn glyph_text(ctx: &egui::Context, icon: Icon) -> String {
    let Some((pass, loaded)) = ctx.data(|d| d.get_temp::<(u64, Vec<char>)>(glyphs_flag())) else {
        return String::new();
    };
    if ctx.cumulative_pass_nr() <= pass {
        return String::new();
    }
    GLYPHS
        .iter()
        .find(|(candidate, ch)| *candidate == icon && loaded.contains(ch))
        .map(|(_, ch)| ch.to_string())
        .unwrap_or_default()
}

// ── Pair, evidence and keys ──────────────────────────────────────────────

/// Compare's view of its pair, or `None` without a valid pair.
fn current_view(cx: &WorkspaceCx<'_>) -> Option<CompareView> {
    let results = cx.scan.results.as_ref()?;
    projections::compare_view(results, cx.queue, cx.compare.pair?)
}

/// The request whose answer the panes show: the pinned key in the current
/// mode, or `None` before the pair has been answered once.
fn shown_request(compare: &CompareState) -> Option<ComparisonRequest> {
    let pair = compare.pair?;
    let pinned = compare
        .pinned
        .filter(|k| (k.target, k.counterpart) == (pair.target, pair.counterpart))?;
    Some(ComparisonRequest {
        mode: compare.mode,
        ..pinned
    })
}

/// The ready comparison on screen and its request ID: the evidence Keep
/// both, Queue B and Review unique text bind to. The result is the slot's
/// shared `Arc`, never a copy: a 5 MB pair holds about 100,000 rows, and
/// the T2 frame budget (§11.9) has no room for copying them per frame.
fn shown(compare: &CompareState) -> Option<(RequestId, Arc<Comparison>)> {
    let request = shown_request(compare)?;
    let slot = &compare.comparison;
    let answered = slot.matches(&request)
        && slot
            .result
            .as_ref()
            .is_some_and(|result| result.request == request);
    if !answered {
        return None;
    }
    slot.aligned()?;
    Some((slot.request?, Arc::clone(slot.result.as_ref()?)))
}

/// The aligned rows of a ready comparison.
fn ready(comparison: &Comparison) -> Option<&AlignedComparison> {
    match &comparison.outcome {
        ComparisonOutcome::Ready(aligned) => Some(aligned),
        ComparisonOutcome::Unavailable(_) => None,
    }
}

/// How many changes the comparison on screen holds.
fn change_count(compare: &CompareState) -> usize {
    shown(compare)
        .as_ref()
        .and_then(|(_, c)| ready(c))
        .map_or(0, |a| a.hunks.len())
}

/// Pins the slot's key while it answers Compare's own pair and snapshots.
fn pin(compare: &mut CompareState) {
    let (Some(pair), Some(key)) = (compare.pair, compare.comparison.key) else {
        return;
    };
    if (key.target, key.counterpart) == (pair.target, pair.counterpart)
        && compare.target_snapshot == Some(key.target_snapshot)
        && compare.counterpart_snapshot == Some(key.counterpart_snapshot)
    {
        compare.pinned = Some(key);
    }
}

/// Copies the remembered wrap, linked scroll and split into Compare's
/// state once per context, before Compare first draws (C-08).
fn adopt_preferences(ctx: &egui::Context, cx: &mut WorkspaceCx<'_>) {
    let key = Id::unique("fd.compare.preferences_adopted");
    if ctx.data(|d| d.get_temp::<bool>(key)).is_some() {
        return;
    }
    ctx.data_mut(|d| d.insert_temp(key, true));
    cx.compare.wrap = cx.preferences.compare_wrap;
    cx.compare.linked_scroll = cx.preferences.compare_linked_scroll;
    cx.compare.split_ratio = cx.preferences.compare_split_ratio;
}

/// Takes a pressed `key` with exactly Alt (`alt`) or no modifier from this
/// frame's events, release included. Exact matching, as the global keymap
/// does: egui's `consume_key` ignores extra Shift and Alt.
fn take_key(ui: &Ui, alt: bool, key: Key) -> bool {
    ui.input_mut(|input| {
        let mut hit = false;
        input.events.retain(|event| {
            let Event::Key {
                key: k,
                pressed,
                modifiers,
                ..
            } = event
            else {
                return true;
            };
            let exact = *k == key
                && modifiers.alt == alt
                && !modifiers.ctrl
                && !modifiers.command
                && !modifiers.shift;
            if exact {
                hit |= *pressed;
            }
            !exact
        });
        hit
    })
}

fn back_to_findings(intents: &mut IntentQueue) {
    intents.push(UiIntent::SwitchWorkspace(WorkspaceState::Findings));
}

/// Re-aims Compare at candidate `index` of the stepper.
fn step_candidate(cx: &WorkspaceCx<'_>, intents: &mut IntentQueue, index: usize) {
    let (Some(pair), Some(&counterpart)) = (cx.compare.pair, cx.compare.candidates.get(index))
    else {
        return;
    };
    intents.push(UiIntent::OpenCompare {
        target: pair.target,
        counterpart,
        return_row: cx.compare.return_row,
    });
}

fn set_change(compare: &mut CompareState, forward: bool) {
    let changes = change_count(compare);
    if changes == 0 {
        return;
    }
    let active = compare.active_change.min(changes - 1);
    compare.active_change = if forward {
        (active + 1).min(changes - 1)
    } else {
        active.saturating_sub(1)
    };
}

fn toggle_wrap(cx: &mut WorkspaceCx<'_>) {
    cx.compare.wrap = !cx.compare.wrap;
    cx.preferences.compare_wrap = cx.compare.wrap;
}

fn toggle_linked(cx: &mut WorkspaceCx<'_>) {
    cx.compare.linked_scroll = !cx.compare.linked_scroll;
    cx.preferences.compare_linked_scroll = cx.compare.linked_scroll;
}

/// The §11.10 Compare chords this view owns.
fn keys(ui: &Ui, cx: &mut WorkspaceCx<'_>, intents: &mut IntentQueue) {
    if !cx.input_enabled {
        return;
    }
    if take_key(ui, false, Key::Escape) {
        back_to_findings(intents);
    }
    if take_key(ui, true, Key::ArrowDown) {
        set_change(cx.compare, true);
    }
    if take_key(ui, true, Key::ArrowUp) {
        set_change(cx.compare, false);
    }
    let index = cx.compare.candidate_index;
    if take_key(ui, true, Key::ArrowRight) && index + 1 < cx.compare.candidates.len() {
        step_candidate(cx, intents, index + 1);
    }
    if take_key(ui, true, Key::ArrowLeft) && index > 0 {
        step_candidate(cx, intents, index - 1);
    }
    if take_key(ui, true, Key::Z) {
        toggle_wrap(cx);
    }
    if take_key(ui, true, Key::L) {
        toggle_linked(cx);
    }
}

// ── Drawing helpers ──────────────────────────────────────────────────────

fn styled(text: impl Into<String>, style: CompactStyle, color: Color32) -> RichText {
    RichText::new(text)
        .text_style(style.text_style())
        .color(color)
}

fn label(ui: &mut Ui, text: impl Into<String>, style: CompactStyle, color: Color32) -> Response {
    ui.add(egui::Label::new(styled(text, style, color)).selectable(false))
}

fn text_width(ui: &Ui, text: &str, style: CompactStyle) -> f32 {
    ui.painter()
        .layout_no_wrap(text.to_owned(), style.font_id(), Color32::WHITE)
        .size()
        .x
}

fn button_width(ui: &Ui, text: &str) -> f32 {
    text_width(ui, text, CompactStyle::ControlLabel) + 2.0 * ui.spacing().button_padding.x
}

/// A control with a fixed `id`, so it can be an overlay's invoker that
/// focus returns to, named `name` for assistive technology and tests. The
/// fixed response carries no widget info of its own, so it is named once.
fn control(ui: &mut Ui, button: egui::Button<'_>, id: Id, name: &str, enabled: bool) -> Response {
    let drawn = ui.add_enabled(enabled, button);
    drawn.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, name));
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    ui.interact(drawn.rect, id, sense).union(drawn)
}

/// A bordered toolbar action (`Action /` in the frames).
fn action(ui: &mut Ui, text: &str, id: Id, enabled: bool, palette: &Palette) -> Response {
    let button = egui::Button::new(styled(
        text,
        CompactStyle::ControlLabel,
        palette.color(Token::Ink),
    ))
    .min_size(Vec2::new(0.0, CONTROL_HEIGHT));
    control(ui, button, id, text, enabled)
}

/// A frameless action (the stepper arrows, ‹ Previous, Next ›).
fn plain(ui: &mut Ui, text: &str, name: &str, enabled: bool, palette: &Palette) -> Response {
    let button = egui::Button::new(styled(
        text,
        CompactStyle::ControlLabel,
        palette.color(Token::Ink),
    ))
    .frame(false)
    .min_size(Vec2::new(0.0, CONTROL_HEIGHT));
    let response = ui.add_enabled(enabled, button);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, name));
    response
}

fn keep_both_id() -> Id {
    Id::unique("fd.compare.keep_both")
}

fn queue_b_id() -> Id {
    Id::unique("fd.compare.queue_b")
}

fn only_in_id(b: bool) -> Id {
    Id::unique(("fd.compare.only_in", b))
}

// ── Toolbar ──────────────────────────────────────────────────────────────

/// The workspace's own toolbar items, before the shared ones: ‹ Findings,
/// the group label and pair, the candidate stepper, Keep both and Queue B
/// for cleanup….
pub fn toolbar(ui: &mut Ui, cx: &mut WorkspaceCx<'_>, intents: &mut IntentQueue) {
    ensure_glyphs(ui.ctx());
    adopt_preferences(ui.ctx(), cx);
    pin(cx.compare);
    let palette = theme::palette(mode_of(ui.ctx()));
    let view = current_view(cx);
    if let Some(view) = &view {
        if cx.compare.candidates != view.candidates {
            cx.compare.candidates.clone_from(&view.candidates);
        }
        cx.compare.candidate_index = view.candidate_index;
    }

    let back = action(ui, BACK_NAME, Id::unique("fd.compare.back"), true, palette);
    if cx.input_enabled && back.clicked() {
        back_to_findings(intents);
    }

    // The trailing items and the shell's shared ones are measured first,
    // so what does not fit is dropped from the leading side (frame
    // `29:3648` drops the group label and the pair).
    let spacing = ui.spacing().item_spacing.x;
    let review = format!("Review cleanup ({})", cx.queue.queued_count());
    let shared = CONTROL_HEIGHT + button_width(ui, &review) + 2.0 * spacing;
    let trailing =
        button_width(ui, KEEP_BOTH_NAME) + button_width(ui, QUEUE_B_NAME) + 2.0 * spacing;
    if let Some(view) = &view {
        leading(ui, cx, intents, view, shared + trailing, palette);
    }
    let room = ui.available_width() - shared - trailing;
    if room > 0.0 {
        ui.add_space(room);
    }

    let evidence = shown(cx.compare).map(|(request, _)| request);
    let stale = shown_request(cx.compare)
        .and_then(|request| projections::stale_note(&request, cx.queue))
        .is_some();
    let ready = view.is_some() && evidence.is_some() && !stale;
    let keep = action(ui, KEEP_BOTH_NAME, keep_both_id(), ready, palette);
    let keep = keep.on_disabled_hover_text(disabled_reason(view.is_some(), stale));
    let queue = action(ui, QUEUE_B_NAME, queue_b_id(), ready, palette);
    let queue = queue.on_disabled_hover_text(disabled_reason(view.is_some(), stale));
    let (Some(pair), Some(reviewed)) = (cx.compare.pair, evidence) else {
        return;
    };
    if cx.input_enabled && ready && keep.clicked() {
        intents.push(UiIntent::Review {
            command: ReviewCommand::KeepBoth {
                file: pair.target,
                counterpart: pair.counterpart,
                reviewed,
            },
            invoker: Some(keep_both_id()),
        });
    }
    if cx.input_enabled && ready && queue.clicked() {
        intents.push(UiIntent::Review {
            command: ReviewCommand::RequestQueue {
                file: pair.target,
                counterpart: pair.counterpart,
                origin: QueueOrigin::Compare,
                reviewed: Some(reviewed),
            },
            invoker: Some(queue_b_id()),
        });
    }
}

const fn disabled_reason(has_pair: bool, stale: bool) -> &'static str {
    if !has_pair {
        "No pair is shown"
    } else if stale {
        "The evidence changed; compare again first"
    } else {
        "Waiting for the comparison"
    }
}

/// The group label, the pair and the candidate stepper; `reserved` is the
/// width the trailing and shared items need.
fn leading(
    ui: &mut Ui,
    cx: &WorkspaceCx<'_>,
    intents: &mut IntentQueue,
    view: &CompareView,
    reserved: f32,
    palette: &Palette,
) {
    let spacing = ui.spacing().item_spacing.x;
    let stepper_label = projections::candidate_label(view.candidate_index, view.candidates.len());
    let stepper = 2.0 * button_width(ui, "›")
        + text_width(ui, &stepper_label, CompactStyle::Supporting)
        + 2.0 * spacing;
    let labels = text_width(ui, &view.group_label, CompactStyle::GroupEmphasis)
        + text_width(ui, &view.pair_label, CompactStyle::ControlLabel)
        + 2.0 * spacing;
    if ui.available_width() - reserved - stepper >= labels {
        label(
            ui,
            &view.group_label,
            CompactStyle::GroupEmphasis,
            palette.color(Token::Ink),
        );
        label(
            ui,
            &view.pair_label,
            CompactStyle::ControlLabel,
            palette.color(Token::Ink),
        );
    }
    let index = view.candidate_index;
    let count = view.candidates.len();
    let previous = plain(ui, "‹", PREVIOUS_CANDIDATE_NAME, index > 0, palette)
        .on_hover_text("Previous candidate (Alt+Left)");
    label(
        ui,
        stepper_label,
        CompactStyle::Supporting,
        palette.color(Token::Muted),
    );
    let next = plain(ui, "›", NEXT_CANDIDATE_NAME, index + 1 < count, palette)
        .on_hover_text("Next candidate (Alt+Right)");
    if cx.input_enabled && previous.clicked() && index > 0 {
        step_candidate(cx, intents, index - 1);
    }
    if cx.input_enabled && next.clicked() && index + 1 < count {
        step_candidate(cx, intents, index + 1);
    }
}

// ── Body ─────────────────────────────────────────────────────────────────

/// The workspace body below the toolbar: evidence row, change row, the
/// pinned file identities and the panes; or the no-pair explanation.
pub fn show(ui: &mut Ui, cx: &mut WorkspaceCx<'_>, intents: &mut IntentQueue) {
    ensure_glyphs(ui.ctx());
    pin(cx.compare);
    keys(ui, cx, intents);
    let palette = theme::palette(mode_of(ui.ctx()));
    let Some(view) = current_view(cx) else {
        no_pair(ui, palette);
        return;
    };
    if cx.input_enabled {
        resubmit(ui, cx.compare, intents);
    }
    let current = shown(cx.compare);
    let aligned = current.as_ref().and_then(|(_, c)| ready(c));
    if let Some(aligned) = aligned
        && !aligned.hunks.is_empty()
    {
        cx.compare.active_change = cx.compare.active_change.min(aligned.hunks.len() - 1);
    }

    evidence_row(ui, cx, intents, &view, aligned, palette);
    ui.add_space(GAP);
    change_row(ui, cx, aligned, palette);
    ui.add_space(GAP);
    identities(ui, cx, &view, palette);
    diff_view::show(ui, cx.compare, cx.input_enabled);
    if cx.preferences.compare_split_ratio.to_bits() != cx.compare.split_ratio.to_bits() {
        cx.preferences.compare_split_ratio = cx.compare.split_ratio;
    }
    if let Some(MenuKind::OnlyIn { b }) = cx.menu {
        only_in_menu(ui, cx, intents, b, aligned, palette);
    } else {
        ui.ctx()
            .data_mut(|d| d.remove::<bool>(Id::unique("fd.compare.menu_drawn")));
    }
}

/// Least egui time between two submissions of the same request, so an
/// engine that answers Busy is asked again about four times a second
/// rather than every frame.
const RETRY_SECONDS: f64 = 0.25;

/// Asks again for the pinned request when the slot holds another one
/// (Review unique text's, or the other mode's). Callers skip it under an
/// overlay, whose own request this would supersede.
///
/// An admitted submission puts its key in the slot at once
/// (`events::note_submitted`), so the slot matching is the only success
/// signal; while it does not match, the same request is sent at most once
/// per [`RETRY_SECONDS`]. A new request (another mode or pair) goes at
/// once. Rejected: retrying on the next engine event, because a Busy
/// engine may have no event of ours left to send.
fn resubmit(ui: &Ui, compare: &CompareState, intents: &mut IntentQueue) {
    let Some(request) = shown_request(compare) else {
        return;
    };
    if compare.comparison.matches(&request) {
        return;
    }
    let ctx = ui.ctx();
    let key = Id::unique("fd.compare.last_submit");
    let now = ctx.input(|i| i.time);
    if let Some((sent, at)) = ctx.data(|d| d.get_temp::<(ComparisonRequest, f64)>(key))
        && sent == request
        && now - at < RETRY_SECONDS
    {
        // Wake up for the retry even when nothing else repaints.
        let wait = RETRY_SECONDS - (now - at);
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(wait.max(0.0)));
        return;
    }
    ctx.data_mut(|d| d.insert_temp(key, (request, now)));
    intents.push(UiIntent::Submit(EngineRequest::LoadComparison(request)));
}

/// The disabled state: no pair exists (§11.2).
fn no_pair(ui: &mut Ui, palette: &Palette) {
    let rect = ui.available_rect_before_wrap();
    ui.painter().rect(
        rect,
        theme::CORNER_RADIUS,
        palette.color(Token::Surface),
        Stroke::new(theme::BORDER_WIDTH, palette.color(Token::Line)),
        StrokeKind::Inside,
    );
    ui.scope_builder(UiBuilder::new().max_rect(rect), |ui| {
        ui.with_layout(Layout::top_down(Align::Center), |ui| {
            ui.add_space((rect.height() / 2.0 - 40.0).max(0.0));
            label(
                ui,
                NO_PAIR_TITLE,
                CompactStyle::Title,
                palette.color(Token::Ink),
            );
            ui.add(
                egui::Label::new(styled(
                    NO_PAIR_BODY,
                    CompactStyle::Body,
                    palette.color(Token::Muted),
                ))
                .selectable(false)
                .wrap(),
            );
        });
    });
    ui.advance_cursor_after_rect(rect);
}

/// Relation, score, the two Only-in menus, the stale marker and the
/// reminder (frame `13:1372`).
fn evidence_row(
    ui: &mut Ui,
    cx: &mut WorkspaceCx<'_>,
    intents: &mut IntentQueue,
    view: &CompareView,
    aligned: Option<&AlignedComparison>,
    palette: &Palette,
) {
    ui.allocate_ui_with_layout(
        Vec2::new(ui.available_width(), ROW_HEIGHT),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing.x = 20.0;
            let (relation, color) = match view.relation {
                Some(kind @ (RelationKind::SimilarText | RelationKind::Containment)) => {
                    (projections::kind_label(kind), Token::Amber)
                }
                Some(kind) => (projections::kind_label(kind), Token::Green),
                None => ("No direct match", Token::Muted),
            };
            label(
                ui,
                relation,
                CompactStyle::GroupEmphasis,
                palette.color(color),
            );
            if let Some(score) = &view.score {
                label(
                    ui,
                    score,
                    CompactStyle::ControlLabel,
                    palette.color(Token::Ink),
                );
            }
            for b in [false, true] {
                only_in_button(ui, cx, intents, b, aligned, palette);
            }
            if let Some(note) = shown_request(cx.compare)
                .and_then(|request| projections::stale_note(&request, cx.queue))
            {
                stale_marker(ui, cx, intents, &note, palette);
            }
            let reminder = text_width(ui, READ_ONLY_NOTE, CompactStyle::Supporting);
            if ui.available_width() >= reminder {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    label(
                        ui,
                        READ_ONLY_NOTE,
                        CompactStyle::Supporting,
                        palette.color(Token::Muted),
                    );
                });
            }
        },
    );
}

fn only_in_button(
    ui: &mut Ui,
    cx: &WorkspaceCx<'_>,
    intents: &mut IntentQueue,
    b: bool,
    aligned: Option<&AlignedComparison>,
    palette: &Palette,
) {
    let unique = aligned.map(|a| if b { &a.only_in_b } else { &a.only_in_a });
    let caption = unique.map_or_else(
        || format!("Only in {}: …", letter(b)),
        |u| diff_view::only_in_caption(letter(b), u.count(), u.normalized),
    );
    let name = unique.map_or_else(
        || format!("Only in {}: … ▾", letter(b)),
        |u| only_in_name(b, u.count(), u.normalized),
    );
    let color = palette.color(if b { Token::DiffB } else { Token::DiffA });
    let chevron = glyph_text(ui.ctx(), Icon::Expanded);
    let button = egui::Button::new(styled(
        format!("{caption} {chevron}"),
        CompactStyle::Supporting,
        color,
    ))
    .frame(false);
    let response = control(ui, button, only_in_id(b), &name, aligned.is_some());
    if cx.input_enabled && aligned.is_some() && response.clicked() {
        intents.push(UiIntent::ToggleMenu {
            kind: MenuKind::OnlyIn { b },
            invoker: Some(only_in_id(b)),
        });
    }
}

/// The stale-evidence marker: the evidence the panes show is no longer the
/// published view's (spec §11.5 rule 1: never show stale results as
/// current), with the way back to current evidence.
fn stale_marker(
    ui: &mut Ui,
    cx: &WorkspaceCx<'_>,
    intents: &mut IntentQueue,
    note: &str,
    palette: &Palette,
) {
    ui.spacing_mut().item_spacing.x = 8.0;
    egui::Frame::new()
        .fill(palette.color(Token::AmberSoft))
        .corner_radius(theme::CORNER_RADIUS)
        .inner_margin(egui::Margin::symmetric(6, 2))
        .show(ui, |ui| {
            label(
                ui,
                STALE_BADGE,
                CompactStyle::ControlLabel,
                palette.color(Token::Amber),
            );
        });
    label(
        ui,
        note,
        CompactStyle::Supporting,
        palette.color(Token::Ink),
    );
    let again = action(
        ui,
        COMPARE_AGAIN_NAME,
        Id::unique("fd.compare.again"),
        true,
        palette,
    );
    if cx.input_enabled
        && again.clicked()
        && let Some(pair) = cx.compare.pair
    {
        intents.push(UiIntent::OpenCompare {
            target: pair.target,
            counterpart: pair.counterpart,
            return_row: cx.compare.return_row,
        });
    }
    ui.spacing_mut().item_spacing.x = 20.0;
}

/// Change position, ‹ Previous and Next ›, Wrap, Linked scroll and the
/// Original | Normalized switch (frame `13:1379`).
fn change_row(
    ui: &mut Ui,
    cx: &mut WorkspaceCx<'_>,
    aligned: Option<&AlignedComparison>,
    palette: &Palette,
) {
    ui.allocate_ui_with_layout(
        Vec2::new(ui.available_width(), ROW_HEIGHT),
        Layout::left_to_right(Align::Center),
        |ui| {
            let changes = aligned.map_or(0, |a| a.hunks.len());
            let active = cx.compare.active_change;
            label(
                ui,
                projections::change_label(active, changes),
                CompactStyle::Supporting,
                palette.color(Token::Muted),
            );
            let previous = plain(
                ui,
                PREVIOUS_CHANGE_NAME,
                PREVIOUS_CHANGE_NAME,
                active > 0,
                palette,
            )
            .on_hover_text("Previous change (Alt+Up)");
            let next = plain(
                ui,
                NEXT_CHANGE_NAME,
                NEXT_CHANGE_NAME,
                active + 1 < changes,
                palette,
            )
            .on_hover_text("Next change (Alt+Down)");
            if cx.input_enabled && previous.clicked() {
                set_change(cx.compare, false);
            }
            if cx.input_enabled && next.clicked() {
                set_change(cx.compare, true);
            }
            ui.add(egui::Separator::default().vertical().spacing(8.0));
            let wrap = check_box(
                ui,
                cx.compare.wrap,
                Some(WRAP_NAME),
                WRAP_NAME,
                cx.input_enabled,
            )
            .on_hover_text("Toggle wrap (Alt+Z)");
            if cx.input_enabled && wrap.clicked() {
                toggle_wrap(cx);
            }
            let linked = check_box(
                ui,
                cx.compare.linked_scroll,
                Some(LINKED_SCROLL_NAME),
                LINKED_SCROLL_NAME,
                cx.input_enabled,
            )
            .on_hover_text("Toggle linked scroll (Alt+L)");
            if cx.input_enabled && linked.clicked() {
                toggle_linked(cx);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                view_mode(ui, cx, palette);
            });
        },
    );
}

/// The bordered Original | Normalized switch. Choosing a mode only sets
/// it; [`show`] then asks for the pinned request in that mode.
fn view_mode(ui: &mut Ui, cx: &mut WorkspaceCx<'_>, palette: &Palette) {
    egui::Frame::new()
        .fill(palette.color(Token::Surface))
        .stroke(Stroke::new(theme::BORDER_WIDTH, palette.color(Token::Line)))
        .corner_radius(theme::CORNER_RADIUS)
        .inner_margin(1)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            // Right to left: Normalized is drawn first.
            for (mode, name) in [
                (CompareMode::Normalized, NORMALIZED_NAME),
                (CompareMode::Original, ORIGINAL_NAME),
            ] {
                let selected = cx.compare.mode == mode;
                let color = palette.color(if selected {
                    Token::Primary
                } else {
                    Token::Muted
                });
                let mut button = egui::Button::new(styled(name, CompactStyle::ControlLabel, color))
                    .min_size(Vec2::new(0.0, 26.0))
                    .corner_radius(0.0);
                button = if selected {
                    button.fill(palette.color(Token::PrimarySoft))
                } else {
                    button.frame(false)
                };
                let response = ui.add_enabled(cx.input_enabled, button);
                response.widget_info(|| {
                    egui::WidgetInfo::selected(egui::Role::Button, cx.input_enabled, selected, name)
                });
                if cx.input_enabled && response.clicked() && !selected {
                    cx.compare.mode = mode;
                }
            }
        });
}

/// The pinned file identity above each pane, on the panes' own split.
fn identities(ui: &mut Ui, cx: &WorkspaceCx<'_>, view: &CompareView, palette: &Palette) {
    let area = ui.available_rect_before_wrap();
    let ratio = cx.compare.split_ratio.clamp(SPLIT_RANGE.0, SPLIT_RANGE.1);
    let usable = (area.width() - PANE_GAP).max(2.0);
    let split = area.left() + (usable * ratio).round();
    let top = area.top();
    let bottom = top + IDENTITY_HEIGHT;
    let rects = [
        Rect::from_min_max(area.min, egui::pos2(split, bottom)),
        Rect::from_min_max(
            egui::pos2(split + PANE_GAP, top),
            egui::pos2(area.right(), bottom),
        ),
    ];
    for (rect, identity) in rects.into_iter().zip([&view.a, &view.b]) {
        pinned_identity(ui, rect, identity, palette);
    }
    ui.advance_cursor_after_rect(Rect::from_min_max(
        area.min,
        egui::pos2(area.right(), bottom),
    ));
}

/// One pinned identity (frame `13:1410`): icon, name, path with size and
/// date when they fit, and the side's role.
fn pinned_identity(ui: &mut Ui, rect: Rect, identity: &PaneIdentity, palette: &Palette) {
    // One accessibility node for the whole row, its texts painted. Rejected:
    // a label widget per text, which puts a second node named exactly like
    // the file next to Review unique text's own when that popup opens over
    // Compare, so a query for the popup's file name is no longer unique
    // (`tests/review_popups.rs` asks for it by name). One node per row is
    // also how the diff widget names its rows.
    let response = ui.interact(
        rect,
        Id::unique(("fd.compare.identity", identity.file)),
        Sense::hover(),
    );
    let name = projections::identity_name(identity);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &name));

    let painter = ui.painter();
    painter.rect(
        rect,
        egui::CornerRadius {
            nw: theme::CORNER_RADIUS,
            ne: theme::CORNER_RADIUS,
            sw: 0,
            se: 0,
        },
        palette.color(Token::Code),
        Stroke::new(theme::BORDER_WIDTH, palette.color(Token::Line)),
        StrokeKind::Inside,
    );
    let inner = rect.shrink(8.0);
    let ink = palette.color(Token::Ink);
    let muted = palette.color(Token::Muted);
    let role = painter.layout_no_wrap(
        identity.role.clone(),
        CompactStyle::Supporting.font_id(),
        ink,
    );
    let role_left = inner.right() - role.size().x;
    painter.galley(
        egui::pos2(role_left, inner.center().y - role.size().y / 2.0),
        role,
        ink,
    );
    let icon = glyph_text(ui.ctx(), Icon::Document);
    if !icon.is_empty() {
        painter.text(
            egui::pos2(inner.left() + 10.0, inner.center().y),
            Align2::CENTER_CENTER,
            icon,
            egui::FontId::new(18.0, CompactStyle::ControlLabel.font_id().family),
            ink,
        );
    }
    let left = inner.left() + 28.0;
    let width = (role_left - 8.0 - left).max(0.0);
    let detail = format!(
        "{}{}{}",
        identity.path,
        projections::IDENTITY_SEPARATOR,
        identity.size_and_date
    );
    // The size and date go first when the row is narrow (frame `29:3648`).
    let path = if text_width(ui, &detail, CompactStyle::PathDetail) <= width {
        detail
    } else {
        identity.path.clone()
    };
    for (text, style, color, top) in [
        (identity.name.clone(), CompactStyle::GroupEmphasis, ink, 0.0),
        (path, CompactStyle::PathDetail, muted, 20.0),
    ] {
        let mut job = LayoutJob::single_section(
            text,
            TextFormat {
                font_id: style.font_id(),
                line_height: Some(style.line_height()),
                color,
                ..TextFormat::default()
            },
        );
        job.wrap = egui::text::TextWrapping::truncate_at_width(width);
        let galley = ui.painter().layout_job(job);
        ui.painter()
            .galley(egui::pos2(left, inner.top() + top), galley, color);
    }
}

// ── The Only-in menus ────────────────────────────────────────────────────

/// Draws the "Only in A/B ▾" menu the overlay slot holds.
fn only_in_menu(
    ui: &Ui,
    cx: &mut WorkspaceCx<'_>,
    intents: &mut IntentQueue,
    b: bool,
    aligned: Option<&AlignedComparison>,
    palette: &Palette,
) {
    let ctx = ui.ctx().clone();
    let drawn_key = Id::unique("fd.compare.menu_drawn");
    let mut first = ctx.data(|d| d.get_temp::<bool>(drawn_key)) != Some(b);
    ctx.data_mut(|d| d.insert_temp(drawn_key, b));
    let anchor = ctx
        .read_response(only_in_id(b))
        .map_or(Rect::NOTHING, |r| r.rect);
    let frame = egui::Frame::new()
        .fill(palette.color(Token::Surface))
        .stroke(Stroke::new(theme::BORDER_WIDTH, palette.color(Token::Line)))
        .corner_radius(theme::CORNER_RADIUS)
        .shadow(theme::shadow(mode_of(&ctx)))
        .inner_margin(egui::Margin::symmetric(1, 4));
    let unique = aligned.map(|a| if b { &a.only_in_b } else { &a.only_in_a });
    let has_lines = unique.is_some_and(|u| u.count() > 0);
    let area = egui::Area::new(Id::unique("fd.compare.only_in_menu"))
        .order(egui::Order::Foreground)
        .constrain(true)
        .fixed_pos(anchor.left_bottom() + Vec2::new(0.0, 2.0))
        .show(&ctx, |ui| {
            frame.show(ui, |ui| {
                ui.set_width(MENU_WIDTH - 2.0);
                ui.spacing_mut().item_spacing.y = 0.0;
                let jump = menu_item(ui, &jump_item_name(b), has_lines, palette);
                if std::mem::take(&mut first) {
                    jump.request_focus();
                }
                if jump.clicked()
                    && let Some(aligned) = aligned
                {
                    intents.push(UiIntent::CloseOverlay);
                    if let Some(change) = change_holding_first_unique(aligned, b) {
                        cx.compare.active_change = change;
                    }
                }
                if menu_item(ui, COPY_LINES_NAME, has_lines, palette).clicked()
                    && let Some(aligned) = aligned
                {
                    let text = if b {
                        aligned.copy_only_in_b()
                    } else {
                        aligned.copy_only_in_a()
                    };
                    intents.push(UiIntent::CloseOverlay);
                    intents.push(UiIntent::CopyText(text.text));
                }
                let review = unique_subject(cx.compare, b);
                if menu_item(ui, REVIEW_UNIQUE_NAME, review.is_some(), palette).clicked()
                    && let Some(subject) = review
                {
                    intents.push(UiIntent::CloseOverlay);
                    intents.push(UiIntent::OpenDialog {
                        kind: DialogKind::UniqueTextReview,
                        subject: DialogSubject::UniqueText(subject),
                        invoker: Some(only_in_id(b)),
                    });
                }
            });
        });
    // A press outside the menu and its button closes it; on the button the
    // button's own toggle closes it.
    let pressed_outside = ctx.input(|i| {
        i.pointer.any_pressed()
            && i.pointer
                .interact_pos()
                .is_some_and(|pos| !area.response.rect.contains(pos) && !anchor.contains(pos))
    });
    if pressed_outside {
        intents.push(UiIntent::CloseOverlay);
    }
}

/// Jump to first line: the change holding the side's first unique line, so
/// the widget brings both panes to it. A unique line outside every change
/// (moved text) jumps to the next change after it.
fn change_holding_first_unique(aligned: &AlignedComparison, b: bool) -> Option<usize> {
    let unique = if b {
        &aligned.only_in_b
    } else {
        &aligned.only_in_a
    };
    let line = *unique.lines.first()?;
    let row = aligned
        .rows
        .iter()
        .position(|row| if b { row.b } else { row.a } == Some(line))?;
    aligned
        .hunks
        .iter()
        .position(|hunk| hunk.rows.contains(&row))
        .or_else(|| aligned.hunks.iter().position(|hunk| hunk.rows.start >= row))
        .or_else(|| aligned.hunks.len().checked_sub(1))
}

/// The Review unique text subject for side `b` (B, the target, against A;
/// or A against B), bound to the evidence on screen. Only B's review names
/// the comparison: the engine binds a reviewed request to its own
/// `(target, counterpart)` order, so A's review carries none.
fn unique_subject(compare: &CompareState, b: bool) -> Option<UniqueTextSubject> {
    let pair = compare.pair?;
    let request = shown_request(compare)?;
    let reviewed = shown(compare).map(|(id, _)| id);
    Some(if b {
        UniqueTextSubject {
            scan: request.scan,
            target: pair.target,
            counterpart: pair.counterpart,
            target_snapshot: request.target_snapshot,
            counterpart_snapshot: request.counterpart_snapshot,
            rules: request.rules,
            reviewed,
            origin: QueueOrigin::Compare,
        }
    } else {
        UniqueTextSubject {
            scan: request.scan,
            target: pair.counterpart,
            counterpart: pair.target,
            target_snapshot: request.counterpart_snapshot,
            counterpart_snapshot: request.target_snapshot,
            rules: request.rules,
            reviewed: None,
            origin: QueueOrigin::Compare,
        }
    })
}

/// A 28-high menu row.
fn menu_item(ui: &mut Ui, name: &str, enabled: bool, palette: &Palette) -> Response {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), MENU_ITEM_HEIGHT), sense);
    let owned = name.to_owned();
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, &owned));
    let painter = ui.painter();
    if enabled && (response.hovered() || response.has_focus()) {
        painter.rect_filled(
            rect,
            theme::CORNER_RADIUS,
            palette.color(Token::PrimarySoft),
        );
    }
    if response.has_focus() {
        painter.rect_stroke(
            rect,
            theme::CORNER_RADIUS,
            Stroke::new(theme::BORDER_WIDTH, palette.color(Token::Focus)),
            StrokeKind::Inside,
        );
    }
    painter.text(
        rect.left_center() + Vec2::new(10.0, 0.0),
        Align2::LEFT_CENTER,
        name,
        CompactStyle::Table.font_id(),
        palette.color(if enabled { Token::Ink } else { Token::Quiet }),
    );
    response
}
