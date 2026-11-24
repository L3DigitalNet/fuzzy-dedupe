// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! The Queue identical copies popup (spec §11.6, frame `27:1868`), the only
//! bulk queueing path (§11.4).
//!
//! The dialog host (`ui/widgets/dialog_host.rs`) owns the title row, the close
//! control, Esc, Enter, sizing, focus restoration and the pinned footer slot;
//! this file draws the body and the footer inside the 16-unit content inset.
//! Enter runs Queue N files.
//!
//! The popup states its scope, the rules it applies, and the file count and
//! size before anything is queued; Queue N files then sends one
//! `QueueIdenticalCopies` command through `UiIntent::Review`. The review
//! service decides what enters the queue (plan D-003) and skips silently
//! whatever its rule excludes, so the stated count is a [`forecast`] of that
//! rule over the presentation read model, never an authority.
//!
//! Scope. The subject is the invoker's statement of what the current filter
//! shows: `QueueScope::Groups` lists the shown groups, and
//! `AllByteIdenticalGroups` means the filter hides none. The popup opens on
//! "All byte-identical groups", as the frame does.
//!
//! The rule itself is not here. [`forecast`] calls the engine's exported
//! eligibility forecast (`fuzzy_dedupe_engine::review::forecast`) over the
//! read model, and the review service queues exactly what the same function
//! returns on its own state, so the popup's "Queue N files" cannot drift
//! from what is queued (PV-T33-002). No eligibility condition may be added
//! to this crate: one that the service does not share is how the stated
//! count went wrong before (plan T33). What the read model must mirror for
//! the two to agree is `QueueState`'s `ForecastFacts` impl in
//! `presentation/state.rs`.
//!
//! This file also holds the small pieces both T17 popups draw with
//! (`current_reference`, the footer row, the tinted icon and the text helper);
//! `unique_text_review.rs` imports them, so a change here reaches both.

use std::sync::Arc;

use egui::{Color32, Margin, RichText, Stroke, Vec2};
use fuzzy_dedupe_domain::{FileId, Group, QueueScope, ScanId, ScanResults};
use fuzzy_dedupe_engine::review::{IdenticalForecast, forecast as eligibility};
use fuzzy_dedupe_engine::service::ReviewCommand;

use crate::presentation::commands::UiIntent;
use crate::presentation::state::{DialogSubject, QueueState};
use crate::ui::dialogs::{DialogCx, DialogResponse};
use crate::ui::glyphs;
use crate::ui::theme::{self, CompactStyle, Icon, Palette, Token};
use crate::ui::widgets::dialog_host;

/// The two scopes of spec §11.4.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ScopeChoice {
    /// Every byte-identical group of the scan.
    #[default]
    All,
    /// Only the groups the current filter shows (the popup's subject).
    Shown,
}

/// The forecast for both scopes, computed once per read-model change: a
/// 100,000-row scan is not walked on every frame the popup is open. Shared
/// so a frame reads it without copying either file list.
#[derive(Clone, Debug, PartialEq)]
struct Cached {
    key: CacheKey,
    all: IdenticalForecast,
    shown: IdenticalForecast,
}

/// Everything the forecast reads changes one of these: results and review
/// outcomes bump `FindingsState::revision` (`events.rs`), and a newly
/// published validation view is a new `Arc`. The pointer is compared only
/// together with the revision, so a reused address cannot alias alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CacheKey {
    scan: Option<ScanId>,
    revision: u64,
    view: Option<usize>,
}

/// The popup's draft, created when it opens and dropped when it closes, so
/// closing never commits it (spec §11.6).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Draft {
    pub choice: ScopeChoice,
    cached: Option<Arc<Cached>>,
}

/// The draft for a newly opened popup. Queues nothing: the forecast is
/// local and the popup needs no engine job.
pub fn open(cx: &mut DialogCx<'_>) -> Draft {
    let mut draft = Draft::default();
    refresh(&mut draft, cx);
    draft
}

/// Draws the body; [`footer`] draws Cancel and Queue N files. Never closes
/// the popup.
pub fn show(ui: &mut egui::Ui, draft: &mut Draft, cx: &mut DialogCx<'_>) -> DialogResponse {
    let palette = theme::palette(dialog_host::mode_of(ui.ctx()));
    let cached = refresh(draft, cx);
    let (all, shown) = (&cached.all, &cached.shown);

    ui.spacing_mut().item_spacing = Vec2::new(8.0, 12.0);
    ui.label(
        RichText::new(
            "Adds byte-identical copies to the cleanup queue. Nothing is removed until you \
             review the cleanup.",
        )
        .text_style(CompactStyle::Body.text_style())
        .color(palette.color(Token::Ink)),
    );

    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 6.0;
        ui.label(text(
            "Scope",
            CompactStyle::GroupEmphasis,
            palette,
            Token::Ink,
        ));
        for (choice, label, forecast) in [
            (ScopeChoice::All, "All byte-identical groups", all),
            (
                ScopeChoice::Shown,
                "Only groups shown by the current filter",
                shown,
            ),
        ] {
            let label = format!("{label} · {}", groups(forecast.identical_groups));
            let radio = egui::RadioButton::new(
                draft.choice == choice,
                text(&label, CompactStyle::Table, palette, Token::Ink),
            );
            if ui.add(radio).clicked() {
                draft.choice = choice;
            }
        }
    });

    egui::Frame::new()
        .fill(palette.color(Token::Code))
        .stroke(Stroke::new(theme::BORDER_WIDTH, palette.color(Token::Line)))
        .corner_radius(theme::CORNER_RADIUS)
        .inner_margin(Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 4.0;
            for rule in RULES {
                ui.horizontal(|ui| {
                    icon(ui, Icon::Check, palette.color(Token::Green));
                    ui.label(text(rule, CompactStyle::Table, palette, Token::Ink));
                });
            }
        });

    let forecast = match draft.choice {
        ScopeChoice::All => all,
        ScopeChoice::Shown => shown,
    };
    ui.label(text(
        &summary(forecast),
        CompactStyle::GroupEmphasis,
        palette,
        Token::Ink,
    ));
    DialogResponse::KeepOpen
}

/// Cancel, then Queue N files for the chosen scope, in the footer slot the
/// dialog host pins below the body. Returns [`DialogResponse::Close`] when
/// either closed the popup.
pub fn footer(ui: &mut egui::Ui, draft: &mut Draft, cx: &mut DialogCx<'_>) -> DialogResponse {
    let palette = theme::palette(dialog_host::mode_of(ui.ctx()));
    let cached = refresh(draft, cx);
    let forecast = match draft.choice {
        ScopeChoice::All => &cached.all,
        ScopeChoice::Shown => &cached.shown,
    };
    let mut response = DialogResponse::KeepOpen;
    let queue_label = format!("Queue {}", files(forecast.files.len()));
    if footer_row(
        ui,
        palette,
        &queue_label,
        true,
        !forecast.files.is_empty(),
        &mut response,
    ) {
        response = submit(draft, cx);
    }
    response
}

/// The popup's non-destructive default action, run by Enter and by its
/// primary button: queue the forecast scope through the review service.
/// With nothing to queue it sends nothing and stays open.
pub fn submit(draft: &mut Draft, cx: &mut DialogCx<'_>) -> DialogResponse {
    let cached = refresh(draft, cx);
    let (forecast, scope) = match draft.choice {
        ScopeChoice::All => (&cached.all, QueueScope::AllByteIdenticalGroups),
        ScopeChoice::Shown => (&cached.shown, shown_scope(cx.subject)),
    };
    if forecast.files.is_empty() {
        return DialogResponse::KeepOpen;
    }
    cx.intents.push(UiIntent::Review {
        command: ReviewCommand::QueueIdenticalCopies { scope },
        invoker: None,
    });
    DialogResponse::Close
}

/// The rules the popup states, as frame `27:1868` lists them. The kept-file
/// skip (§11.4, C-05 I-6) has no line of its own there; [`forecast`]
/// applies it, so the stated count already reflects it.
const RULES: [&str; 4] = [
    "Only files whose bytes match their reference exactly",
    "The reference of each group is always kept",
    "Protected files are never queued",
    "Similar-text and containment matches are never included",
];

fn shown_scope(subject: &DialogSubject) -> QueueScope {
    match subject {
        DialogSubject::QueueIdentical(scope) => scope.clone(),
        _ => QueueScope::AllByteIdenticalGroups,
    }
}

/// Both scopes' forecasts, recomputed only when the read model changed.
fn refresh(draft: &mut Draft, cx: &DialogCx<'_>) -> Arc<Cached> {
    let key = CacheKey {
        scan: cx.scan.scan,
        revision: cx.findings.revision,
        view: cx.queue.view.as_ref().map(|v| Arc::as_ptr(v).addr()),
    };
    if let Some(cached) = draft.cached.as_ref().filter(|c| c.key == key) {
        return Arc::clone(cached);
    }
    let (all, shown) = match cx.scan.results.as_deref() {
        None => (IdenticalForecast::default(), IdenticalForecast::default()),
        Some(results) => {
            let all = forecast(results, &QueueScope::AllByteIdenticalGroups, cx.queue);
            let shown = match shown_scope(cx.subject) {
                QueueScope::AllByteIdenticalGroups => all.clone(),
                scope @ QueueScope::Groups(_) => forecast(results, &scope, cx.queue),
            };
            (all, shown)
        }
    };
    let cached = Arc::new(Cached { key, all, shown });
    draft.cached = Some(Arc::clone(&cached));
    cached
}

/// The group's current reference by the recorded roles
/// ([`QueueState::reference`]).
pub(crate) fn current_reference(group: &Group, queue: &QueueState) -> Option<FileId> {
    queue.reference(group)
}

/// What Queue identical copies will queue over `scope`: the engine's
/// eligibility forecast evaluated over the read model. The popup's count,
/// file set and size come only from here.
#[must_use]
pub fn forecast(
    results: &ScanResults,
    scope: &QueueScope,
    queue: &QueueState,
) -> IdenticalForecast {
    eligibility::queue_identical(results, scope, queue)
}

/// A size in binary units, one decimal, rounded to nearest (spec §11.4
/// Size): "12.8 KiB", "1.8 MiB"; below 1 KiB, whole bytes.
#[must_use]
pub fn binary_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KiB", "MiB", "GiB", "TiB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut unit: u128 = 1024;
    for (i, name) in UNITS.iter().enumerate() {
        let tenths = (u128::from(bytes) * 10 + unit / 2) / unit;
        // Promote when rounding reaches 1024.0 of this unit, so 1,048,575
        // bytes reads "1.0 MiB", never "1024.0 KiB".
        if tenths < 10_240 || i == UNITS.len() - 1 {
            return format!("{}.{} {name}", tenths / 10, tenths % 10);
        }
        unit *= 1024;
    }
    unreachable!("the last unit always returns")
}

fn files(n: usize) -> String {
    if n == 1 {
        "1 file".to_owned()
    } else {
        format!("{n} files")
    }
}

fn groups(n: usize) -> String {
    if n == 1 {
        "1 group".to_owned()
    } else {
        format!("{n} groups")
    }
}

fn summary(forecast: &IdenticalForecast) -> String {
    if forecast.files.is_empty() {
        return "No identical copies to queue in this scope.".to_owned();
    }
    format!(
        "{} in {} will be queued · {}",
        files(forecast.files.len()),
        groups(forecast.groups),
        binary_size(forecast.bytes)
    )
}

pub(crate) fn text(s: &str, style: CompactStyle, palette: &Palette, token: Token) -> RichText {
    RichText::new(s)
        .text_style(style.text_style())
        .color(palette.color(token))
}

/// Draws `icon` 14 units high in `color`, through the glyph registry
/// ([`glyphs::text`]).
///
/// A glyph, not an image, so it takes a token colour (`ui/glyphs.rs`). An
/// icon the registry cannot draw (not installed on this context, or its
/// SVG failed to parse) reserves its space and is never drawn.
pub(crate) fn icon(ui: &mut egui::Ui, icon: Icon, color: Color32) {
    const SIZE: f32 = 14.0;
    match glyphs::text(ui.ctx(), icon) {
        Some(glyph) => {
            ui.label(RichText::new(glyph).size(SIZE).color(color));
        }
        None => ui.add_space(SIZE),
    }
}

/// The footer row of both T17 popups: Cancel, then the primary action,
/// right aligned within the footer slot; `filled` draws the primary action
/// in the primary colour, as `27:1868` does and `16:932` does not. Returns
/// whether the primary action was pressed; Cancel sets `response` to close.
///
/// Laid out left to right after a measured gap rather than right-to-left,
/// so Tab reaches Cancel before the primary action, in reading order. The
/// gap is only right when it matches what egui lays out: the primary button
/// is at least its text plus 6 units a side, and never narrower than egui's
/// own button padding makes it, and the 8-unit spacing between the buttons
/// is set here, not inherited from the caller. A width egui then widens, or
/// a gap read from whatever spacing the caller left, pushes the primary
/// button past the 16-unit content inset.
pub(crate) fn footer_row(
    ui: &mut egui::Ui,
    palette: &Palette,
    primary: &str,
    filled: bool,
    primary_enabled: bool,
    response: &mut DialogResponse,
) -> bool {
    const CANCEL_WIDTH: f32 = 80.0;
    const HEIGHT: f32 = 30.0;
    const GAP: f32 = 8.0;
    const PAD: f32 = 6.0;
    let font = CompactStyle::ControlLabel.font_id();
    let padding = ui.spacing().button_padding.x.max(PAD);
    let text_width = ui
        .painter()
        .layout_no_wrap(primary.to_owned(), font, Color32::PLACEHOLDER)
        .size()
        .x;
    let primary_width = text_width + 2.0 * padding;
    let mut pressed = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = GAP;
        ui.add_space((ui.available_width() - CANCEL_WIDTH - GAP - primary_width).max(0.0));
        let cancel = egui::Button::new(text(
            "Cancel",
            CompactStyle::ControlLabel,
            palette,
            Token::Ink,
        ))
        .min_size(Vec2::new(CANCEL_WIDTH, HEIGHT));
        if ui.add(cancel).clicked() {
            *response = DialogResponse::Close;
        }
        let ink = if filled { Token::White } else { Token::Ink };
        let mut button = egui::Button::new(text(primary, CompactStyle::ControlLabel, palette, ink))
            .min_size(Vec2::new(primary_width, HEIGHT));
        if filled {
            button = button.fill(palette.color(Token::Primary));
        }
        pressed = ui.add_enabled(primary_enabled, button).clicked();
    });
    pressed
}

#[cfg(test)]
mod tests {
    use super::binary_size;

    #[test]
    fn binary_size_uses_binary_units_rounded_to_a_tenth() {
        assert_eq!(binary_size(0), "0 B");
        assert_eq!(binary_size(1023), "1023 B");
        assert_eq!(binary_size(1024), "1.0 KiB");
        assert_eq!(binary_size(13_108), "12.8 KiB");
        assert_eq!(binary_size(1_048_575), "1.0 MiB");
        assert_eq!(binary_size(1_887_437), "1.8 MiB");
    }
}
