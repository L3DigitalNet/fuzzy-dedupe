# Project Status

## Current snapshot

- Fuzzy DeDupe targets fuzzy text-content duplicate detection; the README currently provides only a one-line project description.
- The specification records a from-scratch Rust desktop application under `GPL-3.0-only`, with no editing or AI similarity.
- V1 is plain text only and Linux only, built behind platform adapters so macOS and Windows can follow (spec v0.5, C-013, C-014).
- Approved UI direction uses Findings and Compare workspaces with supporting popups.
- The app runs on fixture scenarios with no real engine; no safety or performance results are validated yet.
- egui/eframe is the committed UI framework ([ADR 0001](adr/adr-0001-use-egui-for-the-v1-desktop-ui.md), amended 2026-11-22); Slint only by explicit owner decision.
- Assistive-technology support is a non-goal (NG-003). Storage, matching algorithms, and Linux package formats remain open.
- Spec v0.13 defines the MS-0 and MS-1 exit lists; KDE Plasma 6.8.3 on Wayland is the reference desktop.
- MS-0 is complete and closed out (PRs #9, #10).
- Latest `main` admission: `dabdf3232be0c54970f39df1173c246956cc2890`; six `main` runs green, 0 unadmitted.
- MS-1 (plan revision 3, 43 tasks): T1 to T24 and T29 to T43 are done; T25 and T26 are prepared; T27 and T28 need the owner.
- EV-001 (font fallback) and EV-006 (CI snapshots) predate T40 to T43; T27 reruns them or records equivalence (REQ-046).
- EV-002, the design review (`docs/evidence/ms-1/design-review.md`): all 19 frames pass in 48 captures on KDE; the owner signs at T27.
- The CI `ui-test` job renders 32 snapshot baselines (33 tests) on Mesa lavapipe with embedded fonts.
- Catalog 5 is configured with seven packages under Project Standards 5.29.0.
- Codex has project-scoped Figma and Context7 MCP servers in `.codex/config.toml`; its `figma-*` skills are local and untracked (C1 in [conventions](handoff/conventions.md)).
- Rust 1.96.0 is pinned in `rust-toolchain.toml`; `.rexec.toml` pins nextest, deny, and audit binaries. The MS-0 Cargo workspace and CI gates exist.
- rexec cannot converge a declared `rust` tool (L3DigitalNet/rexec#26), so the worker uses its own rustup.
- The [specification register](handoff/specs-plans.md) points to the Standard-profile product and architecture draft, SPEC-FZDP.
