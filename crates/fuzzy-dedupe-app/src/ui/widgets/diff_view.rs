// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! The paired, aligned, read-only diff view (spec §11.5, IR-006): wrap,
//! linked scroll and model-backed per-pane selection. Stub until plan T14,
//! which fills [`show`] in this file only.

use crate::presentation::state::CompareState;

/// Draws both panes into the remaining Compare area. `input_enabled` is
/// false while a popup or menu is open (spec §11.6).
pub fn show(ui: &mut egui::Ui, compare: &mut CompareState, input_enabled: bool) {
    let _ = (ui, compare, input_enabled);
}
