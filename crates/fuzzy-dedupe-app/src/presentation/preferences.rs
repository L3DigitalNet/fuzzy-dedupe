// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! Versioned `preferences.toml` through the platform adapter (C-08, DR-009).
//!
//! Stub until plan T20, which fills [`load`] and [`store`]: the file lives
//! in [`AppDirs::config_dir`], is written only through the app-owned writer
//! [`AppFiles::write_atomic`], and maps to and from
//! [`PreferencesState`] here, so `presentation/state.rs` never changes for
//! it. Until then every start uses the defaults and nothing is written.

use fuzzy_dedupe_platform::app_files::{AppFileError, AppFiles};
use fuzzy_dedupe_platform::dirs::AppDirs;

use crate::presentation::state::PreferencesState;

/// The preferences at startup. An unreadable or newer file yields the
/// defaults with a warning (C-08). Stub: always the defaults.
#[must_use]
pub fn load(dirs: &AppDirs) -> PreferencesState {
    let _ = dirs;
    PreferencesState::default()
}

/// Writes `preferences` after one changed. Stub: writes nothing.
///
/// # Errors
///
/// The writer's error once T20 implements the write.
pub fn store(
    files: &AppFiles,
    dirs: &AppDirs,
    preferences: &PreferencesState,
) -> Result<(), AppFileError> {
    let _ = (files, dirs, preferences);
    Ok(())
}
