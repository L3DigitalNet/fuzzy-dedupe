// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! The Review cleanup popup (spec §11.6, frames `16:886`, `27:1678`,
//! `27:1390`, `35:2416`, `27:1533`): one dialog hosting the review,
//! running, stopped and finished states of the simulated cleanup (plan T18).
//!
//! The dialog host (`ui/widgets/dialog_host.rs`) owns the title row, the close
//! control, Esc, Enter, sizing, focus restoration and the pinned footer slot;
//! this file draws the body and each phase's footer inside the 16-unit
//! content inset and supplies the phase's title through [`title`]. Enter
//! only dismisses this popup: its primary action moves files to Trash,
//! which no key may trigger (spec §11.10).
//!
//! Everything shown is read from `OperationState` (C-06 events applied by
//! `presentation/events.rs`) and the scan results; the only things this file
//! sends are `BuildCleanupPlan` when it opens, `ExecuteApprovedPlan` when the
//! user confirms, and `StopCleanup`. No control here, in any state, offers
//! permanent deletion (FR-021, WH-007).
//!
//! Visibility across runs. A run that stops reports its items in
//! `RunReport`, but the revised plan that `OperationState` then holds keeps
//! only the items left to confirm (T8 `run.rs`): moved, failed and changed
//! files leave it. The draft therefore keeps every report seen while the
//! popup is open, and the table and journal merge them, so a failed or
//! changed file stays named with its reason after a retry (spec §11.6
//! Stopped, UI check 11).

use std::cell::RefCell;
use std::sync::Arc;

use egui::{Align, Color32, Layout, Rect, RichText, Stroke, Ui, Vec2};
use fuzzy_dedupe_domain::{
    FileId, Measurement, Member, PlanItem, PlanItemReason, PlanItemStatus, PlanRevision,
};
use fuzzy_dedupe_engine::cleanup_sim::{DialogStatus, RunReport};
use fuzzy_dedupe_engine::service::{BuildCleanupPlan, EngineRequest, ExecuteApprovedPlan};

use crate::presentation::commands::UiIntent;
use crate::presentation::state::{CleanupPhase, OperationState, QueueState, ScanState};
use crate::ui::dialogs::{DialogCx, DialogResponse};
use crate::ui::theme::{self, CompactStyle, Palette, Token};
use crate::ui::widgets::dialog_host;

/// The popup's draft, created when it opens and dropped when it closes, so
/// closing never commits it (spec §11.6).
#[derive(Clone, Debug, PartialEq)]
pub struct Draft {
    /// Cancel still has to take the initial focus. Taken on the first frame
    /// the review is drawn: the plan arrives a few frames after the popup
    /// opens, and the host focuses its close control on the opening frame.
    focus_cancel: bool,
    /// The plan revision this popup confirmed. The confirm button stays
    /// disabled for it even before `ExecuteApprovedPlan` is dispatched and
    /// the phase turns to Running, so a second click in the same frame, or
    /// the frame before dispatch, cannot submit again (single flight). A
    /// revised plan has a new revision and needs its own confirmation.
    submitted: Option<PlanRevision>,
    /// Every run report seen while open, oldest first (see the module docs).
    runs: Vec<Arc<RunReport>>,
    /// Show journal replaces the table with the per-item journal.
    journal: bool,
}

impl Default for Draft {
    fn default() -> Self {
        Self {
            focus_cancel: true,
            submitted: None,
            runs: Vec::new(),
            journal: false,
        }
    }
}

/// The draft for a newly opened popup. Asks for a fresh plan of the current
/// scan's queue, unless a run is in progress: reopening the popup then shows
/// that run rather than replacing its plan.
pub fn open(cx: &mut DialogCx<'_>) -> Draft {
    let running = matches!(
        cx.operation.phase,
        CleanupPhase::Running | CleanupPhase::Stopping
    );
    if !running && let Some(scan) = cx.scan.scan {
        cx.intents
            .push(UiIntent::Submit(EngineRequest::BuildCleanupPlan(
                BuildCleanupPlan { scan },
            )));
    }
    Draft::default()
}

/// The title for the current phase (spec §11.6; frames `27:1390`,
/// `35:2416`, `27:1533`). Called by the dialog host through
/// `DialogDraft::title` in `ui/dialogs/mod.rs`.
#[must_use]
pub fn title(cx: &DialogCx<'_>) -> &'static str {
    match cx.operation.phase {
        CleanupPhase::Idle | CleanupPhase::Building | CleanupPhase::Review => "Review cleanup",
        CleanupPhase::Running | CleanupPhase::Stopping => "Moving files to Trash",
        CleanupPhase::Stopped => "Cleanup stopped",
        CleanupPhase::Finished => "Cleanup finished",
    }
}

// Column widths and row heights of frame `16:886`.
const COLUMNS: [f32; 6] = [172.0, 164.0, 88.0, 204.0, 64.0, 128.0];
/// The narrowest each column shrinks to before the table scrolls sideways
/// (see [`column_widths`]). Only File, Folder and Kept copy may shrink.
const MIN_COLUMNS: [f32; 6] = [96.0, 88.0, 88.0, 112.0, 64.0, 128.0];
const COLUMN_GAP: f32 = 12.0;
const CELL_PAD: f32 = 8.0;
const HEADER_HEIGHT: f32 = 28.0;
const ROW_HEIGHT: f32 = 51.0;
/// Rows shown before the table scrolls. The table is virtualized: only the
/// visible rows are laid out, however large the plan (spec §11.6).
const VISIBLE_ROWS: f32 = 6.0;
const STOP_RULE: &str = "If a file or its kept copy changed since the scan, cleanup stops before \
                         it and asks you to confirm the rest.";
const NO_PERMANENT_DELETE: &str =
    "No permanent-delete fallback. Space is not reclaimed until Trash is emptied.";
const NEVER_DELETES: &str =
    "Fuzzy DeDupe never deletes permanently. This file will be left in place.";

/// Draws the body of the current phase; [`footer`] draws its actions.
/// Never closes the popup itself.
pub fn show(ui: &mut Ui, draft: &mut Draft, cx: &mut DialogCx<'_>) -> DialogResponse {
    // The body records every run report it sees, so the footer, drawn after
    // it in the same frame, counts the same rows (see the module docs).
    if let Some(report) = &cx.operation.report
        && !draft.runs.iter().any(|seen| seen.plan == report.plan)
    {
        draft.runs.push(Arc::clone(report));
    }
    let view = View::new(ui, draft, cx);
    ui.spacing_mut().item_spacing.y = 12.0;
    match view.op.phase {
        CleanupPhase::Idle | CleanupPhase::Building => view.preparing(ui),
        CleanupPhase::Review => view.review(ui),
        CleanupPhase::Running | CleanupPhase::Stopping => view.running(ui),
        CleanupPhase::Stopped | CleanupPhase::Finished => view.ended(ui, draft),
    }
    view.send(cx);
    DialogResponse::KeepOpen
}

/// The current phase's note and actions, in the footer slot the dialog
/// host pins below the body. Returns [`DialogResponse::Close`] when Cancel
/// or Close closed the popup.
pub fn footer(ui: &mut Ui, draft: &mut Draft, cx: &mut DialogCx<'_>) -> DialogResponse {
    let view = View::new(ui, draft, cx);
    let response = view.footer(ui, draft);
    view.send(cx);
    response
}

/// One frame's read of the state, and the intents its controls sent, which
/// [`show`] and [`footer`] hand to the dialog context once drawing is done.
struct View<'a> {
    op: &'a OperationState,
    scan: &'a ScanState,
    queue: &'a QueueState,
    palette: &'static Palette,
    /// Every item seen while the popup is open, newest state per file.
    rows: Vec<PlanItem>,
    out: RefCell<Vec<UiIntent>>,
}

/// The table rows, each newer state replacing the older one for its file
/// while keeping the order in which files first appeared.
///
/// Which source is newest depends on the phase. While a plan is reviewed or
/// runs, the live items are newer than any earlier report. Once a run has
/// ended, its report is the final word: the live items then hold either the
/// revised plan (the same states) or, after a finished run, whatever the
/// last `ItemChanged` left, which a report must never be overridden by.
fn merged_rows(op: &OperationState, runs: &[Arc<RunReport>]) -> Vec<PlanItem> {
    let reports = runs.iter().flat_map(|r| r.items.iter());
    let ended = matches!(op.phase, CleanupPhase::Stopped | CleanupPhase::Finished);
    let newer: Vec<&PlanItem> = if ended {
        op.items.iter().chain(reports).collect()
    } else {
        reports.chain(&op.items).collect()
    };
    let mut rows: Vec<PlanItem> = Vec::new();
    for item in newer {
        match rows.iter_mut().find(|row| row.target == item.target) {
            Some(row) => *row = *item,
            None => rows.push(*item),
        }
    }
    rows
}

fn plural(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

/// A logical size in binary units with one truncated decimal: "12.8 KiB".
/// Truncation, so three targets that each read 12.8, 12.8 and 22.4 KiB also
/// total "48.0 KiB" as the frame shows.
fn size_label(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KiB", "MiB", "GiB", "TiB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut scale: u64 = 1024;
    let mut unit = 0;
    while unit + 1 < UNITS.len() && bytes / scale >= 1024 {
        scale *= 1024;
        unit += 1;
    }
    let tenths = u128::from(bytes) * 10 / u128::from(scale);
    format!("{}.{} {}", tenths / 10, tenths % 10, UNITS[unit])
}

/// Why an item left the plan or cannot run, in words.
const fn reason_text(reason: PlanItemReason) -> &'static str {
    match reason {
        PlanItemReason::NoTrash => "its volume has no usable Trash",
        PlanItemReason::TargetChanged => "the file changed since review",
        PlanItemReason::CounterpartChanged => "its kept copy changed since review",
        PlanItemReason::EvidenceChanged => "its review evidence changed",
        PlanItemReason::TrashMoveFailed => "the move to Trash failed",
        PlanItemReason::OutcomeUncertain => "the outcome of the move was uncertain",
    }
}

fn status_color(palette: &Palette, status: DialogStatus) -> Color32 {
    palette.color(match status {
        DialogStatus::Moved => Token::Green,
        DialogStatus::Moving => Token::Primary,
        DialogStatus::Failed | DialogStatus::StoppedChanged => Token::Red,
        DialogStatus::BlockedNoTrash | DialogStatus::NeedsCheck => Token::Amber,
        DialogStatus::Ready | DialogStatus::Waiting => Token::Muted,
    })
}

impl<'a> View<'a> {
    fn new(ui: &Ui, draft: &Draft, cx: &DialogCx<'a>) -> Self {
        let op: &OperationState = cx.operation;
        Self {
            op,
            scan: cx.scan,
            queue: cx.queue,
            palette: theme::palette(dialog_host::mode_of(ui.ctx())),
            rows: merged_rows(op, &draft.runs),
            out: RefCell::new(Vec::new()),
        }
    }

    /// Hands the intents this frame's controls sent to the dialog context.
    fn send(self, cx: &mut DialogCx<'_>) {
        for intent in self.out.into_inner() {
            cx.intents.push(intent);
        }
    }
}

impl View<'_> {
    fn member(&self, file: FileId) -> Option<&Member> {
        self.scan.results.as_ref()?.group_of(file)?.member(file)
    }

    fn name(&self, file: FileId) -> String {
        self.member(file)
            .map_or_else(|| format!("File {}", file.get()), |m| m.name.clone())
    }

    /// The full display path, which names a file unambiguously where two
    /// targets share a file name.
    fn path(&self, file: FileId) -> String {
        self.member(file)
            .map_or_else(|| self.name(file), |m| m.display_path.clone())
    }

    /// The folder as the cleanup frames print it, with a trailing slash.
    fn folder(&self, file: FileId) -> String {
        self.member(file).map_or_else(String::new, |m| {
            if m.folder.ends_with('/') {
                m.folder.clone()
            } else {
                format!("{}/", m.folder)
            }
        })
    }

    fn protected(&self, file: FileId) -> bool {
        self.member(file).is_some_and(|m| m.folder_protected)
            || self
                .queue
                .view
                .as_ref()
                .is_some_and(|view| view.is_protected(file))
    }

    /// The direct match between a target and the kept copy the plan names.
    /// Read from the group's edge, not the member's stored link: a later
    /// Make reference changes the counterpart, and the stored link then
    /// measures against a file this plan does not keep.
    fn measurement(&self, item: &PlanItem) -> Option<Measurement> {
        self.scan
            .results
            .as_ref()?
            .group_of(item.target)?
            .edge_between(item.target, item.counterpart)?
            .measurement_from(item.target)
    }

    fn match_label(&self, item: &PlanItem) -> String {
        match self.measurement(item) {
            Some(Measurement::Identical(_)) => "Identical".to_owned(),
            Some(Measurement::SameText) => "Same text".to_owned(),
            Some(Measurement::Similar(p)) => {
                format!("{}.{}% similar", p.get() / 10, p.get() % 10)
            }
            Some(Measurement::Containment { coverage, .. }) => {
                format!("{}.{}% contained", coverage.get() / 10, coverage.get() % 10)
            }
            None => "—".to_owned(),
        }
    }

    fn text(&self, text: impl Into<String>, style: CompactStyle, token: Token) -> RichText {
        RichText::new(text)
            .font(style.font_id())
            .color(self.palette.color(token))
    }

    fn supporting(&self, ui: &mut Ui, text: impl Into<String>) {
        ui.label(self.text(text, CompactStyle::Supporting, Token::Muted));
    }

    fn summary(&self, ui: &mut Ui, text: impl Into<String>) {
        ui.label(self.text(text, CompactStyle::Table, Token::Muted));
    }

    fn count(&self, status: PlanItemStatus) -> usize {
        self.rows.iter().filter(|r| r.status == status).count()
    }

    fn preparing(&self, ui: &mut Ui) {
        if self.scan.scan.is_none() {
            self.summary(ui, "No scan results are loaded, so nothing is queued.");
        } else {
            self.summary(ui, "Building the cleanup plan…");
        }
    }

    fn review(&self, ui: &mut Ui) {
        let op = self.op;
        let blocked = op
            .items
            .iter()
            .filter(|i| i.status == PlanItemStatus::Blocked)
            .count();
        let mut parts = vec![plural(op.items.len(), "file queued", "files queued")];
        if blocked > 0 {
            parts.push(format!("{blocked} cannot be moved"));
        } else {
            let identical = op
                .items
                .iter()
                .all(|i| matches!(self.measurement(i), Some(Measurement::Identical(_))));
            if identical && !op.items.is_empty() {
                parts.push("all byte-identical".to_owned());
            }
            let mut kept: Vec<FileId> = op.items.iter().map(|i| i.counterpart).collect();
            kept.sort_unstable();
            kept.dedup();
            parts.push(plural(kept.len(), "kept copy", "kept copies"));
        }
        self.summary(ui, parts.join(" · "));
        self.table(ui);
        self.blocked_notices(ui);
        self.supporting(ui, STOP_RULE);
        self.destination(ui);
    }

    /// "Destination: system Trash · N logical size" for what the
    /// confirmation would move: blocked items stay in place and do not count.
    fn destination(&self, ui: &mut Ui) {
        let bytes: u64 = self
            .op
            .items
            .iter()
            .filter(|i| i.status == PlanItemStatus::Planned)
            .map(|i| i.size_bytes)
            .sum();
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            self.supporting(
                ui,
                format!(
                    "Destination: system Trash · {} logical size",
                    size_label(bytes)
                ),
            );
            self.supporting(ui, NO_PERMANENT_DELETE);
        });
    }

    /// Each blocked target, named, and what happens to it (frame `27:1678`).
    fn blocked_notices(&self, ui: &mut Ui) {
        let blocked: Vec<&PlanItem> = self
            .rows
            .iter()
            .filter(|r| r.status == PlanItemStatus::Blocked)
            .collect();
        if blocked.is_empty() {
            return;
        }
        self.notice(ui, |ui| {
            for item in blocked {
                let folder = self.folder(item.target);
                ui.label(self.text(
                    format!("{folder} is on a volume without a Trash folder."),
                    CompactStyle::Supporting,
                    Token::Ink,
                ));
            }
            ui.label(self.text(NEVER_DELETES, CompactStyle::Supporting, Token::Ink));
        });
    }

    fn notice(&self, ui: &mut Ui, body: impl FnOnce(&mut Ui)) {
        egui::Frame::new()
            .fill(self.palette.color(Token::AmberSoft))
            .corner_radius(theme::CORNER_RADIUS)
            .inner_margin(Vec2::new(12.0, 8.0))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = 2.0;
                body(ui);
            });
    }

    fn running(&self, ui: &mut Ui) {
        let live: Vec<&PlanItem> = self
            .op
            .items
            .iter()
            .filter(|i| i.status != PlanItemStatus::Blocked)
            .collect();
        let moved = live
            .iter()
            .filter(|i| i.status == PlanItemStatus::Succeeded)
            .count();
        self.summary(
            ui,
            format!("{moved} of {} moved", plural(live.len(), "file", "files")),
        );
        let fraction = if live.is_empty() {
            0.0
        } else {
            f32::from(u16::try_from(moved).unwrap_or(u16::MAX))
                / f32::from(u16::try_from(live.len()).unwrap_or(u16::MAX))
        };
        self.progress(ui, fraction);
        self.table(ui);
        self.supporting(
            ui,
            "Each file and its kept copy are checked again just before the move.",
        );
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            self.supporting(ui, "Destination: system Trash");
            self.supporting(ui, "Stopping leaves the remaining files in place.");
        });
    }

    fn progress(&self, ui: &mut Ui, fraction: f32) {
        let (rect, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 6.0), egui::Sense::hover());
        let painter = ui.painter();
        painter.rect_filled(rect, 3.0, self.palette.color(Token::Line));
        let mut done = rect;
        done.set_width(rect.width() * fraction.clamp(0.0, 1.0));
        painter.rect_filled(done, 3.0, self.palette.color(Token::Primary));
    }

    /// Stopped and finished: the summary counts, the table or the journal,
    /// and each file that left the plan named with its reason.
    fn ended(&self, ui: &mut Ui, draft: &Draft) {
        let moved = self.count(PlanItemStatus::Succeeded);
        // A Trash failure counts as failed, never as moved or stopped.
        let failed = self.count(PlanItemStatus::Failed);
        let stopped = self.count(PlanItemStatus::Stopped);
        let blocked = self.count(PlanItemStatus::Blocked);
        let left = self.count(PlanItemStatus::Planned);
        let mut parts = vec![format!("{moved} moved"), format!("{failed} failed")];
        if self.op.phase == CleanupPhase::Stopped {
            if stopped > 0 {
                parts.push(format!(
                    "stopped before {}",
                    plural(stopped, "changed file", "changed files")
                ));
            }
            if left > 0 {
                parts.push(format!("{left} left to confirm"));
            }
            if blocked > 0 {
                parts.push(format!("{blocked} blocked"));
            }
        }
        self.summary(ui, parts.join(" · "));
        if draft.journal {
            self.journal(ui, &draft.runs);
        } else {
            self.table(ui);
        }
        self.outcome_notices(ui, &draft.runs);
        if self.op.phase == CleanupPhase::Stopped {
            self.blocked_notices(ui);
            self.supporting(ui, STOP_RULE);
            self.destination(ui);
        } else {
            let total = self.rows.len() - blocked;
            if failed == 0 && stopped == 0 && left == 0 && moved > 0 {
                self.supporting(
                    ui,
                    format!(
                        "All {} moved to the system Trash.",
                        plural(moved, "file was", "files were")
                    ),
                );
            } else {
                self.supporting(
                    ui,
                    format!(
                        "{moved} of {} moved to the system Trash.",
                        plural(total, "file was", "files were")
                    ),
                );
            }
            self.supporting(ui, "Moved files can be restored from the system Trash.");
        }
    }

    /// Failed and changed files with their reasons, and every uncertain
    /// outcome with what reconciliation found, from every run's journal:
    /// the uncertain state itself never survives to a run's end (T8 settles
    /// it before reporting), so only the journal still shows it.
    fn outcome_notices(&self, ui: &mut Ui, runs: &[Arc<RunReport>]) {
        let mut lines = Vec::new();
        for row in &self.rows {
            let reason = row.reason.map_or("", reason_text);
            match row.status {
                PlanItemStatus::Failed => lines.push(format!(
                    "{}: Failed, {reason}. It was taken out of the plan.",
                    self.path(row.target)
                )),
                PlanItemStatus::Stopped => lines.push(format!(
                    "{}: Changed since review, {reason}. It was taken out of the plan.",
                    self.path(row.target)
                )),
                _ => {}
            }
        }
        for run in runs {
            for (at, entry) in run.journal.iter().enumerate() {
                if entry.status != PlanItemStatus::NeedsReconciliation {
                    continue;
                }
                let found = run.journal[at + 1..]
                    .iter()
                    .find(|later| later.target == entry.target)
                    .map(|later| later.status);
                let outcome = match found {
                    Some(PlanItemStatus::Succeeded) => "it was found moved to Trash",
                    Some(PlanItemStatus::Planned) => {
                        "it was found not moved and returned to the plan"
                    }
                    _ => "it still needs a check",
                };
                lines.push(format!(
                    "{}: Needs check, {}; {outcome}.",
                    self.path(entry.target),
                    reason_text(PlanItemReason::OutcomeUncertain)
                ));
            }
        }
        if lines.is_empty() {
            return;
        }
        self.notice(ui, |ui| {
            for line in lines {
                ui.label(self.text(line, CompactStyle::Supporting, Token::Ink));
            }
        });
    }

    /// Show journal: every change of every run seen, in order.
    fn journal(&self, ui: &mut Ui, runs: &[Arc<RunReport>]) {
        egui::Frame::new()
            .fill(self.palette.color(Token::Code))
            .stroke(Stroke::new(
                theme::BORDER_WIDTH,
                self.palette.color(Token::Line),
            ))
            .corner_radius(theme::CORNER_RADIUS)
            .inner_margin(CELL_PAD)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                egui::ScrollArea::vertical()
                    .id_salt("fd.cleanup.journal")
                    .max_height(ROW_HEIGHT * VISIBLE_ROWS)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        for (index, run) in runs.iter().enumerate() {
                            ui.label(self.text(
                                format!("Run {}", index + 1),
                                CompactStyle::GroupEmphasis,
                                Token::Ink,
                            ));
                            for entry in &run.journal {
                                let status = DialogStatus::of(entry.status);
                                let mut line =
                                    format!("{} — {}", self.path(entry.target), status.label());
                                if let Some(reason) = entry.reason {
                                    line.push_str(" — ");
                                    line.push_str(reason_text(reason));
                                }
                                ui.label(self.text(line, CompactStyle::PathDetail, Token::Ink));
                            }
                        }
                    });
            });
    }

    /// The File, Folder, Match, Kept copy, Size and Status table.
    ///
    /// The columns keep the frame's widths while they fit; under zoom or a
    /// narrow window the flexible ones shrink (see [`column_widths`]), and
    /// past their minimums the whole table, header included, scrolls
    /// sideways inside its own frame (spec §11.6, "grow or internally
    /// scroll"), so no column is ever drawn past the popup body.
    fn table(&self, ui: &mut Ui) {
        egui::Frame::new()
            .fill(self.palette.color(Token::Code))
            .stroke(Stroke::new(
                theme::BORDER_WIDTH,
                self.palette.color(Token::Line),
            ))
            .corner_radius(theme::CORNER_RADIUS)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                let viewport = ui.available_width();
                let widths = column_widths(viewport - fixed_span());
                let width = viewport.max(widths.iter().sum::<f32>() + fixed_span());
                let revealed = RefCell::new(None);
                egui::ScrollArea::horizontal()
                    .id_salt("fd.cleanup.table.x")
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.set_width(width);
                        let header = row_rect(ui, width, HEADER_HEIGHT, egui::Sense::hover()).rect;
                        for (index, title) in
                            ["File", "Folder", "Match", "Kept copy", "Size", "Status"]
                                .into_iter()
                                .enumerate()
                        {
                            cell(
                                ui,
                                cell_rect(header, &widths, index),
                                [self.text(title, CompactStyle::GroupEmphasis, Token::Ink)],
                            );
                        }
                        egui::ScrollArea::vertical()
                            .id_salt("fd.cleanup.table")
                            .max_height(ROW_HEIGHT * VISIBLE_ROWS)
                            .auto_shrink([false, true])
                            .show_rows(ui, ROW_HEIGHT, self.rows.len(), |ui, range| {
                                let first = range.start;
                                let top = ui.cursor().top();
                                let pending = ui.data(|d| d.get_temp::<usize>(pending_row_id()));
                                let mut focused = None;
                                for (index, item) in range.clone().zip(&self.rows[range]) {
                                    let response = self.row(ui, width, &widths, item);
                                    let arrived = pending == Some(index);
                                    if arrived {
                                        response.request_focus();
                                        ui.data_mut(|d| d.remove::<usize>(pending_row_id()));
                                    }
                                    if response.gained_focus() || arrived {
                                        // Consumed by this vertical area, the
                                        // innermost one still open, so a row
                                        // Tab or a row key reaches at the
                                        // table's edge scrolls into view.
                                        response.scroll_to_me(None);
                                        revealed
                                            .replace(Some(response.rect.intersect(ui.clip_rect())));
                                    }
                                    if response.has_focus() {
                                        focused = Some((index, response.id));
                                    }
                                }
                                if let Some((current, id)) = focused {
                                    // A page is the table's own viewport,
                                    // not what the popup body leaves of it:
                                    // at 200% on 1024 × 600 the body shows
                                    // about one row, and Page Down would be
                                    // no more than Down. The body scrolls
                                    // to the target row instead.
                                    let page = rows_in(ROW_HEIGHT * VISIBLE_ROWS);
                                    if let Some(to) = self.row_keys(ui, id, current, page) {
                                        // The target may lie outside the drawn
                                        // range, so it cannot take focus yet:
                                        // scroll its slot into view now, without
                                        // animation, and focus it on the next
                                        // frame, when `show_rows` draws it.
                                        ui.data_mut(|d| d.insert_temp(pending_row_id(), to));
                                        let y = if to >= first {
                                            top + rows_span(to - first)
                                        } else {
                                            top - rows_span(first - to)
                                        };
                                        let slot = Rect::from_min_size(
                                            egui::pos2(ui.min_rect().left(), y),
                                            Vec2::new(width, ROW_HEIGHT),
                                        );
                                        ui.scroll_to_rect_animation(
                                            slot,
                                            None,
                                            egui::style::ScrollAnimation::none(),
                                        );
                                        ui.ctx().request_repaint();
                                    }
                                }
                            });
                    });
                // ORDER: after both table areas have closed, so the popup
                // body's scroll area is the innermost one left and takes
                // this target. egui hands a target to the innermost open
                // area alone, on both axes, and the body's own reveal
                // (`dialog_host::reveal_focused`) skips a row because the
                // table's clips cut it; without this, a row focused by Tab
                // could sit below the body's visible clip at 200% zoom.
                if let Some(rect) = revealed.into_inner() {
                    ui.scroll_to_rect(rect, None);
                }
            });
    }

    /// One plan item's row: focusable, so the keyboard reaches each file's
    /// full paths, which the truncated cells cannot show (spec line 1125:
    /// hover alone is insufficient). Focus, or the pointer over the row,
    /// opens a tooltip naming the file's and its kept copy's full display
    /// paths. Tab and Shift+Tab move between the drawn rows; the row keys
    /// reach every row (see [`View::row_keys`]).
    fn row(&self, ui: &mut Ui, width: f32, widths: &[f32; 6], item: &PlanItem) -> egui::Response {
        let status = DialogStatus::of(item.status);
        // The row's id is its file's, not egui's positional auto id: the
        // virtualised table draws a different item first as it scrolls, and
        // a positional id would leave focus on the slot, not the file.
        let rect = row_rect(ui, width, ROW_HEIGHT, egui::Sense::hover()).rect;
        let response = ui.interact(
            rect,
            egui::Id::unique(("fd.cleanup.row", item.target)),
            egui::Sense::focusable_noninteractive(),
        );
        let target = self.path(item.target);
        let kept = self.path(item.counterpart);
        let label = format!("{target}, kept copy {kept}");
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Row, true, &label));
        let painter = ui.painter();
        painter.rect_filled(rect, 0.0, self.palette.color(Token::Surface));
        painter.hline(
            rect.x_range(),
            rect.top(),
            Stroke::new(theme::BORDER_WIDTH, self.palette.color(Token::Line)),
        );
        if response.has_focus() {
            painter.rect_stroke(
                rect,
                0.0,
                Stroke::new(theme::BORDER_WIDTH, self.palette.color(Token::Focus)),
                egui::StrokeKind::Inside,
            );
        }

        let mut detail = self.folder(item.counterpart);
        if self.protected(item.counterpart) {
            detail.push_str(" · protected");
        }
        let state_style = if matches!(status, DialogStatus::Ready | DialogStatus::Waiting) {
            CompactStyle::Table
        } else {
            CompactStyle::ControlLabel
        };
        let at = |index| cell_rect(rect, widths, index);
        cell(
            ui,
            at(0),
            [self.text(self.name(item.target), CompactStyle::Table, Token::Ink)],
        );
        cell(
            ui,
            at(1),
            [self.text(self.folder(item.target), CompactStyle::Table, Token::Muted)],
        );
        cell(
            ui,
            at(2),
            [self.text(self.match_label(item), CompactStyle::Table, Token::Ink)],
        );
        cell(
            ui,
            at(3),
            [
                self.text(self.name(item.counterpart), CompactStyle::Table, Token::Ink),
                self.text(detail, CompactStyle::Supporting, Token::Muted),
            ],
        );
        cell(
            ui,
            at(4),
            [self.text(
                size_label(item.size_bytes),
                CompactStyle::Table,
                Token::Muted,
            )],
        );
        cell(
            ui,
            at(5),
            [RichText::new(status.label())
                .font(state_style.font_id())
                .color(status_color(self.palette, status))],
        );

        // The pointer test reads the row's rect, not `response.hovered()`:
        // the cell labels are drawn over the row and take its hover.
        if response.has_focus() || ui.rect_contains_pointer(rect) {
            egui::Tooltip::for_widget(&response).show(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(self.text(target, CompactStyle::PathDetail, Token::Ink));
                ui.label(self.text("Kept copy", CompactStyle::Supporting, Token::Muted));
                ui.label(self.text(kept, CompactStyle::PathDetail, Token::Ink));
            });
        }
        response
    }

    /// Reads the row-focus keys while row `current` of the table has focus
    /// (spec §11.10, the Findings table's "Move row focus" set): Up and
    /// Down, Page Up and Page Down by `page` rows, Home and End. Returns
    /// the row to move to, if any.
    ///
    /// Tab is the only way into the table, and Tab from a row still leaves
    /// it, so this is what reaches rows the virtualised table has not drawn
    /// (NFR-005); Tab alone stops at the drawn ones. Esc and Enter are
    /// consumed by the dialog host before the body draws. The keys are
    /// consumed here, and the workspace behind the modal reads none of them
    /// while a popup is open (`shell.rs`, `input_enabled`).
    fn row_keys(&self, ui: &Ui, id: egui::Id, current: usize, page: usize) -> Option<usize> {
        // Arrow keys would otherwise also move egui's own focus to the
        // geometrically nearest widget, racing this navigation.
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(
                id,
                egui::EventFilter {
                    horizontal_arrows: false,
                    vertical_arrows: true,
                    tab: false,
                    escape: false,
                },
            );
        });
        let last = self.rows.len().saturating_sub(1);
        let pressed = |key| ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, key));
        let mut target = None;
        if pressed(egui::Key::ArrowDown) {
            target = Some((current + 1).min(last));
        }
        if pressed(egui::Key::ArrowUp) {
            target = Some(current.saturating_sub(1));
        }
        if pressed(egui::Key::PageDown) {
            target = Some((current + page).min(last));
        }
        if pressed(egui::Key::PageUp) {
            target = Some(current.saturating_sub(page));
        }
        if pressed(egui::Key::Home) {
            target = Some(0);
        }
        if pressed(egui::Key::End) {
            target = Some(last);
        }
        target.filter(|&to| to != current)
    }

    fn footer(&self, ui: &mut Ui, draft: &mut Draft) -> DialogResponse {
        let op = self.op;
        let moved_so_far = |what: &str| {
            let moved = self.count(PlanItemStatus::Succeeded);
            format!("{} {what}", plural(moved, "file", "files"))
        };
        let note = match op.phase {
            CleanupPhase::Idle | CleanupPhase::Building | CleanupPhase::Review => {
                "Nothing has been removed.".to_owned()
            }
            CleanupPhase::Running | CleanupPhase::Stopping => {
                let moved = op
                    .items
                    .iter()
                    .filter(|i| i.status == PlanItemStatus::Succeeded)
                    .count();
                format!("{} moved so far.", plural(moved, "file", "files"))
            }
            CleanupPhase::Stopped => moved_so_far("moved. Confirm the rest to continue."),
            CleanupPhase::Finished => moved_so_far("moved to Trash."),
        };
        let mut response = DialogResponse::KeepOpen;
        // Both layouts centre on the cross axis and span the whole 30-unit
        // footer slot, so the note and the buttons share one centre line.
        // egui aligns a button's label by its Ui's layout, not the button's
        // own height: `Align::Min` here draws each label at the top of its
        // button. `ui.horizontal` is avoided too: its row starts at
        // `interact_size.y` (18) high, and the note would centre on that.
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            self.supporting(ui, note);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| match op.phase {
                CleanupPhase::Idle | CleanupPhase::Building | CleanupPhase::Review => {
                    self.confirm_button(ui, draft);
                    let cancel = ui.add(self.secondary_button("Cancel", true));
                    if draft.focus_cancel
                        && matches!(op.phase, CleanupPhase::Review | CleanupPhase::Idle)
                    {
                        cancel.request_focus();
                        draft.focus_cancel = false;
                    }
                    if cancel.clicked() {
                        response = DialogResponse::Close;
                    }
                }
                CleanupPhase::Running | CleanupPhase::Stopping => {
                    self.confirm_button(ui, draft);
                    let running = op.phase == CleanupPhase::Running;
                    let stop = ui.add_enabled(running, self.secondary_button("Stop", false));
                    if stop.clicked()
                        && let Some(revision) = op.revision
                    {
                        self.cx_push(UiIntent::Submit(EngineRequest::StopCleanup(revision)));
                    }
                }
                CleanupPhase::Stopped => {
                    self.confirm_button(ui, draft);
                    if ui.add(self.secondary_button("Close", false)).clicked() {
                        response = DialogResponse::Close;
                    }
                    self.journal_toggle(ui, draft);
                }
                CleanupPhase::Finished => {
                    let close = egui::Button::new(self.text(
                        "Close",
                        CompactStyle::ControlLabel,
                        Token::White,
                    ))
                    .fill(self.palette.color(Token::Primary))
                    .min_size(Vec2::new(0.0, 30.0));
                    if ui.add(close).clicked() {
                        response = DialogResponse::Close;
                    }
                    self.journal_toggle(ui, draft);
                }
            });
        });
        response
    }

    fn journal_toggle(&self, ui: &mut Ui, draft: &mut Draft) {
        let label = if draft.journal {
            "Hide journal"
        } else {
            "Show journal"
        };
        if ui.add(self.secondary_button(label, false)).clicked() {
            draft.journal = !draft.journal;
        }
    }

    fn secondary_button(&self, label: &str, emphasis: bool) -> egui::Button<'static> {
        let border = if emphasis {
            Token::Primary
        } else {
            Token::Line
        };
        egui::Button::new(self.text(label, CompactStyle::ControlLabel, Token::Ink))
            .fill(self.palette.color(Token::Surface))
            .stroke(Stroke::new(
                if emphasis { 2.0 } else { theme::BORDER_WIDTH },
                self.palette.color(border),
            ))
            .min_size(Vec2::new(0.0, 30.0))
    }

    /// "Move N files to Trash": enabled only for an unconfirmed plan with
    /// something to move that this popup has not already submitted. While a
    /// run is in progress it reads "Moving…" and stays disabled.
    fn confirm_button(&self, ui: &mut Ui, draft: &mut Draft) {
        let op = self.op;
        let count = op.confirmed_count();
        let running = matches!(op.phase, CleanupPhase::Running | CleanupPhase::Stopping);
        let label = if running {
            "Moving…".to_owned()
        } else {
            format!("Move {} to Trash", plural(count, "file", "files"))
        };
        let enabled = matches!(op.phase, CleanupPhase::Review | CleanupPhase::Stopped)
            && !op.authorized
            && count > 0
            && op.revision.is_some()
            && draft.submitted != op.revision;
        let button = egui::Button::new(self.text(label, CompactStyle::ControlLabel, Token::White))
            .fill(self.palette.color(Token::Red))
            .min_size(Vec2::new(0.0, 30.0));
        if ui.add_enabled(enabled, button).clicked()
            && enabled
            && let Some(plan) = op.revision
        {
            draft.submitted = Some(plan);
            self.cx_push(UiIntent::Submit(EngineRequest::ExecuteApprovedPlan(
                ExecuteApprovedPlan { plan },
            )));
        }
    }

    fn cx_push(&self, intent: UiIntent) {
        self.out.borrow_mut().push(intent);
    }
}

/// The gaps and the row's inner padding: the part of the table's width
/// that is not column.
fn fixed_span() -> f32 {
    COLUMNS[1..].iter().map(|_| COLUMN_GAP).sum::<f32>() + 2.0 * CELL_PAD
}

/// The column widths for `room` units of column space (P4, spec §11.6).
///
/// Every column keeps its frame width while they all fit. Below that each
/// one shrinks from its frame width toward its [`MIN_COLUMNS`] width by the
/// same fraction, so only the flexible columns (File, Folder, Kept copy)
/// move; past the minimums the widths stay there and the table scrolls
/// sideways. Rejected: scaling all six columns alike. Match, Size and
/// Status hold short fixed words ("Identical", "12.8 KiB", "Blocked: no
/// Trash") that become unreadable once truncated, where a truncated name or
/// folder is still recognisable and the focused row shows it in full.
fn column_widths(room: f32) -> [f32; 6] {
    let natural: f32 = COLUMNS.iter().sum();
    let least: f32 = MIN_COLUMNS.iter().sum();
    let t = ((room - least) / (natural - least)).clamp(0.0, 1.0);
    std::array::from_fn(|i| MIN_COLUMNS[i] + (COLUMNS[i] - MIN_COLUMNS[i]) * t)
}

/// Allocates one full-width row or header band of exactly `height`.
///
/// Its cells are drawn afterwards into child Uis of the returned rect
/// (see [`cell`]), never through `scope_builder` or `allocate_ui_*` on this
/// Ui. Those move the parent's cursor to the bottom of what the child
/// actually drew (egui's `advance_after_rects` sets `cursor.min.y`; it does
/// not take a maximum), so after this allocation they would rewind the
/// cursor to the bottom of the row's text: rows shrink to their text
/// height (about 33 units, T24 triage), the header band to one line, each
/// row draws over the rest of the previous one's fill, and `show_rows`,
/// which assumes `ROW_HEIGHT` per row, leaves blank space under the last.
fn row_rect(ui: &mut Ui, width: f32, height: f32, sense: egui::Sense) -> egui::Response {
    ui.allocate_exact_size(Vec2::new(width, height), sense).1
}

/// Where the table keeps the row that a row key moved to and that takes
/// focus once drawn (see [`View::row_keys`]). egui temp data, not the
/// draft: it lives across the frames of one move and nothing else.
fn pending_row_id() -> egui::Id {
    egui::Id::unique("fd.cleanup.row.pending")
}

/// The rows wholly visible in `height`, at least one: a page for Page Up
/// and Page Down.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a finite, non-negative row count, floored"
)]
fn rows_in(height: f32) -> usize {
    (height / ROW_HEIGHT).floor().max(1.0) as usize
}

/// The height of `rows` table rows.
#[expect(
    clippy::cast_precision_loss,
    reason = "row counts stay far below f32's 2^24 exact-integer range"
)]
fn rows_span(rows: usize) -> f32 {
    rows as f32 * ROW_HEIGHT
}

/// Column `index`'s rect within `row`, inside the row's padding.
fn cell_rect(row: Rect, widths: &[f32; 6], index: usize) -> Rect {
    let left = row.left() + CELL_PAD + widths[..index].iter().map(|w| w + COLUMN_GAP).sum::<f32>();
    Rect::from_min_size(
        egui::pos2(left, row.top()),
        Vec2::new(widths[index], row.height()),
    )
}

/// One table cell: its lines stacked, truncated rather than wrapped into
/// the next column, and centred as a block in the cell's height.
///
/// The lines are laid out before anything is added, so the block's height
/// is known and the offset to centre it can be added first. Rejected: a
/// `top_down` layout `with_main_align(Align::Center)`. egui reads a
/// layout's main alignment only to align content within a single widget,
/// such as a button's text; it never places child widgets, so the lines
/// stacked from the cell's top. `egui::Aligned` would centre by measuring
/// on a discarded pass, which a virtualized table pays on every scroll.
fn cell<const N: usize>(ui: &mut Ui, rect: Rect, lines: [RichText; N]) {
    let mut ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::top_down(Align::Min)),
    );
    ui.spacing_mut().item_spacing.y = 0.0;
    let galleys = lines.map(|line| {
        egui::WidgetText::from(line).into_galley(
            &ui,
            Some(egui::TextWrapMode::Truncate),
            rect.width(),
            egui::FontSelection::Default,
        )
    });
    let height: f32 = galleys.iter().map(|g| g.size().y).sum();
    ui.add_space(((rect.height() - height) / 2.0).max(0.0));
    for galley in galleys {
        ui.add(egui::Label::new(galley));
    }
}
