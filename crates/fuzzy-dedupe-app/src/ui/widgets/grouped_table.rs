// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! The grouped, virtualized Findings table (spec §11.4, §11.9): stable row
//! identity over an `egui_table` body. Stub until plan T13, which fills
//! [`show`] in this file only.

use crate::presentation::commands::IntentQueue;
use crate::presentation::state::{FindingsState, QueueState, ScanState};

/// Draws the table into the remaining Findings height. `input_enabled` is
/// false while a popup or menu is open: the table then reacts to no key, so
/// no shortcut reaches it under a popup (spec §11.6).
pub fn show(
    ui: &mut egui::Ui,
    findings: &mut FindingsState,
    scan: &ScanState,
    queue: &QueueState,
    intents: &mut IntentQueue,
    input_enabled: bool,
) {
    let _ = (ui, findings, scan, queue, intents, input_enabled);
}
