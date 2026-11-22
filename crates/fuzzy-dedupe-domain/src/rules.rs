// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! Matching rules (spec §6, §11.6): what counts as a match. Unicode NFC
//! always applies and is deliberately not an option here.

use crate::ids::RulesRevision;
use crate::results::Permille;

/// Match mode, the first control of the Matching rules popup.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MatchMode {
    /// Byte-identical files only.
    ExactOnly,
    /// Exact matches plus fuzzy text matches.
    ExactAndFuzzy,
}

/// The four Ignore options of spec §11.6. Their defaults stay open until the
/// MS-3 benchmarks (OQ-005, OQ-006), so this type has no `Default`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "four independent user toggles, one per spec §11.6 checkbox; a bit set would only \
              hide the same four booleans from the popup's draft binding"
)]
pub struct IgnoreOptions {
    pub whitespace: bool,
    pub line_endings: bool,
    pub letter_case: bool,
    pub punctuation: bool,
}

/// One saved set of matching rules. A scan keeps an immutable copy of the
/// rules it used, so changing the saved rules never rewrites old evidence.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MatchingRules {
    pub mode: MatchMode,
    /// Lowest similarity a fuzzy match may report. Fixture values are
    /// illustrative, not algorithm defaults (OQ-005).
    pub minimum_similarity: Permille,
    pub ignore: IgnoreOptions,
    pub revision: RulesRevision,
}

/// Compare's view mode, part of a comparison's cache key (plan C-04).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CompareMode {
    /// Original text; unique-line counts compare raw line text.
    Original,
    /// NFC plus the scan's enabled Ignore options on both sides; counts are
    /// labelled as normalized.
    Normalized,
}
