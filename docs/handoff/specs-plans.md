# Specifications and Plans

## Active artifacts

| Artifact | Status | Purpose |
| --- | --- | --- |
| [Fuzzy DeDupe specification](../specs/canonical/fuzzy-dedupe.md) | SPEC-FZDP, Standard draft v0.6 | Current requirements, compact UI handoff, proposed Rust architecture, acceptance criteria, and open decisions. |

- Confirmed in the draft: Fuzzy DeDupe name, from-scratch Rust implementation, `GPL-3.0-only`, compact Findings/Compare UI, and a plain-text-only, Linux-only V1 with portable platform adapters.
- Editing and semantic/AI similarity are excluded; additional media belong to later releases.
- [ADR 0001](../adr/adr-0001-use-egui-for-the-v1-desktop-ui.md) selects egui/eframe, pending the MS-1 prototype. SQLite and the matching algorithms still need validation.
- The draft records no completed executable, deletion-safety validation, or performance benchmarks; its earlier research was not revalidated here.
