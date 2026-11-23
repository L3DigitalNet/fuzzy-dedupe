// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! Per-user directories (C-07; spec §8.3 row `etcetera`, DR-009, NFR-009).
//!
//! The application keeps its configuration in `$XDG_CONFIG_HOME/fuzzy-dedupe`
//! and its state, including `logs/`, in `$XDG_STATE_HOME/fuzzy-dedupe`, with
//! the XDG Base Directory fallbacks `$HOME/.config` and `$HOME/.local/state`
//! when a variable is unset, empty or relative.
//!
//! The resolution is written here rather than delegated to `etcetera`'s
//! `Xdg` strategy because that strategy reads the process environment
//! itself, so a test could only steer it with `std::env::set_var`, which is
//! `unsafe` in edition 2024, races every other test thread, and is barred by
//! the workspace `unsafe_code` lint. [`AppDirs::resolve`] instead takes the
//! environment as a lookup function; production passes `std::env::var_os`
//! and `etcetera`'s home directory ([`AppDirs::from_system`]).

use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

/// The directory name under each XDG base directory.
pub const APP_DIR_NAME: &str = "fuzzy-dedupe";
/// The log directory name under the state directory.
pub const LOG_DIR_NAME: &str = "logs";

/// The application's resolved per-user directories.
///
/// Every path is absolute whenever the home directory given to
/// [`AppDirs::resolve`] is. The directories are not created here; the
/// app-owned writer creates them on first write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppDirs {
    config: PathBuf,
    state: PathBuf,
    logs: PathBuf,
}

/// The per-user directories could not be determined.
#[derive(Debug)]
pub enum DirsError {
    /// The platform reported no home directory.
    NoHome,
    /// The platform reported a home directory that is not absolute, so every
    /// fallback path would resolve against the working directory.
    HomeNotAbsolute(PathBuf),
}

impl std::fmt::Display for DirsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoHome => f.write_str("the home directory could not be determined"),
            Self::HomeNotAbsolute(p) => {
                write!(f, "the home directory {} is not absolute", p.display())
            }
        }
    }
}

impl std::error::Error for DirsError {}

impl AppDirs {
    /// Resolve the directories from an environment lookup and a home
    /// directory.
    ///
    /// `env` maps a variable name to its value, as `std::env::var_os` does.
    /// Only `XDG_CONFIG_HOME` and `XDG_STATE_HOME` are read. A value that is
    /// not an absolute path, including an empty one, is ignored as the XDG
    /// Base Directory specification requires, and the `home` fallback is
    /// used instead. An absolute value containing a `..` component is
    /// ignored the same way.
    pub fn resolve(env: impl Fn(&str) -> Option<OsString>, home: &Path) -> Self {
        let base = |var: &str, fallback: &[&str]| {
            env(var)
                .map(PathBuf::from)
                // `..` is refused as well as relative values: the app-owned
                // writer rejects any path with a `..` component (it compares
                // paths lexically), so accepting one here would resolve a
                // root that every write under it then fails against.
                .filter(|p| p.is_absolute() && !p.components().any(|c| c == Component::ParentDir))
                .unwrap_or_else(|| fallback.iter().fold(home.to_path_buf(), |p, c| p.join(c)))
        };
        let config = base("XDG_CONFIG_HOME", &[".config"]).join(APP_DIR_NAME);
        let state = base("XDG_STATE_HOME", &[".local", "state"]).join(APP_DIR_NAME);
        let logs = state.join(LOG_DIR_NAME);
        Self {
            config,
            state,
            logs,
        }
    }

    /// Resolve the directories for the current user from the process
    /// environment and `etcetera`'s home directory.
    ///
    /// # Errors
    ///
    /// [`DirsError`] when no absolute home directory is available.
    pub fn from_system() -> Result<Self, DirsError> {
        let home = etcetera::home_dir().map_err(|_| DirsError::NoHome)?;
        if !home.is_absolute() {
            return Err(DirsError::HomeNotAbsolute(home));
        }
        // A closure, not the bare `std::env::var_os` path: that generic
        // function instantiates for one lifetime of `&str`, not for every
        // lifetime the `Fn(&str)` bound requires.
        Ok(Self::resolve(|name| std::env::var_os(name), &home))
    }

    /// `$XDG_CONFIG_HOME/fuzzy-dedupe`, where `preferences.toml` lives.
    #[must_use]
    pub fn config_dir(&self) -> &Path {
        &self.config
    }

    /// `$XDG_STATE_HOME/fuzzy-dedupe`.
    #[must_use]
    pub fn state_dir(&self) -> &Path {
        &self.state
    }

    /// `$XDG_STATE_HOME/fuzzy-dedupe/logs`, the only directory
    /// [`crate::log_retention::expire_logs`] deletes in.
    #[must_use]
    pub fn log_dir(&self) -> &Path {
        &self.logs
    }
}
