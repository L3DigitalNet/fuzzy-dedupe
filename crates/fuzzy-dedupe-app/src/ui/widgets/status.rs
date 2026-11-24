// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! The compact status bar (spec §11.3, frames `13:648`, `13:1304`): one
//! 26-unit line across the window foot with the workspace's summary on the
//! left and the queue count on the right
//! ([`projections::status_line`]). Per-operation feedback (a refused
//! request, an opener failure) replaces the left text until the next
//! message or a new scan.
//!
//! The line never wraps and its two texts never overprint, at any window
//! size or zoom (spec line 869, NFR-005). When both do not fit, each is
//! elided with "…" inside its own share of the bar, by this priority: the
//! right text keeps its whole width up to half the bar, or more when the
//! left text leaves room, because it carries the queue count cleanup acts
//! on and is normally the shorter text; the left text gets the rest, so it
//! never drops below half the bar unless it needs less. Rejected: hiding
//! the left text outright, which would also hide a refusal or error
//! message.

use egui::{Align, Layout, Rect, Sense, Stroke};

use crate::presentation::projections;
use crate::presentation::state::{PresentationState, Severity};
use crate::ui::theme::{self, CompactStyle, Token};
use crate::ui::widgets::dialog_host::mode_of;

/// The status bar's height in units (spec §11.3).
pub const HEIGHT: f32 = 26.0;
/// The bar's horizontal text inset (frame `44:4033`).
pub const INSET: f32 = 12.0;
/// The accessible name the bar's region carries.
pub const ACCESSIBLE_NAME: &str = "Status bar";

/// Draws the bar into `ui`, which the shell sizes to [`HEIGHT`].
pub fn show(ui: &mut egui::Ui, state: &PresentationState) {
    let palette = theme::palette(mode_of(ui.ctx()));
    let rect: Rect = ui.max_rect();
    ui.painter().rect(
        rect,
        0.0,
        palette.color(Token::Surface),
        Stroke::new(theme::BORDER_WIDTH, palette.color(Token::Line)),
        egui::StrokeKind::Inside,
    );
    let region = ui.interact(rect, egui::Id::unique("fd.status"), Sense::hover());
    region.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Group, true, ACCESSIBLE_NAME));

    let line = projections::status_line(state);
    let style = CompactStyle::Supporting.text_style();
    let font = CompactStyle::Supporting.font_id();
    let (left, left_color) = match &state.status {
        Some(message) => (
            message.text.as_str(),
            match message.severity {
                Severity::Info => palette.color(Token::Muted),
                Severity::Warning => palette.color(Token::Amber),
                Severity::Error => palette.color(Token::Red),
            },
        ),
        None => (line.left.as_str(), palette.color(Token::Muted)),
    };
    let muted = palette.color(Token::Muted);
    let natural = |text: &str| {
        ui.painter()
            .layout_no_wrap(text.to_owned(), font.clone(), muted)
            .size()
            .x
    };
    let inner = rect.shrink2(egui::vec2(INSET, 0.0));
    let (left_width, right_width) = shares(inner.width(), natural(left), natural(&line.right));

    let left_rect = Rect::from_min_size(inner.left_top(), egui::vec2(left_width, inner.height()));
    let right_rect = Rect::from_min_max(
        egui::pos2(inner.right() - right_width, inner.top()),
        inner.right_bottom(),
    );
    for (rect, layout, text, color) in [
        (
            left_rect,
            Layout::left_to_right(Align::Center),
            left,
            left_color,
        ),
        (
            right_rect,
            Layout::right_to_left(Align::Center),
            line.right.as_str(),
            muted,
        ),
    ] {
        ui.scope_builder(egui::UiBuilder::new().max_rect(rect).layout(layout), |ui| {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(text)
                        .text_style(style.clone())
                        .color(color),
                )
                .truncate(),
            );
        });
    }
}

/// The gap kept between the two texts when either is elided.
const TEXT_GAP: f32 = INSET;

/// The widths the left and right texts may take in a bar `available` wide,
/// given their unelided widths, by the priority in the module header.
fn shares(available: f32, left: f32, right: f32) -> (f32, f32) {
    let room = (available - TEXT_GAP).max(0.0);
    if left + right <= room {
        return (left, right);
    }
    let right = right.min((room - left).max(room / 2.0));
    (room - right, right)
}
