// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! The Matching rules popup (spec §11.6, frames `16:735` and `26:1283`),
//! plus the rules controls New scan reuses in its collapsible section.
//!
//! The dialog host (`ui/widgets/dialog_host.rs`) owns the title row, the close
//! control, Esc, Enter, sizing, focus restoration and the pinned footer slot;
//! this file draws the body and the footer inside the 16-unit content inset.
//! Enter runs Save rules.
//!
//! Three copies of the rules stay separate (spec §11.6): the saved rules
//! (`PreferencesState::saved_rules`), this popup's [`Draft`], and the current
//! scan's stored copy (`ScanState::rules` and `ScanResults::rules`). Save
//! rules commits only the first, through `UiIntent::SetSavedRules`; nothing
//! here can reach the stored copy, so saving never relabels existing
//! evidence (IR-003). Closing in any other way drops the draft.
//!
//! No approved default exists for any rule (OQ-005, OQ-006), so a draft
//! starts from the saved rules, else from the current scan's stored copy,
//! else unset: no match mode chosen and an empty similarity field, which
//! Save rules refuses until the user fills them. The design scenario's
//! `95.0%` therefore reaches the popup only as that scan's stored value.
//!
//! Spec over frame: `26:1283` lists four match modes, but §11.6 and the
//! C-01 `MatchMode` have two (Exact only, Exact + fuzzy text), so the menu
//! offers those two under the frame's labels.

use egui::{Align, Color32, Layout, Response, RichText, Sense, Stroke, Ui, Vec2};
use fuzzy_dedupe_domain::{IgnoreOptions, MatchMode, MatchingRules, Permille, RulesRevision};

use crate::presentation::commands::UiIntent;
use crate::ui::dialogs::{DialogCx, DialogResponse};
use crate::ui::glyphs;
use crate::ui::theme::{self, CompactStyle, Icon, Palette, Token};
use crate::ui::widgets::dialog_host::mode_of;

/// One step of the − and + controls, in per-mille: 1.0 percentage point.
const STEP: u16 = 10;

/// The inline message for a similarity the draft cannot save.
pub const SIMILARITY_ERROR: &str =
    "Enter a minimum similarity from 0 to 100%, to one decimal place.";
/// The inline message for a draft with no match mode yet.
pub const MODE_ERROR: &str = "Choose a match mode.";

/// The Cancel action's width in `16:792`; Save rules sizes to its label.
const CANCEL_WIDTH: f32 = 78.0;
/// The Match mode button's width in `16:750`.
const MODE_WIDTH: f32 = 190.0;
/// The similarity value box's width in `26:1268`.
const VALUE_WIDTH: f32 = 96.0;
/// The shared control height of the compact controls (spec §11.3).
const CONTROL_HEIGHT: f32 = 30.0;

/// The editable rules: what both this popup and New scan's Matching rules
/// section hold. The similarity is kept as typed, so an invalid entry stays
/// on screen with its message instead of snapping back.
#[derive(Clone, Debug, PartialEq)]
pub struct RulesDraft {
    /// `None` only when no saved or stored rules existed to start from.
    pub mode: Option<MatchMode>,
    /// Percent text, such as `95.0`.
    pub similarity: String,
    pub ignore: IgnoreOptions,
    /// The rules the draft started from. An unchanged draft commits with
    /// this revision; a changed one gets a new revision.
    pub base: Option<MatchingRules>,
    /// The newer of the saved and the current scan's revisions, so a new
    /// revision never repeats the one the current scan's evidence carries.
    newest: Option<RulesRevision>,
    /// The message the last commit attempt produced, until the next one.
    pub error: Option<&'static str>,
}

impl RulesDraft {
    /// A draft from the saved rules, else the current scan's stored copy.
    #[must_use]
    pub fn from_current(cx: &DialogCx<'_>) -> Self {
        let saved = cx.preferences.saved_rules;
        let stored = cx.scan.rules;
        let base = saved.or(stored);
        let newest = [saved, stored]
            .into_iter()
            .flatten()
            .map(|rules| rules.revision)
            .max();
        Self {
            mode: base.map(|rules| rules.mode),
            similarity: base.map_or_else(String::new, |r| percent_text(r.minimum_similarity)),
            ignore: base.map_or(
                IgnoreOptions {
                    whitespace: false,
                    line_endings: false,
                    letter_case: false,
                    punctuation: false,
                },
                |rules| rules.ignore,
            ),
            base,
            newest,
            error: None,
        }
    }

    /// The rules this draft commits, or the message that blocks it. The
    /// revision is the base's when nothing changed, else one above every
    /// revision seen, so a changed rule set is always a new saved version.
    ///
    /// # Errors
    ///
    /// [`MODE_ERROR`] with no match mode, [`SIMILARITY_ERROR`] when the
    /// similarity is not a 0–100% value with at most one decimal.
    pub fn rules(&self) -> Result<MatchingRules, &'static str> {
        let mode = self.mode.ok_or(MODE_ERROR)?;
        let minimum_similarity = parse_similarity(&self.similarity).ok_or(SIMILARITY_ERROR)?;
        let mut rules = MatchingRules {
            mode,
            minimum_similarity,
            ignore: self.ignore,
            revision: RulesRevision::new(0),
        };
        match self.base {
            Some(base) if same_content(&base, &rules) => rules.revision = base.revision,
            _ => {
                let next = self.newest.map_or(1, |r| r.get().saturating_add(1));
                rules.revision = RulesRevision::new(next);
            }
        }
        Ok(rules)
    }

    /// Validates the draft, recording the message to show when it fails.
    pub fn commit(&mut self) -> Option<MatchingRules> {
        match self.rules() {
            Ok(rules) => {
                self.error = None;
                Some(rules)
            }
            Err(message) => {
                self.error = Some(message);
                None
            }
        }
    }

    /// The one-line summary New scan shows for its collapsed section, as
    /// in `36:2429`: "Exact + fuzzy text · 95% similar · ignores whitespace
    /// and line endings". An unparseable similarity is left out rather
    /// than shown as a number it is not.
    #[must_use]
    pub fn summary(&self) -> String {
        let mut parts = vec![self.mode.map_or("No match mode", mode_label).to_owned()];
        if self.mode == Some(MatchMode::ExactAndFuzzy)
            && let Some(value) = parse_similarity(&self.similarity)
        {
            parts.push(format!("{} similar", short_percent(value)));
        }
        parts.push(ignore_summary(self.ignore));
        parts.join(" · ")
    }

    fn step(&mut self, up: bool) {
        let current = parse_similarity(&self.similarity)
            .or_else(|| self.base.map(|rules| rules.minimum_similarity))
            .map_or(0, Permille::get);
        let next = if up {
            current.saturating_add(STEP).min(Permille::FULL.get())
        } else {
            current.saturating_sub(STEP)
        };
        if let Some(value) = Permille::new(next) {
            self.similarity = percent_text(value);
        }
    }
}

/// Whether two rule sets match in everything but their revision.
fn same_content(a: &MatchingRules, b: &MatchingRules) -> bool {
    a.mode == b.mode && a.minimum_similarity == b.minimum_similarity && a.ignore == b.ignore
}

/// The popup's draft, created when it opens and dropped when it closes, so
/// closing never commits it (spec §11.6).
///
/// Boxed for the reason `new_scan::Draft` is: drafts sit inline in the
/// dialog host's `Overlay` enum.
#[derive(Clone, Debug, PartialEq)]
pub struct Draft {
    rules: Box<RulesDraft>,
}

/// The draft for a newly opened popup.
pub fn open(cx: &mut DialogCx<'_>) -> Draft {
    Draft {
        rules: Box::new(RulesDraft::from_current(cx)),
    }
}

/// Draws the body; [`footer`] draws the actions. Never closes the popup.
pub fn show(ui: &mut Ui, draft: &mut Draft, cx: &mut DialogCx<'_>) -> DialogResponse {
    let _ = cx;
    let palette = theme::palette(mode_of(ui.ctx()));
    ui.spacing_mut().item_spacing.y = 12.0;
    rules_controls(ui, &mut draft.rules, palette, "fd.matching_rules");

    divider(ui, palette);
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for line in [
            "Changes apply to the next scan.",
            "Existing results keep their original matching rules.",
        ] {
            ui.label(
                RichText::new(line)
                    .text_style(CompactStyle::Supporting.text_style())
                    .color(palette.color(Token::Muted)),
            );
        }
    });
    DialogResponse::KeepOpen
}

/// Cancel and Save rules, in the footer slot the dialog host pins below the
/// body. Returns [`DialogResponse::Close`] when either closed the popup.
pub fn footer(ui: &mut Ui, draft: &mut Draft, cx: &mut DialogCx<'_>) -> DialogResponse {
    let palette = theme::palette(mode_of(ui.ctx()));
    let mut response = DialogResponse::KeepOpen;
    action_row(ui, |ui| {
        if primary_button(ui, "Save rules", palette).clicked() {
            response = submit(draft, cx);
        }
        if secondary_button(ui, "Cancel", CANCEL_WIDTH).clicked() {
            response = DialogResponse::Close;
        }
    });
    response
}

/// The `Dialog actions` row: 30 units high, buttons from the right edge
/// with 8-unit gaps, so add the primary action first.
pub(crate) fn action_row(ui: &mut Ui, actions: impl FnOnce(&mut Ui)) {
    ui.allocate_ui_with_layout(
        Vec2::new(ui.available_width(), CONTROL_HEIGHT),
        Layout::right_to_left(Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            actions(ui);
        },
    );
}

/// Save rules: commits the draft as the saved rules and closes, or keeps
/// the popup open with the inline message when the draft is invalid.
pub fn submit(draft: &mut Draft, cx: &mut DialogCx<'_>) -> DialogResponse {
    match draft.rules.commit() {
        Some(rules) => {
            cx.intents.push(UiIntent::SetSavedRules(rules));
            DialogResponse::Close
        }
        None => DialogResponse::KeepOpen,
    }
}

// ── Shared rules controls ───────────────────────────────────────────────

/// Draws Match mode, Minimum similarity with its help line, and the four
/// Ignore options (`16:747` to `26:1282`), plus the draft's inline error.
/// `salt` keeps the combo box's popup ID apart from another instance.
pub fn rules_controls(ui: &mut Ui, draft: &mut RulesDraft, palette: &Palette, salt: &str) {
    labelled_row(ui, "Match mode", |ui| {
        let selected = draft.mode.map_or("Choose…", mode_label);
        egui::ComboBox::from_id_salt((salt, "mode"))
            .width(MODE_WIDTH)
            .height(CONTROL_HEIGHT * 4.0)
            .selected_text(selected)
            .show_ui(ui, |ui| {
                for mode in [MatchMode::ExactOnly, MatchMode::ExactAndFuzzy] {
                    let chosen = draft.mode == Some(mode);
                    if ui.selectable_label(chosen, mode_label(mode)).clicked() {
                        draft.mode = Some(mode);
                    }
                }
            })
            .response
            .accessible_name("Match mode");
    });

    labelled_row(ui, "Minimum similarity", |ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        if step_button(ui, "+", "Increase minimum similarity").clicked() {
            draft.step(true);
        }
        if step_button(ui, "−", "Decrease minimum similarity").clicked() {
            draft.step(false);
        }
        similarity_field(ui, draft, palette);
    });
    ui.label(
        RichText::new("How much text two files must share to be listed as similar.")
            .text_style(CompactStyle::Supporting.text_style())
            .color(palette.color(Token::Quiet)),
    );

    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.label(
            RichText::new("Text normalization")
                .text_style(CompactStyle::GroupEmphasis.text_style())
                .color(palette.color(Token::Ink)),
        );
        let ignore = &mut draft.ignore;
        for (flag, label) in [
            (&mut ignore.whitespace, "Ignore whitespace differences"),
            (&mut ignore.line_endings, "Ignore line-ending differences"),
            (&mut ignore.letter_case, "Ignore letter case"),
            (&mut ignore.punctuation, "Ignore punctuation"),
        ] {
            if check_box(ui, *flag, Some(label), label, true).clicked() {
                *flag = !*flag;
            }
        }
    });

    if let Some(message) = draft.error {
        ui.label(
            RichText::new(message)
                .text_style(CompactStyle::Supporting.text_style())
                .color(palette.color(Token::Red)),
        );
    }
}

/// A 30-unit row: `label` on the left, `controls` laid out right to left.
fn labelled_row(ui: &mut Ui, label: &str, controls: impl FnOnce(&mut Ui)) {
    ui.allocate_ui_with_layout(
        Vec2::new(ui.available_width(), CONTROL_HEIGHT),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.label(RichText::new(label).text_style(CompactStyle::Table.text_style()));
            ui.with_layout(Layout::right_to_left(Align::Center), controls);
        },
    );
}

/// The 96-unit value box of `26:1268`: the editable number, right
/// aligned, then a muted `%`.
fn similarity_field(ui: &mut Ui, draft: &mut RulesDraft, palette: &Palette) {
    egui::Frame::new()
        .fill(palette.color(Token::Surface))
        .stroke(Stroke::new(theme::BORDER_WIDTH, palette.color(Token::Line)))
        .corner_radius(theme::CORNER_RADIUS)
        .inner_margin(egui::Margin::symmetric(8, 0))
        .show(ui, |ui| {
            ui.set_min_size(Vec2::new(VALUE_WIDTH - 16.0, CONTROL_HEIGHT - 2.0));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.label(
                    RichText::new("%")
                        .text_style(CompactStyle::Table.text_style())
                        .color(palette.color(Token::Muted)),
                );
                let edit = egui::TextEdit::singleline(&mut draft.similarity)
                    .frame(egui::Frame::NONE)
                    .align(egui::Align2::RIGHT_CENTER)
                    .desired_width(ui.available_width())
                    .font(CompactStyle::Table.text_style());
                ui.add(edit).accessible_name("Minimum similarity");
            });
        });
}

fn step_button(ui: &mut Ui, glyph: &str, name: &str) -> Response {
    let response = ui.add(
        egui::Button::new(RichText::new(glyph).text_style(CompactStyle::ControlLabel.text_style()))
            .frame(false)
            .min_size(Vec2::splat(CONTROL_HEIGHT)),
    );
    let name = name.to_owned();
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
    response
}

/// A bordered footer or form action (`Action / Cancel`, `+ Add folder`).
pub(crate) fn secondary_button(ui: &mut Ui, label: &str, width: f32) -> Response {
    ui.add(
        egui::Button::new(RichText::new(label).text_style(CompactStyle::ControlLabel.text_style()))
            .min_size(Vec2::new(width, CONTROL_HEIGHT)),
    )
}

/// The filled primary action (`Action / Save rules`, `Action / Start scan`).
pub(crate) fn primary_button(ui: &mut Ui, label: &str, palette: &Palette) -> Response {
    ui.add(
        egui::Button::new(
            RichText::new(label)
                .text_style(CompactStyle::ControlLabel.text_style())
                .color(palette.color(Token::White)),
        )
        .fill(palette.color(Token::Primary))
        .min_size(Vec2::new(0.0, CONTROL_HEIGHT)),
    )
}

/// The 1-unit `line` divider of `16:787`.
pub(crate) fn divider(ui: &mut Ui, palette: &Palette) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
    ui.painter()
        .rect_filled(rect, 0.0, palette.color(Token::Line));
}

// ── The DD/Compact checkbox ──────────────────────────────────────────────

/// The `DD/Compact/Queue checkbox` (`13:562`/`13:563`) with an optional
/// visible label to its right, 26 units high as in `16:769`. The caller
/// toggles its own value on `clicked()`; keyboard activation (Space,
/// Enter) arrives as a click like any egui control. `accessible` names it
/// for assistive technology and tests when no visible label does.
pub(crate) fn check_box(
    ui: &mut Ui,
    on: bool,
    visible: Option<&str>,
    accessible: &str,
    enabled: bool,
) -> Response {
    const BOX: f32 = 16.0;
    const GAP: f32 = 8.0;
    let palette = theme::palette(mode_of(ui.ctx()));
    let galley = visible.map(|text| {
        egui::WidgetText::from(RichText::new(text).text_style(CompactStyle::Table.text_style()))
            .into_galley(
                ui,
                Some(egui::TextWrapMode::Extend),
                f32::INFINITY,
                CompactStyle::Table.text_style(),
            )
    });
    let width = BOX + galley.as_ref().map_or(0.0, |g| GAP + g.size().x);
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 26.0), sense);
    let name = accessible.to_owned();
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, enabled, on, &name));

    if ui.is_rect_visible(rect) {
        let fade = |color: Color32| {
            if enabled {
                color
            } else {
                color.gamma_multiply(0.5)
            }
        };
        let square = egui::Rect::from_min_size(
            egui::pos2(rect.left(), rect.center().y - BOX / 2.0),
            Vec2::splat(BOX),
        );
        let (fill, border) = if on {
            (Token::Primary, Token::Primary)
        } else {
            (Token::Surface, Token::Quiet)
        };
        let painter = ui.painter();
        painter.rect(
            square,
            3.0,
            fade(palette.color(fill)),
            Stroke::new(theme::BORDER_WIDTH, fade(palette.color(border))),
            egui::StrokeKind::Inside,
        );
        // A glyph, so the check takes the white token on the primary fill
        // (`ui/glyphs.rs`).
        if on && let Some(check) = glyphs::text(ui.ctx(), Icon::Check) {
            painter.text(
                square.center(),
                egui::Align2::CENTER_CENTER,
                check,
                egui::FontId::proportional(14.0),
                fade(palette.color(Token::White)),
            );
        }
        if response.has_focus() {
            painter.rect_stroke(
                square.expand(2.0),
                4.0,
                Stroke::new(2.0, palette.color(Token::Focus)),
                egui::StrokeKind::Outside,
            );
        }
        if let Some(galley) = galley {
            let at = egui::pos2(
                square.right() + GAP,
                rect.center().y - galley.size().y / 2.0,
            );
            painter.galley(at, galley, palette.color(Token::Ink));
        }
    }
    response
}

// ── Formatting and parsing ──────────────────────────────────────────────

/// The menu label for a match mode, as in `26:1323`.
#[must_use]
pub const fn mode_label(mode: MatchMode) -> &'static str {
    match mode {
        MatchMode::ExactOnly => "Exact copies only",
        MatchMode::ExactAndFuzzy => "Exact + fuzzy text",
    }
}

/// A similarity as the field shows it: always one decimal, `95.0`.
#[must_use]
pub fn percent_text(value: Permille) -> String {
    format!("{}.{}", value.get() / 10, value.get() % 10)
}

/// A similarity as the summary shows it: `95%`, or `95.5%` when the tenth
/// is not zero.
fn short_percent(value: Permille) -> String {
    match value.get() % 10 {
        0 => format!("{}%", value.get() / 10),
        tenth => format!("{}.{tenth}%", value.get() / 10),
    }
}

/// "ignores whitespace and line endings", or "ignores no differences".
fn ignore_summary(ignore: IgnoreOptions) -> String {
    let names: Vec<&str> = [
        (ignore.whitespace, "whitespace"),
        (ignore.line_endings, "line endings"),
        (ignore.letter_case, "letter case"),
        (ignore.punctuation, "punctuation"),
    ]
    .into_iter()
    .filter_map(|(on, name)| on.then_some(name))
    .collect();
    match names.as_slice() {
        [] => "ignores no differences".to_owned(),
        [one] => format!("ignores {one}"),
        [rest @ .., last] => format!("ignores {} and {last}", rest.join(", ")),
    }
}

/// Parses the similarity field: a percentage from 0 to 100 with at most one
/// decimal, optionally followed by `%`. Rejects a sign, an exponent and a
/// second decimal rather than rounding, so the saved value is exactly the
/// one on screen. `None` for anything else.
#[must_use]
pub fn parse_similarity(text: &str) -> Option<Permille> {
    let text = text.trim();
    let text = text.strip_suffix('%').unwrap_or(text).trim_end();
    let (whole, tenth) = match text.split_once('.') {
        Some((whole, tenth)) => (whole, Some(tenth)),
        None => (text, None),
    };
    let digits = |s: &str| !s.is_empty() && s.len() <= 3 && s.bytes().all(|b| b.is_ascii_digit());
    if !digits(whole) {
        return None;
    }
    let tenth = match tenth {
        None => 0,
        Some(t) if t.len() == 1 && digits(t) => t.parse::<u16>().ok()?,
        Some(_) => return None,
    };
    let value = whole
        .parse::<u16>()
        .ok()?
        .checked_mul(10)?
        .checked_add(tenth)?;
    Permille::new(value)
}
