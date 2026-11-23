// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! System fallback-font lookup (C-07, spec §8.5), for glyphs the embedded
//! IBM Plex and Noto Emoji faces lack.
//!
//! A stub until plan T21 fills it with the fontconfig lookup: it returns no
//! fonts, so the app renders with its embedded faces alone. The trait shape
//! is what the app codes against now; T21 owns this file and may refine it.

/// One system font to register as a fallback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemFont {
    /// The family name, used as the font's registration key and as a hint
    /// for ordering fallbacks.
    pub family: String,
    /// The font file's bytes (TrueType, OpenType or a collection).
    pub data: Vec<u8>,
}

/// Looks up system fonts to use as fallbacks.
pub trait FontLookup {
    /// The fallback fonts, in preference order. Never fails: an empty list
    /// means the embedded faces are used alone.
    fn fallback_fonts(&self) -> Vec<SystemFont>;
}

/// The system implementation of [`FontLookup`]; returns nothing until T21.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemFontLookup;

impl FontLookup for SystemFontLookup {
    fn fallback_fonts(&self) -> Vec<SystemFont> {
        Vec::new()
    }
}
