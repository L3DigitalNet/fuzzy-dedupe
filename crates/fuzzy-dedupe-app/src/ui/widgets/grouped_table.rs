// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! The grouped, virtualized Findings table (spec §11.4, §11.9): stable row
//! identity over an `egui_table` body. [`show`] is the entry point; the
//! Findings view (`ui/findings.rs`) draws the toolbar, filter row, states
//! and menus around it.
//!
//! Rows come from the cached [`FindingsProjection`], rebuilt by
//! [`refresh_projection`] only when its key changes; `egui_table` then asks
//! only for the visible rows, so the 100,000-row `large` scenario never
//! builds a widget per row. Every interaction is keyed by [`RowId`] or
//! [`FileId`], never by position, so focus and queue state survive sorting,
//! filtering, expansion and scrolling.
//!
//! Queue state is never decided here: the checkbox and Space send a review
//! command and draw whatever the engine's published view says next frame.
//!
//! Menus: the table draws none. A row-menu request (right-click, ⋯, Menu
//! key, Shift+F10) is returned from [`show`] as a [`RowMenuRequest`], and
//! the Findings view asks the dialog host for it.
//!
//! Scroll offset: `FindingsState::scroll_offset` records the offset of the
//! table's own scroll area (the one `egui_table` creates) on every frame
//! the table draws, and is applied to that scroll area once, on the first
//! frame the table draws after a frame it did not draw. See
//! [`ScrollProbe`] for how the scroll area is reached and when a restored
//! offset shows.
//!
//! Column widths: `FindingsState::column_widths` is the source of truth.
//! Each frame writes those widths into `egui_table`'s own stored state
//! before it draws, and reads back only a drag-resize afterwards. Rejected:
//! letting `egui_table` keep widths itself, because on a table it has not
//! seen it runs an auto-size pass that shrinks every column to its content,
//! which loses the §11.4 reference widths on the first frame.

use std::collections::HashMap;
use std::sync::Arc;

use egui::{Align2, CornerRadius, Id, Key, Modifiers, Rect, Sense, Stroke, StrokeKind, Ui};
use egui_table::{AutoSizeMode, CellInfo, Column, HeaderCellInfo, HeaderRow, TableDelegate};
use fuzzy_dedupe_domain::{FileId, GroupId, ScanResults};
use fuzzy_dedupe_engine::service::ReviewCommand;

use crate::presentation::commands::{IntentQueue, UiIntent};
use crate::presentation::projections::{
    self, FindingsProjection, ProjectionKey, QueueBox, VisibleRow,
};
use crate::presentation::state::{ColumnId, FindingsState, QueueState, RowId, ScanState};
use crate::ui::theme::{self, CompactStyle, Icon, Palette, Token};
use crate::ui::widgets::dialog_host::mode_of;

/// Header and body row heights at default zoom (spec §11.3).
pub const HEADER_HEIGHT: f32 = 30.0;
pub const ROW_HEIGHT: f32 = 28.0;
/// Horizontal cell padding (frame `13:633`).
pub const CELL_PAD: f32 = 8.0;
/// Below this window width the [`COLLAPSIBLE`] columns hide (spec §11.3
/// collapse order, step 2). Exactly 1280 keeps them, as for the rail.
pub const COLLAPSE_BELOW: f32 = 1280.0;
/// The columns the collapse order hides.
pub const COLLAPSIBLE: [ColumnId; 3] = [ColumnId::Size, ColumnId::Modified, ColumnId::Protected];
/// The columns Choose columns… may hide; the rest carry identity, evidence
/// or intent and always show.
pub const OPTIONAL: [ColumnId; 4] = [
    ColumnId::Folder,
    ColumnId::Modified,
    ColumnId::Size,
    ColumnId::Protected,
];
/// The row menu's width (frame `28:3719`).
pub const ROW_MENU_WIDTH: f32 = 250.0;

/// The table's keyboard focus target. Rows are not focusable themselves:
/// only the visible ones exist each frame, so focus on a row widget would
/// be lost as soon as it scrolled out. Row focus is `FindingsState::focused`.
#[must_use]
pub fn table_id() -> Id {
    Id::unique("fd.findings.table")
}

/// A member row's queue checkbox, stable per file: the invoker focus
/// returns to when a popup it opened closes.
#[must_use]
pub fn checkbox_id(file: FileId) -> Id {
    Id::unique(("fd.findings.queue", file))
}

/// The ⋯ button of the focused row.
#[must_use]
pub fn row_menu_button_id() -> Id {
    Id::unique("fd.findings.row_menu_button")
}

/// The column header text.
#[must_use]
pub const fn column_title(column: ColumnId) -> &'static str {
    match column {
        ColumnId::Queue => "Queue",
        ColumnId::Group => "Group",
        ColumnId::Match => "Match",
        ColumnId::Unique => "Unique",
        ColumnId::FileName => "File name",
        ColumnId::Folder => "Folder",
        ColumnId::Modified => "Modified",
        ColumnId::Size => "Size",
        ColumnId::Protected => "Protected",
        ColumnId::Decision => "Decision",
    }
}

/// The columns shown for a window `window_width` units wide, in order.
#[must_use]
pub fn visible_columns(findings: &FindingsState, window_width: f32) -> Vec<ColumnId> {
    let collapsed = window_width < COLLAPSE_BELOW;
    ColumnId::ALL
        .into_iter()
        .filter(|c| !findings.hidden_columns.contains(c))
        .filter(|c| !(collapsed && COLLAPSIBLE.contains(c)))
        .collect()
}

/// A column's width: the user's, or the §11.4 reference width.
#[must_use]
pub fn column_width(findings: &FindingsState, column: ColumnId) -> f32 {
    findings
        .column_widths
        .get(&column)
        .copied()
        .unwrap_or_else(|| column.reference_width())
}

// Private-use code points for the table's Lucide icons, drawn as glyphs so
// they take the text colour (see the note above the icon table in
// theme.rs). Distinct from the shell's U+E100..U+E103.
const GLYPHS: [(Icon, char); 7] = [
    (Icon::Expanded, '\u{E110}'),
    (Icon::Collapsed, '\u{E111}'),
    (Icon::Document, '\u{E112}'),
    (Icon::RowMenu, '\u{E113}'),
    (Icon::Check, '\u{E114}'),
    (Icon::Search, '\u{E115}'),
    (Icon::Alert, '\u{E116}'),
];

fn glyph(icon: Icon) -> char {
    GLYPHS
        .iter()
        .find(|(candidate, _)| *candidate == icon)
        .map_or('\u{FFFD}', |(_, ch)| *ch)
}

fn glyphs_flag() -> Id {
    Id::unique("fd.findings.glyphs")
}

/// Registers the Findings icon glyphs once per context. egui rebuilds its
/// fonts only at the next pass, so [`icon_text`] draws nothing until then.
pub fn ensure_glyphs(ctx: &egui::Context) {
    if ctx.data(|d| d.get_temp::<u64>(glyphs_flag())).is_some() {
        return;
    }
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| d.insert_temp(glyphs_flag(), pass));
    for (icon, ch) in GLYPHS {
        match egui_extras::SvgGlyph::from_bytes(icon.bytes()) {
            Ok(svg) => ctx.add_glyph_rasterizer(
                svg.with_color(false)
                    .with_baseline_offset(-0.2)
                    .into_rasterizer(ch.to_string()),
            ),
            Err(err) => tracing::warn!(icon = icon.lucide_name(), %err, "icon glyph not loaded"),
        }
    }
}

/// `icon` as text in the current text colour, or nothing while its glyph
/// is not yet usable. The fonts panic on a code point no font or
/// rasterizer draws, so a glyph is never laid out in the pass that
/// registered it.
#[must_use]
pub fn icon_text(ctx: &egui::Context, icon: Icon) -> String {
    let installed = ctx.data(|d| d.get_temp::<u64>(glyphs_flag()));
    match installed {
        Some(pass) if ctx.cumulative_pass_nr() > pass => glyph(icon).to_string(),
        _ => String::new(),
    }
}

/// The current projection, rebuilt only when its key differs from the
/// cached one's. `None` without results.
pub fn refresh_projection(
    findings: &mut FindingsState,
    scan: &ScanState,
    queue: &QueueState,
) -> Option<Arc<FindingsProjection>> {
    let Some(results) = scan.results.as_ref() else {
        findings.projection = None;
        return None;
    };
    let key = ProjectionKey::of(findings, Some(results.scan));
    if let Some(cached) = &findings.projection
        && cached.key == key
    {
        return Some(Arc::clone(cached));
    }
    let built = Arc::new(projections::findings_projection(results, queue, key));
    findings.projection = Some(Arc::clone(&built));
    Some(built)
}

/// The request that queues or unqueues `file`: unqueue when the engine's
/// view lists it as queued, otherwise a queue request against the row's
/// current counterpart, which the review service accepts, refuses or
/// answers with `NeedsReview`.
#[must_use]
pub fn queue_toggle(
    queue: &QueueState,
    file: FileId,
    counterpart: Option<FileId>,
    origin: fuzzy_dedupe_domain::QueueOrigin,
) -> ReviewCommand {
    if queue.is_queued(file) {
        ReviewCommand::RemoveFromQueue { file }
    } else {
        ReviewCommand::RequestQueue {
            file,
            // A reference row has no counterpart; naming itself lets the
            // service answer with its own Reference refusal.
            counterpart: counterpart.unwrap_or(file),
            origin,
            reviewed: None,
        }
    }
}

/// A row menu the table's input asked for: the view asks the dialog host
/// to open `MenuKind::Row(row)` at `at`, with focus returning to `invoker`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RowMenuRequest {
    pub row: RowId,
    pub at: egui::Pos2,
    pub invoker: Id,
}

/// What a frame's input asked the table to do, applied after drawing so
/// the delegate only reads state.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Action {
    Focus(RowId),
    ToggleGroup(GroupId),
    Compare(RowId),
    Queue {
        row: RowId,
        invoker: Id,
    },
    RowMenu {
        row: RowId,
        at: egui::Pos2,
        invoker: Id,
    },
    CopyPath(FileId),
}

/// Draws the table into the remaining Findings height and returns the row
/// menu its input asked for, if any. `input_enabled` is false while a popup
/// or menu is open: the table then reacts to no key, so no shortcut reaches
/// it under a popup (spec §11.6).
#[must_use]
pub fn show(
    ui: &mut egui::Ui,
    findings: &mut FindingsState,
    scan: &ScanState,
    queue: &QueueState,
    intents: &mut IntentQueue,
    input_enabled: bool,
) -> Option<RowMenuRequest> {
    let results = scan.results.clone()?;
    let projection = refresh_projection(findings, scan, queue)?;
    let restoring = note_drawn(ui.ctx());
    ensure_glyphs(ui.ctx());
    let palette = theme::palette(mode_of(ui.ctx()));
    let window_width = ui.ctx().content_rect().width();
    let columns = visible_columns(findings, window_width);
    let outer = ui.available_rect_before_wrap();
    let page = rows_per_page(outer.height());

    let mut actions = Vec::new();
    let table_response = ui.interact(outer, table_id(), Sense::click());
    if table_response.clicked() || table_response.secondary_clicked() {
        table_response.request_focus();
    }
    let focused_widget = ui.memory(egui::Memory::focused);
    let keys_for_table = input_enabled && focused_widget.is_none_or(|id| id == table_id());
    let menu_from_keyboard =
        keys_for_table && table_keys(ui, findings, &projection, &results, page, &mut actions);

    let reveal = std::mem::take(&mut findings.reveal_focus);
    let focused_pos = findings.focused.and_then(|row| projection.position(row));
    let mut table = egui_table::Table::new()
        .id_salt("fd.findings.table")
        .num_rows(projection.rows.len() as u64)
        .columns(
            columns
                .iter()
                .map(|c| {
                    Column::new(column_width(findings, *c))
                        .id(column_key(*c))
                        .range(24.0..=f32::INFINITY)
                })
                .collect::<Vec<_>>(),
        )
        .headers([HeaderRow::new(HEADER_HEIGHT)])
        .auto_size_mode(AutoSizeMode::Never);
    if reveal && let Some(pos) = focused_pos {
        table = table.scroll_to_row(pos as u64, None);
    }

    let table_focused = ui.memory(|m| m.has_focus(table_id()));
    let scroll = ScrollProbe {
        restore: restoring.then_some(findings.scroll_offset),
        area: None,
    };
    let mut delegate = Delegate {
        results: &results,
        projection: &projection,
        findings,
        queue,
        columns: &columns,
        palette,
        table_focused,
        actions: &mut actions,
        row_rects: HashMap::new(),
        scroll,
    };
    ui.painter().rect(
        outer,
        CornerRadius::same(theme::CORNER_RADIUS),
        palette.color(Token::Surface),
        Stroke::new(theme::BORDER_WIDTH, palette.color(Token::Line)),
        StrokeKind::Inside,
    );
    // The stored state is keyed by the ID of the `Ui` the table shows in,
    // so the widths are written and read back inside the same scope.
    let state_id = ui
        .scope_builder(
            egui::UiBuilder::new().max_rect(outer.shrink(theme::BORDER_WIDTH)),
            |ui| {
                let state_id = table.get_id(ui);
                write_widths(ui, state_id, delegate.findings, &columns);
                table.show(ui, &mut delegate);
                state_id
            },
        )
        .inner;
    let row_rects = std::mem::take(&mut delegate.row_rects);
    let scroll_area = delegate.scroll.area;

    read_widths(ui, state_id, findings, &columns);
    // Not on a restoring pass: its scroll area still holds the old offset,
    // and recording that would overwrite the offset being restored. Later
    // passes record the restored one once the area has reached it.
    if !restoring && let Some(offset) = ScrollProbe::offset(ui.ctx(), scroll_area) {
        findings.scroll_offset = offset;
    }

    if menu_from_keyboard && let Some(row @ RowId::Member { .. }) = findings.focused {
        let at = row_rects.get(&row).map_or(outer.left_top(), |rect| {
            egui::pos2(rect.right() - ROW_MENU_WIDTH, rect.bottom())
        });
        actions.push(Action::RowMenu {
            row,
            at,
            invoker: table_id(),
        });
    }
    if input_enabled {
        apply(findings, &results, queue, intents, actions)
    } else {
        None
    }
}

/// Takes the arrow keys for row focus instead of egui's focus movement and
/// reads the §11.10 Findings keys; returns whether a row menu was asked for.
fn table_keys(
    ui: &Ui,
    findings: &mut FindingsState,
    projection: &FindingsProjection,
    results: &ScanResults,
    page: usize,
    actions: &mut Vec<Action>,
) -> bool {
    ui.memory_mut(|m| {
        m.set_focus_lock_filter(
            table_id(),
            egui::EventFilter {
                horizontal_arrows: true,
                vertical_arrows: true,
                tab: false,
                escape: false,
            },
        );
    });
    read_keys(ui, findings, projection, results, page, actions)
}

/// Records that the table draws in this pass, and returns whether it did
/// not draw in the pass before: the view is being restored (the return
/// from Compare, or rows shown again after a state message), so the
/// recorded offset is applied.
fn note_drawn(ctx: &egui::Context) -> bool {
    let pass = ctx.cumulative_pass_nr();
    let key = table_id().with("drawn_pass");
    let last = ctx.data_mut(|d| {
        let last = d.get_temp::<u64>(key);
        d.insert_temp(key, pass);
        last
    });
    last.is_none_or(|last| last.saturating_add(1) < pass)
}

/// Reaches the scroll area `egui_table` creates for the body, which it
/// does not expose: from the first body row drawn, the nearest enclosing
/// `UiKind::ScrollArea` node of the `Ui` stack is that area's content, and
/// the area's state ID is its parent's scope ID salted the way
/// `egui::ScrollArea` salts an area without an `id_salt`. Rejected:
/// spelling out `egui_table`'s chain of child scopes, which a version bump
/// would silently break; the stack walk finds the area whatever lies
/// between.
///
/// Restoring goes through egui's scroll delta with no animation, the only
/// way to move an area's offset from outside (`State`'s offset has no
/// setter). The area `egui_table` builds is animated, so the delta becomes
/// a zero-length scroll target that the area reaches at the start of its
/// next pass, after that pass has already placed its content. The
/// restoring pass is therefore discarded, so its rerun reaches the target
/// within the restoring frame and the next frame draws at it; without the
/// discard a changed offset would show only two frames after the restore.
/// egui allows one discard per frame, so when another discard already ran
/// this frame (`egui_table`'s first-draw sizing pass) it shows one frame
/// later still. The first restored frame is exact whenever the
/// recorded offset is the one the area last drew, which holds for every
/// return that writes no new offset while the table is hidden (the IR-001
/// return from Compare): the area keeps its state across frames it is not
/// drawn, and this delta is then zero.
struct ScrollProbe {
    /// The offset to apply, on a restoring pass, until applied.
    restore: Option<f32>,
    /// The body's scroll area state ID, once a row found it.
    area: Option<Id>,
}

impl ScrollProbe {
    /// The vertical offset `area` holds, clamped to its content.
    fn offset(ctx: &egui::Context, area: Option<Id>) -> Option<f32> {
        egui::scroll_area::State::load(ctx, area?).map(|stored| stored.clamped_offset().y)
    }

    fn visit(&mut self, ui: &Ui) {
        if self.area.is_none() {
            self.area = ui
                .stack()
                .iter()
                .find(|node| node.kind() == Some(egui::UiKind::ScrollArea))
                .and_then(|content| content.parent.as_ref())
                .map(|area| area.scope_id.with(egui::IdSalt::new("scroll_area")));
        }
        let Some(target) = self.restore.take() else {
            return;
        };
        let Some(current) = self
            .area
            .and_then(|area| egui::scroll_area::State::load(ui.ctx(), area))
            .map(|stored| stored.unclamped_offset().y)
        else {
            return;
        };
        // Exact on purpose: the round trip must restore the very value
        // recorded, and an equal offset needs no move.
        #[expect(clippy::float_cmp, reason = "an exact restore of a recorded value")]
        let moved = current != target;
        if moved {
            // egui's scroll delta is the content's movement: the opposite
            // sign of the offset change.
            ui.scroll_with_delta_animation(
                egui::vec2(0.0, current - target),
                egui::style::ScrollAnimation::none(),
            );
            ui.ctx()
                .request_discard("restore the Findings table's scroll offset");
        }
    }
}

fn column_key(column: ColumnId) -> Id {
    Id::unique(("fd.findings.column", column))
}

/// Puts `FindingsState::column_widths` into `egui_table`'s stored state, so
/// the table neither auto-sizes nor keeps a width of its own.
fn write_widths(ui: &Ui, state_id: Id, findings: &FindingsState, columns: &[ColumnId]) {
    let mut stored = egui_table::TableState::load(ui.ctx(), state_id).unwrap_or_default();
    for (i, c) in columns.iter().enumerate() {
        let column = Column::new(0.0).id(column_key(*c));
        stored
            .col_widths
            .insert(column.id_for(i), column_width(findings, *c));
    }
    stored.parent_width = Some(ui.available_width());
    stored.store(ui.ctx(), state_id);
}

/// Takes back a drag-resize: the stored width of a column whose resize
/// handle is being dragged. Only a drag counts: `egui_table` also rewrites
/// widths on its own sizing passes (egui's first-frame and discard passes),
/// and taking those would shrink every column to its content.
fn read_widths(ui: &Ui, state_id: Id, findings: &mut FindingsState, columns: &[ColumnId]) {
    let Some(after) = egui_table::TableState::load(ui.ctx(), state_id) else {
        return;
    };
    for (i, c) in columns.iter().enumerate() {
        let column = Column::new(0.0).id(column_key(*c));
        let handle = state_id.with(column.id_for(i)).with("resize");
        if ui.ctx().is_being_dragged(handle)
            && let Some(width) = after.col_widths.get(&column.id_for(i))
            && (width - column_width(findings, *c)).abs() > 0.25
        {
            findings.column_widths.insert(*c, *width);
        }
    }
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a floored, at-least-one count of rows in a window-sized height"
)]
fn rows_per_page(height: f32) -> usize {
    ((height - HEADER_HEIGHT) / ROW_HEIGHT).floor().max(1.0) as usize
}

/// Reads the §11.10 Findings keys. Returns whether a row menu was asked
/// for (Menu key or Shift+F10), which needs the row's drawn rectangle.
fn read_keys(
    ui: &Ui,
    findings: &mut FindingsState,
    projection: &FindingsProjection,
    results: &ScanResults,
    page: usize,
    actions: &mut Vec<Action>,
) -> bool {
    let rows = &projection.rows;
    let pressed = |modifiers, key| ui.input_mut(|i| i.consume_key(modifiers, key));
    let current = findings.focused.and_then(|row| projection.position(row));
    let last = rows.len().saturating_sub(1);
    let moved = |to: usize| rows.get(to).map(|r| r.id);
    let mut target = None;
    if pressed(Modifiers::NONE, Key::ArrowDown) {
        target = moved(current.map_or(0, |i| (i + 1).min(last)));
    }
    if pressed(Modifiers::NONE, Key::ArrowUp) {
        target = moved(current.map_or(last, |i| i.saturating_sub(1)));
    }
    if pressed(Modifiers::NONE, Key::PageDown) {
        target = moved(current.map_or(0, |i| (i + page).min(last)));
    }
    if pressed(Modifiers::NONE, Key::PageUp) {
        target = moved(current.map_or(0, |i| i.saturating_sub(page)));
    }
    if pressed(Modifiers::NONE, Key::Home) {
        target = moved(0);
    }
    if pressed(Modifiers::NONE, Key::End) {
        target = moved(last);
    }
    if let Some(row) = target {
        findings.focused = Some(row);
        findings.reveal_focus = true;
    }
    let Some(focused) = findings.focused else {
        return false;
    };
    let group_of = |row: RowId| match row {
        RowId::Group(group) | RowId::Member { group, .. } => group,
    };
    if pressed(Modifiers::NONE, Key::ArrowRight)
        && let RowId::Group(group) = focused
        && !findings.expanded.contains(&group)
    {
        actions.push(Action::ToggleGroup(group));
    }
    if pressed(Modifiers::NONE, Key::ArrowLeft) {
        let group = group_of(focused);
        if findings.expanded.contains(&group) {
            actions.push(Action::ToggleGroup(group));
        }
        actions.push(Action::Focus(RowId::Group(group)));
    }
    if pressed(Modifiers::NONE, Key::Enter) {
        match focused {
            RowId::Group(group) => actions.push(Action::ToggleGroup(group)),
            RowId::Member { .. } => actions.push(Action::Compare(focused)),
        }
    }
    if pressed(Modifiers::NONE, Key::Space) && matches!(focused, RowId::Member { .. }) {
        actions.push(Action::Queue {
            row: focused,
            invoker: table_id(),
        });
    }
    if pressed(Modifiers::CTRL | Modifiers::SHIFT, Key::C)
        && let RowId::Member { file, .. } = focused
        && results.group_of(file).is_some()
    {
        actions.push(Action::CopyPath(file));
    }
    let menu = pressed(Modifiers::NONE, Key::Menu) || pressed(Modifiers::SHIFT, Key::F10);
    menu && matches!(focused, RowId::Member { .. })
}

/// Carries out the frame's actions and returns the row menu asked for.
/// Queue intents name the control that sent them, so a `NeedsReview` popup
/// returns focus there.
fn apply(
    findings: &mut FindingsState,
    results: &ScanResults,
    queue: &QueueState,
    intents: &mut IntentQueue,
    actions: Vec<Action>,
) -> Option<RowMenuRequest> {
    let mut row_menu = None;
    for action in actions {
        match action {
            Action::Focus(row) => findings.focused = Some(row),
            Action::CopyPath(file) => intents.push(UiIntent::CopyPath(file)),
            Action::ToggleGroup(group) => {
                if !findings.expanded.remove(&group) {
                    findings.expanded.insert(group);
                }
            }
            Action::Compare(row) => {
                if let Some((member, view)) = member_at(results, queue, row)
                    && let Some(counterpart) = view.counterpart
                {
                    intents.push(UiIntent::OpenCompare {
                        target: member,
                        counterpart,
                        return_row: Some(row),
                    });
                }
            }
            Action::Queue { row, invoker } => {
                if let Some((file, view)) = member_at(results, queue, row) {
                    intents.push(UiIntent::Review {
                        command: queue_toggle(
                            queue,
                            file,
                            view.counterpart,
                            if invoker == table_id() {
                                fuzzy_dedupe_domain::QueueOrigin::Shortcut
                            } else {
                                fuzzy_dedupe_domain::QueueOrigin::Checkbox
                            },
                        ),
                        invoker: Some(invoker),
                    });
                }
            }
            Action::RowMenu { row, at, invoker } => {
                findings.focused = Some(row);
                row_menu = Some(RowMenuRequest { row, at, invoker });
            }
        }
    }
    row_menu
}

/// The member file at `row` and its evidence, if `row` is a member row of
/// the current results.
#[must_use]
pub fn member_at(
    results: &ScanResults,
    queue: &QueueState,
    row: RowId,
) -> Option<(FileId, projections::MemberView)> {
    let RowId::Member { group, file } = row else {
        return None;
    };
    let group = results.groups.iter().find(|g| g.id == group)?;
    let member = group.member(file)?;
    Some((file, projections::member_view(group, queue, member)))
}

/// One row's pointer input this frame.
struct RowClicks {
    clicked: bool,
    secondary_clicked: bool,
    double_clicked: bool,
    pointer: Option<egui::Pos2>,
}

struct Delegate<'a> {
    results: &'a ScanResults,
    projection: &'a FindingsProjection,
    findings: &'a FindingsState,
    queue: &'a QueueState,
    columns: &'a [ColumnId],
    palette: &'static Palette,
    table_focused: bool,
    actions: &'a mut Vec<Action>,
    row_rects: HashMap<RowId, Rect>,
    scroll: ScrollProbe,
}

impl Delegate<'_> {
    fn row(&self, row_nr: u64) -> Option<VisibleRow> {
        usize::try_from(row_nr)
            .ok()
            .and_then(|i| self.projection.rows.get(i))
            .copied()
    }

    fn text(&self, text: impl Into<String>, style: CompactStyle, token: Token) -> egui::RichText {
        egui::RichText::new(text)
            .text_style(style.text_style())
            .color(self.palette.color(token))
    }
}

/// A cell's content area: the cell less its horizontal padding, with the
/// content clipped and never wider than the column, so content never
/// grows `egui_table`'s stored width.
fn padded(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    let rect = ui.max_rect().shrink2(egui::vec2(CELL_PAD, 0.0));
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
        |ui| {
            ui.set_clip_rect(rect.intersect(ui.clip_rect()));
            ui.spacing_mut().item_spacing.x = 6.0;
            add(ui);
        },
    );
}

fn label(ui: &mut Ui, text: egui::RichText) {
    ui.add(egui::Label::new(text).truncate().selectable(false));
}

impl TableDelegate for Delegate<'_> {
    fn header_cell_ui(&mut self, ui: &mut Ui, cell: &HeaderCellInfo) {
        ui.painter()
            .rect_filled(ui.max_rect(), 0.0, self.palette.color(Token::Code));
        ui.painter().hline(
            ui.max_rect().x_range(),
            ui.max_rect().bottom() - 0.5,
            Stroke::new(theme::BORDER_WIDTH, self.palette.color(Token::Line)),
        );
        let Some(column) = self.columns.get(cell.group_index) else {
            return;
        };
        let text = self.text(
            column_title(*column),
            CompactStyle::GroupEmphasis,
            Token::Muted,
        );
        padded(ui, |ui| label(ui, text));
    }

    fn row_ui(&mut self, ui: &mut Ui, row_nr: u64) {
        self.scroll.visit(ui);
        let Some(row) = self.row(row_nr) else {
            return;
        };
        let rect = ui.max_rect();
        self.row_rects.insert(row.id, rect);
        let focused = self.findings.focused == Some(row.id);
        let fill = match (row.member, focused) {
            (_, true) => Token::PrimarySoft,
            (None, false) => Token::Rail,
            (Some(_), false) if row.context => Token::Code,
            (Some(_), false) => Token::Surface,
        };
        let painter = ui.painter();
        painter.rect_filled(rect, 0.0, self.palette.color(fill));
        painter.hline(
            rect.x_range(),
            rect.bottom() - 0.5,
            Stroke::new(theme::BORDER_WIDTH, self.palette.color(Token::Line)),
        );
        if focused && self.table_focused {
            painter.rect_stroke(
                rect,
                0.0,
                Stroke::new(theme::BORDER_WIDTH, self.palette.color(Token::Focus)),
                StrokeKind::Inside,
            );
        }
        // Row clicks are read from the pointer, not from a widget under the
        // cells: the cell labels sit above any row-wide widget and take its
        // clicks (egui hit-tests the topmost widget). The Queue cell is left
        // out so a checkbox click never also focuses or compares the row.
        let queue_width = if self.columns.first() == Some(&ColumnId::Queue) {
            column_width(self.findings, ColumnId::Queue)
        } else {
            0.0
        };
        let target =
            Rect::from_min_max(egui::pos2(rect.left() + queue_width, rect.top()), rect.max);
        let over = ui.rect_contains_pointer(target);
        let response = RowClicks {
            clicked: over && ui.input(|i| i.pointer.primary_clicked()),
            secondary_clicked: over && ui.input(|i| i.pointer.secondary_clicked()),
            double_clicked: over
                && ui.input(|i| {
                    i.pointer
                        .button_double_clicked(egui::PointerButton::Primary)
                }),
            pointer: ui.input(|i| i.pointer.interact_pos()),
        };
        if response.clicked || response.secondary_clicked || response.double_clicked {
            ui.memory_mut(|m| m.request_focus(table_id()));
            self.actions.push(Action::Focus(row.id));
        }
        if response.clicked
            && let RowId::Group(group) = row.id
        {
            self.actions.push(Action::ToggleGroup(group));
        }
        if response.double_clicked && row.member.is_some() {
            self.actions.push(Action::Compare(row.id));
        }
        if response.secondary_clicked && row.member.is_some() {
            let at = response.pointer.unwrap_or(rect.left_bottom());
            self.actions.push(Action::RowMenu {
                row: row.id,
                at,
                invoker: table_id(),
            });
        }
    }

    fn cell_ui(&mut self, ui: &mut Ui, cell: &CellInfo) {
        let (Some(row), Some(column)) = (self.row(cell.row_nr), self.columns.get(cell.col_nr))
        else {
            return;
        };
        let group = &self.results.groups[row.group];
        match row.member {
            None => self.header_cell(ui, *column, group),
            Some(index) => self.member_cell(ui, *column, row, &group.members[index], group),
        }
    }

    fn default_row_height(&self) -> f32 {
        ROW_HEIGHT
    }
}

impl Delegate<'_> {
    fn header_cell(&mut self, ui: &mut Ui, column: ColumnId, group: &fuzzy_dedupe_domain::Group) {
        let strong =
            |d: &Self, text: String, token| d.text(text, CompactStyle::GroupEmphasis, token);
        let text = match column {
            ColumnId::Group => {
                let icon = if self.findings.expanded.contains(&group.id) {
                    Icon::Expanded
                } else {
                    Icon::Collapsed
                };
                let chevron = self.text(icon_text(ui.ctx(), icon), CompactStyle::Table, Token::Ink);
                let name = strong(self, projections::group_label(group), Token::Ink);
                padded(ui, |ui| {
                    label(ui, chevron);
                    label(ui, name);
                });
                return;
            }
            ColumnId::Match => strong(
                self,
                projections::header_match_label(group, self.queue).to_owned(),
                Token::Ink,
            ),
            ColumnId::FileName => strong(
                self,
                projections::group_title(group, self.findings, self.queue),
                Token::Ink,
            ),
            ColumnId::Decision => strong(
                self,
                projections::header_decision(group, self.queue, self.findings.last_compared),
                Token::Muted,
            ),
            _ => return,
        };
        padded(ui, |ui| label(ui, text));
    }

    fn member_cell(
        &mut self,
        ui: &mut Ui,
        column: ColumnId,
        row: VisibleRow,
        member: &fuzzy_dedupe_domain::Member,
        group: &fuzzy_dedupe_domain::Group,
    ) {
        let view = projections::member_view(group, self.queue, member);
        let plain = |d: &Self, text: String, token| d.text(text, CompactStyle::Table, token);
        let text = match column {
            ColumnId::Queue => {
                self.checkbox(ui, row, member, &view);
                return;
            }
            ColumnId::Group if row.context => plain(self, "Context".to_owned(), Token::Quiet),
            ColumnId::Group => return,
            ColumnId::Match => plain(self, projections::match_label(group, &view), Token::Ink),
            ColumnId::Unique => plain(self, projections::unique_label(&view), Token::Muted),
            ColumnId::FileName => {
                let icon = plain(self, icon_text(ui.ctx(), Icon::Document), Token::Muted);
                let name = plain(self, member.name.clone(), Token::Ink);
                padded(ui, |ui| {
                    label(ui, icon);
                    label(ui, name);
                });
                return;
            }
            ColumnId::Folder => plain(
                self,
                projections::folder_label(&member.folder),
                Token::Muted,
            ),
            ColumnId::Modified => {
                plain(self, projections::date_label(member.modified), Token::Muted)
            }
            ColumnId::Size => plain(
                self,
                projections::size_label(member.size_bytes),
                Token::Muted,
            ),
            ColumnId::Protected => {
                let protected = member.folder_protected || self.queue.is_protected(member.file);
                plain(
                    self,
                    if protected { "Protected" } else { "—" }.to_owned(),
                    Token::Muted,
                )
            }
            ColumnId::Decision => {
                self.decision_cell(ui, row, member, &view);
                return;
            }
        };
        padded(ui, |ui| label(ui, text));
    }

    fn decision_cell(
        &mut self,
        ui: &mut Ui,
        row: VisibleRow,
        member: &fuzzy_dedupe_domain::Member,
        view: &projections::MemberView,
    ) {
        let decision = projections::decision_label(member, view, self.queue);
        let (style, token) = match decision {
            "Queued" => (CompactStyle::ControlLabel, Token::Red),
            "Review first" | "Counterpart queued" | "Counterpart of a queued file" => {
                (CompactStyle::ControlLabel, Token::Amber)
            }
            "Kept" => (CompactStyle::ControlLabel, Token::Green),
            "Reference" => (CompactStyle::Table, Token::Muted),
            _ => (CompactStyle::Table, Token::Ink),
        };
        let text = self.text(decision, style, token);
        let focused = self.findings.focused == Some(row.id);
        let more = self.text(
            icon_text(ui.ctx(), Icon::RowMenu),
            CompactStyle::Table,
            Token::Muted,
        );
        let mut menu = None;
        padded(ui, |ui| {
            label(ui, text);
            if focused {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let button = ui.add(egui::Button::new(more).frame(false));
                    let response = ui.interact(button.rect, row_menu_button_id(), Sense::click());
                    response.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::Role::Button, true, "Row menu")
                    });
                    if response.clicked() || button.clicked() {
                        menu = Some(egui::pos2(
                            response.rect.right() - ROW_MENU_WIDTH,
                            response.rect.bottom(),
                        ));
                    }
                });
            }
        });
        if let Some(at) = menu {
            self.actions.push(Action::RowMenu {
                row: row.id,
                at,
                invoker: row_menu_button_id(),
            });
        }
    }

    fn checkbox(
        &mut self,
        ui: &mut Ui,
        row: VisibleRow,
        member: &fuzzy_dedupe_domain::Member,
        view: &projections::MemberView,
    ) {
        let state = projections::queue_box(member, view, self.queue);
        if state == QueueBox::None {
            return;
        }
        let rect = Rect::from_center_size(ui.max_rect().center(), egui::vec2(16.0, 16.0));
        let enabled = state != QueueBox::Disabled;
        let sense = if enabled {
            Sense::click()
        } else {
            Sense::hover()
        };
        let response = ui.interact(rect, checkbox_id(member.file), sense);
        let name = format!("Queue {}", member.name);
        let checked = state == QueueBox::Checked;
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::Role::CheckBox, enabled, checked, &name)
        });
        let palette = self.palette;
        let (fill, stroke) = match state {
            QueueBox::Checked => (palette.color(Token::Primary), palette.color(Token::Primary)),
            QueueBox::Disabled => (palette.color(Token::Canvas), palette.color(Token::Line)),
            _ => (palette.color(Token::Surface), palette.color(Token::Quiet)),
        };
        if checked {
            ui.painter()
                .rect_filled(ui.max_rect(), 0.0, palette.color(Token::RedSoft));
        }
        let painter = ui.painter();
        painter.rect(
            rect,
            CornerRadius::same(3),
            fill,
            Stroke::new(theme::BORDER_WIDTH, stroke),
            StrokeKind::Inside,
        );
        if checked {
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                icon_text(ui.ctx(), Icon::Check),
                egui::FontId::new(14.0, CompactStyle::Table.font_id().family),
                palette.color(Token::White),
            );
        }
        if response.has_focus() {
            painter.rect_stroke(
                rect.expand(2.0),
                CornerRadius::same(4),
                Stroke::new(2.0 * theme::BORDER_WIDTH, palette.color(Token::Focus)),
                StrokeKind::Outside,
            );
        }
        // Checkbox clicks never open Compare and never move row focus:
        // queue intent and inspection focus stay independent (§11.4).
        if enabled && response.clicked() {
            self.actions.push(Action::Queue {
                row: row.id,
                invoker: checkbox_id(member.file),
            });
        }
    }
}
