// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! Domain results into the text, rows and metrics views draw (spec §11.8).
//!
//! Projections are pure functions of presentation state: no engine call and
//! no I/O, so a view can run them while drawing. T12 provides the shell's
//! projections (status line, labels); T13 adds the Findings row projections
//! and T15 the Compare ones, each in its own section of this file.

use std::fmt::Write as _;

use fuzzy_dedupe_domain::Refusal;

use crate::presentation::state::{PresentationState, ScanStage};

/// The status bar's two ends.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusLine {
    /// Scan counts and completion, or the scan's progress.
    pub left: String,
    /// The queue count.
    pub right: String,
}

/// `12648` as `12,648`.
#[must_use]
pub fn thousands(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// The compact status line (frames `13:648`, `28:1399`).
#[must_use]
pub fn status_line(state: &PresentationState) -> StatusLine {
    const SEP: &str = "   ·   ";
    let left = match (&state.scan.results, state.scan.stage) {
        (_, ScanStage::Running | ScanStage::Stopping) => {
            let progress = state.scan.progress.as_ref();
            let files = progress.map_or(0, |p| p.files_read);
            let groups = progress.map_or(0, |p| p.groups_found);
            format!(
                "Scanning{SEP}{} files read{SEP}{} groups found",
                thousands(files),
                thousands(groups)
            )
        }
        (None, _) => "No scan loaded".to_owned(),
        (Some(results), done) => {
            let groups = results.groups.len() as u64;
            let matched: u64 = results.groups.iter().map(|g| g.members.len() as u64).sum();
            let mut summary = format!(
                "{} text files scanned{SEP}{} groups{SEP}{} matched files",
                thousands(state.scan.files_scanned),
                thousands(groups),
                thousands(matched)
            );
            if done == ScanStage::Partial {
                summary.push_str(SEP);
                summary.push_str("partial: stopped before every file was read");
            } else if let Some(time) = state.scan.finished_at {
                let _ = write!(summary, "{SEP}finished {:02}:{:02}", time.hour, time.minute);
            }
            summary
        }
    };
    let mut right = format!("{} queued", thousands(state.queue.queued_count() as u64));
    if let Some(note) = findings_queue_note(state) {
        right.push_str(", ");
        right.push_str(&note);
    }
    StatusLine { left, right }
}

// ── Findings projections (T13) ───────────────────────────────────────────

/// The status bar's note on queued files the current search or filter
/// hides, such as "3 outside the search" (spec §11.4). T13 fills it; until
/// then there is no note.
#[must_use]
pub fn findings_queue_note(state: &PresentationState) -> Option<String> {
    let _ = state;
    None
}

// ── Compare projections (T15) ────────────────────────────────────────────

/// The toolbar's "Review cleanup (N)" label.
#[must_use]
pub fn review_cleanup_label(state: &PresentationState) -> String {
    format!("Review cleanup ({})", state.queue.queued_count())
}

/// The status-bar text for a refused review command (C-05 refusal labels).
#[must_use]
pub const fn refusal_text(refusal: Refusal) -> &'static str {
    match refusal {
        Refusal::Reference => "The reference is never queued.",
        Refusal::Protected => "Protected files are never queued.",
        Refusal::CounterpartQueued => "Counterpart queued",
        Refusal::CounterpartOfQueuedFile => "Counterpart of a queued file",
        Refusal::NotByteIdentical => "Only byte-identical copies are queued here.",
        Refusal::UnknownFile => "That file is not in the current results.",
        Refusal::EvidenceUnavailable | Refusal::EvidenceMismatch => {
            "The comparison changed; compare again before queueing."
        }
    }
}
