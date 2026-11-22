// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! The single source of every spec §11.7 visual token, text style and
//! embedded font and icon (plan T11).
//!
//! Views never construct a colour, font family or icon themselves: they ask
//! this module by name ([`Token`], [`CompactStyle`], [`Icon`]). This is the
//! only file that may contain a hex colour literal; `tests/theme_tokens.rs`
//! scans `src/` and fails on one anywhere else.
//!
//! Callers install the theme once at startup with [`install`], passing any
//! system fallback fonts the platform adapter found. The theme never looks a
//! font up itself (plan T11 invariant, T21): it only embeds IBM Plex and Noto
//! Emoji and appends what it is given.
//!
//! The spec tables are authoritative over the Figma file (spec §11.1
//! precedence); `tests/theme_tokens.rs` reads them straight from the spec, so a
//! token edited here without a spec change fails the suite.

use std::collections::BTreeMap;
use std::sync::Arc;

use egui::{
    Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Shadow, Stroke,
    TextStyle, Visuals,
};

/// Light or dark, matching the Figma `DD · Semantic` variable modes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mode {
    Light,
    Dark,
}

impl Mode {
    pub const ALL: [Self; 2] = [Self::Light, Self::Dark];

    /// The egui theme whose style slot this mode fills.
    #[must_use]
    pub const fn egui_theme(self) -> egui::Theme {
        match self {
            Self::Light => egui::Theme::Light,
            Self::Dark => egui::Theme::Dark,
        }
    }
}

/// Generates [`Token`] and both palettes from one table, so a token can never
/// exist in one mode and be missing from the other, and `Token::ALL` order is
/// the palette index order by construction.
macro_rules! tokens {
    ($($variant:ident = $name:literal, $light:literal, $dark:literal;)+) => {
        /// A spec §11.7 colour token, named as in the spec table.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Token {
            $($variant,)+
        }

        impl Token {
            /// Every token, in spec table order.
            pub const ALL: [Self; tokens!(@count $($variant)+)] = [$(Self::$variant,)+];

            /// The token's name in the spec table and the Figma variables.
            #[must_use]
            pub const fn name(self) -> &'static str {
                match self {
                    $(Self::$variant => $name,)+
                }
            }
        }

        const LIGHT_COLORS: [Color32; Token::ALL.len()] = [$(rgb($light),)+];
        const DARK_COLORS: [Color32; Token::ALL.len()] = [$(rgb($dark),)+];
    };
    (@count $($variant:ident)+) => { [$(tokens!(@one $variant),)+].len() };
    (@one $variant:ident) => { () };
}

// The `shadow` row's RGB only; its opacity is SHADOW_OPACITY_PERCENT, because
// egui stores Color32 premultiplied and a 6% alpha cannot round-trip the RGB.
tokens! {
    Canvas = "canvas", 0xF4_F6_F9, 0x11_14_18;
    Surface = "surface", 0xFF_FF_FF, 0x19_1D_23;
    Rail = "rail", 0xEB_EF_F5, 0x0D_10_14;
    Ink = "ink", 0x20_2C_3D, 0xE3_E8_EF;
    Muted = "muted", 0x5B_68_7A, 0xA6_B0_BD;
    Quiet = "quiet", 0x63_71_85, 0x8E_99_A8;
    Primary = "primary", 0x32_5C_C9, 0x8A_A8_F8;
    PrimarySoft = "primarySoft", 0xE9_EF_FD, 0x1E_2A_44;
    PrimaryHover = "primaryHover", 0x24_49_AA, 0xA9_C0_FB;
    Focus = "focus", 0x32_5C_C9, 0x8A_A8_F8;
    Green = "green", 0x23_6C_54, 0x6F_CF_9F;
    GreenSoft = "greenSoft", 0xE9_F4_EF, 0x15_30_2A;
    Amber = "amber", 0x8B_56_14, 0xE3_B0_5E;
    AmberSoft = "amberSoft", 0xFF_F2_DC, 0x33_28_0F;
    Red = "red", 0xAA_35_3B, 0xF2_87_8D;
    RedSoft = "redSoft", 0xFC_EC_EE, 0x3B_1B_1F;
    Line = "line", 0xDD_E3_EB, 0x2A_31_3B;
    Code = "code", 0xFA_FB_FD, 0x14_18_1D;
    White = "white", 0xFF_FF_FF, 0x0D_10_14;
    Shadow = "shadow", 0x10_18_28, 0x00_00_00;
    DiffA = "diffA", 0x5B_3A_9E, 0xB9_A2_F2;
    DiffASoft = "diffASoft", 0xF1_EB_FB, 0x26_1F_3D;
    DiffAStrong = "diffAStrong", 0xD9_C8_F4, 0x43_35_6E;
    DiffB = "diffB", 0x8B_56_14, 0xE3_B0_5E;
    DiffBSoft = "diffBSoft", 0xFF_F2_DC, 0x33_28_0F;
    DiffBStrong = "diffBStrong", 0xF7_D7_9A, 0x5A_43_12;
}

const fn rgb(hex: u32) -> Color32 {
    let [_, r, g, b] = hex.to_be_bytes();
    Color32::from_rgb(r, g, b)
}

/// Every token's value in one mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    colors: [Color32; Token::ALL.len()],
    shadow_opacity_percent: u8,
}

impl Palette {
    /// The token's opaque colour. For [`Token::Shadow`] this is the RGB only;
    /// use [`shadow`] for the overlay shadow with its opacity applied.
    #[must_use]
    pub const fn color(&self, token: Token) -> Color32 {
        self.colors[token as usize]
    }

    /// The `shadow` token's stated opacity: 6% light, 40% dark.
    #[must_use]
    pub const fn shadow_opacity_percent(&self) -> u8 {
        self.shadow_opacity_percent
    }
}

const LIGHT: Palette = Palette {
    colors: LIGHT_COLORS,
    shadow_opacity_percent: 6,
};

const DARK: Palette = Palette {
    colors: DARK_COLORS,
    shadow_opacity_percent: 40,
};

/// The palette for one mode.
#[must_use]
pub const fn palette(mode: Mode) -> &'static Palette {
    match mode {
        Mode::Light => &LIGHT,
        Mode::Dark => &DARK,
    }
}

/// Every text-on-background pairing the views may use, as (text, background).
///
/// `tests/theme_tokens.rs` proves each meets 4.5:1 in both modes (spec §11.7).
/// A view that needs a pairing not listed here adds it here first, so the
/// contrast proof always covers what ships. Deliberately absent, because they
/// fall below 4.5:1 in at least one mode: `quiet` on `rail`, `primarySoft`,
/// any soft tint or diff tint (hence `muted` for changed-row line numbers);
/// `muted` on `diffAStrong` and `diffBStrong`.
pub const TEXT_PAIRINGS: &[(Token, Token)] = &[
    // Main text on every surface it appears on, including selected and diff rows.
    (Token::Ink, Token::Canvas),
    (Token::Ink, Token::Surface),
    (Token::Ink, Token::Rail),
    (Token::Ink, Token::Code),
    (Token::Ink, Token::PrimarySoft),
    (Token::Ink, Token::GreenSoft),
    (Token::Ink, Token::AmberSoft),
    (Token::Ink, Token::RedSoft),
    (Token::Ink, Token::DiffASoft),
    (Token::Ink, Token::DiffAStrong),
    (Token::Ink, Token::DiffBSoft),
    (Token::Ink, Token::DiffBStrong),
    // Secondary text, and line numbers on changed rows.
    (Token::Muted, Token::Canvas),
    (Token::Muted, Token::Surface),
    (Token::Muted, Token::Rail),
    (Token::Muted, Token::Code),
    (Token::Muted, Token::PrimarySoft),
    (Token::Muted, Token::AmberSoft),
    (Token::Muted, Token::DiffASoft),
    (Token::Muted, Token::DiffBSoft),
    // Compact supporting text, including unchanged-row line numbers.
    (Token::Quiet, Token::Canvas),
    (Token::Quiet, Token::Surface),
    (Token::Quiet, Token::Code),
    // Selected navigation, links and selected text.
    (Token::Primary, Token::Canvas),
    (Token::Primary, Token::Surface),
    (Token::Primary, Token::Rail),
    (Token::Primary, Token::PrimarySoft),
    (Token::PrimaryHover, Token::Canvas),
    (Token::PrimaryHover, Token::Surface),
    (Token::PrimaryHover, Token::PrimarySoft),
    // Status labels on plain surfaces and on their own tint.
    (Token::Green, Token::Canvas),
    (Token::Green, Token::Surface),
    (Token::Green, Token::Code),
    (Token::Green, Token::GreenSoft),
    (Token::Amber, Token::Canvas),
    (Token::Amber, Token::Surface),
    (Token::Amber, Token::Code),
    (Token::Amber, Token::AmberSoft),
    (Token::Red, Token::Canvas),
    (Token::Red, Token::Surface),
    (Token::Red, Token::Code),
    (Token::Red, Token::RedSoft),
    // A and B side markers on plain rows and on their own row tint.
    (Token::DiffA, Token::Canvas),
    (Token::DiffA, Token::Surface),
    (Token::DiffA, Token::DiffASoft),
    (Token::DiffB, Token::Canvas),
    (Token::DiffB, Token::Surface),
    (Token::DiffB, Token::DiffBSoft),
    // Labels on filled primary and destructive buttons.
    (Token::White, Token::Primary),
    (Token::White, Token::Red),
];

/// Corner radius of shared compact controls, popups and menus, in units.
pub const CORNER_RADIUS: u8 = 5;

/// Width of the `line` border every raised surface keeps, in units.
pub const BORDER_WIDTH: f32 = 1.0;

/// The `DD · Geometry` spacing scale, in units. Spacing uses only these steps.
pub const SPACING_SCALE: [u8; 8] = [4, 6, 8, 12, 16, 24, 32, 40];

/// The one overlay shadow (y 4, blur 16, no spread), for popups and menus
/// only; routine rows and table sections never carry it.
#[must_use]
pub fn shadow(mode: Mode) -> Shadow {
    let palette = palette(mode);
    let rgb = palette.color(Token::Shadow);
    let alpha = (u16::from(palette.shadow_opacity_percent()) * 255 + 50) / 100;
    Shadow {
        offset: [0, 4],
        blur: 16,
        spread: 0,
        color: Color32::from_rgba_unmultiplied(
            rgb.r(),
            rgb.g(),
            rgb.b(),
            u8::try_from(alpha).unwrap_or(u8::MAX),
        ),
    }
}

/// egui's visuals for one mode, expressed in tokens.
///
/// Starts from egui's own light or dark visuals so fields this theme does not
/// set keep coherent defaults, then overrides every fill, stroke, text colour
/// and radius a stock widget draws with. Every text colour set here is a
/// declared [`TEXT_PAIRINGS`] entry against the fill it sits on.
#[must_use]
pub fn visuals(mode: Mode) -> Visuals {
    let p = palette(mode);
    let c = |token| p.color(token);
    let radius = CornerRadius::same(CORNER_RADIUS);
    let line = Stroke::new(BORDER_WIDTH, c(Token::Line));
    let ink = Stroke::new(BORDER_WIDTH, c(Token::Ink));

    let mut v = match mode {
        Mode::Light => Visuals::light(),
        Mode::Dark => Visuals::dark(),
    };
    v.override_text_color = None;
    // egui otherwise derives weak text by fading the strong colour, which
    // produces an untested pairing; `muted` is the spec's secondary text.
    v.weak_text_color = Some(c(Token::Muted));
    v.hyperlink_color = c(Token::Primary);
    v.faint_bg_color = c(Token::Code);
    v.extreme_bg_color = c(Token::Surface);
    v.text_edit_bg_color = Some(c(Token::Surface));
    v.code_bg_color = c(Token::Code);
    v.warn_fg_color = c(Token::Amber);
    v.error_fg_color = c(Token::Red);
    v.panel_fill = c(Token::Canvas);
    v.window_fill = c(Token::Surface);
    v.window_stroke = line;
    v.window_corner_radius = radius;
    v.menu_corner_radius = radius;
    v.window_shadow = shadow(mode);
    v.popup_shadow = shadow(mode);
    v.selection.bg_fill = c(Token::PrimarySoft);
    v.selection.stroke = Stroke::new(BORDER_WIDTH, c(Token::Primary));

    let widget = |fill: Token, border: Stroke| egui::style::WidgetVisuals {
        bg_fill: c(fill),
        weak_bg_fill: c(fill),
        bg_stroke: border,
        corner_radius: radius,
        fg_stroke: ink,
        expansion: 0.0,
    };
    v.widgets.noninteractive = widget(Token::Surface, line);
    v.widgets.inactive = widget(Token::Surface, line);
    v.widgets.hovered = widget(Token::Canvas, line);
    v.widgets.active = widget(
        Token::PrimarySoft,
        Stroke::new(BORDER_WIDTH, c(Token::Focus)),
    );
    v.widgets.open = widget(Token::PrimarySoft, line);
    v
}

/// One of the four embedded IBM Plex files, each its own egui font family so
/// a style picks its weight by family (egui has no weight axis).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FontFace {
    SansRegular,
    SansMedium,
    SansSemiBold,
    MonoRegular,
}

/// Key of the embedded Noto Emoji in [`FontDefinitions::font_data`].
pub const NOTO_EMOJI_FONT_NAME: &str = "NotoEmoji-Regular";

/// SHA-256 of `assets/fonts/NotoEmoji-Regular.ttf`: Noto Emoji 3.002 as
/// published in google/fonts (THIRD-PARTY-NOTICES.md records the source).
/// `tests/theme_tokens.rs` checks the file and the notice against it.
pub const NOTO_EMOJI_SHA256: &str =
    "de6c18832938afc99caf132b39d6a30a19bac7f2e812e28db2535b4608d27551";

static NOTO_EMOJI: &[u8] = include_bytes!("../../assets/fonts/NotoEmoji-Regular.ttf");

impl FontFace {
    pub const ALL: [Self; 4] = [
        Self::SansRegular,
        Self::SansMedium,
        Self::SansSemiBold,
        Self::MonoRegular,
    ];

    /// Key of this face in [`FontDefinitions::font_data`]; the file stem.
    #[must_use]
    pub const fn data_name(self) -> &'static str {
        match self {
            Self::SansRegular => "IBMPlexSans-Regular",
            Self::SansMedium => "IBMPlexSans-Medium",
            Self::SansSemiBold => "IBMPlexSans-SemiBold",
            Self::MonoRegular => "IBMPlexMono-Regular",
        }
    }

    /// Family and weight as the spec §11.7 style table writes them.
    #[must_use]
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::SansRegular => "IBM Plex Sans Regular",
            Self::SansMedium => "IBM Plex Sans Medium",
            Self::SansSemiBold => "IBM Plex Sans SemiBold",
            Self::MonoRegular => "IBM Plex Mono Regular",
        }
    }

    /// The egui family whose chain this face leads. Regular Sans and Mono take
    /// egui's built-in families, so stock widgets render in Plex too.
    #[must_use]
    pub fn family(self) -> FontFamily {
        match self {
            Self::SansRegular => FontFamily::Proportional,
            Self::MonoRegular => FontFamily::Monospace,
            Self::SansMedium | Self::SansSemiBold => FontFamily::Name(self.data_name().into()),
        }
    }

    /// The embedded, unmodified font file.
    #[must_use]
    pub const fn bytes(self) -> &'static [u8] {
        match self {
            Self::SansRegular => include_bytes!("../../assets/fonts/IBMPlexSans-Regular.ttf"),
            Self::SansMedium => include_bytes!("../../assets/fonts/IBMPlexSans-Medium.ttf"),
            Self::SansSemiBold => include_bytes!("../../assets/fonts/IBMPlexSans-SemiBold.ttf"),
            Self::MonoRegular => include_bytes!("../../assets/fonts/IBMPlexMono-Regular.ttf"),
        }
    }

    /// The embedded, unmodified Noto Emoji file.
    #[must_use]
    pub const fn noto_emoji_bytes() -> &'static [u8] {
        NOTO_EMOJI
    }
}

/// egui font definitions: in every family, its Plex face, then Noto Emoji,
/// then `system_fallbacks` in the order given.
///
/// `system_fallbacks` are (name, data) pairs the platform adapter found for
/// scripts Plex lacks (spec §11.7 Fonts, plan T21). A fallback whose name
/// equals an embedded font's is skipped with a warning: replacing the data
/// under that key would silently swap Plex or Noto Emoji for a system file.
///
/// Built from [`FontDefinitions::empty`], never `default()`, so nothing of
/// egui's own bundled fonts can enter even if a dependency re-enabled them.
#[must_use]
pub fn font_definitions(system_fallbacks: Vec<(String, FontData)>) -> FontDefinitions {
    let mut font_data: BTreeMap<String, Arc<FontData>> = FontFace::ALL
        .iter()
        .map(|face| {
            (
                face.data_name().to_owned(),
                Arc::new(FontData::from_static(face.bytes())),
            )
        })
        .collect();
    font_data.insert(
        NOTO_EMOJI_FONT_NAME.to_owned(),
        Arc::new(FontData::from_static(NOTO_EMOJI)),
    );

    let mut fallback_names = Vec::with_capacity(system_fallbacks.len());
    for (name, data) in system_fallbacks {
        if font_data.contains_key(&name) {
            tracing::warn!(font = %name, "system fallback font shadows an embedded font; skipped");
            continue;
        }
        font_data.insert(name.clone(), Arc::new(data));
        fallback_names.push(name);
    }

    let families = FontFace::ALL
        .iter()
        .map(|face| {
            let chain = [face.data_name(), NOTO_EMOJI_FONT_NAME]
                .into_iter()
                .map(str::to_owned)
                .chain(fallback_names.iter().cloned())
                .collect();
            (face.family(), chain)
        })
        .collect();

    FontDefinitions {
        font_data,
        families,
    }
}

/// The eight `DD/Compact/*` text styles of spec §11.7.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CompactStyle {
    Title,
    Body,
    ControlLabel,
    Table,
    GroupEmphasis,
    Supporting,
    Document,
    PathDetail,
}

impl CompactStyle {
    pub const ALL: [Self; 8] = [
        Self::Title,
        Self::Body,
        Self::ControlLabel,
        Self::Table,
        Self::GroupEmphasis,
        Self::Supporting,
        Self::Document,
        Self::PathDetail,
    ];

    /// The style's name in the spec table, also its egui [`TextStyle::Name`].
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Title => "Title",
            Self::Body => "Body",
            Self::ControlLabel => "Control label",
            Self::Table => "Table",
            Self::GroupEmphasis => "Group/header emphasis",
            Self::Supporting => "Supporting text",
            Self::Document => "Document",
            Self::PathDetail => "Path/gutter detail",
        }
    }

    #[must_use]
    pub const fn face(self) -> FontFace {
        match self {
            Self::Title | Self::GroupEmphasis => FontFace::SansSemiBold,
            Self::Body | Self::Table | Self::Supporting => FontFace::SansRegular,
            Self::ControlLabel => FontFace::SansMedium,
            Self::Document | Self::PathDetail => FontFace::MonoRegular,
        }
    }

    /// Font size in units, before zoom. None is below 12 (spec §11.7).
    #[must_use]
    pub const fn size(self) -> f32 {
        match self {
            Self::Title => 16.0,
            Self::Body | Self::Document => 14.0,
            Self::ControlLabel | Self::Table | Self::GroupEmphasis => 13.0,
            Self::Supporting | Self::PathDetail => 12.0,
        }
    }

    /// Line height in units, before zoom. egui's [`FontId`] has no line
    /// height, so text laid out in this style sets it on its `TextFormat`.
    #[must_use]
    pub const fn line_height(self) -> f32 {
        match self {
            Self::Title | Self::Document => 22.0,
            Self::Body => 20.0,
            Self::ControlLabel | Self::Table | Self::GroupEmphasis | Self::Supporting => 18.0,
            Self::PathDetail => 16.0,
        }
    }

    #[must_use]
    pub fn font_id(self) -> FontId {
        FontId::new(self.size(), self.face().family())
    }

    #[must_use]
    pub fn text_style(self) -> TextStyle {
        TextStyle::Name(self.label().into())
    }
}

/// Puts the tokens and compact styles into an egui style for one mode.
///
/// egui's five built-in text styles are mapped onto compact styles too, so a
/// stock widget never falls back to egui's own sizes, some of which are
/// below the 12-unit floor.
pub fn apply_style(style: &mut egui::Style, mode: Mode) {
    style.visuals = visuals(mode);
    let mut text_styles: BTreeMap<TextStyle, FontId> = CompactStyle::ALL
        .iter()
        .map(|s| (s.text_style(), s.font_id()))
        .collect();
    for (builtin, compact) in [
        (TextStyle::Heading, CompactStyle::Title),
        (TextStyle::Body, CompactStyle::Body),
        (TextStyle::Button, CompactStyle::ControlLabel),
        (TextStyle::Small, CompactStyle::Supporting),
        (TextStyle::Monospace, CompactStyle::Document),
    ] {
        text_styles.insert(builtin, compact.font_id());
    }
    style.text_styles = text_styles;
}

/// In-app zoom: 80% to 200% in 10-point steps (spec §11.10, plan A-004).
///
/// Only on-grid values are representable, so a remembered preference that is
/// off the grid is rejected by [`Zoom::from_percent`] rather than carried
/// into the UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Zoom(u16);

impl Zoom {
    pub const MIN_PERCENT: u16 = 80;
    pub const MAX_PERCENT: u16 = 200;
    pub const STEP_PERCENT: u16 = 10;
    pub const DEFAULT_PERCENT: u16 = 100;

    /// The zoom at `percent`, if it is one of the steps.
    #[must_use]
    pub const fn from_percent(percent: u16) -> Option<Self> {
        if percent >= Self::MIN_PERCENT
            && percent <= Self::MAX_PERCENT
            && (percent - Self::MIN_PERCENT).is_multiple_of(Self::STEP_PERCENT)
        {
            Some(Self(percent))
        } else {
            None
        }
    }

    /// Every step, smallest first.
    pub fn all() -> impl Iterator<Item = Self> {
        (Self::MIN_PERCENT..=Self::MAX_PERCENT)
            .step_by(usize::from(Self::STEP_PERCENT))
            .map(Self)
    }

    #[must_use]
    pub const fn percent(self) -> u16 {
        self.0
    }

    /// The egui zoom factor, 1.0 at 100%.
    #[must_use]
    pub fn factor(self) -> f32 {
        f32::from(self.0) / 100.0
    }

    /// One step larger, saturating at 200%.
    #[must_use]
    pub const fn zoom_in(self) -> Self {
        if self.0 >= Self::MAX_PERCENT {
            self
        } else {
            Self(self.0 + Self::STEP_PERCENT)
        }
    }

    /// One step smaller, saturating at 80%.
    #[must_use]
    pub const fn zoom_out(self) -> Self {
        if self.0 <= Self::MIN_PERCENT {
            self
        } else {
            Self(self.0 - Self::STEP_PERCENT)
        }
    }

    #[must_use]
    pub const fn reset(self) -> Self {
        Self(Self::DEFAULT_PERCENT)
    }
}

impl Default for Zoom {
    fn default() -> Self {
        Self(Self::DEFAULT_PERCENT)
    }
}

/// Generates [`Icon`] from one table of (variant, Lucide file stem), so the
/// embedded bytes, the loader URI and the inventory can never disagree.
macro_rules! icons {
    ($($(#[$doc:meta])* $variant:ident => $file:literal,)+) => {
        /// Every Lucide icon the UI draws (spec §11.7 Icons). Each variant
        /// embeds `assets/icons/<lucide_name>.svg`, unmodified from the
        /// pinned release; `tests/theme_tokens.rs` fails if a file in that
        /// directory has no variant here, or a variant has no file.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Icon {
            $($(#[$doc])* $variant,)+
        }

        impl Icon {
            pub const ALL: &'static [Self] = &[$(Self::$variant,)+];

            /// The Lucide icon name, which is also the file stem.
            #[must_use]
            pub const fn lucide_name(self) -> &'static str {
                match self {
                    $(Self::$variant => $file,)+
                }
            }

            /// The embedded SVG.
            #[must_use]
            pub const fn bytes(self) -> &'static [u8] {
                match self {
                    $(Self::$variant => include_bytes!(concat!("../../assets/icons/", $file, ".svg")),)+
                }
            }

            /// The egui image URI. The `.svg` suffix is what routes the bytes
            /// to `egui_extras`' SVG loader, which [`install`] registers.
            #[must_use]
            pub const fn uri(self) -> &'static str {
                match self {
                    $(Self::$variant => concat!("bytes://fuzzy-dedupe/icons/", $file, ".svg"),)+
                }
            }
        }
    };
}

// Lucide draws with `stroke="currentColor"`, which resvg rasterizes black.
// egui's image tint multiplies, so tinting [`Icon::image`] cannot turn black
// into a token colour; a view that needs a coloured icon draws the bytes as an
// `egui_extras::SvgGlyph`, which tints a grey-only SVG with the text colour.
icons! {
    /// Findings entry in the navigation rail.
    Findings => "table-2",
    /// Compare entry in the navigation rail.
    Compare => "columns-2",
    /// Main menu entry at the foot of the rail.
    Menu => "menu",
    /// Toolbar button that opens Matching rules.
    Settings => "settings",
    /// Trailing icon of the Findings search field.
    Search => "search",
    /// Expanded group header, and an expanded popup section.
    Expanded => "chevron-down",
    /// Collapsed group header, and a collapsed popup section.
    Collapsed => "chevron-right",
    /// The group's reference file.
    Reference => "anchor",
    /// A file row, and the file identity above each Compare pane.
    Document => "file-text",
    /// A protected file.
    Protected => "lock",
    /// Per-row menu at the end of a Findings row.
    RowMenu => "ellipsis",
    /// Scan problems and a missing acknowledgment.
    Alert => "triangle-exclamation-point",
    /// Checked queue box, and the selected item of a menu.
    Check => "check",
}

impl Icon {
    /// The icon as an egui image, loaded through the SVG loader. It renders
    /// black whatever the tint; see the note above the icon table.
    pub fn image(self) -> egui::Image<'static> {
        egui::Image::from_bytes(self.uri(), self.bytes())
    }
}

/// Installs fonts, both modes' styles and the SVG loader on `ctx`.
///
/// Also turns off egui's own keyboard zoom: its steps and bounds are not the
/// A-004 grid, so Ctrl+= / Ctrl+- / Ctrl+0 must go through [`Zoom`] and
/// [`apply_zoom`] instead (spec §11.10).
pub fn install(ctx: &egui::Context, system_fallbacks: Vec<(String, FontData)>) {
    ctx.set_fonts(font_definitions(system_fallbacks));
    for mode in Mode::ALL {
        let mut style = egui::Style::default();
        apply_style(&mut style, mode);
        ctx.set_style_of(mode.egui_theme(), style);
    }
    ctx.options_mut(|options| options.zoom_with_keyboard = false);
    egui_extras::install_image_loaders(ctx);
}

/// Applies `zoom` to the whole UI.
pub fn apply_zoom(ctx: &egui::Context, zoom: Zoom) {
    ctx.set_zoom_factor(zoom.factor());
}
