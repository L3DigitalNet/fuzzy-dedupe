// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! The About popup (spec §11.6, frame `44:3858`): the GPLv3 Appropriate
//! Legal Notices (FR-028).
//!
//! The summary shows the product name and the package version, a one-line
//! description, the copyright line, the `GPL-3.0-only` and no-warranty
//! statements, and three actions:
//! - View license and Third-party notices replace the summary with the
//!   embedded text in a read-only, selectable `DD/Compact/document` view with
//!   Back. The swap happens inside this popup's draft, never as a second
//!   popup, so About stays the one open popup (FR-019).
//! - Source code opens the package `repository` address in the browser
//!   through the opener adapter (C-07), as a `UiIntent::OpenWebAddress`.
//!
//! Legal-text contract: `LICENSE` and `THIRD-PARTY-NOTICES.md` are embedded
//! with `include_str!` and shown byte for byte; the text view lays them out
//! (wrapping is display only) but never edits or normalizes them. The
//! summary's own wording must never grant "or later" terms; the embedded GPL
//! text is exempt, since it says "or any later version" itself (`LICENSE`
//! line 572). `tests/info_popups.rs` checks both sides.
//!
//! The dialog host (`ui/widgets/dialog_host.rs`) owns the title row, the close
//! control, Esc, Enter, sizing and focus restoration; this file draws the body
//! and footer inside the 16-unit content inset. Enter dismisses it, like Close.

use egui::{Ui, Vec2};

use crate::presentation::commands::UiIntent;
use crate::ui::dialogs::keyboard_shortcuts::{CONTROL_HEIGHT, footer, styled_galley};
use crate::ui::dialogs::{DialogCx, DialogResponse};
use crate::ui::theme::{self, CompactStyle, Palette, Token};
use crate::ui::widgets::dialog_host;

/// The repository's GPLv3 text, embedded unchanged.
const LICENSE_TEXT: &str = include_str!("../../../../../LICENSE");
/// The repository's third-party notices, embedded unchanged.
const NOTICES_TEXT: &str = include_str!("../../../../../THIRD-PARTY-NOTICES.md");

/// The Source code address: the workspace `repository` field, inherited by
/// this package.
const SOURCE_ADDRESS: &str = env!("CARGO_PKG_REPOSITORY");

// Cargo sets CARGO_PKG_REPOSITORY to "" when the manifest field is missing,
// and the opener rejects anything but https (C-07). Without this check a
// dropped field would build cleanly and turn Source code into a button that
// only reports an error at run time.
const _: () = assert!(
    starts_with(SOURCE_ADDRESS.as_bytes(), b"https://"),
    "the workspace Cargo.toml `repository` must be an https address"
);

const fn starts_with(text: &[u8], prefix: &[u8]) -> bool {
    if text.len() < prefix.len() {
        return false;
    }
    let mut i = 0;
    while i < prefix.len() {
        if text[i] != prefix[i] {
            return false;
        }
        i += 1;
    }
    true
}

pub const PRODUCT_NAME: &str = "Fuzzy DeDupe";
pub const DESCRIPTION: &str = "Finds identical and near-identical text files and helps you move \
     unwanted copies to the Trash safely. Everything runs on this computer; nothing is uploaded.";
pub const COPYRIGHT: &str = "Copyright © 2026 L3Digital LLC";
/// The license statement. Its identifier is the package `license` field, so
/// the displayed license cannot differ from the package metadata.
pub const LICENSE_STATEMENT: &str = concat!(
    "This program is free software: you can redistribute it and/or modify it under the terms \
     of the GNU General Public License, version 3 only (",
    env!("CARGO_PKG_LICENSE"),
    ")."
);
pub const NO_WARRANTY: &str =
    "It comes with ABSOLUTELY NO WARRANTY, to the extent permitted by law.";

/// One of the embedded texts About can show in place of its summary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Notice {
    License,
    ThirdParty,
}

impl Notice {
    /// The summary action that opens this text.
    #[must_use]
    pub const fn action(self) -> &'static str {
        match self {
            Self::License => "View license",
            Self::ThirdParty => "Third-party notices",
        }
    }

    const fn heading(self) -> &'static str {
        match self {
            Self::License => "License",
            Self::ThirdParty => "Third-party notices",
        }
    }

    /// The embedded text, byte-identical to its repository file.
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::License => LICENSE_TEXT,
            Self::ThirdParty => NOTICES_TEXT,
        }
    }
}

/// What the popup body shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum View {
    #[default]
    Summary,
    Text(Notice),
}

/// The popup's draft, created when it opens and dropped when it closes, so
/// closing never commits it (spec §11.6). Reopening About starts on the
/// summary.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Draft {
    pub view: View,
    /// Set for one frame when the body swaps, so keyboard focus lands in the
    /// new body instead of vanishing with the control that was clicked: on
    /// Back after a text opens, and on that text's action after Back.
    refocus: Option<Notice>,
}

/// The draft for a newly opened popup. Pushes nothing.
pub fn open(cx: &mut DialogCx<'_>) -> Draft {
    let _ = cx;
    Draft::default()
}

/// Draws the summary or the open text, and the footer. Returns
/// [`DialogResponse::Close`] when Close was pressed.
pub fn show(ui: &mut Ui, draft: &mut Draft, cx: &mut DialogCx<'_>) -> DialogResponse {
    let palette = theme::palette(dialog_host::mode_of(ui.ctx()));
    ui.spacing_mut().item_spacing = Vec2::ZERO;
    match draft.view {
        View::Summary => summary(ui, draft, cx, palette),
        View::Text(notice) => text_view(ui, draft, notice, palette),
    }
    let section_gap: f32 = theme::SPACING_SCALE[3].into();
    ui.add_space(section_gap);
    footer(ui, None, palette)
}

fn label(ui: &mut Ui, text: &str, style: CompactStyle, color: Token, palette: &Palette) {
    ui.add(
        egui::Label::new(
            egui::RichText::new(text)
                .text_style(style.text_style())
                .line_height(Some(style.line_height()))
                .color(palette.color(color)),
        )
        .wrap(),
    );
}

fn action_button(ui: &mut Ui, text: &str) -> egui::Response {
    ui.add(egui::Button::new(text).min_size(Vec2::new(0.0, CONTROL_HEIGHT)))
}

fn summary(ui: &mut Ui, draft: &mut Draft, cx: &mut DialogCx<'_>, palette: &Palette) {
    // Gaps from frame 44:3858: 2 inside the identity block, 6 between the
    // legal notices, 12 between sections, 8 between actions.
    let [_, small, between, section, ..] = theme::SPACING_SCALE.map(f32::from);
    label(ui, PRODUCT_NAME, CompactStyle::Title, Token::Ink, palette);
    ui.add_space(2.0);
    let version = concat!("Version ", env!("CARGO_PKG_VERSION"));
    label(ui, version, CompactStyle::Supporting, Token::Muted, palette);
    ui.add_space(section);
    label(ui, DESCRIPTION, CompactStyle::Body, Token::Ink, palette);
    ui.add_space(section);
    ui.separator();
    ui.add_space(section);
    label(
        ui,
        COPYRIGHT,
        CompactStyle::GroupEmphasis,
        Token::Ink,
        palette,
    );
    ui.add_space(small);
    label(
        ui,
        LICENSE_STATEMENT,
        CompactStyle::Supporting,
        Token::Muted,
        palette,
    );
    ui.add_space(small);
    label(
        ui,
        NO_WARRANTY,
        CompactStyle::Supporting,
        Token::Muted,
        palette,
    );
    ui.add_space(section);

    let refocus = draft.refocus.take();
    ui.horizontal(|ui| {
        for (index, notice) in [Notice::License, Notice::ThirdParty]
            .into_iter()
            .enumerate()
        {
            if index > 0 {
                ui.add_space(between);
            }
            let button = action_button(ui, notice.action());
            if refocus == Some(notice) {
                button.request_focus();
            }
            if button.clicked() {
                draft.view = View::Text(notice);
                draft.refocus = Some(notice);
            }
        }
        ui.add_space(between);
        if action_button(ui, "Source code").clicked() {
            cx.intents
                .push(UiIntent::OpenWebAddress(SOURCE_ADDRESS.to_owned()));
        }
    });
}

fn text_view(ui: &mut Ui, draft: &mut Draft, notice: Notice, palette: &Palette) {
    let between: f32 = theme::SPACING_SCALE[2].into();
    let mut back_pressed = false;
    ui.horizontal(|ui| {
        let back = action_button(ui, "Back");
        if draft.refocus.take().is_some() {
            back.request_focus();
        }
        back_pressed = back.clicked();
        ui.add_space(between);
        ui.allocate_ui_with_layout(
            Vec2::new(ui.available_width(), CONTROL_HEIGHT),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                label(
                    ui,
                    notice.heading(),
                    CompactStyle::GroupEmphasis,
                    Token::Ink,
                    palette,
                );
            },
        );
    });
    ui.add_space(between);

    // The text scrolls inside its own area sized to what is left above the
    // footer, so Back and Close stay in view however long the text is.
    let section_gap: f32 = theme::SPACING_SCALE[3].into();
    let height = (ui.clip_rect().bottom() - ui.cursor().top() - CONTROL_HEIGHT - section_gap)
        .max(CONTROL_HEIGHT);
    let ink = palette.color(Token::Ink);
    let mut layouter = |ui: &Ui, text: &dyn egui::TextBuffer, wrap_width: f32| {
        styled_galley(ui, text.as_str(), CompactStyle::Document, ink, wrap_width)
    };
    egui::ScrollArea::vertical()
        .id_salt(("fd.about.text", notice))
        .max_height(height)
        .min_scrolled_height(height)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            // A `&str` buffer makes the TextEdit read-only yet selectable and
            // copyable, and marks it read-only for assistive technology; a
            // `String` buffer would accept typing.
            let mut text: &str = notice.text();
            ui.add(
                egui::TextEdit::multiline(&mut text)
                    .desired_width(f32::INFINITY)
                    .background_color(palette.color(Token::Code))
                    .layouter(&mut layouter),
            );
        });

    if back_pressed {
        draft.view = View::Summary;
        draft.refocus = Some(notice);
    }
}
