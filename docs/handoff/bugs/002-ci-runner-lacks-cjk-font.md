# 002: CI runner lacks a CJK font

## Cause

The GitHub `ubuntu-24.04` runner has only IBM Plex Sans, Noto Emoji and DejaVu Sans. A kittest harness that renders the `unicode` scenario through `SystemFontLookup` panics there ("No glyph for 非") under kittest's default missing-glyph policy.

The rexec worker and the workstation have Noto Sans CJK, so the same test passed through rexec and failed in CI (PR #56, T39).

## Fix

Such harnesses call `.allow_missing_glyphs()`. Glyph coverage is proved separately on the workstation (EV-001).

## Lesson

A rexec-green test that depends on system fonts is not CI-green. Check font-dependent tests against the CI image.
