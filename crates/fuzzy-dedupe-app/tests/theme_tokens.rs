// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! PV-T11-001 and the test-provable points of PV-T11-002 (plan T11).
//!
//! The oracle is the spec itself: the §11.7 token, text-style and geometry
//! statements are parsed out of `docs/specs/canonical/fuzzy-dedupe.md` at run
//! time rather than transcribed here. That keeps hex colour literals out of
//! every file except `ui/theme.rs` (plan T11 invariant), and it makes a spec
//! revision that changes a token fail this suite until the theme follows.
//! Each parser asserts the row count it expects, so a reformatted table fails
//! loudly instead of passing over zero rows.
//!
//! Checksums use a local SHA-256 because the app manifest, which T11a settled,
//! has no hashing crate and this leg may not add one.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use egui::{Color32, FontFamily, TextStyle};
use fuzzy_dedupe_app::ui::theme::{
    self, CompactStyle, FontFace, Icon, Mode, NOTO_EMOJI_SHA256, Token, Zoom,
};

/// WCAG 2.x minimum contrast for normal-size text (spec §11.7).
const MIN_TEXT_CONTRAST: f64 = 4.5;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
}

/// The text of spec §11.7, from its heading to the next `###` heading.
fn spec_section_11_7() -> String {
    let spec = read(&repo_root().join("docs/specs/canonical/fuzzy-dedupe.md"));
    let start = spec.find("### 11.7 ").expect("spec has a §11.7 heading");
    let rest = &spec[start..];
    let end = rest[4..].find("\n### ").map_or(rest.len(), |i| i + 4);
    rest[..end].to_owned()
}

fn cells(row: &str) -> Vec<String> {
    row.trim()
        .trim_matches('|')
        .split('|')
        .map(|cell| cell.trim().to_owned())
        .collect()
}

/// One spec colour: opaque RGB plus the stated opacity (100 when none).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SpecColor {
    rgb: [u8; 3],
    opacity_percent: u8,
}

fn parse_spec_color(text: &str) -> SpecColor {
    let text = text.trim();
    let hash = text
        .find('#')
        .unwrap_or_else(|| panic!("no colour in {text:?}"));
    let hex = &text[hash + 1..hash + 7];
    let channel = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).expect("hex channel");
    let opacity_percent = text.split(" at ").nth(1).map_or(100, |pct| {
        pct.trim()
            .trim_end_matches('%')
            .parse()
            .expect("opacity percent")
    });
    SpecColor {
        rgb: [channel(0), channel(2), channel(4)],
        opacity_percent,
    }
}

/// Token name → (light, dark), from the §11.7 token table.
fn spec_tokens() -> BTreeMap<String, (SpecColor, SpecColor)> {
    let section = spec_section_11_7();
    let mut tokens = BTreeMap::new();
    for line in section.lines().filter(|l| l.starts_with("| `")) {
        let row = cells(line);
        let names: Vec<&str> = row[0].split(" / ").map(|n| n.trim_matches('`')).collect();
        let light: Vec<&str> = row[1].split(" / ").collect();
        let dark: Vec<&str> = row[2].split(" / ").collect();
        assert_eq!(names.len(), light.len(), "light cell count in {line:?}");
        assert_eq!(names.len(), dark.len(), "dark cell count in {line:?}");
        for ((name, l), d) in names.iter().zip(&light).zip(&dark) {
            let previous = tokens.insert(
                (*name).to_owned(),
                (parse_spec_color(l), parse_spec_color(d)),
            );
            assert!(previous.is_none(), "token {name} listed twice");
        }
    }
    // 19 table rows expand to 26 tokens; a count drift means the table
    // changed shape and this parser, not the theme, needs review.
    assert_eq!(tokens.len(), 26, "spec §11.7 token count");
    tokens
}

/// Style label → (family/weight, size, line height), from the §11.7 style table.
fn spec_styles() -> BTreeMap<String, (String, u8, u8)> {
    let section = spec_section_11_7();
    let mut lines = section
        .lines()
        .skip_while(|l| !l.contains("Compact text style"));
    lines.next().expect("style table header");
    lines.next().expect("style table separator");
    let mut styles = BTreeMap::new();
    for line in lines.take_while(|l| l.starts_with('|')) {
        let row = cells(line);
        let (size, line_height) = row[2].split_once(" / ").expect("size / line height");
        styles.insert(
            row[0].clone(),
            (
                row[1].clone(),
                size.trim().parse().expect("size"),
                line_height.trim().parse().expect("line height"),
            ),
        );
    }
    assert_eq!(styles.len(), 8, "spec §11.7 compact style count");
    styles
}

fn number_before(text: &str, marker: &str) -> u8 {
    let end = text
        .find(marker)
        .unwrap_or_else(|| panic!("spec lacks {marker:?}"));
    let digits: String = text[..end]
        .chars()
        .rev()
        .take_while(char::is_ascii_digit)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    digits.parse().expect("number before marker")
}

fn spec_geometry_scale() -> Vec<u8> {
    let section = spec_section_11_7();
    let start = section.find("scale of ").expect("geometry scale sentence") + "scale of ".len();
    let end = start + section[start..].find(" units").expect("scale end");
    section[start..end]
        .replace(" and ", ", ")
        .split(", ")
        .map(|n| n.trim().parse().expect("scale step"))
        .collect()
}

fn rgb(color: Color32) -> [u8; 3] {
    [color.r(), color.g(), color.b()]
}

fn same(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
}

fn relative_luminance(rgb: [u8; 3]) -> f64 {
    let linear = |c: u8| {
        let c = f64::from(c) / 255.0;
        if c <= 0.040_45 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(rgb[0]) + 0.7152 * linear(rgb[1]) + 0.0722 * linear(rgb[2])
}

fn contrast_ratio(a: [u8; 3], b: [u8; 3]) -> f64 {
    let (la, lb) = (relative_luminance(a), relative_luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

#[test]
fn theme_tokens_every_token_in_both_modes_equals_the_spec_table() {
    let spec = spec_tokens();
    let theme_names: BTreeSet<&str> = Token::ALL.iter().map(|t| t.name()).collect();
    let spec_names: BTreeSet<&str> = spec.keys().map(String::as_str).collect();
    assert_eq!(
        theme_names, spec_names,
        "theme token set differs from §11.7"
    );

    for token in Token::ALL {
        let (light, dark) = spec[token.name()];
        for (mode, want) in [(Mode::Light, light), (Mode::Dark, dark)] {
            let palette = theme::palette(mode);
            assert_eq!(
                rgb(palette.color(token)),
                want.rgb,
                "{} in {mode:?}",
                token.name()
            );
            let opacity = if token == Token::Shadow {
                palette.shadow_opacity_percent()
            } else {
                100
            };
            assert_eq!(
                opacity,
                want.opacity_percent,
                "{} opacity in {mode:?}",
                token.name()
            );
            if token != Token::Shadow {
                assert_eq!(palette.color(token).a(), 255, "{} is opaque", token.name());
            }
        }
    }
}

#[test]
fn theme_tokens_overlay_shadow_is_y4_blur16_no_spread() {
    for mode in Mode::ALL {
        let shadow = theme::shadow(mode);
        assert_eq!(shadow.offset, [0, 4], "{mode:?}");
        assert_eq!(shadow.blur, 16, "{mode:?}");
        assert_eq!(shadow.spread, 0, "{mode:?}");
        let visuals = theme::visuals(mode);
        assert_eq!(visuals.window_shadow, shadow, "{mode:?} window shadow");
        assert_eq!(visuals.popup_shadow, shadow, "{mode:?} popup shadow");
    }
}

#[test]
fn theme_tokens_each_style_has_its_stated_family_size_and_line_height() {
    let spec = spec_styles();
    let theme_labels: BTreeSet<&str> = CompactStyle::ALL.iter().map(|s| s.label()).collect();
    let spec_labels: BTreeSet<&str> = spec.keys().map(String::as_str).collect();
    assert_eq!(
        theme_labels, spec_labels,
        "theme style set differs from §11.7"
    );

    let fonts = theme::font_definitions(Vec::new());
    for style in CompactStyle::ALL {
        let (family_weight, size, line_height) = &spec[style.label()];
        let face = style.face();
        assert_eq!(face.display_name(), family_weight, "{} face", style.label());
        assert!(
            same(style.size(), f32::from(*size)),
            "{} size",
            style.label()
        );
        assert!(
            same(style.line_height(), f32::from(*line_height)),
            "{} line height",
            style.label()
        );
        // The egui family the style resolves to must lead with that face's
        // embedded file, or the stated weight would not be what renders.
        let font_id = style.font_id();
        assert_eq!(font_id.family, face.family(), "{} family", style.label());
        assert!(
            same(font_id.size, style.size()),
            "{} font id size",
            style.label()
        );
        assert_eq!(
            fonts.families[&font_id.family].first().map(String::as_str),
            Some(face.data_name()),
            "{} primary font",
            style.label()
        );
    }
}

#[test]
fn theme_tokens_no_style_is_below_12_units() {
    let floor = f32::from(number_before(&spec_section_11_7(), " units. Routine rows"));
    assert!(same(floor, 12.0), "spec floor moved: {floor}");
    for style in CompactStyle::ALL {
        assert!(
            style.size() >= floor,
            "{} is {}",
            style.label(),
            style.size()
        );
    }
    // Every egui text style the theme installs, including egui's built-in
    // names that stock widgets fall back to, respects the same floor.
    for mode in Mode::ALL {
        let mut style = egui::Style::default();
        theme::apply_style(&mut style, mode);
        for (text_style, font_id) in &style.text_styles {
            assert!(font_id.size >= floor, "{text_style:?} is {}", font_id.size);
        }
        for builtin in [
            TextStyle::Small,
            TextStyle::Body,
            TextStyle::Button,
            TextStyle::Heading,
            TextStyle::Monospace,
        ] {
            assert!(
                style.text_styles.contains_key(&builtin),
                "{builtin:?} missing"
            );
        }
    }
}

#[test]
fn theme_tokens_declared_text_pairings_meet_wcag_4_5_to_1() {
    assert!(!theme::TEXT_PAIRINGS.is_empty());
    let mut failures = Vec::new();
    for mode in Mode::ALL {
        let palette = theme::palette(mode);
        for &(text, background) in theme::TEXT_PAIRINGS {
            let ratio = contrast_ratio(rgb(palette.color(text)), rgb(palette.color(background)));
            if ratio < MIN_TEXT_CONTRAST {
                failures.push(format!(
                    "{} on {} in {mode:?}: {ratio:.2}:1",
                    text.name(),
                    background.name()
                ));
            }
        }
    }
    assert!(failures.is_empty(), "below 4.5:1: {failures:#?}");
}

#[test]
fn theme_tokens_radius_and_geometry_scale_match_the_spec() {
    let section = spec_section_11_7();
    assert_eq!(
        theme::CORNER_RADIUS,
        number_before(&section, "-unit radius")
    );
    assert_eq!(theme::SPACING_SCALE.to_vec(), spec_geometry_scale());
    for mode in Mode::ALL {
        let visuals = theme::visuals(mode);
        assert_eq!(visuals.window_corner_radius, egui::CornerRadius::same(5));
        assert_eq!(visuals.menu_corner_radius, egui::CornerRadius::same(5));
        assert_eq!(
            visuals.widgets.inactive.corner_radius,
            egui::CornerRadius::same(5)
        );
    }
}

#[test]
fn theme_tokens_zoom_runs_80_to_200_in_10_point_steps() {
    let steps: Vec<u16> = Zoom::all().map(Zoom::percent).collect();
    let expected: Vec<u16> = (80..=200).step_by(10).collect();
    assert_eq!(steps, expected);
    assert_eq!(Zoom::default().percent(), 100);
    assert!(same(Zoom::default().factor(), 1.0));

    let mut zoom = Zoom::default();
    for _ in 0..20 {
        zoom = zoom.zoom_in();
    }
    assert_eq!(zoom.percent(), 200, "zoom in saturates at 200%");
    for _ in 0..20 {
        zoom = zoom.zoom_out();
    }
    assert_eq!(zoom.percent(), 80, "zoom out saturates at 80%");
    assert!(same(zoom.factor(), 0.8));
    assert_eq!(zoom.reset().percent(), 100);

    assert_eq!(Zoom::from_percent(150).map(Zoom::percent), Some(150));
    for off_grid in [0, 70, 79, 81, 155, 210, u16::MAX] {
        assert_eq!(Zoom::from_percent(off_grid), None, "{off_grid}% accepted");
    }
}

#[test]
fn theme_tokens_noto_emoji_follows_plex_then_caller_fallbacks_in_every_family() {
    let fallback = || egui::FontData::from_static(FontFace::SansRegular.bytes());
    let fonts = theme::font_definitions(vec![
        ("system-cjk".to_owned(), fallback()),
        ("system-arabic".to_owned(), fallback()),
    ]);
    let families: Vec<FontFamily> = FontFace::ALL.iter().map(|f| f.family()).collect();
    assert_eq!(fonts.families.len(), families.len(), "unexpected families");
    for face in FontFace::ALL {
        let chain = &fonts.families[&face.family()];
        assert_eq!(
            chain,
            &[
                face.data_name(),
                theme::NOTO_EMOJI_FONT_NAME,
                "system-cjk",
                "system-arabic",
            ],
            "{face:?} fallback chain"
        );
    }
    for name in fonts.families.values().flatten() {
        assert!(fonts.font_data.contains_key(name), "{name} has no data");
    }
}

#[test]
fn theme_tokens_a_colliding_fallback_never_replaces_an_embedded_font() {
    let impostor = egui::FontData::from_static(FontFace::MonoRegular.bytes());
    let fonts = theme::font_definitions(vec![(
        FontFace::SansRegular.data_name().to_owned(),
        impostor,
    )]);
    assert_eq!(
        fonts.font_data[FontFace::SansRegular.data_name()].bytes(),
        FontFace::SansRegular.bytes(),
    );
    for chain in fonts.families.values() {
        assert_eq!(chain.len(), 2, "colliding fallback was appended: {chain:?}");
    }
}

#[test]
fn theme_tokens_egui_bundled_fonts_are_absent() {
    // Empty only when epaint's `default_fonts` feature is off in this build's
    // resolved graph, which includes the dev-dependencies (plan D-001).
    assert!(egui::FontDefinitions::builtin_font_names().is_empty());
    assert_eq!(
        egui::FontDefinitions::default(),
        egui::FontDefinitions::empty()
    );
}

#[test]
fn theme_tokens_every_embedded_icon_is_in_the_inventory_and_parses() {
    let dir = crate_dir().join("assets/icons");
    let on_disk: BTreeSet<String> = fs::read_dir(&dir)
        .expect("icon directory")
        .map(|entry| {
            entry
                .expect("dir entry")
                .file_name()
                .into_string()
                .expect("utf-8")
        })
        .collect();
    let inventory: BTreeSet<String> = Icon::ALL
        .iter()
        .map(|icon| format!("{}.svg", icon.lucide_name()))
        .collect();
    assert_eq!(on_disk, inventory, "assets/icons and Icon::ALL differ");

    for icon in Icon::ALL {
        let path = dir.join(format!("{}.svg", icon.lucide_name()));
        assert_eq!(
            icon.bytes(),
            fs::read(&path).expect("icon file"),
            "{icon:?}"
        );
        assert_eq!(
            icon.uri().rsplit_once('.').map(|(_, ext)| ext),
            Some("svg"),
            "{icon:?} uri needs the svg loader"
        );
        egui_extras::SvgGlyph::from_bytes(icon.bytes())
            .unwrap_or_else(|err| panic!("{icon:?} does not parse: {err}"));
    }
}

#[test]
fn theme_tokens_noto_emoji_checksum_matches_code_and_notices() {
    let path = crate_dir().join("assets/fonts/NotoEmoji-Regular.ttf");
    let actual = sha256_hex(&fs::read(&path).expect("Noto Emoji file"));
    assert_eq!(
        actual, NOTO_EMOJI_SHA256,
        "file differs from the pinned release"
    );
    assert_eq!(sha256_hex(FontFace::noto_emoji_bytes()), NOTO_EMOJI_SHA256);

    let notices = notice_checksums();
    assert_eq!(
        notices.get("crates/fuzzy-dedupe-app/assets/fonts/NotoEmoji-Regular.ttf"),
        Some(&NOTO_EMOJI_SHA256.to_owned()),
        "THIRD-PARTY-NOTICES.md entry"
    );
}

#[test]
fn theme_tokens_every_notice_checksum_matches_its_file() {
    let notices = notice_checksums();
    // Four Plex fonts, Noto Emoji, and one row per embedded icon.
    assert_eq!(notices.len(), 5 + Icon::ALL.len(), "notice table rows");
    for (path, want) in &notices {
        let bytes = fs::read(repo_root().join(path)).unwrap_or_else(|e| panic!("{path}: {e}"));
        assert_eq!(&sha256_hex(&bytes), want, "{path}");
    }
}

#[test]
fn theme_tokens_no_hex_colour_literal_outside_theme_rs() {
    let src = crate_dir().join("src");
    let theme_rs = src.join("ui/theme.rs");
    let mut offenders = Vec::new();
    let mut stack = vec![src];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).expect("src dir") {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                stack.push(path);
            } else if path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("rs"))
                && path != theme_rs
            {
                let text = read(&path);
                if has_hex_colour(&text) || text.contains("Color32::from_rgb") {
                    offenders.push(path.display().to_string());
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "colour literals outside theme.rs: {offenders:?}"
    );
}

/// True for a `#` followed by exactly six hex digits, the §11.7 colour form.
fn has_hex_colour(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.iter().enumerate().any(|(i, &b)| {
        b == b'#'
            && bytes.len() >= i + 7
            && bytes[i + 1..i + 7].iter().all(u8::is_ascii_hexdigit)
            && bytes.get(i + 7).is_none_or(|c| !c.is_ascii_hexdigit())
    })
}

/// Repository path → SHA-256 from every `| `path` | `sha` |` notice row.
fn notice_checksums() -> BTreeMap<String, String> {
    let notices = read(&repo_root().join("THIRD-PARTY-NOTICES.md"));
    notices
        .lines()
        .filter(|l| l.starts_with("| `crates/"))
        .map(|line| {
            let row = cells(line);
            (
                row[0].trim_matches('`').to_owned(),
                row[1].trim_matches('`').to_owned(),
            )
        })
        .collect()
}

/// SHA-256 round constants (FIPS 180-4 §4.2.2).
const SHA256_K: [u32; 64] = [
    0x428a_2f98,
    0x7137_4491,
    0xb5c0_fbcf,
    0xe9b5_dba5,
    0x3956_c25b,
    0x59f1_11f1,
    0x923f_82a4,
    0xab1c_5ed5,
    0xd807_aa98,
    0x1283_5b01,
    0x2431_85be,
    0x550c_7dc3,
    0x72be_5d74,
    0x80de_b1fe,
    0x9bdc_06a7,
    0xc19b_f174,
    0xe49b_69c1,
    0xefbe_4786,
    0x0fc1_9dc6,
    0x240c_a1cc,
    0x2de9_2c6f,
    0x4a74_84aa,
    0x5cb0_a9dc,
    0x76f9_88da,
    0x983e_5152,
    0xa831_c66d,
    0xb003_27c8,
    0xbf59_7fc7,
    0xc6e0_0bf3,
    0xd5a7_9147,
    0x06ca_6351,
    0x1429_2967,
    0x27b7_0a85,
    0x2e1b_2138,
    0x4d2c_6dfc,
    0x5338_0d13,
    0x650a_7354,
    0x766a_0abb,
    0x81c2_c92e,
    0x9272_2c85,
    0xa2bf_e8a1,
    0xa81a_664b,
    0xc24b_8b70,
    0xc76c_51a3,
    0xd192_e819,
    0xd699_0624,
    0xf40e_3585,
    0x106a_a070,
    0x19a4_c116,
    0x1e37_6c08,
    0x2748_774c,
    0x34b0_bcb5,
    0x391c_0cb3,
    0x4ed8_aa4a,
    0x5b9c_ca4f,
    0x682e_6ff3,
    0x748f_82ee,
    0x78a5_636f,
    0x84c8_7814,
    0x8cc7_0208,
    0x90be_fffa,
    0xa450_6ceb,
    0xbef9_a3f7,
    0xc671_78f2,
];

/// FIPS 180-4 SHA-256, lower-case hex.
fn sha256_hex(data: &[u8]) -> String {
    let mut state: [u32; 8] = [
        0x6a09_e667,
        0xbb67_ae85,
        0x3c6e_f372,
        0xa54f_f53a,
        0x510e_527f,
        0x9b05_688c,
        0x1f83_d9ab,
        0x5be0_cd19,
    ];
    let bit_len = u64::try_from(data.len()).expect("length fits u64") * 8;
    let mut message = data.to_vec();
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bit_len.to_be_bytes());
    for block in message.chunks_exact(64) {
        sha256_compress(&mut state, block);
    }
    state
        .iter()
        .fold(String::with_capacity(64), |mut hex, word| {
            write!(hex, "{word:08x}").expect("writing to a String cannot fail");
            hex
        })
}

/// One 64-byte block of the SHA-256 compression function.
fn sha256_compress(state: &mut [u32; 8], block: &[u8]) {
    let mut schedule = [0u32; 64];
    for (slot, word) in schedule.iter_mut().zip(block.chunks_exact(4)) {
        *slot = u32::from_be_bytes(word.try_into().expect("4 bytes"));
    }
    for i in 16..64 {
        let (w15, w2) = (schedule[i - 15], schedule[i - 2]);
        let sigma0 = w15.rotate_right(7) ^ w15.rotate_right(18) ^ (w15 >> 3);
        let sigma1 = w2.rotate_right(17) ^ w2.rotate_right(19) ^ (w2 >> 10);
        schedule[i] = schedule[i - 16]
            .wrapping_add(sigma0)
            .wrapping_add(schedule[i - 7])
            .wrapping_add(sigma1);
    }
    // Working variables a..h (here va..vh) live in `work[0..8]`; each round
    // shifts them down one slot and writes the new a and e.
    let mut work = *state;
    for (round_k, word) in SHA256_K.iter().zip(schedule) {
        let [va, vb, vc, _, ve, vf, vg, vh] = work;
        let big_sigma1 = ve.rotate_right(6) ^ ve.rotate_right(11) ^ ve.rotate_right(25);
        let choose = (ve & vf) ^ (!ve & vg);
        let temp1 = vh
            .wrapping_add(big_sigma1)
            .wrapping_add(choose)
            .wrapping_add(*round_k)
            .wrapping_add(word);
        let big_sigma0 = va.rotate_right(2) ^ va.rotate_right(13) ^ va.rotate_right(22);
        let majority = (va & vb) ^ (va & vc) ^ (vb & vc);
        work.rotate_right(1);
        work[0] = temp1.wrapping_add(big_sigma0.wrapping_add(majority));
        work[4] = work[4].wrapping_add(temp1);
    }
    for (slot, value) in state.iter_mut().zip(work) {
        *slot = slot.wrapping_add(value);
    }
}

#[test]
fn theme_tokens_sha256_matches_the_fips_test_vectors() {
    // FIPS 180-4 examples: empty input and "abc", so a checksum failure above
    // can be trusted to be the file, not this implementation.
    assert_eq!(
        sha256_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}
