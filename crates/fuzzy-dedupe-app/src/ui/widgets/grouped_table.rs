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
//!
//! Flexible columns (§11.4: "File name and folder widths are flexible"):
//! the widths drawn are [`fitted_widths`] of the stored ones, which equal
//! them whenever the table has room, as at 1600 × 960. Below that, File
//! name and Folder give way first, down to [`FLEX_MINIMUMS`], then every
//! other column but Queue gives way proportionally down to its own floor
//! (Group never below its widest label, [`group_label_width`]), and only
//! then does the table scroll sideways. The stored widths, which
//! the preferences keep, are never rewritten by fitting: a window made wide
//! again draws the user's widths again.

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
use crate::ui::glyphs;
use crate::ui::theme::{self, CompactStyle, Icon, Palette, Token};
use crate::ui::widgets::dialog_host::mode_of;

/// Header and body row heights at default zoom (spec §11.3).
pub const HEADER_HEIGHT: f32 = 30.0;
pub const ROW_HEIGHT: f32 = 28.0;
/// Horizontal cell padding (frame `13:633`).
pub const CELL_PAD: f32 = 8.0;
/// The gap between items inside one cell, as the Group cell's chevron and
/// label.
const CELL_GAP: f32 = 6.0;
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
/// What the body reserves right of the columns for its scrollbar: the
/// reference table's 1422-unit inner width less the 1408 the §11.4 widths
/// total (frame `13:648`).
pub const SCROLLBAR_RESERVE: f32 = 14.0;
/// The reference window width (spec §11.3, 1600 × 960): at or above it the
/// columns keep their stored widths.
pub const REFERENCE_WINDOW_WIDTH: f32 = 1600.0;
/// The floors File name and Folder shrink to before any other column gives
/// way (§11.4 flexible columns). File name keeps its icon and about twelve
/// characters; Folder keeps "~/…/" and a short last component.
pub const FLEX_MINIMUMS: [(ColumnId, f32); 2] =
    [(ColumnId::FileName, 140.0), (ColumnId::Folder, 96.0)];
/// The floor every other column shrinks to once File name and Folder are
/// at theirs, as a share of its stored width. Queue never shrinks: its
/// checkbox and header need the whole 48 (frame `13:648`).
const SECOND_STAGE_FLOOR: f32 = 0.55;
/// Accessible name of the full-path line a keyboard-focused member row
/// shows (spec line 1125); its value is the full path.
pub const FULL_PATH_NAME: &str = "Full path";

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

/// The ⋯ button of `file`'s member row, stable per file: the invoker
/// focus returns to when the row menu it opened closes.
#[must_use]
pub fn row_menu_button_id(file: FileId) -> Id {
    Id::unique(("fd.findings.row_menu_button", file))
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

/// The widths `columns` are drawn at in a table body `room` units wide
/// (columns plus [`SCROLLBAR_RESERVE`]), in a window `window_width` wide,
/// in order. `group_floor` is the narrowest Group may become: the width of
/// the widest group label in the current results ([`group_label_width`]),
/// or 0 where no group row is drawn.
///
/// Stage 0: the stored widths, whenever they fit or the window is at least
/// the 1600-unit reference width, so §11.4's widths hold there exactly
/// ("only below the reference width" do columns give way). Stage 1: File
/// name and Folder shrink in proportion to their excess over
/// [`FLEX_MINIMUMS`]. Stage 2: every column but Queue shrinks in proportion
/// to its excess over [`SECOND_STAGE_FLOOR`] of its stored width, except
/// that Group stops at `group_floor`: below it "G002 (3)" and "G004 (3)"
/// both elide to "G00…" and group identity is lost, so the other columns
/// absorb the difference instead. Past that the widths stay at their
/// floors and the body scrolls sideways (at
/// 1024 × 600 under 200% zoom no readable set of the always-shown columns
/// fits 440 points). Rejected: shrinking every column from the start, which
/// would move the evidence columns before the two §11.4 names as flexible.
#[must_use]
pub fn fitted_widths(
    findings: &FindingsState,
    columns: &[ColumnId],
    room: f32,
    window_width: f32,
    group_floor: f32,
) -> Vec<f32> {
    let stored: Vec<f32> = columns.iter().map(|c| column_width(findings, *c)).collect();
    let target = (room - SCROLLBAR_RESERVE).max(0.0);
    let total: f32 = stored.iter().sum();
    if total <= target || window_width >= REFERENCE_WINDOW_WIDTH {
        return stored;
    }
    let flex_floor = |c: ColumnId, w: f32| {
        FLEX_MINIMUMS
            .iter()
            .find(|(f, _)| *f == c)
            .map(|(_, min)| min.min(w))
    };
    let stage = |widths: &[f32], floor: &dyn Fn(usize) -> Option<f32>, excess: f32| {
        let slack: f32 = (0..widths.len())
            .filter_map(|i| floor(i).map(|f| widths[i] - f))
            .sum();
        let take = excess.min(slack);
        let mut out = widths.to_vec();
        if slack > 0.0 {
            for (i, w) in out.iter_mut().enumerate() {
                if let Some(f) = floor(i) {
                    *w -= take * (*w - f) / slack;
                }
            }
        }
        out
    };
    let first = stage(
        &stored,
        &|i| flex_floor(columns[i], stored[i]),
        total - target,
    );
    let total: f32 = first.iter().sum();
    if total <= target {
        return first;
    }
    let floor = |i: usize| {
        let c = columns[i];
        if c == ColumnId::Queue {
            None
        } else {
            let share = flex_floor(c, stored[i]).unwrap_or(stored[i] * SECOND_STAGE_FLOOR);
            let floor = if c == ColumnId::Group {
                share.max(group_floor)
            } else {
                share
            };
            Some(floor.min(first[i]))
        }
    };
    stage(&first, &floor, total - target)
}

/// The width a Group cell needs to show its widest content whole: the cell
/// padding, the chevron, the gap after it and the widest group label in
/// `results` (or the "Group" title, if wider), at the fonts `header_cell`
/// and `header_title` draw with. The floor [`fitted_widths`] keeps Group at.
///
/// Every group counts, not only the visible or filtered ones, so the width
/// does not jump as the user scrolls, searches or filters. The `large`
/// scenario has 100,000 groups, so labels are ranked by the sum of their
/// glyph advances, which needs no layout, and only the widest is laid out;
/// the result is cached per results value and scale.
#[must_use]
pub fn group_label_width(ui: &Ui, results: &Arc<ScanResults>) -> f32 {
    #[derive(Clone)]
    struct Cached {
        results: std::sync::Weak<ScanResults>,
        pixels_per_point: f32,
        width: f32,
    }
    let id = Id::unique("fd.findings.group_label_width");
    let pixels_per_point = ui.ctx().pixels_per_point();
    let hit = ui.ctx().data(|d| {
        d.get_temp::<Cached>(id).filter(|c| {
            c.pixels_per_point.to_bits() == pixels_per_point.to_bits()
                && std::ptr::eq(c.results.as_ptr(), Arc::as_ptr(results))
                && c.results.strong_count() > 0
        })
    });
    if let Some(cached) = hit {
        return cached.width;
    }
    let font = CompactStyle::GroupEmphasis.font_id();
    let widest = ui.ctx().fonts_mut(|fonts| {
        results
            .groups
            .iter()
            .map(|group| {
                let text = projections::group_label(group);
                let advance: f32 = text.chars().map(|c| fonts.glyph_width(&font, c)).sum();
                (advance, text)
            })
            .max_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, text)| text)
    });
    let measure = |text: String, font: egui::FontId| {
        ui.painter()
            .layout_no_wrap(text, font, egui::Color32::WHITE)
            .size()
            .x
    };
    let chevron = [Icon::Expanded, Icon::Collapsed]
        .into_iter()
        .filter_map(|icon| glyphs::text(ui.ctx(), icon))
        .map(|text| measure(text, CompactStyle::Table.font_id()))
        .fold(0.0, f32::max);
    let name = widest.map_or(0.0, |text| measure(text, font.clone()));
    let title = measure(column_title(ColumnId::Group).to_owned(), font);
    // `padded`'s horizontal padding and item spacing; the extra unit
    // absorbs layout rounding, without which a label measured exactly at
    // its width can still elide.
    let width = 2.0 * CELL_PAD + (chevron + CELL_GAP + name).max(title) + 1.0;
    ui.ctx().data_mut(|d| {
        d.insert_temp(
            id,
            Cached {
                results: Arc::downgrade(results),
                pixels_per_point,
                width,
            },
        );
    });
    width
}

/// The header row alone, across the top of `ui`'s remaining rect, for the
/// table region's states that show no rows (frames `28:1399`, `28:2171`,
/// `30:2370`): the same titles, fill and widths as the table's header, so
/// the region reads as the table, empty. Drawn by the view after it frames
/// the region; returns the strip's rect.
pub fn header_strip(ui: &mut Ui, findings: &FindingsState) -> Rect {
    let palette = theme::palette(mode_of(ui.ctx()));
    let region = ui.available_rect_before_wrap().shrink(theme::BORDER_WIDTH);
    let strip = Rect::from_min_size(region.min, egui::vec2(region.width(), HEADER_HEIGHT));
    let columns = visible_columns(findings, ui.ctx().content_rect().width());
    let widths = fitted_widths(
        findings,
        &columns,
        region.width(),
        ui.ctx().content_rect().width(),
        0.0,
    );
    ui.painter()
        .rect_filled(strip, 0.0, palette.color(Token::Code));
    let mut left = strip.left();
    for (column, width) in columns.iter().zip(widths) {
        let cell = Rect::from_min_size(
            egui::pos2(left, strip.top()),
            egui::vec2(width, HEADER_HEIGHT),
        )
        .intersect(strip);
        left += width;
        if cell.width() <= 0.0 {
            break;
        }
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(cell));
        header_title(&mut child, *column, palette);
    }
    strip
}

/// One header cell's title: muted group-emphasis text at the cell's left
/// padding. Queue keeps no right padding, so its title shows whole in the
/// 48-unit column (frame `13:648` sets "Queue" at 8 in a 48 cell, 40 wide)
/// instead of eliding to "Qu…".
fn header_title(ui: &mut Ui, column: ColumnId, palette: &Palette) {
    let text = egui::RichText::new(column_title(column))
        .text_style(CompactStyle::GroupEmphasis.text_style())
        .color(palette.color(Token::Muted));
    if column == ColumnId::Queue {
        let max = ui.max_rect();
        let rect = Rect::from_min_max(egui::pos2(max.left() + CELL_PAD, max.top()), max.max);
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(rect)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
            |ui| {
                ui.set_clip_rect(max.intersect(ui.clip_rect()));
                ui.add(
                    egui::Label::new(text)
                        .wrap_mode(egui::TextWrapMode::Extend)
                        .selectable(false),
                );
            },
        );
    } else {
        padded(ui, |ui| label(ui, text));
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
    let palette = theme::palette(mode_of(ui.ctx()));
    let window_width = ui.ctx().content_rect().width();
    let columns = visible_columns(findings, window_width);
    let outer = ui.available_rect_before_wrap();
    let page = rows_per_page(outer.height());
    let widths = fitted_widths(
        findings,
        &columns,
        outer.width() - 2.0 * theme::BORDER_WIDTH,
        window_width,
        group_label_width(ui, &results),
    );

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
        .columns(table_columns(&columns, &widths))
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
        widths: &widths,
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
                // `egui_table` draws every column's right edge in this
                // stroke; frame `13:648` has no vertical cell rules. The
                // hovered and dragged strokes stay, so a resize handle
                // still shows where it can be grabbed.
                ui.visuals_mut().widgets.noninteractive.bg_stroke = Stroke::NONE;
                table_scrollbar(ui, palette);
                let state_id = table.get_id(ui);
                write_widths(ui, state_id, &columns, &widths);
                table.show(ui, &mut delegate);
                state_id
            },
        )
        .inner;
    let row_rects = std::mem::take(&mut delegate.row_rects);
    let scroll_area = delegate.scroll.area;

    read_widths(ui, state_id, findings, &columns, &widths);
    // Not on a restoring pass: its scroll area still holds the old offset,
    // and recording that would overwrite the offset being restored. Later
    // passes record the restored one once the area has reached it.
    if !restoring && let Some(offset) = ScrollProbe::offset(ui.ctx(), scroll_area) {
        findings.scroll_offset = offset;
    }

    if table_focused {
        focused_full_path(ui, findings, &results, &row_rects, outer, palette);
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

/// Styles the scroll bars of the scroll area `egui_table` creates inside
/// `ui`: frame `13:648` shows the vertical bar at rest, a `quiet` thumb on a
/// `code` track inside the [`SCROLLBAR_RESERVE`].
///
/// egui's default floating style keeps the bar hidden until the pointer
/// hovers it, and `egui_table` exposes no scroll-area setting, so the table
/// draws inside a `Ui` whose scroll style is solid (opaque at rest). The
/// solid bar's 4 + 6 + 3 units fit the 14-unit reserve, so the columns,
/// which leave exactly the reserve free, never trigger sideways scrolling.
/// Like any egui bar it shows only while the rows overflow the body: with
/// nothing to scroll there is no thumb to show.
///
/// SCOPE: `ui` is the table's own scope. The thumb colour is egui's
/// `inactive.fg_stroke` and the track `extreme_bg_color`; nothing in the
/// table reads either (its cells paint with palette colours), and setting
/// them on the theme instead would restyle every other scroll area and
/// text field.
fn table_scrollbar(ui: &mut Ui, palette: &Palette) {
    ui.spacing_mut().scroll = egui::style::ScrollStyle {
        bar_width: 6.0,
        bar_inner_margin: 4.0,
        bar_outer_margin: 3.0,
        foreground_color: true,
        ..egui::style::ScrollStyle::solid()
    };
    let visuals = ui.visuals_mut();
    visuals.extreme_bg_color = palette.color(Token::Code);
    visuals.widgets.inactive.fg_stroke.color = palette.color(Token::Quiet);
}

/// Shows [`full_path`] for the focused row when it is a visible member row.
fn focused_full_path(
    ui: &Ui,
    findings: &FindingsState,
    results: &ScanResults,
    row_rects: &HashMap<RowId, Rect>,
    table: Rect,
    palette: &Palette,
) {
    if let Some(row @ RowId::Member { file, .. }) = findings.focused
        && let Some(rect) = row_rects.get(&row)
        && let Some(member) = results.group_of(file).and_then(|g| g.member(file))
    {
        full_path(ui, *rect, table, &member.display_path, palette);
    }
}

/// The focused member row's full path, shown while the table has keyboard
/// focus (spec line 1125: "keyboard-reachable details …; hover alone is
/// insufficient"). Rows are not widgets (see [`table_id`]), so a focus
/// tooltip is drawn here, under the row (above it near the table's foot),
/// wrapped to the table's width so the whole path shows at any zoom. Copy
/// path stays Ctrl+Shift+C and the row menu. Rejected: widening the Folder
/// cell inline, which would push the Decision column off a narrow table.
fn full_path(ui: &Ui, row: Rect, table: Rect, path: &str, palette: &Palette) {
    let max_width = (table.width() - 4.0 * CELL_PAD).max(80.0);
    let below = row.bottom() + 2.0 + 3.0 * ROW_HEIGHT <= table.bottom();
    let anchor = egui::pos2(
        row.left() + CELL_PAD,
        if below {
            row.bottom() + 2.0
        } else {
            row.top() - 2.0
        },
    );
    let pivot = if below {
        Align2::LEFT_TOP
    } else {
        Align2::LEFT_BOTTOM
    };
    egui::Area::new(Id::unique("fd.findings.full_path"))
        .order(egui::Order::Tooltip)
        .interactable(false)
        .constrain(true)
        .pivot(pivot)
        .fixed_pos(anchor)
        .show(ui.ctx(), |ui| {
            egui::Frame::new()
                .fill(palette.color(Token::Surface))
                .stroke(Stroke::new(theme::BORDER_WIDTH, palette.color(Token::Line)))
                .corner_radius(theme::CORNER_RADIUS)
                .shadow(theme::shadow(mode_of(ui.ctx())))
                .inner_margin(egui::Margin::symmetric(8, 4))
                .show(ui, |ui| {
                    ui.set_max_width(max_width);
                    let response = ui.add(
                        egui::Label::new(
                            egui::RichText::new(path)
                                .text_style(CompactStyle::PathDetail.text_style())
                                .color(palette.color(Token::Ink)),
                        )
                        .wrap_mode(egui::TextWrapMode::Wrap)
                        .selectable(false),
                    );
                    // A tooltip node named FULL_PATH_NAME whose value is the
                    // path. Not a Label: the Findings tests read every Label
                    // under the header as a table cell, and this line sits
                    // over the next row.
                    response.widget_info(|| {
                        let mut info =
                            egui::WidgetInfo::labeled(egui::Role::Tooltip, true, FULL_PATH_NAME);
                        info.current_text_value = Some(path.to_owned());
                        info
                    });
                });
        });
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

/// `egui_table`'s columns at their fitted widths, keyed per column so a
/// hidden column never shifts another's stored width.
fn table_columns(columns: &[ColumnId], widths: &[f32]) -> Vec<Column> {
    columns
        .iter()
        .zip(widths)
        .map(|(c, width)| {
            Column::new(*width)
                .id(column_key(*c))
                .range(24.0..=f32::INFINITY)
        })
        .collect()
}

fn column_key(column: ColumnId) -> Id {
    Id::unique(("fd.findings.column", column))
}

/// Puts the fitted widths of `FindingsState::column_widths` into
/// `egui_table`'s stored state, so the table neither auto-sizes nor keeps a
/// width of its own.
fn write_widths(ui: &Ui, state_id: Id, columns: &[ColumnId], widths: &[f32]) {
    let mut stored = egui_table::TableState::load(ui.ctx(), state_id).unwrap_or_default();
    for (i, (c, width)) in columns.iter().zip(widths).enumerate() {
        let column = Column::new(0.0).id(column_key(*c));
        stored.col_widths.insert(column.id_for(i), *width);
    }
    stored.parent_width = Some(ui.available_width());
    stored.store(ui.ctx(), state_id);
}

/// Takes back a drag-resize: the stored width of a column whose resize
/// handle is being dragged. Only a drag counts: `egui_table` also rewrites
/// widths on its own sizing passes (egui's first-frame and discard passes),
/// and taking those would shrink every column to its content. The drag is
/// measured against the fitted width it started from, so a drag on a
/// shrunk column stores the width the user sees.
fn read_widths(
    ui: &Ui,
    state_id: Id,
    findings: &mut FindingsState,
    columns: &[ColumnId],
    fitted: &[f32],
) {
    let Some(after) = egui_table::TableState::load(ui.ctx(), state_id) else {
        return;
    };
    for (i, (c, drawn)) in columns.iter().zip(fitted).enumerate() {
        let column = Column::new(0.0).id(column_key(*c));
        let handle = state_id.with(column.id_for(i)).with("resize");
        if ui.ctx().is_being_dragged(handle)
            && let Some(width) = after.col_widths.get(&column.id_for(i))
            && (width - drawn).abs() > 0.25
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
    /// The fitted widths `columns` are drawn at.
    widths: &'a [f32],
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
            ui.spacing_mut().item_spacing.x = CELL_GAP;
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
        header_title(ui, *column, self.palette);
    }

    fn row_ui(&mut self, ui: &mut Ui, row_nr: u64) {
        self.scroll.visit(ui);
        let Some(row) = self.row(row_nr) else {
            return;
        };
        let rect = ui.max_rect();
        self.row_rects.insert(row.id, rect);
        let focused = self.findings.focused == Some(row.id);
        // A queued member row is filled `code` across the whole row (frames
        // `13:648`, `29:2214`, `37:2466`); its Queue cell adds redSoft on
        // top in `checkbox`. Queued is read from `queue_box`, the same test
        // that checks the box and paints that redSoft, so the row tint and
        // the cell tint never disagree; a reference row has no box and so
        // keeps `surface`. Read at draw time, not from the projection: the
        // projection is cached and need not rebuild when the queue changes.
        let queued = row.member.is_some_and(|index| {
            let group = &self.results.groups[row.group];
            let member = &group.members[index];
            let view = projections::member_view(group, self.queue, member);
            projections::queue_box(member, &view, self.queue) == QueueBox::Checked
        });
        let fill = match (row.member, focused) {
            (_, true) => Token::PrimarySoft,
            (None, false) => Token::Rail,
            (Some(_), false) if row.context || queued => Token::Code,
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
            self.widths.first().copied().unwrap_or(0.0)
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
                let chevron = self.text(
                    glyphs::text(ui.ctx(), icon).unwrap_or_default(),
                    CompactStyle::Table,
                    Token::Ink,
                );
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
            ColumnId::Unique => {
                // A count paints `amber`, the §11.7 recoverable-caution
                // role (frames `13:648`, `29:2214`, `37:2466`, `44:3954`):
                // it mirrors the owner's gate that Review unique text
                // applies only when Unique > 0. Read from the numeric
                // count, not the label, so the colour and `unique_label`'s
                // "—" for None or zero cannot drift apart.
                let token = if view.unique.is_some_and(|n| n > 0) {
                    Token::Amber
                } else {
                    Token::Muted
                };
                plain(self, projections::unique_label(&view), token)
            }
            ColumnId::FileName => {
                // The reference row carries the anchor and a bold name
                // (frame `13:648`); `member_view` names the current
                // reference, which Make this the reference can move.
                // A protected non-reference row carries the lock in place
                // of the file icon, in the ink token (frame `13:648`'s
                // Requirements_v2 row); a protected reference keeps its
                // anchor, as Project_Plan does there.
                let reference = view.counterpart.is_none();
                let (icon, token) = if reference {
                    (Icon::Reference, Token::Ink)
                } else if is_protected(member, self.queue) {
                    (Icon::Protected, Token::Ink)
                } else {
                    (Icon::Document, Token::Muted)
                };
                let icon = plain(
                    self,
                    glyphs::text(ui.ctx(), icon).unwrap_or_default(),
                    token,
                );
                let name = if reference {
                    self.text(member.name.clone(), CompactStyle::GroupEmphasis, Token::Ink)
                } else {
                    plain(self, member.name.clone(), Token::Ink)
                };
                padded(ui, |ui| {
                    label(ui, icon);
                    label(ui, name);
                });
                return;
            }
            ColumnId::Folder => {
                self.folder_cell(ui, &projections::folder_label(&member.folder));
                return;
            }
            ColumnId::Modified => {
                plain(self, projections::date_label(member.modified), Token::Muted)
            }
            ColumnId::Size => plain(
                self,
                projections::size_label(member.size_bytes),
                Token::Muted,
            ),
            ColumnId::Protected => {
                // Frame `13:648` sets "Protected" in the green token.
                if is_protected(member, self.queue) {
                    plain(self, "Protected".to_owned(), Token::Green)
                } else {
                    plain(self, "—".to_owned(), Token::Muted)
                }
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
            "Protected" => (CompactStyle::Table, Token::Green),
            "Reference" => (CompactStyle::Table, Token::Muted),
            _ => (CompactStyle::Table, Token::Ink),
        };
        let text = self.text(decision, style, token);
        let focused = self.findings.focused == Some(row.id);
        let more = glyphs::text(ui.ctx(), Icon::RowMenu).unwrap_or_default();
        let button_id = row_menu_button_id(member.file);
        let mut menu = None;
        let palette = self.palette;
        padded(ui, |ui| {
            // The ⋯ is reserved first, at the cell's right edge, so a long
            // decision label elides before it instead of covering it.
            let cell = ui.max_rect();
            let button = Rect::from_min_max(
                egui::pos2(cell.right() - ROW_MENU_BUTTON_WIDTH, cell.top()),
                cell.max,
            );
            let text_rect =
                Rect::from_min_max(cell.min, egui::pos2(button.left() - 4.0, cell.bottom()));
            ui.scope_builder(
                egui::UiBuilder::new()
                    .max_rect(text_rect)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
                |ui| label(ui, text),
            );
            // Every member row shows ⋯ (frame `13:648`). Only the focused
            // row's is a Tab stop: one per visible row would put dozens of
            // stops between the toolbar and whatever follows, and the
            // keyboard route to a row menu is the Menu key or Shift+F10.
            let sense = if focused {
                Sense::click()
            } else {
                Sense::CLICK
            };
            let response = ui.interact(button, button_id, sense);
            response
                .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Row menu"));
            let ink = palette.color(if response.hovered() || response.has_focus() {
                Token::Ink
            } else {
                Token::Muted
            });
            ui.painter().text(
                button.center(),
                Align2::CENTER_CENTER,
                &more,
                CompactStyle::Table.font_id(),
                ink,
            );
            if response.clicked() {
                menu = Some(egui::pos2(button.right() - ROW_MENU_WIDTH, button.bottom()));
            }
        });
        if let Some(at) = menu {
            self.actions.push(Action::RowMenu {
                row: row.id,
                at,
                invoker: button_id,
            });
        }
    }

    /// The Folder cell: the folder shortened from the front to fit the
    /// cell, keeping "~/" or "/" and as many trailing components as fit,
    /// with "…/" for the rest (frame `29:2214`: "~/…/Projects/"). Only the
    /// drawn text shortens; the accessible value, Copy path and the search
    /// keep the stored folder (§11.4 "shorten only the visual
    /// representation").
    fn folder_cell(&self, ui: &mut Ui, folder: &str) {
        let palette = self.palette;
        padded(ui, |ui| {
            let room = ui.max_rect().width();
            let font = CompactStyle::Table.font_id();
            let shown = shorten_folder(folder, |s| {
                ui.painter()
                    .layout_no_wrap(s.to_owned(), font.clone(), palette.color(Token::Muted))
                    .size()
                    .x
                    <= room
            });
            let response = ui.add(
                egui::Label::new(
                    egui::RichText::new(shown)
                        .text_style(CompactStyle::Table.text_style())
                        .color(palette.color(Token::Muted)),
                )
                .truncate()
                .selectable(false),
            );
            let full = folder.to_owned();
            response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &full));
        });
    }

    fn checkbox(
        &mut self,
        ui: &mut Ui,
        row: VisibleRow,
        member: &fuzzy_dedupe_domain::Member,
        view: &projections::MemberView,
    ) {
        let state = projections::queue_box(member, view, self.queue);
        // A protected member shows no checkbox at all (frame `13:648`): it
        // can never be queued, and its lock and Protected value say why. A
        // counterpart-queued row keeps its disabled box, because it becomes
        // queueable again once its counterpart is unqueued.
        if state == QueueBox::None
            || (state == QueueBox::Disabled && is_protected(member, self.queue))
        {
            return;
        }
        // A member that needs review first keeps a working checkbox, outlined
        // in the amber token on the amber soft fill (frame `13:648`), so the
        // caution shows before the click that would ask for that review.
        let review_first = state == QueueBox::Unchecked
            && projections::decision_label(member, view, self.queue) == "Review first";
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
            _ if review_first => (palette.color(Token::AmberSoft), palette.color(Token::Amber)),
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
                glyphs::text(ui.ctx(), Icon::Check).unwrap_or_default(),
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

/// Whether `member` is protected, by its folder or by the user: the same
/// rule `projections::queue_box` and `projections::decision_label` apply.
fn is_protected(member: &fuzzy_dedupe_domain::Member, queue: &QueueState) -> bool {
    member.folder_protected || queue.is_protected(member.file)
}

/// The ⋯ hit area at a Decision cell's right edge.
const ROW_MENU_BUTTON_WIDTH: f32 = 20.0;

/// `folder` as drawn in a Folder cell: whole when `fits` accepts it, else
/// its root ("~/" or "/") then "…/" then the longest run of trailing
/// components `fits` accepts, else "…/" and the last component alone (the
/// label then elides that). `folder` ends in "/" (`folder_label`).
#[must_use]
pub fn shorten_folder(folder: &str, fits: impl Fn(&str) -> bool) -> String {
    if fits(folder) {
        return folder.to_owned();
    }
    let (root, rest) = if let Some(rest) = folder.strip_prefix("~/") {
        ("~/", rest)
    } else if let Some(rest) = folder.strip_prefix('/') {
        ("/", rest)
    } else {
        ("", folder)
    };
    let parts: Vec<&str> = rest.trim_end_matches('/').split('/').collect();
    for skip in 1..parts.len() {
        let candidate = format!("{root}…/{}/", parts[skip..].join("/"));
        if fits(&candidate) {
            return candidate;
        }
    }
    format!("{root}…/{}/", parts.last().copied().unwrap_or_default())
}
