// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! The one icon-glyph registry (spec §11.7 Icons, §11.8): every [`Icon`]
//! has exactly one private-use code point, and [`install`] registers all of
//! them on a context at once.
//!
//! Views draw an icon that must take a token colour as text: [`text`]
//! returns the icon's code point, which the registered rasterizer draws in
//! the text colour. `Icon::image` cannot do this; Lucide's `currentColor`
//! rasterizes black and egui's image tint only multiplies (see the note
//! above the icon table in `theme.rs`).
//!
//! Why one registry installed at startup, not lazy registration per view:
//! egui activates a glyph rasterizer only from the pass after the one that
//! added it, and laying out a code point nothing draws panics under
//! `egui_kittest`'s missing-glyph policy. A view that registered its glyphs
//! while drawing therefore drew no icon on its first frame and had to ask
//! for another; and views that each kept a table gave the same icon several
//! code points. Registered before the first pass, every icon draws on the
//! first frame.
//!
//! Contract: this is the only file that adds a glyph rasterizer or assigns
//! a private-use code point. `tests/glyph_registry.rs` scans `src/**` and
//! fails the gate on any registration or code point elsewhere.

use egui::{Context, Id};

use crate::ui::theme::Icon;

/// The icon's code point, in the Private Use Area (U+E000–U+F8FF), so no
/// font in the fallback chain can draw a different character there.
///
/// An exhaustive match, so a new [`Icon`] variant cannot compile without
/// one; `tests/glyph_registry.rs` checks that no two icons share one.
#[must_use]
pub const fn code_point(icon: Icon) -> char {
    match icon {
        Icon::Findings => '\u{E100}',
        Icon::Compare => '\u{E101}',
        Icon::Menu => '\u{E102}',
        Icon::Settings => '\u{E103}',
        Icon::Search => '\u{E104}',
        Icon::Expanded => '\u{E105}',
        Icon::Collapsed => '\u{E106}',
        Icon::Reference => '\u{E107}',
        Icon::Document => '\u{E108}',
        Icon::Protected => '\u{E109}',
        Icon::RowMenu => '\u{E10A}',
        Icon::Alert => '\u{E10B}',
        Icon::Check => '\u{E10C}',
    }
}

/// The icons whose rasterizer `install` registered on a context; one whose
/// SVG failed to parse is absent.
#[derive(Clone)]
struct Installed(Vec<Icon>);

fn installed_id() -> Id {
    Id::unique("fd.glyphs.installed")
}

/// Registers every icon's glyph on `ctx`. Safe to call more than once: a
/// context that already has the registry is left as it is.
///
/// Call it outside a pass, as `FuzzyDedupeApp::new` does and as a test
/// harness that builds its context without `FuzzyDedupeApp::new` must,
/// right after `theme::install`. Registered inside a pass, the glyphs would
/// be laid out before their rasterizers are active and panic under
/// `egui_kittest`'s missing-glyph policy.
///
/// Never fails: an icon whose SVG does not parse is logged and left out,
/// and [`text`] then returns `None` for it, so it is never drawn.
pub fn install(ctx: &Context) {
    if ctx
        .data(|d| d.get_temp::<Installed>(installed_id()))
        .is_some()
    {
        return;
    }
    let mut loaded = Vec::with_capacity(Icon::ALL.len());
    for &icon in Icon::ALL {
        match egui_extras::SvgGlyph::from_bytes(icon.bytes()) {
            Ok(svg) => {
                // Not a colour glyph, so it takes the text colour. These two
                // options fix every icon's drawn appearance in every view;
                // changing either restyles all of them.
                ctx.add_glyph_rasterizer(
                    svg.with_color(false)
                        .with_baseline_offset(-0.2)
                        .into_rasterizer(code_point(icon).to_string()),
                );
                loaded.push(icon);
            }
            Err(err) => tracing::warn!(icon = icon.lucide_name(), %err, "icon glyph not loaded"),
        }
    }
    ctx.data_mut(|d| d.insert_temp(installed_id(), Installed(loaded)));
}

/// `icon` as a one-character string to lay out as text, or `None` when it
/// cannot be drawn on `ctx`: [`install`] has not run there, or the icon's
/// SVG failed to parse. Callers draw nothing (or reserve the icon's space)
/// for `None`, never a substitute character.
#[must_use]
pub fn text(ctx: &Context, icon: Icon) -> Option<String> {
    let drawable = ctx.data(|d| {
        d.get_temp::<Installed>(installed_id())
            .is_some_and(|installed| installed.0.contains(&icon))
    });
    drawable.then(|| code_point(icon).to_string())
}
