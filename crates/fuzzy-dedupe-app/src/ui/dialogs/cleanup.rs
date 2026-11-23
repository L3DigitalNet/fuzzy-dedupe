// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! The Review cleanup popup (spec §11.6, frames `16:886`, `27:1678`,
//! `27:1390`, `35:2416`, `27:1533`): one dialog hosting the review,
//! running, stopped and finished states of the simulated cleanup (plan T18).
//!
//! The dialog host (`ui/widgets/dialog_host.rs`) owns the title row, the close
//! control, Esc, Enter, sizing and focus restoration; this file draws the body
//! and footer inside the 16-unit content inset and supplies the phase's title
//! through [`title`]. Enter only dismisses this popup: its primary action
//! moves files to Trash, which no key may trigger (spec §11.10).
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

use egui::{Align, Color32, Layout, RichText, Stroke, Ui, Vec2};
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

/// Draws the body and footer. Returns [`DialogResponse::Close`] when a
/// footer action closed the popup.
pub fn show(ui: &mut Ui, draft: &mut Draft, cx: &mut DialogCx<'_>) -> DialogResponse {
    let palette = theme::palette(dialog_host::mode_of(ui.ctx()));
    let op: &OperationState = cx.operation;
    if let Some(report) = &op.report
        && !draft.runs.iter().any(|seen| seen.plan == report.plan)
    {
        draft.runs.push(Arc::clone(report));
    }
    let view = View {
        op,
        scan: cx.scan,
        queue: cx.queue,
        palette,
        rows: merged_rows(op, &draft.runs),
        out: RefCell::new(Vec::new()),
    };
    ui.spacing_mut().item_spacing.y = 12.0;

    match op.phase {
        CleanupPhase::Idle | CleanupPhase::Building => view.preparing(ui),
        CleanupPhase::Review => view.review(ui),
        CleanupPhase::Running | CleanupPhase::Stopping => view.running(ui),
        CleanupPhase::Stopped | CleanupPhase::Finished => view.ended(ui, draft),
    }
    ui.add_space(2.0);
    let response = view.footer(ui, draft);
    for intent in view.out.into_inner() {
        cx.intents.push(intent);
    }
    response
}

/// One frame's read of the state, and the intents its controls sent, which
/// [`show`] hands to the dialog context once drawing is done.
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
                ui.vertical(|ui| {
                    self.row_frame(ui, HEADER_HEIGHT, None, |ui| {
                        for (title, width) in
                            ["File", "Folder", "Match", "Kept copy", "Size", "Status"]
                                .into_iter()
                                .zip(COLUMNS)
                        {
                            cell(ui, width, HEADER_HEIGHT, |ui| {
                                ui.label(self.text(title, CompactStyle::GroupEmphasis, Token::Ink));
                            });
                        }
                    });
                    egui::ScrollArea::vertical()
                        .id_salt("fd.cleanup.table")
                        .max_height(ROW_HEIGHT * VISIBLE_ROWS)
                        .auto_shrink([false, true])
                        .show_rows(ui, ROW_HEIGHT, self.rows.len(), |ui, range| {
                            for item in &self.rows[range] {
                                self.row(ui, item);
                            }
                        });
                });
            });
    }

    fn row_frame(&self, ui: &mut Ui, height: f32, fill: Option<Token>, body: impl FnOnce(&mut Ui)) {
        let width = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), egui::Sense::hover());
        if let Some(fill) = fill {
            ui.painter()
                .rect_filled(rect, 0.0, self.palette.color(fill));
            ui.painter().hline(
                rect.x_range(),
                rect.top(),
                Stroke::new(theme::BORDER_WIDTH, self.palette.color(Token::Line)),
            );
        }
        let inner = rect.shrink2(Vec2::new(CELL_PAD, 0.0));
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(inner)
                .layout(Layout::left_to_right(Align::Center)),
            |ui| {
                ui.spacing_mut().item_spacing.x = COLUMN_GAP;
                body(ui);
            },
        );
    }

    fn row(&self, ui: &mut Ui, item: &PlanItem) {
        let status = DialogStatus::of(item.status);
        self.row_frame(ui, ROW_HEIGHT, Some(Token::Surface), |ui| {
            let [file, folder, kind, kept, size, state] = COLUMNS;
            cell(ui, file, ROW_HEIGHT, |ui| {
                ui.label(self.text(self.name(item.target), CompactStyle::Table, Token::Ink));
            });
            cell(ui, folder, ROW_HEIGHT, |ui| {
                ui.label(self.text(self.folder(item.target), CompactStyle::Table, Token::Muted));
            });
            cell(ui, kind, ROW_HEIGHT, |ui| {
                ui.label(self.text(self.match_label(item), CompactStyle::Table, Token::Ink));
            });
            cell(ui, kept, ROW_HEIGHT, |ui| {
                ui.label(self.text(self.name(item.counterpart), CompactStyle::Table, Token::Ink));
                let mut detail = self.folder(item.counterpart);
                if self.protected(item.counterpart) {
                    detail.push_str(" · protected");
                }
                ui.label(self.text(detail, CompactStyle::Supporting, Token::Muted));
            });
            cell(ui, size, ROW_HEIGHT, |ui| {
                ui.label(self.text(
                    size_label(item.size_bytes),
                    CompactStyle::Table,
                    Token::Muted,
                ));
            });
            cell(ui, state, ROW_HEIGHT, |ui| {
                let style = if matches!(status, DialogStatus::Ready | DialogStatus::Waiting) {
                    CompactStyle::Table
                } else {
                    CompactStyle::ControlLabel
                };
                ui.label(
                    RichText::new(status.label())
                        .font(style.font_id())
                        .color(status_color(self.palette, status)),
                );
            });
        });
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
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            self.supporting(ui, note);
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| match op.phase {
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

/// One table cell of fixed width, its content vertically centered and
/// truncated rather than wrapped into the next column.
fn cell(ui: &mut Ui, width: f32, height: f32, body: impl FnOnce(&mut Ui)) {
    ui.allocate_ui_with_layout(
        Vec2::new(width, height),
        Layout::top_down(Align::Min).with_main_align(Align::Center),
        |ui| {
            ui.set_width(width);
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
            body(ui);
        },
    );
}
