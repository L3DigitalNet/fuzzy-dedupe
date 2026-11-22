// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! Textual layer of the no-file-mutation guard (MS-1 plan D-006, Appendix A
//! C-09; spec §19 MS-1 exit item 8: no code that moves, renames or deletes
//! the user's files, and no Trash dependency).
//!
//! Threat model: the guard is a regression barrier against the accidental
//! or casual introduction of user-file mutation, not a sandbox against a
//! deliberately hostile developer. Where exact resolution would be costly it
//! rejects the construct outright (fail closed) instead of resolving it.
//!
//! The Clippy layer, the workspace `clippy.toml` run by the `clippy` job, is
//! authoritative for the code it compiles because it resolves paths. This
//! test is the bounded second layer, and also the only check that keeps
//! that layer intact. Over the workspace it fails when:
//!
//! - source under `crates/*/src/` outside the allowlist, including any
//!   `#[cfg(test)]` code, reaches a disallowed route through a qualified
//!   path, a `use` tree (an imported listed item, any `as` alias of a path
//!   through `fs` or `process`, an alias or glob of the `std`, `core` or
//!   `alloc` root, of `std::os` or of `std::os::<platform>`, a glob of an
//!   `fs` or `process` module, any re-export), `extern crate std as`, a
//!   method form (`.rename(`, `.set_permissions(`, `.set_len(`,
//!   `.set_times(`, `.set_modified(`), `Command::new`, or a `type` alias of a
//!   path through `fs` or `process`;
//! - scoped source builds a path into a guarded module inside a macro
//!   (`fs::$x`, `process::$x`, `std::$x`), which closes macro-built forms
//!   under `cfg`s host Clippy never compiles;
//! - an allowlisted file defines a `macro_rules!` macro, or invokes one
//!   defined anywhere in the workspace;
//! - scoped source declares an FFI block (`extern "…" {`) or a `#[link]`
//!   attribute, uses `include!` or any `#[path]`, declares an unresolvable
//!   out-of-line module, or, under src/, a `cfg_attr` that conditionally
//!   sets `cfg` or `path`;
//! - a file under `crates/*/tests/` names a delete, rename, link,
//!   permission, length, timestamp or process item, uses a filesystem copy
//!   form (`fs::copy`, `copy` imported from an `fs` module), or carries a
//!   `disallowed_methods` `expect` whose item body never names
//!   `CARGO_TARGET_TMPDIR`;
//! - any `allow` form of either lint exists, or an `expect` of either lint
//!   is not an item-level attribute with a `reason` at its allowlisted
//!   location, and, under src/, outside every region whose `cfg` requires
//!   `test` (outer or inner attribute, or an out-of-line test module's file);
//! - `clippy.toml` lacks a C-09 minimum entry or one of the guard's added
//!   entries, is not the only Clippy configuration file, or spells any
//!   string with an escape or as a multi-line string;
//! - a manifest lowers either lint, or a group containing them, in any TOML
//!   spelling, or uses an escape or multi-line string in a lint table;
//! - a manifest or `Cargo.lock` names a package containing `trash`;
//! - a `REUSE.toml` annotation glob matches `clippy.toml`;
//! - a scoped tree (`crates/*/src`, `crates/*/tests`) contains a symbolic
//!   link, a scope boundary (a crate directory, its `Cargo.toml`, `src/` or
//!   `tests/`, or the root `Cargo.toml`) is one, or the crate directories
//!   differ from the root `[workspace] members`.
//!
//! Lexing, not parsing: comments and string, byte-string, raw-string and char
//! literals are dropped and the rest becomes identifier and punctuation
//! tokens, so a mention in prose never trips the scan and a call split over
//! several lines still does. Rejected: a regex or line-based grep, which
//! cannot tell a string from code and misses multiline forms; and a `syn`
//! parse, which would be a new direct dependency, and every direct
//! dependency needs an owner-approved spec §8.3 row, which `syn` lacks (plan
//! §3.4). The guard therefore uses only std, including a small TOML reader.
//!
//! The textual scan reads every `cfg` branch; the Clippy job compiles only
//! the Linux host configuration. Declared residue, one line each:
//!
//! - Clippy lints only code compiled for the Linux host; inactive-`cfg` code
//!   has only this scan, whose macro-built-path rule covers that form.
//! - Running Clippy for other targets needs a `rust.yml` job change, which
//!   is outside T3's claim.
//! - Items re-exported under a new name through another workspace crate are
//!   seen only by host Clippy.
//! - A macro outside the allowlisted files that takes a module path as a
//!   fragment (`$m::remove_file` with `$m` = `std::fs`) is seen only by host
//!   Clippy.
//! - `type` aliases of an already imported name (`use std::fs::File;` then
//!   `type H = File;`) outside the allowlisted files are seen only by host
//!   Clippy.
//! - `CARGO_TARGET_TMPDIR` proves a test item names its fixture directory,
//!   not that every write lands inside it; destination tracking is out of
//!   scope.
//! - `build.rs`, `examples/` and `benches/` are outside C-09's named scope.
//! - A crate-local module named `fs` or `process` is treated as the std one,
//!   a deliberate false positive.
//!
//! Requirements: run from inside the workspace checkout (the workspace root
//! is `CARGO_MANIFEST_DIR/../..`), with `clippy.toml`, `Cargo.toml`,
//! `Cargo.lock` and `REUSE.toml` present at that root. The scoped trees are
//! walked completely with no skips. The repository walk for configuration
//! and manifests skips tool caches only at the root, and any directory
//! outside `crates/` that holds its own `.git`, so a Git worktree checked
//! out below the root (as `.claude/worktrees/` holds) is never mistaken for
//! workspace files.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Debug)]
struct Finding {
    path: String,
    line: usize,
    rule: &'static str,
    detail: String,
}

impl std::fmt::Display for Finding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}:{}: [{}] {}",
            self.path, self.line, self.rule, self.detail
        )
    }
}

fn finding(path: &str, line: usize, rule: &'static str, detail: impl Into<String>) -> Finding {
    Finding {
        path: path.to_owned(),
        line,
        rule,
        detail: detail.into(),
    }
}

const RULE_QUALIFIED: &str = "qualified-path";
const RULE_USE_TREE: &str = "use-tree";
const RULE_METHOD: &str = "method-form";
const RULE_TEST_NAME: &str = "test-file-name";
const RULE_ALLOW: &str = "allow-form";
const RULE_EXPECT_LOCATION: &str = "expect-location";
const RULE_EXPECT_REASON: &str = "expect-reason";
const RULE_CLIPPY_ENTRY: &str = "clippy-toml-entry";
const RULE_CLIPPY_FILE: &str = "clippy-config-file";
const RULE_MANIFEST_LINT: &str = "manifest-lowers-lint";
const RULE_TRASH: &str = "trash-package";
const RULE_REUSE: &str = "reuse-annotation";
const RULE_MACRO: &str = "macro-in-allowlisted-file";
const RULE_FFI: &str = "ffi";
const RULE_TEST_TMPDIR: &str = "test-fixture-dir";
const RULE_SYMLINK: &str = "symlink-in-scope";
const RULE_MODULE: &str = "module-tree";
const RULE_TOML: &str = "toml-parse";
const RULE_TOML_ESCAPE: &str = "toml-unsupported-string";
const RULE_MEMBERS: &str = "workspace-members";
const RULE_CFG_ATTR: &str = "cfg-attr";
const RULE_MACRO_PATH: &str = "macro-built-path";
const RULE_TYPE_ALIAS: &str = "type-alias";

// ── C-09 rule data ─────────────────────────────────────────────────────────

// Mirror of the C-09 minimum lists, which `clippy.toml` must contain. Keep in
// step with C-09: a later plan may add entries to `clippy.toml` without
// touching these, but removing one here would hide a removal there.
const MIN_DISALLOWED_METHODS: &[&str] = &[
    "std::fs::remove_file",
    "std::fs::remove_dir",
    "std::fs::remove_dir_all",
    "std::fs::rename",
    "std::fs::hard_link",
    "std::fs::soft_link",
    "std::fs::set_permissions",
    "std::fs::write",
    "std::fs::copy",
    "std::fs::create_dir",
    "std::fs::create_dir_all",
    "std::fs::File::create",
    "std::fs::File::create_new",
    "std::fs::OpenOptions::open",
];
const MIN_DISALLOWED_TYPES: &[&str] = &["std::process::Command"];

// Entries the guard adds beyond the C-09 minimum (T3 executor discretion;
// C-09 allows additions, never removals): further routes that create links,
// directories or files, or change a file's permissions, length or
// timestamps. Required in `clippy.toml` like the minimum. The Windows
// `symlink_file` and `symlink_dir` are absent: they do not resolve on the
// Linux host, Clippy warns about an unresolvable entry and `-D warnings`
// would fail, so they are textual-only (`FS_IMPORTABLE`).
const EXTRA_DISALLOWED_METHODS: &[&str] = &[
    "std::os::unix::fs::symlink",
    "std::fs::DirBuilder::create",
    "std::fs::File::set_permissions",
    "std::fs::File::set_len",
    "std::fs::File::set_times",
    "std::fs::File::set_modified",
    "std::fs::File::options",
];

// Items of any `fs` module (`std::fs`, `std::os::unix::fs`,
// `std::os::windows::fs`) whose import or `fs::` path is itself a finding:
// every free function in the minimum and added lists, the Windows symlink
// functions, `DirBuilder`, and `OpenOptions`, whose only use beyond
// `File::open` is writing or creating, and which Clippy rejects even for a
// read-only `open`. `File` is absent on purpose: `File::open` is how source
// is read.
const FS_IMPORTABLE: &[&str] = &[
    "remove_file",
    "remove_dir",
    "remove_dir_all",
    "rename",
    "hard_link",
    "soft_link",
    "set_permissions",
    "write",
    "copy",
    "create_dir",
    "create_dir_all",
    "OpenOptions",
    "DirBuilder",
    "symlink",
    "symlink_file",
    "symlink_dir",
];
const PROCESS_IMPORTABLE: &[&str] = &["Command"];

// `A::B` token pairs flagged outside `use` statements, with the C-09 name
// each one reaches. `fs::X` covers every `FS_IMPORTABLE` item separately.
const QUALIFIED_PAIRS: &[(&str, &str, &str)] = &[
    ("File", "create", "create"),
    ("File", "create_new", "create_new"),
    ("File", "options", "OpenOptions"),
    ("File", "set_permissions", "set_permissions"),
    ("File", "set_len", "set_len"),
    ("File", "set_times", "set_times"),
    ("File", "set_modified", "set_modified"),
    ("OpenOptions", "new", "OpenOptions"),
    ("OpenOptions", "open", "OpenOptions"),
    ("process", "Command", "Command"),
    ("Command", "new", "Command"),
];

// `.name(` method forms flagged outside `use` statements: `File` methods
// that alter a file in place, and `rename` on any handle.
const METHOD_FORMS: &[&str] = &[
    "rename",
    "set_permissions",
    "set_len",
    "set_times",
    "set_modified",
];

// Crate roots that reach `fs` or `process`; aliasing one hides every path
// below it from this scan (round-2 review finding 1).
const CRATE_ROOTS: &[&str] = &["std", "core", "alloc"];

// A raw identifier of a keyword (`r#use`) is an ordinary name, never the
// keyword, so the lexer keeps its `r#` prefix; any other raw identifier
// (`r#remove_file`) names the same item as the plain spelling and is
// normalized to it.
const RUST_KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern",
    "false", "fn", "for", "gen", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut",
    "pub", "ref", "return", "self", "Self", "static", "struct", "super", "trait", "true", "try",
    "type", "unsafe", "use", "where", "while", "abstract", "become", "box", "do", "final", "macro",
    "override", "priv", "typeof", "unsized", "virtual", "yield",
];

// Names that may not appear as identifiers in `crates/*/tests/**`: the
// delete, rename, link, permission, length, timestamp and process entries.
// Integration tests may build fixtures with the write and create entries
// only (C-09). `copy` is absent: as a bare name it also means `std::io::copy`
// or a local, so test files reject only its filesystem forms
// (`test_copy_findings`).
const TEST_FILE_BANNED: &[&str] = &[
    "symlink",
    "symlink_file",
    "symlink_dir",
    "set_len",
    "set_times",
    "set_modified",
    "remove_file",
    "remove_dir",
    "remove_dir_all",
    "rename",
    "hard_link",
    "soft_link",
    "set_permissions",
    "Command",
];

const LINT_METHODS: &str = "disallowed_methods";
const LINT_TYPES: &str = "disallowed_types";
// Clippy groups that contain both lints, and rustc's `warnings` group:
// allowing or expecting any of these would lower the lints as surely as
// naming them.
const LOWERING_GROUPS: &[&str] = &["all", "style", "warnings"];

/// The C-09 allowlist: each `src/` file that may use some entries, the lint
/// its `expect` names, and the C-09 names it may reach.
struct Allowed {
    path: &'static str,
    lint: &'static str,
    names: &'static [&'static str],
}

const ALLOWLIST: &[Allowed] = &[
    Allowed {
        path: "crates/fuzzy-dedupe-platform/src/app_files.rs",
        lint: LINT_METHODS,
        names: &[
            "write",
            "create_dir",
            "create_dir_all",
            "create",
            "create_new",
            "OpenOptions",
            "rename",
        ],
    },
    Allowed {
        path: "crates/fuzzy-dedupe-platform/src/log_retention.rs",
        lint: LINT_METHODS,
        names: &["remove_file"],
    },
    Allowed {
        path: "crates/fuzzy-dedupe-platform/src/open.rs",
        lint: LINT_TYPES,
        names: &["Command"],
    },
];

// ── Lexer ──────────────────────────────────────────────────────────────────

#[derive(Debug)]
struct Token {
    text: String,
    line: usize,
    ident: bool,
}

/// Lexer output: the tokens, plus the raw text of every string-like
/// literal, each with the number of tokens before it, which only the
/// `CARGO_TARGET_TMPDIR` proxy reads.
struct Lexed {
    tokens: Vec<Token>,
    strings: Vec<(usize, String)>,
}

/// Tokenize Rust source into identifiers and punctuation, dropping comments,
/// literals, lifetimes and numbers.
///
/// Comments nest (`/* /* */ */`), as Rust's do. A raw identifier `r#name`
/// yields `name`, except a raw keyword, which keeps its prefix
/// (`RUST_KEYWORDS`). `::` is a single token; every other punctuation character
/// is its own token. Unterminated literals or comments run to end of input
/// rather than panic, so a malformed file degrades to fewer tokens; the
/// compiler rejects such a file anyway.
fn lex(src: &str) -> Lexed {
    let c: Vec<char> = src.chars().collect();
    let at = |i: usize| c.get(i).copied().unwrap_or('\0');
    let mut out = Vec::new();
    let mut strings = Vec::new();
    let mut line = 1;
    let mut i = 0;
    while i < c.len() {
        let ch = c[i];
        let token = move |text: String, ident: bool| Token { text, line, ident };
        if let Some(end) = skipped_end(&c, i) {
            // Comments and literals vanish, but their newlines still count.
            let end = end.min(c.len());
            if ch != '/' {
                strings.push((out.len(), c[i..end].iter().collect()));
            }
            line += c[i..end].iter().filter(|&&x| x == '\n').count();
            i = end;
        } else if ch == '\'' {
            // A lifetime or label: drop the quote and its identifier.
            i += 1;
            while i < c.len() && is_ident_char(c[i]) {
                i += 1;
            }
        } else if is_ident_start(ch) {
            let raw = ch == 'r' && at(i + 1) == '#';
            let start = if raw { i + 2 } else { i };
            let mut j = start;
            while j < c.len() && is_ident_char(c[j]) {
                j += 1;
            }
            let name: String = c[start..j].iter().collect();
            let text = if raw && RUST_KEYWORDS.contains(&name.as_str()) {
                format!("r#{name}")
            } else {
                name
            };
            out.push(token(text, true));
            i = j;
        } else if ch.is_ascii_digit() {
            while i < c.len() && is_ident_char(c[i]) {
                i += 1;
            }
        } else if ch == ':' && at(i + 1) == ':' {
            out.push(token("::".to_owned(), false));
            i += 2;
        } else {
            if ch == '\n' {
                line += 1;
            }
            if !ch.is_whitespace() {
                out.push(token(ch.to_string(), false));
            }
            i += 1;
        }
    }
    Lexed {
        tokens: out,
        strings,
    }
}

fn is_ident_start(ch: char) -> bool {
    ch == '_' || ch.is_alphabetic()
}

fn is_ident_char(ch: char) -> bool {
    ch == '_' || ch.is_alphanumeric()
}

/// Return the index just past a comment, string, byte-string, C-string or
/// char literal starting at `i`, or `None` if none starts there. A `'` that
/// opens a lifetime returns `None`.
fn skipped_end(c: &[char], i: usize) -> Option<usize> {
    let at = |k: usize| c.get(k).copied().unwrap_or('\0');
    let ch = c[i];
    if ch == '/' && at(i + 1) == '/' {
        return Some((i..c.len()).find(|&j| c[j] == '\n').unwrap_or(c.len()));
    }
    if ch == '/' && at(i + 1) == '*' {
        let mut depth = 0usize;
        let mut j = i;
        while j < c.len() {
            if c[j] == '/' && at(j + 1) == '*' {
                depth += 1;
                j += 2;
            } else if c[j] == '*' && at(j + 1) == '/' {
                depth -= 1;
                j += 2;
                if depth == 0 {
                    break;
                }
            } else {
                j += 1;
            }
        }
        return Some(j);
    }
    if let Some(end) = raw_string_end(c, i) {
        return Some(end);
    }
    let prefixed = (ch == 'b' || ch == 'c') && matches!(at(i + 1), '"' | '\'');
    let q = if prefixed { i + 1 } else { i };
    match at(q) {
        '"' if ch == '"' || prefixed => {
            let mut j = q + 1;
            while j < c.len() && c[j] != '"' {
                j += if c[j] == '\\' { 2 } else { 1 };
            }
            Some(j + 1)
        }
        '\'' if at(q + 1) == '\\' => {
            // Escaped char literal ('\n', '\'', '\u{1F600}'): the escaped
            // character itself may be a quote, so the search starts after it.
            let mut j = q + 3;
            while j < c.len() && c[j] != '\'' {
                j += 1;
            }
            Some(j + 1)
        }
        '\'' if at(q + 2) == '\'' => Some(q + 3),
        _ => None,
    }
}

/// Return the index just past a raw string literal (`r"…"`, `r#"…"#`,
/// `br"…"`, `cr#"…"#`) starting at `i`, or `None` if none starts there.
fn raw_string_end(c: &[char], i: usize) -> Option<usize> {
    let mut j = i;
    if matches!(c.get(j), Some('b' | 'c')) {
        j += 1;
    }
    if c.get(j) != Some(&'r') {
        return None;
    }
    j += 1;
    let hashes = c[j..].iter().take_while(|&&ch| ch == '#').count();
    j += hashes;
    if c.get(j) != Some(&'"') {
        return None;
    }
    j += 1;
    while j < c.len() {
        if c[j] == '"'
            && c[j + 1..]
                .iter()
                .take(hashes)
                .filter(|&&ch| ch == '#')
                .count()
                == hashes
        {
            return Some(j + 1 + hashes);
        }
        j += 1;
    }
    Some(c.len())
}

/// Return the index of the token closing the group opened at `open`.
fn matching_close(t: &[Token], open: usize) -> usize {
    let (o, cl) = match t[open].text.as_str() {
        "(" => ("(", ")"),
        "[" => ("[", "]"),
        _ => ("{", "}"),
    };
    let mut depth = 0usize;
    for (k, tok) in t.iter().enumerate().skip(open) {
        if tok.text == o {
            depth += 1;
        } else if tok.text == cl {
            depth -= 1;
            if depth == 0 {
                return k;
            }
        }
    }
    t.len() - 1
}

// ── Rust source scan ───────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
enum FileKind {
    Src,
    Test,
}

fn crate_relative(path: &str) -> Option<(&str, &str)> {
    let rest = path.strip_prefix("crates/")?;
    let (_crate, rest) = rest.split_once('/')?;
    rest.split_once('/')
}

fn file_kind(path: &str) -> Option<FileKind> {
    match crate_relative(path)? {
        ("src", _) => Some(FileKind::Src),
        ("tests", _) => Some(FileKind::Test),
        _ => None,
    }
}

fn is_scanned_rust(path: &str) -> bool {
    Path::new(path).extension().is_some_and(|e| e == "rs") && file_kind(path).is_some()
}

/// An attribute `#[…]` or `#![…]`, as token indices.
struct Attr {
    start: usize,
    end: usize,
    inner: bool,
}

impl Attr {
    /// The attribute's leading identifier: `cfg` in `#[cfg(test)]`.
    fn head<'t>(&self, t: &'t [Token]) -> Option<&'t str> {
        let open = if self.inner {
            self.start + 2
        } else {
            self.start + 1
        };
        t.get(open + 1).filter(|x| x.ident).map(|x| x.text.as_str())
    }

    /// `#[cfg(P)]` where `P` cannot hold without `test`: `test`,
    /// `all(…, test, …)`, or `any(…)` whose every branch requires `test`.
    /// `not(test)` and `any(test, unix)` do not, so code under them is
    /// ordinary code (round-3 item N7).
    fn is_cfg_test(&self, t: &[Token]) -> bool {
        let open = if self.inner {
            self.start + 2
        } else {
            self.start + 1
        };
        self.head(t) == Some("cfg")
            && t.get(open + 2).is_some_and(|x| x.text == "(")
            && cfg_requires_test(t, open + 3).0
    }
}

/// Evaluate the cfg predicate starting at token `k`: whether it requires
/// `test`, and the index just past it. An unknown predicate (`unix`,
/// `feature = "x"`) does not require `test`.
fn cfg_requires_test(t: &[Token], k: usize) -> (bool, usize) {
    let Some(tok) = t.get(k) else {
        return (false, k);
    };
    if t.get(k + 1).is_some_and(|x| x.text == "(") {
        let close = matching_close(t, k + 1);
        let mut parts = Vec::new();
        let mut j = k + 2;
        while j < close {
            if t[j].text == "," {
                j += 1;
                continue;
            }
            let (req, next) = cfg_requires_test(t, j);
            parts.push(req);
            j = next.max(j + 1);
        }
        let req = match tok.text.as_str() {
            "all" => parts.iter().any(|r| *r),
            "any" => !parts.is_empty() && parts.iter().all(|r| *r),
            _ => false,
        };
        return (req, close + 1);
    }
    // `key = "value"` arrives as `key =` (the lexer drops the string).
    let next = if t.get(k + 1).is_some_and(|x| x.text == "=") {
        k + 2
    } else {
        k + 1
    };
    (tok.ident && tok.text == "test", next)
}

fn attributes(t: &[Token]) -> Vec<Attr> {
    let mut out = Vec::new();
    let mut k = 0;
    while k < t.len() {
        if t[k].text == "#" {
            let inner = t.get(k + 1).is_some_and(|x| x.text == "!");
            let open = if inner { k + 2 } else { k + 1 };
            if t.get(open).is_some_and(|x| x.text == "[") {
                let end = matching_close(t, open);
                out.push(Attr {
                    start: k,
                    end,
                    inner,
                });
                k = end + 1;
                continue;
            }
        }
        k += 1;
    }
    out
}

/// The outer attributes directly above the item whose keyword is at `item`,
/// skipping a visibility, and the index of the first of them.
fn outer_attrs_before<'a>(t: &[Token], attrs: &'a [Attr], item: usize) -> (usize, Vec<&'a Attr>) {
    let mut first = item;
    if first > 0 && t[first - 1].text == ")" {
        if let Some(p) = (0..first - 1).rev().find(|&p| t[p].text == "(")
            && p > 0
            && t[p - 1].text == "pub"
        {
            first = p - 1;
        }
    } else if first > 0 && t[first - 1].text == "pub" {
        first -= 1;
    }
    let mut found = Vec::new();
    while let Some(a) = attrs.iter().find(|a| !a.inner && a.end + 1 == first) {
        found.push(a);
        first = a.start;
    }
    (first, found)
}

/// An inline module `mod name { … }`: its name, the index of its first outer
/// attribute (or keyword), and its brace indices.
struct InlineMod {
    name: String,
    first: usize,
    open: usize,
    close: usize,
    outer_test: bool,
}

fn inline_mods(t: &[Token], attrs: &[Attr]) -> Vec<InlineMod> {
    let mut out = Vec::new();
    for (m, tok) in t.iter().enumerate() {
        let is_block = tok.ident
            && tok.text == "mod"
            && t.get(m + 1).is_some_and(|x| x.ident)
            && t.get(m + 2).is_some_and(|x| x.text == "{");
        if is_block {
            let (first, outer) = outer_attrs_before(t, attrs, m);
            out.push(InlineMod {
                name: t[m + 1].text.clone(),
                first,
                open: m + 2,
                close: matching_close(t, m + 2),
                outer_test: outer.iter().any(|a| a.is_cfg_test(t)),
            });
        }
    }
    out
}

/// Token ranges that are test-only code: an inline module with an outer
/// `#[cfg(test)]` (from its first attribute, so an allowance on the module
/// itself counts as inside) or an inner `#![cfg(test)]`; a file-level
/// `#![cfg(test)]`; and the whole file when `file_is_test`, which the
/// workspace scan sets for the file of an out-of-line test module.
fn test_regions(
    t: &[Token],
    attrs: &[Attr],
    mods: &[InlineMod],
    file_is_test: bool,
) -> Vec<(usize, usize)> {
    let whole = (0, t.len().saturating_sub(1));
    let mut out: Vec<(usize, usize)> = mods
        .iter()
        .filter(|m| m.outer_test)
        .map(|m| (m.first, m.close))
        .collect();
    for a in attrs.iter().filter(|a| a.inner && a.is_cfg_test(t)) {
        let innermost = mods
            .iter()
            .filter(|m| m.open < a.start && a.start < m.close)
            .min_by_key(|m| m.close - m.open);
        out.push(innermost.map_or(whole, |m| (m.first, m.close)));
    }
    if file_is_test {
        out.push(whole);
    }
    out
}

fn in_regions(regions: &[(usize, usize)], k: usize) -> bool {
    regions.iter().any(|&(s, e)| k >= s && k <= e)
}

/// One lint reference inside an attribute, with its level.
struct LintRef {
    line: usize,
    level: String,
    lint: String,
    has_reason: bool,
    inner: bool,
    in_test_module: bool,
    attr_end: usize,
}

/// Every `clippy::disallowed_*` or lowering-group reference in an
/// attribute, attributed to its nearest enclosing lint-level call. Scanning
/// the whole attribute, not just its head, is what catches
/// `cfg_attr(test, allow(…))`.
fn lint_refs(t: &[Token], attrs: &[Attr], regions: &[(usize, usize)]) -> Vec<LintRef> {
    let levels = ["allow", "expect", "warn", "deny", "forbid"];
    let mut out = Vec::new();
    for a in attrs {
        let mut stack: Vec<(String, usize)> = Vec::new();
        for k in a.start..=a.end {
            let tok = &t[k];
            if tok.ident && t.get(k + 1).is_some_and(|x| x.text == "(") {
                stack.push((tok.text.clone(), matching_close(t, k + 1)));
            }
            while stack.last().is_some_and(|(_, close)| *close < k) {
                stack.pop();
            }
            let lint = if tok.ident
                && tok.text == "clippy"
                && t.get(k + 1).is_some_and(|x| x.text == "::")
            {
                t.get(k + 2).map(|x| x.text.clone())
            } else if tok.ident && tok.text == "warnings" && k > 0 && t[k - 1].text != "::" {
                Some(tok.text.clone())
            } else {
                None
            };
            let Some(lint) = lint else { continue };
            let watched = lint == LINT_METHODS
                || lint == LINT_TYPES
                || LOWERING_GROUPS.contains(&lint.as_str());
            let level = stack
                .iter()
                .rev()
                .find(|(name, _)| levels.contains(&name.as_str()));
            if let (true, Some((level, close))) = (watched, level) {
                out.push(LintRef {
                    line: tok.line,
                    level: level.clone(),
                    lint,
                    has_reason: t[k..=*close].iter().any(|x| x.ident && x.text == "reason"),
                    inner: a.inner,
                    in_test_module: in_regions(regions, a.start),
                    attr_end: a.end,
                });
            }
        }
    }
    out
}

/// A flattened leaf of a `use` tree.
struct UseLeaf {
    segs: Vec<String>,
    alias: bool,
    glob: bool,
}

fn parse_use_tree(
    t: &[Token],
    pos: &mut usize,
    end: usize,
    prefix: &[String],
    out: &mut Vec<UseLeaf>,
) {
    let mut segs = prefix.to_vec();
    while *pos < end {
        let tok = &t[*pos];
        match tok.text.as_str() {
            "::" => *pos += 1,
            "*" => {
                *pos += 1;
                out.push(UseLeaf {
                    segs,
                    alias: false,
                    glob: true,
                });
                return;
            }
            "{" => {
                let close = matching_close(t, *pos);
                *pos += 1;
                while *pos < close {
                    if t[*pos].text == "," {
                        *pos += 1;
                    } else {
                        parse_use_tree(t, pos, close, &segs, out);
                    }
                }
                *pos = close + 1;
                return;
            }
            "as" => {
                *pos += 2;
                out.push(UseLeaf {
                    segs,
                    alias: true,
                    glob: false,
                });
                return;
            }
            "," | "}" => break,
            _ if tok.ident => {
                if tok.text != "self" {
                    segs.push(tok.text.clone());
                }
                *pos += 1;
            }
            _ => *pos += 1,
        }
    }
    out.push(UseLeaf {
        segs,
        alias: false,
        glob: false,
    });
}

/// A disallowed route reached by source, before allowlist filtering.
struct Hit {
    token: usize,
    rule: &'static str,
    // The C-09 name reached, or `None` for a form (glob, `as` alias,
    // re-export, root alias) that no allowlist entry permits.
    name: Option<&'static str>,
    detail: String,
}

fn intern(name: &str) -> Option<&'static str> {
    FS_IMPORTABLE
        .iter()
        .chain(PROCESS_IMPORTABLE)
        .chain(QUALIFIED_PAIRS.iter().map(|(_, _, n)| n))
        .chain(METHOD_FORMS)
        .copied()
        .find(|n| *n == name)
}

/// Judge one `use` leaf, returning the finding detail and the C-09 name it
/// imports (`None` when no allowlist entry could permit the form).
fn judge_use_leaf(leaf: &UseLeaf, is_pub: bool) -> Option<(String, Option<&'static str>)> {
    let s: Vec<&str> = leaf.segs.iter().map(String::as_str).collect();
    let path = s.join("::");
    // `use std as s;`, `use std::{self as s}`, `use std::os::unix::fs as f`:
    // every path below the alias becomes invisible to this scan, so the
    // alias itself is the finding (round-2 review finding 1).
    // Only paths that still reach an `fs` or `process` module count: the
    // root itself, `std::os`, and `std::os::<platform>`. Read-only
    // extension imports such as `std::os::unix::ffi::OsStrExt as _` pass
    // (round-3 item N6); `std::os::<platform>::fs` is judged below.
    let rooted = s.first().is_some_and(|r| CRATE_ROOTS.contains(r));
    let reaches_guarded = rooted && (s.len() == 1 || (s[1] == "os" && s.len() <= 3));
    if reaches_guarded && (leaf.alias || leaf.glob) {
        let form = if leaf.glob { "glob" } else { "alias" };
        return Some((
            format!("{form} of `{path}`, which reaches fs or process modules"),
            None,
        ));
    }
    // Any `fs` or `process` segment, not only `std::fs`: `use s::fs::x`
    // after a root alias, and the `std::os::*::fs` extension modules.
    let m = s.iter().position(|x| matches!(*x, "fs" | "process"))?;
    let module = s[..=m].join("::");
    let importable = if s[m] == "fs" {
        FS_IMPORTABLE
    } else {
        PROCESS_IMPORTABLE
    };
    let listed = s
        .get(m + 1)
        .copied()
        .filter(|i| s.len() == m + 2 && importable.contains(i));
    let detail = if leaf.glob {
        format!("glob import `{path}::*` reaches every {module} item")
    } else if leaf.alias {
        format!("`use {path} as …` renames a {module} path, hiding it from this scan")
    } else if is_pub {
        format!("re-export of `{path}` from {module}")
    } else if let Some(i) = listed {
        format!("imports C-09 item `{i}` from {module}")
    } else {
        return None;
    };
    let name = if leaf.glob || leaf.alias || is_pub {
        None
    } else {
        listed.and_then(intern)
    };
    Some((detail, name))
}

/// Collect `use`-tree and `extern crate … as` hits, and return the token
/// ranges of every `use` statement so the qualified-path pass skips them.
fn use_hits(t: &[Token], hits: &mut Vec<Hit>) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    for (k, tok) in t.iter().enumerate() {
        if tok.ident && tok.text == "extern" && t.get(k + 1).is_some_and(|x| x.text == "crate") {
            let name = t.get(k + 2).map_or("", |x| x.text.as_str());
            if CRATE_ROOTS.contains(&name) && t.get(k + 3).is_some_and(|x| x.text == "as") {
                hits.push(Hit {
                    token: k,
                    rule: RULE_USE_TREE,
                    name: None,
                    detail: format!("`extern crate {name} as …` aliases a crate root"),
                });
            }
        }
        if !(tok.ident && tok.text == "use") {
            continue;
        }
        let mut is_pub = k > 0 && t[k - 1].text == "pub";
        if k > 0
            && t[k - 1].text == ")"
            && let Some(p) = (0..k - 1).rev().find(|&p| t[p].text == "(")
        {
            is_pub = p > 0 && t[p - 1].text == "pub";
        }
        let mut end = k + 1;
        let mut depth = 0i32;
        while end < t.len() {
            match t[end].text.as_str() {
                "{" => depth += 1,
                "}" => depth -= 1,
                ";" if depth <= 0 => break,
                _ => {}
            }
            end += 1;
        }
        ranges.push((k, end));
        let mut leaves = Vec::new();
        let mut pos = k + 1;
        while pos < end {
            if t[pos].text == "," {
                pos += 1;
            } else {
                parse_use_tree(t, &mut pos, end, &[], &mut leaves);
            }
        }
        for leaf in &leaves {
            if let Some((detail, name)) = judge_use_leaf(leaf, is_pub) {
                hits.push(Hit {
                    token: k,
                    rule: RULE_USE_TREE,
                    name,
                    detail,
                });
            }
        }
    }
    ranges
}

/// Report every `allow` form, and every `expect` that is not an item-level
/// attribute with a `reason` at a location C-09 allowlists for its lint.
fn check_lint_refs(
    path: &str,
    kind: FileKind,
    allowed: Option<&Allowed>,
    refs: &[LintRef],
    out: &mut Vec<Finding>,
) {
    for r in refs {
        let lowering_group = LOWERING_GROUPS.contains(&r.lint.as_str());
        if r.level == "allow" || (lowering_group && r.level == "expect") {
            out.push(finding(path, r.line, RULE_ALLOW, format!(
                "`{}({})` lowers the C-09 lints; only an allowlisted `expect` of a named lint is permitted",
                r.level, r.lint
            )));
            continue;
        }
        if r.level != "expect" {
            continue;
        }
        let place_ok = match kind {
            FileKind::Src => allowed.is_some_and(|a| a.lint == r.lint),
            FileKind::Test => r.lint == LINT_METHODS,
        };
        // An integration test is test code throughout, so the test-region
        // rule applies under src/ only; `#![cfg(test)]` atop a tests file
        // keeps the integration-test allowance.
        let in_test_module = kind == FileKind::Src && r.in_test_module;
        if !place_ok || r.inner || in_test_module {
            let why = if r.inner {
                "a crate- or module-level `#![expect]` is not item-level"
            } else if in_test_module {
                "no allowance may sit inside a `#[cfg(test)]` region under src/"
            } else {
                "C-09 does not allowlist this lint at this path"
            };
            out.push(finding(
                path,
                r.line,
                RULE_EXPECT_LOCATION,
                format!("`expect(clippy::{})`: {why}", r.lint),
            ));
        } else if !r.has_reason {
            out.push(finding(
                path,
                r.line,
                RULE_EXPECT_REASON,
                format!("`expect(clippy::{})` has no `reason`", r.lint),
            ));
        }
    }
}

/// Collect qualified-path and method-form hits outside `use` statements,
/// which `use_hits` already covers.
fn path_hits(t: &[Token], use_ranges: &[(usize, usize)], hits: &mut Vec<Hit>) {
    for k in 0..t.len().saturating_sub(2) {
        if in_regions(use_ranges, k) {
            continue;
        }
        let (a, sep, b) = (&t[k], &t[k + 1], &t[k + 2]);
        if a.text == "."
            && b.text == "("
            && let Some(m) = METHOD_FORMS.iter().copied().find(|m| *m == sep.text)
        {
            hits.push(Hit {
                token: k,
                rule: RULE_METHOD,
                name: Some(m),
                detail: format!("`.{m}(` method form"),
            });
            continue;
        }
        if !(a.ident && sep.text == "::" && b.ident) {
            continue;
        }
        let name = match a.text.as_str() {
            "fs" => FS_IMPORTABLE.iter().copied().find(|n| *n == b.text),
            "DirBuilder" => Some("DirBuilder"),
            _ => QUALIFIED_PAIRS
                .iter()
                .find(|(x, y, _)| *x == a.text && *y == b.text)
                .map(|(_, _, n)| *n),
        };
        if let Some(name) = name {
            hits.push(Hit {
                token: k,
                rule: RULE_QUALIFIED,
                name: Some(name),
                detail: format!("`{}::{}` reaches disallowed entry `{name}`", a.text, b.text),
            });
        }
    }
}

/// Findings that do not depend on the allowlisted names: FFI, `include!`,
/// `macro_rules!` in allowlisted files, and the integration-test rules.
fn structural_findings(
    path: &str,
    kind: FileKind,
    allowed: Option<&Allowed>,
    lexed: &Lexed,
    attrs: &[Attr],
    refs: &[LintRef],
    out: &mut Vec<Finding>,
) {
    let t = &lexed.tokens;
    for (k, tok) in t.iter().enumerate() {
        let next = t.get(k + 1).map_or("", |x| x.text.as_str());
        // The lexer drops the ABI string, so `extern "C" {` arrives as
        // `extern {`; `extern "C" fn` (an exported callback) arrives as
        // `extern fn` and is not FFI into another library.
        if tok.ident && tok.text == "extern" && next == "{" {
            out.push(finding(
                path,
                tok.line,
                RULE_FFI,
                "foreign `extern` block; MS-1 source does no FFI of its own",
            ));
        }
        if tok.ident && tok.text == "include" && next == "!" {
            out.push(finding(
                path,
                tok.line,
                RULE_MODULE,
                "`include!` pulls in source this scan never reads",
            ));
        }
        if tok.ident && tok.text == "macro_rules" && next == "!" && allowed.is_some() {
            out.push(finding(path, tok.line, RULE_MACRO, "`macro_rules!` in an allowlisted file could assemble a disallowed path the scan cannot see"));
        }
    }
    for a in attrs.iter().filter(|a| a.head(t) == Some("link")) {
        out.push(finding(
            path,
            t[a.start].line,
            RULE_FFI,
            "`#[link]` attribute; MS-1 source does no FFI of its own",
        ));
    }
    if kind != FileKind::Test {
        return;
    }
    for tok in t
        .iter()
        .filter(|x| x.ident && TEST_FILE_BANNED.contains(&x.text.as_str()))
    {
        out.push(finding(
            path,
            tok.line,
            RULE_TEST_NAME,
            format!(
                "`{}` in an integration test; tests may only write and create fixtures (C-09)",
                tok.text
            ),
        ));
    }
    test_copy_findings(path, t, out);
    for r in refs
        .iter()
        .filter(|r| r.level == "expect" && r.lint == LINT_METHODS)
    {
        if !item_names_tmpdir(lexed, r.attr_end) {
            out.push(finding(path, r.line, RULE_TEST_TMPDIR, "a `disallowed_methods` allowance in a test file must build its fixtures under `CARGO_TARGET_TMPDIR`, which the allowed item's body never names"));
        }
    }
}

/// Whether the body of the item an attribute ending at `attr_end` governs
/// names `CARGO_TARGET_TMPDIR`, in a string (`env!("CARGO_TARGET_TMPDIR")`)
/// or as an identifier. The attribute's own `reason` and the rest of the
/// file do not count (round-3 item F6). This proves the item names the
/// directory, not that every write lands inside it.
fn item_names_tmpdir(lexed: &Lexed, attr_end: usize) -> bool {
    let t = &lexed.tokens;
    let mut k = attr_end + 1;
    while k < t.len() && t[k].text != "{" && t[k].text != ";" {
        k += 1;
    }
    if t.get(k).is_none_or(|x| x.text != "{") {
        return false;
    }
    let close = matching_close(t, k);
    t[k..=close].iter().any(|x| x.text == "CARGO_TARGET_TMPDIR")
        || lexed
            .strings
            .iter()
            .any(|(at, s)| *at > k && *at <= close && s.contains("CARGO_TARGET_TMPDIR"))
}

/// The filesystem copy forms a test file may not use: `fs::copy` and any
/// `use` of `copy` from an `fs` module. `std::io::copy`, a local named
/// `copy` and a `.copy(` method pass (round-3 item N4).
fn test_copy_findings(path: &str, t: &[Token], out: &mut Vec<Finding>) {
    let mut hits = Vec::new();
    let use_ranges = use_hits(t, &mut hits);
    let in_use = |k: usize| in_regions(&use_ranges, k);
    for k in 0..t.len().saturating_sub(2) {
        if !in_use(k) && t[k].text == "fs" && t[k + 1].text == "::" && t[k + 2].text == "copy" {
            out.push(finding(path, t[k].line, RULE_TEST_NAME, "`fs::copy` in an integration test; tests may only write and create fixtures (C-09)"));
        }
    }
    for &(s, e) in &use_ranges {
        let body = &t[s..=e.min(t.len() - 1)];
        let fs_copy = body.iter().any(|x| x.text == "fs") && body.iter().any(|x| x.text == "copy");
        if fs_copy {
            out.push(finding(
                path,
                t[s].line,
                RULE_TEST_NAME,
                "`use` of `copy` from an `fs` module in an integration test (C-09)",
            ));
        }
    }
}

/// Fail-closed rules for constructs that could reach a guarded module
/// without spelling a path the scan can judge (round-3 items F8, F9, N1,
/// N2, N3): `cfg_attr` that conditionally sets `cfg` or `path` under src/;
/// macro-built paths (`fs::$x`, `process::$x`, `std::$x`); invocations, in
/// an allowlisted file, of any macro defined by `macro_rules!` anywhere in
/// the workspace; every `#[path]`; and `type` aliases through `fs` or
/// `process`.
fn guarded_path_findings(
    path: &str,
    kind: FileKind,
    allowed: Option<&Allowed>,
    t: &[Token],
    attrs: &[Attr],
    ctx: &ScanCtx<'_>,
    out: &mut Vec<Finding>,
) {
    for a in attrs {
        let body = &t[a.start..=a.end];
        if kind == FileKind::Src
            && a.head(t) == Some("cfg_attr")
            && body
                .iter()
                .skip(4)
                .any(|x| x.ident && (x.text == "cfg" || x.text == "path"))
        {
            out.push(finding(
                path,
                t[a.start].line,
                RULE_CFG_ATTR,
                "`cfg_attr` may not conditionally set `cfg` or `path` under src/",
            ));
        }
        if a.head(t) == Some("path") {
            out.push(finding(
                path,
                t[a.start].line,
                RULE_MODULE,
                "`#[path]` can place module source outside the scanned trees",
            ));
        }
    }
    for (k, tok) in t.iter().enumerate() {
        let next = t.get(k + 1).map_or("", |x| x.text.as_str());
        let after = t.get(k + 2).map_or("", |x| x.text.as_str());
        let guarded = matches!(tok.text.as_str(), "fs" | "process" | "os")
            || CRATE_ROOTS.contains(&tok.text.as_str());
        if tok.ident && guarded && next == "::" && after == "$" {
            out.push(finding(
                path,
                tok.line,
                RULE_MACRO_PATH,
                format!(
                    "`{}::$…` builds a path into a guarded module inside a macro",
                    tok.text
                ),
            ));
        }
        let invokes = tok.ident
            && next == "!"
            && matches!(after, "(" | "[" | "{")
            && k.checked_sub(1).is_none_or(|p| t[p].text != "macro_rules");
        if invokes && allowed.is_some() && ctx.workspace_macros.contains(&tok.text) {
            out.push(finding(
                path,
                tok.line,
                RULE_MACRO,
                format!(
                    "`{}!` is a workspace `macro_rules!` macro, invoked in an allowlisted file",
                    tok.text
                ),
            ));
        }
        if tok.ident && tok.text == "type" && t.get(k + 1).is_some_and(|x| x.ident) {
            let end = (k..t.len())
                .find(|&j| t[j].text == ";")
                .unwrap_or(t.len() - 1);
            let rhs_start = (k..end).find(|&j| t[j].text == "=");
            let through = rhs_start.is_some_and(|r| {
                (r..end).any(|j| {
                    matches!(t[j].text.as_str(), "fs" | "process")
                        && t.get(j + 1).is_some_and(|x| x.text == "::")
                })
            });
            if through {
                out.push(finding(
                    path,
                    tok.line,
                    RULE_TYPE_ALIAS,
                    "`type` alias of a path through `fs` or `process` hides it from this scan",
                ));
            }
        }
    }
}

/// Every `macro_rules!` name defined in `src`.
fn macro_names(src: &str) -> Vec<String> {
    let t = lex(src).tokens;
    (0..t.len().saturating_sub(2))
        .filter(|&k| t[k].text == "macro_rules" && t[k + 1].text == "!" && t[k + 2].ident)
        .map(|k| t[k + 2].text.clone())
        .collect()
}

/// Workspace context a single-file scan cannot derive from its own text.
struct ScanCtx<'a> {
    // The file is an out-of-line `#[cfg(test)]` module (`module_tree`).
    file_is_test: bool,
    // Every `macro_rules!` name defined anywhere in scoped source.
    workspace_macros: &'a BTreeSet<String>,
}

fn scan_rust(path: &str, src: &str) -> Vec<Finding> {
    let none = BTreeSet::new();
    scan_rust_with(
        path,
        src,
        &ScanCtx {
            file_is_test: false,
            workspace_macros: &none,
        },
    )
}

/// Scan one file. `file_is_test` marks the whole file as a `#[cfg(test)]`
/// region; the workspace scan sets it for out-of-line test modules
/// (`module_tree`).
fn scan_rust_with(path: &str, src: &str, ctx: &ScanCtx<'_>) -> Vec<Finding> {
    let file_is_test = ctx.file_is_test;
    let Some(kind) = file_kind(path) else {
        return Vec::new();
    };
    let lexed = lex(src);
    let t = &lexed.tokens;
    let attrs = attributes(t);
    let mods = inline_mods(t, &attrs);
    let regions = test_regions(t, &attrs, &mods, file_is_test);
    let refs = lint_refs(t, &attrs, &regions);
    let mut out = Vec::new();

    let allowed = (kind == FileKind::Src)
        .then(|| ALLOWLIST.iter().find(|a| a.path == path))
        .flatten();
    check_lint_refs(path, kind, allowed, &refs, &mut out);
    structural_findings(path, kind, allowed, &lexed, &attrs, &refs, &mut out);
    guarded_path_findings(path, kind, allowed, t, &attrs, ctx, &mut out);
    if kind == FileKind::Test {
        return out;
    }

    let mut hits = Vec::new();
    let use_ranges = use_hits(t, &mut hits);
    path_hits(t, &use_ranges, &mut hits);

    // An allowlisted file may reach its own entries outside any test region,
    // provided it holds an item-level `expect` of its lint. The textual layer
    // cannot tell which item an attribute governs, so per-item coverage is
    // Clippy's job: an uncovered call there still fails `disallowed_*`, and
    // an `expect` covering nothing fails `unfulfilled_lint_expectations`.
    // Because such an `expect` suppresses every disallowed method in its
    // item, not only the permitted one, the alias, glob and macro rules above
    // are what keep a second route out of that item. Rejected: requiring the
    // `expect` to precede the hit, which wrongly rejects a file-top `use` of
    // a permitted item. Everything else is a finding.
    let has_expect = |a: &Allowed| {
        refs.iter()
            .any(|r| r.level == "expect" && r.lint == a.lint && !r.inner && !r.in_test_module)
    };
    let mut seen = BTreeSet::new();
    for h in hits {
        let permitted = allowed.is_some_and(|a| {
            h.name.is_some_and(|n| a.names.contains(&n))
                && !in_regions(&regions, h.token)
                && has_expect(a)
        });
        let line = t[h.token].line;
        if !permitted && seen.insert((line, h.rule, h.detail.clone())) {
            out.push(finding(path, line, h.rule, h.detail));
        }
    }
    out
}

// ── Module tree ────────────────────────────────────────────────────────────

/// Out-of-line module resolution over the scanned sources.
struct ModuleTree {
    // Files of `#[cfg(test)]` out-of-line modules, transitively.
    test_files: BTreeSet<String>,
    // `#[path]` modules and `mod x;` declarations resolving to no scanned
    // file.
    findings: Vec<Finding>,
}

/// The directory a file's `mod x;` children resolve in: its own directory
/// for a crate or `mod.rs` root, else a directory named after its stem.
fn child_dir(path: &str) -> String {
    let (dir, file) = path.rsplit_once('/').unwrap_or(("", path));
    let crate_root = matches!(file, "lib.rs" | "main.rs" | "mod.rs")
        || crate_relative(path).is_some_and(|(top, rest)| top == "tests" && !rest.contains('/'))
        || dir.ends_with("/src/bin");
    if crate_root {
        dir.to_owned()
    } else {
        format!("{dir}/{}", file.trim_end_matches(".rs"))
    }
}

/// Resolve every out-of-line `mod x;` in `sources` to a scanned file, and
/// mark test-module files. This is the walk's independent completeness
/// check: the compiler's view of each crate must land on files the scan
/// read, so a module file the walk missed, or one moved outside the scoped
/// trees with `#[path]`, fails here.
fn module_tree(sources: &BTreeMap<String, String>) -> ModuleTree {
    let mut findings = Vec::new();
    let mut edges: Vec<(String, String, bool)> = Vec::new();
    let mut test_files = BTreeSet::new();
    for (path, src) in sources {
        let t = lex(src).tokens;
        let attrs = attributes(&t);
        let mods = inline_mods(&t, &attrs);
        let regions = test_regions(&t, &attrs, &mods, false);
        if attrs.iter().any(|a| {
            a.inner
                && a.is_cfg_test(&t)
                && !mods.iter().any(|m| m.open < a.start && a.start < m.close)
        }) {
            test_files.insert(path.clone());
        }
        for (k, tok) in t.iter().enumerate() {
            let decl = tok.ident
                && tok.text == "mod"
                && t.get(k + 1).is_some_and(|x| x.ident)
                && t.get(k + 2).is_some_and(|x| x.text == ";");
            if !decl {
                continue;
            }
            // `mod r#type;` lives in `type.rs` (round-3 item N5).
            let name = t[k + 1].text.trim_start_matches("r#");
            let (_, outer) = outer_attrs_before(&t, &attrs, k);
            // `scan_rust_with` reports every `#[path]`; this pass only
            // declines to guess where one points.
            if outer.iter().any(|a| a.head(&t) == Some("path")) {
                continue;
            }
            let mut dir = child_dir(path);
            let mut enclosing: Vec<&InlineMod> =
                mods.iter().filter(|m| m.open < k && k < m.close).collect();
            enclosing.sort_by_key(|m| m.open);
            for m in enclosing {
                dir = format!("{dir}/{}", m.name);
            }
            let candidates = [format!("{dir}/{name}.rs"), format!("{dir}/{name}/mod.rs")];
            let is_test = outer.iter().any(|a| a.is_cfg_test(&t)) || in_regions(&regions, k);
            match candidates.iter().find(|c| sources.contains_key(*c)) {
                Some(child) => edges.push((path.clone(), child.clone(), is_test)),
                None => findings.push(finding(
                    path,
                    tok.line,
                    RULE_MODULE,
                    format!(
                        "`mod {name};` resolves to neither {} nor {} among the scanned files",
                        candidates[0], candidates[1]
                    ),
                )),
            }
        }
    }
    loop {
        let before = test_files.len();
        for (parent, child, is_test) in &edges {
            if *is_test || test_files.contains(parent) {
                test_files.insert(child.clone());
            }
        }
        if test_files.len() == before {
            break;
        }
    }
    ModuleTree {
        test_files,
        findings,
    }
}

// ── TOML (a small std-only reader) ─────────────────────────────────────────

/// One assignment, flattened to its full key path. Array elements appear as
/// `[i]` segments and array-of-tables entries as `[n]`; every table header
/// is recorded with an empty value, so `[dependencies.trash]` is visible
/// even with no keys below it.
#[derive(Debug)]
struct TomlValue {
    path: Vec<String>,
    value: String,
    line: usize,
    // False when the statement that produced this value, or its table
    // header, used a backslash escape or a multi-line string.
    plain: bool,
}

/// A recursive-descent reader for the TOML the guard checks. It handles
/// basic, literal and multi-line strings, quoted and dotted keys, inline
/// tables, nested arrays, comments, tables and arrays of tables. Scalars
/// other than strings are kept as raw text. It is not a validator: it
/// accepts some invalid TOML, which Cargo or reuse would reject anyway, and
/// returns an error only where it cannot continue.
struct TomlReader {
    c: Vec<char>,
    i: usize,
    line: usize,
    out: Vec<TomlValue>,
    // Set by `string` on an escape or a multi-line string; read and reset
    // per statement by `document`.
    taint: bool,
}

impl TomlReader {
    fn peek(&self, k: usize) -> char {
        self.c.get(self.i + k).copied().unwrap_or('\0')
    }

    fn bump(&mut self) -> char {
        let ch = self.peek(0);
        if ch == '\n' {
            self.line += 1;
        }
        self.i += 1;
        ch
    }

    fn eof(&self) -> bool {
        self.i >= self.c.len()
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(0), ' ' | '\t' | '\r') {
            self.bump();
        }
    }

    /// Skip whitespace, newlines and comments.
    fn skip_all(&mut self) {
        loop {
            self.skip_ws();
            match self.peek(0) {
                '\n' => {
                    self.bump();
                }
                '#' => {
                    while !self.eof() && self.peek(0) != '\n' {
                        self.bump();
                    }
                }
                _ => return,
            }
        }
    }

    fn string(&mut self) -> Result<String, String> {
        let q = self.bump();
        let multi = self.peek(0) == q && self.peek(1) == q;
        self.taint |= multi;
        if multi {
            self.bump();
            self.bump();
        }
        let mut s = String::new();
        loop {
            if self.eof() {
                return Err(format!("line {}: unterminated string", self.line));
            }
            let ch = self.bump();
            if ch == q && (!multi || (self.peek(0) == q && self.peek(1) == q)) {
                if multi {
                    self.bump();
                    self.bump();
                }
                return Ok(s);
            }
            if ch == '\n' && !multi {
                return Err(format!("line {}: newline in string", self.line));
            }
            if ch == '\\' && q == '"' {
                self.taint = true;
                s.push(self.bump());
            } else {
                s.push(ch);
            }
        }
    }

    fn key(&mut self) -> Result<Vec<String>, String> {
        let mut segs = Vec::new();
        loop {
            self.skip_ws();
            let seg = if matches!(self.peek(0), '"' | '\'') {
                self.string()?
            } else {
                let start = self.i;
                while self.peek(0).is_ascii_alphanumeric() || matches!(self.peek(0), '_' | '-') {
                    self.bump();
                }
                if self.i == start {
                    return Err(format!("line {}: expected a key", self.line));
                }
                self.c[start..self.i].iter().collect()
            };
            segs.push(seg);
            self.skip_ws();
            if self.peek(0) == '.' {
                self.bump();
            } else {
                return Ok(segs);
            }
        }
    }

    fn value(&mut self, path: &[String]) -> Result<(), String> {
        self.skip_ws();
        let line = self.line;
        let sub = |p: &[String], extra: String| {
            let mut v = p.to_vec();
            v.push(extra);
            v
        };
        match self.peek(0) {
            '"' | '\'' => {
                let value = self.string()?;
                self.out.push(TomlValue {
                    path: path.to_vec(),
                    value,
                    line,
                    plain: true,
                });
            }
            '[' => {
                self.bump();
                let mut n = 0;
                loop {
                    self.skip_all();
                    if self.peek(0) == ']' {
                        self.bump();
                        return Ok(());
                    }
                    self.value(&sub(path, format!("[{n}]")))?;
                    n += 1;
                    self.skip_all();
                    match self.bump() {
                        ',' => {}
                        ']' => return Ok(()),
                        _ => return Err(format!("line {}: expected `,` or `]`", self.line)),
                    }
                }
            }
            '{' => {
                self.bump();
                loop {
                    self.skip_ws();
                    if self.peek(0) == '}' {
                        self.bump();
                        return Ok(());
                    }
                    let k = self.key()?;
                    self.skip_ws();
                    if self.bump() != '=' {
                        return Err(format!("line {}: expected `=`", self.line));
                    }
                    let mut p = path.to_vec();
                    p.extend(k);
                    self.value(&p)?;
                    self.skip_ws();
                    match self.bump() {
                        ',' => {}
                        '}' => return Ok(()),
                        _ => return Err(format!("line {}: expected `,` or `}}`", self.line)),
                    }
                }
            }
            _ => {
                let start = self.i;
                while !self.eof() && !matches!(self.peek(0), ',' | ']' | '}' | '\n' | '#') {
                    self.bump();
                }
                let value: String = self.c[start..self.i]
                    .iter()
                    .collect::<String>()
                    .trim()
                    .to_owned();
                if value.is_empty() {
                    return Err(format!("line {line}: expected a value"));
                }
                self.out.push(TomlValue {
                    path: path.to_vec(),
                    value,
                    line,
                    plain: true,
                });
            }
        }
        Ok(())
    }

    fn document(mut self) -> Result<Vec<TomlValue>, String> {
        let mut table: Vec<String> = Vec::new();
        let mut aot_counts: BTreeMap<Vec<String>, usize> = BTreeMap::new();
        let mut table_taint = false;
        loop {
            self.skip_all();
            if self.eof() {
                return Ok(self.out);
            }
            let line = self.line;
            let first = self.out.len();
            self.taint = false;
            let header = self.peek(0) == '[';
            if header {
                self.bump();
                let aot = self.peek(0) == '[';
                if aot {
                    self.bump();
                }
                let k = self.key()?;
                for _ in 0..if aot { 2 } else { 1 } {
                    self.skip_ws();
                    if self.bump() != ']' {
                        return Err(format!("line {line}: unclosed table header"));
                    }
                }
                table.clone_from(&k);
                if aot {
                    let n = aot_counts.entry(k).or_insert(0);
                    table.push(format!("[{n}]"));
                    *n += 1;
                }
                self.out.push(TomlValue {
                    path: table.clone(),
                    value: String::new(),
                    line,
                    plain: true,
                });
                table_taint = self.taint;
            } else {
                let k = self.key()?;
                self.skip_ws();
                if self.bump() != '=' {
                    return Err(format!("line {line}: expected `=`"));
                }
                let mut p = table.clone();
                p.extend(k);
                self.value(&p)?;
            }
            // Taint covers the whole statement, so an escape in one array
            // element or key segment marks every value it produced, and a
            // tainted header marks every value of its table.
            if self.taint || table_taint {
                for v in &mut self.out[first..] {
                    v.plain = false;
                }
            }
        }
    }
}

fn parse_toml(src: &str) -> Result<Vec<TomlValue>, String> {
    TomlReader {
        c: src.chars().collect(),
        i: 0,
        line: 1,
        out: Vec::new(),
        taint: false,
    }
    .document()
}

/// Parse `content`, or report the parse failure as a finding: a file the
/// guard cannot read must fail it, never pass it.
fn parse_or_report(path: &str, content: &str, out: &mut Vec<Finding>) -> Vec<TomlValue> {
    parse_toml(content).unwrap_or_else(|e| {
        out.push(finding(path, 0, RULE_TOML, format!("cannot parse: {e}")));
        Vec::new()
    })
}

/// Lowercased, with `-` as `_`, the way Cargo and Clippy compare lint names
/// and configuration keys.
fn norm(seg: &str) -> String {
    seg.to_lowercase().replace('-', "_")
}

fn check_clippy_toml(content: &str) -> Vec<Finding> {
    let mut out = Vec::new();
    let values = parse_or_report("clippy.toml", content, &mut out);
    let entries = |key: &str| -> BTreeSet<&str> {
        values
            .iter()
            .filter(|v| {
                let p: Vec<String> = v.path.iter().map(|s| norm(s)).collect();
                p.first().is_some_and(|k| k == key)
                    && (p.len() == 2 || (p.len() == 3 && p[2] == "path"))
                    && p[1].starts_with('[')
            })
            .map(|v| v.value.as_str())
            .collect()
    };
    // A decoded escape could spell a path that matches while the raw text
    // reads differently, so this file must spell every string plainly
    // (round-3 items F3, F4).
    for v in values.iter().filter(|v| !v.plain) {
        out.push(finding(
            "clippy.toml",
            v.line,
            RULE_TOML_ESCAPE,
            format!(
                "`{}`: unsupported escape or multi-line string; spell it plainly",
                v.path.join(".")
            ),
        ));
    }
    let methods = entries("disallowed_methods");
    let types = entries("disallowed_types");
    let wanted = MIN_DISALLOWED_METHODS
        .iter()
        .chain(EXTRA_DISALLOWED_METHODS)
        .map(|w| ("disallowed-methods", w, &methods))
        .chain(
            MIN_DISALLOWED_TYPES
                .iter()
                .map(|w| ("disallowed-types", w, &types)),
        );
    for (key, want, have) in wanted {
        if !have.contains(*want) {
            out.push(finding(
                "clippy.toml",
                0,
                RULE_CLIPPY_ENTRY,
                format!("`{key}` lacks the required entry `{want}`"),
            ));
        }
    }
    out
}

fn is_clippy_config(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    name == "clippy.toml" || name == ".clippy.toml"
}

fn check_clippy_config_paths(paths: &[String]) -> Vec<Finding> {
    let mut out: Vec<Finding> = paths
        .iter()
        .filter(|p| *p != "clippy.toml")
        .map(|p| finding(p, 0, RULE_CLIPPY_FILE, "a second Clippy configuration file would replace the workspace clippy.toml for every crate below it"))
        .collect();
    if !paths.iter().any(|p| p == "clippy.toml") {
        out.push(finding(
            "clippy.toml",
            0,
            RULE_CLIPPY_FILE,
            "the workspace clippy.toml is missing",
        ));
    }
    out
}

fn is_manifest(path: &str) -> bool {
    path == "Cargo.toml" || (path.starts_with("crates/") && path.ends_with("/Cargo.toml"))
}

/// Whether one flattened manifest value lowers a C-09 lint: under
/// `lints.<tool>.<lint>` or `workspace.lints.<tool>.<lint>`, as the lint's
/// value or its `level`, set to `allow` or `expect`, for either lint or a
/// group containing them.
fn lowers_lint(p: &[String], value: &str) -> bool {
    let Some(i) = p.iter().position(|s| s == "lints") else {
        return false;
    };
    if !(i == 0 || (i == 1 && p[0] == "workspace")) {
        return false;
    }
    let (Some(tool), Some(lint)) = (p.get(i + 1), p.get(i + 2)) else {
        return false;
    };
    let rest = &p[i + 3..];
    let watched = match tool.as_str() {
        "clippy" => matches!(
            lint.as_str(),
            "disallowed_methods" | "disallowed_types" | "all" | "style"
        ),
        "rust" => lint == "warnings",
        _ => false,
    };
    watched
        && (rest.is_empty() || rest == ["level"])
        && matches!(norm(value).as_str(), "allow" | "expect")
}

fn check_manifest(path: &str, content: &str) -> Vec<Finding> {
    let mut out = Vec::new();
    for v in parse_or_report(path, content, &mut out) {
        let p: Vec<String> = v.path.iter().map(|s| norm(s)).collect();
        if !v.plain && p.iter().any(|s| s == "lints") {
            out.push(finding(path, v.line, RULE_TOML_ESCAPE, format!("`{}`: unsupported escape or multi-line string in a lint table; spell it plainly", v.path.join("."))));
        }
        if lowers_lint(&p, &v.value) {
            out.push(finding(
                path,
                v.line,
                RULE_MANIFEST_LINT,
                format!("`{} = {}` lowers a C-09 lint", v.path.join("."), v.value),
            ));
        }
        let value_trash = v.value.to_lowercase().contains("trash");
        let trash = match p.iter().position(|s| s.ends_with("dependencies")) {
            Some(d) => {
                p[d + 1..].iter().any(|s| s.contains("trash"))
                    || (p.last().is_some_and(|s| s == "package") && value_trash)
            }
            None => p == ["package", "name"] && value_trash,
        };
        if trash {
            out.push(finding(
                path,
                v.line,
                RULE_TRASH,
                format!("`{}` names a Trash crate (exit item 8)", v.path.join(".")),
            ));
        }
    }
    out
}

fn check_lockfile(content: &str) -> Vec<Finding> {
    let mut out = Vec::new();
    for v in parse_or_report("Cargo.lock", content, &mut out) {
        let package_name = v.path.len() == 3 && v.path[0] == "package" && v.path[2] == "name";
        if package_name && v.value.to_lowercase().contains("trash") {
            out.push(finding(
                "Cargo.lock",
                v.line,
                RULE_TRASH,
                format!(
                    "package `{}`: the lockfile resolves a Trash crate (exit item 8)",
                    v.value
                ),
            ));
        }
    }
    out
}

/// Match a REUSE annotation glob: `*` within one path component, `**` across
/// components, `?` one character.
///
/// `**/` also matches zero directories, so `**/*.toml` matches a root file.
/// Whether reuse itself reads it that way does not matter here: this check
/// only has to be at least as broad as reuse, and a false match costs one
/// rewritten glob, while a missed one silently lets an annotation cover
/// `clippy.toml`.
fn glob_match(pat: &[char], s: &[char]) -> bool {
    match pat.first() {
        None => s.is_empty(),
        Some('*') if pat.get(1) == Some(&'*') && pat.get(2) == Some(&'/') => {
            glob_match(&pat[3..], s) || (0..=s.len()).any(|i| glob_match(&pat[2..], &s[i..]))
        }
        Some('*') if pat.get(1) == Some(&'*') => {
            (0..=s.len()).any(|i| glob_match(&pat[2..], &s[i..]))
        }
        Some('*') => (0..=s.len())
            .take_while(|&i| i == 0 || s[i - 1] != '/')
            .any(|i| glob_match(&pat[1..], &s[i..])),
        Some('?') => !s.is_empty() && s[0] != '/' && glob_match(&pat[1..], &s[1..]),
        Some(c) => s.first() == Some(c) && glob_match(&pat[1..], &s[1..]),
    }
}

fn check_reuse(content: &str) -> Vec<Finding> {
    let target: Vec<char> = "clippy.toml".chars().collect();
    let mut out = Vec::new();
    for v in parse_or_report("REUSE.toml", content, &mut out) {
        let is_path = v.path.first().is_some_and(|s| s == "annotations")
            && v.path.get(2).is_some_and(|s| s == "path")
            && v.path.len() <= 4;
        if is_path && glob_match(&v.value.chars().collect::<Vec<_>>(), &target) {
            out.push(finding("REUSE.toml", v.line, RULE_REUSE, format!("annotation path `{}` matches clippy.toml, which must carry its own SPDX header", v.value)));
        }
    }
    out
}

/// Reconcile the root manifest's `[workspace] members` with the crate
/// directories found under `crates/`: a crate the workspace builds but the
/// scan never walks, or a walked directory the workspace ignores, fails
/// (round-3 item F2). A member with `*` is matched as a glob.
fn check_members(manifest: &str, crate_dirs: &[String]) -> Vec<Finding> {
    let mut out = Vec::new();
    let members: Vec<String> = parse_or_report("Cargo.toml", manifest, &mut out)
        .into_iter()
        .filter(|v| v.path.len() == 3 && v.path[0] == "workspace" && v.path[1] == "members")
        .map(|v| v.value)
        .collect();
    if members.is_empty() {
        out.push(finding(
            "Cargo.toml",
            0,
            RULE_MEMBERS,
            "no `[workspace] members` list to reconcile with crates/",
        ));
    }
    let matches = |m: &str, d: &str| {
        glob_match(
            &m.chars().collect::<Vec<_>>(),
            &d.chars().collect::<Vec<_>>(),
        )
    };
    for m in members.iter().filter(|m| !m.contains('*')) {
        if !crate_dirs.contains(m) {
            out.push(finding(
                "Cargo.toml",
                0,
                RULE_MEMBERS,
                format!("workspace member `{m}` is not a directory the scan walks"),
            ));
        }
    }
    for d in crate_dirs {
        if !members.iter().any(|m| matches(m, d)) {
            out.push(finding(
                d,
                0,
                RULE_MEMBERS,
                "crate directory is not a workspace member",
            ));
        }
    }
    out
}

// ── Workspace walk ─────────────────────────────────────────────────────────

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum EntryKind {
    File,
    Dir,
    Symlink,
    Other,
}

struct Entry {
    rel: String,
    kind: EntryKind,
}

/// Every entry below `root/start`, relative to `root` and `/`-separated,
/// never following a symbolic link. A directory is entered when
/// `descend(rel, has_own_git)` allows it; `has_own_git` is whether the
/// directory holds a `.git` entry of its own.
fn list_tree(root: &Path, start: &str, descend: &dyn Fn(&str, bool) -> bool) -> Vec<Entry> {
    let mut out = Vec::new();
    let mut stack = vec![PathBuf::from(start)];
    while let Some(rel) = stack.pop() {
        let dir = root.join(&rel);
        let entries = std::fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("cannot list {}: {e}", dir.display()));
        for entry in entries {
            let entry = entry.unwrap_or_else(|e| panic!("cannot list {}: {e}", dir.display()));
            let child = rel.join(entry.file_name());
            let child_rel = child.to_string_lossy().replace('\\', "/");
            let ty = entry
                .file_type()
                .unwrap_or_else(|e| panic!("cannot stat {child_rel}: {e}"));
            let kind = if ty.is_symlink() {
                EntryKind::Symlink
            } else if ty.is_dir() {
                EntryKind::Dir
            } else if ty.is_file() {
                EntryKind::File
            } else {
                EntryKind::Other
            };
            if kind == EntryKind::Dir {
                let has_own_git = root.join(&child).join(".git").symlink_metadata().is_ok();
                if descend(&child_rel, has_own_git) {
                    stack.push(child);
                }
            }
            out.push(Entry {
                rel: child_rel,
                kind,
            });
        }
    }
    out.sort_by(|a, b| a.rel.cmp(&b.rel));
    out
}

/// Whether the repository walk (configuration files and manifests) enters a
/// directory. Tool caches are skipped only at the root, where they live; a
/// directory outside `crates/` with its own `.git` is a foreign checkout,
/// such as a Git worktree under `.claude/worktrees/` with its own
/// `clippy.toml`. Inside `crates/` nothing is skipped, so a nested checkout
/// there still has its Clippy configuration found.
fn repo_descend(rel: &str, has_own_git: bool) -> bool {
    const ROOT_SKIP: &[&str] = &[
        ".git",
        "target",
        "node_modules",
        ".venv",
        ".pytest_cache",
        ".ruff_cache",
    ];
    if !rel.contains('/') && ROOT_SKIP.contains(&rel) {
        return false;
    }
    !has_own_git || rel.starts_with("crates/")
}

/// A symbolic link in a scoped tree could point the compiler at source the
/// scan never reads, or make the scan read a file the compiler never sees.
fn check_scoped_entries(entries: &[Entry]) -> Vec<Finding> {
    entries
        .iter()
        .filter(|e| e.kind == EntryKind::Symlink)
        .map(|e| {
            finding(
                &e.rel,
                0,
                RULE_SYMLINK,
                "symbolic link inside a scanned source tree",
            )
        })
        .collect()
}

/// Every guard test lives here so that nextest's `test(no_file_mutation)`
/// filter, which matches test names rather than the binary name, selects all
/// of them.
mod no_file_mutation {
    use super::*;

    const APP_MAIN: &str = "crates/fuzzy-dedupe-app/src/main.rs";
    const DOMAIN_LIB: &str = "crates/fuzzy-dedupe-domain/src/lib.rs";
    const APP_FILES: &str = "crates/fuzzy-dedupe-platform/src/app_files.rs";
    const LOG_RETENTION: &str = "crates/fuzzy-dedupe-platform/src/log_retention.rs";
    const OPEN_RS: &str = "crates/fuzzy-dedupe-platform/src/open.rs";
    const TEST_FILE: &str = "crates/fuzzy-dedupe-app/tests/fixture.rs";

    #[track_caller]
    fn assert_flags(findings: &[Finding], rule: &str) {
        assert!(
            findings.iter().any(|f| f.rule == rule),
            "expected a `{rule}` finding, got {findings:#?}"
        );
    }

    #[track_caller]
    fn assert_clean(findings: &[Finding]) {
        assert!(
            findings.is_empty(),
            "expected no findings, got {findings:#?}"
        );
    }

    // ── Workspace ──────────────────────────────────────────────────────────

    #[test]
    fn workspace_is_clean() {
        let root = workspace_root();
        let read = |rel: &str| {
            std::fs::read_to_string(root.join(rel))
                .unwrap_or_else(|e| panic!("cannot read {rel}: {e}"))
        };
        let mut findings = Vec::new();

        // Scoped trees: walked completely, with no skips at any depth.
        let crate_dirs: Vec<String> = list_tree(&root, "crates", &|_, _| false)
            .into_iter()
            .filter(|e| e.kind == EntryKind::Dir)
            .map(|e| e.rel)
            .collect();
        assert!(!crate_dirs.is_empty(), "no crates found under crates/");
        // Scope boundaries: a symlinked crate directory, manifest, `src/` or
        // `tests/` root would let the compiler read a tree the walk does not
        // (round-3 item F2).
        let kind_of = |rel: &str| match root.join(rel).symlink_metadata() {
            Ok(m) if m.file_type().is_symlink() => Some(EntryKind::Symlink),
            Ok(_) => Some(EntryKind::File),
            Err(_) => None,
        };
        let mut boundaries: Vec<Entry> = list_tree(&root, "crates", &|_, _| false)
            .into_iter()
            .filter(|e| e.kind == EntryKind::Symlink)
            .collect();
        for rel in std::iter::once("Cargo.toml".to_owned()).chain(
            crate_dirs
                .iter()
                .flat_map(|d| ["Cargo.toml", "src", "tests"].map(|x| format!("{d}/{x}"))),
        ) {
            if let Some(kind) = kind_of(&rel) {
                boundaries.push(Entry { rel, kind });
            }
        }
        findings.extend(check_scoped_entries(&boundaries));
        findings.extend(check_members(&read("Cargo.toml"), &crate_dirs));
        let mut scoped = Vec::new();
        for dir in &crate_dirs {
            for sub in ["src", "tests"] {
                let tree = format!("{dir}/{sub}");
                if root.join(&tree).is_dir() {
                    scoped.extend(list_tree(&root, &tree, &|_, _| true));
                }
            }
        }
        findings.extend(check_scoped_entries(&scoped));
        let sources: BTreeMap<String, String> = scoped
            .iter()
            .filter(|e| e.kind == EntryKind::File && is_scanned_rust(&e.rel))
            .map(|e| (e.rel.clone(), read(&e.rel)))
            .collect();

        // Vacuity guard: every crate's root file must be among the scanned
        // sources, and `module_tree` below proves every out-of-line module
        // the compiler would load was scanned too.
        for dir in &crate_dirs {
            let roots = [format!("{dir}/src/lib.rs"), format!("{dir}/src/main.rs")];
            assert!(
                roots.iter().any(|r| sources.contains_key(r)),
                "the walk scanned no crate root under {dir}/src/"
            );
        }

        let tree = module_tree(&sources);
        findings.extend(tree.findings);
        let macros: BTreeSet<String> = sources.values().flat_map(|s| macro_names(s)).collect();
        for (rel, src) in &sources {
            let ctx = ScanCtx {
                file_is_test: tree.test_files.contains(rel),
                workspace_macros: &macros,
            };
            findings.extend(scan_rust_with(rel, src, &ctx));
        }

        let files = list_tree(&root, "", &repo_descend);
        let configs: Vec<String> = files
            .iter()
            .filter(|e| e.kind != EntryKind::Dir && is_clippy_config(&e.rel))
            .map(|e| e.rel.clone())
            .collect();
        findings.extend(check_clippy_config_paths(&configs));
        findings.extend(check_clippy_toml(&read("clippy.toml")));
        for e in files
            .iter()
            .filter(|e| e.kind == EntryKind::File && is_manifest(&e.rel))
        {
            findings.extend(check_manifest(&e.rel, &read(&e.rel)));
        }
        findings.extend(check_lockfile(&read("Cargo.lock")));
        findings.extend(check_reuse(&read("REUSE.toml")));

        let report: Vec<String> = findings.iter().map(ToString::to_string).collect();
        assert!(
            report.is_empty(),
            "no-file-mutation guard:\n{}",
            report.join("\n")
        );
    }

    #[test]
    fn ci_test_job_has_no_no_tests_warn() {
        let workflow = std::fs::read_to_string(workspace_root().join(".github/workflows/rust.yml"))
            .expect("rust.yml is readable");
        let live = workflow
            .lines()
            .filter(|l| !l.trim_start().starts_with('#'))
            .any(|l| l.contains("--no-tests=warn"));
        assert!(!live, "rust.yml still passes --no-tests=warn to nextest");
    }

    // ── Textual canaries under src/ ────────────────────────────────────────

    #[test]
    fn flags_fully_qualified_call() {
        let src = "fn f(p: &std::path::Path) { let _ = std::fs::remove_file(p); }";
        assert_flags(&scan_rust(APP_MAIN, src), RULE_QUALIFIED);
    }

    #[test]
    fn flags_module_qualified_call() {
        let src = "use std::fs;\nfn f() { let _ = fs::remove_dir(\"x\"); }";
        assert_flags(&scan_rust(APP_MAIN, src), RULE_QUALIFIED);
    }

    #[test]
    fn flags_imported_item() {
        let src = "use std::fs::remove_file;\nfn f() { let _ = remove_file(\"x\"); }";
        assert_flags(&scan_rust(APP_MAIN, src), RULE_USE_TREE);
    }

    #[test]
    fn flags_nested_imported_item() {
        let src = "use std::{io, fs::{self, remove_file}};";
        assert_flags(&scan_rust(APP_MAIN, src), RULE_USE_TREE);
    }

    #[test]
    fn flags_aliased_item() {
        let src = "use std::fs::rename as mv;\nfn f() { let _ = mv(\"a\", \"b\"); }";
        assert_flags(&scan_rust(APP_MAIN, src), RULE_USE_TREE);
    }

    #[test]
    fn flags_aliased_module() {
        assert_flags(
            &scan_rust(APP_MAIN, "use std::process as p;"),
            RULE_USE_TREE,
        );
        assert_flags(&scan_rust(APP_MAIN, "use std::fs as f;"), RULE_USE_TREE);
        assert_flags(
            &scan_rust(APP_MAIN, "use std::process::{self as p};"),
            RULE_USE_TREE,
        );
    }

    #[test]
    fn flags_glob_import() {
        assert_flags(&scan_rust(APP_MAIN, "use std::fs::*;"), RULE_USE_TREE);
    }

    #[test]
    fn flags_reexport() {
        let src = "pub use std::fs::remove_dir_all;";
        assert_flags(&scan_rust(DOMAIN_LIB, src), RULE_USE_TREE);
        let src = "pub(crate) use std::fs::remove_dir_all as tidy;";
        assert_flags(&scan_rust(DOMAIN_LIB, src), RULE_USE_TREE);
    }

    #[test]
    fn flags_multiline_forms() {
        let call =
            "fn f() {\n    let _ = std::fs::\n        remove_dir_all(\n        \"x\",\n    );\n}";
        assert_flags(&scan_rust(APP_MAIN, call), RULE_QUALIFIED);
        let tree = "use std::{\n    fs::{\n        self as f,\n    },\n};";
        assert_flags(&scan_rust(APP_MAIN, tree), RULE_USE_TREE);
    }

    #[test]
    fn flags_call_inside_cfg_test_module() {
        let src = "pub fn f() {}\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() {\n        let _ = std::fs::remove_file(\"x\");\n    }\n}";
        assert_flags(&scan_rust(APP_MAIN, src), RULE_QUALIFIED);
    }

    #[test]
    fn flags_rename_method_form() {
        let src = "fn f(d: &Dir) { d.rename(\"a\", \"b\"); }";
        assert_flags(&scan_rust(APP_MAIN, src), RULE_METHOD);
    }

    #[test]
    fn flags_process_command() {
        let qualified = "fn f() { let _ = std::process::Command::new(\"xdg-open\"); }";
        assert_flags(&scan_rust(APP_MAIN, qualified), RULE_QUALIFIED);
        let bare = "fn f() { let _ = Command::new(\"xdg-open\"); }";
        assert_flags(&scan_rust(APP_MAIN, bare), RULE_QUALIFIED);
        let imported = "use std::process::{Command, ExitCode};";
        assert_flags(&scan_rust(APP_MAIN, imported), RULE_USE_TREE);
    }

    #[test]
    fn flags_write_and_create_entry_points() {
        for src in [
            "fn f() { let _ = std::fs::write(\"x\", b\"\"); }",
            "fn f() { let _ = std::fs::copy(\"a\", \"b\"); }",
            "fn f() { let _ = std::fs::create_dir_all(\"x\"); }",
            "fn f() { let _ = std::fs::File::create(\"x\"); }",
            "fn f() { let _ = File::create_new(\"x\"); }",
            "fn f() { let _ = std::fs::OpenOptions::new().write(true).open(\"x\"); }",
            "fn f() { let _ = std::fs::hard_link(\"a\", \"b\"); }",
            "fn f(p: std::fs::Permissions) { let _ = std::fs::set_permissions(\"x\", p); }",
        ] {
            assert_flags(&scan_rust(APP_MAIN, src), RULE_QUALIFIED);
        }
    }

    // ── Mentions that must be accepted ─────────────────────────────────────

    #[test]
    fn accepts_names_in_strings_and_comments() {
        let src = r##"
//! Never calls std::fs::remove_file.
/// Documents `fs::rename` and `Command::new`.
// use std::fs::remove_file;
/* outer /* nested std::fs::remove_dir_all */ still a comment fs::write */
const A: &str = "std::fs::remove_file(p); use std::process::Command;";
const B: &str = r#"fs::rename("a", "b") .rename( "#;
const C: &[u8] = b"fs::remove_dir";
const D: char = '"';
const E: char = '\'';
fn f<'a>(x: &'a str) -> &'a str { let _ = "\"fs::copy"; x }
fn g(d: &str) -> bool { d.contains("rename") }
use std::fs::{self, File};
use std::process::ExitCode;
fn h(p: &std::path::Path) -> std::io::Result<String> { let _ = File::open(p)?; fs::read_to_string(p) }
"##;
        assert_clean(&scan_rust(APP_MAIN, src));
    }

    // ── Allowlist ──────────────────────────────────────────────────────────

    #[test]
    fn accepts_allowlisted_call_with_expect() {
        let retention = "#[expect(clippy::disallowed_methods, reason = \"rotates the app's own logs\")]\npub fn expire(p: &std::path::Path) { let _ = std::fs::remove_file(p); }";
        assert_clean(&scan_rust(LOG_RETENTION, retention));
        let writer = "use std::fs::OpenOptions;\n#[expect(clippy::disallowed_methods, reason = \"app-owned writer\")]\npub fn put(t: &std::path::Path, d: &std::path::Path) {\n    let _ = std::fs::write(t, b\"\");\n    let _ = OpenOptions::new().create_new(true).open(t);\n    let _ = std::fs::rename(t, d);\n}";
        assert_clean(&scan_rust(APP_FILES, writer));
        let open = "#[expect(clippy::disallowed_types, reason = \"the only process spawn\")]\npub fn open(u: &str) { let _ = std::process::Command::new(\"xdg-open\").arg(u); }";
        assert_clean(&scan_rust(OPEN_RS, open));
        let fixture = "#[test]\n#[expect(clippy::disallowed_methods, reason = \"fixture under CARGO_TARGET_TMPDIR\")]\nfn t() { std::fs::write(env!(\"CARGO_TARGET_TMPDIR\"), b\"\").ok(); }";
        assert_clean(&scan_rust(TEST_FILE, fixture));
    }

    #[test]
    fn flags_allowlisted_call_without_expect() {
        let src = "pub fn expire(p: &std::path::Path) { let _ = std::fs::remove_file(p); }";
        assert_flags(&scan_rust(LOG_RETENTION, src), RULE_QUALIFIED);
        let src = "pub fn open() { let _ = std::process::Command::new(\"xdg-open\"); }";
        assert_flags(&scan_rust(OPEN_RS, src), RULE_QUALIFIED);
    }

    #[test]
    fn flags_entry_outside_its_allowlisted_file() {
        // log_retention.rs may delete but never rename; open.rs may spawn but
        // never write.
        let src = "#[expect(clippy::disallowed_methods, reason = \"x\")]\npub fn f(p: &std::path::Path) { let _ = std::fs::rename(p, p); }";
        assert_flags(&scan_rust(LOG_RETENTION, src), RULE_QUALIFIED);
        let src = "#[expect(clippy::disallowed_methods, reason = \"x\")]\npub fn f() { let _ = std::fs::write(\"x\", b\"\"); }";
        assert_flags(&scan_rust(OPEN_RS, src), RULE_EXPECT_LOCATION);
        let src = "#[expect(clippy::disallowed_methods, reason = \"x\")]\npub fn f() { let _ = std::fs::remove_file(\"x\"); }";
        assert_flags(&scan_rust(APP_FILES, src), RULE_QUALIFIED);
    }

    #[test]
    fn flags_expect_outside_allowlist() {
        let src = "#[expect(clippy::disallowed_methods, reason = \"x\")]\nfn f() { let _ = std::fs::remove_file(\"x\"); }";
        assert_flags(&scan_rust(APP_MAIN, src), RULE_EXPECT_LOCATION);
        let src = "#[expect(clippy::disallowed_types, reason = \"x\")]\nfn f() {}";
        assert_flags(&scan_rust(LOG_RETENTION, src), RULE_EXPECT_LOCATION);
        let src = "#[expect(clippy::disallowed_types, reason = \"x\")]\nfn f() {}";
        assert_flags(&scan_rust(TEST_FILE, src), RULE_EXPECT_LOCATION);
    }

    #[test]
    fn flags_crate_level_and_test_module_expect() {
        let src = "#![expect(clippy::disallowed_methods, reason = \"x\")]\npub fn f() { let _ = std::fs::remove_file(\"x\"); }";
        assert_flags(&scan_rust(LOG_RETENTION, src), RULE_EXPECT_LOCATION);
        let src = "#[cfg(test)]\nmod tests {\n    #[expect(clippy::disallowed_methods, reason = \"x\")]\n    fn t() { let _ = std::fs::remove_file(\"x\"); }\n}";
        assert_flags(&scan_rust(LOG_RETENTION, src), RULE_EXPECT_LOCATION);
        let src =
            "#[cfg(test)]\n#[expect(clippy::disallowed_methods, reason = \"x\")]\nmod tests {}";
        assert_flags(&scan_rust(LOG_RETENTION, src), RULE_EXPECT_LOCATION);
    }

    #[test]
    fn flags_expect_without_reason() {
        let src = "#[expect(clippy::disallowed_methods)]\npub fn f(p: &std::path::Path) { let _ = std::fs::remove_file(p); }";
        assert_flags(&scan_rust(LOG_RETENTION, src), RULE_EXPECT_REASON);
    }

    #[test]
    fn flags_every_allow_form() {
        for (path, src) in [
            (APP_MAIN, "#[allow(clippy::disallowed_methods)]\nfn f() {}"),
            (APP_MAIN, "#[allow(clippy::disallowed_types)]\nfn f() {}"),
            (
                LOG_RETENTION,
                "#[allow(clippy::disallowed_methods, reason = \"x\")]\nfn f() {}",
            ),
            (APP_MAIN, "#![allow(clippy::disallowed_methods)]"),
            (
                APP_MAIN,
                "#[cfg_attr(test, allow(clippy::disallowed_types))]\nfn f() {}",
            ),
            (APP_MAIN, "#[allow(clippy::style)]\nfn f() {}"),
            (
                APP_MAIN,
                "#[expect(clippy::all, reason = \"x\")]\nfn f() {}",
            ),
            (TEST_FILE, "#[allow(clippy::disallowed_methods)]\nfn f() {}"),
        ] {
            assert_flags(&scan_rust(path, src), RULE_ALLOW);
        }
    }

    // ── Integration-test files ─────────────────────────────────────────────

    #[test]
    fn flags_delete_and_process_names_in_test_files() {
        for src in [
            "#[test]\nfn t() { std::fs::remove_file(\"x\").ok(); }",
            "use std::fs::remove_dir_all;",
            "#[test]\nfn t() { std::fs::rename(\"a\", \"b\").ok(); }",
            "#[test]\nfn t() { std::fs::hard_link(\"a\", \"b\").ok(); }",
            "use std::process::Command;",
        ] {
            assert_flags(&scan_rust(TEST_FILE, src), RULE_TEST_NAME);
        }
    }

    // ── Clippy configuration ───────────────────────────────────────────────

    fn full_clippy_toml(skip: &str) -> String {
        let entry = |p: &&str| format!("  {{ path = \"{p}\", reason = \"C-09\" }},\n");
        let methods: String = MIN_DISALLOWED_METHODS
            .iter()
            .chain(EXTRA_DISALLOWED_METHODS)
            .filter(|p| **p != skip)
            .map(entry)
            .collect();
        let types: String = MIN_DISALLOWED_TYPES
            .iter()
            .filter(|p| **p != skip)
            .map(entry)
            .collect();
        format!("# comment\ndisallowed-methods = [\n{methods}]\ndisallowed-types = [\n{types}]\n")
    }

    #[test]
    fn clippy_minimum_lists_are_the_c09_lists() {
        // Pins the mirror itself: an emptied or truncated constant would make
        // every configuration check below pass vacuously.
        assert_eq!(MIN_DISALLOWED_METHODS.len(), 14);
        assert_eq!(MIN_DISALLOWED_TYPES, ["std::process::Command"]);
        assert_clean(&check_clippy_toml(&full_clippy_toml("")));
    }

    #[test]
    fn flags_clippy_toml_missing_an_entry() {
        assert_flags(
            &check_clippy_toml(&full_clippy_toml("std::fs::remove_file")),
            RULE_CLIPPY_ENTRY,
        );
        assert_flags(
            &check_clippy_toml(&full_clippy_toml("std::process::Command")),
            RULE_CLIPPY_ENTRY,
        );
        // The path must be an entry, not merely text inside a reason string.
        let hidden = full_clippy_toml("std::fs::rename")
            .replace("reason = \"C-09\"", "reason = \"see std::fs::rename\"");
        assert_flags(&check_clippy_toml(&hidden), RULE_CLIPPY_ENTRY);
        assert_flags(&check_clippy_toml(""), RULE_CLIPPY_ENTRY);
        assert_flags(&check_clippy_toml("disallowed-methods = ["), RULE_TOML);
    }

    #[test]
    fn flags_second_or_missing_clippy_config() {
        let one = |s: &[&str]| s.iter().map(ToString::to_string).collect::<Vec<_>>();
        assert_clean(&check_clippy_config_paths(&one(&["clippy.toml"])));
        assert_flags(
            &check_clippy_config_paths(&one(&[
                "clippy.toml",
                "crates/fuzzy-dedupe-app/clippy.toml",
            ])),
            RULE_CLIPPY_FILE,
        );
        assert_flags(
            &check_clippy_config_paths(&one(&["clippy.toml", ".clippy.toml"])),
            RULE_CLIPPY_FILE,
        );
        assert_flags(&check_clippy_config_paths(&[]), RULE_CLIPPY_FILE);
        assert!(is_clippy_config("crates/fuzzy-dedupe-app/.clippy.toml"));
        assert!(!is_clippy_config("docs/clippy.toml.md"));
    }

    // ── Manifests and lockfile ─────────────────────────────────────────────

    #[test]
    fn flags_manifest_that_lowers_either_lint() {
        for (path, toml) in [
            (
                "Cargo.toml",
                "[workspace.lints.clippy]\ndisallowed_methods = \"allow\"\n",
            ),
            (
                "Cargo.toml",
                "[workspace.lints.clippy]\npedantic = { level = \"warn\", priority = -1 }\ndisallowed-types = { level = \"allow\", priority = 1 }\n",
            ),
            (
                "crates/x/Cargo.toml",
                "[lints.clippy]\ndisallowed_types = \"allow\"\n",
            ),
            (
                "crates/x/Cargo.toml",
                "[lints]\nclippy = { disallowed_methods = \"expect\" }\n",
            ),
            (
                "Cargo.toml",
                "[workspace.lints.clippy]\nstyle = \"allow\"\n",
            ),
        ] {
            assert_flags(&check_manifest(path, toml), RULE_MANIFEST_LINT);
        }
        let fine = "[workspace.lints.clippy]\n# disallowed_methods = \"allow\" is forbidden\npedantic = { level = \"warn\", priority = -1 }\ndisallowed_methods = \"deny\"\n";
        assert_clean(&check_manifest("Cargo.toml", fine));
    }

    #[test]
    fn flags_trash_in_manifest() {
        for toml in [
            "[dependencies]\ntrash = \"5\"\n",
            "[dev-dependencies]\nbin = { package = \"trash\", version = \"5\" }\n",
            "[target.'cfg(unix)'.dependencies.trash]\nversion = \"5\"\n",
            "[workspace.dependencies]\nTrash-RS = \"1\"\n",
        ] {
            assert_flags(&check_manifest("crates/x/Cargo.toml", toml), RULE_TRASH);
        }
        let fine = "[package]\nname = \"x\"\n# no trash crate here\n[dependencies]\nfuzzy-dedupe-domain = { path = \"../d\" }\n";
        assert_clean(&check_manifest("crates/x/Cargo.toml", fine));
    }

    #[test]
    fn flags_trash_in_lockfile() {
        let lock = "version = 4\n\n[[package]]\nname = \"fuzzy-dedupe-app\"\nversion = \"0.1.0\"\ndependencies = [\n \"trash\",\n]\n\n[[package]]\nname = \"trash\"\nversion = \"5.2.2\"\n";
        assert_flags(&check_lockfile(lock), RULE_TRASH);
        let clean =
            "version = 4\n\n[[package]]\nname = \"fuzzy-dedupe-app\"\nversion = \"0.1.0\"\n";
        assert_clean(&check_lockfile(clean));
    }

    #[test]
    fn flags_reuse_annotation_matching_clippy_toml() {
        for glob in [
            "clippy.toml",
            "*.toml",
            "**",
            "**/*.toml",
            "clippy.*",
            "clip?y.toml",
        ] {
            let reuse = format!(
                "version = 1\n[[annotations]]\npath = [\n  \"README.md\", # x\n  \"{glob}\",\n]\n"
            );
            assert_flags(&check_reuse(&reuse), RULE_REUSE);
        }
        let fine = "version = 1\n[[annotations]]\npath = \"crates/fuzzy-dedupe-app/assets/fonts/*.ttf\"\n[[annotations]]\npath = [\"docs/**/*.md\", \".clippy.toml.bak\"]\n";
        assert_clean(&check_reuse(fine));
    }

    // ── Round 2 review findings ────────────────────────────────────────────

    #[test]
    fn flags_crate_root_aliases() {
        for src in [
            "use std as s;",
            "use ::std as s;",
            "use core as c;",
            "extern crate std as s;",
            "use std::{self as s};",
            "use std::os::unix::fs as ufs;",
            "use std::os::unix::fs::symlink;",
        ] {
            assert_flags(&scan_rust(APP_MAIN, src), RULE_USE_TREE);
        }
    }

    #[test]
    fn flags_root_alias_route_under_an_allowlisted_expect() {
        // The review's bypass: the item-level `expect` in log_retention.rs
        // suppresses every disallowed method, so the alias chain itself must
        // be the finding.
        let src = "use std as s;\nuse s::fs::remove_dir_all as erase;\n#[expect(clippy::disallowed_methods, reason = \"x\")]\npub fn f(p: &std::path::Path) { let _ = erase(p); }";
        let found = scan_rust(LOG_RETENTION, src);
        assert_flags(&found, RULE_USE_TREE);
        assert!(
            found.iter().filter(|f| f.rule == RULE_USE_TREE).count() >= 2,
            "{found:#?}"
        );
    }

    #[test]
    fn flags_macro_rules_only_in_allowlisted_files() {
        let src = "macro_rules! m { () => {}; }";
        for path in [APP_FILES, LOG_RETENTION, OPEN_RS] {
            assert_flags(&scan_rust(path, src), RULE_MACRO);
        }
        assert_clean(&scan_rust(APP_MAIN, src));
    }

    #[test]
    fn flags_other_entries_in_each_allowlisted_file() {
        let m = "#[expect(clippy::disallowed_methods, reason = \"x\")]\n";
        let t = "#[expect(clippy::disallowed_types, reason = \"x\")]\n";
        for (path, src) in [
            (
                APP_FILES,
                format!("{m}pub fn f() {{ let _ = std::fs::remove_dir(\"x\"); }}"),
            ),
            (
                LOG_RETENTION,
                format!("{m}pub fn f() {{ let _ = std::fs::write(\"x\", b\"\"); }}"),
            ),
            (
                OPEN_RS,
                format!("{t}pub fn f() {{ let _ = std::fs::remove_file(\"x\"); }}"),
            ),
        ] {
            assert_flags(&scan_rust(path, &src), RULE_QUALIFIED);
        }
    }

    #[test]
    fn flags_symlink_in_scoped_tree() {
        let e = |rel: &str, kind| Entry {
            rel: rel.to_owned(),
            kind,
        };
        assert_flags(
            &check_scoped_entries(&[
                e("crates/x/src/lib.rs", EntryKind::File),
                e("crates/x/src/evil.rs", EntryKind::Symlink),
            ]),
            RULE_SYMLINK,
        );
        assert_flags(
            &check_scoped_entries(&[e("crates/x/tests/linked", EntryKind::Symlink)]),
            RULE_SYMLINK,
        );
        assert_clean(&check_scoped_entries(&[
            e("crates/x/src/lib.rs", EntryKind::File),
            e("crates/x/src/node_modules", EntryKind::Dir),
            e("crates/x/src/fifo", EntryKind::Other),
        ]));
    }

    #[test]
    fn repo_walk_skips_only_root_caches_and_foreign_checkouts() {
        assert!(!repo_descend("node_modules", false));
        assert!(!repo_descend("target", false));
        assert!(!repo_descend(".git", false));
        assert!(!repo_descend(".claude/worktrees/t3", true));
        assert!(repo_descend("crates/x/node_modules", false));
        assert!(repo_descend("crates/x/target", false));
        assert!(repo_descend("docs/target", false));
        assert!(repo_descend("crates/x/vendored", true));
    }

    fn sources(files: &[(&str, &str)]) -> std::collections::BTreeMap<String, String> {
        files
            .iter()
            .map(|(p, s)| ((*p).to_owned(), (*s).to_owned()))
            .collect()
    }

    #[test]
    fn flags_module_not_in_scanned_set() {
        let tree = module_tree(&sources(&[("crates/x/src/lib.rs", "mod a;\nmod b;")]));
        assert_flags(&tree.findings, RULE_MODULE);
        let tree = module_tree(&sources(&[
            (
                "crates/x/src/lib.rs",
                "pub mod a;\nmod b;\nmod outer { mod inner; }",
            ),
            ("crates/x/src/a.rs", "mod c;"),
            ("crates/x/src/a/c.rs", ""),
            ("crates/x/src/b/mod.rs", ""),
            ("crates/x/src/outer/inner.rs", ""),
            ("crates/x/tests/t.rs", "mod common;"),
            ("crates/x/tests/common/mod.rs", ""),
        ]));
        assert_clean(&tree.findings);
        // The per-file scan reports `#[path]` (`flags_every_path_attribute`).
        assert_flags(
            &scan_rust(
                "crates/x/src/lib.rs",
                "#[path = \"../../../elsewhere.rs\"]\nmod a;",
            ),
            RULE_MODULE,
        );
        assert_flags(
            &scan_rust(APP_MAIN, "include!(\"elsewhere.inc\");"),
            RULE_MODULE,
        );
    }

    #[test]
    fn clippy_entry_paths_respect_string_boundaries() {
        // `reason` precedes `path`, so a substring search for `path` lands
        // inside the reason string and reads the decoy as the entry.
        let decoy = |entry: &str| {
            full_clippy_toml("std::fs::remove_file").replace(
                "disallowed-methods = [\n",
                &format!("disallowed-methods = [\n  {entry},\n"),
            )
        };
        for entry in [
            "{ reason = \"path = 'std::fs::remove_file'\", path = \"std::fs::read\" }",
            "{ reason = 'path = \"std::fs::remove_file\"', path = \"std::fs::read\" }",
            "{ reason = \"x\", other_path = \"std::fs::remove_file\", path = \"std::fs::read\" }",
        ] {
            assert_flags(&check_clippy_toml(&decoy(entry)), RULE_CLIPPY_ENTRY);
        }
        let literal = full_clippy_toml("std::fs::remove_file").replace(
            "disallowed-methods = [\n",
            "disallowed-methods = [\n  { reason = 'x', path = 'std::fs::remove_file' },\n",
        );
        assert_clean(&check_clippy_toml(&literal));
        let bare = full_clippy_toml("std::fs::remove_file").replace(
            "disallowed-methods = [\n",
            "disallowed-methods = [\n  \"std::fs::remove_file\",\n",
        );
        assert_clean(&check_clippy_toml(&bare));
    }

    #[test]
    fn flags_manifest_lowering_in_any_toml_spelling() {
        for toml in [
            "[workspace.lints.clippy]\ndisallowed_methods = 'allow'\n",
            "[workspace.lints.clippy.disallowed_methods]\nlevel = \"allow\"\npriority = 1\n",
            "[lints.clippy.disallowed_types]\nlevel = 'expect'\n",
            "[lints.clippy]\n\"disallowed-methods\" = \"allow\"\n",
            "[lints]\nclippy.all = \"allow\"\n",
            "[lints]\nclippy = { style = { level = \"allow\", priority = 2 } }\n",
            "[lints.rust]\nwarnings = \"allow\"\n",
            "[workspace]\nlints.clippy.disallowed_types = \"allow\"\n",
        ] {
            assert_flags(
                &check_manifest("crates/x/Cargo.toml", toml),
                RULE_MANIFEST_LINT,
            );
        }
        let fine = "[lints.clippy.disallowed_methods]\nlevel = \"deny\"\n[lints.clippy]\npedantic = { level = 'warn', priority = -1 }\n";
        assert_clean(&check_manifest("crates/x/Cargo.toml", fine));
    }

    #[test]
    fn extra_mutation_routes_are_required_in_clippy_toml() {
        assert!(EXTRA_DISALLOWED_METHODS.contains(&"std::os::unix::fs::symlink"));
        assert!(EXTRA_DISALLOWED_METHODS.contains(&"std::fs::File::set_permissions"));
        for e in EXTRA_DISALLOWED_METHODS {
            assert_flags(&check_clippy_toml(&full_clippy_toml(e)), RULE_CLIPPY_ENTRY);
        }
    }

    #[test]
    fn flags_extra_mutation_routes_textually() {
        for src in [
            "fn f() { let _ = std::os::unix::fs::symlink(\"a\", \"b\"); }",
            "fn f() { let _ = std::os::windows::fs::symlink_file(\"a\", \"b\"); }",
            "fn f() { let _ = std::os::windows::fs::symlink_dir(\"a\", \"b\"); }",
            "fn f() { let _ = std::fs::DirBuilder::new().create(\"x\"); }",
            "fn f(x: &std::fs::File) { let _ = x.set_permissions(p()); }",
            "fn f(x: &std::fs::File) { let _ = x.set_len(0); }",
            "fn f(x: &std::fs::File) { let _ = x.set_times(t()); }",
            "fn f(x: &std::fs::File) { let _ = x.set_modified(t()); }",
            "fn f() { let _ = std::fs::File::options(); }",
        ] {
            let found = scan_rust(APP_MAIN, src);
            assert!(
                found
                    .iter()
                    .any(|f| f.rule == RULE_QUALIFIED || f.rule == RULE_METHOD),
                "{src}: {found:#?}"
            );
        }
    }

    #[test]
    fn flags_ffi_in_scoped_source() {
        for (path, src) in [
            (
                APP_MAIN,
                "unsafe extern \"C\" { fn unlink(p: *const u8) -> i32; }",
            ),
            (APP_MAIN, "extern { fn unlink(p: *const u8) -> i32; }"),
            (APP_MAIN, "#[link(name = \"c\")]\nunsafe extern \"C\" {}"),
            (
                TEST_FILE,
                "unsafe extern \"C\" { fn unlink(p: *const u8) -> i32; }",
            ),
        ] {
            assert_flags(&scan_rust(path, src), RULE_FFI);
        }
        assert_clean(&scan_rust(
            APP_MAIN,
            "pub extern \"C\" fn callback() {}\nextern crate std;",
        ));
    }

    #[test]
    fn test_files_may_not_copy_and_must_use_target_tmpdir() {
        assert_flags(
            &scan_rust(
                TEST_FILE,
                "#[test]\nfn t() { std::fs::copy(\"a\", \"b\").ok(); }",
            ),
            RULE_TEST_NAME,
        );
        let no_tmpdir = "#[test]\n#[expect(clippy::disallowed_methods, reason = \"fixture\")]\nfn t() { std::fs::write(\"/tmp/x\", b\"\").ok(); }";
        assert_flags(&scan_rust(TEST_FILE, no_tmpdir), RULE_TEST_TMPDIR);
    }

    #[test]
    fn raw_identifiers_keep_keywords_distinct() {
        let src = "fn r#use() {}\nfn f() { r#use(); let _ = std::fs::remove_file(\"x\"); }";
        assert_flags(&scan_rust(APP_MAIN, src), RULE_QUALIFIED);
        let src = "fn f() { let _ = std::fs::r#remove_file(\"x\"); }";
        assert_flags(&scan_rust(APP_MAIN, src), RULE_QUALIFIED);
    }

    #[test]
    fn inner_and_out_of_line_cfg_test_are_test_regions() {
        let expect = "#[expect(clippy::disallowed_methods, reason = \"x\")]\nfn t() { let _ = std::fs::remove_file(\"x\"); }";
        let inline = format!("mod tests {{\n    #![cfg(test)]\n    {expect}\n}}");
        assert_flags(&scan_rust(LOG_RETENTION, &inline), RULE_EXPECT_LOCATION);
        let file_level = format!("#![cfg(test)]\n{expect}");
        assert_flags(&scan_rust(LOG_RETENTION, &file_level), RULE_EXPECT_LOCATION);
        assert_flags(
            &scan_rust_with(LOG_RETENTION, expect, &test_ctx(true, &[])),
            RULE_EXPECT_LOCATION,
        );
        assert_clean(&scan_rust_with(
            LOG_RETENTION,
            expect,
            &test_ctx(false, &[]),
        ));

        let tree = module_tree(&sources(&[
            ("crates/x/src/lib.rs", "#[cfg(test)]\nmod tests;\nmod a;"),
            ("crates/x/src/tests.rs", "mod deep;"),
            ("crates/x/src/tests/deep.rs", ""),
            ("crates/x/src/a.rs", "#[cfg(test)]\nmod t;"),
            ("crates/x/src/a/t/mod.rs", ""),
        ]));
        assert_clean(&tree.findings);
        let want: BTreeSet<String> = [
            "crates/x/src/tests.rs",
            "crates/x/src/tests/deep.rs",
            "crates/x/src/a/t/mod.rs",
        ]
        .iter()
        .map(ToString::to_string)
        .collect();
        assert_eq!(tree.test_files, want);
    }

    // ── Round 3 review items ───────────────────────────────────────────────

    fn test_ctx(file_is_test: bool, macros: &[&str]) -> ScanCtx<'static> {
        let set: BTreeSet<String> = macros.iter().map(ToString::to_string).collect();
        ScanCtx {
            file_is_test,
            workspace_macros: Box::leak(Box::new(set)),
        }
    }

    #[test]
    fn test_files_accept_io_copy_and_local_copy() {
        // N4: only filesystem copy forms are banned in test files.
        let src = "use std::io::copy;\n#[test]\nfn t() {\n    let copy = 1;\n    let _ = copy;\n    let mut r: &[u8] = b\"\";\n    let _ = std::io::copy(&mut r, &mut std::io::sink());\n    let _ = io::copy(&mut r, &mut std::io::sink());\n    x.copy();\n}";
        assert_clean(&scan_rust(TEST_FILE, src));
        for bad in [
            "#[test]\nfn t() { fs::copy(\"a\", \"b\").ok(); }",
            "use std::fs::copy;",
            "use std::fs::{copy as c};",
        ] {
            assert_flags(&scan_rust(TEST_FILE, bad), RULE_TEST_NAME);
        }
    }

    #[test]
    fn os_aliases_flag_only_fs_and_process_routes() {
        // N6: read-only std::os extension imports must pass.
        let fine = "use std::os::unix::ffi::OsStrExt as _;\nuse std::os::unix::ffi::OsStringExt;\nuse std::os::unix::ffi;\nuse std::ffi::OsStr as O;\nuse std::os::unix::ffi::*;";
        assert_clean(&scan_rust(APP_MAIN, fine));
        for bad in [
            "use std::os as o;",
            "use std::os::unix as u;",
            "use std::os::unix::*;",
            "use std::os::*;",
            "use std::*;",
            "use std::os::unix::process as p;",
            "use std::os::unix::fs::{self as f};",
        ] {
            assert_flags(&scan_rust(APP_MAIN, bad), RULE_USE_TREE);
        }
    }

    #[test]
    fn only_cfg_requiring_test_marks_test_code() {
        // N7: not(test) and any(test, …) do not require test.
        let body = "#[expect(clippy::disallowed_methods, reason = \"x\")]\npub fn f(p: &std::path::Path) { let _ = std::fs::remove_file(p); }";
        assert_clean(&scan_rust(
            LOG_RETENTION,
            &format!("#![cfg(not(test))]\n{body}"),
        ));
        assert_clean(&scan_rust(
            LOG_RETENTION,
            &format!("mod m {{\n    #![cfg(any(test, unix))]\n    {body}\n}}"),
        ));
        assert_clean(&scan_rust(
            LOG_RETENTION,
            &format!("#[cfg(not(test))]\nmod m {{\n    {body}\n}}"),
        ));
        assert_flags(
            &scan_rust(
                LOG_RETENTION,
                &format!("#[cfg(all(unix, test))]\nmod m {{\n    {body}\n}}"),
            ),
            RULE_EXPECT_LOCATION,
        );
        assert_flags(
            &scan_rust(
                LOG_RETENTION,
                &format!("#[cfg(any(test, all(test, unix)))]\nmod m {{\n    {body}\n}}"),
            ),
            RULE_EXPECT_LOCATION,
        );
        // An integration test that opens with `#![cfg(test)]` keeps the
        // integration-test allowance.
        let itest = "#![cfg(test)]\n#[test]\n#[expect(clippy::disallowed_methods, reason = \"fixture\")]\nfn t() { std::fs::write(concat!(env!(\"CARGO_TARGET_TMPDIR\"), \"/x\"), b\"\").ok(); }";
        assert_clean(&scan_rust(TEST_FILE, itest));
    }

    #[test]
    fn raw_keyword_module_resolves_to_plain_file() {
        // N5
        let tree = module_tree(&sources(&[
            ("crates/x/src/lib.rs", "mod r#type;"),
            ("crates/x/src/type.rs", ""),
        ]));
        assert_clean(&tree.findings);
    }

    #[test]
    fn workspace_members_match_crate_dirs() {
        // F2
        let dirs = |d: &[&str]| d.iter().map(ToString::to_string).collect::<Vec<_>>();
        let manifest = "[workspace]\nmembers = [\n    \"crates/a\",\n    \"crates/b\",\n]\n";
        assert_clean(&check_members(manifest, &dirs(&["crates/a", "crates/b"])));
        assert_flags(&check_members(manifest, &dirs(&["crates/a"])), RULE_MEMBERS);
        assert_flags(
            &check_members(manifest, &dirs(&["crates/a", "crates/b", "crates/c"])),
            RULE_MEMBERS,
        );
        assert_clean(&check_members(
            "[workspace]\nmembers = [\"crates/*\"]\n",
            &dirs(&["crates/a", "crates/z"]),
        ));
        assert_flags(
            &check_members("[package]\nname = \"x\"\n", &dirs(&["crates/a"])),
            RULE_MEMBERS,
        );
    }

    #[test]
    fn escapes_and_multiline_strings_fail_closed() {
        // F3, F4
        let esc = full_clippy_toml("").replacen("reason = \"C-09\"", "reason = \"a \\\"b\\\"\"", 1);
        assert_flags(&check_clippy_toml(&esc), RULE_TOML_ESCAPE);
        let multi = full_clippy_toml("").replacen("reason = \"C-09\"", "reason = \"\"\"x\"\"\"", 1);
        assert_flags(&check_clippy_toml(&multi), RULE_TOML_ESCAPE);
        let path_esc =
            full_clippy_toml("").replacen("std::fs::remove_file", "std::fs::remove\\u005ffile", 1);
        assert_flags(&check_clippy_toml(&path_esc), RULE_TOML_ESCAPE);
        for toml in [
            "[lints.clippy]\n\"disallowed\\u005fmethods\" = \"allow\"\n",
            "[lints.clippy]\ndisallowed_methods = \"\"\"allow\"\"\"\n",
            "[workspace.lints.clippy]\nstyle = \"\\u0061llow\"\n",
        ] {
            assert_flags(
                &check_manifest("crates/x/Cargo.toml", toml),
                RULE_TOML_ESCAPE,
            );
        }
        assert_clean(&check_manifest(
            "crates/x/Cargo.toml",
            "[package]\ndescription = \"a \\\"b\\\"\"\n",
        ));
    }

    #[test]
    fn target_tmpdir_must_be_in_the_allowed_item() {
        // F6
        let in_reason = "#[test]\n#[expect(clippy::disallowed_methods, reason = \"under CARGO_TARGET_TMPDIR\")]\nfn t() { std::fs::write(\"/tmp/x\", b\"\").ok(); }";
        assert_flags(&scan_rust(TEST_FILE, in_reason), RULE_TEST_TMPDIR);
        let elsewhere = "const D: &str = env!(\"CARGO_TARGET_TMPDIR\");\n#[test]\n#[expect(clippy::disallowed_methods, reason = \"x\")]\nfn t() { std::fs::write(\"/tmp/x\", b\"\").ok(); }";
        assert_flags(&scan_rust(TEST_FILE, elsewhere), RULE_TEST_TMPDIR);
    }

    #[test]
    fn flags_conditional_cfg_and_path_in_cfg_attr() {
        // F8
        assert_flags(
            &scan_rust(APP_MAIN, "#[cfg_attr(unix, cfg(test))]\nmod t {}"),
            RULE_CFG_ATTR,
        );
        assert_flags(
            &scan_rust(APP_MAIN, "#[cfg_attr(unix, path = \"x.rs\")]\nmod a;"),
            RULE_CFG_ATTR,
        );
        assert_clean(&scan_rust(
            APP_MAIN,
            "#[cfg_attr(test, derive(Debug))]\nstruct S;",
        ));
    }

    #[test]
    fn flags_macro_built_guarded_paths() {
        // F9, N3
        assert_flags(
            &scan_rust(
                APP_MAIN,
                "#[cfg(windows)]\nmacro_rules! m { ($f:ident) => { std::fs::$f(\"x\") }; }",
            ),
            RULE_MACRO_PATH,
        );
        assert_flags(
            &scan_rust(
                APP_MAIN,
                "macro_rules! m { ($t:ident) => { process::$t::new(\"x\") }; }",
            ),
            RULE_MACRO_PATH,
        );
        let call = "#[expect(clippy::disallowed_methods, reason = \"x\")]\npub fn f() { m!(remove_dir_all); }";
        assert_flags(
            &scan_rust_with(LOG_RETENTION, call, &test_ctx(false, &["m"])),
            RULE_MACRO,
        );
        let std_macros = "pub fn f() -> Vec<u8> { println!(\"x\"); assert!(1 != 2); vec![] }";
        assert_clean(&scan_rust_with(
            LOG_RETENTION,
            std_macros,
            &test_ctx(false, &["m"]),
        ));
    }

    #[test]
    fn flags_every_path_attribute() {
        // N1
        assert_flags(
            &scan_rust(APP_MAIN, "#[path = \"x.rs\"]\nmod a {}"),
            RULE_MODULE,
        );
        assert_flags(
            &scan_rust(APP_MAIN, "#[path = \"x.rs\"]\nmod a;"),
            RULE_MODULE,
        );
    }

    #[test]
    fn flags_type_aliases_through_guarded_modules() {
        // N2
        assert_flags(
            &scan_rust(APP_MAIN, "type Handle = std::fs::File;"),
            RULE_TYPE_ALIAS,
        );
        assert_flags(
            &scan_rust(APP_MAIN, "pub type P = process::Child;"),
            RULE_TYPE_ALIAS,
        );
        assert_clean(&scan_rust(
            APP_MAIN,
            "type R = std::io::Result<()>;\ntrait T { type Item; }",
        ));
        assert_flags(
            &scan_rust(APP_FILES, "use std::fs::File as F;"),
            RULE_USE_TREE,
        );
    }
}
