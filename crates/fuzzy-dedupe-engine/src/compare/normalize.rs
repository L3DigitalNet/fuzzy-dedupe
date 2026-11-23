// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! The Normalized view's line profile (spec §10.1, §11.5 rule 8; plan T6).
//!
//! A normalized line is the original line text, without its terminator,
//! after these steps in this order:
//!
//! 1. Unicode NFC, always (spec §6: NFC is not an option).
//! 2. Ignore punctuation: every [`is_punctuation`] character is removed.
//! 3. Ignore letter case: Unicode lowercase (`str::to_lowercase`).
//! 4. Ignore whitespace differences: leading and trailing whitespace is
//!    removed and every inner run of whitespace becomes one space.
//!
//! Ignore line-ending differences never changes line text, because line
//! text excludes its terminator in every mode (§10.1). It decides only the
//! terminator of the normalized pane text: LF when set, the original
//! terminator otherwise.
//!
//! The order is load-bearing. Punctuation goes before whitespace so that
//! "a , b" becomes "a b" rather than "a  b", and NFC goes first so that
//! canonically equivalent input (precomposed or decomposed accents) is
//! indistinguishable after every later step.
//!
//! This is the MS-1 profile only. The final tokenization and normalization
//! profiles stay open until the MS-3 benchmarks (spec OQ-006); whoever
//! replaces this keeps one profile for counts, rows and copy text.

use fuzzy_dedupe_domain::IgnoreOptions;
use unicode_normalization::UnicodeNormalization;

/// `line` (no terminator) under the profile in the module header.
#[must_use]
pub fn normalize_line(line: &str, ignore: IgnoreOptions) -> String {
    let mut text: String = line.nfc().collect();
    if ignore.punctuation {
        text.retain(|c| !is_punctuation(c));
    }
    if ignore.letter_case {
        text = text.to_lowercase();
    }
    if ignore.whitespace {
        text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    }
    text
}

/// What Ignore punctuation removes: ASCII punctuation, the Latin-1
/// punctuation marks, General Punctuation (U+2010 to U+2027 and U+2030 to
/// U+205E, so not the spaces and format controls of that block), CJK
/// Symbols and Punctuation (U+3001 to U+3003 and U+3008 to U+301F) and the
/// fullwidth forms of ASCII punctuation.
///
/// Rejected: the full Unicode punctuation category, which needs a character
/// database crate that spec §8.3 does not approve; these ranges cover the
/// scripts the fixtures use.
#[must_use]
pub fn is_punctuation(c: char) -> bool {
    c.is_ascii_punctuation()
        || matches!(
            c,
            '\u{A1}' | '\u{A7}' | '\u{AB}' | '\u{B6}' | '\u{B7}' | '\u{BB}' | '\u{BF}'
            | '\u{2010}'..='\u{2027}'
            | '\u{2030}'..='\u{205E}'
            | '\u{3001}'..='\u{3003}'
            | '\u{3008}'..='\u{301F}'
            | '\u{FF01}'..='\u{FF0F}'
            | '\u{FF1A}'..='\u{FF20}'
            | '\u{FF3B}'..='\u{FF40}'
            | '\u{FF5B}'..='\u{FF65}'
        )
}
