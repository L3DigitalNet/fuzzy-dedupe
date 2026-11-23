<!--
SPDX-FileCopyrightText: 2026 L3Digital LLC
SPDX-License-Identifier: GPL-3.0-only
-->

# Symbolic-link fixtures

These committed symbolic links let the platform tests prove the no-follow checks of the app-owned writer and the log expiry (owner policy "trust the root, no links below"). A regular file standing in for a link cannot tell `symlink_metadata` from `metadata`, and no test may create a link itself, because the C-09 rules bar link creation in test code. The links live here, outside `tests/`, because the T3 guard in `crates/fuzzy-dedupe-app/tests/no_file_mutation.rs` rejects any symbolic link inside the scanned source trees `crates/*/src` and `crates/*/tests`. Every link is relative, stays inside this directory and points at a tracked file or at a directory holding one, so it resolves in a fresh clone. Each test first asserts that its fixture entry is still a link, so a checkout or sync that dereferenced the links fails loudly instead of passing vacuously.
