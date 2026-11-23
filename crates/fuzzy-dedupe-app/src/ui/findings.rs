// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! The Findings workspace (spec §11.2, §11.4): search, Actions ▾, Sort ▾,
//! Compare, relationship filters and the grouped table, plus the table
//! region's states: before the first scan (01a), scanning (01b), discovery
//! problems (01c), no rows matching the search (01e) and a scan that found
//! no relationships. [`toolbar`] and [`show`] are the entry points.
//!
//! The shell (`ui/shell.rs`) draws the toolbar row and its shared trailing
//! items (Review cleanup (N), Settings); [`toolbar`] fills the leading part
//! of that row, left to right.
//!
//! Menus. The dialog host owns the one overlay slot (C-12) and the shell
//! draws only the main menu, so this view draws its own three menus
//! (Actions ▾, Sort ▾, the row menu) whenever `WorkspaceCx::menu` names one
//! of them. That field is the only source of which menu is open; the view
//! keeps no copy, so a menu the host closes or swaps is drawn closed or
//! swapped on the next frame. What the slot cannot hold (a row menu's
//! anchor, whether the menu has drawn yet, Choose columns… expanded) is
//! [`MenuLocal`], egui temporary data that resets whenever the slot holds
//! no Findings menu. Every menu item first closes the overlay, then sends
//! its own intent, so the dialog host's focus return and a following popup
//! see a closed menu.
//!
//! Interpretations recorded for review:
//! - 01c places the discovery-problem notice in the status bar, which is
//!   T12's file; here it sits at the end of the filter row instead.
//! - No frame shows the empty-scan state; its text is this file's.

use egui::{Align, Align2, Id, Layout, Sense, Stroke, StrokeKind, Ui, Vec2};
use fuzzy_dedupe_domain::{Coverage, MemberDecision, QueueOrigin, QueueScope, RelationKind};
use fuzzy_dedupe_engine::service::{EngineRequest, ReviewCommand};

use crate::presentation::commands::{IntentQueue, UiIntent};
use crate::presentation::projections::{self, RELATION_KINDS, SORT_ITEMS};
use crate::presentation::state::{
    DialogKind, DialogSubject, MenuKind, RowId, ScanStage, UniqueTextSubject,
};
use crate::ui::WorkspaceCx;
use crate::ui::shell::{CONTROL_HEIGHT, GAP};
use crate::ui::theme::{self, CompactStyle, Icon, Palette, Token};
use crate::ui::widgets::dialog_host::mode_of;
use crate::ui::widgets::grouped_table::{self, icon_text};

/// The search field's width (spec §11.3).
pub const SEARCH_WIDTH: f32 = 340.0;
/// The Compare toolbar action's width (frame `13:711`).
pub const COMPARE_WIDTH: f32 = 100.0;
/// Filter row and chip heights (spec §11.3).
pub const FILTER_ROW_HEIGHT: f32 = 28.0;
pub const CHIP_HEIGHT: f32 = 26.0;
/// The filter row's trailing statement (spec §11.4).
pub const MEASURED_NOTE: &str = "Match and Unique are measured against the file each row names";
/// Accessible names of the toolbar controls.
pub const SEARCH_NAME: &str = "Search file names or folders";
pub const CLEAR_SEARCH_NAME: &str = "Clear search";
pub const ACTIONS_NAME: &str = "Actions ▾";
pub const COMPARE_NAME: &str = "Compare";
/// The Actions ▾ items, in order (spec §11.2).
pub const ACTIONS_ITEMS: [&str; 4] = [
    "Queue identical copies…",
    "Expand all groups",
    "Collapse all groups",
    "Choose columns…",
];
/// The row-menu items after the first, in order (spec §11.4); the first is
/// [`projections::compare_item_label`].
pub const ROW_MENU_ITEMS: [&str; 6] = [
    "Review unique text…",
    "Keep — do not queue",
    "Make this the reference",
    "Protect this file",
    "Open containing folder",
    "Copy path",
];

const MENU_WIDTH: f32 = grouped_table::ROW_MENU_WIDTH;
const MENU_ITEM_HEIGHT: f32 = 28.0;

#[must_use]
pub fn search_id() -> Id {
    Id::unique("fd.findings.search")
}

#[must_use]
pub fn actions_button_id() -> Id {
    Id::unique("fd.findings.actions")
}

#[must_use]
pub fn sort_button_id() -> Id {
    Id::unique("fd.findings.sort")
}

/// "Sort: Match type".
#[must_use]
pub fn sort_button_label(cx: &WorkspaceCx<'_>) -> String {
    format!("Sort: {}", projections::sort_label(cx.findings.sort))
}

/// Findings-local menu sub-state: what the overlay slot does not hold.
///
/// It lives in egui temporary data under [`menu_local_id`], not in
/// `FindingsState`, and is tagged with the menu it describes. It is used
/// only while `WorkspaceCx::menu` names that same menu, and is dropped
/// whenever the slot holds no Findings menu, so a closed, refused or
/// host-swapped menu never lends its anchor or expansion to the next one.
#[derive(Clone, Copy, Debug, PartialEq)]
struct MenuLocal {
    /// The menu this sub-state belongs to. A tag only: which menu is open
    /// is always read from `WorkspaceCx::menu`.
    owner: MenuKind,
    /// Where a row menu opens (the pointer, or below the focused row for
    /// the keyboard). Toolbar menus anchor to their button and need none.
    anchor: Option<egui::Pos2>,
    /// The pass that wrote this sub-state. A request is dispatched after
    /// its pass, so during that pass the slot does not hold the menu yet
    /// and the sub-state must survive the reset in [`sync_menu`].
    written_pass: u64,
    /// The menu has drawn: its first item took keyboard focus.
    drawn: bool,
    /// Choose columns… is expanded inside Actions ▾.
    choosing_columns: bool,
    /// An item closed the menu while drawing; the sub-state is dropped.
    closed: bool,
}

impl MenuLocal {
    fn new(owner: MenuKind, anchor: Option<egui::Pos2>, pass: u64) -> Self {
        Self {
            owner,
            anchor,
            written_pass: pass,
            drawn: false,
            choosing_columns: false,
            closed: false,
        }
    }
}

fn menu_local_id() -> Id {
    Id::unique("fd.findings.menu_local")
}

/// Whether `menu` is one this view draws.
const fn is_findings_menu(menu: Option<MenuKind>) -> bool {
    matches!(
        menu,
        Some(MenuKind::Actions | MenuKind::Sort | MenuKind::Row(_))
    )
}

/// Drops the menu sub-state whenever the slot holds no Findings menu,
/// except sub-state a request wrote in this very pass.
fn sync_menu(ui: &Ui, cx: &WorkspaceCx<'_>) {
    if is_findings_menu(cx.menu) {
        return;
    }
    let pass = ui.ctx().cumulative_pass_nr();
    ui.ctx().data_mut(|d| {
        if d.get_temp::<MenuLocal>(menu_local_id())
            .is_some_and(|local| local.written_pass < pass)
        {
            d.remove::<MenuLocal>(menu_local_id());
        }
    });
}

/// Asks the dialog host for menu `kind`, invoked by `invoker`. The host
/// opens it when the intent is dispatched, after this pass, or refuses it
/// when the slot is taken; either way the slot alone decides what draws.
fn open_menu(
    ctx: &egui::Context,
    intents: &mut IntentQueue,
    kind: MenuKind,
    anchor: Option<egui::Pos2>,
    invoker: Id,
) {
    let local = MenuLocal::new(kind, anchor, ctx.cumulative_pass_nr());
    ctx.data_mut(|d| d.insert_temp(menu_local_id(), local));
    intents.push(UiIntent::ToggleMenu {
        kind,
        invoker: Some(invoker),
    });
}

/// Closes the open Findings menu: the overlay, and its sub-state with it.
fn close_menu(local: &mut MenuLocal, intents: &mut IntentQueue) {
    local.closed = true;
    intents.push(UiIntent::CloseOverlay);
}

/// A toolbar button whose accessible name is `name`; `text` is what it
/// draws, which may hold an icon glyph in place of a character the fonts
/// lack ("▾" is not in IBM Plex Sans).
fn button(ui: &mut Ui, text: &str, name: &str, id: Id, enabled: bool) -> egui::Response {
    let drawn = ui.add_enabled(
        enabled,
        egui::Button::new(
            egui::RichText::new(text).text_style(CompactStyle::ControlLabel.text_style()),
        )
        .min_size(Vec2::new(0.0, CONTROL_HEIGHT)),
    );
    drawn.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, name));
    // A fixed ID on top of the drawn button: the button is its menu's
    // invoker, which focus returns to, so its ID must be known and stable.
    // It carries no widget info of its own, so the button is named once.
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    ui.interact(drawn.rect, id, sense).union(drawn)
}

/// The search field with its clear control (spec §11.4: search filters
/// file names and folders and has a clear control when populated).
fn search_field(ui: &mut Ui, cx: &mut WorkspaceCx<'_>, palette: &Palette) {
    // Ctrl+F focuses the search; Esc in the search clears it (§11.10).
    if cx.input_enabled && ui.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::F)) {
        ui.memory_mut(|m| m.request_focus(search_id()));
    }
    let search_focused = ui.memory(|m| m.has_focus(search_id()));
    if cx.input_enabled
        && search_focused
        && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
    {
        cx.findings.search.clear();
    }

    let edit = egui::TextEdit::singleline(&mut cx.findings.search)
        .id(search_id())
        .hint_text(SEARCH_NAME)
        .font(CompactStyle::Table.text_style())
        .desired_width(SEARCH_WIDTH - 30.0)
        .min_size(Vec2::new(0.0, CONTROL_HEIGHT))
        .interactive(cx.input_enabled);
    let response = ui.add(edit);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, SEARCH_NAME));
    if cx.findings.search.is_empty() {
        ui.add_sized(
            [24.0, CONTROL_HEIGHT],
            egui::Label::new(
                egui::RichText::new(icon_text(ui.ctx(), Icon::Search))
                    .color(palette.color(Token::Muted)),
            ),
        );
    } else {
        let clear = ui.add_sized([24.0, CONTROL_HEIGHT], egui::Button::new("×").frame(false));
        clear
            .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, CLEAR_SEARCH_NAME));
        if cx.input_enabled && clear.clicked() {
            cx.findings.search.clear();
        }
    }
}

/// The workspace's own toolbar items, before the shared ones.
pub fn toolbar(ui: &mut egui::Ui, cx: &mut WorkspaceCx<'_>, intents: &mut IntentQueue) {
    sync_menu(ui, cx);
    grouped_table::ensure_glyphs(ui.ctx());
    let palette = theme::palette(mode_of(ui.ctx()));

    search_field(ui, cx, palette);

    let has_results = cx.scan.results.is_some();
    let actions_text = format!("Actions {}", icon_text(ui.ctx(), Icon::Expanded));
    let actions = button(
        ui,
        &actions_text,
        ACTIONS_NAME,
        actions_button_id(),
        has_results,
    );
    if cx.input_enabled && actions.clicked() {
        open_menu(
            ui.ctx(),
            intents,
            MenuKind::Actions,
            None,
            actions_button_id(),
        );
    }
    let sort_label = sort_button_label(cx);
    let sort = button(ui, &sort_label, &sort_label, sort_button_id(), has_results);
    if cx.input_enabled && sort.clicked() {
        open_menu(ui.ctx(), intents, MenuKind::Sort, None, sort_button_id());
    }

    // Compare sits right before the shell's shared items; their width is
    // the gear plus Review cleanup (N) laid out at the control font.
    let review = format!("Review cleanup ({})", cx.queue.queued_count());
    let review_width = ui
        .painter()
        .layout_no_wrap(
            review,
            CompactStyle::ControlLabel.font_id(),
            palette.color(Token::White),
        )
        .size()
        .x
        + 2.0 * ui.spacing().button_padding.x;
    let spacing = ui.spacing().item_spacing.x;
    let shared = CONTROL_HEIGHT + review_width + 2.0 * spacing;
    let room = ui.available_width() - shared - COMPARE_WIDTH - spacing;
    if room > 0.0 {
        ui.add_space(room);
    }
    let target = compare_target(cx);
    let compare = ui
        .add_enabled_ui(target.is_some(), |ui| {
            ui.add_sized(
                [COMPARE_WIDTH, CONTROL_HEIGHT],
                egui::Button::new(
                    egui::RichText::new(COMPARE_NAME)
                        .text_style(CompactStyle::ControlLabel.text_style()),
                ),
            )
        })
        .inner;
    let compare = if target.is_some() {
        compare.on_hover_text("Compare the focused row against its counterpart (Enter)")
    } else {
        compare.on_disabled_hover_text("Focus a member row to compare it against its counterpart")
    };
    if cx.input_enabled
        && compare.clicked()
        && let Some((row, file, counterpart)) = target
    {
        intents.push(UiIntent::OpenCompare {
            target: file,
            counterpart,
            return_row: Some(row),
        });
    }
}

/// The focused member row, its file and its counterpart, if it has one.
fn compare_target(
    cx: &WorkspaceCx<'_>,
) -> Option<(
    RowId,
    fuzzy_dedupe_domain::FileId,
    fuzzy_dedupe_domain::FileId,
)> {
    let results = cx.scan.results.as_ref()?;
    let row = cx.findings.focused?;
    let (file, view) = grouped_table::member_at(results, cx.queue, row)?;
    Some((row, file, view.counterpart?))
}

/// The workspace body below the toolbar: filter row and table.
pub fn show(ui: &mut egui::Ui, cx: &mut WorkspaceCx<'_>, intents: &mut IntentQueue) {
    sync_menu(ui, cx);
    grouped_table::ensure_glyphs(ui.ctx());
    let palette = theme::palette(mode_of(ui.ctx()));
    cx.findings.last_compared = cx.compare.pair.map(|pair| pair.group);
    let projection = grouped_table::refresh_projection(cx.findings, cx.scan, cx.queue);

    filter_row(ui, cx, projection.as_deref(), palette);
    if cx.findings.show_issues && !cx.scan.issues.is_empty() {
        issues_list(ui, cx, palette);
    }
    ui.add_space(GAP);

    let running = matches!(cx.scan.stage, ScanStage::Running | ScanStage::Stopping);
    match (&cx.scan.results, projection.as_deref()) {
        _ if running => scanning(ui, cx, intents, palette),
        (None, _) => {
            if message(
                ui,
                palette,
                "No findings yet",
                "Scan folders to find identical and near-identical text files. Scanning never changes files.",
                Some("Choose folders to scan"),
            ) && cx.input_enabled
            {
                intents.push(UiIntent::OpenDialog {
                    kind: DialogKind::NewScan,
                    subject: DialogSubject::None,
                    invoker: Some(message_action_id()),
                });
            }
        }
        (Some(results), _) if results.groups.is_empty() || results.coverage == Coverage::Empty => {
            if message(
                ui,
                palette,
                "No relationships found",
                "The scan finished and found no identical or similar text files.",
                Some("New scan"),
            ) && cx.input_enabled
            {
                intents.push(UiIntent::OpenDialog {
                    kind: DialogKind::NewScan,
                    subject: DialogSubject::None,
                    invoker: Some(message_action_id()),
                });
            }
        }
        (Some(results), Some(projection)) if projection.rows.is_empty() => {
            let search = cx.findings.search.trim().to_owned();
            let hidden = results.groups.len();
            let (title, body) = if search.is_empty() {
                (
                    "No groups match this filter".to_owned(),
                    format!(
                        "{} groups are hidden.",
                        projections::thousands(hidden as u64)
                    ),
                )
            } else {
                (
                    format!("No files match “{search}”"),
                    format!(
                        "No file name or folder in this scan contains that text. {} groups are hidden.",
                        projections::thousands(hidden as u64)
                    ),
                )
            };
            if message(ui, palette, &title, &body, Some("Clear search and filters"))
                && cx.input_enabled
            {
                cx.findings.search.clear();
                cx.findings.match_filter = None;
            }
        }
        (Some(_), _) => {
            let row_menu = grouped_table::show(
                ui,
                cx.findings,
                cx.scan,
                cx.queue,
                intents,
                cx.input_enabled,
            );
            if let Some(request) = row_menu {
                open_menu(
                    ui.ctx(),
                    intents,
                    MenuKind::Row(request.row),
                    Some(request.at),
                    request.invoker,
                );
            }
        }
    }

    if cx.preferences.findings_column_widths != cx.findings.column_widths {
        cx.preferences
            .findings_column_widths
            .clone_from(&cx.findings.column_widths);
    }

    draw_menu(ui, cx, intents, palette);
}

fn message_action_id() -> Id {
    Id::unique("fd.findings.state_action")
}

/// The filter row: one chip per relation type with its count, then the
/// discovery-problem notice and the measurement statement at the end.
fn filter_row(
    ui: &mut Ui,
    cx: &mut WorkspaceCx<'_>,
    projection: Option<&projections::FindingsProjection>,
    palette: &Palette,
) {
    ui.allocate_ui_with_layout(
        Vec2::new(ui.available_width(), FILTER_ROW_HEIGHT),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let counts = projection.map(|p| p.counts);
            let mut chip = |ui: &mut Ui, label: String, filter: Option<RelationKind>| {
                let selected = cx.findings.match_filter == filter;
                let text = egui::RichText::new(&label)
                    .text_style(CompactStyle::ControlLabel.text_style())
                    .color(palette.color(if selected { Token::White } else { Token::Ink }));
                let mut chip = egui::Button::new(text).min_size(Vec2::new(0.0, CHIP_HEIGHT));
                chip = if selected {
                    chip.fill(palette.color(Token::Primary))
                } else {
                    chip.frame(false)
                };
                let response = ui.add(chip);
                response.widget_info(|| {
                    egui::WidgetInfo::selected(egui::Role::Button, true, selected, &label)
                });
                if cx.input_enabled && response.clicked() {
                    cx.findings.match_filter = filter;
                }
            };
            chip(
                ui,
                counts.map_or_else(
                    || "All".to_owned(),
                    |c| format!("All {}", projections::thousands(c.all as u64)),
                ),
                None,
            );
            for kind in RELATION_KINDS {
                let name = projections::kind_label(kind);
                let label = counts.map_or_else(
                    || name.to_owned(),
                    |c| format!("{name} {}", projections::thousands(c.of(kind) as u64)),
                );
                chip(ui, label, Some(kind));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(MEASURED_NOTE)
                        .text_style(CompactStyle::Supporting.text_style())
                        .color(palette.color(Token::Muted)),
                );
                let issues = cx.scan.issues.len();
                if issues > 0 {
                    let toggle = if cx.findings.show_issues {
                        "Hide details"
                    } else {
                        "Show details"
                    };
                    let link = ui.link(
                        egui::RichText::new(toggle)
                            .text_style(CompactStyle::Supporting.text_style()),
                    );
                    if cx.input_enabled && link.clicked() {
                        cx.findings.show_issues = !cx.findings.show_issues;
                    }
                    let files = if issues == 1 { "file" } else { "files" };
                    ui.label(
                        egui::RichText::new(format!(
                            "{} {} {files} could not be read",
                            icon_text(ui.ctx(), Icon::Alert),
                            projections::thousands(issues as u64)
                        ))
                        .text_style(CompactStyle::Supporting.text_style())
                        .color(palette.color(Token::Amber)),
                    );
                }
            });
        },
    );
}

/// The discovery problems, kept accessible after the scan (spec §11.6).
fn issues_list(ui: &mut Ui, cx: &WorkspaceCx<'_>, palette: &Palette) {
    egui::Frame::new()
        .fill(palette.color(Token::AmberSoft))
        .corner_radius(theme::CORNER_RADIUS)
        .inner_margin(egui::Margin::symmetric(8, 4))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::ScrollArea::vertical()
                .max_height(6.0 * 18.0)
                .show(ui, |ui| {
                    for issue in &cx.scan.issues {
                        ui.label(
                            egui::RichText::new(format!(
                                "{}  {}",
                                issue_text(issue.kind),
                                issue.display_path
                            ))
                            .text_style(CompactStyle::Supporting.text_style())
                            .color(palette.color(Token::Ink)),
                        );
                    }
                });
        });
}

/// One discovery problem's reason (FR-006).
#[must_use]
pub const fn issue_text(kind: fuzzy_dedupe_domain::IssueKind) -> &'static str {
    use fuzzy_dedupe_domain::IssueKind;
    match kind {
        IssueKind::PermissionDenied => "Permission denied:",
        IssueKind::ReadFailed => "Could not be read:",
        IssueKind::DecodingFailed => "Not decodable as text:",
        IssueKind::ChangedDuringRead => "Changed while being read:",
        IssueKind::Vanished => "Disappeared before it was read:",
        IssueKind::MissingFolder => "Folder missing:",
    }
}

/// A centered state message in the table region, with an optional primary
/// action; returns whether the action was clicked.
fn message(ui: &mut Ui, palette: &Palette, title: &str, body: &str, action: Option<&str>) -> bool {
    let rect = ui.available_rect_before_wrap();
    ui.painter().rect(
        rect,
        theme::CORNER_RADIUS,
        palette.color(Token::Surface),
        Stroke::new(theme::BORDER_WIDTH, palette.color(Token::Line)),
        StrokeKind::Inside,
    );
    let mut clicked = false;
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
        ui.with_layout(Layout::top_down(Align::Center), |ui| {
            ui.add_space((rect.height() / 2.0 - 60.0).max(0.0));
            ui.label(
                egui::RichText::new(title)
                    .text_style(CompactStyle::Title.text_style())
                    .color(palette.color(Token::Ink)),
            );
            ui.label(
                egui::RichText::new(body)
                    .text_style(CompactStyle::Body.text_style())
                    .color(palette.color(Token::Muted)),
            );
            if let Some(action) = action {
                ui.add_space(4.0);
                let response = ui.add(
                    egui::Button::new(
                        egui::RichText::new(action)
                            .text_style(CompactStyle::ControlLabel.text_style())
                            .color(palette.color(Token::White)),
                    )
                    .fill(palette.color(Token::Primary))
                    .min_size(Vec2::new(0.0, CONTROL_HEIGHT)),
                );
                let response = ui
                    .interact(response.rect, message_action_id(), Sense::click())
                    .union(response);
                clicked = response.clicked();
            }
        });
    });
    ui.allocate_rect(rect, Sense::hover());
    clicked
}

/// Frame 01b: the folder being read, the counts, elapsed time and Stop
/// scan, in the table region.
fn scanning(ui: &mut Ui, cx: &WorkspaceCx<'_>, intents: &mut IntentQueue, palette: &Palette) {
    let rect = ui.available_rect_before_wrap();
    ui.painter().rect(
        rect,
        theme::CORNER_RADIUS,
        palette.color(Token::Surface),
        Stroke::new(theme::BORDER_WIDTH, palette.color(Token::Line)),
        StrokeKind::Inside,
    );
    let stopping = cx.scan.stage == ScanStage::Stopping;
    let progress = cx.scan.progress.as_ref();
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
        ui.with_layout(Layout::top_down(Align::Center), |ui| {
            ui.add_space((rect.height() / 2.0 - 70.0).max(0.0));
            ui.label(
                egui::RichText::new(if stopping {
                    "Stopping…"
                } else {
                    "Scanning…"
                })
                .text_style(CompactStyle::Title.text_style())
                .color(palette.color(Token::Ink)),
            );
            if let Some(progress) = progress {
                ui.label(
                    egui::RichText::new(format!("Reading files in {}", progress.current_folder))
                        .text_style(CompactStyle::PathDetail.text_style())
                        .color(palette.color(Token::Muted)),
                );
            }
            ui.add(
                egui::ProgressBar::new(0.0)
                    .animate(!stopping)
                    .desired_width(360.0)
                    .desired_height(6.0),
            );
            let (files, groups, elapsed) = progress.map_or((0, 0, 0), |p| {
                (p.files_read, p.groups_found, p.elapsed.as_secs())
            });
            ui.label(
                egui::RichText::new(format!(
                    "{} files read · {} groups so far · {}:{:02} elapsed",
                    projections::thousands(files),
                    projections::thousands(groups),
                    elapsed / 60,
                    elapsed % 60
                ))
                .text_style(CompactStyle::Supporting.text_style())
                .color(palette.color(Token::Muted)),
            );
            let stop = ui.add_enabled(
                !stopping && cx.input_enabled,
                egui::Button::new(
                    egui::RichText::new("Stop scan")
                        .text_style(CompactStyle::ControlLabel.text_style()),
                )
                .min_size(Vec2::new(0.0, CONTROL_HEIGHT)),
            );
            if stop.clicked()
                && let Some(scan) = cx.scan.scan
            {
                intents.push(UiIntent::Submit(EngineRequest::CancelScan(scan)));
            }
        });
    });
    ui.allocate_rect(rect, Sense::hover());
}

/// A 28-high menu row with an optional trailing shortcut hint.
fn menu_item(
    ui: &mut Ui,
    label: &str,
    hint: Option<&str>,
    enabled: bool,
    palette: &Palette,
) -> egui::Response {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), MENU_ITEM_HEIGHT), sense);
    let name = label.to_owned();
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, &name));
    let painter = ui.painter();
    if enabled && (response.hovered() || response.has_focus()) {
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
    let ink = palette.color(if enabled { Token::Ink } else { Token::Quiet });
    painter.text(
        rect.left_center() + Vec2::new(10.0, 0.0),
        Align2::LEFT_CENTER,
        label,
        CompactStyle::Table.font_id(),
        ink,
    );
    if let Some(hint) = hint {
        painter.text(
            rect.right_center() - Vec2::new(10.0, 0.0),
            Align2::RIGHT_CENTER,
            hint,
            CompactStyle::Supporting.font_id(),
            palette.color(Token::Quiet),
        );
    }
    response
}

/// Where menu `kind` opens when its sub-state names no anchor: below its
/// toolbar button (drawn earlier this pass), or for a row menu the host
/// opened itself, at the table's top-left corner.
fn default_anchor(ctx: &egui::Context, kind: MenuKind) -> egui::Pos2 {
    let below = |id| {
        ctx.read_response(id)
            .map(|r| r.rect.left_bottom() + Vec2::new(0.0, 2.0))
    };
    match kind {
        MenuKind::Actions => below(actions_button_id()),
        MenuKind::Sort => below(sort_button_id()),
        _ => ctx
            .read_response(grouped_table::table_id())
            .map(|r| r.rect.left_top()),
    }
    .unwrap_or(egui::Pos2::ZERO)
}

/// Draws the Findings menu the overlay slot holds, if it holds one.
fn draw_menu(ui: &Ui, cx: &mut WorkspaceCx<'_>, intents: &mut IntentQueue, palette: &Palette) {
    let ctx = ui.ctx().clone();
    let Some(kind) = cx.menu.filter(|_| is_findings_menu(cx.menu)) else {
        return;
    };
    let pass = ctx.cumulative_pass_nr();
    let mut local = ctx
        .data(|d| d.get_temp::<MenuLocal>(menu_local_id()))
        .filter(|local| local.owner == kind)
        .unwrap_or_else(|| MenuLocal::new(kind, None, pass));
    let anchor = local.anchor.unwrap_or_else(|| default_anchor(&ctx, kind));
    let frame = egui::Frame::new()
        .fill(palette.color(Token::Surface))
        .stroke(Stroke::new(theme::BORDER_WIDTH, palette.color(Token::Line)))
        .corner_radius(theme::CORNER_RADIUS)
        .shadow(theme::shadow(mode_of(&ctx)))
        .inner_margin(egui::Margin::symmetric(1, 4));
    let mut first = !local.drawn;
    let mut focus_first = |response: &egui::Response| {
        if std::mem::take(&mut first) {
            response.request_focus();
        }
    };
    let area = egui::Area::new(Id::unique("fd.findings.menu"))
        .order(egui::Order::Foreground)
        .constrain(true)
        .fixed_pos(anchor)
        .show(&ctx, |ui| {
            frame.show(ui, |ui| {
                ui.set_width(MENU_WIDTH - 2.0);
                ui.spacing_mut().item_spacing.y = 0.0;
                match kind {
                    MenuKind::Sort => {
                        sort_menu(ui, cx, intents, palette, &mut local, &mut focus_first);
                    }
                    MenuKind::Actions => {
                        actions_menu(ui, cx, intents, palette, &mut local, &mut focus_first);
                    }
                    MenuKind::Row(row) => {
                        row_menu(ui, cx, intents, palette, row, &mut local, &mut focus_first);
                    }
                    MenuKind::Main | MenuKind::OnlyIn { .. } => {}
                }
            });
        });
    local.drawn = true;
    // A press outside the menu closes it, as a menu does.
    let pressed_outside = ctx.input(|i| {
        i.pointer.any_pressed()
            && i.pointer
                .interact_pos()
                .is_some_and(|pos| !area.response.rect.contains(pos))
    });
    if pressed_outside && !local.closed {
        close_menu(&mut local, intents);
    }
    ctx.data_mut(|d| {
        if local.closed {
            d.remove::<MenuLocal>(menu_local_id());
        } else {
            d.insert_temp(menu_local_id(), local);
        }
    });
}

fn sort_menu(
    ui: &mut Ui,
    cx: &mut WorkspaceCx<'_>,
    intents: &mut IntentQueue,
    palette: &Palette,
    local: &mut MenuLocal,
    focus_first: &mut dyn FnMut(&egui::Response),
) {
    for (key, label) in SORT_ITEMS {
        let current = cx.findings.sort == key;
        let hint = current.then(|| icon_text(ui.ctx(), Icon::Check));
        let item = menu_item(ui, label, hint.as_deref(), true, palette);
        focus_first(&item);
        if item.clicked() {
            cx.findings.sort = key;
            close_menu(local, intents);
        }
    }
}

fn actions_menu(
    ui: &mut Ui,
    cx: &mut WorkspaceCx<'_>,
    intents: &mut IntentQueue,
    palette: &Palette,
    local: &mut MenuLocal,
    focus_first: &mut dyn FnMut(&egui::Response),
) {
    let queue_identical = menu_item(ui, ACTIONS_ITEMS[0], None, true, palette);
    focus_first(&queue_identical);
    if queue_identical.clicked() {
        let scope = match &cx.findings.projection {
            Some(p) if p.is_filtered() => QueueScope::Groups(p.shown_groups.clone()),
            _ => QueueScope::AllByteIdenticalGroups,
        };
        close_menu(local, intents);
        intents.push(UiIntent::OpenDialog {
            kind: DialogKind::QueueIdentical,
            subject: DialogSubject::QueueIdentical(scope),
            invoker: Some(actions_button_id()),
        });
    }
    if menu_item(ui, ACTIONS_ITEMS[1], None, true, palette).clicked() {
        if let Some(results) = &cx.scan.results {
            cx.findings.expanded = results.groups.iter().map(|g| g.id).collect();
        }
        close_menu(local, intents);
    }
    if menu_item(ui, ACTIONS_ITEMS[2], None, true, palette).clicked() {
        cx.findings.expanded.clear();
        close_menu(local, intents);
    }
    ui.separator();
    let choosing = local.choosing_columns;
    if menu_item(ui, ACTIONS_ITEMS[3], None, true, palette).clicked() {
        local.choosing_columns = !choosing;
    }
    if choosing {
        for column in grouped_table::OPTIONAL {
            let mut shown = !cx.findings.hidden_columns.contains(&column);
            ui.horizontal(|ui| {
                ui.add_space(10.0);
                if ui
                    .checkbox(&mut shown, grouped_table::column_title(column))
                    .changed()
                {
                    if shown {
                        cx.findings.hidden_columns.remove(&column);
                    } else {
                        cx.findings.hidden_columns.insert(column);
                    }
                }
            });
        }
    }
}

fn row_menu(
    ui: &mut Ui,
    cx: &mut WorkspaceCx<'_>,
    intents: &mut IntentQueue,
    palette: &Palette,
    row: RowId,
    local: &mut MenuLocal,
    focus_first: &mut dyn FnMut(&egui::Response),
) {
    let Some(results) = cx.scan.results.clone() else {
        return;
    };
    let RowId::Member { group, file } = row else {
        return;
    };
    let Some(group) = results.groups.iter().find(|g| g.id == group) else {
        return;
    };
    let Some(member) = group.member(file) else {
        return;
    };
    let view = projections::member_view(group, cx.queue, member);
    let is_reference = view.counterpart.is_none();

    let compare = menu_item(
        ui,
        &projections::compare_item_label(group, &view),
        Some("Enter"),
        !is_reference,
        palette,
    );
    focus_first(&compare);
    if compare.clicked()
        && let Some(counterpart) = view.counterpart
    {
        close_menu(local, intents);
        intents.push(UiIntent::OpenCompare {
            target: file,
            counterpart,
            return_row: Some(row),
        });
    }
    let invoker = Some(grouped_table::table_id());
    let review = menu_item(ui, ROW_MENU_ITEMS[0], None, !is_reference, palette);
    if review.clicked()
        && let Some(counterpart) = view.counterpart
        && let Some(subject) = unique_text_subject(cx, &results, group, file, counterpart)
    {
        close_menu(local, intents);
        intents.push(UiIntent::OpenDialog {
            kind: DialogKind::UniqueTextReview,
            subject: DialogSubject::UniqueText(subject),
            invoker,
        });
    }
    let review_command = |command| UiIntent::Review { command, invoker };
    if menu_item(ui, ROW_MENU_ITEMS[1], None, !is_reference, palette).clicked() {
        let kept = cx.queue.decisions.get(&file) == Some(&MemberDecision::Kept);
        close_menu(local, intents);
        intents.push(review_command(ReviewCommand::MarkKept {
            file,
            kept: !kept,
        }));
    }
    ui.separator();
    if menu_item(ui, ROW_MENU_ITEMS[2], None, !is_reference, palette).clicked() {
        close_menu(local, intents);
        intents.push(review_command(ReviewCommand::MakeReference { file }));
    }
    if menu_item(
        ui,
        ROW_MENU_ITEMS[3],
        None,
        !member.folder_protected,
        palette,
    )
    .clicked()
    {
        let protected = cx.queue.is_protected(file);
        close_menu(local, intents);
        intents.push(review_command(ReviewCommand::ProtectFile {
            file,
            protected: !protected,
        }));
    }
    ui.separator();
    if menu_item(ui, ROW_MENU_ITEMS[4], None, true, palette).clicked() {
        close_menu(local, intents);
        intents.push(UiIntent::OpenContainingFolder(file));
    }
    if menu_item(ui, ROW_MENU_ITEMS[5], Some("Ctrl+Shift+C"), true, palette).clicked() {
        close_menu(local, intents);
        intents.push(UiIntent::CopyPath(file));
    }
}

/// Review unique text's subject for a row-menu request: the pair's current
/// snapshots (the engine's view, else the scan's) and rules revision, with
/// the row menu as the queue origin.
fn unique_text_subject(
    cx: &WorkspaceCx<'_>,
    results: &fuzzy_dedupe_domain::ScanResults,
    group: &fuzzy_dedupe_domain::Group,
    file: fuzzy_dedupe_domain::FileId,
    counterpart: fuzzy_dedupe_domain::FileId,
) -> Option<UniqueTextSubject> {
    let snapshot = |f| {
        cx.queue
            .snapshot(f)
            .or_else(|| group.member(f).map(|m| m.snapshot))
    };
    Some(UniqueTextSubject {
        scan: results.scan,
        target: file,
        counterpart,
        target_snapshot: snapshot(file)?,
        counterpart_snapshot: snapshot(counterpart)?,
        rules: cx
            .queue
            .view
            .as_ref()
            .map_or(results.rules.revision, |v| v.rules),
        reviewed: None,
        origin: QueueOrigin::RowMenu,
    })
}
