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
//!   leaves the mechanics to the executor). Toolbar actions that still do
//!   not fit move into the More actions menu (`ToolbarFit` states the
//!   order), and the change row wraps between its groups (`change_row`).
//! - The identity rows take keyboard focus; focus shows the full path and
//!   Ctrl+Shift+C copies it (`identities`).
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
use crate::ui::dialogs::matching_rules::check_box;
use crate::ui::shell::{CONTROL_HEIGHT, GAP};
use crate::ui::theme::{self, CompactStyle, Icon, Palette, Token};
use crate::ui::widgets::dialog_host::mode_of;
use crate::ui::widgets::diff_view;
use crate::ui::{WorkspaceCx, glyphs};

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
/// The toolbar's overflow button (spec §11.3 collapse step 3), shown only
/// while an action has moved into its menu.
pub const MORE_ACTIONS_NAME: &str = "More actions";
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

/// The Only-in menu's accessible name: its visible count text and "▾".
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

/// `icon` as text in the current text colour, or "" when the glyph
/// registry cannot draw it on `ctx` (see [`glyphs::text`]).
fn glyph_text(ctx: &egui::Context, icon: Icon) -> String {
    glyphs::text(ctx, icon).unwrap_or_default()
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

    let evidence = shown(cx.compare).map(|(request, _)| request);
    let stale = shown_request(cx.compare)
        .and_then(|request| projections::stale_note(&request, cx.queue))
        .is_some();
    let ready = view.is_some() && evidence.is_some() && !stale;
    let fit = fit_toolbar(ui, view.as_ref());

    if let Some(view) = &view {
        leading(ui, cx, intents, view, fit, palette);
    }
    // The trailing items sit at the budget's right edge, as frame
    // `13:1304` places Keep both and Queue B beside Review cleanup. The
    // budget is the shell's (`shell::toolbar_row`), which already excludes
    // the shared items, so nothing else is subtracted here.
    let room = ui.available_width() - fit.trailing_width(ui);
    if room > 0.0 {
        ui.add_space(room);
    }

    // Toolbar buttons act only while no overlay is open; menu items act
    // while their own menu is the overlay, so each source checks its own.
    let mut decision = None;
    if fit.keep_both() {
        let keep = action(ui, KEEP_BOTH_NAME, keep_both_id(), ready, palette)
            .on_disabled_hover_text(disabled_reason(view.is_some(), stale));
        if cx.input_enabled && keep.clicked() {
            decision = Some((Decision::KeepBoth, keep_both_id()));
        }
    }
    if fit.queue_b() {
        let queue = action(ui, QUEUE_B_NAME, queue_b_id(), ready, palette)
            .on_disabled_hover_text(disabled_reason(view.is_some(), stale));
        if cx.input_enabled && queue.clicked() {
            decision = Some((Decision::QueueB, queue_b_id()));
        }
    }
    if fit.overflows() {
        let more = more_button(ui, palette);
        if cx.input_enabled && more.clicked() {
            intents.push(UiIntent::ToggleMenu {
                kind: MenuKind::Actions,
                invoker: Some(more_id()),
            });
        }
    }
    if cx.menu == Some(MenuKind::Actions) {
        if let Some(chosen) = overflow_menu(ui, cx, intents, fit, ready, palette) {
            decision = Some((chosen, more_id()));
        }
    } else {
        ui.ctx()
            .data_mut(|d| d.remove::<bool>(Id::unique("fd.compare.more_drawn")));
    }

    let (Some((decision, invoker)), Some(pair), Some(reviewed), true) =
        (decision, cx.compare.pair, evidence, ready)
    else {
        return;
    };
    let command = match decision {
        Decision::KeepBoth => ReviewCommand::KeepBoth {
            file: pair.target,
            counterpart: pair.counterpart,
            reviewed,
        },
        Decision::QueueB => ReviewCommand::RequestQueue {
            file: pair.target,
            counterpart: pair.counterpart,
            origin: QueueOrigin::Compare,
            reviewed: Some(reviewed),
        },
    };
    intents.push(UiIntent::Review {
        command,
        invoker: Some(invoker),
    });
}

/// The two pair decisions the toolbar or its overflow menu sends.
#[derive(Clone, Copy)]
enum Decision {
    KeepBoth,
    QueueB,
}

/// Which of the toolbar's optional items are drawn in its budget (spec
/// §11.3 collapse step 3). What is not drawn is either dropped (the group
/// label and pair, which are text and repeat the identity header) or moved
/// into the "More actions" menu ([`MORE_ACTIONS_NAME`]).
///
/// Overflow priority, decided by the executor (T40; the spec and frames
/// give none): the labels go first, as frame `29:3648` drops them; then
/// actions move into the menu from the trailing end, Queue B for cleanup…
/// before Keep both, and the candidate stepper last. ‹ Findings never
/// moves. Moving from the trailing end keeps the visible actions a prefix
/// of the full row, so the menu always holds "the rest" in reading order.
/// Rejected: moving the stepper first, as the narrowest item that would
/// free little room while it is the only pointer route between candidates
/// besides Alt+Left/Right.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum ToolbarFit {
    /// Everything fits.
    All,
    /// The group label and pair are dropped.
    NoLabels,
    /// Queue B for cleanup… has moved into the menu too.
    QueueBMoved,
    /// Keep both as well.
    DecisionsMoved,
    /// The candidate stepper as well: only ‹ Findings and More actions.
    StepperMoved,
}

impl ToolbarFit {
    fn labels(self) -> bool {
        self == Self::All
    }

    fn stepper(self) -> bool {
        self < Self::StepperMoved
    }

    fn keep_both(self) -> bool {
        self < Self::DecisionsMoved
    }

    fn queue_b(self) -> bool {
        self < Self::QueueBMoved
    }

    fn overflows(self) -> bool {
        self >= Self::QueueBMoved
    }

    /// The width of the items drawn after the stretch: the visible
    /// decisions and the More actions button.
    fn trailing_width(self, ui: &Ui) -> f32 {
        let spacing = ui.spacing().item_spacing.x;
        let mut widths = Vec::new();
        if self.keep_both() {
            widths.push(button_width(ui, KEEP_BOTH_NAME));
        }
        if self.queue_b() {
            widths.push(button_width(ui, QUEUE_B_NAME));
        }
        if self.overflows() {
            widths.push(CONTROL_HEIGHT);
        }
        row_width(&widths, spacing)
    }
}

/// `widths` laid side by side with `spacing` between neighbours.
fn row_width(widths: &[f32], spacing: f32) -> f32 {
    widths
        .iter()
        .enumerate()
        .map(|(index, width)| if index == 0 { *width } else { width + spacing })
        .sum()
}

/// Chooses the [`ToolbarFit`] for the width left after ‹ Findings, trying
/// each step of the overflow priority in turn. Widths are measured at the
/// fonts the items draw with; the frameless stepper arrows are measured
/// with button padding they do not draw, which only errs toward room.
fn fit_toolbar(ui: &Ui, view: Option<&CompareView>) -> ToolbarFit {
    let budget = ui.available_width();
    [
        ToolbarFit::All,
        ToolbarFit::NoLabels,
        ToolbarFit::QueueBMoved,
        ToolbarFit::DecisionsMoved,
    ]
    .into_iter()
    .find(|fit| toolbar_need(ui, view, *fit) <= budget)
    .unwrap_or(ToolbarFit::StepperMoved)
}

/// The narrowest budget in which the toolbar shows every item, ‹ Findings
/// included, with nothing dropped or moved into More actions
/// ([`ToolbarFit::All`]): the minimum `shell::toolbar_row` keeps beside the
/// full Review cleanup label before it shortens that label (see its
/// contract). Frame `29:3648` is the case where it does not fit.
#[must_use]
pub fn toolbar_min_width(ui: &Ui, cx: &WorkspaceCx<'_>) -> f32 {
    let view = current_view(cx);
    button_width(ui, BACK_NAME)
        + ui.spacing().item_spacing.x
        + toolbar_need(ui, view.as_ref(), ToolbarFit::All)
}

/// The width the items after ‹ Findings need at `fit`, measured at the
/// fonts they draw with.
fn toolbar_need(ui: &Ui, view: Option<&CompareView>, fit: ToolbarFit) -> f32 {
    let spacing = ui.spacing().item_spacing.x;
    let (labels, stepper) = view.map_or((0.0, 0.0), |view| {
        let stepper_label =
            projections::candidate_label(view.candidate_index, view.candidates.len());
        (
            text_width(ui, &view.group_label, CompactStyle::GroupEmphasis)
                + spacing
                + text_width(ui, &view.pair_label, CompactStyle::ControlLabel),
            row_width(
                &[
                    button_width(ui, "‹"),
                    text_width(ui, &stepper_label, CompactStyle::Supporting),
                    button_width(ui, "›"),
                ],
                spacing,
            ),
        )
    });
    let has_view = view.is_some();
    let mut widths = Vec::new();
    if fit.labels() && has_view {
        widths.push(labels);
    }
    if fit.stepper() && has_view {
        widths.push(stepper);
    }
    let trailing = fit.trailing_width(ui);
    if trailing > 0.0 {
        widths.push(trailing);
    }
    row_width(&widths, spacing)
}

fn more_id() -> Id {
    Id::unique("fd.compare.more")
}

/// The square "⋯" button that opens the overflow menu.
fn more_button(ui: &mut Ui, palette: &Palette) -> Response {
    let button = egui::Button::new(styled(
        glyph_text(ui.ctx(), Icon::RowMenu),
        CompactStyle::ControlLabel,
        palette.color(Token::Ink),
    ))
    .min_size(Vec2::splat(CONTROL_HEIGHT));
    control(ui, button, more_id(), MORE_ACTIONS_NAME, true).on_hover_text("More actions")
}

/// Draws the More actions menu below its button, holding exactly the
/// actions `fit` left out, in toolbar order. Returns the decision chosen
/// this frame, if any; the stepper items act here.
///
/// The menu is the overlay slot's `MenuKind::Actions`, the §11.3 "Actions
/// menu" secondary actions move into. Findings draws the same kind for its
/// own Actions ▾; the two never meet, because each view draws only while
/// it is the workspace and the slot is closed by the host's Esc, an outside
/// press, or an item. Rejected: a Compare-only `MenuKind` variant, which
/// every exhaustive match on the kind (Findings' menu drawer among them)
/// would have to learn.
fn overflow_menu(
    ui: &Ui,
    cx: &WorkspaceCx<'_>,
    intents: &mut IntentQueue,
    fit: ToolbarFit,
    ready: bool,
    palette: &Palette,
) -> Option<Decision> {
    let ctx = ui.ctx().clone();
    // A resize that made everything fit took the button away; a menu with
    // nothing in it must not stay open over the toolbar.
    if !fit.overflows() {
        intents.push(UiIntent::CloseOverlay);
        return None;
    }
    let drawn_key = Id::unique("fd.compare.more_drawn");
    let mut first = ctx.data(|d| d.get_temp::<bool>(drawn_key)).is_none();
    ctx.data_mut(|d| d.insert_temp(drawn_key, true));
    let anchor = ctx
        .read_response(more_id())
        .map_or(Rect::NOTHING, |r| r.rect);
    let frame = egui::Frame::new()
        .fill(palette.color(Token::Surface))
        .stroke(Stroke::new(theme::BORDER_WIDTH, palette.color(Token::Line)))
        .corner_radius(theme::CORNER_RADIUS)
        .shadow(theme::shadow(mode_of(&ctx)))
        .inner_margin(egui::Margin::symmetric(1, 4));
    let index = cx.compare.candidate_index;
    let count = cx.compare.candidates.len();
    let mut chosen = None;
    let area = egui::Area::new(Id::unique("fd.compare.more_menu"))
        .order(egui::Order::Foreground)
        .constrain(true)
        .pivot(Align2::RIGHT_TOP)
        .fixed_pos(anchor.right_bottom() + Vec2::new(0.0, 2.0))
        .show(&ctx, |ui| {
            frame.show(ui, |ui| {
                ui.set_width(MENU_WIDTH - 2.0);
                ui.spacing_mut().item_spacing.y = 0.0;
                let mut focus_first = |response: &Response| {
                    if std::mem::take(&mut first) {
                        response.request_focus();
                    }
                };
                if !fit.stepper() {
                    let previous = menu_item(ui, PREVIOUS_CANDIDATE_NAME, index > 0, palette);
                    focus_first(&previous);
                    if previous.clicked() && index > 0 {
                        intents.push(UiIntent::CloseOverlay);
                        step_candidate(cx, intents, index - 1);
                    }
                    let next = menu_item(ui, NEXT_CANDIDATE_NAME, index + 1 < count, palette);
                    if next.clicked() && index + 1 < count {
                        intents.push(UiIntent::CloseOverlay);
                        step_candidate(cx, intents, index + 1);
                    }
                }
                for (shown, name, decision) in [
                    (fit.keep_both(), KEEP_BOTH_NAME, Decision::KeepBoth),
                    (fit.queue_b(), QUEUE_B_NAME, Decision::QueueB),
                ] {
                    if shown {
                        continue;
                    }
                    let item = menu_item(ui, name, ready, palette);
                    focus_first(&item);
                    if item.clicked() && ready {
                        intents.push(UiIntent::CloseOverlay);
                        chosen = Some(decision);
                    }
                }
            });
        });
    let pressed_outside = ctx.input(|i| {
        i.pointer.any_pressed()
            && i.pointer
                .interact_pos()
                .is_some_and(|pos| !area.response.rect.contains(pos) && !anchor.contains(pos))
    });
    if pressed_outside {
        intents.push(UiIntent::CloseOverlay);
    }
    chosen
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

/// The group label and pair, and the candidate stepper, each when `fit`
/// keeps it in the toolbar.
fn leading(
    ui: &mut Ui,
    cx: &WorkspaceCx<'_>,
    intents: &mut IntentQueue,
    view: &CompareView,
    fit: ToolbarFit,
    palette: &Palette,
) {
    if fit.labels() {
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
    if !fit.stepper() {
        return;
    }
    let index = view.candidate_index;
    let count = view.candidates.len();
    let previous = plain(ui, "‹", PREVIOUS_CANDIDATE_NAME, index > 0, palette)
        .on_hover_text("Previous candidate (Alt+Left)");
    label(
        ui,
        projections::candidate_label(index, count),
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

    // The rows stack with exactly `GAP` between them and the identity
    // header sits flush on the panes (frame `13:1304`: rows at 52 and 88,
    // the paired region at 124). egui's `item_spacing.y`, added after each
    // allocation, would push the panes 9 units down; the rows are allocated
    // with none, and the panes get the theme's spacing back.
    let item_spacing = ui.spacing().item_spacing;
    ui.spacing_mut().item_spacing.y = 0.0;
    evidence_row(ui, cx, intents, &view, aligned, palette);
    ui.add_space(GAP);
    change_row(ui, cx, aligned, palette);
    ui.add_space(GAP);
    let header = identity_band(ui);
    ui.spacing_mut().item_spacing = item_spacing;
    // The identities take their focus nodes before the panes, so Tab
    // reaches the header before the panes, in reading order; they are
    // painted after the panes because they show the notices this frame's
    // panes report, a copy made this frame included. During a splitter
    // drag the nodes therefore follow the split one frame late.
    let focus = identity_nodes(ui, cx, &view, header);
    let notices = diff_view::show(ui, cx.compare, cx.input_enabled);
    identities(ui, cx, intents, &view, header, &notices, &focus, palette);
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

/// The change row's three groups, in reading order. Each stays whole on
/// one line; the row wraps between groups, never inside one.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ChangeGroup {
    /// Change position, ‹ Previous and Next ›.
    Navigation,
    /// Wrap and Linked scroll, after a divider when it follows the
    /// navigation on the same line.
    Toggles,
    /// The Original | Normalized switch, at the line's trailing edge.
    Mode,
}

/// Change position, ‹ Previous and Next ›, Wrap, Linked scroll and the
/// Original | Normalized switch (frame `13:1379`).
///
/// The row is width-checked (T40): when its three groups do not fit one
/// line, it wraps between groups onto further 28-high lines, `GAP` apart,
/// and everything below moves down. Wrapping keeps every control directly
/// on screen and keyboard-reachable in place. Rejected: overflowing Wrap or
/// Linked scroll into a menu, which would hide the state of two toggles
/// the panes' layout depends on; and shrinking the groups, which would cut
/// the labels the outcome requires whole.
fn change_row(
    ui: &mut Ui,
    cx: &mut WorkspaceCx<'_>,
    aligned: Option<&AlignedComparison>,
    palette: &Palette,
) {
    let changes = aligned.map_or(0, |a| a.hunks.len());
    let lines = change_lines(ui, cx, changes);
    for (index, line) in lines.iter().enumerate() {
        if index > 0 {
            ui.add_space(GAP);
        }
        change_line(ui, cx, line, changes, palette);
    }
}

/// Splits the change row's groups into lines that each fit the row width,
/// greedily in reading order. The divider before the toggles is counted
/// only where it is drawn, beside the navigation.
fn change_lines(ui: &Ui, cx: &WorkspaceCx<'_>, changes: usize) -> Vec<Vec<ChangeGroup>> {
    let spacing = ui.spacing().item_spacing.x;
    let width = ui.available_width();
    let check = |text: &str| 16.0 + 8.0 + text_width(ui, text, CompactStyle::Table);
    let navigation = row_width(
        &[
            text_width(
                ui,
                &projections::change_label(cx.compare.active_change, changes),
                CompactStyle::Supporting,
            ),
            text_width(ui, PREVIOUS_CHANGE_NAME, CompactStyle::ControlLabel),
            text_width(ui, NEXT_CHANGE_NAME, CompactStyle::ControlLabel),
        ],
        spacing,
    );
    let toggles = row_width(&[check(WRAP_NAME), check(LINKED_SCROLL_NAME)], spacing);
    let divider = DIVIDER_WIDTH + spacing;
    let mode = view_mode_width(ui);
    let group_width = |group: ChangeGroup, after: Option<ChangeGroup>| match group {
        ChangeGroup::Navigation => navigation,
        ChangeGroup::Toggles if after == Some(ChangeGroup::Navigation) => divider + toggles,
        ChangeGroup::Toggles => toggles,
        ChangeGroup::Mode => mode,
    };
    let mut lines: Vec<Vec<ChangeGroup>> = Vec::new();
    let mut used = 0.0;
    for group in [
        ChangeGroup::Navigation,
        ChangeGroup::Toggles,
        ChangeGroup::Mode,
    ] {
        if let Some(line) = lines.last_mut() {
            let extra = spacing + group_width(group, line.last().copied());
            if used + extra <= width {
                used += extra;
                line.push(group);
                continue;
            }
        }
        used = group_width(group, None);
        lines.push(vec![group]);
    }
    lines
}

/// The vertical divider's own width; `egui::Separator` with spacing 8.
const DIVIDER_WIDTH: f32 = 8.0;

/// One line of the change row holding `groups`.
fn change_line(
    ui: &mut Ui,
    cx: &mut WorkspaceCx<'_>,
    groups: &[ChangeGroup],
    changes: usize,
    palette: &Palette,
) {
    // The row keeps its 28 units while ‹ Previous and Next › stay 30 high,
    // overhanging it by 1 above and below as frame `13:1304` draws them
    // (y −1 in the row). The controls are laid out in that 30-high band,
    // centred on the row, in a child the parent never allocates, so only
    // the row itself advances the stack. Rejected: `allocate_ui_with_layout`
    // or `scope_builder`, which allocate the band (or the row grown to its
    // tallest control) and push everything below 1 or 2 units down.
    let row = Rect::from_min_size(ui.cursor().min, Vec2::new(ui.available_width(), ROW_HEIGHT));
    let band = row.expand2(Vec2::new(0.0, (CONTROL_HEIGHT - ROW_HEIGHT) / 2.0));
    ui.advance_cursor_after_rect(row);
    let mut row_ui = ui.new_child(
        UiBuilder::new()
            .max_rect(band)
            .layout(Layout::left_to_right(Align::Center)),
    );
    let ui = &mut row_ui;
    let active = cx.compare.active_change;
    if groups.contains(&ChangeGroup::Navigation) {
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
    }
    if groups.contains(&ChangeGroup::Toggles) {
        if groups.contains(&ChangeGroup::Navigation) {
            ui.add(egui::Separator::default().vertical().spacing(DIVIDER_WIDTH));
        }
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
    }
    if groups.contains(&ChangeGroup::Mode) {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            view_mode(ui, cx, palette);
        });
    }
}

/// Horizontal padding inside each segment of the Original | Normalized
/// switch (frame `13:1379`: 8 either side of the label).
const SEGMENT_PADDING: f32 = 8.0;

/// The switch's laid-out width: both segments and the frame's 1-unit
/// margin either side.
fn view_mode_width(ui: &Ui) -> f32 {
    [ORIGINAL_NAME, NORMALIZED_NAME]
        .iter()
        .map(|name| text_width(ui, name, CompactStyle::ControlLabel) + 2.0 * SEGMENT_PADDING)
        .sum::<f32>()
        + 2.0
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
                // Both segments are sized from their own label plus
                // `SEGMENT_PADDING`. Rejected: a frameless egui button for
                // the unselected segment, which drops the button margin
                // with the frame, so "Normalized" ran into the switch's
                // border and read as clipped (T24).
                let width =
                    text_width(ui, name, CompactStyle::ControlLabel) + 2.0 * SEGMENT_PADDING;
                let mut button = egui::Button::new(styled(name, CompactStyle::ControlLabel, color))
                    .min_size(Vec2::new(width, 26.0))
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

/// Allocates the pinned identities' 54-high band across the paired region
/// and returns it. The panes start at its bottom edge, so callers allocate
/// it with no vertical item spacing (see [`show`]).
fn identity_band(ui: &mut Ui) -> Rect {
    let area = ui.available_rect_before_wrap();
    let band = Rect::from_min_size(area.min, Vec2::new(area.width(), IDENTITY_HEIGHT));
    ui.advance_cursor_after_rect(band);
    band
}

/// The two identity rects in `band`, A then B, on the panes' split.
fn identity_rects(compare: &CompareState, band: Rect) -> [Rect; 2] {
    let ratio = compare.split_ratio.clamp(SPLIT_RANGE.0, SPLIT_RANGE.1);
    let usable = (band.width() - PANE_GAP).max(2.0);
    let split = band.left() + (usable * ratio).round();
    [
        Rect::from_min_max(band.min, egui::pos2(split, band.bottom())),
        Rect::from_min_max(egui::pos2(split + PANE_GAP, band.top()), band.max),
    ]
}

/// One focusable accessibility node per identity row, its texts painted
/// later by [`pinned_identity`]. Rejected: a label widget per text, which
/// puts a second node named exactly like the file next to Review unique
/// text's own when that popup opens over Compare, so a query for the
/// popup's file name is no longer unique (`tests/review_popups.rs` asks for
/// it by name). One node per row is also how the diff widget names its
/// rows.
///
/// The row is focusable (spec line 1125: details and copy actions must be
/// keyboard-reachable, hover alone is insufficient), so Tab reaches it and
/// [`identities`] then shows the full path and takes the Copy path chord.
fn identity_nodes(
    ui: &mut Ui,
    cx: &WorkspaceCx<'_>,
    view: &CompareView,
    band: Rect,
) -> [Response; 2] {
    let rects = identity_rects(cx.compare, band);
    [(&view.a, rects[0]), (&view.b, rects[1])].map(|(identity, rect)| {
        let response = ui.interact(
            rect,
            Id::unique(("fd.compare.identity", identity.file)),
            Sense::focusable_noninteractive(),
        );
        let name = projections::identity_name(identity);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &name));
        response
    })
}

/// Takes Ctrl+Shift+C, exactly, from this frame's events: the Copy path
/// chord Findings binds for its rows (`grouped_table.rs`), so the one chord
/// copies a path in both workspaces. egui sets `command` beside `ctrl` on
/// this platform family, so either counts as Ctrl.
fn take_copy_path(ui: &Ui) -> bool {
    ui.input_mut(|input| {
        let mut hit = false;
        input.events.retain(|event| {
            let Event::Key {
                key: Key::C,
                pressed,
                modifiers,
                ..
            } = event
            else {
                return true;
            };
            let exact = (modifiers.ctrl || modifiers.command) && modifiers.shift && !modifiers.alt;
            if exact {
                hit |= *pressed;
            }
            !exact
        });
        hit
    })
}

/// Paints the pinned file identity above each pane into `band`, on the
/// panes' own split, each with its pane's notice from `notices`, and gives
/// the row with keyboard focus its full path and Copy path.
///
/// Full-path access (spec §11.3 "accessible full path", line 1125): while
/// a row has keyboard focus, a tooltip shows its complete path and names
/// the chord; Ctrl+Shift+C then copies the path through the shared
/// `UiIntent::CopyPath`, which copies the member's stored `display_path`,
/// the same string the row shows. Pointer users get the same tooltip on
/// hover. Rejected: expanding the path line in place, which has no room in
/// the 54-high row for a deep path at 200%; and a Copy path button in the
/// row, which would compete with the role and the notice for the row's
/// trailing edge at 1024 units.
#[allow(clippy::too_many_arguments)]
fn identities(
    ui: &mut Ui,
    cx: &WorkspaceCx<'_>,
    intents: &mut IntentQueue,
    view: &CompareView,
    band: Rect,
    notices: &diff_view::PaneNotices,
    focus: &[Response; 2],
    palette: &Palette,
) {
    let rects = identity_rects(cx.compare, band);
    for (((rect, identity), notice), response) in rects
        .into_iter()
        .zip([&view.a, &view.b])
        .zip(notices)
        .zip(focus)
    {
        pinned_identity(ui, rect, identity, notice.as_deref(), palette);
        let details = |ui: &mut Ui| {
            ui.add(
                egui::Label::new(styled(
                    identity.path.clone(),
                    CompactStyle::PathDetail,
                    palette.color(Token::Ink),
                ))
                .selectable(false)
                .wrap(),
            );
            label(
                ui,
                COPY_PATH_HINT,
                CompactStyle::Supporting,
                palette.color(Token::Muted),
            );
        };
        if response.has_focus() {
            response.show_tooltip_ui(details);
            if cx.input_enabled && take_copy_path(ui) {
                intents.push(UiIntent::CopyPath(identity.file));
            }
        } else {
            response.clone().on_hover_ui(details);
        }
    }
}

/// The second line of the identity's full-path tooltip.
pub const COPY_PATH_HINT: &str = "Copy path: Ctrl+Shift+C";

/// The colour of an identity's role, from the decision it names: the
/// tokens the Findings Decision cell gives the same words
/// (`grouped_table.rs`, `decision_cell`), so the pane says what Findings
/// says about the row in the same colour. Frame `13:1304` draws "B · Review
/// first" amber and "A · Reference" ink; the Decision cell's muted
/// Reference is not followed here, because the frame does not.
fn role_token(role: &str) -> Token {
    let decision = role
        .split_once(" · ")
        .map_or(role, |(_, decision)| decision);
    match decision {
        "Queued" => Token::Red,
        "Review first" | "Counterpart queued" | "Counterpart of a queued file" => Token::Amber,
        "Kept" => Token::Green,
        _ => Token::Ink,
    }
}

/// One pinned identity (frame `13:1410`): icon, name, path with size and
/// date when they fit, and the side's role; with a pane notice, the role
/// moves up to the name's line and the notice takes the path's line at the
/// trailing edge.
///
/// The notice is IR-006's statement that the pane shows (or copied)
/// normalized text. It moved here from the pane caption the widget no
/// longer draws (frame `13:1304` has none), and it is its own accessibility
/// node, so assistive technology hears it apart from the file's identity.
fn pinned_identity(
    ui: &mut Ui,
    rect: Rect,
    identity: &PaneIdentity,
    notice: Option<&str>,
    palette: &Palette,
) {
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
    let role_color = palette.color(role_token(&identity.role));
    let role = painter.layout_no_wrap(
        identity.role.clone(),
        CompactStyle::Supporting.font_id(),
        role_color,
    );
    let role_left = inner.right() - role.size().x;
    let role_top = if notice.is_some() {
        inner.top()
    } else {
        inner.center().y - role.size().y / 2.0
    };
    painter.galley(egui::pos2(role_left, role_top), role, role_color);
    // Each text line ends 8 before whatever trails it on that line: the
    // role (centred across both lines, or on the name's line beside a
    // notice) and the notice on the path's line.
    let mut path_end = role_left;
    if let Some(notice) = notice {
        let galley =
            painter.layout_no_wrap(notice.to_owned(), CompactStyle::Supporting.font_id(), muted);
        let at = egui::pos2(inner.right() - galley.size().x, inner.top() + 20.0);
        let notice_rect = Rect::from_min_size(at, galley.size());
        path_end = notice_rect.left();
        painter.galley(at, galley, muted);
        ui.interact(
            notice_rect,
            Id::unique(("fd.compare.identity_notice", identity.file)),
            Sense::hover(),
        )
        .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, notice));
    }
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
    let name_width = (role_left - 8.0 - left).max(0.0);
    let path_width = (path_end - 8.0 - left).max(0.0);
    let detail = format!(
        "{}{}{}",
        identity.path,
        projections::IDENTITY_SEPARATOR,
        identity.size_and_date
    );
    // The size and date go first when the row is narrow (frame `29:3648`).
    let path = if text_width(ui, &detail, CompactStyle::PathDetail) <= path_width {
        detail
    } else {
        identity.path.clone()
    };
    for (text, style, color, top, width) in [
        (
            identity.name.clone(),
            CompactStyle::GroupEmphasis,
            ink,
            0.0,
            name_width,
        ),
        (path, CompactStyle::PathDetail, muted, 20.0, path_width),
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
