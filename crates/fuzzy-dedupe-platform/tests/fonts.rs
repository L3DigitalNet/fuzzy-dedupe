// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! PV-T21-001 (plan T21, exit item 7): glyph coverage of the `unicode`
//! scenario's Japanese, Arabic and emoji file names.
//!
//! Every test sits in the `fonts` module so `-E 'test(fonts)'` selects them
//! all.
//!
//! Coverage is decided from each font's own `cmap` table, read from face 0
//! of the bytes the adapter returns. Face 0 is deliberate: the app registers
//! a [`SystemFont`] with `egui::FontData::from_owned`, which always uses face
//! 0, so checking any other face would prove a glyph egui never sees.
//!
//! Only the system-font test is `#[ignore]`d: it needs the reference
//! workstation's installed fonts (plan §3.4 reference-workstation
//! exception), so rexec and CI never depend on them. The emoji check and the
//! negative control read only the embedded app fonts and run everywhere.
//!
//! Requirements: the embedded IBM Plex Sans Regular and Noto Emoji files in
//! `crates/fuzzy-dedupe-app/assets/fonts/` (included at compile time, so a
//! moved file fails the build rather than the check). The platform crate
//! may not depend on the app crate (plan C-1), so the files are read by path.

mod fonts {
    use fuzzy_dedupe_platform::fonts::{FontLookup, SystemFont, SystemFontLookup};

    /// The app's primary face for Findings rows; the first in egui's chain.
    static PLEX_SANS: &[u8] =
        include_bytes!("../../fuzzy-dedupe-app/assets/fonts/IBMPlexSans-Regular.ttf");
    /// The embedded emoji fallback, second in egui's chain (plan T11).
    static NOTO_EMOJI: &[u8] =
        include_bytes!("../../fuzzy-dedupe-app/assets/fonts/NotoEmoji-Regular.ttf");

    // The names below are copied from the `unicode` scenario in
    // crates/fuzzy-dedupe-engine/src/fixtures/scenarios.rs (fn `unicode`):
    // folder segments and file names, each a separate rendered label. The
    // platform crate cannot depend on the engine (plan C-1), so an edit to
    // that scenario's names must be mirrored here.

    /// Japanese folder segments and file names.
    const JAPANESE: &[&str] = &[
        "文書",
        "会議",
        "議事録_ミーティング.txt",
        "プロジェクト",
        "二〇二六年度",
        "要件定義",
        "レビュー済み",
        "最終版",
        "アーカイブ",
        "長いフォルダー名の例",
        "非常に長いファイル名の例_要件定義書_最終版_確定.txt",
    ];

    /// Arabic folder segments and file names.
    const ARABIC: &[&str] = &["أرشيف", "تقرير_الاجتماع.txt", "نسخة.txt"];

    /// The emoji file name.
    const EMOJI: &[&str] = &["📝 notes ✅.txt"];

    // ── Coverage ─────────────────────────────────────────────────────────

    /// The characters of `name` no font in `faces` maps to a glyph.
    fn uncovered(name: &str, faces: &[&[u8]]) -> Vec<char> {
        name.chars()
            .filter(|&c| !faces.iter().any(|face| has_glyph(face, c)))
            .collect()
    }

    /// For each name, the characters Plex lacks that none of `fallbacks`
    /// covers either. Noto Emoji is left out on purpose: PV-T21-001 requires
    /// Japanese and Arabic glyphs from a fontconfig-supplied font.
    fn missing_with(
        names: &[&'static str],
        fallbacks: &[SystemFont],
    ) -> Vec<(&'static str, Vec<char>)> {
        let mut faces: Vec<&[u8]> = vec![PLEX_SANS];
        faces.extend(fallbacks.iter().map(|font| font.data.as_slice()));
        names
            .iter()
            .map(|&name| (name, uncovered(name, &faces)))
            .filter(|(_, missing)| !missing.is_empty())
            .collect()
    }

    #[test]
    #[ignore = "workstation only: needs the reference workstation's system fonts (plan §3.4)"]
    fn fonts_system_fallbacks_cover_japanese_and_arabic_names() {
        let fallbacks = SystemFontLookup.fallback_fonts();
        for font in &fallbacks {
            // Printed for EV-001: the families the adapter selected.
            println!(
                "fallback family: {} ({} bytes)",
                font.family,
                font.data.len()
            );
        }
        assert!(
            !fallbacks.is_empty(),
            "the adapter supplied no fallback fonts"
        );

        let japanese = missing_with(JAPANESE, &fallbacks);
        assert!(
            japanese.is_empty(),
            "Japanese names lack glyphs: {japanese:?}"
        );
        let arabic = missing_with(ARABIC, &fallbacks);
        assert!(arabic.is_empty(), "Arabic names lack glyphs: {arabic:?}");

        // Every script character must come from a fontconfig font, not from
        // Plex: a check passing only on Plex would mean the names changed.
        for name in JAPANESE.iter().chain(ARABIC) {
            for c in name.chars().filter(|&c| !has_glyph(PLEX_SANS, c)) {
                assert!(
                    fallbacks.iter().any(|font| has_glyph(&font.data, c)),
                    "{c:?} in {name:?} has no fontconfig glyph"
                );
            }
        }

        let families: Vec<&str> = fallbacks.iter().map(|f| f.family.as_str()).collect();
        let mut unique = families.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(
            unique.len(),
            families.len(),
            "duplicate family keys: {families:?}"
        );
    }

    #[test]
    fn fonts_embedded_noto_emoji_covers_the_emoji_names() {
        for name in EMOJI {
            let missing: Vec<char> = name
                .chars()
                .filter(|&c| !has_glyph(PLEX_SANS, c) && !has_glyph(NOTO_EMOJI, c))
                .collect();
            assert!(
                missing.is_empty(),
                "{name:?} lacks emoji glyphs: {missing:?}"
            );
            // The pictographs themselves must come from Noto Emoji.
            for c in name.chars().filter(|c| !c.is_ascii()) {
                assert!(has_glyph(NOTO_EMOJI, c), "{c:?} missing from Noto Emoji");
            }
        }
    }

    /// The negative control: with the adapter disabled (no fallbacks) the
    /// same check reports missing glyphs in every Japanese and Arabic name,
    /// so the system-font test cannot pass on the embedded faces alone.
    #[test]
    fn fonts_without_the_adapter_japanese_and_arabic_lack_glyphs() {
        let japanese = missing_with(JAPANESE, &[]);
        assert_eq!(
            japanese.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
            JAPANESE,
            "every Japanese name lacks glyphs without fallbacks"
        );
        let arabic = missing_with(ARABIC, &[]);
        assert_eq!(
            arabic.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
            ARABIC,
            "every Arabic name lacks glyphs without fallbacks"
        );
        for (name, missing) in japanese.iter().chain(&arabic) {
            println!(
                "without the adapter, {name:?} lacks {} glyphs",
                missing.len()
            );
        }
    }

    /// The parser itself: Plex has Latin and lacks CJK; Noto Emoji has the
    /// memo pictograph, which needs a format 12 subtable (it is above the
    /// BMP), so both subtable formats are exercised on known answers.
    #[test]
    fn fonts_cmap_reader_answers_known_glyphs() {
        assert!(has_glyph(PLEX_SANS, 'a'));
        assert!(has_glyph(PLEX_SANS, '_'));
        assert!(!has_glyph(PLEX_SANS, '会'));
        assert!(has_glyph(NOTO_EMOJI, '📝'));
        assert!(!has_glyph(NOTO_EMOJI, '会'));
    }

    // ── A minimal cmap reader ────────────────────────────────────────────
    //
    // Hand-rolled rather than a font-parsing crate: no such crate has an
    // approved spec §8.3 row (plan §3.4 Dependencies), and coverage needs
    // only the sfnt table directory plus cmap formats 4 and 12, which every
    // Unicode font this check meets carries. Any malformed offset reads as
    // "no glyph", so a damaged font fails the check instead of panicking.

    fn u16_at(data: &[u8], at: usize) -> Option<u16> {
        Some(u16::from_be_bytes(data.get(at..at + 2)?.try_into().ok()?))
    }

    fn u32_at(data: &[u8], at: usize) -> Option<u32> {
        Some(u32::from_be_bytes(data.get(at..at + 4)?.try_into().ok()?))
    }

    fn has_glyph(data: &[u8], c: char) -> bool {
        glyph_id(data, u32::from(c)).is_some_and(|glyph| glyph != 0)
    }

    fn glyph_id(data: &[u8], code: u32) -> Option<u32> {
        // A collection ('ttcf') lists its faces' offsets; take face 0, as
        // egui does.
        let face = if data.get(0..4)? == b"ttcf" {
            u32_at(data, 12)? as usize
        } else {
            0
        };
        let tables = u16_at(data, face + 4)? as usize;
        let cmap = (0..tables)
            .map(|i| face + 12 + i * 16)
            .find(|&record| data.get(record..record + 4) == Some(b"cmap"))
            .and_then(|record| u32_at(data, record + 8))? as usize;
        let subtables = u16_at(data, cmap + 2)? as usize;
        (0..subtables).find_map(|i| {
            let record = cmap + 4 + i * 8;
            let platform = u16_at(data, record)?;
            let encoding = u16_at(data, record + 2)?;
            // Unicode subtables only: platform 0 (any), or Windows BMP (1)
            // and full repertoire (10).
            if !(platform == 0 || (platform == 3 && matches!(encoding, 1 | 10))) {
                return None;
            }
            let table = cmap + u32_at(data, record + 4)? as usize;
            match u16_at(data, table)? {
                4 => format4(data, table, code),
                12 => format12(data, table, code),
                _ => None,
            }
            .filter(|&glyph| glyph != 0)
        })
    }

    fn format4(data: &[u8], table: usize, code: u32) -> Option<u32> {
        let code = u16::try_from(code).ok()?;
        let segments = (u16_at(data, table + 6)? / 2) as usize;
        let ends = table + 14;
        let starts = ends + segments * 2 + 2;
        let deltas = starts + segments * 2;
        let range_offsets = deltas + segments * 2;
        for i in 0..segments {
            let end = u16_at(data, ends + i * 2)?;
            if code > end {
                continue;
            }
            let start = u16_at(data, starts + i * 2)?;
            if code < start {
                return None;
            }
            let delta = u16_at(data, deltas + i * 2)?;
            let at = range_offsets + i * 2;
            let range_offset = u16_at(data, at)?;
            if range_offset == 0 {
                return Some(u32::from(code.wrapping_add(delta)));
            }
            let glyph_at = at + range_offset as usize + (code - start) as usize * 2;
            let glyph = u16_at(data, glyph_at)?;
            return (glyph != 0).then(|| u32::from(glyph.wrapping_add(delta)));
        }
        None
    }

    fn format12(data: &[u8], table: usize, code: u32) -> Option<u32> {
        let groups = u32_at(data, table + 12)? as usize;
        (0..groups).find_map(|i| {
            let group = table + 16 + i * 12;
            let start = u32_at(data, group)?;
            let end = u32_at(data, group + 4)?;
            (start..=end)
                .contains(&code)
                .then(|| u32_at(data, group + 8).map(|first| first + (code - start)))?
        })
    }
}
