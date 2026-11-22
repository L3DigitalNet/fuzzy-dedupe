// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! Scan progress, the counts the scanning state (frame 01b) and the status
//! bar show (spec §11.6).

use std::time::Duration;

/// One coalesced progress report. Counts never decrease within one scan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScanProgress {
    /// Display form of the folder being read.
    pub current_folder: String,
    pub files_read: u64,
    pub groups_found: u64,
    pub elapsed: Duration,
}
