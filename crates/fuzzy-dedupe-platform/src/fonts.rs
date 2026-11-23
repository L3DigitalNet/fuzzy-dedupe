// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! System fallback-font lookup (C-07, spec §8.5 and §11.7 Fonts), for
//! glyphs the embedded IBM Plex and Noto Emoji faces lack.
//!
//! On Linux, [`SystemFontLookup`] asks fontconfig for one sans-serif font
//! per script Plex lacks: Japanese (with it the CJK ideographs Chinese
//! names share), Arabic, Hebrew and Thai, in that order. The app appends
//! the result after Plex and Noto Emoji in every egui family
//! (`crates/fuzzy-dedupe-app/src/ui/theme.rs`, `font_definitions`), so the
//! order here only decides which fallback wins for a character two of them
//! share. Other targets return no fonts until they get their own adapter.
//!
//! The lookup reads whole font files (a CJK collection is tens of
//! megabytes) and is meant to run once at startup, never per frame:
//! `FuzzyDedupeApp::new` calls it exactly once.

/// One system font to register as a fallback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemFont {
    /// The family name, used as the font's registration key and as a hint
    /// for ordering fallbacks. Unique within one lookup's result.
    pub family: String,
    /// The font file's bytes (TrueType, OpenType or a collection). Face 0
    /// is the face that covers the script: the app registers the bytes
    /// with `egui::FontData::from_owned`, which always reads face 0, so the
    /// adapter never returns a collection whose matching face is another.
    pub data: Vec<u8>,
}

/// Looks up system fonts to use as fallbacks.
pub trait FontLookup {
    /// The fallback fonts, in preference order. Never fails: an empty list
    /// means the embedded faces are used alone.
    fn fallback_fonts(&self) -> Vec<SystemFont>;
}

/// The system implementation of [`FontLookup`]: fontconfig on Linux,
/// nothing elsewhere.
///
/// Degradation: a missing `libfontconfig` (it is loaded at run time, not
/// linked), a script with no installed font, or an unreadable font file
/// drops that font and keeps the rest; the app then shows replacement boxes
/// for that script only, which is the behavior before this adapter existed.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemFontLookup;

impl FontLookup for SystemFontLookup {
    fn fallback_fonts(&self) -> Vec<SystemFont> {
        #[cfg(target_os = "linux")]
        {
            linux::fallback_fonts()
        }
        #[cfg(not(target_os = "linux"))]
        {
            Vec::new()
        }
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use std::ffi::CStr;

    use fontconfig::{FC_FAMILY, FC_LANG, FontFormat, Fontconfig, Pattern, UnicodeCoverage};

    use super::SystemFont;

    /// fontconfig language tags for the scripts Plex lacks, in fallback
    /// order. A font qualifies for a tag only when fontconfig's own
    /// orthography coverage lists it, which is how a fallback is known to
    /// hold the script's glyphs without parsing the file here. `ja` rather
    /// than `zh-*` because the reference fixtures are Japanese, and a
    /// Japanese CJK face also covers the common ideographs.
    const SCRIPTS: [&CStr; 4] = [c"ja", c"ar", c"he", c"th"];

    pub(super) fn fallback_fonts() -> Vec<SystemFont> {
        // `None` when the library cannot be loaded or initialised: no
        // fallbacks, never a startup failure.
        let Some(fc) = Fontconfig::new() else {
            return Vec::new();
        };
        let mut fonts: Vec<SystemFont> = Vec::new();
        let mut files: Vec<String> = Vec::new();
        for lang in SCRIPTS {
            let Some((family, file)) = best_face(&fc, lang) else {
                continue;
            };
            // One font file can serve two scripts (a pan-script family);
            // registering it twice would double the memory and, under the
            // same family key, overwrite the first entry in egui.
            if files.contains(&file) || fonts.iter().any(|font| font.family == family) {
                continue;
            }
            // Read, never written: the C-09 confinement covers only writes.
            let Ok(data) = std::fs::read(&file) else {
                continue;
            };
            files.push(file);
            fonts.push(SystemFont { family, data });
        }
        fonts
    }

    /// The best sans-serif face for `lang` that egui can use: an outline
    /// font that is face 0 of its file and whose coverage includes `lang`.
    ///
    /// Walks fontconfig's sorted candidates rather than taking its single
    /// best match, because the best match can be a later face of a
    /// collection (egui would load face 0 instead and silently render the
    /// wrong font) or, when no installed font covers the script, a font
    /// that lacks it.
    fn best_face(fc: &Fontconfig, lang: &CStr) -> Option<(String, String)> {
        let mut pattern = Pattern::new(fc).ok()?;
        pattern.add_string(FC_FAMILY, c"sans-serif").ok()?;
        pattern.add_string(FC_LANG, lang).ok()?;
        let candidates = pattern.sort_fonts(UnicodeCoverage::NoTrim).ok()?;
        let lang = lang.to_str().ok()?;
        candidates.iter().find_map(|candidate| {
            // The low 16 bits are the face; the high bits name a variable
            // font's instance, which egui ignores in favor of the default
            // instance, so any instance of face 0 is acceptable.
            if candidate.face_index().ok()? & 0xFFFF != 0 {
                return None;
            }
            if !matches!(
                candidate.format(),
                Ok(FontFormat::TrueType | FontFormat::CFF)
            ) {
                return None;
            }
            if !candidate.lang_set().ok()?.any(|covered| covered == lang) {
                return None;
            }
            let family = candidate.get_string(FC_FAMILY).ok()?.to_owned();
            let file = candidate.filename().ok()?.to_owned();
            Some((family, file))
        })
    }
}
