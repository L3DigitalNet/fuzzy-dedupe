# Specifications and Plans

## Active artifacts

| Artifact | Status | Purpose |
| --- | --- | --- |
| [Fuzzy DeDupe specification](../specs/canonical/fuzzy-dedupe.md) | SPEC-FZDP, Standard draft v0.13 | Current requirements, compact UI handoff, proposed Rust architecture, acceptance criteria, and open decisions. |
| [MS-0 plan](../plans/2026-11-22-ms-0-scaffold-workspace-plan.md) | plan_format 3, complete (PR #9, `0b6dcbc`; EV-002) | Scaffold the eight-crate workspace, Rust CI gates, REUSE licensing and IBM Plex fonts. |
| [MS-1 plan](../plans/2026-11-22-ms-1-ui-on-fixtures-plan.md) | plan_format 3, revision 3, 43 tasks; T1–T24 and T29–T43 done; T25–T28 remain (owner) | Build the §11 compact UI in egui on fixtures; owner sign-off at T27. |

- Confirmed in the draft: Fuzzy DeDupe name, from-scratch Rust implementation, `GPL-3.0-only`, compact Findings/Compare UI, and a plain-text-only, Linux-only V1 with portable platform adapters.
- MS-1 inherits MS-0's Appendix A contracts C-1 to C-7 in the MS-0 plan; read them there, not here.
- C-1 crate set; C-2 lint split; C-3 `rust.yml`/`reuse.yml` job IDs; C-4 font paths; C-5 REUSE coverage and extension rule.
- C-6 dependency policy; C-7 boundary policy in `.github/boundaries.toml`, checked by `check-boundaries.py`.
- Editing and semantic/AI similarity are excluded; additional media belong to later releases.
- [ADR 0001](../adr/adr-0001-use-egui-for-the-v1-desktop-ui.md) commits to egui/eframe (amended 2026-11-22). SQLite and the matching algorithms still need validation.
- v0.7 defined the MS-0 and MS-1 exit lists in §19, v0.8 added the main menu, Keyboard shortcuts and About, and v0.9 corrected gaps found while planning; v0.12 made KDE Plasma 6.8.3 on Wayland the reference desktop; v0.13 made an empty file-type list mean all plain-text files; its source decisions are in the Basic Memory `knowledge` note `notes/fuzzy-dedupe-v07-decisions-2026-11-21`.
- The app runs on fixtures only; no deletion-safety validation or performance benchmark exists yet (T25 measures the UI); earlier research was not revalidated here.
