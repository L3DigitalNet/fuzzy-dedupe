// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! The Findings workspace (spec §11.2, §11.4): search, Actions ▾, Sort ▾,
//! Compare, relationship filters and the grouped table. Stub until plan
//! T13, which fills [`toolbar`] and [`show`] in this file only.
//!
//! The shell (`ui/shell.rs`) draws the toolbar row and its shared trailing
//! items (Review cleanup (N), Settings); [`toolbar`] fills the leading part
//! of that row, left to right.

use crate::presentation::commands::IntentQueue;
use crate::ui::WorkspaceCx;

/// The workspace's own toolbar items, before the shared ones.
pub fn toolbar(ui: &mut egui::Ui, cx: &mut WorkspaceCx<'_>, intents: &mut IntentQueue) {
    let _ = (ui, cx, intents);
}

/// The workspace body below the toolbar: filter row and table.
pub fn show(ui: &mut egui::Ui, cx: &mut WorkspaceCx<'_>, intents: &mut IntentQueue) {
    let _ = (ui, cx, intents);
}
