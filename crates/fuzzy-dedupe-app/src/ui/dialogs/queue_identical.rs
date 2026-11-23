// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! The Queue identical copies popup (spec §11.6, frame `27:1868`), the only
//! bulk queueing path (§11.4).
//!
//! The dialog host (`ui/widgets/dialog_host.rs`) owns the title row, the close
//! control, Esc, Enter, sizing and focus restoration; this file draws the body
//! and footer inside the 16-unit content inset. Enter runs Queue N files.
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
//! Cross-file contract with `fuzzy-dedupe-engine/src/review/policy.rs`
//! (`ReviewState::queue_identical`): [`forecast`] mirrors that eligibility
//! rule term for term. An edit to either must update the other, or the
//! popup's "Queue N files" stops matching what the service queues
//! (PV-T17-002 pins the equality).
//!
//! This file also holds the small pieces both T17 popups draw with
//! (`current_reference`, the footer, the tinted icon and the text helper);
//! `unique_text_review.rs` imports them, so a change here reaches both.

use std::sync::Arc;

use egui::{Color32, Id, Margin, RichText, Stroke, Vec2};
use fuzzy_dedupe_domain::{
    CounterpartBasis, FileId, Group, Measurement, MemberDecision, QueueScope, ScanId, Verification,
};
use fuzzy_dedupe_engine::service::ReviewCommand;

use crate::presentation::commands::UiIntent;
use crate::presentation::state::{DialogSubject, QueueState};
use crate::ui::dialogs::{DialogCx, DialogResponse};
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

/// What Queue identical copies would queue in one scope.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Forecast {
    /// Files the review service will queue.
    pub files: usize,
    /// Groups those files belong to.
    pub groups: usize,
    /// Their total size in bytes.
    pub bytes: u64,
    /// Groups in scope holding any byte-verified exact copy of their
    /// reference, the count each scope's radio states.
    pub identical_groups: usize,
}

/// The forecast for both scopes, computed once per read-model change: a
/// 100,000-row scan is not walked on every frame the popup is open.
#[derive(Clone, Debug, PartialEq)]
struct Cached {
    key: CacheKey,
    all: Forecast,
    shown: Forecast,
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
    cached: Option<Cached>,
}

/// The draft for a newly opened popup. Queues nothing: the forecast is
/// local and the popup needs no engine job.
pub fn open(cx: &mut DialogCx<'_>) -> Draft {
    let mut draft = Draft::default();
    refresh(&mut draft, cx);
    draft
}

/// Draws the body and footer. Returns [`DialogResponse::Close`] when a
/// footer action closed the popup.
pub fn show(ui: &mut egui::Ui, draft: &mut Draft, cx: &mut DialogCx<'_>) -> DialogResponse {
    let ctx = ui.ctx().clone();
    let palette = theme::palette(dialog_host::mode_of(&ctx));
    let (all, shown) = refresh(draft, cx);
    let mut response = DialogResponse::KeepOpen;

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
                    icon(ui, Icon::Check, CHECK_GLYPH, palette.color(Token::Green));
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

    let queue_label = format!("Queue {}", files(forecast.files));
    if footer(
        ui,
        palette,
        &queue_label,
        true,
        forecast.files > 0,
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
    let (all, shown) = refresh(draft, cx);
    let (forecast, scope) = match draft.choice {
        ScopeChoice::All => (all, QueueScope::AllByteIdenticalGroups),
        ScopeChoice::Shown => (shown, shown_scope(cx.subject)),
    };
    if forecast.files == 0 {
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
fn refresh(draft: &mut Draft, cx: &DialogCx<'_>) -> (Forecast, Forecast) {
    let key = CacheKey {
        scan: cx.scan.scan,
        revision: cx.findings.revision,
        view: cx.queue.view.as_ref().map(|v| Arc::as_ptr(v).addr()),
    };
    if let Some(cached) = draft.cached.as_ref().filter(|c| c.key == key) {
        return (cached.all, cached.shown);
    }
    let (all, shown) = match cx.scan.results.as_deref() {
        None => (Forecast::default(), Forecast::default()),
        Some(results) => {
            let all = forecast(results.groups.iter(), cx.queue);
            let shown = match shown_scope(cx.subject) {
                QueueScope::AllByteIdenticalGroups => all,
                QueueScope::Groups(ids) => forecast(
                    results.groups.iter().filter(|g| ids.contains(&g.id)),
                    cx.queue,
                ),
            };
            (all, shown)
        }
    };
    draft.cached = Some(Cached { key, all, shown });
    (all, shown)
}

/// The group's current reference by the read model: a row the review
/// service reported as Reference, else the scan's own reference for rows it
/// never reported (an unreported row has not changed since the scan).
pub(crate) fn current_reference(group: &Group, queue: &QueueState) -> Option<FileId> {
    group
        .members
        .iter()
        .find(|m| match queue.decisions.get(&m.file) {
            Some(decision) => *decision == MemberDecision::Reference,
            None => m.is_reference(),
        })
        .map(|m| m.file)
}

/// What Queue identical copies will queue over `groups`, by the review
/// service's per-member rule (`ReviewState::queue_identical`): a member is
/// taken only when it is not the reference, not protected, not kept, not
/// queued, not the counterpart of a queued file, its counterpart is the
/// reference, and its edge to the reference is Same bytes with bytes
/// compared equal. A hash match alone is not "bytes match their reference
/// exactly" (§11.4).
///
/// The counterpart term has one blind spot: the read model does not carry
/// a counterpart chosen by reviewing in Compare. Rows whose decision proves
/// a non-reference counterpart (Review first, Counterpart queued: an exact
/// copy of the reference needs no review, and the reference is never
/// queued) and scan-loaded Compare choices are excluded; an Unreviewed row
/// re-aimed in Compare onto a member it needs no review against is not
/// visible here and would be over-counted by one.
#[must_use]
pub fn forecast<'a>(groups: impl Iterator<Item = &'a Group>, queue: &QueueState) -> Forecast {
    let view = queue.view.as_deref();
    let protected = |file: &FileId| view.is_some_and(|v| v.is_protected(*file));
    let queued = |file: &FileId| view.is_some_and(|v| v.queued(*file).is_some());
    // FR-025: "Counterpart of a queued file". Counterparts never cross
    // groups, so one scan-wide set is the group-local check.
    let counterparts_of_queued: Vec<FileId> = view
        .map(|v| v.queue.iter().map(|q| q.counterpart).collect())
        .unwrap_or_default();
    let mut out = Forecast::default();
    for group in groups {
        let Some(reference) = current_reference(group, queue) else {
            continue;
        };
        let scan_reference = group.reference().map(|m| m.file);
        let exact = |file: FileId| {
            group.edge_between(file, reference).is_some_and(|e| {
                e.measurement == Measurement::Identical(Verification::ByteVerified)
            })
        };
        let mut taken = 0;
        let mut identical = false;
        for m in &group.members {
            let file = m.file;
            if file == reference || !exact(file) {
                continue;
            }
            identical = true;
            let decision = queue.decisions.get(&file).copied();
            // Make this the reference discards Compare choices, so a loaded
            // one only holds while the scan's reference is still current.
            let loaded_choice = scan_reference == Some(reference)
                && m.link()
                    .is_some_and(|l| l.basis == CounterpartBasis::ReviewedInCompare);
            let eligible = !protected(&file)
                && decision != Some(MemberDecision::Kept)
                && !queued(&file)
                && !queued(&reference)
                && !counterparts_of_queued.contains(&file)
                && !matches!(
                    decision,
                    Some(MemberDecision::ReviewFirst | MemberDecision::CounterpartQueued)
                )
                && !loaded_choice;
            if eligible {
                taken += 1;
                out.bytes += m.size_bytes;
            }
        }
        out.files += taken;
        out.groups += usize::from(taken > 0);
        out.identical_groups += usize::from(identical);
    }
    out
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

fn summary(forecast: Forecast) -> String {
    if forecast.files == 0 {
        return "No identical copies to queue in this scope.".to_owned();
    }
    format!(
        "{} in {} will be queued · {}",
        files(forecast.files),
        groups(forecast.groups),
        binary_size(forecast.bytes)
    )
}

pub(crate) fn text(s: &str, style: CompactStyle, palette: &Palette, token: Token) -> RichText {
    RichText::new(s)
        .text_style(style.text_style())
        .color(palette.color(token))
}

/// Private-use code points for the popups' tinted icons. Cross-file
/// contract: `ui/shell.rs` `GLYPHS` holds U+E100 to U+E103; these stay
/// outside that run so neither registration replaces the other's glyph
/// (egui keys a glyph rasterizer by its text).
pub(crate) const CHECK_GLYPH: char = '\u{E170}';
pub(crate) const ALERT_GLYPH: char = '\u{E171}';

/// Draws `icon` 14 units high in `color`, as glyph `ch`.
///
/// A glyph, not an image, so it takes a token colour: Lucide's
/// `currentColor` rasterizes black and egui's image tint only multiplies
/// (see the note above the icon table in `theme.rs`). The glyph is
/// registered on first use, once per context, and egui activates a glyph
/// rasterizer only from the next pass; laying the character out in the
/// registering pass panics under egui's missing-glyph policy. That pass
/// therefore reserves the icon's space and asks for a repaint. An icon
/// whose SVG fails to parse is logged and never drawn.
pub(crate) fn icon(ui: &mut egui::Ui, icon: Icon, ch: char, color: Color32) {
    const SIZE: f32 = 14.0;
    let ctx = ui.ctx().clone();
    let id = Id::unique(("fd.dialog_glyph", ch));
    let pass = ctx.cumulative_pass_nr();
    // The pass the glyph was registered in; `u64::MAX` marks one that
    // failed to parse and must never be laid out.
    let ready = if let Some(at) = ctx.data(|d| d.get_temp::<u64>(id)) {
        at < pass
    } else {
        let at = register_glyph(&ctx, icon, ch).map_or(u64::MAX, |()| pass);
        ctx.data_mut(|d| d.insert_temp(id, at));
        false
    };
    if ready {
        ui.label(RichText::new(ch.to_string()).size(SIZE).color(color));
    } else {
        ui.add_space(SIZE);
    }
}

fn register_glyph(ctx: &egui::Context, icon: Icon, ch: char) -> Option<()> {
    match egui_extras::SvgGlyph::from_bytes(icon.bytes()) {
        Ok(svg) => {
            ctx.add_glyph_rasterizer(
                svg.with_color(false)
                    .with_baseline_offset(-0.2)
                    .into_rasterizer(ch.to_string()),
            );
            ctx.request_repaint();
            Some(())
        }
        Err(err) => {
            tracing::warn!(icon = icon.lucide_name(), %err, "icon glyph not loaded");
            None
        }
    }
}

/// The footer of both T17 popups: Cancel, then the primary action, right
/// aligned; `filled` draws the primary action in the primary colour, as
/// `27:1868` does and `16:932` does not. Laid out left to right after a measured gap rather than
/// right-to-left, so Tab reaches Cancel before the primary action, in
/// reading order. Returns whether the primary action was pressed; Cancel
/// sets `response` to close.
pub(crate) fn footer(
    ui: &mut egui::Ui,
    palette: &Palette,
    primary: &str,
    filled: bool,
    primary_enabled: bool,
    response: &mut DialogResponse,
) -> bool {
    const CANCEL_WIDTH: f32 = 80.0;
    const HEIGHT: f32 = 30.0;
    const PAD: f32 = 6.0;
    let font = CompactStyle::ControlLabel.font_id();
    let primary_width = ui
        .painter()
        .layout_no_wrap(primary.to_owned(), font, Color32::PLACEHOLDER)
        .size()
        .x
        + 2.0 * PAD;
    let mut pressed = false;
    ui.horizontal(|ui| {
        let gap = ui.spacing().item_spacing.x;
        ui.add_space((ui.available_width() - CANCEL_WIDTH - gap - primary_width).max(0.0));
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
