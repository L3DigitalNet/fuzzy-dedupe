// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! The egui user interface. GUI types stay inside this crate; the engine and
//! domain never see them (spec §8.5).
//!
//! `shell` draws the rail, main menu, toolbar row and status bar and hosts
//! the two workspaces (`findings`, `compare`); `dialogs` holds the popups and
//! `widgets` the reusable pieces. Views read presentation state and push
//! intents; none calls a service while drawing (spec §5.2).

pub mod compare;
pub mod dialogs;
pub mod findings;
pub mod glyphs;
pub mod shell;
pub mod theme;
pub mod widgets;

use crate::presentation::state::{
    CompareState, FindingsState, MenuKind, OperationState, PreferencesState, QueueState, ScanState,
};

/// What a workspace view may read and change while drawing. Dialog state is
/// absent on purpose: a view requests a popup or menu with an intent, and
/// reads back only which menu is open.
pub struct WorkspaceCx<'a> {
    pub findings: &'a mut FindingsState,
    pub compare: &'a mut CompareState,
    pub queue: &'a QueueState,
    pub scan: &'a ScanState,
    pub operation: &'a OperationState,
    /// Mutable so a view records its own view preferences (column widths,
    /// split, wrap, linked scroll); `app.rs` writes changes through T20's
    /// `preferences::store`.
    pub preferences: &'a mut PreferencesState,
    /// False while a popup or menu is open: the view then reacts to no key,
    /// so no shortcut reaches it under a popup (spec §11.6).
    pub input_enabled: bool,
    /// The menu the dialog host's single overlay slot holds this frame
    /// (C-12), read from `DialogState` by `shell::show`. It is the only
    /// source of which menu is open: a view draws the menu named here and
    /// keeps no copy of it, so a close or swap the host makes (Esc, a
    /// dispatch-side close) is seen on the next frame.
    pub menu: Option<MenuKind>,
}
