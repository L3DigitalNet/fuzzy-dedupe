// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! Typed requests from the UI to the application services (plan C-12), and
//! the global keyboard dispatcher (spec §11.10).
//!
//! Views never call the engine, the platform adapters or the clipboard while
//! drawing (spec §5.2). They push a [`UiIntent`] into the frame's
//! [`IntentQueue`]; `app.rs` drains the queue after the UI pass and hands
//! each intent to [`dispatch`], the only code that reaches [`Services`].
//! Review commands are synchronous engine calls there (plan D-008); jobs are
//! submitted without blocking and their results arrive as events
//! (`presentation/events.rs`).
//!
//! Open-location intents go to the C-07 [`Opener`], whose handle is polled
//! on later frames, never waited on; copy-path intents go to egui's
//! clipboard output. Nothing here deletes, writes or spawns anything itself.

use std::path::PathBuf;

use egui::{Context, Key, ViewportCommand};
use fuzzy_dedupe_domain::{
    CompareMode, FileId, GroupId, MatchingRules, Member, QueueOrigin, ScanSetup,
};
use fuzzy_dedupe_engine::compare::ComparisonRequest;
use fuzzy_dedupe_engine::service::{
    Engine, EngineRequest, ReviewCommand, ReviewStatus, SubmitError,
};
use fuzzy_dedupe_platform::dirs::AppDirs;
use fuzzy_dedupe_platform::open::{Location, OpenHandle, Opener};
use fuzzy_dedupe_platform::picker::{FolderPicker, PickHandle, PickOutcome, PickRequest};

use crate::presentation::events;
use crate::presentation::state::{
    ComparePair, DialogKind, DialogSubject, Invoker, MenuKind, PresentationState, RowId, Severity,
    StatusMessage, ThemeOverride, UniqueTextSubject, WorkspaceState,
};
use crate::ui::widgets::dialog_host;

/// One request from the UI, dispatched after the UI pass.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UiIntent {
    /// Open a popup about `subject` (see [`DialogSubject::fits`]). Refused
    /// while any popup or menu is open, or when the subject does not fit.
    OpenDialog {
        kind: DialogKind,
        subject: DialogSubject,
        invoker: Invoker,
    },
    /// Open a menu, or close it when that menu is already open.
    ToggleMenu {
        kind: MenuKind,
        invoker: Invoker,
    },
    /// Close the open popup or menu without committing anything, and return
    /// focus to its invoker.
    CloseOverlay,
    SwitchWorkspace(WorkspaceState),
    /// Show `target` against `counterpart` in Compare (Enter on a row, the
    /// Compare action, Review unique text's Open comparison): sets Compare's
    /// pair from the current snapshots, requests the comparison in Compare's
    /// mode and switches workspace. `return_row` is refocused on return.
    OpenCompare {
        target: FileId,
        counterpart: FileId,
        return_row: Option<RowId>,
    },
    SetTheme(ThemeOverride),
    ZoomIn,
    ZoomOut,
    ZoomReset,
    /// Matching rules' Save rules: the saved rules, not the current scan's
    /// stored copy (spec §11.6).
    SetSavedRules(MatchingRules),
    /// New scan: the current scan setup preference (folders, protection,
    /// file types).
    SetScanSetup(ScanSetup),
    /// New scan's + Add folder. The chosen folders arrive in
    /// `DialogState::picked_folders` on a later frame.
    PickFolders(PickRequest),
    /// Main menu Open log folder: the adapter-resolved log directory.
    OpenLogFolder,
    /// Row menu Open containing folder.
    OpenContainingFolder(FileId),
    /// About's Source code, and any other `https` link.
    OpenWebAddress(String),
    /// Row menu Copy path, Ctrl+Shift+C.
    CopyPath(FileId),
    /// Copy unique lines, Copy those lines, and any other copy of text a
    /// view already holds.
    CopyText(String),
    /// A synchronous review command (C-05). `invoker` is the control that
    /// sent it: when a queue request answers `NeedsReview`, Review unique
    /// text opens for that pair and closing it returns focus there.
    Review {
        command: ReviewCommand,
        invoker: Invoker,
    },
    /// An asynchronous engine job (C-02): scans, comparisons, cleanup.
    Submit(EngineRequest),
    Quit,
}

/// The UI's intents for one frame, drained by `app.rs` after the UI pass.
///
/// Bounded: a view that pushes in a loop cannot grow it without limit. An
/// intent beyond the bound is dropped with a warning, which loses one input
/// rather than stalling the frame.
#[derive(Debug, Default)]
pub struct IntentQueue {
    items: Vec<UiIntent>,
}

impl IntentQueue {
    /// More than any single frame of input can produce.
    pub const CAPACITY: usize = 256;

    pub fn push(&mut self, intent: UiIntent) {
        if self.items.len() < Self::CAPACITY {
            self.items.push(intent);
        } else {
            tracing::warn!(?intent, "intent queue full; dropping an intent");
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Takes every queued intent, oldest first.
    pub fn drain(&mut self) -> Vec<UiIntent> {
        std::mem::take(&mut self.items)
    }
}

/// The application services intents reach. Only [`dispatch`] and `app.rs`
/// touch them, never a widget.
pub struct Services {
    pub engine: Engine,
    pub opener: Box<dyn Opener>,
    pub picker: Box<dyn FolderPicker>,
    pub dirs: AppDirs,
    /// Opens in flight; polled each frame by [`Services::poll`].
    pending_opens: Vec<OpenHandle>,
    /// The folder pick in flight, at most one.
    pending_pick: Option<PickHandle>,
    /// The validation-view revision last mirrored into presentation.
    view_revision: Option<u64>,
}

impl Services {
    #[must_use]
    pub fn new(
        engine: Engine,
        opener: Box<dyn Opener>,
        picker: Box<dyn FolderPicker>,
        dirs: AppDirs,
    ) -> Self {
        Self {
            engine,
            opener,
            picker,
            dirs,
            pending_opens: Vec::new(),
            pending_pick: None,
            view_revision: None,
        }
    }

    fn open(&mut self, ctx: &Context, location: Location) {
        let repaint = ctx.clone();
        let handle = self
            .opener
            .open(location, Box::new(move || repaint.request_repaint()));
        self.pending_opens.push(handle);
    }

    /// Collects finished opens and picks without blocking; a failure
    /// becomes a status message (C-07: errors are actionable status, never
    /// a panic). Also mirrors a newer validation view.
    pub fn poll(&mut self, state: &mut PresentationState) {
        self.pending_opens.retain_mut(|handle| match handle.poll() {
            None => true,
            Some(Ok(())) => false,
            Some(Err(err)) => {
                tracing::warn!(%err, "open location failed");
                state.status = Some(error(err.to_string()));
                false
            }
        });
        if let Some(handle) = self.pending_pick.as_mut()
            && let Some(result) = handle.poll()
        {
            self.pending_pick = None;
            match result {
                Ok(PickOutcome::Chosen(folders)) => state.dialog.picked_folders = Some(folders),
                Ok(PickOutcome::Dismissed) => {}
                Err(err) => state.status = Some(error(err.to_string())),
            }
        }
        self.sync_view(state);
    }

    /// Mirrors the engine's newest validation view into the queue read
    /// model when its revision changed.
    pub fn sync_view(&mut self, state: &mut PresentationState) {
        let Some(published) = self.engine.validation().current() else {
            return;
        };
        if self.view_revision == Some(published.revision) {
            return;
        }
        self.view_revision = Some(published.revision);
        events::sync_view(state, published.view);
    }
}

fn error(text: String) -> StatusMessage {
    StatusMessage {
        severity: Severity::Error,
        text,
    }
}

fn find_member(state: &PresentationState, file: FileId) -> Option<(GroupId, &Member)> {
    state
        .scan
        .results
        .as_ref()?
        .groups
        .iter()
        .find_map(|group| {
            let member = group.members.iter().find(|member| member.file == file)?;
            Some((group.id, member))
        })
}

/// The comparison request for a pair in `mode`, from the engine's current
/// snapshots and rules (falling back to the scan's own), or `None` when no
/// scan holds both files.
#[must_use]
pub fn comparison_request(
    state: &PresentationState,
    target: FileId,
    counterpart: FileId,
    mode: CompareMode,
) -> Option<ComparisonRequest> {
    let results = state.scan.results.as_ref()?;
    let snapshot = |file| {
        state
            .queue
            .snapshot(file)
            .or_else(|| find_member(state, file).map(|(_, m)| m.snapshot))
    };
    let rules = state
        .queue
        .view
        .as_ref()
        .map_or(results.rules.revision, |view| view.rules);
    Some(ComparisonRequest {
        scan: results.scan,
        target,
        counterpart,
        target_snapshot: snapshot(target)?,
        counterpart_snapshot: snapshot(counterpart)?,
        rules,
        mode,
    })
}

/// Carries out one intent. Intents the dispatch itself produces (a popup's
/// `open` asking for a comparison or a plan) go to `followups`, which
/// `app.rs` dispatches in the same frame.
pub fn dispatch(
    intent: UiIntent,
    state: &mut PresentationState,
    services: &mut Services,
    ctx: &Context,
    followups: &mut IntentQueue,
) {
    match intent {
        UiIntent::OpenDialog {
            kind,
            subject,
            invoker,
        } => {
            if let Err(refused) = dialog_host::open(state, kind, subject, invoker, followups) {
                tracing::debug!(?kind, ?refused, "popup request refused");
            }
        }
        UiIntent::ToggleMenu { kind, invoker } => {
            if state.dialog.menu() == Some(kind) {
                dialog_host::close(&mut state.dialog);
            } else if let Err(refused) = dialog_host::open_menu(&mut state.dialog, kind, invoker) {
                tracing::debug!(?kind, ?refused, "menu request refused");
            }
        }
        UiIntent::CloseOverlay => dialog_host::close(&mut state.dialog),
        UiIntent::SwitchWorkspace(workspace) => state.workspace = workspace,
        UiIntent::OpenCompare {
            target,
            counterpart,
            return_row,
        } => open_compare(state, target, counterpart, return_row, followups),
        UiIntent::SetTheme(theme) => state.preferences.theme = theme,
        UiIntent::ZoomIn => state.preferences.zoom = state.preferences.zoom.zoom_in(),
        UiIntent::ZoomOut => state.preferences.zoom = state.preferences.zoom.zoom_out(),
        UiIntent::ZoomReset => state.preferences.zoom = state.preferences.zoom.reset(),
        UiIntent::SetSavedRules(rules) => state.preferences.saved_rules = Some(rules),
        UiIntent::SetScanSetup(setup) => state.preferences.scan_setup = setup,
        UiIntent::PickFolders(request) => {
            if services.pending_pick.is_none() {
                let repaint = ctx.clone();
                let handle = services
                    .picker
                    .pick_folders(request, Box::new(move || repaint.request_repaint()));
                services.pending_pick = Some(handle);
            }
        }
        UiIntent::OpenLogFolder => {
            let dir = services.dirs.log_dir().to_path_buf();
            services.open(ctx, Location::Folder(dir));
        }
        UiIntent::OpenContainingFolder(file) => {
            if let Some((_, member)) = find_member(state, file) {
                let folder = PathBuf::from(&member.folder);
                services.open(ctx, Location::Folder(folder));
            }
        }
        UiIntent::OpenWebAddress(address) => services.open(ctx, Location::WebAddress(address)),
        UiIntent::CopyPath(file) => {
            if let Some((_, member)) = find_member(state, file) {
                ctx.copy_text(member.display_path.clone());
            }
        }
        UiIntent::CopyText(text) => ctx.copy_text(text),
        UiIntent::Review { command, invoker } => {
            review(state, services, command, invoker, followups);
        }
        UiIntent::Submit(request) => match services.engine.submit(request.clone()) {
            Ok(origin) => events::note_submitted(state, origin, &request),
            Err(err) => report_submit_error(state, err),
        },
        UiIntent::Quit => ctx.send_viewport_cmd(ViewportCommand::Close),
    }
}

fn open_compare(
    state: &mut PresentationState,
    target: FileId,
    counterpart: FileId,
    return_row: Option<RowId>,
    followups: &mut IntentQueue,
) {
    let Some((group, _)) = find_member(state, target) else {
        return;
    };
    let Some(request) = comparison_request(state, target, counterpart, state.compare.mode) else {
        return;
    };
    let compare = &mut state.compare;
    if compare.pair
        != Some(ComparePair {
            group,
            target,
            counterpart,
        })
    {
        compare.active_change = 0;
        compare.selection = Default::default();
    }
    compare.pair = Some(ComparePair {
        group,
        target,
        counterpart,
    });
    compare.target_snapshot = Some(request.target_snapshot);
    compare.counterpart_snapshot = Some(request.counterpart_snapshot);
    compare.return_row = return_row;
    state.workspace = WorkspaceState::Compare;
    if !state.compare.comparison.matches(&request) {
        followups.push(UiIntent::Submit(EngineRequest::LoadComparison(request)));
    }
}

/// Applies a review command; a `NeedsReview` answer opens Review unique
/// text for that pair, invoked by the control that sent the request.
fn review(
    state: &mut PresentationState,
    services: &mut Services,
    command: ReviewCommand,
    invoker: Invoker,
    followups: &mut IntentQueue,
) {
    let (origin, reviewed) = match &command {
        ReviewCommand::RequestQueue {
            origin, reviewed, ..
        } => (*origin, *reviewed),
        _ => (QueueOrigin::Checkbox, None),
    };
    // Only the dispatcher knows which command produced the outcome: a bulk
    // Queue identical copies of one file and a single queue request both
    // answer `Queued { files: 1 }`, and the C-05 outcome must not change to
    // tell them apart. Only the bulk result sets a status line.
    let bulk = matches!(command, ReviewCommand::QueueIdenticalCopies { .. });
    let outcome = services.engine.review(command);
    services.sync_view(state);
    if bulk {
        events::apply_bulk_review(state, &outcome);
    } else {
        events::apply_review(state, &outcome);
    }
    if let ReviewStatus::NeedsReview { file, counterpart } = outcome.status {
        let Some(request) = comparison_request(state, file, counterpart, CompareMode::Original)
        else {
            return;
        };
        let subject = UniqueTextSubject {
            scan: request.scan,
            target: file,
            counterpart,
            target_snapshot: request.target_snapshot,
            counterpart_snapshot: request.counterpart_snapshot,
            rules: request.rules,
            reviewed,
            origin,
        };
        // A row menu that sent the request may still be open. It closes
        // without its own focus return: focus must come back to `invoker`,
        // the control that sent the request, when the popup closes.
        if state.dialog.menu().is_some() {
            dialog_host::close(&mut state.dialog);
            state.dialog.restore_focus = None;
        }
        if let Err(refused) = dialog_host::open(
            state,
            DialogKind::UniqueTextReview,
            DialogSubject::UniqueText(subject),
            invoker,
            followups,
        ) {
            tracing::debug!(?refused, "Review unique text not opened");
        }
    }
}

fn report_submit_error(state: &mut PresentationState, err: SubmitError) {
    tracing::warn!(?err, "engine request not admitted");
    let text = match err {
        SubmitError::Busy => "Busy; try again in a moment.".to_owned(),
        SubmitError::NotImplemented(_) => "That action is not available yet.".to_owned(),
        SubmitError::ScanMismatch { .. } => "The results changed; review again.".to_owned(),
        SubmitError::Stopped => "The engine stopped; restart Fuzzy DeDupe.".to_owned(),
    };
    state.status = Some(error(text));
}

/// The actions bound to global chords (spec §11.10, Global scope).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GlobalAction {
    NewScan,
    ShowFindings,
    ShowCompare,
    MatchingRules,
    ReviewCleanup,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    MainMenu,
    KeyboardShortcuts,
    Quit,
}

/// A key with exactly these modifiers. Matching is exact on Ctrl, Alt and
/// Shift: egui's own `consume_shortcut` ignores extra Shift and Alt, which
/// would let Shift+F10 (the Findings row menu) open the main menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Chord {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub key: Key,
}

impl Chord {
    const fn plain(key: Key) -> Self {
        Self {
            ctrl: false,
            alt: false,
            shift: false,
            key,
        }
    }

    const fn ctrl(key: Key) -> Self {
        Self {
            ctrl: true,
            alt: false,
            shift: false,
            key,
        }
    }

    const fn ctrl_shift(key: Key) -> Self {
        Self {
            ctrl: true,
            alt: false,
            shift: true,
            key,
        }
    }

    fn matches(self, key: Key, modifiers: egui::Modifiers) -> bool {
        self.key == key
            && self.ctrl == modifiers.ctrl
            && self.alt == modifiers.alt
            && self.shift == modifiers.shift
    }
}

/// The §11.10 Global rows. There is no single-letter binding: every letter
/// key here carries Ctrl. The Shift variants exist because the logical key
/// for `+` and `?` needs Shift on common layouts; Ctrl+? is the shortcuts
/// window's second chord beside F1.
pub const GLOBAL_KEYMAP: &[(Chord, GlobalAction)] = &[
    (Chord::ctrl(Key::N), GlobalAction::NewScan),
    (Chord::ctrl(Key::Num1), GlobalAction::ShowFindings),
    (Chord::ctrl(Key::Num2), GlobalAction::ShowCompare),
    (Chord::ctrl(Key::Comma), GlobalAction::MatchingRules),
    (Chord::ctrl(Key::Enter), GlobalAction::ReviewCleanup),
    (Chord::ctrl(Key::Equals), GlobalAction::ZoomIn),
    (Chord::ctrl(Key::Plus), GlobalAction::ZoomIn),
    (Chord::ctrl_shift(Key::Plus), GlobalAction::ZoomIn),
    (Chord::ctrl_shift(Key::Equals), GlobalAction::ZoomIn),
    (Chord::ctrl(Key::Minus), GlobalAction::ZoomOut),
    (Chord::ctrl(Key::Num0), GlobalAction::ZoomReset),
    (Chord::plain(Key::F10), GlobalAction::MainMenu),
    (Chord::plain(Key::F1), GlobalAction::KeyboardShortcuts),
    (
        Chord::ctrl(Key::Questionmark),
        GlobalAction::KeyboardShortcuts,
    ),
    (
        Chord::ctrl_shift(Key::Questionmark),
        GlobalAction::KeyboardShortcuts,
    ),
    (
        Chord::ctrl_shift(Key::Slash),
        GlobalAction::KeyboardShortcuts,
    ),
    (Chord::ctrl(Key::Q), GlobalAction::Quit),
];

/// The intent a global action produces. `focused` is the widget focused
/// when the chord was pressed, which becomes the popup's invoker; `menu`
/// is the rail Menu entry, the main menu's invoker however it opens.
#[must_use]
pub fn global_intent(action: GlobalAction, focused: Invoker, menu: Invoker) -> UiIntent {
    let dialog = |kind| UiIntent::OpenDialog {
        kind,
        subject: DialogSubject::None,
        invoker: focused,
    };
    match action {
        GlobalAction::NewScan => dialog(DialogKind::NewScan),
        GlobalAction::ShowFindings => UiIntent::SwitchWorkspace(WorkspaceState::Findings),
        GlobalAction::ShowCompare => UiIntent::SwitchWorkspace(WorkspaceState::Compare),
        GlobalAction::MatchingRules => dialog(DialogKind::MatchingRules),
        GlobalAction::ReviewCleanup => dialog(DialogKind::Cleanup),
        GlobalAction::ZoomIn => UiIntent::ZoomIn,
        GlobalAction::ZoomOut => UiIntent::ZoomOut,
        GlobalAction::ZoomReset => UiIntent::ZoomReset,
        GlobalAction::MainMenu => UiIntent::ToggleMenu {
            kind: MenuKind::Main,
            invoker: menu,
        },
        GlobalAction::KeyboardShortcuts => dialog(DialogKind::KeyboardShortcuts),
        GlobalAction::Quit => UiIntent::Quit,
    }
}

/// Reads and consumes this frame's global chords.
///
/// While a popup or menu is open nothing is dispatched, except F10 closing
/// the main menu: a popup blocks every underlying shortcut (spec §11.6).
/// Blocked chords are still consumed. Left in the input, a blocked chord
/// would reach the popup's own focused widget instead, because egui
/// activates a focused button on Enter whatever the modifiers, so Ctrl+Enter
/// (Review cleanup) would press the popup's close button.
pub fn read_global_chords(
    ctx: &Context,
    state: &PresentationState,
    menu: Invoker,
    intents: &mut IntentQueue,
) {
    let overlay_open = state.dialog.is_open();
    let main_menu_open = state.dialog.menu() == Some(MenuKind::Main);
    let focused = ctx.memory(egui::Memory::focused);
    let actions = ctx.input_mut(|input| {
        let mut actions = Vec::new();
        input.events.retain(|event| {
            let egui::Event::Key {
                key,
                pressed,
                modifiers,
                ..
            } = event
            else {
                return true;
            };
            let Some(&(_, action)) = GLOBAL_KEYMAP
                .iter()
                .find(|(chord, _)| chord.matches(*key, *modifiers))
            else {
                return true;
            };
            let admitted = !overlay_open || (main_menu_open && action == GlobalAction::MainMenu);
            if *pressed && admitted {
                actions.push(action);
            }
            false
        });
        actions
    });
    for action in actions {
        intents.push(global_intent(action, focused, menu));
    }
}

/// The column of the Keyboard shortcuts popup a row belongs to (spec
/// §11.6: Global; Findings; Compare and Popups).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShortcutScope {
    Global,
    Findings,
    Compare,
    Popups,
}

/// One row of the §11.10 keyboard map, as displayed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ShortcutRow {
    pub scope: ShortcutScope,
    /// The spec's Keys cell; " / " separates the chords of one row.
    pub keys: &'static str,
    pub action: &'static str,
}

const fn row(scope: ShortcutScope, keys: &'static str, action: &'static str) -> ShortcutRow {
    ShortcutRow {
        scope,
        keys,
        action,
    }
}

/// The spec §11.10 keyboard map, row for row, for the Keyboard shortcuts
/// popup (T19) to display. The Global rows are bound by [`GLOBAL_KEYMAP`];
/// the Findings and Compare rows by those workspaces (T13, T15); the Popups
/// rows by the dialog host. Cross-file contract: a §11.10 change updates
/// this table and the binding that implements the row together.
pub const SHORTCUT_TABLE: &[ShortcutRow] = &[
    row(ShortcutScope::Global, "Ctrl+N", "New scan"),
    row(
        ShortcutScope::Global,
        "Ctrl+1 / Ctrl+2",
        "Findings / Compare",
    ),
    row(ShortcutScope::Global, "Ctrl+,", "Matching rules"),
    row(ShortcutScope::Global, "Ctrl+Enter", "Open Review cleanup"),
    row(
        ShortcutScope::Global,
        "Ctrl+= / Ctrl+- / Ctrl+0",
        "Zoom in / zoom out / reset zoom",
    ),
    row(ShortcutScope::Global, "F10", "Main menu"),
    row(ShortcutScope::Global, "Ctrl+Q", "Quit"),
    row(ShortcutScope::Findings, "Ctrl+F", "Search; Esc clears it"),
    row(
        ShortcutScope::Findings,
        "Arrow keys, Page Up/Down, Home/End",
        "Move row focus",
    ),
    row(
        ShortcutScope::Findings,
        "Right / Left",
        "Expand / collapse a group",
    ),
    row(
        ShortcutScope::Findings,
        "Enter",
        "Compare the row against its counterpart",
    ),
    row(
        ShortcutScope::Findings,
        "Space",
        "Queue or unqueue; opens Review unique text when review is needed",
    ),
    row(ShortcutScope::Findings, "Menu key or Shift+F10", "Row menu"),
    row(ShortcutScope::Findings, "Ctrl+Shift+C", "Copy path"),
    row(
        ShortcutScope::Compare,
        "Alt+Down / Alt+Up",
        "Next / previous change",
    ),
    row(
        ShortcutScope::Compare,
        "Alt+Right / Alt+Left",
        "Next / previous candidate",
    ),
    row(ShortcutScope::Compare, "F6", "Switch pane"),
    row(
        ShortcutScope::Compare,
        "Alt+Z / Alt+L",
        "Toggle wrap / toggle linked scroll",
    ),
    row(
        ShortcutScope::Compare,
        "Ctrl+A / Ctrl+C",
        "Select the whole file in the pane / copy",
    ),
    row(ShortcutScope::Compare, "Esc", "Back to Findings"),
    row(ShortcutScope::Popups, "Tab / Shift+Tab", "Move focus"),
    row(ShortcutScope::Popups, "Esc", "Cancel"),
    row(
        ShortcutScope::Popups,
        "Enter",
        "Default action, never destructive",
    ),
];
