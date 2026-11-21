# Project Status

## Current snapshot

- Fuzzy DeDupe targets fuzzy text-content duplicate detection; the README currently provides only a one-line project description.
- The specification records a from-scratch Rust desktop application under `GPL-3.0-only`, with no editing or AI similarity.
- V1 is plain text only and Linux only, built behind platform adapters so macOS and Windows can follow (spec v0.5, C-013, C-014).
- Approved UI direction uses Findings and Compare workspaces with supporting popups; the draft reports no completed executable or validated safety/performance results.
- egui/eframe is the selected UI framework ([ADR 0001](adr/adr-0001-use-egui-for-the-v1-desktop-ui.md)), final once the MS-1 prototype passes; Slint is the fallback.
- Assistive-technology support is a non-goal (NG-003). Storage, matching algorithms, and Linux package formats remain open.
- Catalog 5 is configured with seven packages under Project Standards 5.29.0.
- Codex has project-scoped Figma and Context7 MCP servers in `.codex/config.toml` and 17 vendored `figma-*` skills; see C1 in [conventions](handoff/conventions.md).
- Rust 1.96.0 is pinned in `rust-toolchain.toml`; `.rexec.toml` pins nextest, deny, and audit binaries. No Cargo workspace exists yet.
- rexec cannot converge a declared `rust` tool (L3DigitalNet/rexec#26), so the worker uses its own rustup.
- The [specification register](handoff/specs-plans.md) points to the Standard-profile product and architecture draft, SPEC-FZDP.
