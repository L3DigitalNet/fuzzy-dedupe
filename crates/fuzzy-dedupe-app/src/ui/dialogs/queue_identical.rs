// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! The Queue identical copies popup (spec §11.6, frame `27:1868`). Stub until
//! plan T17, which fills the draft, [`show`] and [`submit`] in this file only.
//!
//! The dialog host (`ui/widgets/dialog_host.rs`) owns the title row, the close
//! control, Esc, Enter, sizing and focus restoration; this file draws the body
//! and footer inside the 16-unit content inset. Enter runs Queue N files.

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

/// The popup's non-destructive default action, run by Enter and by its
/// primary button. Stub: closes without committing anything.
pub fn submit(draft: &mut Draft, cx: &mut DialogCx<'_>) -> DialogResponse {
    let _ = (draft, cx);
    DialogResponse::Close
}
