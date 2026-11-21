# Project Status

## Current snapshot

- Fuzzy DeDupe targets fuzzy text-content duplicate detection; the README currently provides only a one-line project description.
- The specification records a from-scratch Rust desktop application under `GPL-3.0-only`, with text-only V1 and no editing or AI similarity.
- Approved UI direction uses Findings and Compare workspaces with supporting popups; the draft reports no completed executable or validated safety/performance results.
- GUI framework, storage, matching algorithms, initial formats, and release platforms remain recommendations or open decisions in the specification.
- Catalog 5 is configured with seven packages under Project Standards 5.29.0.
- Codex has project-scoped Figma and Context7 MCP servers in `.codex/config.toml` and 17 vendored `figma-*` skills; see C1 in [conventions](handoff/conventions.md).
- Rust 1.96.0 is pinned in `rust-toolchain.toml`; `.rexec.toml` pins nextest, deny, and audit binaries. No Cargo workspace exists yet.
- rexec cannot converge a declared `rust` tool (L3DigitalNet/rexec#26), so the worker uses its own rustup.
- The [specification register](handoff/specs-plans.md) points to the Standard-profile product and architecture draft, SPEC-FZDP.
