// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! The application shell (spec §11.2, §11.3; frames `13:648`, `28:1399`,
//! `44:3954`, `29:2214`): navigation rail, main menu, the toolbar row with
//! its shared trailing items, the status bar, and the workspace switch.
//!
//! Layout, in units: a 26-high status bar across the window foot; above it
//! the rail, 152 wide, collapsed to 48-wide icons when the window is
//! narrower than 1280 or the user collapsed it; the work area takes the
//! rest with a 12-unit inset, a 32-high toolbar row and the workspace body
//! 8 below it. The rail's collapse threshold reads the window width, not
//! the work area, so zoom and the rail itself cannot feed back into it.
//!
//! The workspace views fill the leading toolbar items and the body
//! (`findings.rs`, `compare.rs`); the shell owns Review cleanup (N) and the
//! Settings gear, which both workspaces share. Nothing here calls a
//! service: every action is a pushed [`UiIntent`].

use egui::{
    Align, Align2, Color32, CornerRadius, FontId, Id, Layout, Rect, Response, Sense, Stroke,
    StrokeKind, Ui, Vec2,
};

use crate::presentation::commands::{IntentQueue, UiIntent};
use crate::presentation::projections;
use crate::presentation::state::{
    DialogKind, DialogSubject, MenuKind, PresentationState, ThemeOverride, WorkspaceState,
};
use crate::ui::theme::{self, CompactStyle, Icon, Palette, Token};
use crate::ui::widgets::dialog_host::mode_of;
use crate::ui::widgets::status;
use crate::ui::{WorkspaceCx, compare, findings, glyphs};

/// Rail width, expanded and collapsed (spec §11.3).
pub const RAIL_WIDTH: f32 = 152.0;
pub const RAIL_COLLAPSED_WIDTH: f32 = 48.0;
/// The rail collapses when the window is narrower than this (spec §11.3
/// collapse order, step 1). Exactly 1280 keeps it expanded.
pub const RAIL_COLLAPSE_BELOW: f32 = 1280.0;
/// Toolbar row and control heights (spec §11.3).
pub const TOOLBAR_HEIGHT: f32 = 32.0;
pub const CONTROL_HEIGHT: f32 = 30.0;
/// Work-area inset and the compact vertical gap (spec §11.3).
pub const WORK_INSET: f32 = 12.0;
pub const GAP: f32 = 8.0;
/// The main menu's width (frame `44:3954`).
pub const MENU_WIDTH: f32 = 260.0;
/// Accessible names of the shell's regions and controls, which tests and
/// assistive tooling find them by.
pub const RAIL_NAME: &str = "Navigation rail";
pub const MENU_ENTRY_NAME: &str = "Menu";
pub const SETTINGS_NAME: &str = "Matching rules";

/// The main menu's six entries, in order (spec §11.2).
pub const MAIN_MENU_ENTRIES: [&str; 6] = [
    "Appearance",
    "Zoom",
    "Keyboard shortcuts…",
    "Open log folder",
    "About Fuzzy DeDupe",
    "Quit",
];

const RAIL_PAD: f32 = 8.0;
const RAIL_TOP: f32 = 18.0;
const NAV_ITEM_HEIGHT: f32 = 34.0;
const ICON_SIZE: f32 = 18.0;
const ICON_GAP: f32 = 10.0;

/// The rail Menu entry's ID: the main menu's invoker however it opens, so
/// closing the menu always returns focus there.
#[must_use]
pub fn menu_entry_id() -> Id {
    Id::unique("fd.rail.menu")
}

/// The rail's width for a window `window_width` units wide.
#[must_use]
pub fn rail_width(window_width: f32, user_collapsed: bool) -> f32 {
    if user_collapsed || window_width < RAIL_COLLAPSE_BELOW {
        RAIL_COLLAPSED_WIDTH
    } else {
        RAIL_WIDTH
    }
}

/// Draws the whole shell for one frame.
pub fn show(ui: &mut Ui, state: &mut PresentationState, intents: &mut IntentQueue) {
    let ctx = ui.ctx().clone();
    let palette = theme::palette(mode_of(&ctx));
    let window_width = ctx.content_rect().width();
    let collapsed = rail_width(window_width, state.preferences.rail_collapsed) < RAIL_WIDTH;
    let input_enabled = !state.dialog.is_open();
    let review_label = projections::review_cleanup_label(state);

    egui::Panel::bottom("fd.status")
        .exact_size(status::HEIGHT)
        .resizable(false)
        .show_separator_line(false)
        .frame(egui::Frame::NONE)
        .show(ui, |ui| status::show(ui, state));

    let menu_entry = egui::Panel::left("fd.rail")
        .exact_size(rail_width(window_width, state.preferences.rail_collapsed))
        .resizable(false)
        .show_separator_line(false)
        .frame(egui::Frame::NONE.fill(palette.color(Token::Rail)))
        .show(ui, |ui| rail(ui, state, intents, palette, collapsed))
        .inner;

    egui::CentralPanel::default()
        .frame(
            egui::Frame::NONE
                .fill(palette.color(Token::Canvas))
                .inner_margin(WORK_INSET),
        )
        .show(ui, |ui| {
            let PresentationState {
                workspace,
                findings,
                compare,
                queue,
                scan,
                operation,
                preferences,
                dialog,
                ..
            } = state;
            let mut cx = WorkspaceCx {
                findings,
                compare,
                queue,
                scan,
                operation,
                preferences,
                input_enabled,
                menu: dialog.menu(),
            };
            let workspace = *workspace;
            // egui adds `item_spacing.y` after every allocation, which would
            // stack on the §11.3 gap and put the body 11 below the toolbar
            // row, not 8. The row is therefore allocated with no vertical
            // spacing, so `GAP` is the whole gap, and the body gets the
            // theme's spacing back for its own content. Rejected: zeroing
            // the spacing in `ui/theme.rs`, which would also move every
            // popup body and the rail.
            let item_spacing = ui.spacing().item_spacing;
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.allocate_ui_with_layout(
                Vec2::new(ui.available_width(), TOOLBAR_HEIGHT),
                Layout::left_to_right(Align::Center),
                |ui| {
                    match workspace {
                        WorkspaceState::Findings => findings::toolbar(ui, &mut cx, intents),
                        WorkspaceState::Compare => compare::toolbar(ui, &mut cx, intents),
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        shared_toolbar_items(ui, &review_label, intents, palette);
                    });
                },
            );
            ui.add_space(GAP);
            ui.spacing_mut().item_spacing = item_spacing;
            match workspace {
                WorkspaceState::Findings => findings::show(ui, &mut cx, intents),
                WorkspaceState::Compare => compare::show(ui, &mut cx, intents),
            }
        });

    if state.dialog.menu() == Some(MenuKind::Main) {
        main_menu(&ctx, state, intents, palette, menu_entry);
    }
}

fn rail(
    ui: &mut Ui,
    state: &PresentationState,
    intents: &mut IntentQueue,
    palette: &Palette,
    collapsed: bool,
) -> Rect {
    let rect = ui.max_rect();
    let region = ui.interact(rect, Id::unique("fd.rail"), Sense::hover());
    region.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Group, true, RAIL_NAME));

    let item_width = rect.width() - 2.0 * RAIL_PAD;
    let mut top = rect.top() + RAIL_TOP;
    let mut next = |height: f32| {
        let item = Rect::from_min_size(
            egui::pos2(rect.left() + RAIL_PAD, top),
            Vec2::new(item_width, height),
        );
        top += height + GAP;
        item
    };

    let new_scan = rail_control(
        ui,
        next(CONTROL_HEIGHT),
        Id::unique("fd.rail.new_scan"),
        RailLook::Action,
        None,
        if collapsed { "+" } else { "+  New scan" },
        "New scan",
        palette,
    );
    if new_scan.clicked() {
        intents.push(UiIntent::OpenDialog {
            subject: DialogSubject::None,
            kind: DialogKind::NewScan,
            invoker: Some(new_scan.id),
        });
    }
    for (workspace, icon, label) in [
        (WorkspaceState::Findings, Icon::Findings, "Findings"),
        (WorkspaceState::Compare, Icon::Compare, "Compare"),
    ] {
        let selected = state.workspace == workspace;
        let response = rail_control(
            ui,
            next(NAV_ITEM_HEIGHT),
            Id::unique(("fd.rail.nav", label)),
            if selected {
                RailLook::Selected
            } else {
                RailLook::Plain
            },
            Some(icon),
            if collapsed { "" } else { label },
            label,
            palette,
        );
        if response.clicked() {
            intents.push(UiIntent::SwitchWorkspace(workspace));
        }
    }

    // Menu entry and the "Local processing" note sit at the rail foot.
    let note_height = if collapsed { 0.0 } else { 18.0 + GAP };
    let menu_rect = Rect::from_min_size(
        egui::pos2(
            rect.left() + RAIL_PAD,
            rect.bottom() - RAIL_PAD - note_height - NAV_ITEM_HEIGHT,
        ),
        Vec2::new(item_width, NAV_ITEM_HEIGHT),
    );
    let menu_open = state.dialog.menu() == Some(MenuKind::Main);
    let menu = rail_control(
        ui,
        menu_rect,
        menu_entry_id(),
        if menu_open {
            RailLook::Selected
        } else {
            RailLook::Plain
        },
        Some(Icon::Menu),
        if collapsed { "" } else { MENU_ENTRY_NAME },
        MENU_ENTRY_NAME,
        palette,
    );
    if menu.clicked() {
        intents.push(UiIntent::ToggleMenu {
            kind: MenuKind::Main,
            invoker: Some(menu.id),
        });
    }
    if !collapsed {
        ui.painter().text(
            egui::pos2(rect.left() + RAIL_PAD, rect.bottom() - RAIL_PAD - 18.0),
            Align2::LEFT_TOP,
            "Local processing",
            CompactStyle::Supporting.font_id(),
            palette.color(Token::Muted),
        );
    }
    menu.rect
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RailLook {
    /// The bordered New scan action.
    Action,
    /// The current workspace, or the Menu entry while its menu is open.
    Selected,
    Plain,
}

/// One rail control at `rect` with a fixed `id`, so its ID is known before
/// it draws (the main menu's invoker) and stays stable across frames.
#[expect(
    clippy::too_many_arguments,
    reason = "one private painter for three rail control looks; a struct would only rename the arguments"
)]
fn rail_control(
    ui: &mut Ui,
    rect: Rect,
    id: Id,
    look: RailLook,
    icon: Option<Icon>,
    text: &str,
    accessible_name: &str,
    palette: &Palette,
) -> Response {
    let response = ui.interact(rect, id, Sense::click());
    let name = accessible_name.to_owned();
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
    let radius = CornerRadius::same(theme::CORNER_RADIUS);
    let (fill, stroke, ink) = match look {
        RailLook::Action => (
            palette.color(Token::Surface),
            Stroke::new(theme::BORDER_WIDTH, palette.color(Token::Line)),
            palette.color(Token::Ink),
        ),
        RailLook::Selected => (
            palette.color(Token::PrimarySoft),
            Stroke::NONE,
            palette.color(Token::Primary),
        ),
        RailLook::Plain if response.hovered() => (
            palette.color(Token::Canvas),
            Stroke::NONE,
            palette.color(Token::Ink),
        ),
        RailLook::Plain => (
            Color32::TRANSPARENT,
            Stroke::NONE,
            palette.color(Token::Ink),
        ),
    };
    let painter = ui.painter();
    painter.rect(rect, radius, fill, stroke, StrokeKind::Inside);
    if response.has_focus() {
        painter.rect_stroke(
            rect,
            radius,
            Stroke::new(2.0 * theme::BORDER_WIDTH, palette.color(Token::Focus)),
            StrokeKind::Inside,
        );
    }
    let font = CompactStyle::ControlLabel.font_id();
    match icon {
        Some(icon) => {
            let icon_pos = egui::pos2(rect.left() + RAIL_PAD + ICON_SIZE / 2.0, rect.center().y);
            // Rail and toolbar icons are text glyphs so they take the
            // theme's ink; `Icon::image` cannot be tinted (`ui/glyphs.rs`).
            if let Some(glyph) = glyphs::text(ui.ctx(), icon) {
                painter.text(
                    icon_pos,
                    Align2::CENTER_CENTER,
                    glyph,
                    FontId::new(ICON_SIZE, font.family.clone()),
                    ink,
                );
            }
            if !text.is_empty() {
                painter.text(
                    egui::pos2(
                        rect.left() + RAIL_PAD + ICON_SIZE + ICON_GAP,
                        rect.center().y,
                    ),
                    Align2::LEFT_CENTER,
                    text,
                    font,
                    ink,
                );
            }
        }
        None => {
            painter.text(rect.center(), Align2::CENTER_CENTER, text, font, ink);
        }
    }
    response
}

/// Review cleanup (N) and the Settings gear, right to left.
fn shared_toolbar_items(ui: &mut Ui, label: &str, intents: &mut IntentQueue, palette: &Palette) {
    let settings = ui.add(
        egui::Button::new(
            egui::RichText::new(glyphs::text(ui.ctx(), Icon::Settings).unwrap_or_default())
                .size(ICON_SIZE),
        )
        .min_size(Vec2::splat(CONTROL_HEIGHT)),
    );
    settings.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, SETTINGS_NAME));
    let settings = settings.on_hover_text("Matching rules (Ctrl+,)");
    if settings.clicked() {
        intents.push(UiIntent::OpenDialog {
            subject: DialogSubject::None,
            kind: DialogKind::MatchingRules,
            invoker: Some(settings.id),
        });
    }

    let review = ui
        .add(
            egui::Button::new(egui::RichText::new(label).color(palette.color(Token::White)))
                .fill(palette.color(Token::Primary))
                .min_size(Vec2::new(0.0, CONTROL_HEIGHT)),
        )
        .on_hover_text("Review cleanup (Ctrl+Enter)");
    if review.clicked() {
        intents.push(UiIntent::OpenDialog {
            subject: DialogSubject::None,
            kind: DialogKind::Cleanup,
            invoker: Some(review.id),
        });
    }
}

fn main_menu(
    ctx: &egui::Context,
    state: &mut PresentationState,
    intents: &mut IntentQueue,
    palette: &Palette,
    anchor: Rect,
) {
    let focus_entry = std::mem::take(&mut state.dialog.focus_entry);
    let invoker = Some(menu_entry_id());
    let theme_now = state.preferences.theme;
    let zoom = state.preferences.zoom;
    let frame = egui::Frame::new()
        .fill(palette.color(Token::Surface))
        .stroke(Stroke::new(theme::BORDER_WIDTH, palette.color(Token::Line)))
        .corner_radius(theme::CORNER_RADIUS)
        .shadow(theme::shadow(mode_of(ctx)))
        .inner_margin(egui::Margin::symmetric(10, 4));
    let area = egui::Area::new(Id::unique("fd.main_menu"))
        .order(egui::Order::Foreground)
        .pivot(Align2::LEFT_BOTTOM)
        .fixed_pos(egui::pos2(anchor.left() + 1.0, anchor.top() - 3.0))
        .show(ctx, |ui| {
            frame.show(ui, |ui| {
                ui.set_width(MENU_WIDTH - 20.0);
                ui.label(
                    egui::RichText::new(MAIN_MENU_ENTRIES[0])
                        .text_style(CompactStyle::Supporting.text_style())
                        .color(palette.color(Token::Muted)),
                );
                ui.horizontal(|ui| {
                    for choice in ThemeOverride::ALL {
                        let option = ui.add_sized(
                            [78.0, 26.0],
                            egui::Button::selectable(theme_now == choice, choice.label()),
                        );
                        if focus_entry && choice == theme_now {
                            option.request_focus();
                        }
                        if option.clicked() {
                            intents.push(UiIntent::SetTheme(choice));
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.label(MAIN_MENU_ENTRIES[1]);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let plus = ui.add_sized([30.0, 30.0], egui::Button::new("+"));
                        plus.widget_info(|| {
                            egui::WidgetInfo::labeled(egui::Role::Button, true, "Zoom in")
                        });
                        ui.add_sized(
                            [44.0, 18.0],
                            egui::Label::new(format!("{}%", zoom.percent())),
                        );
                        let minus = ui.add_sized([30.0, 30.0], egui::Button::new("−"));
                        minus.widget_info(|| {
                            egui::WidgetInfo::labeled(egui::Role::Button, true, "Zoom out")
                        });
                        if plus.clicked() {
                            intents.push(UiIntent::ZoomIn);
                        }
                        if minus.clicked() {
                            intents.push(UiIntent::ZoomOut);
                        }
                    });
                });
                ui.separator();
                if menu_item(ui, MAIN_MENU_ENTRIES[2], Some("F1"), palette).clicked() {
                    intents.push(UiIntent::CloseOverlay);
                    intents.push(UiIntent::OpenDialog {
                        subject: DialogSubject::None,
                        kind: DialogKind::KeyboardShortcuts,
                        invoker,
                    });
                }
                if menu_item(ui, MAIN_MENU_ENTRIES[3], None, palette).clicked() {
                    intents.push(UiIntent::CloseOverlay);
                    intents.push(UiIntent::OpenLogFolder);
                }
                if menu_item(ui, MAIN_MENU_ENTRIES[4], None, palette).clicked() {
                    intents.push(UiIntent::CloseOverlay);
                    intents.push(UiIntent::OpenDialog {
                        subject: DialogSubject::None,
                        kind: DialogKind::About,
                        invoker,
                    });
                }
                ui.separator();
                if menu_item(ui, MAIN_MENU_ENTRIES[5], Some("Ctrl+Q"), palette).clicked() {
                    intents.push(UiIntent::Quit);
                }
            });
        });

    // A press outside the menu and its entry closes it, as a menu does.
    let pressed_outside = ctx.input(|i| {
        i.pointer.any_pressed()
            && i.pointer
                .interact_pos()
                .is_some_and(|pos| !area.response.rect.contains(pos) && !anchor.contains(pos))
    });
    if pressed_outside {
        intents.push(UiIntent::CloseOverlay);
    }
}

/// A 28-high main-menu row with an optional trailing shortcut hint.
fn menu_item(ui: &mut Ui, label: &str, hint: Option<&str>, palette: &Palette) -> Response {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 28.0), Sense::click());
    let name = label.to_owned();
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
    let painter = ui.painter();
    if response.hovered() || response.has_focus() {
        painter.rect_filled(
            rect,
            theme::CORNER_RADIUS,
            palette.color(Token::PrimarySoft),
        );
    }
    if response.has_focus() {
        painter.rect_stroke(
            rect,
            theme::CORNER_RADIUS,
            Stroke::new(theme::BORDER_WIDTH, palette.color(Token::Focus)),
            StrokeKind::Inside,
        );
    }
    painter.text(
        rect.left_center(),
        Align2::LEFT_CENTER,
        label,
        CompactStyle::Table.font_id(),
        palette.color(Token::Ink),
    );
    if let Some(hint) = hint {
        painter.text(
            rect.right_center(),
            Align2::RIGHT_CENTER,
            hint,
            CompactStyle::Supporting.font_id(),
            palette.color(Token::Quiet),
        );
    }
    response
}
