// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! Scan setup (spec §6): the folders, their protection and the file types
//! for the next scan. V1 keeps one current setup (WH-006).

use std::path::PathBuf;

/// One folder to scan. Folder protection belongs to the setup and is never a
/// review decision: no binding change and no new scan clears it (C-05 I-9).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ScanFolder {
    /// `PathBuf`, not a string, so a non-UTF-8 path survives losslessly
    /// (spec §8.5 forbids byte-string-only or string-only path assumptions).
    pub path: PathBuf,
    /// Files under a protected folder can never be queued.
    pub protect: bool,
}

/// The folders and file types of a scan. The folder order matters: the
/// reference defaults to a protected file, else to one from the first-listed
/// folder, else to the first by path (spec §6, owner reading B).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ScanSetup {
    pub folders: Vec<ScanFolder>,
    /// File-name extensions without the dot, such as `txt` and `md`.
    pub file_types: Vec<String>,
}
