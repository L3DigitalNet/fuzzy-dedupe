// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! Versioned `preferences.toml` through the platform adapter (C-08, DR-009).
//!
//! The file is [`FILE_NAME`] in [`AppDirs::config_dir`], read with
//! `std::fs::read` and written only through the app-owned writer
//! [`AppFiles::write_atomic`]. This module is the whole mapping between
//! [`PreferencesState`] and the file, so `presentation/state.rs` never
//! changes for it; `app.rs` decides when to load and when to store.
//!
//! Version 1 layout (key names are this module's choice within C-08):
//!
//! ```toml
//! version = 1
//! [window]                  # absent until a size was recorded; no position
//! width = 1440.0
//! height = 870.0
//! maximized = false
//! [view]
//! zoom_percent = 110
//! theme = "system"          # "system", "light" or "dark"
//! rail_collapsed = false
//! [findings.column_widths]  # only columns the user resized
//! file_name = 312.5
//! [compare]
//! split_ratio = 0.5
//! wrap = false
//! linked_scroll = true
//! [scan_setup]
//! file_types = ["txt", "md"]
//! [[scan_setup.folders]]
//! path = "/srv/notes"       # or path_os = { Unix = [..] } when not UTF-8
//! protect = true
//! [matching_rules]          # absent until the user saves a set (OQ-005)
//! mode = "exact_and_fuzzy"  # or "exact_only"
//! minimum_similarity = 850  # per mille
//! revision = 3
//! [matching_rules.ignore]
//! whitespace = true
//! line_endings = true
//! letter_case = false
//! punctuation = false
//! ```
//!
//! SCOPE: the file never holds scan results, the queue or review decisions
//! (DR-009); [`PreferencesState`] has no field for them, and the file
//! structs below mirror only its fields.
//!
//! Error rules (C-08). The file is *unreadable* when it cannot be read for
//! any reason but absence, is not TOML, has a missing or non-integer
//! `version`, or has a key of the wrong type; it is *newer* when `version`
//! is above [`PreferencesState::VERSION`]. Both give the defaults with a
//! warning, and `app.rs` does not overwrite the file until a preference
//! changes, because it compares against what [`load`] returned. A missing
//! file is the first start and is silent. Within a readable version 1
//! file, a missing key takes its default and a well-typed but invalid value
//! (an off-grid zoom, a non-finite or non-positive width, a split outside
//! 0 to 1, a similarity above 1000) falls back for that field alone, with a
//! warning, so one hand-edited value cannot discard the rest. Unknown keys
//! are ignored.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io;
use std::path::PathBuf;

use fuzzy_dedupe_domain::{
    IgnoreOptions, MatchMode, MatchingRules, Permille, RulesRevision, ScanFolder, ScanSetup,
};
use fuzzy_dedupe_platform::app_files::{AppFileError, AppFiles};
use fuzzy_dedupe_platform::dirs::AppDirs;
use serde::{Deserialize, Serialize};

use crate::presentation::state::{ColumnId, PreferencesState, ThemeOverride, WindowPreference};
use crate::ui::theme::Zoom;

/// The file's name inside [`AppDirs::config_dir`].
pub const FILE_NAME: &str = "preferences.toml";

/// Why a file was set aside for the defaults.
#[derive(Debug)]
enum Rejected {
    Unreadable(String),
    Newer(i64),
}

/// The preferences at startup: the stored ones, or the defaults when the
/// file is absent, unreadable or newer (see the module's error rules).
/// Never fails, so a broken file never prevents startup.
#[must_use]
pub fn load(dirs: &AppDirs) -> PreferencesState {
    let path = dirs.config_dir().join(FILE_NAME);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return PreferencesState::default(),
        Err(err) => {
            tracing::warn!(path = %path.display(), %err, "preferences unreadable; using the defaults");
            return PreferencesState::default();
        }
    };
    match parse(&bytes) {
        Ok(preferences) => preferences,
        Err(Rejected::Unreadable(reason)) => {
            tracing::warn!(path = %path.display(), %reason, "preferences unreadable; using the defaults");
            PreferencesState::default()
        }
        Err(Rejected::Newer(version)) => {
            tracing::warn!(
                path = %path.display(),
                version,
                supported = PreferencesState::VERSION,
                "preferences from a newer version; using the defaults"
            );
            PreferencesState::default()
        }
    }
}

/// Writes `preferences` as a version 1 file, replacing the old one
/// atomically, so a crash leaves either the old file or the new one.
///
/// # Errors
///
/// The writer's error; `app.rs` logs it and retries on the next change.
pub fn store(
    files: &AppFiles,
    dirs: &AppDirs,
    preferences: &PreferencesState,
) -> Result<(), AppFileError> {
    let path = dirs.config_dir().join(FILE_NAME);
    let text = toml::to_string(&FileV1::from_state(preferences)).map_err(|err| {
        // Unreachable for these types (string keys, no unsupported values);
        // reported as a write failure rather than a panic all the same.
        AppFileError::Io {
            path: path.clone(),
            source: io::Error::new(io::ErrorKind::InvalidData, err),
        }
    })?;
    files.write_atomic(&path, text.as_bytes())
}

fn parse(bytes: &[u8]) -> Result<PreferencesState, Rejected> {
    let text = std::str::from_utf8(bytes).map_err(|err| Rejected::Unreadable(err.to_string()))?;
    let table: toml::Table =
        toml::from_str(text).map_err(|err| Rejected::Unreadable(err.to_string()))?;
    // The version is read on its own before the body: a newer file may use
    // a layout this build cannot deserialize, and must be reported as newer,
    // not as malformed.
    let version = match table.get("version") {
        Some(toml::Value::Integer(version)) => *version,
        Some(_) => return Err(Rejected::Unreadable("version is not an integer".into())),
        None => return Err(Rejected::Unreadable("no version".into())),
    };
    if version > i64::from(PreferencesState::VERSION) {
        return Err(Rejected::Newer(version));
    }
    if version != i64::from(PreferencesState::VERSION) {
        return Err(Rejected::Unreadable(format!("unknown version {version}")));
    }
    let file: FileV1 = table
        .try_into()
        .map_err(|err: toml::de::Error| Rejected::Unreadable(err.to_string()))?;
    Ok(file.into_state())
}

// ── Version 1 file structs ──────────────────────────────────────────────
//
// Mirrors of the state types rather than serde derives on them: the domain
// types are frozen (C-01, T4) and serde-free, and a separate layer keeps
// the file's key names stable when a state type is renamed.

#[derive(Serialize, Deserialize)]
struct FileV1 {
    version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    window: Option<WindowV1>,
    #[serde(default)]
    view: ViewV1,
    #[serde(default)]
    findings: FindingsV1,
    #[serde(default)]
    compare: CompareV1,
    #[serde(default)]
    scan_setup: ScanSetupV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    matching_rules: Option<RulesV1>,
}

#[derive(Serialize, Deserialize)]
struct WindowV1 {
    width: f32,
    height: f32,
    #[serde(default)]
    maximized: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(default)]
struct ViewV1 {
    zoom_percent: u16,
    theme: ThemeV1,
    rail_collapsed: bool,
}

impl Default for ViewV1 {
    fn default() -> Self {
        let defaults = PreferencesState::default();
        Self {
            zoom_percent: defaults.zoom.percent(),
            theme: defaults.theme.into(),
            rail_collapsed: defaults.rail_collapsed,
        }
    }
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ThemeV1 {
    System,
    Light,
    Dark,
}

impl From<ThemeOverride> for ThemeV1 {
    fn from(theme: ThemeOverride) -> Self {
        match theme {
            ThemeOverride::System => Self::System,
            ThemeOverride::Light => Self::Light,
            ThemeOverride::Dark => Self::Dark,
        }
    }
}

impl From<ThemeV1> for ThemeOverride {
    fn from(theme: ThemeV1) -> Self {
        match theme {
            ThemeV1::System => Self::System,
            ThemeV1::Light => Self::Light,
            ThemeV1::Dark => Self::Dark,
        }
    }
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct FindingsV1 {
    /// Keyed by [`column_key`], not deserialized into [`ColumnId`]
    /// directly, so a column a later build drops or renames is skipped
    /// with a warning instead of making the whole file unreadable.
    column_widths: BTreeMap<String, f32>,
}

#[derive(Serialize, Deserialize)]
#[serde(default)]
struct CompareV1 {
    split_ratio: f32,
    wrap: bool,
    linked_scroll: bool,
}

impl Default for CompareV1 {
    fn default() -> Self {
        let defaults = PreferencesState::default();
        Self {
            split_ratio: defaults.compare_split_ratio,
            wrap: defaults.compare_wrap,
            linked_scroll: defaults.compare_linked_scroll,
        }
    }
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct ScanSetupV1 {
    folders: Vec<FolderV1>,
    file_types: Vec<String>,
}

/// One folder. A UTF-8 path is written as `path`; any other path as
/// `path_os`, because the spec requires paths to be stored losslessly (§9)
/// and a TOML string must be UTF-8.
///
/// `path_os` is serde's own `OsString` encoding, an externally tagged
/// `Unix` byte array or `Windows` UTF-16 array. Rejected: converting the
/// bytes here with `std::os::unix::ffi`, which the boundaries check (R5)
/// forbids outside `fuzzy-dedupe-platform`; serde's implementation keeps
/// the OS-specific code out of this crate's source. A path written on
/// another OS fails to deserialize, which makes the file unreadable.
#[derive(Serialize, Deserialize)]
struct FolderV1 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    path_os: Option<OsString>,
    #[serde(default)]
    protect: bool,
}

#[derive(Serialize, Deserialize)]
struct RulesV1 {
    mode: ModeV1,
    minimum_similarity: u16,
    revision: u64,
    ignore: IgnoreV1,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ModeV1 {
    ExactOnly,
    ExactAndFuzzy,
}

#[derive(Serialize, Deserialize)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "mirrors the four spec §11.6 Ignore checkboxes of IgnoreOptions one key each"
)]
struct IgnoreV1 {
    whitespace: bool,
    line_endings: bool,
    letter_case: bool,
    punctuation: bool,
}

/// The stable file key of a Findings column.
const fn column_key(column: ColumnId) -> &'static str {
    match column {
        ColumnId::Queue => "queue",
        ColumnId::Group => "group",
        ColumnId::Match => "match",
        ColumnId::Unique => "unique",
        ColumnId::FileName => "file_name",
        ColumnId::Folder => "folder",
        ColumnId::Modified => "modified",
        ColumnId::Size => "size",
        ColumnId::Protected => "protected",
        ColumnId::Decision => "decision",
    }
}

fn column_from_key(key: &str) -> Option<ColumnId> {
    ColumnId::ALL.into_iter().find(|c| column_key(*c) == key)
}

/// A width or window extent the UI can use: finite and above zero.
fn usable_extent(value: f32) -> bool {
    value.is_finite() && value > 0.0
}

impl FileV1 {
    fn from_state(state: &PreferencesState) -> Self {
        Self {
            version: PreferencesState::VERSION,
            window: state.window.map(|w| WindowV1 {
                width: w.width,
                height: w.height,
                maximized: w.maximized,
            }),
            view: ViewV1 {
                zoom_percent: state.zoom.percent(),
                theme: state.theme.into(),
                rail_collapsed: state.rail_collapsed,
            },
            findings: FindingsV1 {
                column_widths: state
                    .findings_column_widths
                    .iter()
                    .map(|(c, w)| (column_key(*c).to_owned(), *w))
                    .collect(),
            },
            compare: CompareV1 {
                split_ratio: state.compare_split_ratio,
                wrap: state.compare_wrap,
                linked_scroll: state.compare_linked_scroll,
            },
            scan_setup: ScanSetupV1 {
                folders: state
                    .scan_setup
                    .folders
                    .iter()
                    .map(FolderV1::from_folder)
                    .collect(),
                file_types: state.scan_setup.file_types.clone(),
            },
            matching_rules: state.saved_rules.map(|r| RulesV1 {
                mode: match r.mode {
                    MatchMode::ExactOnly => ModeV1::ExactOnly,
                    MatchMode::ExactAndFuzzy => ModeV1::ExactAndFuzzy,
                },
                minimum_similarity: r.minimum_similarity.get(),
                revision: r.revision.get(),
                ignore: IgnoreV1 {
                    whitespace: r.ignore.whitespace,
                    line_endings: r.ignore.line_endings,
                    letter_case: r.ignore.letter_case,
                    punctuation: r.ignore.punctuation,
                },
            }),
        }
    }

    /// The state this file describes, each invalid value replaced by its
    /// default with a warning (the per-field rule in the module docs).
    fn into_state(self) -> PreferencesState {
        let defaults = PreferencesState::default();

        let window = self.window.and_then(|w| {
            if usable_extent(w.width) && usable_extent(w.height) {
                Some(WindowPreference {
                    width: w.width,
                    height: w.height,
                    maximized: w.maximized,
                })
            } else {
                tracing::warn!(w.width, w.height, "stored window size ignored");
                None
            }
        });

        let zoom = Zoom::from_percent(self.view.zoom_percent).unwrap_or_else(|| {
            tracing::warn!(
                percent = self.view.zoom_percent,
                "stored zoom off the grid; ignored"
            );
            defaults.zoom
        });

        let mut findings_column_widths = BTreeMap::new();
        for (key, width) in self.findings.column_widths {
            match column_from_key(&key) {
                Some(column) if usable_extent(width) => {
                    findings_column_widths.insert(column, width);
                }
                Some(_) => tracing::warn!(column = %key, width, "stored column width ignored"),
                None => tracing::warn!(column = %key, "unknown column in preferences; ignored"),
            }
        }

        let split = self.compare.split_ratio;
        let compare_split_ratio = if split.is_finite() && (0.0..=1.0).contains(&split) {
            split
        } else {
            tracing::warn!(split, "stored compare split ignored");
            defaults.compare_split_ratio
        };

        let scan_setup = ScanSetup {
            folders: self
                .scan_setup
                .folders
                .into_iter()
                .filter_map(FolderV1::into_folder)
                .collect(),
            file_types: self.scan_setup.file_types,
        };

        let saved_rules = self.matching_rules.and_then(|r| {
            let Some(minimum_similarity) = Permille::new(r.minimum_similarity) else {
                tracing::warn!(r.minimum_similarity, "stored matching rules ignored");
                return None;
            };
            Some(MatchingRules {
                mode: match r.mode {
                    ModeV1::ExactOnly => MatchMode::ExactOnly,
                    ModeV1::ExactAndFuzzy => MatchMode::ExactAndFuzzy,
                },
                minimum_similarity,
                ignore: IgnoreOptions {
                    whitespace: r.ignore.whitespace,
                    line_endings: r.ignore.line_endings,
                    letter_case: r.ignore.letter_case,
                    punctuation: r.ignore.punctuation,
                },
                revision: RulesRevision::new(r.revision),
            })
        });

        PreferencesState {
            version: PreferencesState::VERSION,
            window,
            zoom,
            theme: self.view.theme.into(),
            rail_collapsed: self.view.rail_collapsed,
            findings_column_widths,
            compare_split_ratio,
            compare_wrap: self.compare.wrap,
            compare_linked_scroll: self.compare.linked_scroll,
            scan_setup,
            saved_rules,
        }
    }
}

impl FolderV1 {
    fn from_folder(folder: &ScanFolder) -> Self {
        let (path, path_os) = match folder.path.to_str() {
            Some(text) => (Some(text.to_owned()), None),
            None => (None, Some(folder.path.clone().into_os_string())),
        };
        Self {
            path,
            path_os,
            protect: folder.protect,
        }
    }

    /// The folder, or `None` with a warning when it names no usable path;
    /// the rest of the setup is kept.
    fn into_folder(self) -> Option<ScanFolder> {
        let path = match (self.path, self.path_os) {
            (Some(text), _) => PathBuf::from(text),
            (None, Some(os)) => PathBuf::from(os),
            (None, None) => {
                tracing::warn!("stored scan folder without a path; ignored");
                return None;
            }
        };
        Some(ScanFolder {
            path,
            protect: self.protect,
        })
    }
}
