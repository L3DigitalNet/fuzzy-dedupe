// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! The Keyboard shortcuts popup (spec §11.6, frame `44:3681`): the §11.10
//! keyboard map in three columns (Global; Findings; Compare and Popups), one
//! key cap per chord, the note that no shortcut moves a file to Trash, and
//! Close.
//!
//! The rows are [`SHORTCUT_TABLE`], the single copy of §11.10 that T12 checks
//! against the global keymap, so the popup cannot drift from the bindings.
//! Where the frame splits rows (it draws "Findings" and "Compare" as
//! separate rows), the spec table wins (plan wave contract: §11 tables over
//! frames). The one row the popup adds is the frame's Keyboard shortcuts
//! row ([`SHORTCUTS_WINDOW_ROW`]): §11.10 states those chords in prose, not
//! as a table row, and where §11 is silent the frame decides (spec
//! precedence rule; T40 item P6).
//!
//! Each chord of a row gets its own key cap (§11.6, [`chords`]). Caps that
//! do not fit beside the action wrap onto lines below it, so at 200% zoom
//! no cap runs past its column.
//!
//! Each row is one accessible list item named "action: keys", with its text
//! and key caps painted, so a screen reader announces a row once rather than
//! as a label plus a run of cap labels.
//!
//! The dialog host (`ui/widgets/dialog_host.rs`) owns the title row, the close
//! control, Esc, Enter, sizing, focus restoration and the pinned footer slot;
//! this file draws the body and the footer inside the 16-unit content inset.
//! Enter dismisses it, like Close.

use std::sync::Arc;

use egui::text::{LayoutJob, TextFormat};
use egui::{Align2, Color32, Galley, Rect, Sense, Stroke, Ui, Vec2};

use crate::presentation::commands::{SHORTCUT_TABLE, ShortcutRow, ShortcutScope};
use crate::ui::dialogs::{DialogCx, DialogResponse};
use crate::ui::theme::{self, CompactStyle, Palette, Token};
use crate::ui::widgets::dialog_host;

/// The popup's draft, created when it opens and dropped when it closes, so
/// closing never commits it (spec §11.6). The popup has no state of its own.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Draft {}

/// The note under the map (spec §11.6).
pub const NO_TRASH_NOTE: &str = "No shortcut moves a file to Trash, and none is a single letter.";

/// Measurements from frame `44:3681` and its `DD/Compact/Key cap`.
const COLUMN_GAP: f32 = 24.0;
const GROUP_GAP: f32 = 16.0;
const ROW_HEIGHT: f32 = 28.0;
const ROW_GAP: f32 = 6.0;
const HEADING_PAD: f32 = 6.0;
const CAP_PAD: Vec2 = Vec2::new(6.0, 2.0);
const CAP_RADIUS: u8 = 4;
/// Footer controls: control height and the fixed Close width.
pub(crate) const CONTROL_HEIGHT: f32 = 30.0;
pub(crate) const CLOSE_WIDTH: f32 = 80.0;
/// Caps wider than this share of the column move under the action text, so
/// a long chord ("Arrow keys, Page Up/Down, Home/End") never squeezes the
/// action into a one-word column.
const INLINE_CAPS_SHARE: f32 = 0.6;

/// The columns and the scopes each holds, in §11.6 order.
const COLUMNS: [&[ShortcutScope]; 3] = [
    &[ShortcutScope::Global],
    &[ShortcutScope::Findings],
    &[ShortcutScope::Compare, ShortcutScope::Popups],
];

/// The draft for a newly opened popup. Pushes nothing.
pub fn open(cx: &mut DialogCx<'_>) -> Draft {
    let _ = cx;
    Draft::default()
}

/// Draws the map; [`footer`] draws the note and Close. Never closes the
/// popup.
pub fn show(ui: &mut Ui, draft: &mut Draft, cx: &mut DialogCx<'_>) -> DialogResponse {
    let _ = (draft, cx);
    let palette = theme::palette(dialog_host::mode_of(ui.ctx()));
    ui.spacing_mut().item_spacing = Vec2::ZERO;

    let width = ui.available_width();
    let column_width = ((width - COLUMN_GAP * 2.0) / 3.0).max(0.0);
    ui.horizontal_top(|ui| {
        for (index, scopes) in COLUMNS.iter().enumerate() {
            if index > 0 {
                ui.add_space(COLUMN_GAP);
            }
            ui.vertical(|ui| {
                ui.set_width(column_width);
                for (group, scope) in scopes.iter().enumerate() {
                    if group > 0 {
                        ui.add_space(GROUP_GAP);
                    }
                    heading(ui, scope_title(*scope), palette);
                    for row in displayed_rows(*scope) {
                        shortcut_row(ui, row, palette);
                    }
                }
            });
        }
    });
    DialogResponse::KeepOpen
}

/// The no-Trash note and Close, in the footer slot the dialog host pins
/// below the map. Returns [`DialogResponse::Close`] when Close was pressed.
pub fn footer(ui: &mut Ui, draft: &mut Draft, cx: &mut DialogCx<'_>) -> DialogResponse {
    let _ = (draft, cx);
    let palette = theme::palette(dialog_host::mode_of(ui.ctx()));
    close_footer(ui, Some(NO_TRASH_NOTE), palette)
}

/// Frame `44:3681`'s Global row for this window. §11.10 names the chords
/// in its prose ("A shortcuts window (F1 or Ctrl+?) lists it") rather than
/// as a table row, so the row lives here, not in [`SHORTCUT_TABLE`], whose
/// rows T12 checks one for one against the spec table. Both chords are
/// bound in `GLOBAL_KEYMAP`.
pub const SHORTCUTS_WINDOW_ROW: ShortcutRow = ShortcutRow {
    scope: ShortcutScope::Global,
    keys: "F1 / Ctrl+?",
    action: "Keyboard shortcuts",
};

/// The rows `scope`'s column shows: its [`SHORTCUT_TABLE`] rows in table
/// order, with [`SHORTCUTS_WINDOW_ROW`] after Main menu in the Global
/// column, where the frame draws it (or last, should Main menu ever leave
/// the table).
fn displayed_rows(scope: ShortcutScope) -> Vec<&'static ShortcutRow> {
    let mut rows: Vec<&'static ShortcutRow> = SHORTCUT_TABLE
        .iter()
        .filter(|row| row.scope == scope)
        .collect();
    if scope == SHORTCUTS_WINDOW_ROW.scope {
        let at = rows
            .iter()
            .position(|row| row.action == "Main menu")
            .map_or(rows.len(), |index| index + 1);
        rows.insert(at, &SHORTCUTS_WINDOW_ROW);
    }
    rows
}

fn scope_title(scope: ShortcutScope) -> &'static str {
    match scope {
        ShortcutScope::Global => "Global",
        ShortcutScope::Findings => "Findings",
        ShortcutScope::Compare => "Compare",
        ShortcutScope::Popups => "Popups",
    }
}

/// `text` laid out in `style` with the style's line height, which egui's
/// `FontId` cannot carry (see [`CompactStyle::line_height`]).
pub(crate) fn styled_galley(
    ui: &Ui,
    text: &str,
    style: CompactStyle,
    color: Color32,
    wrap_width: f32,
) -> Arc<Galley> {
    let mut job = LayoutJob::single_section(
        text.to_owned(),
        TextFormat {
            font_id: style.font_id(),
            line_height: Some(style.line_height()),
            color,
            ..TextFormat::default()
        },
    );
    job.wrap.max_width = wrap_width;
    ui.ctx().fonts_mut(|fonts| fonts.layout_job(job))
}

fn heading(ui: &mut Ui, title: &str, palette: &Palette) {
    let style = CompactStyle::GroupEmphasis;
    let galley = styled_galley(ui, title, style, palette.color(Token::Ink), f32::INFINITY);
    let height = style.line_height() + HEADING_PAD + theme::BORDER_WIDTH;
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
    let name = title.to_owned();
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Heading, true, &name));
    let painter = ui.painter();
    painter.galley(rect.min, galley, palette.color(Token::Ink));
    let y = rect.bottom() - theme::BORDER_WIDTH / 2.0;
    painter.hline(
        rect.x_range(),
        y,
        Stroke::new(theme::BORDER_WIDTH, palette.color(Token::Line)),
    );
}

/// The chords of one Keys cell, one key cap each (spec §11.6). §11.10
/// writes a row's chords three ways: "Ctrl+1 / Ctrl+2", "Arrow keys, Page
/// Up/Down, Home/End" and "Menu key or Shift+F10". A slash with no spaces
/// round it ("Page Up/Down") is part of one key's name, not a separator.
fn chords(keys: &str) -> Vec<&str> {
    keys.split(" / ")
        .flat_map(|part| part.split(", "))
        .flat_map(|part| part.split(" or "))
        .collect()
}

fn shortcut_row(ui: &mut Ui, row: &ShortcutRow, palette: &Palette) {
    let ink = palette.color(Token::Ink);
    let width = ui.available_width();
    // A cap's text wraps inside the cap only when the chord alone is wider
    // than the column, so no cap ever runs past its column (at 200% the
    // columns of a 1024 × 600 window are about 130 units wide).
    let cap_text_width = (width - 2.0 * CAP_PAD.x).max(1.0);
    let caps: Vec<Arc<Galley>> = chords(row.keys)
        .into_iter()
        .map(|chord| styled_galley(ui, chord, CompactStyle::PathDetail, ink, cap_text_width))
        .collect();
    let cap_size = |cap: &Arc<Galley>| cap.size() + 2.0 * CAP_PAD;
    // Each cap plus the gap after it, less the one gap after the last cap.
    let caps_width = caps
        .iter()
        .map(|cap| cap_size(cap).x + ROW_GAP)
        .sum::<f32>()
        - ROW_GAP;
    let inline = caps_width <= width * INLINE_CAPS_SHARE;
    let label_width = if inline {
        width - caps_width - ROW_GAP
    } else {
        width
    };
    let label = styled_galley(
        ui,
        row.action,
        CompactStyle::Table,
        ink,
        label_width.max(1.0),
    );

    // Caps that do not sit beside the action flow into lines under it,
    // filled in chord order and each aligned right, so a row of three
    // chords that fits no single line takes two rather than overflowing.
    let mut lines: Vec<Vec<Arc<Galley>>> = Vec::new();
    if !inline {
        let mut used = 0.0;
        for cap in caps.iter().cloned() {
            let size = cap_size(&cap).x;
            match lines.last_mut() {
                Some(line) if used + ROW_GAP + size <= width => {
                    used += ROW_GAP + size;
                    line.push(cap);
                }
                _ => {
                    used = size;
                    lines.push(vec![cap]);
                }
            }
        }
    }
    let line_height = |line: &[Arc<Galley>]| {
        line.iter()
            .map(|cap| cap_size(cap).y)
            .fold(0.0_f32, f32::max)
    };

    // The first band holds the action, vertically centred in at least one
    // row height; inline caps sit at its right.
    let band = ROW_HEIGHT.max(label.size().y + ROW_GAP);
    let height = band
        + lines
            .iter()
            .map(|line| line_height(line) + ROW_GAP)
            .sum::<f32>();
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let name = format!("{}: {}", row.action, row.keys);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ListItem, true, &name));

    let painter = ui.painter();
    let first = Rect::from_min_size(rect.min, Vec2::new(width, band));
    let label_pos = Align2::LEFT_CENTER
        .align_size_within_rect(label.size(), first)
        .min;
    painter.galley(label_pos, label, ink);

    let paint_line = |line: &[Arc<Galley>], band: Rect| {
        let mut right = band.right();
        for cap in line.iter().rev() {
            let cap_rect = Align2::RIGHT_CENTER.align_size_within_rect(
                cap_size(cap),
                Rect::from_min_max(band.min, egui::pos2(right, band.bottom())),
            );
            painter.rect(
                cap_rect,
                CAP_RADIUS,
                palette.color(Token::Code),
                Stroke::new(theme::BORDER_WIDTH, palette.color(Token::Line)),
                egui::StrokeKind::Inside,
            );
            painter.galley(cap_rect.min + CAP_PAD, Arc::clone(cap), ink);
            right = cap_rect.left() - ROW_GAP;
        }
    };
    if inline {
        paint_line(&caps, first);
    } else {
        let mut top = first.bottom();
        for line in &lines {
            let h = line_height(line);
            paint_line(
                line,
                Rect::from_min_size(egui::pos2(rect.left(), top), Vec2::new(width, h)),
            );
            top += h + ROW_GAP;
        }
    }
}

/// The footer row shared by Keyboard shortcuts and About, drawn in the
/// dialog host's pinned footer slot: an optional supporting note at the
/// left and an 80-wide Close at the right.
///
/// The note's width leaves exactly the spacing token and Close, so the row
/// sets its own zero item spacing: egui's default spacing after the note
/// would push Close past the 16-unit content inset.
pub(crate) fn close_footer(ui: &mut Ui, note: Option<&str>, palette: &Palette) -> DialogResponse {
    let mut response = DialogResponse::KeepOpen;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing = Vec2::ZERO;
        let spacing: f32 = theme::SPACING_SCALE[2].into();
        let note_width = (ui.available_width() - CLOSE_WIDTH - spacing).max(0.0);
        ui.allocate_ui_with_layout(
            Vec2::new(note_width, CONTROL_HEIGHT),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_min_size(Vec2::new(note_width, CONTROL_HEIGHT));
                if let Some(note) = note {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(note)
                                .text_style(CompactStyle::Supporting.text_style())
                                .line_height(Some(CompactStyle::Supporting.line_height()))
                                .color(palette.color(Token::Muted)),
                        )
                        .wrap(),
                    );
                }
            },
        );
        ui.add_space(spacing);
        let close = egui::Button::new("Close").min_size(Vec2::new(CLOSE_WIDTH, CONTROL_HEIGHT));
        if ui.add(close).clicked() {
            response = DialogResponse::Close;
        }
    });
    response
}
