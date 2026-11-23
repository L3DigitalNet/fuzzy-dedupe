// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! The dialog host (spec §11.6, plan C-12): one modal popup or menu at a
//! time, with the behavior every popup shares.
//!
//! Callers open an overlay with [`open`] or [`open_menu`], naming the
//! control that invoked it, and close it with [`close`]. A second request
//! while any popup or menu is open is refused with [`Refused`] and changes
//! nothing; a popup never opens over another (spec §11.6).
//!
//! For the open popup, [`show`] draws the title, a reachable close control
//! and the popup's own body (`ui/dialogs/*.rs`) inside a modal at the
//! popup's reference size, shrunk to the usable viewport. Esc cancels and
//! Enter runs the popup's non-destructive default; both are consumed here
//! before the popup's widgets see them. Closing in any way puts focus back
//! on the invoker on the next frame ([`begin_frame`]).
//!
//! Frame order contract with `app.rs`: [`begin_frame`] runs before anything
//! draws, the shell and workspace draw next, and [`show`] runs last, so the
//! modal layer sits above the workspace it blocks.

use egui::{Context, Id, Key, Modifiers, Stroke, Vec2};

use crate::presentation::commands::IntentQueue;
use crate::presentation::state::{
    DialogKind, DialogState, DialogSubject, Invoker, MenuKind, OpenDialog, Overlay,
    PresentationState,
};
use crate::ui::dialogs::{DialogCx, DialogDraft, DialogResponse};
use crate::ui::theme::{self, CompactStyle, Token};

/// Why an overlay request was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Refused {
    /// A popup or menu is already open.
    OverlayOpen,
    /// The subject is not the one the popup needs (see
    /// `DialogSubject::fits`): Review unique text without its pair, Queue
    /// identical copies without its scope, or a subject for any other.
    WrongSubject,
}

/// The popup content inset shared by every popup (spec §11.3).
pub const CONTENT_INSET: f32 = 16.0;

/// Opens popup `kind` about `subject`, invoked by `invoker`, with a fresh
/// draft. The popup's `open` may push intents into `intents`, such as the
/// comparison Review unique text needs or the plan Review cleanup builds;
/// they are dispatched in the same frame.
///
/// # Errors
///
/// [`Refused::OverlayOpen`] when a popup or menu is open, and
/// [`Refused::WrongSubject`] when `subject` does not fit `kind`; nothing
/// changes either way.
pub fn open(
    state: &mut PresentationState,
    kind: DialogKind,
    subject: DialogSubject,
    invoker: Invoker,
    intents: &mut IntentQueue,
) -> Result<(), Refused> {
    if state.dialog.is_open() {
        return Err(Refused::OverlayOpen);
    }
    if !subject.fits(kind) {
        return Err(Refused::WrongSubject);
    }
    state.dialog.picked_folders = None;
    let mut picked = None;
    let mut cx = DialogCx {
        subject: &subject,
        findings: &state.findings,
        compare: &state.compare,
        queue: &state.queue,
        scan: &state.scan,
        operation: &state.operation,
        preferences: &state.preferences,
        picked_folders: &mut picked,
        intents,
    };
    let draft = DialogDraft::open(kind, &mut cx);
    state.dialog.overlay = Overlay::Dialog(OpenDialog {
        kind,
        subject,
        invoker,
        draft,
        validation: Vec::new(),
        pending: false,
    });
    state.dialog.focus_entry = true;
    // A popup opened straight from a menu (main menu → About) must not have
    // the menu's pending focus return land under it.
    state.dialog.restore_focus = None;
    Ok(())
}

/// Opens menu `kind`, invoked by `invoker`.
///
/// # Errors
///
/// [`Refused::OverlayOpen`] when a popup or menu is open; nothing changes.
pub fn open_menu(
    dialog: &mut DialogState,
    kind: MenuKind,
    invoker: Invoker,
) -> Result<(), Refused> {
    if dialog.is_open() {
        return Err(Refused::OverlayOpen);
    }
    dialog.overlay = Overlay::Menu { kind, invoker };
    dialog.focus_entry = true;
    dialog.restore_focus = None;
    Ok(())
}

/// Closes the open popup or menu without committing anything and schedules
/// focus back to its invoker. Does nothing when none is open.
pub fn close(dialog: &mut DialogState) {
    let invoker = match std::mem::take(&mut dialog.overlay) {
        Overlay::None => return,
        Overlay::Menu { invoker, .. } => invoker,
        Overlay::Dialog(open) => open.invoker,
    };
    dialog.focus_entry = false;
    dialog.restore_focus = invoker;
}

/// Start-of-frame work: returns focus to the last closed overlay's invoker.
///
/// Runs before the invoker is drawn. egui keeps a focus request for a
/// widget not yet drawn this frame (it counts as recently gained), so the
/// invoker holds focus once it draws.
///
/// The request waits while egui still reports a modal layer. egui updates
/// that layer at the end of each pass, so after a popup closed from inside
/// its own drawing (its close control) the next pass still treats the
/// workspace as under the modal and drops a focus request made there; the
/// restore would silently not happen. One pass later the layer is gone.
pub fn begin_frame(ctx: &Context, dialog: &mut DialogState) {
    if ctx.memory(egui::Memory::top_modal_layer).is_none()
        && let Some(id) = dialog.restore_focus.take()
    {
        ctx.memory_mut(|memory| memory.request_focus(id));
    }
}

/// Whether this frame's popup Esc or Enter belongs to an egui popup inside
/// the overlay (a combo box list), which must close first.
fn inner_popup_open(ctx: &Context) -> bool {
    egui::Popup::is_any_open(ctx)
}

/// Handles Esc for an open menu: closes it and restores focus.
pub fn handle_menu_keys(ctx: &Context, dialog: &mut DialogState) {
    if dialog.menu().is_some() && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
        close(dialog);
    }
}

/// Draws the open popup, if any, and applies Esc, Enter and its close
/// control. Draws nothing for a menu; the shell draws menus.
pub fn show(ctx: &Context, state: &mut PresentationState, intents: &mut IntentQueue) {
    let PresentationState {
        findings,
        compare,
        queue,
        scan,
        operation,
        preferences,
        dialog,
        ..
    } = state;
    let DialogState {
        overlay,
        focus_entry,
        picked_folders,
        ..
    } = dialog;
    let Overlay::Dialog(OpenDialog {
        kind,
        subject,
        draft,
        validation,
        pending,
        ..
    }) = overlay
    else {
        return;
    };
    let kind = *kind;
    let mut cx = DialogCx {
        subject,
        findings,
        compare,
        queue,
        scan,
        operation,
        preferences,
        picked_folders,
        intents,
    };

    // Keys first, so the popup's own widgets never see a consumed Esc or
    // Enter: a focused button must not also fire on the Enter that ran the
    // default action.
    let mut response = DialogResponse::KeepOpen;
    if !inner_popup_open(ctx) {
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
            response = DialogResponse::Close;
        } else if !*pending && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter)) {
            response = draft.default_action(kind, &mut cx);
        }
    }

    if response == DialogResponse::KeepOpen {
        let focus_entry = std::mem::take(focus_entry);
        response = draw(ctx, kind, draft, validation, &mut cx, focus_entry);
    }
    if response == DialogResponse::Close {
        close(dialog);
    }
}

/// The popup's size: its reference size, shrunk to the viewport minus the
/// content inset on each side so the action footer stays reachable.
#[must_use]
pub fn popup_size(ctx: &Context, kind: DialogKind) -> Vec2 {
    let [width, height] = kind.reference_size();
    let room = ctx.content_rect().size() - Vec2::splat(2.0 * CONTENT_INSET);
    Vec2::new(width.min(room.x), height.min(room.y)).max(Vec2::ZERO)
}

fn draw(
    ctx: &Context,
    kind: DialogKind,
    draft: &mut DialogDraft,
    validation: &[String],
    cx: &mut DialogCx<'_>,
    focus_entry: bool,
) -> DialogResponse {
    // The frame's 1-unit border is drawn outside its fill, so the content is
    // the reference size less both borders; the painted popup is then
    // exactly the reference size.
    let size = popup_size(ctx, kind) - Vec2::splat(2.0 * theme::BORDER_WIDTH);
    let palette = theme::palette(mode_of(ctx));
    let frame = egui::Frame::new()
        .fill(palette.color(Token::Surface))
        .stroke(Stroke::new(theme::BORDER_WIDTH, palette.color(Token::Line)))
        .corner_radius(theme::CORNER_RADIUS)
        .shadow(theme::shadow(mode_of(ctx)));
    let modal = egui::Modal::new(Id::unique(("fd.dialog", kind))).frame(frame);
    modal
        .show(ctx, |ui| {
            ui.set_min_size(size);
            ui.set_max_size(size);
            let mut response = DialogResponse::KeepOpen;
            egui::Frame::new()
                .inner_margin(CONTENT_INSET)
                .show(ui, |ui| {
                    ui.set_min_size(size - Vec2::splat(2.0 * CONTENT_INSET));
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(draft.title(kind, cx))
                                .text_style(CompactStyle::Title.text_style()),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let close = ui
                                .add(egui::Button::new("×").frame(false))
                                .on_hover_text("Close (Esc)");
                            close.widget_info(|| {
                                egui::WidgetInfo::labeled(egui::Role::Button, true, "Close")
                            });
                            if focus_entry {
                                close.request_focus();
                            }
                            if close.clicked() {
                                response = DialogResponse::Close;
                            }
                        });
                    });
                    ui.add_space(CONTENT_INSET / 2.0);
                    for message in validation {
                        ui.colored_label(palette.color(Token::Red), message);
                    }
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            if draft.show(ui, cx) == DialogResponse::Close {
                                response = DialogResponse::Close;
                            }
                        });
                });
            response
        })
        .inner
}

/// The palette mode egui is drawing this frame in.
#[must_use]
pub fn mode_of(ctx: &Context) -> theme::Mode {
    match ctx.theme() {
        egui::Theme::Light => theme::Mode::Light,
        egui::Theme::Dark => theme::Mode::Dark,
    }
}
