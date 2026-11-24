// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! The application-owned popups (spec §11.6) and the one shape they share.
//!
//! Each popup's file owns its draft type and exposes `open` (the draft for a
//! new popup), `show` (the body), `footer` (the action footer, which the
//! dialog host pins below the scrolling body) and, for popups whose Enter
//! runs a primary action, `submit`. The dispatch below is the only place that
//! names every popup, so a dialog task edits its own file and never this
//! one (plan D-004).
//!
//! | Popup                  | File                    | Filled by |
//! | ---------------------- | ----------------------- | --------- |
//! | Matching rules         | `matching_rules.rs`     | T16       |
//! | New scan               | `new_scan.rs`           | T16       |
//! | Review unique text     | `unique_text_review.rs` | T17       |
//! | Queue identical copies | `queue_identical.rs`    | T17       |
//! | Review cleanup         | `cleanup.rs`            | T18       |
//! | Keyboard shortcuts     | `keyboard_shortcuts.rs` | T19       |
//! | About                  | `about.rs`              | T19       |

pub mod about;
pub mod cleanup;
pub mod keyboard_shortcuts;
pub mod matching_rules;
pub mod new_scan;
pub mod queue_identical;
pub mod unique_text_review;

use std::path::PathBuf;

use crate::presentation::commands::IntentQueue;
use crate::presentation::state::{
    CompareState, DefaultAction, DialogKind, DialogSubject, FindingsState, OperationState,
    PreferencesState, QueueState, ScanState,
};

/// What a popup reads while open, and where it sends its requests. Popups
/// never call services directly: a committed action is a pushed intent.
pub struct DialogCx<'a> {
    /// What this popup is about (see `DialogSubject::fits`).
    pub subject: &'a DialogSubject,
    pub findings: &'a FindingsState,
    pub compare: &'a CompareState,
    pub queue: &'a QueueState,
    pub scan: &'a ScanState,
    pub operation: &'a OperationState,
    pub preferences: &'a PreferencesState,
    /// Folders the picker returned since the last frame (`UiIntent::
    /// PickFolders`); the popup takes them.
    pub picked_folders: &'a mut Option<Vec<PathBuf>>,
    pub intents: &'a mut IntentQueue,
}

/// Whether a popup stays open after this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DialogResponse {
    KeepOpen,
    /// Close and return focus to the invoker. Only intents the popup pushed
    /// are committed; the draft is dropped.
    Close,
}

/// The open popup's draft, one variant per popup, each the type its own
/// file defines.
#[derive(Clone, Debug, PartialEq)]
pub enum DialogDraft {
    MatchingRules(matching_rules::Draft),
    NewScan(new_scan::Draft),
    UniqueTextReview(unique_text_review::Draft),
    QueueIdentical(queue_identical::Draft),
    Cleanup(cleanup::Draft),
    KeyboardShortcuts(keyboard_shortcuts::Draft),
    About(about::Draft),
}

impl DialogDraft {
    /// A fresh draft for `kind`. A popup's `open` may push intents, such as
    /// Review unique text's `LoadComparison` for its pair or Review
    /// cleanup's `BuildCleanupPlan`.
    pub fn open(kind: DialogKind, cx: &mut DialogCx<'_>) -> Self {
        match kind {
            DialogKind::MatchingRules => Self::MatchingRules(matching_rules::open(cx)),
            DialogKind::NewScan => Self::NewScan(new_scan::open(cx)),
            DialogKind::UniqueTextReview => Self::UniqueTextReview(unique_text_review::open(cx)),
            DialogKind::QueueIdentical => Self::QueueIdentical(queue_identical::open(cx)),
            DialogKind::Cleanup => Self::Cleanup(cleanup::open(cx)),
            DialogKind::KeyboardShortcuts => Self::KeyboardShortcuts(keyboard_shortcuts::open(cx)),
            DialogKind::About => Self::About(about::open(cx)),
        }
    }

    /// The title the dialog host draws for this popup this frame: the
    /// popup's fixed [`DialogKind::title`], except Review cleanup, whose
    /// title follows its phase (spec §11.6: "Moving files to Trash" while
    /// running).
    #[must_use]
    pub fn title(&self, kind: DialogKind, cx: &DialogCx<'_>) -> &'static str {
        match self {
            Self::Cleanup(_) => cleanup::title(cx),
            _ => kind.title(),
        }
    }

    /// The popup's initial size this frame, in units, beside [`Self::title`]:
    /// the popup's [`DialogKind::reference_size`], except New scan, which
    /// is 640 × 778 while its matching rules are expanded (spec §11.6). The
    /// dialog host shrinks it to the viewport, so it is never a minimum.
    #[must_use]
    pub fn size(&self, kind: DialogKind) -> [f32; 2] {
        match self {
            Self::NewScan(draft) => new_scan::size(draft),
            _ => kind.reference_size(),
        }
    }

    /// Draws the popup's body, which the dialog host scrolls above the
    /// footer.
    pub fn show(&mut self, ui: &mut egui::Ui, cx: &mut DialogCx<'_>) -> DialogResponse {
        match self {
            Self::MatchingRules(draft) => matching_rules::show(ui, draft, cx),
            Self::NewScan(draft) => new_scan::show(ui, draft, cx),
            Self::UniqueTextReview(draft) => unique_text_review::show(ui, draft, cx),
            Self::QueueIdentical(draft) => queue_identical::show(ui, draft, cx),
            Self::Cleanup(draft) => cleanup::show(ui, draft, cx),
            Self::KeyboardShortcuts(draft) => keyboard_shortcuts::show(ui, draft, cx),
            Self::About(draft) => about::show(ui, draft, cx),
        }
    }

    /// Draws the popup's action footer into the slot the dialog host pins
    /// below the body (spec §11.6: the footer stays reachable however far
    /// the body scrolls). Every popup has one.
    ///
    /// ORDER: the host calls this after [`Self::show`] in the same frame,
    /// so Tab reaches the body's controls before the footer's, and a footer
    /// may rely on what `show` recorded in the draft this frame (Review
    /// cleanup's run reports, Review unique text's checkbox).
    pub fn footer(&mut self, ui: &mut egui::Ui, cx: &mut DialogCx<'_>) -> DialogResponse {
        match self {
            Self::MatchingRules(draft) => matching_rules::footer(ui, draft, cx),
            Self::NewScan(draft) => new_scan::footer(ui, draft, cx),
            Self::UniqueTextReview(draft) => unique_text_review::footer(ui, draft, cx),
            Self::QueueIdentical(draft) => queue_identical::footer(ui, draft, cx),
            Self::Cleanup(draft) => cleanup::footer(ui, draft, cx),
            Self::KeyboardShortcuts(draft) => keyboard_shortcuts::footer(ui, draft, cx),
            Self::About(draft) => about::footer(ui, draft, cx),
        }
    }

    /// Runs Enter's action for `kind` (see [`DialogKind::default_action`]).
    /// Popups whose default is [`DefaultAction::Dismiss`] close without
    /// calling anything, so Enter can never reach Review cleanup's
    /// Move-to-Trash action.
    pub fn default_action(&mut self, kind: DialogKind, cx: &mut DialogCx<'_>) -> DialogResponse {
        if kind.default_action() == DefaultAction::Dismiss {
            return DialogResponse::Close;
        }
        match self {
            Self::MatchingRules(draft) => matching_rules::submit(draft, cx),
            Self::NewScan(draft) => new_scan::submit(draft, cx),
            Self::UniqueTextReview(draft) => unique_text_review::submit(draft, cx),
            Self::QueueIdentical(draft) => queue_identical::submit(draft, cx),
            Self::Cleanup(_) | Self::KeyboardShortcuts(_) | Self::About(_) => DialogResponse::Close,
        }
    }
}
