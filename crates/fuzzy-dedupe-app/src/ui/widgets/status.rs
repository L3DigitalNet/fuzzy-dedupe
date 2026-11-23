// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! The compact status bar (spec §11.3, frame `13:648`): one 26-unit line
//! across the window foot with scan counts on the left and the queue count
//! on the right. Per-operation feedback (a refused request, an opener
//! failure) replaces the counts until the next message or a new scan.

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
    let inner = rect.shrink2(egui::vec2(INSET, 0.0));
    ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
        ui.horizontal_centered(|ui| {
            let (text, color) = match &state.status {
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
            ui.label(
                egui::RichText::new(text)
                    .text_style(style.clone())
                    .color(color),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(&line.right)
                        .text_style(style)
                        .color(palette.color(Token::Muted)),
                );
            });
        });
    });
}
