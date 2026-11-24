// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! The spec §11.8 presentation state owners (plan C-12).
//!
//! Every owner the §11.8 table names is defined here with its full contents,
//! and [`PreferencesState`] carries every C-08 field, so later tasks fill
//! behavior without reshaping state. Edit ownership after T12: T13 edits the
//! Findings and Queue owners only, T15 the Compare owner only; nothing else
//! edits this file. Per-dialog draft types live in each dialog's own file
//! (`ui/dialogs/*.rs`); [`DialogState`] only holds them.
//!
//! Every owner is plain data. No widget calls the engine, the platform or the
//! filesystem while drawing (spec §5.2): views read these owners and push
//! [`crate::presentation::commands::UiIntent`]s, which `app.rs` dispatches
//! after the UI pass.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;

use fuzzy_dedupe_domain::{
    CleanupPlan, CompareMode, DecisionBinding, FileId, Group, GroupId, MatchingRules, Member,
    MemberDecision, MemberRole, PlanItem, PlanRevision, QueueOrigin, QueueScope, Refusal,
    RelationKind, RequestId, RulesRevision, ScanId, ScanIssue, ScanProgress, ScanResults,
    ScanSetup, SnapshotId, ValidationView,
};
use fuzzy_dedupe_engine::cleanup_sim::{DialogStatus, RunReport, RunSummary, confirmed_count};
use fuzzy_dedupe_engine::compare::{
    AlignedComparison, Comparison, ComparisonOutcome, ComparisonRequest, Unavailable,
};
use fuzzy_dedupe_engine::review::ForecastFacts;

use crate::ui::dialogs::DialogDraft;
use crate::ui::theme::Zoom;

/// Which workspace fills the work area. Exactly Findings or Compare; whether
/// a popup is open is [`DialogState`]'s business and independent of this.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum WorkspaceState {
    #[default]
    Findings,
    Compare,
}

/// A Findings table row, by stable identity rather than position, so focus
/// and expansion survive sorting and filtering (spec §11.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RowId {
    Group(GroupId),
    Member { group: GroupId, file: FileId },
}

/// The group sort keys of spec §11.4; [`SortKey::MatchType`] is the default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SortKey {
    #[default]
    MatchType,
    GroupSize,
    TotalSize,
    NewestChange,
    Folder,
}

/// The Findings table's columns (spec §11.4), keyed by stable ID so width
/// preferences survive a column being hidden.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ColumnId {
    Queue,
    Group,
    Match,
    Unique,
    FileName,
    Folder,
    Modified,
    Size,
    Protected,
    Decision,
}

impl ColumnId {
    pub const ALL: [Self; 10] = [
        Self::Queue,
        Self::Group,
        Self::Match,
        Self::Unique,
        Self::FileName,
        Self::Folder,
        Self::Modified,
        Self::Size,
        Self::Protected,
        Self::Decision,
    ];

    /// The spec §11.4 reference width in units, before zoom.
    #[must_use]
    pub const fn reference_width(self) -> f32 {
        match self {
            Self::Queue => 48.0,
            Self::Group => 104.0,
            Self::Match => 240.0,
            Self::Unique => 76.0,
            Self::FileName => 280.0,
            Self::Folder => 184.0,
            Self::Modified => 120.0,
            Self::Size => 72.0,
            Self::Protected => 88.0,
            Self::Decision => 196.0,
        }
    }
}

/// The Findings workspace: the scan it shows plus the seven IR-001
/// return-context properties, which persist while Compare is shown so that
/// returning restores them exactly.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FindingsState {
    /// The scan these properties were built for, and a revision bumped each
    /// time its rows must be re-projected (results, review or filter change).
    pub scan: Option<ScanId>,
    pub revision: u64,
    // The seven IR-001 return-context properties.
    pub search: String,
    /// `None` is the All filter.
    pub match_filter: Option<RelationKind>,
    pub sort: SortKey,
    pub expanded: BTreeSet<GroupId>,
    pub focused: Option<RowId>,
    /// Widths the user changed; a missing column uses its reference width.
    pub column_widths: BTreeMap<ColumnId, f32>,
    /// Vertical scroll offset of the table body, in logical units, as the
    /// table's own scroll area last reported it. `grouped_table` records
    /// it on every frame the table draws, and applies it once on the first
    /// frame the table draws again after a frame it did not draw (a view
    /// restore, such as the return from Compare). The restored table shows
    /// the recorded offset on that first frame; a different value written
    /// while the table is hidden shows from the frame after (see
    /// `grouped_table::ScrollProbe`).
    pub scroll_offset: f32,
    // View state outside the return context (plan T13).
    /// Optional columns hidden through Actions ▾ → Choose columns….
    pub hidden_columns: BTreeSet<ColumnId>,
    /// Short group titles for the File name column of a header row, such
    /// as the design scenario's "Project plan". A group without one shows
    /// its reference's file name.
    pub group_titles: BTreeMap<GroupId, String>,
    /// The discovery-problem list (frame 01c) is expanded.
    pub show_issues: bool,
    /// The group of Compare's current pair, whose header reads "Last
    /// compared"; copied from `CompareState` by the view each frame.
    pub last_compared: Option<GroupId>,
    /// The focused row moved by keyboard; the table scrolls it into view on
    /// its next draw.
    pub reveal_focus: bool,
    /// The visible-row projection, rebuilt only when its inputs change
    /// (spec §11.9), never per frame.
    pub projection: Option<Arc<crate::presentation::projections::FindingsProjection>>,
}

/// A text position inside one Compare pane: an aligned row and a character
/// offset in that row's source line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TextPosition {
    pub row: usize,
    pub column: usize,
}

/// One pane's model-backed selection (spec §11.5, IR-006). It never crosses
/// into the other pane.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct PaneSelection {
    pub anchor: Option<TextPosition>,
    pub cursor: Option<TextPosition>,
}

/// The pair Compare shows: `target` is the row opened, `counterpart` the
/// file it is measured against (spec §10.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ComparePair {
    pub group: GroupId,
    pub target: FileId,
    pub counterpart: FileId,
}

/// The current C-04 comparison: the request whose result is current, the
/// key it was asked for, and the result once delivered.
///
/// One slot, keyed by pair, serves both Compare and Review unique text.
/// Rejected: a second request slot for the popup. The engine's comparison
/// service keeps one current request and abandons any older one at its next
/// phase boundary (C-04 supersession), so two in-flight slots would leave
/// Compare's request silently never answered. Instead Compare's view state
/// (pair, candidates, mode, wrap, linked scroll, split, change, per-pane
/// selection, return context) lives in [`CompareState`]'s other fields and is
/// never touched by a popup. A popup that needs a different pair submits its
/// own `LoadComparison`, which takes this slot; when Compare draws again and
/// [`ComparisonSlot::matches`] fails for its pair, it submits its request
/// again, which the engine answers from its result cache. Events from any
/// request but [`ComparisonSlot::request`] are stale and discarded.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ComparisonSlot {
    pub request: Option<RequestId>,
    pub key: Option<ComparisonRequest>,
    /// Behind `Arc`: a 5 MB pair is shared, never copied, per frame.
    pub result: Option<Arc<Comparison>>,
}

impl ComparisonSlot {
    /// Whether the slot holds, or awaits, exactly this request's key.
    #[must_use]
    pub fn matches(&self, key: &ComparisonRequest) -> bool {
        self.key.as_ref() == Some(key)
    }

    /// The aligned rows, counts, unique lines and copy text, once ready.
    #[must_use]
    pub fn aligned(&self) -> Option<&AlignedComparison> {
        match &self.result.as_ref()?.outcome {
            ComparisonOutcome::Ready(aligned) => Some(aligned),
            ComparisonOutcome::Unavailable(_) => None,
        }
    }

    /// Why the delivered comparison could not be shown, if it could not.
    #[must_use]
    pub fn unavailable(&self) -> Option<Unavailable> {
        match &self.result.as_ref()?.outcome {
            ComparisonOutcome::Unavailable(why) => Some(*why),
            ComparisonOutcome::Ready(_) => None,
        }
    }
}

/// The Compare workspace (spec §11.8 `CompareState`).
#[derive(Clone, Debug, PartialEq)]
pub struct CompareState {
    pub pair: Option<ComparePair>,
    /// The candidate stepper's files and the index shown.
    pub candidates: Vec<FileId>,
    pub candidate_index: usize,
    pub target_snapshot: Option<SnapshotId>,
    pub counterpart_snapshot: Option<SnapshotId>,
    /// The one comparison presentation tracks; see [`ComparisonSlot`] for
    /// why Review unique text shares it.
    pub comparison: ComparisonSlot,
    pub mode: CompareMode,
    pub wrap: bool,
    pub linked_scroll: bool,
    /// Share of the pane width given to pane A, 0 to 1; 0.5 is the equal split.
    pub split_ratio: f32,
    pub active_change: usize,
    /// Pane A then pane B.
    pub selection: [PaneSelection; 2],
    /// The Findings row Compare was entered from, refocused on return.
    pub return_row: Option<RowId>,
    /// The request Compare's pair was last answered for, in any mode:
    /// both pinned snapshots and the rules revision the panes show. Written
    /// only by `ui/compare.rs`, from [`ComparisonSlot::key`] whenever the
    /// slot holds this pair. Compare re-requests from it (a mode switch, or
    /// the slot taken by Review unique text), so the panes keep showing the
    /// evidence they were opened on, and the stale-evidence check compares
    /// it with the published view. Rejected: rebuilding the request from
    /// the current snapshots, which would silently re-aim the panes at
    /// changed text and hide that the evidence went stale.
    pub pinned: Option<ComparisonRequest>,
}

impl Default for CompareState {
    fn default() -> Self {
        Self {
            pair: None,
            candidates: Vec::new(),
            candidate_index: 0,
            target_snapshot: None,
            counterpart_snapshot: None,
            comparison: ComparisonSlot::default(),
            mode: CompareMode::Original,
            // Frame `13:1304` shows Wrap on; `PreferencesState`'s default
            // agrees, so a fresh Compare and a fresh preferences file match.
            wrap: true,
            linked_scroll: true,
            split_ratio: 0.5,
            active_change: 0,
            selection: [PaneSelection::default(); 2],
            return_row: None,
            pinned: None,
        }
    }
}

/// Every application-owned popup (spec §11.6). Each has its own file under
/// `ui/dialogs/`, owned by the task named there.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DialogKind {
    MatchingRules,
    NewScan,
    UniqueTextReview,
    QueueIdentical,
    Cleanup,
    KeyboardShortcuts,
    About,
}

/// What Enter does in a popup. Neither variant is destructive: Enter never
/// moves a file to Trash (spec §11.6, §11.10).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DefaultAction {
    /// Close without committing anything, like Cancel or Close.
    Dismiss,
    /// The popup's own non-destructive primary action (Save rules, Start
    /// scan, Queue for cleanup, Queue N files), handled by its `submit`.
    Primary,
}

impl DialogKind {
    pub const ALL: [Self; 7] = [
        Self::MatchingRules,
        Self::NewScan,
        Self::UniqueTextReview,
        Self::QueueIdentical,
        Self::Cleanup,
        Self::KeyboardShortcuts,
        Self::About,
    ];

    /// The popup's explicit title.
    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Self::MatchingRules => "Matching rules",
            Self::NewScan => "New scan",
            Self::UniqueTextReview => "Review unique text",
            Self::QueueIdentical => "Queue identical copies",
            Self::Cleanup => "Review cleanup",
            Self::KeyboardShortcuts => "Keyboard shortcuts",
            Self::About => "About Fuzzy DeDupe",
        }
    }

    /// The spec §11.6 initial size in units. The dialog host shrinks it to
    /// the usable viewport; content then scrolls inside it.
    #[must_use]
    pub const fn reference_size(self) -> [f32; 2] {
        match self {
            Self::MatchingRules => [420.0, 421.0],
            Self::NewScan => [640.0, 528.0],
            Self::UniqueTextReview => [600.0, 422.0],
            Self::QueueIdentical => [560.0, 384.0],
            Self::Cleanup => [960.0, 423.0],
            Self::KeyboardShortcuts => [960.0, 494.0],
            Self::About => [480.0, 387.0],
        }
    }

    /// Enter's action. Review cleanup dismisses: its Cancel holds initial
    /// focus and its primary action moves files to Trash, which Enter must
    /// never trigger. Keyboard shortcuts and About only have Close.
    #[must_use]
    pub const fn default_action(self) -> DefaultAction {
        match self {
            Self::Cleanup | Self::KeyboardShortcuts | Self::About => DefaultAction::Dismiss,
            Self::MatchingRules | Self::NewScan | Self::UniqueTextReview | Self::QueueIdentical => {
                DefaultAction::Primary
            }
        }
    }
}

/// The menus the shell and views open. They share the one overlay slot with
/// popups, so a menu and a popup are never open together.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MenuKind {
    /// The main menu (rail Menu entry, F10).
    Main,
    /// The Actions menu (§11.3): Findings' Actions ▾, or Compare's More
    /// actions ⋯, which holds the toolbar actions that overflowed. Each
    /// view draws it only while it is the workspace.
    Actions,
    /// Findings toolbar Sort ▾.
    Sort,
    /// A Findings row menu (right-click, Menu key, Shift+F10).
    Row(RowId),
    /// Compare's "Only in A" (false) or "Only in B" (true) menu.
    OnlyIn { b: bool },
}

/// The control focus returns to when an overlay closes. `None` when the
/// overlay was opened with no focusable invoker on screen.
pub type Invoker = Option<egui::Id>;

/// The pair Review unique text reviews, with the evidence it binds to: the
/// acknowledgment it sends is valid only for these snapshots and rules
/// (C-05 I-7). Built by `commands::dispatch` from a `NeedsReview` outcome,
/// or by a view that opens the popup directly.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct UniqueTextSubject {
    pub scan: ScanId,
    pub target: FileId,
    pub counterpart: FileId,
    pub target_snapshot: SnapshotId,
    pub counterpart_snapshot: SnapshotId,
    pub rules: RulesRevision,
    /// The comparison the pair was reviewed in, when opened from Compare;
    /// the queue request it sends carries it (C-05 Compare origin).
    pub reviewed: Option<RequestId>,
    /// The entry point whose queue request needed review.
    pub origin: QueueOrigin,
}

impl UniqueTextSubject {
    /// The comparison request for this pair in `mode`.
    #[must_use]
    pub const fn comparison(&self, mode: CompareMode) -> ComparisonRequest {
        ComparisonRequest {
            scan: self.scan,
            target: self.target,
            counterpart: self.counterpart,
            target_snapshot: self.target_snapshot,
            counterpart_snapshot: self.counterpart_snapshot,
            rules: self.rules,
            mode,
        }
    }
}

/// What a popup is about. Matching rules and New scan read the preferences,
/// Review cleanup the [`OperationState`], Keyboard shortcuts and About
/// nothing; only these two carry a subject of their own.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum DialogSubject {
    #[default]
    None,
    /// Review unique text's pair.
    UniqueText(UniqueTextSubject),
    /// Queue identical copies' C-05 scope.
    QueueIdentical(QueueScope),
}

impl DialogSubject {
    /// Whether this subject is the one `kind` needs; the dialog host
    /// refuses an open whose subject does not fit.
    #[must_use]
    pub const fn fits(&self, kind: DialogKind) -> bool {
        matches!(
            (self, kind),
            (Self::UniqueText(_), DialogKind::UniqueTextReview)
                | (Self::QueueIdentical(_), DialogKind::QueueIdentical)
                | (
                    Self::None,
                    DialogKind::MatchingRules
                        | DialogKind::NewScan
                        | DialogKind::Cleanup
                        | DialogKind::KeyboardShortcuts
                        | DialogKind::About
                )
        )
    }
}

/// An open popup: its kind, invoker, draft, validation and pending state.
#[derive(Clone, Debug, PartialEq)]
pub struct OpenDialog {
    pub kind: DialogKind,
    pub subject: DialogSubject,
    pub invoker: Invoker,
    /// The popup's own draft type, defined in its dialog file. Closing the
    /// popup drops it; only the popup's `submit` commits anything.
    pub draft: DialogDraft,
    /// Inline validation messages shown in the popup.
    pub validation: Vec<String>,
    /// A submitted action is waiting for the engine; repeated submission is
    /// disabled while set.
    pub pending: bool,
}

/// The single overlay slot: nothing, one menu, or one popup.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Overlay {
    #[default]
    None,
    Menu {
        kind: MenuKind,
        invoker: Invoker,
    },
    Dialog(OpenDialog),
}

/// At most one popup or menu at a time (spec §11.6, C-12). Opening goes
/// through `ui::widgets::dialog_host`, which refuses a second request.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DialogState {
    pub overlay: Overlay,
    /// The overlay was just opened: its first control takes keyboard focus
    /// on its first frame, so focus is visible inside it at once.
    pub focus_entry: bool,
    /// Focus to restore on the next frame, set when an overlay closes.
    pub restore_focus: Option<egui::Id>,
    /// Folders the picker returned for the open popup (New scan's + Add
    /// folder), waiting for the popup to take them.
    pub picked_folders: Option<Vec<PathBuf>>,
}

impl DialogState {
    /// Whether a popup or menu is open, which blocks every underlying
    /// shortcut and pointer interaction.
    #[must_use]
    pub fn is_open(&self) -> bool {
        !matches!(self.overlay, Overlay::None)
    }

    #[must_use]
    pub fn dialog(&self) -> Option<&OpenDialog> {
        match &self.overlay {
            Overlay::Dialog(dialog) => Some(dialog),
            _ => None,
        }
    }

    #[must_use]
    pub fn menu(&self) -> Option<MenuKind> {
        match self.overlay {
            Overlay::Menu { kind, .. } => Some(kind),
            _ => None,
        }
    }
}

/// What the user intends for a queued file. Moving to the system Trash is
/// the only cleanup action V1 has (FR-021).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum IntendedAction {
    #[default]
    MoveToTrash,
}

/// One queued cleanup target as the review service last reported it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct QueueEntry {
    pub counterpart: FileId,
    pub action: IntendedAction,
    /// The unique-text acknowledgment or queue approval the target holds,
    /// bound to its evidence (C-05 I-7).
    pub acknowledgment: Option<DecisionBinding>,
}

/// The queue read model (plan D-003): the engine review service is the only
/// authority, and this mirrors its outcomes. A new scan clears it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct QueueState {
    /// Queued targets by stable file ID; one entry per file however many
    /// groups show it.
    pub entries: BTreeMap<FileId, QueueEntry>,
    /// Each reviewed file's review and protection state (the Decision
    /// column), from the latest outcome that changed it.
    pub decisions: BTreeMap<FileId, MemberDecision>,
    /// Each file's current role, reference or member with its counterpart
    /// and evidence, as the latest review outcome naming that file reported
    /// it. Make this the reference and a Compare review change roles
    /// without touching the immutable results, so a file no outcome has
    /// named still has the role the results give it. Written only by
    /// `events::apply_review`; a new scan's results reset it with the rest
    /// of this owner (`events::install_results`). Read through
    /// [`QueueState::role`], never derived from decisions or queue entries.
    pub roles: BTreeMap<FileId, MemberRole>,
    /// The engine's newest published validation view: current snapshots,
    /// rules revision, protected files and the queue with its bindings.
    /// Comparison requests and stale-evidence checks read snapshots here.
    pub view: Option<Arc<ValidationView>>,
    /// The latest refusal, so the refused row can show its label (C-05).
    pub last_refusal: Option<(FileId, Refusal)>,
}

impl QueueState {
    #[must_use]
    pub fn queued_count(&self) -> usize {
        self.entries.len()
    }

    /// `file`'s current snapshot per the engine, if it knows the file.
    #[must_use]
    pub fn snapshot(&self, file: FileId) -> Option<SnapshotId> {
        self.view.as_ref()?.snapshots.get(&file).copied()
    }

    #[must_use]
    pub fn is_queued(&self, file: FileId) -> bool {
        self.entries.contains_key(&file)
    }

    /// Whether no route may queue `file` per the engine's published view.
    #[must_use]
    pub fn is_protected(&self, file: FileId) -> bool {
        self.view
            .as_ref()
            .is_some_and(|view| view.is_protected(file))
            || self.decisions.get(&file) == Some(&MemberDecision::Protected)
    }

    /// `member`'s current role: the one the review service last reported
    /// for it, else the results' own.
    #[must_use]
    pub fn role(&self, member: &Member) -> MemberRole {
        self.roles.get(&member.file).copied().unwrap_or(member.role)
    }

    /// The group's current reference: the member whose current role is
    /// Reference. Make this the reference reports both the old and the new
    /// reference, so exactly one member holds the role. A malformed group
    /// with none falls back to its first member, as the review service does.
    #[must_use]
    pub fn reference(&self, group: &Group) -> Option<FileId> {
        group
            .members
            .iter()
            .find(|m| matches!(self.role(m), MemberRole::Reference))
            .or_else(|| group.members.first())
            .map(|m| m.file)
    }
}

/// The read model's answers to the Queue identical copies forecast
/// (`fuzzy_dedupe_engine::review::forecast`). The popup states what the
/// review service will queue only while these agree with the service's own
/// answers (`ReviewState` in the engine's `review/policy.rs`), so each one
/// reads a mirror of an outcome, the published view or the results, never
/// a local guess.
impl ForecastFacts for QueueState {
    fn reference(&self, group: &Group) -> Option<FileId> {
        Self::reference(self, group)
    }

    fn counterpart(&self, group: &Group, file: FileId) -> Option<FileId> {
        if Self::reference(self, group) == Some(file) {
            return None;
        }
        match self.role(group.member(file)?) {
            MemberRole::Reference => None,
            MemberRole::Member(link) => Some(link.counterpart),
        }
    }

    fn is_protected(&self, file: FileId) -> bool {
        Self::is_protected(self, file)
    }

    /// A Kept file that is also protected reads as Protected here, the
    /// Decision column's precedence, and so as not kept. The forecast
    /// cannot tell the difference: it skips a protected file whatever its
    /// Kept state.
    fn is_kept(&self, file: FileId) -> bool {
        self.decisions.get(&file) == Some(&MemberDecision::Kept)
    }

    fn queued_counterpart(&self, file: FileId) -> Option<FileId> {
        self.entries.get(&file).map(|entry| entry.counterpart)
    }
}

/// A scan's lifecycle stage as presentation shows it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ScanStage {
    /// No scan has run or been loaded.
    #[default]
    Idle,
    Running,
    /// Stop was requested; waiting for the workers' acknowledgment.
    Stopping,
    Completed,
    Partial,
}

/// A local wall-clock time of day, for "finished 14:32".
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TimeOfDay {
    pub hour: u8,
    pub minute: u8,
}

/// The current scan (spec §11.8 `ScanState`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScanState {
    /// The scan whose events are current; any other scan's are stale.
    pub scan: Option<ScanId>,
    /// The immutable copies the scan used.
    pub setup: Option<ScanSetup>,
    pub rules: Option<MatchingRules>,
    pub stage: ScanStage,
    pub progress: Option<ScanProgress>,
    pub results: Option<Arc<ScanResults>>,
    /// Text files examined, for the status bar.
    pub files_scanned: u64,
    pub finished_at: Option<TimeOfDay>,
    /// Discovery problems, kept accessible after the scan (01c).
    pub issues: Vec<ScanIssue>,
    /// Errors that stopped or refused a scan request.
    pub errors: Vec<String>,
}

/// Where the one cleanup dialog is (spec §11.6): its review, running,
/// stopped and finished states.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CleanupPhase {
    /// No plan requested.
    #[default]
    Idle,
    /// `BuildCleanupPlan` admitted; waiting for `PlanBuilt`.
    Building,
    /// A plan (first or revised) awaits confirmation.
    Review,
    /// Confirmed and running.
    Running,
    /// Stop requested; waiting for `RunEnded`.
    Stopping,
    /// The run ended with items left: the revised plan in
    /// [`OperationState::plan`] needs confirmation again.
    Stopped,
    /// The run ended with nothing left to confirm.
    Finished,
}

/// The cleanup operation (spec §11.8 `OperationState`), from T8's C-06
/// events. Review cleanup (T18) renders from this alone.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OperationState {
    /// The engine-issued plan revision whose events are current. After a
    /// run ends with a revised plan, the revised revision.
    pub revision: Option<PlanRevision>,
    /// The current plan as built or revised.
    pub plan: Option<Arc<CleanupPlan>>,
    /// The plan's items with every `ItemChanged` applied: per-file
    /// validation and execution outcomes.
    pub items: Vec<PlanItem>,
    pub phase: CleanupPhase,
    /// The user confirmed [`Self::revision`]; each revision runs at most
    /// once, and a revised plan starts unauthorized.
    pub authorized: bool,
    /// The last ended run: final items, the per-item journal (Show journal)
    /// and, through [`RunReport::summary`], "N moved · N failed".
    pub report: Option<Arc<RunReport>>,
}

impl OperationState {
    /// "Move N files to Trash": the current plan's confirmable items, by
    /// T8's [`confirmed_count`]; blocked items are excluded.
    #[must_use]
    pub fn confirmed_count(&self) -> usize {
        self.plan.as_deref().map_or(0, confirmed_count)
    }

    /// The last run's summary counts.
    #[must_use]
    pub fn summary(&self) -> Option<RunSummary> {
        self.report.as_ref().map(|report| report.summary())
    }

    /// The Status column for each live item, by T8's [`DialogStatus::of`].
    pub fn statuses(&self) -> impl Iterator<Item = (&PlanItem, DialogStatus)> {
        self.items
            .iter()
            .map(|item| (item, DialogStatus::of(item.status)))
    }
}

/// The theme override in the main menu's Appearance row (FR-027).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ThemeOverride {
    /// Follow the desktop's light or dark setting, live.
    #[default]
    System,
    Light,
    Dark,
}

impl ThemeOverride {
    pub const ALL: [Self; 3] = [Self::System, Self::Light, Self::Dark];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }
}

/// Window geometry remembered across runs. No position: Wayland lets an
/// application neither read nor set it (DR-009).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindowPreference {
    pub width: f32,
    pub height: f32,
    pub maximized: bool,
}

/// Every C-08 `preferences.toml` field. T20 loads and writes it through the
/// platform adapter in `presentation/preferences.rs`; results, the queue and
/// review decisions are never part of it.
#[derive(Clone, Debug, PartialEq)]
pub struct PreferencesState {
    /// The file format version; this build writes 1.
    pub version: u32,
    /// `None` until a size has been recorded; the default window size applies.
    pub window: Option<WindowPreference>,
    pub zoom: Zoom,
    pub theme: ThemeOverride,
    /// The user collapsed the rail. Below 1280 units it collapses anyway.
    pub rail_collapsed: bool,
    pub findings_column_widths: BTreeMap<ColumnId, f32>,
    pub compare_split_ratio: f32,
    pub compare_wrap: bool,
    pub compare_linked_scroll: bool,
    /// The current scan setup: folders with protect flags, and file types.
    pub scan_setup: ScanSetup,
    /// Saved matching rules; `None` until the user saves a set, because no
    /// algorithm default is approved (OQ-005).
    pub saved_rules: Option<MatchingRules>,
}

impl PreferencesState {
    pub const VERSION: u32 = 1;
}

impl Default for PreferencesState {
    fn default() -> Self {
        Self {
            version: Self::VERSION,
            window: None,
            zoom: Zoom::default(),
            theme: ThemeOverride::System,
            rail_collapsed: false,
            findings_column_widths: BTreeMap::new(),
            compare_split_ratio: 0.5,
            // On, as frame `13:1304` shows it (T40). A stored `wrap` in
            // `preferences.toml` still wins; this is only the value a file
            // without one, or no file, starts from (`CompareV1::default`).
            compare_wrap: true,
            compare_linked_scroll: true,
            scan_setup: ScanSetup {
                folders: Vec::new(),
                file_types: Vec::new(),
            },
            saved_rules: None,
        }
    }
}

/// How serious a status message is; it picks the token the status bar uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

/// One-line per-operation feedback in the status bar, such as a refused
/// popup or an opener failure (spec §11.10).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusMessage {
    pub severity: Severity,
    pub text: String,
}

/// Every presentation owner, held by `app.rs`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PresentationState {
    pub workspace: WorkspaceState,
    pub findings: FindingsState,
    pub compare: CompareState,
    pub dialog: DialogState,
    pub queue: QueueState,
    pub scan: ScanState,
    pub operation: OperationState,
    pub preferences: PreferencesState,
    pub status: Option<StatusMessage>,
}
