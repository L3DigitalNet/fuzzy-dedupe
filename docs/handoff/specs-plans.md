# Specifications and Plans

## Active artifacts

| Artifact | Status | Purpose |
| --- | --- | --- |
| [Fuzzy DeDupe specification](../specs/canonical/fuzzy-dedupe.md) | SPEC-FZDP, Standard draft v0.7 | Current requirements, compact UI handoff, proposed Rust architecture, acceptance criteria, and open decisions. |

- Confirmed in the draft: Fuzzy DeDupe name, from-scratch Rust implementation, `GPL-3.0-only`, compact Findings/Compare UI, and a plain-text-only, Linux-only V1 with portable platform adapters.
- Editing and semantic/AI similarity are excluded; additional media belong to later releases.
- [ADR 0001](../adr/adr-0001-use-egui-for-the-v1-desktop-ui.md) commits to egui/eframe (amended 2026-11-22). SQLite and the matching algorithms still need validation.
- v0.7 defines the MS-0 and MS-1 exit lists in §19; its source decisions are in the Basic Memory `knowledge` note `notes/fuzzy-dedupe-v07-decisions-2026-11-21`.
- The draft records no completed executable, deletion-safety validation, or performance benchmarks; its earlier research was not revalidated here.
