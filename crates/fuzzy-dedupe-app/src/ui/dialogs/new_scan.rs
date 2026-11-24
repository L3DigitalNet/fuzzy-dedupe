// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! The New scan popup (spec §11.6; frames `16:806`, `36:2435` with the
//! matching rules expanded, and `26:1367` with folder problems).
//!
//! The dialog host (`ui/widgets/dialog_host.rs`) owns the title row, the close
//! control, Esc, Enter, sizing and focus restoration; this file draws the body
//! ([`show`]) and the footer ([`footer`]) inside the 16-unit content inset, and
//! names the popup's size ([`size`]). The host pins the footer below the body,
//! which scrolls when the popup is shrunk to a small viewport (spec §11.6
//! "keep the action footer reachable"). Enter runs Start scan.
//!
//! The [`Draft`] holds the folders with their Protect files flags, the file
//! types and a matching-rules draft (`matching_rules::RulesDraft`, the same
//! controls the gear's popup draws). Nothing reaches the preferences, the
//! review service or the engine before Start scan; Cancel, × and Esc drop
//! the draft (IR-003). Removing a folder edits the draft's setup only: this
//! file never touches a source file.
//!
//! Start scan pushes, in order: the committed setup (`SetScanSetup`) and,
//! when they changed, the saved rules (`SetSavedRules`); then the review
//! service's `ClearForNewScan`; then `StartScan`. The popup itself closes
//! as `submit` returns, before any of them is dispatched. The clear-then-
//! start order is the plan's (T16) and relies on `commands::dispatch`
//! running intents in queue order: the clear, a synchronous review command,
//! empties the queue and every Kept, per-file Protect and acknowledgment
//! decision of the scan being replaced (FR-026) before the job that
//! replaces it is admitted, while folder protection survives in the setup
//! the start carries (C-05 I-9).
//!
//! Folder problems (§11.6 "inline folder problems"): a missing folder is
//! detected with one `std::fs::metadata` call per listed folder when the
//! popup opens, never listing or opening anything inside it; `open` runs
//! from intent dispatch, after the UI pass, so no filesystem call happens
//! while drawing (spec §5.2). Folders the picker just returned are not
//! checked: the portal chose them as existing directories. A nested folder
//! (one inside another listed folder, compared by path components with no
//! link resolution, per the owner's "trust the root" decision) is scanned
//! once, as part of the outer folder, and is a note, not an error; a
//! missing folder blocks Start scan until it is removed.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use egui::{Align, Id, Layout, RichText, Sense, Stroke, Ui, Vec2};
use fuzzy_dedupe_domain::{ScanFolder, ScanSetup};
use fuzzy_dedupe_engine::fixtures;
use fuzzy_dedupe_engine::service::{EngineRequest, ReviewCommand, StartScan};
use fuzzy_dedupe_platform::picker::PickRequest;

use crate::presentation::commands::UiIntent;
use crate::presentation::projections::thousands;
use crate::presentation::state::DialogKind;
use crate::ui::dialogs::matching_rules::{
    self, RulesDraft, action_row, check_box, primary_button, secondary_button,
};
use crate::ui::dialogs::{DialogCx, DialogResponse};
use crate::ui::glyphs;
use crate::ui::theme::{self, CompactStyle, Icon, Palette, Token};
use crate::ui::widgets::dialog_host::mode_of;

/// The message beside a folder that was not found.
pub const MISSING_FOLDER: &str = "Folder not found. Remove it or choose another folder.";

/// The popup's size with the matching rules expanded (spec §11.6, frame
/// `36:2435`); collapsed, it is `DialogKind::NewScan`'s reference size.
pub const EXPANDED_SIZE: [f32; 2] = [640.0, 778.0];

/// File types the summary lists before "+N more", as in `26:1338`.
const SHOWN_FILE_TYPES: usize = 6;

/// The popup's draft, created when it opens and dropped when it closes, so
/// closing never commits it (spec §11.6).
///
/// Boxed: `OpenDialog` holds every popup's draft inline in the one
/// `Overlay` enum, and this form inline would make its `Dialog` variant
/// large enough to fail clippy's `large_enum_variant` there.
#[derive(Clone, Debug, PartialEq)]
pub struct Draft {
    form: Box<Form>,
}

/// What the draft holds.
#[derive(Clone, Debug, PartialEq)]
struct Form {
    /// In list order, which the reference default depends on (spec §6).
    folders: Vec<ScanFolder>,
    /// Extensions without the dot.
    file_types: Vec<String>,
    /// The file-type editor's text while Edit… is open.
    file_types_text: String,
    editing_file_types: bool,
    /// Listed folders whose metadata showed no directory at open.
    missing: BTreeSet<PathBuf>,
    rules: RulesDraft,
    /// The Matching rules section is expanded (`36:2435`).
    rules_expanded: bool,
    /// Why the last Start scan was refused.
    error: Option<&'static str>,
}

/// The draft for a newly opened popup: the current setup preference and
/// the rules draft (see `RulesDraft::from_current`), with each listed
/// folder checked once for existence.
pub fn open(cx: &mut DialogCx<'_>) -> Draft {
    let setup = &cx.preferences.scan_setup;
    let missing = setup
        .folders
        .iter()
        .filter(|folder| !folder_exists(&folder.path))
        .map(|folder| folder.path.clone())
        .collect();
    let form = Form {
        folders: setup.folders.clone(),
        file_types: setup.file_types.clone(),
        file_types_text: setup.file_types.join(", "),
        editing_file_types: false,
        missing,
        rules: RulesDraft::from_current(cx),
        rules_expanded: false,
        error: None,
    };
    Draft {
        form: Box::new(form),
    }
}

/// Whether `path` names a directory, by its metadata alone. Follows a link
/// at the root itself, which the scan trusts; nothing below it is read.
fn folder_exists(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|metadata| metadata.is_dir())
}

/// Draws the body; [`footer`] draws the actions. Never closes the popup
/// itself: only the footer and the host's ×, Esc and Enter do.
pub fn show(ui: &mut Ui, draft: &mut Draft, cx: &mut DialogCx<'_>) -> DialogResponse {
    let draft = &mut *draft.form;
    if let Some(picked) = cx.picked_folders.take() {
        add_folders(draft, picked);
    }
    let palette = theme::palette(mode_of(ui.ctx()));
    ui.spacing_mut().item_spacing.y = 12.0;

    ui.label(
        RichText::new("Select folders. Scanning never changes files.")
            .text_style(CompactStyle::Table.text_style())
            .color(palette.color(Token::Muted)),
    );
    folder_table(ui, draft, palette);
    if secondary_button(ui, "+ Add folder", 112.0).clicked() {
        cx.intents.push(UiIntent::PickFolders(PickRequest {
            title: "Add folders to scan".to_owned(),
            start_dir: None,
        }));
    }
    file_types(ui, draft, palette);
    rules_section(ui, draft, palette);

    ui.label(
        RichText::new(
            "Protected folders are compared but cannot be queued for removal. References are \
             chosen from protected folders first.",
        )
        .text_style(CompactStyle::Supporting.text_style())
        .color(palette.color(Token::Green)),
    );
    replace_warning(ui, cx, palette);
    if let Some(message) = draft.error {
        ui.label(
            RichText::new(message)
                .text_style(CompactStyle::Supporting.text_style())
                .color(palette.color(Token::Red)),
        );
    }

    DialogResponse::KeepOpen
}

/// The popup's initial size: its reference size, or that of `36:2435`
/// while the matching rules are expanded (spec §11.6). The dialog host
/// shrinks either to the viewport.
#[must_use]
pub fn size(draft: &Draft) -> [f32; 2] {
    if draft.form.rules_expanded {
        EXPANDED_SIZE
    } else {
        DialogKind::NewScan.reference_size()
    }
}

/// Cancel and Start scan, in the footer slot the dialog host pins below the
/// scrolling body, so both stay visible however far the body scrolls.
/// Returns [`DialogResponse::Close`] when either closed the popup.
pub fn footer(ui: &mut Ui, draft: &mut Draft, cx: &mut DialogCx<'_>) -> DialogResponse {
    let draft = &mut *draft.form;
    let palette = theme::palette(mode_of(ui.ctx()));
    let mut response = DialogResponse::KeepOpen;
    action_row(ui, |ui| {
        if primary_button(ui, "Start scan", palette).clicked() {
            response = start(draft, cx);
        }
        if secondary_button(ui, "Cancel", 80.0).clicked() {
            response = DialogResponse::Close;
        }
    });
    response
}

/// Start scan: refuses with an inline message while the draft cannot
/// start, otherwise commits the setup and rules, clears the review
/// decisions and starts the simulated scan, in the order the file header
/// explains, and closes.
pub fn submit(draft: &mut Draft, cx: &mut DialogCx<'_>) -> DialogResponse {
    start(&mut draft.form, cx)
}

fn start(draft: &mut Form, cx: &mut DialogCx<'_>) -> DialogResponse {
    draft.error = blocking_problem(draft);
    if draft.error.is_some() {
        return DialogResponse::KeepOpen;
    }
    let Some(rules) = draft.rules.commit() else {
        // The message sits inside the section; make sure it is visible.
        draft.rules_expanded = true;
        return DialogResponse::KeepOpen;
    };
    let setup = ScanSetup {
        folders: draft.folders.clone(),
        file_types: draft.file_types.clone(),
    };

    cx.intents.push(UiIntent::SetScanSetup(setup.clone()));
    if cx.preferences.saved_rules != Some(rules) {
        cx.intents.push(UiIntent::SetSavedRules(rules));
    }
    cx.intents.push(UiIntent::Review {
        command: ReviewCommand::ClearForNewScan,
        invoker: None,
    });
    // MS-1 scans are simulated (C-03): the engine replays the fixture
    // scenario the app runs on, the one `measure` selects at startup.
    cx.intents
        .push(UiIntent::Submit(EngineRequest::StartScan(StartScan {
            setup,
            rules,
            scenario: crate::measure::startup_scenario(),
            seed: fixtures::DEFAULT_SEED,
        })));
    DialogResponse::Close
}

/// The first reason the draft's setup cannot start a scan.
fn blocking_problem(draft: &Form) -> Option<&'static str> {
    if draft.folders.is_empty() {
        Some("Add a folder to scan.")
    } else if draft
        .folders
        .iter()
        .any(|f| draft.missing.contains(&f.path))
    {
        Some("Remove or replace the folder that was not found.")
    } else if draft.file_types.is_empty() {
        Some("Choose at least one file type.")
    } else {
        None
    }
}

/// Appends picked folders the list does not already hold, unprotected.
fn add_folders(draft: &mut Form, picked: Vec<PathBuf>) {
    for path in picked {
        if !draft.folders.iter().any(|folder| folder.path == path) {
            draft.folders.push(ScanFolder {
                path,
                protect: false,
            });
        }
    }
}

/// How a folder is listed: its path with one trailing slash, as in
/// `16:823`. Lossy only for display; the draft keeps the `PathBuf`.
#[must_use]
pub fn shown_path(path: &Path) -> String {
    let text = path.display().to_string();
    if text.ends_with('/') {
        text
    } else {
        format!("{text}/")
    }
}

/// The listed folder that contains folder `index`, if any: another folder
/// it lies strictly inside, or an earlier entry with the same path. Pure
/// path-component comparison, no filesystem call.
fn container(folders: &[ScanFolder], index: usize) -> Option<usize> {
    let path = &folders[index].path;
    folders.iter().enumerate().position(|(other, folder)| {
        other != index
            && ((path.starts_with(&folder.path) && *path != folder.path)
                || (*path == folder.path && other < index))
    })
}

/// Whether any listed folder containing folder `index` is protected: its
/// files are then protected through that folder whatever its own flag.
fn protected_through_container(folders: &[ScanFolder], index: usize) -> bool {
    let path = &folders[index].path;
    folders
        .iter()
        .enumerate()
        .any(|(other, folder)| other != index && folder.protect && path.starts_with(&folder.path))
}

/// The `Folders to scan` table: a header row, then one 36-unit row per
/// folder with its inline problem, Protect files checkbox and remove ×.
fn folder_table(ui: &mut Ui, draft: &mut Form, palette: &Palette) {
    egui::Frame::new()
        .fill(palette.color(Token::Code))
        .stroke(Stroke::new(theme::BORDER_WIDTH, palette.color(Token::Line)))
        .corner_radius(theme::CORNER_RADIUS)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.set_width(ui.available_width());
            egui::Frame::new()
                .fill(palette.color(Token::Rail))
                .inner_margin(6.0)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        let strong = |text| {
                            RichText::new(text).text_style(CompactStyle::GroupEmphasis.text_style())
                        };
                        ui.label(strong("Folder"));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(strong("Protect files"));
                        });
                    });
                });

            let mut remove = None;
            for index in 0..draft.folders.len() {
                if folder_row(ui, draft, index, palette) {
                    remove = Some(index);
                }
            }
            if let Some(index) = remove {
                draft.folders.remove(index);
            }
        });
}

/// Draws folder `index`; returns whether its × was pressed.
fn folder_row(ui: &mut Ui, draft: &mut Form, index: usize, palette: &Palette) -> bool {
    let shown = shown_path(&draft.folders[index].path);
    let missing = draft.missing.contains(&draft.folders[index].path);
    let inside = container(&draft.folders, index).map(|i| shown_path(&draft.folders[i].path));
    let inherited = protected_through_container(&draft.folders, index);
    let mut removed = false;

    ui.allocate_ui_with_layout(
        Vec2::new(ui.available_width(), 36.0),
        Layout::right_to_left(Align::Center),
        |ui| {
            ui.add_space(8.0);
            ui.spacing_mut().item_spacing.x = 8.0;
            let remove = ui.add(
                egui::Button::new(
                    RichText::new("×").text_style(CompactStyle::ControlLabel.text_style()),
                )
                .frame(false)
                .min_size(Vec2::splat(30.0)),
            );
            let name = format!("Remove {shown}");
            remove.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
            removed = remove.clicked();
            ui.add_space(12.0);

            let folder = &mut draft.folders[index];
            let on = folder.protect || inherited;
            let name = format!("Protect files in {shown}");
            if check_box(ui, on, None, &name, !inherited).clicked() {
                folder.protect = !folder.protect;
            }

            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                ui.add_space(8.0);
                let path_color = if missing { Token::Red } else { Token::Ink };
                ui.add(
                    egui::Label::new(
                        RichText::new(&shown)
                            .text_style(CompactStyle::PathDetail.text_style())
                            .color(palette.color(path_color)),
                    )
                    .truncate(),
                );
                let note = if missing {
                    Some((MISSING_FOLDER.to_owned(), Token::Red))
                } else {
                    inside.map(|outer| (format!("Inside {outer} — scanned once"), Token::Muted))
                };
                if let Some((text, token)) = note {
                    ui.add(
                        egui::Label::new(
                            RichText::new(text)
                                .text_style(CompactStyle::Supporting.text_style())
                                .color(palette.color(token)),
                        )
                        .truncate(),
                    );
                }
            });
        },
    );
    removed
}

/// "File types: .txt   .md … +N more  Edit…", and the inline editor
/// Edit… discloses (spec §11.6 progressive disclosure).
fn file_types(ui: &mut Ui, draft: &mut Form, palette: &Palette) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.label(
            RichText::new("File types:")
                .text_style(CompactStyle::Supporting.text_style())
                .color(palette.color(Token::Muted)),
        );
        if draft.editing_file_types {
            let edit = egui::TextEdit::singleline(&mut draft.file_types_text)
                .font(CompactStyle::PathDetail.text_style())
                .hint_text("txt, md, log")
                .desired_width(360.0);
            if ui.add(edit).accessible_name("File types").changed() {
                draft.file_types = parse_file_types(&draft.file_types_text);
            }
        } else {
            ui.label(
                RichText::new(file_types_summary(&draft.file_types))
                    .text_style(CompactStyle::PathDetail.text_style())
                    .color(palette.color(Token::Ink)),
            );
        }
        let toggle = if draft.editing_file_types {
            "Done"
        } else {
            "Edit…"
        };
        let link = ui.add(
            egui::Button::new(
                RichText::new(toggle)
                    .text_style(CompactStyle::Supporting.text_style())
                    .color(palette.color(Token::Primary)),
            )
            .frame(false),
        );
        if link.clicked() {
            draft.editing_file_types = !draft.editing_file_types;
        }
    });
}

/// The summary of `26:1338`: up to six extensions, then "+N more".
fn file_types_summary(types: &[String]) -> String {
    if types.is_empty() {
        return "none".to_owned();
    }
    let mut parts: Vec<String> = types
        .iter()
        .take(SHOWN_FILE_TYPES)
        .map(|t| format!(".{t}"))
        .collect();
    if types.len() > SHOWN_FILE_TYPES {
        parts.push(format!("+{} more", types.len() - SHOWN_FILE_TYPES));
    }
    parts.join("   ")
}

/// Extensions from the editor's text: separated by commas or spaces, a
/// leading dot dropped, duplicates kept once in first-seen order.
fn parse_file_types(text: &str) -> Vec<String> {
    let mut types: Vec<String> = Vec::new();
    for item in text.split([',', ' ', '\t']) {
        let item = item.trim().trim_start_matches('.');
        if !item.is_empty() && !types.iter().any(|t| t == item) {
            types.push(item.to_owned());
        }
    }
    types
}

/// The collapsible Matching rules section: a header with the one-line
/// summary, and when expanded the gear popup's controls indented 24 units.
fn rules_section(ui: &mut Ui, draft: &mut Form, palette: &Palette) {
    let chevron = if draft.rules_expanded {
        Icon::Expanded
    } else {
        Icon::Collapsed
    };
    let row = ui
        .horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            // Not selectable: a selectable label senses clicks itself and
            // would swallow the click meant for the whole header.
            let text = |ui: &mut Ui, text: RichText| {
                ui.add(egui::Label::new(text).selectable(false));
            };
            match glyphs::text(ui.ctx(), chevron) {
                Some(icon) => text(
                    ui,
                    RichText::new(icon)
                        .size(16.0)
                        .color(palette.color(Token::Ink)),
                ),
                None => {
                    ui.add_space(16.0);
                }
            }
            text(
                ui,
                RichText::new("Matching rules")
                    .text_style(CompactStyle::GroupEmphasis.text_style())
                    .color(palette.color(Token::Ink)),
            );
            text(
                ui,
                RichText::new(draft.rules.summary())
                    .text_style(CompactStyle::Table.text_style())
                    .color(palette.color(Token::Muted)),
            );
        })
        .response
        .rect;
    // A widget of its own with a fixed ID, not `.interact` on the row's
    // response: with that, Tab from Edit… left egui with no focused widget
    // and the next Tab restarted at the title bar's ×, so the keyboard never
    // reached the rules controls or the footer.
    let header = ui.interact(row, Id::unique("fd.new_scan.rules_header"), Sense::click());
    let expanded = draft.rules_expanded;
    header.widget_info(|| {
        egui::WidgetInfo::selected(egui::Role::Button, true, expanded, "Matching rules section")
    });
    if header.has_focus() {
        ui.painter().rect_stroke(
            header.rect.expand(2.0),
            theme::CORNER_RADIUS,
            Stroke::new(2.0, palette.color(Token::Focus)),
            egui::StrokeKind::Outside,
        );
    }
    if header.clicked() {
        draft.rules_expanded = !draft.rules_expanded;
    }
    if draft.rules_expanded {
        ui.horizontal(|ui| {
            ui.add_space(24.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 10.0;
                matching_rules::rules_controls(ui, &mut draft.rules, palette, "fd.new_scan");
            });
        });
    }
}

/// The amber notice of `26:1340`: what Start scan replaces and how many
/// queued files it clears (FR-026). The count is the queue read model's,
/// which mirrors the review service's published queue.
fn replace_warning(ui: &mut Ui, cx: &DialogCx<'_>, palette: &Palette) {
    let queued = cx.queue.queued_count();
    if cx.scan.results.is_none() && queued == 0 {
        return;
    }
    let amber = palette.color(Token::Amber);
    egui::Frame::new()
        .fill(palette.color(Token::AmberSoft))
        .corner_radius(theme::CORNER_RADIUS)
        .inner_margin(egui::Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.label(
                RichText::new("Starting a scan replaces the current results.")
                    .text_style(CompactStyle::GroupEmphasis.text_style())
                    .color(amber),
            );
            ui.label(
                RichText::new(queued_notice(queued))
                    .text_style(CompactStyle::Supporting.text_style())
                    .color(amber),
            );
        });
}

/// "3 queued files will be cleared from the cleanup queue."
#[must_use]
pub fn queued_notice(queued: usize) -> String {
    match queued {
        0 => "No files are queued for cleanup.".to_owned(),
        1 => "1 queued file will be cleared from the cleanup queue.".to_owned(),
        n => format!(
            "{} queued files will be cleared from the cleanup queue.",
            thousands(u64::try_from(n).unwrap_or(u64::MAX))
        ),
    }
}
