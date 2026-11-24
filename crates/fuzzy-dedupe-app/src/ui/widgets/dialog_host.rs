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
//! and the popup's own body (`ui/dialogs/*.rs`) inside a modal at the size
//! the popup's size hook gives (`DialogDraft::size`, its reference size
//! unless the popup overrides it), shrunk to the usable viewport. Every
//! popup's action footer (`DialogDraft::footer`) is drawn in one slot pinned
//! to the foot of the content inset, below a body that scrolls in the space
//! above it. A body control that gains keyboard focus, by Tab or moved
//! there by its popup, is scrolled into the body's visible clip
//! (`reveal_focused`). Esc cancels and Enter runs the popup's
//! non-destructive default; both are consumed here before the popup's
//! widgets see them. Closing in
//! any way puts focus back on the invoker on the next frame
//! ([`begin_frame`]).
//!
//! Frame order contract with `app.rs`: [`begin_frame`] runs before anything
//! draws, the shell and workspace draw next, and [`show`] runs last, so the
//! modal layer sits above the workspace it blocks.

use egui::{Context, Id, Key, Modifiers, Rect, Sense, Stroke, Vec2};

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

/// The size popup `kind` opens at with no size of its own: its reference
/// size, constrained as [`constrained_size`] says.
#[must_use]
pub fn popup_size(ctx: &Context, kind: DialogKind) -> Vec2 {
    constrained_size(ctx, kind.reference_size())
}

/// `size`, shrunk to the viewport minus the content inset on each side, so
/// the whole popup and its action footer stay on screen (REQ-032). Content
/// that no longer fits scrolls inside the popup.
#[must_use]
pub fn constrained_size(ctx: &Context, [width, height]: [f32; 2]) -> Vec2 {
    let room = ctx.content_rect().size() - Vec2::splat(2.0 * CONTENT_INSET);
    Vec2::new(width.min(room.x), height.min(room.y)).max(Vec2::ZERO)
}

/// The pinned footer slot's height, one control row (spec §11.3), and its
/// gap below the scrolling body (frame `36:2435`). Every footer fits one
/// row: Keyboard shortcuts' note is the only text in one, and it stays a
/// single line at the popup's narrowest width (960 wide, or the 992 a
/// 1024-wide window leaves, so the note has over 800 units).
pub const FOOTER_HEIGHT: f32 = 30.0;
/// The gap between the body's scroll area and the pinned footer slot. The
/// body's visible clip ends this far above the slot, so a body control in
/// the gap is already clipped (PV-T38-001 measures against it).
pub const FOOTER_GAP: f32 = 12.0;

fn draw(
    ctx: &Context,
    kind: DialogKind,
    draft: &mut DialogDraft,
    validation: &[String],
    cx: &mut DialogCx<'_>,
    focus_entry: bool,
) -> DialogResponse {
    // The size comes from the popup's size hook each frame, so a popup that
    // grows (New scan expanding its matching rules) resizes in place. The
    // frame's 1-unit border is drawn outside its fill, so the content is
    // that size less both borders; the painted popup is then exactly it.
    let size = constrained_size(ctx, draft.size(kind)) - Vec2::splat(2.0 * theme::BORDER_WIDTH);
    let palette = theme::palette(mode_of(ctx));
    let frame = egui::Frame::new()
        .fill(palette.color(Token::Surface))
        .stroke(Stroke::new(theme::BORDER_WIDTH, palette.color(Token::Line)))
        .corner_radius(theme::CORNER_RADIUS)
        .shadow(theme::shadow(mode_of(ctx)));
    let modal = egui::Modal::new(Id::unique(("fd.dialog", kind))).frame(frame);
    modal
        .show(ctx, |ui| {
            // Exactly `size`, whatever the content asks for: the popup draws
            // into a child of this allocation, and a child never grows its
            // parent. `set_min_size` with `set_max_size` alone let a body a
            // few units too wide widen the whole popup past its size.
            let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
            let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(rect));
            ui.shrink_clip_rect(rect);
            let ui = &mut ui;
            let mut response = DialogResponse::KeepOpen;
            egui::Frame::new()
                .inner_margin(CONTENT_INSET)
                .show(ui, |ui| {
                    let inner = size - Vec2::splat(2.0 * CONTENT_INSET);
                    let content = Rect::from_min_size(ui.cursor().min, inner);
                    ui.set_min_size(inner);
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

                    // The footer slot sits at the foot of the content inset
                    // and the body scrolls in the space above it, so every
                    // footer control stays inside the popup and clickable
                    // however small the viewport or long the body (spec
                    // §11.6). A footer drawn at the end of the body instead
                    // scrolled out of view with it, where a click could not
                    // reach it (T22, plan T36).
                    //
                    // ORDER: the body is drawn first, so Tab reaches every
                    // body control before the footer's.
                    let footer = Rect::from_min_max(
                        egui::pos2(content.left(), content.bottom() - FOOTER_HEIGHT),
                        content.max,
                    );
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .max_height((footer.top() - FOOTER_GAP - ui.cursor().top()).max(0.0))
                        .show(ui, |ui| {
                            if draft.show(ui, cx) == DialogResponse::Close {
                                response = DialogResponse::Close;
                            }
                            // ORDER: after the body draws, inside the scroll
                            // area, so this pass holds the focused control's
                            // rect and the scroll target reaches this area
                            // rather than a nested one that already ended.
                            reveal_focused(ui);
                        });
                    let closed = ui
                        .scope_builder(egui::UiBuilder::new().max_rect(footer), |ui| {
                            draft.footer(ui, cx)
                        })
                        .inner;
                    if closed == DialogResponse::Close {
                        response = DialogResponse::Close;
                    }
                });
            response
        })
        .inner
}

/// Where the host remembers the focused widget it last revealed, so it
/// reveals each arrival of focus once.
fn revealed_key() -> Id {
    Id::unique("fd.dialog.revealed_focus")
}

/// Asks the host to scroll the focused body control into view on the next
/// body draw even though focus has not moved, as when a popup moves focus
/// to the control that already has it (Review unique text's checkbox,
/// focused, then scrolled away, on a second unacknowledged Queue for
/// cleanup). A focus move to a different control needs no call.
pub fn reveal_focus(ctx: &Context) {
    ctx.data_mut(|data| data.remove::<Option<Id>>(revealed_key()));
}

/// Scrolls the host's body so the focused body control lies inside its
/// visible clip (REQ-032, NFR-005). Runs inside the body's scroll area,
/// after the body drew.
///
/// egui 0.37 never does this itself: a scroll target is set only by
/// `scroll_to_me`, `scroll_to_rect` or an accesskit `ScrollIntoView`, not by
/// Tab or `request_focus`. Without it a control below the clip, such as
/// Review unique text's checkbox under the pinned footer band, takes focus
/// out of sight and out of the pointer's reach (T22).
///
/// It reveals once per arrival of focus and then leaves the scroll offset
/// alone; re-scrolling every frame would snap the body back under a user
/// who scrolls away from the focused control with the wheel.
///
/// SCOPE: only a control clipped by this body's clip and nothing else. The
/// title-row × and the footer controls lie outside the clip unclipped by it,
/// and a control inside a nested scroll area (About's license text) is
/// clipped by that area too; both fail the `interact_rect` test below, so
/// the body never scrolls toward a footer control, and a nested area keeps
/// its own scrolling.
fn reveal_focused(ui: &egui::Ui) {
    let ctx = ui.ctx();
    let focused = ctx.memory(egui::Memory::focused);
    let key = revealed_key();
    if ctx.data(|data| data.get_temp::<Option<Id>>(key)) == Some(focused) {
        return;
    }
    ctx.data_mut(|data| data.insert_temp(key, focused));
    let Some(widget) = focused.and_then(|id| ctx.read_response(id)) else {
        return;
    };
    let clip = ui.clip_rect();
    // egui records a widget's interact rect as its rect cut to the clip of
    // the Ui that drew it, so equality says exactly this clip, and no inner
    // one, cut it. A widget outside the body is not cut by it at all.
    let cut_by_body =
        widget.layer_id == ui.layer_id() && widget.interact_rect == widget.rect.intersect(clip);
    if cut_by_body && !clip.contains_rect(widget.rect) {
        // No animation: the focus ring must be in view on the frame focus
        // lands, not after the scroll catches up.
        ui.scroll_to_rect_animation(widget.rect, None, egui::style::ScrollAnimation::none());
    }
}

/// The palette mode egui is drawing this frame in.
#[must_use]
pub fn mode_of(ctx: &Context) -> theme::Mode {
    match ctx.theme() {
        egui::Theme::Light => theme::Mode::Light,
        egui::Theme::Dark => theme::Mode::Dark,
    }
}
