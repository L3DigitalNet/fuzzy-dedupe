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
//! Settings gear, which both workspaces share. [`toolbar_row`] reserves the
//! shared items' space first and hands each workspace toolbar the rest as
//! its whole `ui.available_width()` budget. Nothing here calls a service:
//! every action is a pushed [`UiIntent`].

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
use crate::ui::widgets::{grouped_table, status};
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
/// The region [`toolbar_row`] hands to the workspace toolbar.
pub const WORKSPACE_TOOLBAR_NAME: &str = "Workspace toolbar";

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
    let queued = state.queue.queued_count();
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
            let workspace_min = match workspace {
                WorkspaceState::Findings => findings::toolbar_min_width(ui, &cx),
                WorkspaceState::Compare => compare::toolbar_min_width(ui, &cx),
            };
            toolbar_row(
                ui,
                &review_label,
                queued,
                workspace_min,
                intents,
                |ui, intents| match workspace {
                    WorkspaceState::Findings => findings::toolbar(ui, &mut cx, intents),
                    WorkspaceState::Compare => compare::toolbar(ui, &mut cx, intents),
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
        // The current workspace is never disabled: Compare reached by
        // Ctrl+2 without a pair explains itself in its body.
        let available = workspace != WorkspaceState::Compare || compare_available(state);
        let response = rail_control(
            ui,
            next(NAV_ITEM_HEIGHT),
            Id::unique(("fd.rail.nav", label)),
            if selected {
                RailLook::Selected
            } else if available {
                RailLook::Plain
            } else {
                RailLook::Disabled
            },
            Some(icon),
            if collapsed { "" } else { label },
            label,
            palette,
        );
        let response = response.on_disabled_hover_text(NO_PAIR_EXPLANATION);
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

/// Whether the rail's Compare entry has a pair to show (spec §11.2): the
/// focused Findings row is a member with a counterpart, or the last pair is
/// still a pair of these results.
///
/// Mirrors what `presentation/commands.rs` `switch_to_compare` and the
/// Compare view do with the entry's intent: the first condition is the pair
/// `switch_to_compare` opens, the second the last pair Compare can show
/// (`projections::compare_view`). If either rule changes there, change it
/// here, or the entry is enabled onto the no-pair state or disabled with a
/// pair to show.
fn compare_available(state: &PresentationState) -> bool {
    let Some(results) = state.scan.results.as_ref() else {
        return false;
    };
    let focused = state
        .findings
        .focused
        .and_then(|row| grouped_table::member_at(results, &state.queue, row))
        .is_some_and(|(_, view)| view.counterpart.is_some());
    focused
        || state
            .compare
            .pair
            .is_some_and(|pair| projections::compare_view(results, &state.queue, pair).is_some())
}

/// The disabled rail Compare entry's explanation, in the words of
/// Compare's own no-pair state.
const NO_PAIR_EXPLANATION: &str =
    "No pair to compare. In Findings, focus a file and press Enter or choose Compare.";

#[derive(Clone, Copy, PartialEq, Eq)]
enum RailLook {
    /// The bordered New scan action.
    Action,
    /// The current workspace, or the Menu entry while its menu is open.
    Selected,
    Plain,
    /// Not clickable or focusable; drawn in the quiet ink.
    Disabled,
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
    let enabled = look != RailLook::Disabled;
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let response = ui.interact(rect, id, sense);
    let name = accessible_name.to_owned();
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, &name));
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
        RailLook::Disabled => (
            Color32::TRANSPARENT,
            Stroke::NONE,
            palette.color(Token::Quiet),
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

/// The toolbar row (spec §11.3): the workspace toolbar on the left, then
/// Review cleanup (N) and the Settings gear, which both workspaces share.
///
/// Contract with `findings::toolbar` and `compare::toolbar`: the shared
/// items are measured and their rect reserved first, and `workspace_toolbar`
/// then draws into a child `Ui` whose `max_rect` ends one item spacing
/// before them. Inside it `ui.available_width()` is the workspace's whole
/// budget: nothing else is subtracted, so a workspace that lays its items
/// out within that width never reaches the shared items, and one that
/// overruns it overprints them. Overflowing secondary actions into a menu
/// (§11.3 collapse step 3) is therefore each workspace's job, against this
/// budget. The budget's region carries the accessible name
/// [`WORKSPACE_TOOLBAR_NAME`], so tests can see where it ends.
///
/// The Review cleanup label: `workspace_min` is the narrowest budget in
/// which the active workspace shows every toolbar item with nothing moved
/// into its overflow menu (`findings::toolbar_min_width`, with search at its
/// minimum; `compare::toolbar_min_width`). The full "Review cleanup (N)"
/// shows when that budget remains beside it; otherwise the label shortens
/// to "Cleanup (N)", so the shell gives up label text before a workspace
/// gives up an action. Frame `29:2214` keeps the full label in Findings at
/// 1024 × 600 (its search narrows instead); frame `29:3648` shortens it in
/// Compare at the same size. Widths are in points, so zoom takes part, and
/// at 200% whichever label fits wins. The minimum is measured before the
/// workspace draws and never from the budget it is then given, so the
/// choice cannot oscillate between frames. When even the short label leaves
/// less than `workspace_min`, the workspace collapses into its own menu
/// against the smaller budget. Rejected: a fixed row-width threshold, which
/// shortened the label in Findings at 1024 × 600 where the frame keeps it.
///
/// The shared items are drawn after the workspace toolbar, not before,
/// although their space is taken first: egui's Tab order is drawing order,
/// and keyboard focus must reach the workspace's controls before the
/// shared ones, left to right as they appear. Rejected: each workspace
/// measuring the shared items itself, which duplicates their width rule in
/// both workspaces and cannot follow the short label.
///
/// `review_label` is the full "Review cleanup (N)" and `queued` its N.
pub fn toolbar_row(
    ui: &mut Ui,
    review_label: &str,
    queued: usize,
    workspace_min: f32,
    intents: &mut IntentQueue,
    workspace_toolbar: impl FnOnce(&mut Ui, &mut IntentQueue),
) {
    let palette = theme::palette(mode_of(ui.ctx()));
    let (_, row) = ui.allocate_space(Vec2::new(ui.available_width(), TOOLBAR_HEIGHT));
    let spacing = ui.spacing().item_spacing.x;

    let review_width = |ui: &Ui, label: &str| {
        ui.painter()
            .layout_no_wrap(
                label.to_owned(),
                CompactStyle::ControlLabel.font_id(),
                palette.color(Token::White),
            )
            .size()
            .x
            + 2.0 * ui.spacing().button_padding.x
    };
    // The budget a shared-items width leaves the workspace: the row less
    // the gear, Review cleanup, the gap between them and the gap before.
    let budget_beside = |review: f32| row.width() - (CONTROL_HEIGHT + spacing + review) - spacing;
    let full_width = review_width(ui, review_label);
    let short_label;
    let (label, review_width) = if budget_beside(full_width) >= workspace_min {
        (review_label, full_width)
    } else {
        short_label = format!("Cleanup ({queued})");
        let width = review_width(ui, &short_label);
        (short_label.as_str(), width)
    };
    let shared_width = CONTROL_HEIGHT + spacing + review_width;
    let shared = Rect::from_min_max(
        egui::pos2(row.right() - shared_width, row.top()),
        row.right_bottom(),
    );
    let budget = Rect::from_min_max(
        row.left_top(),
        egui::pos2((shared.left() - spacing).max(row.left()), row.bottom()),
    );

    let region = ui.interact(budget, Id::unique("fd.toolbar.workspace"), Sense::hover());
    region
        .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Group, true, WORKSPACE_TOOLBAR_NAME));
    let mut workspace = ui.new_child(
        egui::UiBuilder::new()
            .id_salt("fd.toolbar.workspace")
            .max_rect(budget)
            .layout(Layout::left_to_right(Align::Center)),
    );
    workspace_toolbar(&mut workspace, intents);

    let mut items = ui.new_child(
        egui::UiBuilder::new()
            .id_salt("fd.toolbar.shared")
            .max_rect(shared)
            .layout(Layout::right_to_left(Align::Center)),
    );
    shared_toolbar_items(
        &mut items,
        label,
        review_label,
        review_width,
        queued,
        intents,
        palette,
    );
}

/// The Settings gear then Review cleanup, right to left, each sized exactly
/// as [`toolbar_row`] measured it so the reserved rect holds them.
///
/// Review cleanup keeps the full "Review cleanup (N)" as its accessible
/// name whatever it shows, so assistive tooling and tests find one name;
/// the short visible label is contained in it. It is disabled while the
/// queue is empty (frame `28:1399`); the Ctrl+Enter chord is unaffected.
fn shared_toolbar_items(
    ui: &mut Ui,
    label: &str,
    accessible_name: &str,
    width: f32,
    queued: usize,
    intents: &mut IntentQueue,
    palette: &Palette,
) {
    // The gear is exactly CONTROL_HEIGHT square, the width `toolbar_row`
    // reserved for it. With egui's button padding the 18-unit glyph asks for
    // 34, `add_sized` grows to fit, and the right-to-left layout then pushed
    // Review cleanup 4 units into the gap before it, leaving 4 between the
    // workspace's last control and Review cleanup instead of 8.
    let settings = ui
        .scope(|ui| {
            ui.spacing_mut().button_padding = Vec2::ZERO;
            ui.add_sized(
                Vec2::splat(CONTROL_HEIGHT),
                egui::Button::new(
                    egui::RichText::new(glyphs::text(ui.ctx(), Icon::Settings).unwrap_or_default())
                        .size(ICON_SIZE),
                ),
            )
        })
        .inner;
    settings.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, SETTINGS_NAME));
    let settings = settings.on_hover_text("Matching rules (Ctrl+,)");
    if settings.clicked() {
        intents.push(UiIntent::OpenDialog {
            subject: DialogSubject::None,
            kind: DialogKind::MatchingRules,
            invoker: Some(settings.id),
        });
    }

    let enabled = queued > 0;
    let review = ui
        .add_enabled_ui(enabled, |ui| {
            ui.add_sized(
                Vec2::new(width, CONTROL_HEIGHT),
                egui::Button::new(
                    egui::RichText::new(label)
                        .text_style(CompactStyle::ControlLabel.text_style())
                        .color(palette.color(Token::White)),
                )
                .fill(palette.color(Token::Primary)),
            )
        })
        .inner;
    let name = accessible_name.to_owned();
    review.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, &name));
    let review = review
        .on_hover_text("Review cleanup (Ctrl+Enter)")
        .on_disabled_hover_text("Nothing is queued for cleanup.");
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
                // Frame `44:3954` stacks these rows at a 28-unit pitch;
                // egui's `item_spacing.y` after each 28-high row would make
                // it 31. Zeroed for this run of rows only, so the separators
                // and the Appearance and Zoom rows keep the theme's spacing.
                ui.scope(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
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
                });
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
