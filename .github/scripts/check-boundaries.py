#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 L3Digital LLC
# SPDX-License-Identifier: GPL-3.0-only
"""Enforce the workspace boundaries of plan D-011 against the C-7 policy.

Usage: python3 .github/scripts/check-boundaries.py [--policy PATH]

Checks the Cargo workspace and its tracked Rust sources against the policy
(default `.github/boundaries.toml` at the repository root) and prints one line
per violation, each beginning with its rule ID and naming the crate and file:

  R1  a workspace edge, of any dependency kind, absent from the policy's C-1
      edge table for the declaring crate;
  R2  a direct third-party dependency of any kind (normal, dev, build or
      target-specific) that the policy's third-party table does not approve
      for the declaring crate;
  R3  a crate from the policy's GUI list declared outside fuzzy-dedupe-app;
  R4  a crate from the policy's OS list, or any target-specific dependency,
      declared outside fuzzy-dedupe-platform;
  R5  OS-specific source outside fuzzy-dedupe-platform (sub-rules 5a-5e at
      `scan_source`).

Exit status: 0 when clean, 1 when any rule fails, 2 when the check itself
cannot run (cargo or git failed, or the policy is malformed). A status-2 line
never begins with a rule ID, so it is never mistaken for a rule verdict.

Requirements: Python 3.11 or newer with only the standard library, because
`tomllib` arrived in 3.11 and the CI job must add no dependency; `cargo` on
PATH, for `cargo metadata --no-deps --format-version 1`, which resolves
nothing, needs no network and never rewrites Cargo.lock; `git` on PATH and a
checkout, because the source scan covers exactly `git ls-files '*.rs'`.

Manifest rules (R1-R4) read Cargo's own metadata rather than Cargo.toml text,
so dotted tables, `workspace = true` inheritance and `package =` aliases are
all seen as Cargo sees them. Rejected: grepping Cargo.toml, which misses
aliases and dotted tables; cargo-deny `[bans]` wrappers, which also govern
transitive edges.

Known residue of the lexical R5 scan, which has no macro expansion and no
name resolution (plan D-011):
  - a macro that assembles an OS path from separate tokens escapes the scan;
    the portability job covers that for domain and platform, review covers
    the other six crates;
  - an OS item that fuzzy-dedupe-platform re-exports through its own public
    API (`pub use`, or a public signature) and that a dependent crate then
    uses is invisible here; review of platform's public API covers it.
"""

from __future__ import annotations

import argparse
import fnmatch
import json
import subprocess
import sys
import tomllib
from dataclasses import dataclass
from pathlib import Path
from typing import Any

# Placement owners are fixed by D-011 rather than read from the policy, so a
# policy edit (or a canary fixture) can approve a crate but never relocate
# where GUI and OS code may live.
APP = "fuzzy-dedupe-app"
PLATFORM = "fuzzy-dedupe-platform"

# D-011's minimum lists. The policy may extend them but may not drop an
# entry: a policy that silently stopped listing `libc` would turn R4 off for
# the most common OS crate while every run still reported clean.
REQUIRED_GUI = frozenset({"egui", "eframe", "egui_*", "winit"})
REQUIRED_OS = frozenset(
    {
        "libc",
        "nix",
        "rustix",
        "windows",
        "windows-sys",
        "winapi",
        "core-foundation",
        "objc2",
    }
)

# 5a: cfg predicate names that make compilation OS-conditional.
OS_CFG_NAMES = frozenset(
    {"target_os", "target_family", "target_vendor", "target_env", "unix", "windows"}
)
STD_ROOTS = frozenset({"std", "core"})


class CheckError(Exception):
    """The check could not run; reported with exit status 2."""


@dataclass(frozen=True)
class Policy:
    edges: dict[str, frozenset[str]]
    third_party: dict[str, frozenset[str]]
    gui: tuple[str, ...]
    os: tuple[str, ...]


@dataclass(frozen=True)
class Failure:
    rule: str
    crate: str
    file: str
    detail: str

    def render(self) -> str:
        return f"{self.rule} {self.crate} {self.file}: {self.detail}"


def _string_list(value: Any, where: str) -> tuple[str, ...]:
    if not isinstance(value, list) or not all(isinstance(v, str) for v in value):
        raise CheckError(f"policy: {where} must be a list of strings")
    return tuple(value)


def load_policy(path: Path) -> Policy:
    try:
        with path.open("rb") as handle:
            data = tomllib.load(handle)
    except OSError as exc:
        raise CheckError(f"cannot read policy {path}: {exc}") from exc
    except tomllib.TOMLDecodeError as exc:
        raise CheckError(f"policy {path} is not valid TOML: {exc}") from exc

    def table(key: str) -> dict[str, Any]:
        value = data.get(key)
        if not isinstance(value, dict):
            raise CheckError(f"policy {path}: missing [{key}] table")
        return value

    edges = {
        crate: frozenset(_string_list(deps, f"[edges].{crate}"))
        for crate, deps in table("edges").items()
    }
    third_party = {
        name: frozenset(_string_list(crates, f"[third-party].{name}"))
        for name, crates in table("third-party").items()
    }
    gui = _string_list(table("gui").get("crates"), "[gui].crates")
    os_list = _string_list(table("os").get("crates"), "[os].crates")
    for label, listed, required in (
        ("[gui].crates", gui, REQUIRED_GUI),
        ("[os].crates", os_list, REQUIRED_OS),
    ):
        missing = sorted(required.difference(listed))
        if missing:
            raise CheckError(
                f"policy {path}: {label} omits D-011 minimum entries: "
                + ", ".join(missing)
            )
    return Policy(edges=edges, third_party=third_party, gui=gui, os=os_list)


def _run(args: list[str], cwd: Path) -> str:
    try:
        done = subprocess.run(
            args, cwd=cwd, capture_output=True, text=True, check=False
        )
    except OSError as exc:
        raise CheckError(f"cannot run {args[0]}: {exc}") from exc
    if done.returncode != 0:
        raise CheckError(
            f"`{' '.join(args)}` exited {done.returncode}:\n{done.stderr.strip()}"
        )
    return done.stdout


def _matches(name: str, patterns: tuple[str, ...]) -> bool:
    return any(fnmatch.fnmatchcase(name, pattern) for pattern in patterns)


def _describe(dep: dict[str, Any]) -> str:
    """Render a dependency as its table and key, so the message names the alias."""
    kind = dep.get("kind") or "normal"
    parts = [f"{kind} dependency {dep['name']}"]
    if dep.get("rename"):
        parts.append(f"(declared as `{dep['rename']}`)")
    if dep.get("target"):
        parts.append(f"for target `{dep['target']}`")
    return " ".join(parts)


def check_manifests(
    metadata: dict[str, Any], policy: Policy, root: Path
) -> tuple[list[Failure], dict[str, Path]]:
    """Apply R1-R4 and return the failures plus each member's crate directory."""
    member_ids = set(metadata["workspace_members"])
    members = [p for p in metadata["packages"] if p["id"] in member_ids]
    member_names = {p["name"] for p in members}
    crate_dirs: dict[str, Path] = {}
    failures: list[Failure] = []

    for package in sorted(members, key=lambda p: p["name"]):
        crate = package["name"]
        manifest = Path(package["manifest_path"])
        crate_dirs[crate] = manifest.parent
        try:
            shown = manifest.relative_to(root).as_posix()
        except ValueError:
            shown = manifest.as_posix()
        allowed_edges = policy.edges.get(crate, frozenset())

        for dep in package["dependencies"]:
            # `name` is the real package name even under a `package =` alias
            # (the alias is `rename`), so every rule below keys on it. A
            # workspace edge is a sourceless dependency on a member; anything
            # else, a local path crate outside the workspace included, is
            # third-party and needs approval.
            name = dep["name"]
            what = _describe(dep)
            is_edge = name in member_names and dep.get("source") is None
            if is_edge:
                if name not in allowed_edges:
                    failures.append(
                        Failure("R1", crate, shown, f"{what} is not a C-1 edge")
                    )
            elif crate not in policy.third_party.get(name, frozenset()):
                failures.append(
                    Failure("R2", crate, shown, f"{what} is not approved by the policy")
                )
            if crate != APP and _matches(name, policy.gui):
                failures.append(
                    Failure("R3", crate, shown, f"{what} is a GUI crate outside {APP}")
                )
            if crate != PLATFORM:
                if _matches(name, policy.os):
                    failures.append(
                        Failure(
                            "R4",
                            crate,
                            shown,
                            f"{what} is an OS crate outside {PLATFORM}",
                        )
                    )
                elif dep.get("target"):
                    failures.append(
                        Failure(
                            "R4",
                            crate,
                            shown,
                            f"{what} is target-specific outside {PLATFORM}",
                        )
                    )
    return failures, crate_dirs


# ── Rust lexer ──────────────────────────────────────────────────────────────


@dataclass(frozen=True)
class Token:
    kind: str  # "ident", "punct", "literal" or "lifetime"
    text: str  # identifiers carry their name without any `r#` prefix
    line: int
    raw: bool = False  # written as `r#name`, so never a keyword


class LexError(Exception):
    def __init__(self, line: int, message: str) -> None:
        super().__init__(message)
        self.line = line


def _ident_start(ch: str) -> bool:
    return ch == "_" or ch.isalpha()


def _ident_char(ch: str) -> bool:
    return ch == "_" or ch.isalnum()


def tokenize(src: str) -> list[Token]:
    """Tokenize Rust source, dropping comments and collapsing literals.

    Comments (nested block comments included) produce no token, and every
    string, byte-string, C-string, raw-string and char literal becomes one
    opaque `literal` token, so a path or cfg named only inside one can never
    reach a rule. Lifetimes are kept as their own token kind and are never
    mistaken for char literals. Because rules match token sequences, the
    whitespace and line breaks inside a path or `use` tree are irrelevant,
    which is the "collapse whitespace" step of D-011.
    """
    tokens: list[Token] = []
    i, n, line = 0, len(src), 1

    # A leading `#!` that does not open an inner attribute `#![...]` is a
    # shebang line, not code.
    if src.startswith("#!") and not src[2:].lstrip().startswith("["):
        while i < n and src[i] != "\n":
            i += 1

    def skip_quoted(start: int, quote: str) -> int:
        """Return the index after the closing quote, honouring backslashes."""
        nonlocal line
        j = start
        while j < n:
            ch = src[j]
            if ch == "\\":
                # An escaped newline is a line continuation; count it so
                # later findings keep the right line number.
                line += src[j + 1 : j + 2] == "\n"
                j += 2
                continue
            if ch == "\n":
                line += 1
            if ch == quote:
                return j + 1
            j += 1
        raise LexError(line, "unterminated literal")

    def skip_raw_string(start: int) -> int:
        """`start` is at the first `#` or `"` after the r/br/cr prefix."""
        nonlocal line
        hashes = 0
        j = start
        while j < n and src[j] == "#":
            hashes += 1
            j += 1
        if j >= n or src[j] != '"':
            raise LexError(line, "malformed raw string literal")
        closer = '"' + "#" * hashes
        end = src.find(closer, j + 1)
        if end < 0:
            raise LexError(line, "unterminated raw string literal")
        line += src.count("\n", j, end)
        return end + len(closer)

    def skip_char(start: int) -> int:
        """`start` is at the opening `'` of a char or byte-char literal."""
        j = start + 1
        if j < n and src[j] == "\\":
            j += 2
        else:
            j += 1
        while j < n and src[j] not in "'\n":
            j += 1
        if j >= n or src[j] != "'":
            raise LexError(line, "unterminated char literal")
        return j + 1

    while i < n:
        ch = src[i]
        if ch == "\n":
            line += 1
            i += 1
        elif ch.isspace():
            i += 1
        elif src.startswith("//", i):
            while i < n and src[i] != "\n":
                i += 1
        elif src.startswith("/*", i):
            depth, i = 1, i + 2
            while depth:
                if i >= n:
                    raise LexError(line, "unterminated block comment")
                if src.startswith("/*", i):
                    depth, i = depth + 1, i + 2
                elif src.startswith("*/", i):
                    depth, i = depth - 1, i + 2
                else:
                    line += src[i] == "\n"
                    i += 1
        elif _ident_start(ch):
            start = i
            while i < n and _ident_char(src[i]):
                i += 1
            word = src[start:i]
            nxt = src[i] if i < n else ""
            if word in ("r", "br", "cr") and nxt in ('"', "#"):
                # `r#` followed by an identifier is a raw identifier, not a
                # raw string; anything else after the hashes must be a quote.
                if (
                    word == "r"
                    and nxt == "#"
                    and i + 1 < n
                    and _ident_start(src[i + 1])
                ):
                    j = i + 1
                    while j < n and _ident_char(src[j]):
                        j += 1
                    tokens.append(Token("ident", src[i + 1 : j], line, raw=True))
                    i = j
                else:
                    tokens.append(Token("literal", "", line))
                    i = skip_raw_string(i)
            elif word in ("b", "c") and nxt == '"':
                tokens.append(Token("literal", "", line))
                i = skip_quoted(i + 1, '"')
            elif word == "b" and nxt == "'":
                tokens.append(Token("literal", "", line))
                i = skip_char(i)
            else:
                tokens.append(Token("ident", word, line))
        elif ch.isdigit():
            # Numbers matter only so their suffix letters never start an
            # identifier; `1.0` and `x.0.1` both tokenize harmlessly.
            while i < n and _ident_char(src[i]):
                i += 1
            if i + 1 < n and src[i] == "." and src[i + 1].isdigit():
                i += 1
                while i < n and _ident_char(src[i]):
                    i += 1
            tokens.append(Token("literal", "", line))
        elif ch == '"':
            tokens.append(Token("literal", "", line))
            i = skip_quoted(i + 1, '"')
        elif ch == "'":
            # A char literal is `'\…'` or exactly one code point between
            # quotes; anything else (`'a`, `'static`, a loop label) is a
            # lifetime, which D-011 says must not be stripped as a literal.
            if i + 1 < n and (src[i + 1] == "\\" or (i + 2 < n and src[i + 2] == "'")):
                tokens.append(Token("literal", "", line))
                i = skip_char(i)
            else:
                j = i + 1
                if src.startswith("r#", j):
                    j += 2
                while j < n and _ident_char(src[j]):
                    j += 1
                tokens.append(Token("lifetime", src[i:j], line))
                i = j
        elif src.startswith("::", i):
            tokens.append(Token("punct", "::", line))
            i += 2
        else:
            tokens.append(Token("punct", ch, line))
            i += 1
    return tokens


# ── use-tree flattening ─────────────────────────────────────────────────────


@dataclass(frozen=True)
class UsePath:
    segments: tuple[str, ...]  # leading `::` dropped; a glob ends with "*"
    alias: str | None
    line: int

    @property
    def glob(self) -> bool:
        return bool(self.segments) and self.segments[-1] == "*"

    def render(self) -> str:
        text = "::".join(self.segments)
        return f"{text} as {self.alias}" if self.alias else text


class UseParseError(Exception):
    pass


def _is_punct(tok: Token | None, text: str) -> bool:
    return tok is not None and tok.kind == "punct" and tok.text == text


def _is_keyword(tok: Token | None, text: str) -> bool:
    return tok is not None and tok.kind == "ident" and not tok.raw and tok.text == text


class UseParser:
    """Flatten one `use` tree into full paths.

    Grammar (the Rust reference's UseTree):
        tree := [path] "::"? "*" | [path] "::"? "{" [tree ("," tree)* ","?] "}"
              | path ["as" (IDENT | "_")]
    with an optional leading `::` on the outermost tree only. Braces nest to
    any depth, and `{self}` / `{self as x}` flatten to the enclosing prefix,
    so `use std::os::{self as host};` becomes `std::os as host`. A segment
    may be `$ident` (a `macro_rules!` metavariable such as `$crate`); any
    other token where a segment is required is a parse failure, which rule
    5d reports instead of skipping the declaration.
    """

    def __init__(self, tokens: list[Token], pos: int) -> None:
        self.tokens = tokens
        self.pos = pos

    def peek(self, ahead: int = 0) -> Token | None:
        k = self.pos + ahead
        return self.tokens[k] if k < len(self.tokens) else None

    def take(self) -> Token:
        tok = self.peek()
        if tok is None:
            raise UseParseError("use declaration runs to end of file")
        self.pos += 1
        return tok

    def parse_declaration(self) -> list[UsePath]:
        if _is_punct(self.peek(), "::"):
            self.take()
        paths = self.parse_tree(())
        if not _is_punct(self.peek(), ";"):
            raise UseParseError("expected `;` after use tree")
        self.take()
        return paths

    def parse_segment(self) -> str | None:
        tok = self.peek()
        if tok is None:
            return None
        if tok.kind == "ident" and not _is_keyword(tok, "as"):
            self.take()
            return tok.text
        nxt = self.peek(1)
        if _is_punct(tok, "$") and nxt is not None and nxt.kind == "ident":
            self.take()
            self.take()
            return "$" + nxt.text
        return None

    def parse_tree(self, prefix: tuple[str, ...]) -> list[UsePath]:
        line = self.peek().line if self.peek() else 0
        segments = list(prefix)
        while True:
            tok = self.peek()
            if _is_punct(tok, "*"):
                self.take()
                return [UsePath((*segments, "*"), None, line)]
            if _is_punct(tok, "{"):
                self.take()
                paths: list[UsePath] = []
                while not _is_punct(self.peek(), "}"):
                    paths.extend(self.parse_tree(tuple(segments)))
                    if _is_punct(self.peek(), ","):
                        self.take()
                    elif not _is_punct(self.peek(), "}"):
                        raise UseParseError("expected `,` or `}` in use group")
                self.take()
                return paths
            segment = self.parse_segment()
            if segment is None:
                raise UseParseError("expected a path segment, `*` or `{`")
            # `self` after a prefix names the prefix itself (only legal as
            # `{self}`); a leading `self` is the current module and is kept.
            if not (segment == "self" and segments):
                segments.append(segment)
            if _is_punct(self.peek(), "::"):
                self.take()
                continue
            alias = None
            if _is_keyword(self.peek(), "as"):
                self.take()
                target = self.take()
                if target.kind != "ident":
                    raise UseParseError("expected a name after `as`")
                alias = target.text
            return [UsePath(tuple(segments), alias, line)]


# ── R5 source scan ──────────────────────────────────────────────────────────


def _bad_os_segment(segments: tuple[str, ...]) -> bool:
    """True when an `os` segment is not followed by `raw`.

    Keys on the segment itself, whatever precedes it, so it needs no list of
    OS module names and no alias resolution: `std::os`, `s::os`, a bare `os`
    after `use std::*;`, `os::*` and `os::unix::…` all fail, while
    `os::raw` and its children are the one portable subtree. Rejected: matching
    `os::unix` text (nested trees split it) or only `std::os`/`core::os`
    (a chained root alias renames the prefix); see plan D-011.
    """
    for k, segment in enumerate(segments):
        if segment == "os" and (k + 1 == len(segments) or segments[k + 1] != "raw"):
            return True
    return False


def _group(tokens: list[Token], open_at: int) -> tuple[list[Token], int]:
    """Return the tokens inside the delimiter group opening at `open_at`."""
    pairs = {"(": ")", "[": "]", "{": "}"}
    stack = [pairs[tokens[open_at].text]]
    k = open_at + 1
    while k < len(tokens) and stack:
        tok = tokens[k]
        if tok.kind == "punct" and tok.text in pairs:
            stack.append(pairs[tok.text])
        elif tok.kind == "punct" and stack and tok.text == stack[-1]:
            stack.pop()
        k += 1
    return tokens[open_at + 1 : k - 1 if not stack else k], k


def _top_level_prefix(group: list[Token]) -> list[Token]:
    """Tokens before the first top-level comma: the predicate of `cfg_attr`."""
    depth = 0
    for k, tok in enumerate(group):
        if tok.kind == "punct" and tok.text in "([{":
            depth += 1
        elif tok.kind == "punct" and tok.text in ")]}":
            depth -= 1
        elif depth == 0 and _is_punct(tok, ","):
            return group[:k]
    return group


def scan_source(src: str) -> list[tuple[int, str]]:
    """Return (line, detail) for every R5 finding in one Rust source file.

    Sub-rules, all reported as R5:
      5a  a `cfg(…)`, `cfg!(…)` or `cfg_attr(…)` predicate naming
          target_os, target_family, target_vendor, target_env, unix or
          windows;
      5b  a flattened `use` path, or a `::` path anywhere else, with an
          `os` segment not followed by `raw`;
      5c  an alias of the `std` or `core` root (`use std as s`,
          `use ::core as s`, `use std::{self as s}`, `extern crate std as s`);
      5d  a `use` tree that cannot be parsed, which fails closed rather than
          being skipped (an unlexable file fails the same way);
      5e  any `include!` invocation, bare or path-qualified, and any `use`
          whose path ends at `include`, so an alias cannot hide a later
          `inc!`; `include_str!` and `include_bytes!` are distinct names and
          stay allowed.
    Identifiers are compared without their `r#` prefix, so `std::r#os` is an
    `os` segment. Because 5c bans the only rename above `os` and 5b keys on
    the segment, no alias resolution is needed. Known false positive: a
    project item named `os` used in a path; field access `self.os` is not a
    path and does not match.
    """
    try:
        tokens = tokenize(src)
    except LexError as exc:
        return [(exc.line, f"(5d) cannot tokenize this file: {exc}")]

    findings: list[tuple[int, str]] = []
    in_use = [False] * len(tokens)
    count = len(tokens)

    # Pass 1: flatten every `use` declaration and mark its tokens, so the
    # generic path scan below never sees a nested tree such as
    # `std::os::{raw::c_char}` as the unflattened `os::{`.
    k = 0
    while k < count:
        tok = tokens[k]
        nxt = tokens[k + 1] if k + 1 < count else None
        # `use<'a>` is edition-2024 precise capturing in a return type, not
        # an import.
        if _is_keyword(tok, "use") and not _is_punct(nxt, "<"):
            parser = UseParser(tokens, k + 1)
            try:
                paths = parser.parse_declaration()
                end = parser.pos
            except UseParseError as exc:
                findings.append((tok.line, f"(5d) unsupported use form: {exc}"))
                end = k + 1
                while end < count and not _is_punct(tokens[end], ";"):
                    end += 1
                end = min(end + 1, count)
                paths = []
            for m in range(k, end):
                in_use[m] = True
            for path in paths:
                segs = path.segments
                if _bad_os_segment(segs):
                    findings.append(
                        (
                            path.line,
                            f"(5b) use {path.render()} goes through an os module other than os::raw",
                        )
                    )
                if len(segs) == 1 and segs[0] in STD_ROOTS and path.alias is not None:
                    findings.append(
                        (
                            path.line,
                            f"(5c) use {path.render()} aliases the {segs[0]} root",
                        )
                    )
                if not path.glob and segs and segs[-1] == "include":
                    findings.append(
                        (
                            path.line,
                            f"(5e) use {path.render()} imports the include! macro",
                        )
                    )
            k = end
            continue
        k += 1

    # Pass 2: everything outside `use` declarations.
    for k, tok in enumerate(tokens):
        if tok.kind != "ident":
            continue
        prev = tokens[k - 1] if k else None
        nxt = tokens[k + 1] if k + 1 < count else None
        after = tokens[k + 2] if k + 2 < count else None
        name = tok.text

        # 5c, extern crate form: `extern crate std as s;`.
        if (
            _is_keyword(tok, "extern")
            and _is_keyword(nxt, "crate")
            and after is not None
            and after.kind == "ident"
            and after.text in STD_ROOTS
            and _is_keyword(tokens[k + 3] if k + 3 < count else None, "as")
        ):
            findings.append(
                (
                    tok.line,
                    f"(5c) extern crate {after.text} as … aliases the {after.text} root",
                )
            )

        if in_use[k]:
            continue

        # 5a. `cfg!` may use any macro delimiter, so `cfg![unix]` counts too.
        open_at = None
        if name in ("cfg", "cfg_attr") and _is_punct(nxt, "("):
            open_at = k + 1
        elif (
            name == "cfg"
            and _is_punct(nxt, "!")
            and after is not None
            and after.kind == "punct"
            and after.text in ("(", "[", "{")
        ):
            open_at = k + 2
        if open_at is not None:
            group, _ = _group(tokens, open_at)
            predicate = _top_level_prefix(group) if name == "cfg_attr" else group
            named = sorted(
                {
                    t.text
                    for t in predicate
                    if t.kind == "ident" and t.text in OS_CFG_NAMES
                }
            )
            if named:
                findings.append(
                    (tok.line, f"(5a) {name} predicate names {', '.join(named)}")
                )

        # 5b outside `use`: an `os` segment in a `::` path. `os::` must be
        # followed by `raw`; `…::os` that ends the path fails outright. A
        # lone `os` (a local, or `self.os`) is not a `::` path.
        if name == "os":
            if _is_punct(nxt, "::"):
                if not (
                    after is not None and after.kind == "ident" and after.text == "raw"
                ):
                    findings.append(
                        (
                            tok.line,
                            "(5b) path goes through an os module other than os::raw",
                        )
                    )
            elif _is_punct(prev, "::"):
                findings.append((tok.line, "(5b) path ends at an os module"))

        # 5e: `include!`, whatever path precedes it (`std::include!`), with
        # any macro delimiter. Requiring the delimiter keeps a comparison
        # such as `include != x` on a local named `include` from matching.
        if (
            name == "include"
            and _is_punct(nxt, "!")
            and after is not None
            and after.kind == "punct"
            and after.text in ("(", "[", "{")
        ):
            findings.append((tok.line, "(5e) include! of Rust source"))

    findings.sort(key=lambda item: item[0])
    return findings


def check_sources(root: Path, crate_dirs: dict[str, Path]) -> list[Failure]:
    """Apply R5 to every tracked `*.rs` file outside fuzzy-dedupe-platform."""
    listed = _run(["git", "ls-files", "-z", "--", "*.rs"], root)
    # Longest directory first, so a crate nested inside another's directory
    # still claims its own files.
    owners = sorted(
        crate_dirs.items(), key=lambda item: len(item[1].parts), reverse=True
    )
    failures: list[Failure] = []
    for rel in sorted(filter(None, listed.split("\0"))):
        path = root / rel
        crate = next(
            (name for name, directory in owners if path.is_relative_to(directory)),
            "(no workspace crate)",
        )
        if crate == PLATFORM:
            continue
        try:
            src = path.read_text(encoding="utf-8")
        except FileNotFoundError:
            # Tracked but deleted in the working tree: there is no source to
            # build, so there is nothing to confine.
            continue
        except (OSError, UnicodeDecodeError) as exc:
            failures.append(
                Failure("R5", crate, rel, f"(5d) cannot read as UTF-8 source: {exc}")
            )
            continue
        for line, detail in scan_source(src):
            failures.append(Failure("R5", crate, f"{rel}:{line}", detail))
    return failures


def main(argv: list[str] | None = None) -> int:
    # Paths resolve from the script's own location, so the check is the same
    # whatever directory CI or a developer runs it from.
    root = Path(__file__).resolve().parents[2]
    parser = argparse.ArgumentParser(
        description="Enforce the plan D-011 workspace boundaries (rules R1-R5)."
    )
    parser.add_argument(
        "--policy",
        type=Path,
        default=root / ".github" / "boundaries.toml",
        help="boundary policy file (default: .github/boundaries.toml)",
    )
    args = parser.parse_args(argv)

    try:
        policy = load_policy(args.policy)
        metadata = json.loads(
            _run(["cargo", "metadata", "--no-deps", "--format-version", "1"], root)
        )
        failures, crate_dirs = check_manifests(metadata, policy, root)
        failures.extend(check_sources(root, crate_dirs))
    except CheckError as exc:
        print(f"check-boundaries: error: {exc}", file=sys.stderr)
        return 2

    for failure in failures:
        print(failure.render())
    if failures:
        print(f"check-boundaries: {len(failures)} failure(s)", file=sys.stderr)
        return 1
    print("check-boundaries: clean", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
