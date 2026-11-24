// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! The Review unique text popup (spec §11.6, frames `16:932` and `44:4079`):
//! the deliberate acknowledgment that keeps an unreviewed score from ever
//! creating removal intent (§11.4, C-05 I-5).
//!
//! The dialog host (`ui/widgets/dialog_host.rs`) owns the title row, the close
//! control, Esc, Enter, sizing, focus restoration and the pinned footer slot;
//! this file draws the body and the footer inside the 16-unit content inset.
//! Enter runs Queue for cleanup, which queues nothing without the
//! acknowledgment.
//!
//! The pair is the popup's `UniqueTextSubject`: scan, target, counterpart,
//! both snapshots and the rules revision the review binds to. The unique
//! lines are side B of the C-04 comparison of exactly that pair, the
//! target's side, in Original mode (the Findings Unique measure). The popup
//! shares the one comparison slot with Compare (`ComparisonSlot` explains
//! why), so `open` asks for its pair's comparison whenever the slot holds
//! another key.
//!
//! Queue for cleanup stays enabled. Without the acknowledgment it sends
//! nothing, shows the `44:4079` prompt under the checkbox and moves focus to
//! the checkbox. With it, it sends `AcknowledgeUniqueText` then
//! `RequestQueue` through `UiIntent::Review`, in that order and in the same
//! frame, and closes; the review service's outcome is authoritative (a
//! refusal reaches the status bar through `events::apply_review`). It also
//! sends nothing while the displayed lines are not the current evidence for
//! the pair: before they load, when they cannot be shown, or when a
//! snapshot or the rules changed since the popup opened. The engine binds
//! an acknowledgment to the snapshots current when it runs, so that check
//! is what keeps it bound to the text the user actually saw.

use egui::{Align2, Id, Margin, Sense, Stroke, Vec2};
use fuzzy_dedupe_domain::{CompareMode, FileId, Member, QueueOrigin, ScanResults};
use fuzzy_dedupe_engine::compare::AlignedComparison;
use fuzzy_dedupe_engine::service::{EngineRequest, ReviewCommand};

use crate::presentation::commands::UiIntent;
use crate::presentation::state::{CompareState, DialogSubject, UniqueTextSubject};
use crate::ui::dialogs::queue_identical::{current_reference, footer_row, icon, text};
use crate::ui::dialogs::{DialogCx, DialogResponse};
use crate::ui::theme::{self, CompactStyle, Icon, Palette, Token};
use crate::ui::widgets::dialog_host;

/// The acknowledgment checkbox's label and accessible name.
pub const ACKNOWLEDGMENT: &str = "I reviewed the unique text in this file.";
/// The inline prompt of frame `44:4079`.
pub const PROMPT: &str =
    "Check this box to confirm you reviewed the unique text. Nothing was queued.";

/// The mode the popup measures in: Original, the measure the Findings Unique
/// column and the review service's review-first rule use (§10.1).
const MODE: CompareMode = CompareMode::Original;

/// Height of one unique-line row (`DD/Compact/Diff line`, 22 units).
const ROW_HEIGHT: f32 = 22.0;
/// Rows shown before the list scrolls inside the popup.
const VISIBLE_ROWS: f32 = 5.0;

/// Why an acknowledged Queue for cleanup still sent nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Blocked {
    /// The unique lines have not loaded yet.
    Loading,
    /// The comparison could not be computed, so there is nothing to review.
    Unavailable,
    /// A snapshot or the rules changed after the popup opened.
    Stale,
}

impl Blocked {
    const fn message(self) -> &'static str {
        match self {
            Self::Loading => "The unique lines are still loading. Nothing was queued.",
            Self::Unavailable => {
                "The unique lines cannot be shown, so this file cannot be queued here."
            }
            Self::Stale => {
                "These files changed since this review opened. Nothing was queued; close and \
                 review again."
            }
        }
    }
}

/// The popup's draft, created when it opens and dropped when it closes, so
/// closing never commits it (spec §11.6).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Draft {
    pub acknowledged: bool,
    /// Queue for cleanup was pressed without the acknowledgment; the prompt
    /// shows until the box is checked.
    pub prompt: bool,
    pub blocked: Option<Blocked>,
    /// Move focus to the checkbox when it next draws. Enter runs before the
    /// popup draws (the host consumes it first), so its focus move waits for
    /// the checkbox to exist this frame.
    focus_checkbox: bool,
    /// The checkbox's ID as [`show`] last drew it, for [`footer`]'s focus
    /// move: the footer is drawn after the body, outside it.
    checkbox: Option<Id>,
}

fn subject(cx: &DialogCx<'_>) -> Option<UniqueTextSubject> {
    match cx.subject {
        DialogSubject::UniqueText(subject) => Some(*subject),
        _ => None,
    }
}

/// The draft for a newly opened popup. Asks for the pair's comparison when
/// the shared slot does not already hold, or await, exactly that key.
pub fn open(cx: &mut DialogCx<'_>) -> Draft {
    if let Some(subject) = subject(cx) {
        let key = subject.comparison(MODE);
        if !cx.compare.comparison.matches(&key) {
            cx.intents
                .push(UiIntent::Submit(EngineRequest::LoadComparison(key)));
        }
    }
    Draft::default()
}

/// The pair's comparison as the slot holds it.
enum Evidence<'a> {
    Loading,
    Ready(&'a AlignedComparison),
    Unavailable,
}

/// Takes the compare owner, not the whole context, so a ready result can be
/// held while the popup pushes intents.
fn evidence<'a>(compare: &'a CompareState, subject: &UniqueTextSubject) -> Evidence<'a> {
    let slot = &compare.comparison;
    if !slot.matches(&subject.comparison(MODE)) {
        return Evidence::Loading;
    }
    match (slot.aligned(), slot.unavailable()) {
        (Some(aligned), _) => Evidence::Ready(aligned),
        (None, Some(_)) => Evidence::Unavailable,
        (None, None) => Evidence::Loading,
    }
}

/// Whether both snapshots and the rules are still the ones the subject
/// binds to, per the engine's newest published view. No view fails closed.
fn current(cx: &DialogCx<'_>, subject: &UniqueTextSubject) -> bool {
    cx.queue.snapshot(subject.target) == Some(subject.target_snapshot)
        && cx.queue.snapshot(subject.counterpart) == Some(subject.counterpart_snapshot)
        && cx
            .queue
            .view
            .as_ref()
            .is_some_and(|view| view.rules == subject.rules && view.scan == subject.scan)
}

/// Draws the body; [`footer`] draws Cancel and Queue for cleanup. Returns
/// [`DialogResponse::Close`] when Open comparison closed the popup.
pub fn show(ui: &mut egui::Ui, draft: &mut Draft, cx: &mut DialogCx<'_>) -> DialogResponse {
    let Some(subject) = subject(cx) else {
        return DialogResponse::Close;
    };
    let palette = theme::palette(dialog_host::mode_of(ui.ctx()));
    let mut response = DialogResponse::KeepOpen;
    ui.spacing_mut().item_spacing = Vec2::new(8.0, 12.0);

    let results = cx.scan.results.clone();
    let target = results.as_deref().and_then(|r| member(r, subject.target));
    let counterpart = results
        .as_deref()
        .and_then(|r| member(r, subject.counterpart));
    let against_reference = results.as_deref().is_some_and(|r| {
        r.groups.iter().any(|g| {
            g.member(subject.target).is_some()
                && current_reference(g, cx.queue) == Some(subject.counterpart)
        })
    });

    identities(ui, palette, target, counterpart, against_reference);

    let ready = match evidence(cx.compare, &subject) {
        Evidence::Ready(aligned) => Some(aligned),
        _ => None,
    };
    let counterpart_name = if against_reference {
        "the reference".to_owned()
    } else {
        counterpart.map_or_else(|| "its counterpart".to_owned(), |m| m.name.clone())
    };
    let (headline, consequence) = match evidence(cx.compare, &subject) {
        Evidence::Loading => ("Loading the unique lines…".to_owned(), String::new()),
        Evidence::Unavailable => (
            "The unique lines cannot be shown.".to_owned(),
            "Open the comparison to see why. This file cannot be queued here.".to_owned(),
        ),
        Evidence::Ready(aligned) => headline(aligned.only_in_b.count(), &counterpart_name),
    };
    unmatched(ui, palette, &headline, &consequence);

    if let Some(aligned) = ready
        && !aligned.only_in_b.lines.is_empty()
    {
        unique_lines(ui, palette, aligned);
    }

    ui.horizontal(|ui| {
        let open = egui::Button::new(text(
            "Open comparison",
            CompactStyle::ControlLabel,
            palette,
            Token::Ink,
        ))
        .min_size(Vec2::new(0.0, 30.0));
        if ui.add(open).clicked() {
            cx.intents.push(UiIntent::OpenCompare {
                target: subject.target,
                counterpart: subject.counterpart,
                return_row: cx.findings.focused,
            });
            response = DialogResponse::Close;
        }
        let copy = egui::Button::new(text(
            "Copy unique lines",
            CompactStyle::ControlLabel,
            palette,
            Token::Ink,
        ))
        .min_size(Vec2::new(0.0, 30.0));
        if ui.add_enabled(ready.is_some(), copy).clicked()
            && let Some(aligned) = ready
        {
            cx.intents
                .push(UiIntent::CopyText(aligned.copy_only_in_b().text));
        }
    });

    ui.label(text(
        "Queueing does not delete the file. Cleanup still requires a separate review and \
         confirmation.",
        CompactStyle::Supporting,
        palette,
        Token::Muted,
    ));

    draft.checkbox = Some(acknowledgment(ui, palette, draft));
    if draft.prompt && !draft.acknowledged {
        notice(ui, palette, PROMPT);
    } else if let Some(blocked) = draft.blocked {
        notice(ui, palette, blocked.message());
    }
    response
}

/// Cancel, then Queue for cleanup, in the footer slot the dialog host pins
/// below the body. Returns [`DialogResponse::Close`] when either closed the
/// popup.
pub fn footer(ui: &mut egui::Ui, draft: &mut Draft, cx: &mut DialogCx<'_>) -> DialogResponse {
    let ctx = ui.ctx().clone();
    let palette = theme::palette(dialog_host::mode_of(&ctx));
    let mut response = DialogResponse::KeepOpen;
    if footer_row(ui, palette, "Queue for cleanup", false, true, &mut response) {
        response = submit(draft, cx);
    }

    // After the footer's buttons, so a click on Queue for cleanup this frame
    // moves focus too: the checkbox drew earlier in the frame, in the body,
    // and its ID is stable, so the request lands on it now rather than a
    // frame later. The host scrolls it into view on the next body draw; the
    // checkbox sits last in the body, under the pinned footer band when the
    // body is long. `reveal_focus` covers the press that finds the checkbox
    // already focused but scrolled away, which moves no focus for the host
    // to notice.
    if std::mem::take(&mut draft.focus_checkbox)
        && let Some(checkbox) = draft.checkbox
    {
        ctx.memory_mut(|memory| memory.request_focus(checkbox));
        dialog_host::reveal_focus(&ctx);
        ctx.request_repaint();
    }
    response
}

/// Queue for cleanup, run by Enter and by the button.
///
/// Without the acknowledgment: nothing is sent, the prompt shows and focus
/// moves to the checkbox. With it, and with the displayed lines current,
/// the acknowledgment and then the queue request go to the review service
/// and the popup closes. `reviewed` carries the Compare comparison the pair
/// was reviewed in, when there was one, so both bind to it (C-05).
pub fn submit(draft: &mut Draft, cx: &mut DialogCx<'_>) -> DialogResponse {
    let Some(subject) = subject(cx) else {
        return DialogResponse::Close;
    };
    if !draft.acknowledged {
        draft.prompt = true;
        draft.blocked = None;
        draft.focus_checkbox = true;
        return DialogResponse::KeepOpen;
    }
    let blocked = match evidence(cx.compare, &subject) {
        Evidence::Loading => Some(Blocked::Loading),
        Evidence::Unavailable => Some(Blocked::Unavailable),
        Evidence::Ready(_) if !current(cx, &subject) => Some(Blocked::Stale),
        Evidence::Ready(_) => None,
    };
    if blocked.is_some() {
        draft.blocked = blocked;
        return DialogResponse::KeepOpen;
    }
    // ORDER: the acknowledgment first. The queue request checks for an
    // acknowledgment bound to exactly the binding it will record (C-05
    // I-5); sent the other way round it answers NeedsReview and reopens
    // this popup.
    cx.intents.push(UiIntent::Review {
        command: ReviewCommand::AcknowledgeUniqueText {
            file: subject.target,
            counterpart: subject.counterpart,
            reviewed: subject.reviewed,
        },
        invoker: None,
    });
    cx.intents.push(UiIntent::Review {
        command: ReviewCommand::RequestQueue {
            file: subject.target,
            counterpart: subject.counterpart,
            origin: QueueOrigin::UniqueTextReview,
            reviewed: subject.reviewed,
        },
        invoker: None,
    });
    DialogResponse::Close
}

fn member(results: &ScanResults, file: FileId) -> Option<&Member> {
    results.groups.iter().find_map(|g| g.member(file))
}

/// "2 lines exist only in this file." and what moving the file costs.
fn headline(count: u64, counterpart: &str) -> (String, String) {
    match count {
        0 => (
            "No lines exist only in this file.".to_owned(),
            format!("Every line is also in {counterpart}."),
        ),
        1 => (
            "1 line exists only in this file.".to_owned(),
            format!(
                "It is not in {counterpart}. Moving this file to Trash removes the only copy \
                 of it."
            ),
        ),
        n => (
            format!("{n} lines exist only in this file."),
            format!(
                "They are not in {counterpart}. Moving this file to Trash removes the only \
                 copy of them."
            ),
        ),
    }
}

/// The target, then "Compared with reference" and the counterpart, each
/// with its folder.
fn identities(
    ui: &mut egui::Ui,
    palette: &Palette,
    target: Option<&Member>,
    counterpart: Option<&Member>,
    against_reference: bool,
) {
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        ui.horizontal(|ui| {
            if let Some(m) = target {
                ui.label(text(
                    &m.name,
                    CompactStyle::GroupEmphasis,
                    palette,
                    Token::Ink,
                ));
                ui.label(text(
                    &m.folder,
                    CompactStyle::PathDetail,
                    palette,
                    Token::Muted,
                ));
            }
        });
        ui.horizontal(|ui| {
            let lead = if against_reference {
                "Compared with reference"
            } else {
                "Compared with"
            };
            ui.label(text(lead, CompactStyle::Supporting, palette, Token::Muted));
            if let Some(m) = counterpart {
                ui.label(text(
                    &m.name,
                    CompactStyle::ControlLabel,
                    palette,
                    Token::Ink,
                ));
                ui.label(text(
                    &m.folder,
                    CompactStyle::PathDetail,
                    palette,
                    Token::Muted,
                ));
            }
        });
    });
}

/// The amber "Unmatched content" box.
fn unmatched(ui: &mut egui::Ui, palette: &Palette, headline: &str, consequence: &str) {
    egui::Frame::new()
        .fill(palette.color(Token::AmberSoft))
        .inner_margin(Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 4.0;
            ui.label(text(
                headline,
                CompactStyle::GroupEmphasis,
                palette,
                Token::Amber,
            ));
            if !consequence.is_empty() {
                ui.label(text(
                    consequence,
                    CompactStyle::Supporting,
                    palette,
                    Token::Amber,
                ));
            }
        });
}

/// The target's unique lines in file order, each with its source line
/// number and the frame's "B" side mark. The target is the comparison's
/// side B, Compare's right pane, so every read here is side B's.
/// Virtualized: only the visible rows are laid out, so a 5 MB target with
/// thousands of unique lines costs a screenful.
fn unique_lines(ui: &mut egui::Ui, palette: &Palette, aligned: &AlignedComparison) {
    let lines = &aligned.only_in_b.lines;
    egui::Frame::new()
        .fill(palette.color(Token::Code))
        .stroke(Stroke::new(theme::BORDER_WIDTH, palette.color(Token::Line)))
        .corner_radius(theme::CORNER_RADIUS)
        .inner_margin(Margin::symmetric(0, 4))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            egui::ScrollArea::vertical()
                .id_salt("fd.unique_lines")
                .max_height(ROW_HEIGHT * VISIBLE_ROWS)
                .auto_shrink([false, true])
                .show_rows(ui, ROW_HEIGHT, lines.len(), |ui, range| {
                    for &line in &lines[range] {
                        let source = aligned.b.original.line(line).unwrap_or("");
                        line_row(ui, palette, line + 1, source);
                    }
                });
        });
}

/// One `DD/Compact/Diff line/only-B` row: side bar, gutter, side mark, text.
fn line_row(ui: &mut egui::Ui, palette: &Palette, number: usize, source: &str) {
    const BAR: f32 = 3.0;
    const GUTTER: f32 = 37.0;
    const GUTTER_PAD: f32 = 6.0;
    const SIDE: f32 = 16.0;
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_HEIGHT), Sense::hover());
    let name = format!("Line {number}: {source}");
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &name));
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, palette.color(Token::DiffBSoft));
    let bar = egui::Rect::from_min_size(rect.min, Vec2::new(BAR, ROW_HEIGHT));
    painter.rect_filled(bar, 0.0, palette.color(Token::DiffB));
    let y = rect.center().y;
    painter.text(
        egui::pos2(rect.left() + BAR + GUTTER - GUTTER_PAD, y),
        Align2::RIGHT_CENTER,
        number.to_string(),
        CompactStyle::PathDetail.font_id(),
        palette.color(Token::Muted),
    );
    painter.text(
        egui::pos2(rect.left() + BAR + GUTTER + SIDE / 2.0, y),
        Align2::CENTER_CENTER,
        "B",
        CompactStyle::PathDetail.font_id(),
        palette.color(Token::DiffB),
    );
    painter.text(
        egui::pos2(rect.left() + BAR + GUTTER + SIDE, y),
        Align2::LEFT_CENTER,
        source,
        CompactStyle::Document.font_id(),
        palette.color(Token::Ink),
    );
}

/// The acknowledgment checkbox; returns its ID for the focus move. In the
/// prompt state its box is drawn in the red token, as in `44:4079`.
fn acknowledgment(ui: &mut egui::Ui, palette: &Palette, draft: &mut Draft) -> Id {
    let prompt = draft.prompt && !draft.acknowledged;
    let response = ui
        .scope(|ui| {
            if prompt {
                let red = Stroke::new(theme::BORDER_WIDTH, palette.color(Token::Red));
                let widgets = &mut ui.visuals_mut().widgets;
                widgets.inactive.bg_stroke = red;
                widgets.hovered.bg_stroke = red;
            }
            ui.add(egui::Checkbox::new(
                &mut draft.acknowledged,
                text(ACKNOWLEDGMENT, CompactStyle::Table, palette, Token::Ink),
            ))
        })
        .inner;
    if response.changed() && draft.acknowledged {
        draft.prompt = false;
        draft.blocked = None;
    }
    response.id
}

/// A red alert line under the checkbox: the prompt or a blocked reason.
fn notice(ui: &mut egui::Ui, palette: &Palette, message: &str) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        icon(ui, Icon::Alert, palette.color(Token::Red));
        ui.label(text(message, CompactStyle::Supporting, palette, Token::Red));
    });
}
