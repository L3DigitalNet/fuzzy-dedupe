// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! The Review cleanup popup (spec §11.6, frame `16:886`). Stub until plan T18,
//! which fills the draft and [`show`] in this file only.
//!
//! The dialog host (`ui/widgets/dialog_host.rs`) owns the title row, the close
//! control, Esc, Enter, sizing and focus restoration; this file draws the body
//! and footer inside the 16-unit content inset. Enter only dismisses this
//! popup: its primary action moves files to Trash, which no key may trigger
//! (spec §11.10). The running, stopped and finished states share this dialog.

use crate::ui::dialogs::{DialogCx, DialogResponse};

/// The popup's draft, created when it opens and dropped when it closes, so
/// closing never commits it (spec §11.6).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Draft {}

/// The draft for a newly opened popup. May push the intents the popup
/// needs at once (a comparison, a plan) into `cx.intents`.
pub fn open(cx: &mut DialogCx<'_>) -> Draft {
    let _ = cx;
    Draft::default()
}

/// Draws the body and footer. Returns [`DialogResponse::Close`] when a
/// footer action closed the popup.
pub fn show(ui: &mut egui::Ui, draft: &mut Draft, cx: &mut DialogCx<'_>) -> DialogResponse {
    let _ = (ui, draft, cx);
    DialogResponse::KeepOpen
}
