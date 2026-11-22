---
plan_format: 3
title: 'MS-1: Build the compact UI on fixtures in egui'
slug: 'ms-1-ui-on-fixtures'
status: active
revision: 1
revises_revision: 0
revision_reason: 'initial plan'
pause_reason: ''
source: 'spec:docs/specs/canonical/fuzzy-dedupe.md'
spec_ref: 'docs/specs/canonical/fuzzy-dedupe.md'
created: 2026-11-22
updated: 2026-11-22
owners:
  - 'L3DigitalNet'
---

# MS-1: Build the compact UI on fixtures in egui

> **Definition, not state.** Authoring drafts live in `.project-pipeline/2026-11-22-ms-1-ui-on-fixtures/authoring/`; generated execution status and evidence pointers live in `.project-pipeline/2026-11-22-ms-1-ui-on-fixtures/execution/`.

## 1. Objective

Deliver the approved compact Fuzzy DeDupe desktop interface (SPEC-FZDP v0.9 §11) as a working egui/eframe application that runs entirely on deterministic fixture data: the shell with its rail and main menu, the Findings and Compare workspaces, and every popup. A simulated scan reports progress and accepts cancellation, Compare shows a real `similar` line diff of two 5 MB fixture texts, the queue enforces the review-first and kept-counterpart rules, and the cleanup dialog runs through review, running, stopped and finished with simulated outcomes.

MS-1 exists to prove, before any real discovery or file operation is built, that egui reaches the MS-1 targets T1 to T4 on the reference workstation and that the implementation matches the review frames. Every item of the MS-1 exit list must hold for the application build the owner signs, so each workstation, design and manual record names the application inputs it validated, and T27 renews or formally reviews any record that predates the signed tree (D-016). MS-1 is complete only when the owner has signed off the evidence file `docs/evidence/ms-1-evidence.md` (T27) and the close-out pull request (T28) has landed with green `main` runs.

The principal invariant is safety by absence: the MS-1 build contains no code that moves or deletes the user's files and no Trash dependency. Two layers enforce it on every pull request: Clippy's `disallowed-methods` and `disallowed-types` lints from a workspace `clippy.toml`, which resolve imports, aliases and re-exports, and a bounded textual guard test (D-006, C-09).

## 2. Authority and Source Map

| Source | Source Role | Authority / Use | Version / Date | Affected Plan Surface |
| --- | --- | --- | --- | --- |
| `request` | normative | Owner authorization of MS-0/MS-1 planning (2026-11-22); dispatch brief constraints on dependencies, commands, owner acceptance and file ownership; coordinator revision: squash landing, MS-0 produced-contract names, T1 rows for `unicode-normalization` and development-only `proptest`, and PC-6 created by the executor on the owner's start instruction; coordinator round-1 decisions (2026-11-22): evidence binding and freshness, age-based log retention, the About phrase-check scope, per-kind decision binding, the `clippy.toml` semantic guard, serialized `Cargo.lock` and `.github/boundaries.toml` changes, alignment with the revised MS-0 plan (contracts C-1 to C-7, D-011, D-012, T6), and `main`-only admission with a close-out PR | 2026-11-22 | §§1–13 |
| `spec:docs/specs/canonical/fuzzy-dedupe.md#19-implementation-plan` | normative | MS-1 deliverable row and the eight-item MS-1 exit list | v0.9 | §6, T1–T27 |
| `spec:docs/specs/canonical/fuzzy-dedupe.md#111-authoritative-design-links-and-revision-precedence` | normative | Figma node IDs, review-frame list, design-review rule, precedence | v0.9 | T2, T9, T22, T24 |
| `spec:docs/specs/canonical/fuzzy-dedupe.md#112-two-workspaces-supporting-tasks-use-popups` | normative | Workspaces, rail, main menu and toolbar actions; §11.3 to §11.12 follow it with layout, Findings, Compare, popups, tokens, module and state owners, T1–T4, keyboard map, UI checks and implementation order | v0.9 | T4–T26 |
| `spec:docs/specs/canonical/fuzzy-dedupe.md#1111-acceptance-checklist-for-the-implementing-agent` | normative | UI checks 01–15; MS-1 gates 01–08, 12, 13 and 14 | v0.9 | §6 |
| `spec:docs/specs/canonical/fuzzy-dedupe.md#7-requirements` | normative | FR-017, FR-018, FR-023, FR-025–FR-028, NFR-005, NFR-007–NFR-009, IR-001–IR-003, IR-006, DR-008, DR-009 | v0.9 | §6 |
| `spec:docs/specs/canonical/fuzzy-dedupe.md#104-state-transitions` | normative | Cleanup item states and stop/re-confirm semantics, simulated in MS-1 | v0.9 | T8, T18 |
| `spec:docs/specs/canonical/fuzzy-dedupe.md#approved-dependencies` | normative | The only direct dependencies MS-1 may add; owner-approved row rule | v0.9 | T1, all code tasks |
| `spec:docs/specs/canonical/fuzzy-dedupe.md#85-design-constraints` | normative | Eight-crate boundaries, dependency direction, platform confinement, MS-1 adapters, lints | v0.9 | §5, T3–T23 |
| `spec:docs/specs/canonical/fuzzy-dedupe.md#172-test-strategy` | normative | CI gates and the snapshot-runner rule | v0.9 | §7, T22 |
| `spec:docs/specs/canonical/fuzzy-dedupe.md#173-requirement-to-test-traceability` | normative | Per-ID verification methods and status column | v0.9 | §6, T27 |
| `spec:docs/specs/canonical/fuzzy-dedupe.md#appendix-b-agent-implementation-contract` | normative | OQ-/DEV- recording, no unagreed dependencies, §17.3 currency | v0.9 | §10, T27 |
| `adr:docs/adr/adr-0001-use-egui-for-the-v1-desktop-ui.md#amendments` | decision | egui/eframe committed; misses are fixed within egui or by owner change of design or target; no Slint spike | amended 2026-11-22 | §3.4, D-001, T25, T27 |
| `repo:AGENTS.md` | operational evidence | GitHub workflow: draft PR, `## Governing work`, `land`; an operator instruction is sufficient authority for the action it names | current branch | §3.4, §10.1, all tasks |
| `repo:.agents/skills/github-workflow/references/pr-standard.md` | operational evidence | Admission classes; `merge` owns the `Workflow-Admission: PR #N` trailer and writes it into the squash commit | package 1.10 | D-009 |
| `repo:.rexec.toml` | operational evidence | Remote execution of heavy commands; pinned nextest, cargo-deny and cargo-audit | `e4d0e8d` | §7 |
| `repo:rust-toolchain.toml` | operational evidence | Pinned Rust 1.96.0 | `e4d0e8d` | §7 |
| `repo:.github/workflows/admission.yml` | current-state evidence | Existing CI is Markdown, spec and admission only; MS-0 adds the §17.2 gates; `admission` runs only on pushes to `main`, so no pull-request acceptance can observe it | `e4d0e8d` | §3.4, §4.1, T22, T28 |
| `repo:LICENSE` | current-state evidence | Unmodified GPLv3 text that About's View license shows byte for byte; line 572 contains "or any later version", which bounds the About phrase check (T19) | `e4d0e8d` | T19 |

Conflict precedence follows spec §11.1: explicit owner decisions and safety requirements, then the §11 compact-UI contract, then current Figma frames, then older material. Where the spec is ambiguous, this plan records a bounded assumption (§11.2) and an open question (§11.3) instead of choosing silently. The ADR's pre-amendment text ("final only once the MS-1 prototype passes", Slint fallback) is superseded by its own amendment and is not authority.

## 3. Scope, Boundaries, and Constraints

### 3.1 In Scope

- Owner-approved record of the MS-1 dependency additions: the named fontconfig lookup crate and any dev-dependency the owner chooses to approve (T1).
- PNG exports of the 19 review frames in `docs/design/reference/` with a manifest (T2).
- A workspace `clippy.toml` whose `disallowed-methods` and `disallowed-types` make the existing `clippy` job reject file-mutation and process-spawn APIs outside the C-09 allowlist, and a workspace test that keeps that configuration intact and textually rejects the same routes and any Trash crate in a manifest or the lockfile (T3).
- MS-1 domain types, the engine's typed request/event contract, and fixture services: result scenarios with 100,000 flattened rows, a simulated scan job, a `similar`-based comparison service with two 5 MB fixture texts, the review/queue policy, and a simulated cleanup run (T4–T9).
- The four MS-1 platform adapters (per-user folders, folder picker, system-font lookup, opening a folder or URL), the app-owned file writer and the age-based expiry of the application's own log files (T10, T21).
- The egui application: theme and tokens, fonts and icons, shell, main menu, dialog host, keyboard map, logging, Findings, the paired diff widget, Compare, all seven popups, and `preferences.toml` (T11–T20).
- The `egui_kittest` snapshot job on the GitHub-hosted Ubuntu runner with Mesa lavapipe, numeric layout assertions, and cross-popup and edge-state integration tests (T22).
- Measurement instrumentation and the T1–T4 runs on the reference workstation (T23, T25).
- The recorded design review (T24), the manual GNOME and KDE Plasma pass (T26), the signed MS-1 evidence file with its freshness review against the signed tree (T27), and the close-out pull request with the `main` proof, the Appendix B.3 completion report, the §17.3 statuses and any `DEV-` rows (T28).

### 3.2 Out of Scope and Deferred

- Real discovery, hashing, decoding, matching, SQLite persistence, file identity, and the Trash backend: MS-2 to MS-5 (§19). MS-1 reads no user file contents.
- Any code that moves, renames or deletes the user's files, and any `trash` dependency (exit item 8; permanent until MS-5 replaces the guard's scope). Writing the application's own preferences and rotating its own logs are allowed (exit item 8).
- Final tokenization and normalization profiles and their defaults: MS-3 (OQ-006). MS-1 applies NFC and the four Ignore options only to display the Normalized view.
- UI check 15, a design-review item that is not an MS-1 exit condition (§8.3). UI checks 09 to 11 are in scope and run against the simulated cleanup (exit item 4).
- Any framework comparison, Slint spike or framework-switch trigger (C-015, ADR 0001 amendment).
- Packaging, release notices beyond the in-repo notice files, and the GUI-independent benchmark harness of §8.5.

### 3.3 Ownership and Preserved Behavior

| Boundary | Owner / Responsibility |
| --- | --- |
| Plan owns | MS-1 additions inside the eight MS-0 crates, the workspace `clippy.toml` (T3), MS-1's `Cargo.lock` resolution changes and its third-party entries in `.github/boundaries.toml` (D-014), `docs/design/reference/`, `docs/evidence/ms-1-evidence.md`, `docs/evidence/ms-1-completion-report.md` and `docs/evidence/ms-1/`, the snapshot workflow job, and the §8.3, §17.3 and Deviations Log edits named in T1 and T28 |
| Depends on | MS-0 produced contracts (§3.4 PC-1 to PC-7, MS-0 Appendix A C-1 to C-7); the owner for approvals, exports fallback, the KDE environment and sign-off; Figma file `d5tA6ap1tH4cTDaJiwve89` |
| Does not own | MS-0's CI jobs other than the two `rust.yml` edits in §3.4, `deny.toml`, `.github/scripts/check-boundaries.py`, the C-1 edge table and GUI and OS crate lists in `.github/boundaries.toml`, crate scaffolding, the workspace `[workspace.lints]` policy and font files; `fuzzy-dedupe-scanner`, `-analyzers`, `-storage` and `-fileops`, which MS-1 leaves untouched; the ADRs; `.gitignore`; `.claude/settings.json` |
| Must preserve | MS-0 dependency direction (only `fuzzy-dedupe-app` depends on egui/eframe; `fuzzy-dedupe-domain` depends on no workspace crate; nothing depends on the app crate); OS-specific code and dependencies only in `fuzzy-dedupe-platform`; every MS-0 gate green on every PR; the four IBM Plex files unmodified |

### 3.4 Constraints and Authorization

- **Preconditions (MS-0 complete, not task dependencies), named as MS-0 produces them.** PC-1: the eight §8.5 crates exist; the workspace denies `unsafe_code` and every crate root except `fuzzy-dedupe-platform` carries `#![forbid(unsafe_code)]`; dependency direction holds and the app→platform edge exists. PC-2: `fuzzy-dedupe-app` builds the `fuzzy-dedupe` binary. PC-3: `.github/workflows/rust.yml` runs jobs `fmt`, `clippy`, `test`, `deny`, `audit`, `portability` and `boundaries` on PRs and on `main` (MS-0 C-3); nextest runs with `--no-tests=warn` until MS-1's first test; `deny.toml` lists `GPL-3.0-only` exceptions for the eight workspace crates; the `boundaries` job runs `python3 .github/scripts/check-boundaries.py` against `.github/boundaries.toml` (MS-0 D-011 rules 1–5, C-7), whose third-party table is empty, so any direct dependency without an entry for its declaring crate fails it. PC-4: `.github/workflows/reuse.yml` runs job `reuse` (reuse 6.3.0); `LICENSES/OFL-1.1.txt` and `LICENSES/GPL-3.0-only.txt` exist; `REUSE.toml` follows MS-0 D-008, D-012 and C-5, so no annotation glob matches `*.rs`, `Cargo.toml`, `deny.toml`, `.github/scripts/**`, `.github/boundaries.toml` or a header-bearing workflow, and every new comment-capable source or build or CI configuration file carries the two SPDX lines. PC-5: `crates/fuzzy-dedupe-app/assets/fonts/{IBMPlexSans-Regular,IBMPlexSans-Medium,IBMPlexSans-SemiBold,IBMPlexMono-Regular}.ttf` are present unmodified, with checksums recorded in `THIRD-PARTY-NOTICES.md` (MS-0 C-4), and MS-0 evidence is at `docs/evidence/ms-0-ci-gates.md` and `docs/evidence/ms-0-completion-report.md`. PC-6: when the owner says to start MS-1, the executor creates the governing MS-1 issue with `gh-workflow new` and admits it to `Ready`; under `AGENTS.md` that operator instruction is sufficient authority. PC-7: MS-0 is complete, which MS-0 defines as its close-out PR having landed with green `main` runs (MS-0 T6 and §10.1 step 4), and MS-0's post-landing handoff record (MS-0 §10.1 step 5, D-013) is on `main`; landing the MS-0 topic PR alone is not enough. No task starts until all seven hold.
- **egui is committed** (C-015, ADR 0001). A missed target is fixed within egui through a correction task, or the owner changes the design or the target. The plan contains no framework alternative.
- **Dependencies.** Only crates with a §8.3 approved row may become direct dependencies of any workspace crate, including dev-dependencies. T1's owner-approved PR adds rows for the named fontconfig crate, `unicode-normalization` (NFC and the Normalized view) and `proptest` (development only). Any other new direct dependency (for example `rand`, `rayon`, `tempfile` or an opener crate) requires an owner-approved table row first; an executor who needs one stops and appends a discovered task for the row. The task that first adds an approved crate to a crate's manifest adds that crate's MS-0 C-7 third-party entry in the same change, citing its §8.3 row: an OS-specific crate may be declared only by `fuzzy-dedupe-platform`, and a GUI crate (`egui`, `eframe`, `egui_*`, `winit`) only by `fuzzy-dedupe-app`. Every such task also claims `Cargo.lock` and `.github/boundaries.toml` and lands serially under D-014.
- **Heavy commands run through rexec:** `rexec -- cargo build --workspace --locked`, `rexec -- cargo clippy --workspace --all-targets --locked -- -D warnings`, `rexec -- cargo nextest run --workspace --locked`, `rexec -- cargo test --doc --workspace --locked`, `rexec -- cargo deny check`, `rexec -- cargo audit`. Exit codes 20–25 are rexec failures, never the command's result. Parallel executors work in separate Git worktrees; rexec serializes invocations per repository root.
- **Reference-workstation exception.** T21's glyph check and captures, T24's captures and T25's T1–T4 runs execute on the owner's reference workstation (Intel Core Ultra 7 155H, integrated Intel Arc graphics, 62 GB, Fedora with GNOME on Wayland; §11.9), not on the rexec worker, because they need the real display, GPU and system fonts. The measured release binary is built locally on the workstation under the same exception, or built through rexec with `--pull target/release/fuzzy-dedupe` if the pulled binary runs there; the evidence records which.
- **Snapshot runner.** Snapshot baselines are generated and checked only on the GitHub-hosted Ubuntu runner with Mesa lavapipe (§17.2), in a UI-test job MS-1 adds to `.github/workflows/rust.yml`. The repository is public, so GitHub-hosted runners are correct for it.
- **Licensing of new files.** Every new `*.rs`, `Cargo.toml`, `clippy.toml` and workflow edit keeps or carries the two SPDX lines (MS-0 D-008); new Markdown carries them inline; new binary or CSV files are covered by a `REUSE.toml` annotation (owner T11) or a per-file `.license` sidecar. `REUSE.toml` annotations follow MS-0 D-012 by origin: Lucide icons are third-party ISC (class d) and the PNG design exports are project-owned `GPL-3.0-only` (class a, §11.1). No MS-1 annotation glob may match a MS-0 C-5 protected path or `clippy.toml`. The `reuse` job passes on every PR.
- **Git and landing.** Git-mutating steps run locally. Non-handoff changes land through a draft PR and `.agents/skills/github-workflow/bin/gh-workflow land --pr N --method squash` (D-009). `admission` runs only on pushes to `main`, so no pull-request acceptance expects it. A task becomes `done`, and a dependent task branches from `main`, only after its squash commit shows green `main` runs of `rust.yml`, `reuse.yml`, `format`, `lint-markdown`, `validate-specs` and `admission`. Task branches are never deleted, locally or on origin, before the T28 close-out PR has landed and its post-landing handoff record is on `main`, because `recover` finds checkpoints with `git log --all`. Handoff-only updates use `Workflow-Admission: handoff` commits outside the task graph.
- **Not authorized:** any operation on user files beyond reading folder metadata in New scan; editing the ADRs, `.gitignore`, `.claude/settings.json`, `deny.toml`, `.github/scripts/check-boundaries.py`, `.github/workflows/reuse.yml`, or any `rust.yml` job other than the `--no-tests=warn` removal (T3) and the added UI-test job (T22); editing `.github/boundaries.toml` beyond adding third-party entries for crates the task itself adds; a wholesale `cargo update`; publishing releases; changing an acceptance threshold without an owner decision recorded in the spec.

## 4. Current State and Target State

### 4.1 Current State

Before MS-0 the repository holds documentation only: SPEC-FZDP v0.9, ADR 0001, handoff documents, the pinned toolchain (`rust-toolchain.toml`, Rust 1.96.0), `.rexec.toml` with nextest, cargo-deny and cargo-audit, and CI for Markdown formatting and lint, spec validation and commit admission. No Cargo workspace, crate, test or design export exists, and `docs/design/` does not exist. MS-0, planned separately, produces the workspace with a `Cargo.lock` listing only the eight workspace packages, `rust.yml`, `deny.toml`, `.github/scripts/check-boundaries.py` and `.github/boundaries.toml`, `reuse.yml`, `REUSE.toml`, `LICENSES/`, the Plex fonts, `docs/evidence/ms-0-ci-gates.md` and `docs/evidence/ms-0-completion-report.md` listed in §3.4. No `clippy.toml` exists. Every PR so far has squash-landed; `main` has no branch protection or ruleset, and `delete_branch_on_merge` is false.

### 4.2 Target State

The `fuzzy-dedupe` binary opens a 1600 × 960 "Fuzzy DeDupe" window (minimum 1024 × 600) that implements §11 against fixtures: the rail with + New scan, Findings, Compare and Menu; the main menu; both workspaces; and all seven popups, in light and dark themes that follow the system with an override. Fixture scenarios cover the review frames' sample data, a 100,000-row result set, edge states, and two 5 MB texts. View preferences persist in `$XDG_CONFIG_HOME/fuzzy-dedupe/preferences.toml`; logs roll daily in `$XDG_STATE_HOME/fuzzy-dedupe/logs`, and the application deletes its own log files once their date is more than 7 days old. CI adds an `egui_kittest` snapshot job, and the existing `clippy` job enforces the workspace `clippy.toml`. `docs/design/reference/` holds the review-frame PNGs, and `docs/evidence/ms-1-evidence.md` records every exit item with the owner's sign-off.

### 4.3 Delta Summary

| Area | Current | Target | Must Preserve |
| --- | --- | --- | --- |
| Application | MS-0 skeleton binary | Full §11 UI on fixtures | No file moves or deletes; no Trash dependency |
| Engine/domain | MS-0 empty crates | MS-1 domain types, request/event contract, fixture services | Domain depends on no workspace crate and no GUI type |
| Platform | MS-0 empty crate | Four MS-1 adapters plus the app-owned file writer | OS-specific code and dependencies only here (NFR-008) |
| CI | `rust.yml` seven jobs (`fmt`, `clippy`, `test`, `deny`, `audit`, `portability`, `boundaries`) and `reuse.yml` from MS-0 | `--no-tests=warn` removed; UI-test job added to `rust.yml`; `clippy.toml` enforced by the existing `clippy` job; C-7 third-party entries for every MS-1 crate | Every MS-0 job stays green and keeps its ID |
| Docs | Spec, ADR, handoff | Reference PNGs, evidence file, completion report, §8.3 rows, §17.3 statuses | Spec IDs and ADR text unchanged |

## 5. Change Surface and Architecture

### 5.1 Components and Ownership

| Component / Surface | Current Responsibility | Planned Responsibility | Paths / Contracts | Owning Task |
| --- | --- | --- | --- | --- |
| `fuzzy-dedupe-domain` | Empty (MS-0) | IDs, results model, matching rules, scan setup, progress, plan-item states | `crates/fuzzy-dedupe-domain/src/**`; Appendix A C-01 | T4 |
| Engine contract and fixtures | Empty (MS-0) | Request/event facade, fixture scenarios, module stubs | `crates/fuzzy-dedupe-engine/src/{lib.rs,service.rs,fixtures/**}`; C-02 | T4 |
| Simulated scan | None | Worker-saturating job with progress and cancellation | `crates/fuzzy-dedupe-engine/src/scan_sim/**`; C-03 | T5 |
| Comparison service | None | `similar` diff into an aligned-row model; 5 MB texts | `crates/fuzzy-dedupe-engine/src/compare/**`; C-04 | T6 |
| Review service | None | Single authority for queue, Kept, Protect, reference, acknowledgments | `crates/fuzzy-dedupe-engine/src/review/**`; C-05 | T7 |
| Simulated cleanup | None | §10.4 item states on IDs only | `crates/fuzzy-dedupe-engine/src/cleanup_sim/**`; C-06 | T8 |
| Design-sample scenario | None | Frame sample data as a fixture | `crates/fuzzy-dedupe-engine/src/fixtures/design.rs` | T9 |
| Platform adapters | Empty (MS-0) | Dirs, picker, open location, app-owned writes, log expiry, font lookup | `crates/fuzzy-dedupe-platform/src/**`; C-07 | T10, T21 |
| Theme, fonts, icons | Fonts on disk (MS-0) | Token adapter, typography, zoom, embedded icons | `crates/fuzzy-dedupe-app/src/ui/theme.rs`, `assets/icons/**` | T11 |
| Shell and presentation core | Skeleton `main.rs` | `app.rs`, shell, dialog host, status, state, commands, events, keyboard, logging | `crates/fuzzy-dedupe-app/src/{main.rs,app.rs,ui/shell.rs,ui/widgets/{dialog_host,status}.rs,presentation/**}` | T12 |
| Findings | None | Grouped virtualized table | `ui/findings.rs`, `ui/widgets/grouped_table.rs` | T13 |
| Paired diff widget | None | Aligned rows, wrap, linked scroll, selection | `ui/widgets/diff_view.rs` | T14 |
| Compare | None | Workspace host and return routing | `ui/compare.rs` | T15 |
| Popups | None | Seven dialogs | `ui/dialogs/*.rs` | T16–T19 |
| Preferences | None | Versioned `preferences.toml` | `presentation/preferences.rs`; C-08 | T20 |
| Snapshot CI and integration suite | None | UI-test job, layout and integration tests | `.github/workflows/rust.yml` (UI-test job), `crates/fuzzy-dedupe-app/tests/**` | T22 |
| Measurement | None | Frame and latency instrumentation, scenario drivers | `crates/fuzzy-dedupe-app/src/measure/**`; C-10 | T23 |
| No-mutation guard | None | `clippy.toml` semantic layer and the textual guard test | `clippy.toml`, `crates/fuzzy-dedupe-app/tests/no_file_mutation.rs`; C-09 | T3 |
| Dependency records | MS-0 lockfile and empty C-7 third-party table | Minimal lock updates and one C-7 entry per added crate | `Cargo.lock`, `.github/boundaries.toml`; D-014 | T4, T6, T10, T11, T21, T22 |
| Evidence | None | Supporting records bound to application inputs, renewals, the signed evidence file and the completion report | `docs/evidence/**`; Appendix A C-14; Appendix C | T21, T22, T24–T28 |

### 5.2 Control / Data / State Flow

```text
widgets (draw only) --emit--> UiIntent queue --app.rs, after the UI pass-->
   review/queue commands ----sync, in-memory----> engine review service (C-05) --> outcome
   job commands (scan, compare, cleanup) --bounded crossbeam channel--> engine workers
engine workers --revisioned events, bounded--> app.rs drains a bounded batch per frame
   --> presentation state owners (§11.8) --> next frame draws
platform adapters <-- app.rs / dialogs (dirs, picker, open location, fonts, app-owned writes)
```

Widgets never call the engine, the platform or the filesystem while drawing. Every job event carries the scan ID, request ID or plan revision that originated it, and the presentation layer discards any event whose origin is no longer current (§11.8). The engine performs no filesystem access in MS-1: fixture contents are generated in memory, and the simulated cleanup operates on IDs only.

### 5.3 Change-Surface Matrix

| Surface | Applies? | Invariant / Required Change | Proof | Task |
| --- | --- | --- | --- | --- |
| Behavior | yes | §11 UI on fixtures; queue and cleanup policy on simulated data | PV-T13-003, PV-T15-004, PV-T7-001 | T4–T20 |
| Architecture / dependency direction | yes | Only the app crate depends on egui; domain depends on no workspace crate; OS code only in platform | PV-T10-001 | T10 (MS-0 gate re-run in every task) |
| Public / cross-task interface | yes | Engine request/event contract, domain model, adapter traits | PV-T4-001, PV-T10-001 | T4, T10 |
| Data / state | yes | `preferences.toml` version 1; scan-scoped in-memory decisions bound per kind (C-05); no restored results; log files older than 7 days expire | PV-T20-001, PV-T7-003, PV-T10-002 | T7, T10, T20 |
| Configuration | yes | UI-test job in `rust.yml`; app crate feature for snapshot tests; `--no-tests=warn` removal; workspace `clippy.toml`; C-7 entries and minimal lock updates | PV-T22-002, PV-T3-001, PV-T3-002 | T3, T22, D-014 tasks |
| Security / trust | yes | No user-file move or delete code; no Trash crate; only the application's own expired logs are deleted; logs hold no document text | PV-T3-001, PV-T3-002, PV-T10-002, PV-T22-006 | T3, T10, T22 |
| Compatibility / migration | no | Greenfield; no prior user state exists | — | — |
| Operations / deployment | yes | Reference-workstation measurement and manual desktop pass; serialized landing with green `main` runs; close-out PR | PV-T25-001, PV-T26-001, PV-T28-001 | T25, T26, T28 |
| Documentation | yes | §8.3 rows; §17.3 statuses and `DEV-` rows; reference manifest; completion report | PV-T1-001, PV-T28-001 | T1, T28 |
| Durable evidence | yes | Evidence file, supporting records bound to application inputs, renewals (Appendix C, C-14) | PV-T27-001, PV-T27-002 | T21, T22, T24–T28 |

### 5.4 Binding Decisions

| ID | Decision | Rationale | Source | Affected Task(s) |
| --- | --- | --- | --- | --- |
| D-001 | Build with egui/eframe on wgpu; pin eframe, egui, egui_extras, egui_table and egui_kittest exactly to one mutually compatible set. eframe persistence stays off; egui's bundled default fonts are disabled; the unmodified monochrome Noto Emoji (OFL-1.1) from a pinned release is embedded as the emoji fallback, and fontconfig covers other scripts. | C-015, ADR 0001, DR-009, §8.3, §11.7 Fonts (v0.9) | spec §8.3, §11.7, ADR 0001 | T11, T21 |
| D-002 | Fixture data, the simulated scan, the comparison service and the simulated cleanup live in `fuzzy-dedupe-engine` behind the typed contract C-02. Rejected: a fixture module inside the app crate, because §8.5 and §11.8 forbid UI code running diffs or owning job lifecycles, and MS-4 would have to move it. | §8.2, §8.5, §11.8, §11.9 | spec §8.5, §11.8 | T4–T9, T12 |
| D-003 | The engine review service is the only authority that evaluates queue entry points and owns the scan-scoped decision set; presentation `QueueState` is a read model updated from its outcomes. Rejected: policy in widgets or presentation, because UI check 09 requires one policy across entry points and §8.5 routes file-operation decisions through the application service. | §8.5, §11.8, DR-008 ownership, UI check 09 | spec §11.8, §7.4 | T7, T13, T15, T17 |
| D-004 | Contract-first stubs: T4, T10 and T12 create every module file later tasks fill, so parallel tasks write disjoint files. Rejected: each task adding its own `mod` lines, which would serialize all view and engine tasks through shared files. | Parallel safety | plan method | T4, T10, T12 |
| D-005 | Fixture "files" are deterministic in-memory buffers; MS-1 writes no fixture file at runtime. Rejected: temporary files, which would need deletion code (exit item 8). | Exit item 8 | spec §19 | T4, T6 |
| D-006 | Exit item 8 is enforced in two layers. The semantic layer is a workspace `clippy.toml` (T3) whose `disallowed-methods` and `disallowed-types` list the C-09 routes; the existing MS-0 `clippy` job (`-D warnings`) enforces it with no workflow edit, and because Clippy resolves paths it catches imported, aliased and re-exported uses. An allowed use carries `#[expect(clippy::disallowed_methods, reason = "…")]` or `#[expect(clippy::disallowed_types, reason = "…")]` and appears only where C-09 allows it; `expect` rather than `allow` makes a stale allowance fail as an unfulfilled expectation. The second layer is a bounded textual workspace test in the existing nextest gate, which also keeps `clippy.toml` complete and the allowances confined, and checks manifests and `Cargo.lock` for a Trash crate. Rejected: a textual scan alone, which misses ordinary Rust import and alias forms (round-1 PA-005); a one-off sign-off grep (no regression protection); and a new CI job (MS-0 owns `rust.yml` jobs). Reopen if the owner approves a dedicated lint tool. | Exit item 8; ownership; coordinator decision | spec §19, `request` | T3, T10, T12 |
| D-007 | Snapshot tests are isolated from the seven MS-0 jobs and run in a UI-test job added to `rust.yml` on a pinned GitHub-hosted Ubuntu image with Mesa lavapipe; baselines come only from that runner, and the job fails when zero snapshot tests execute. The snapshot harness uses embedded fonts only. Rejected: workstation or rexec baselines and real system fallback fonts, both of which differ from the runner and make baselines unstable; and a separate workflow file, because MS-0's contract places the UI-test job in `rust.yml`. | §17.2; MS-0 C-3 | spec §17.2 | T22 |
| D-008 | Review commands are synchronous in-memory calls dispatched by `app.rs` after the UI pass; scan, comparison and cleanup are asynchronous jobs over bounded channels with revisioned events. Rejected: routing cheap review commands through worker threads, which adds ordering hazards without I/O to hide. | §11.8, §11.9 | spec §11.8 | T4, T7, T12 |
| D-009 | Landing: one draft PR per task by default; consecutive dependent tasks finished by one executor may share a PR of at most three task commits, except that a D-014 task shares a PR with no other D-014 task. Land with `gh-workflow land --pr N --method squash`, matching MS-0 and every earlier PR. Task-branch commits carry only the `Plan-*` checkpoint trailers and never a self-written `Workflow-Admission` trailer; `merge` writes `Workflow-Admission: PR #N` into the squash commit. The branch commit is the checkpoint recorded with `state --commit`, and the executor requests `done` only after the squash commit's `main` runs of `rust.yml`, `reuse.yml`, `format`, `lint-markdown`, `validate-specs` and `admission` are green, so no dependent task branches from an unproven `main`. `admission` runs only on pushes to `main`; PR acceptance never expects it. `recover` scans `git log --all`, so no task branch is deleted, locally or on origin, until the T28 close-out PR has landed and its post-landing handoff record is on `main` (`delete_branch_on_merge` is false), and T28 records the checkpoint OID, PR number and squash OID of T1–T27 and completed corrections, while T28's own go to the post-landing handoff record on `main`. Rejected: merge-method landing with self-written admission trailers, because the pr-standard makes `merge` the owner of that trailer and MS-0's probe showed merge and rebase landings leave commits unadmitted. | `recover` discovers checkpoints through `git log --all`; pr-standard admission ownership; MS-0 D-010; coordinator decision | `request`, `repo:AGENTS.md`, pr-standard | all tasks |
| D-010 | Measurement samples are printed to standard output as CSV by a release build in an opt-in measurement mode; the operator redirects them into the evidence directory. Rejected: in-app file writing, which would widen the file-writing allowlist. | Exit item 8; §11.9 | spec §11.9 | T23, T25 |
| D-011 | No crate beyond the §8.3 rows. T1 adds the `unicode-normalization` row, used by T6 for NFC and the Normalized view, and the development-only `proptest` row, used by T6 for normalization properties. Queue invariants keep exhaustive enumeration over bounded group shapes as their oracle, because enumeration covers every small shape deterministically. | §8.3, §17.2; coordinator decision | spec §8.3, `request` | T1, T6, T7 |
| D-012 | The §8.5 fourth adapter, opening a folder or web address with the desktop's default application, spawns `xdg-open` through `std::process::Command` in the platform crate only; it serves Open containing folder, Open log folder and About's Source code. Rejected: an opener crate, which has no approved row. | §8.5 MS-1 adapters (v0.9); FR-028 | spec §8.5 | T10, T19 |
| D-013 | The MS-1 evidence file is `docs/evidence/ms-1-evidence.md`. It sits beside MS-0's `docs/evidence/ms-0-ci-gates.md`, and T28's `docs/evidence/ms-1-completion-report.md` sits beside MS-0's completion report. Supporting records and raw data live in `docs/evidence/ms-1/`; evidence Markdown carries inline SPDX lines and each binary or CSV file a `.license` sidecar, so evidence tasks never edit the root `REUSE.toml`. | §19 exit list requires one evidence file; Appendix B.3; MS-0 D-010 | request | T21, T22, T24–T28 |
| D-014 | `Cargo.lock` and `.github/boundaries.toml` are shared files. Every task that adds a direct dependency to a manifest, or changes a manifest in a way that changes the resolution (T4, T6, T10, T11, T21, T22), claims both, declares `conflicts_with` against every other such task, and lands serially. Before landing, that task rebases on the current `main`, updates the lock minimally by a resolution run without `--locked` (for example `cargo metadata --format-version 1` or `cargo check`; never a wholesale `cargo update`), reviews the lock diff so that it adds only the new crates and their transitive closure and changes no unrelated version, adds the C-7 entry for each crate it adds, and passes the `--locked` gates and the `boundaries` job against the combined manifests. T4 is the declared owner of both files for MS-1; the others modify only their own entries and lock delta. A later task that adds an approved crate to a further crate (for example `tracing` in the engine) is a D-014 task and appends the same claims. Rejected: one serialized integration task that adds every dependency up front, because it would declare crates before their consumers exist and push lock review away from the change that needs it; and independent lock regeneration per branch, which validates a resolution different from the combined one (round-1 PA-006). | §8.3 committed lockfile; §17.2 locked builds; MS-0 C-7; coordinator decision | spec §8.3, §17.2, `request` | T4, T6, T10, T11, T21, T22 |
| D-015 | Log retention is age-based. At startup and at each daily rollover the application deletes its own log files, matching only the roller's file-name pattern inside the platform-resolved log directory, whose file-name date is more than 7 days before the current date in the roller's time base; recent logs, files that do not match, files with an unparseable date, directories and symbolic links are kept. The deletion lives in `crates/fuzzy-dedupe-platform/src/log_retention.rs` (T10), the only module C-09 allows to delete, and takes the current date as a parameter so tests inject the clock; T12 calls it outside the UI pass. A file-count cap on the roller may stay as an extra bound only. Rejected: a count cap alone, because intermittent use makes seven daily files span far more than seven days (round-1 PA-002); and deletion inside the app crate, which would put a delete call outside the platform confinement the guard checks. | NFR-009 "kept for 7 days"; exit item 8 "rotating its own logs is allowed"; coordinator decision | spec NFR-009, §18.5, §19, `request` | T10, T12, T3 |
| D-016 | Evidence freshness: every durable workstation, design, manual or CI record (EV-001 to EV-004, EV-006) names the application commit OID it validated and, at that commit, the tree OID of `crates/` and the object IDs of `Cargo.lock`, the root `Cargo.toml` and `rust-toolchain.toml` (C-14). T24, T25 and T26 depend on every runtime implementation task, T20 and T21 included. T27 compares each record's binding with the signed tree; on a mismatch it reruns the affected checks or, only for a change it shows to be non-functional (documentation, comments, test-only), records an equivalence review naming the diff, both in EV-007. Every correction task lists the evidence records it invalidates. The root `Cargo.toml` and `rust-toolchain.toml` are bound as well as the two inputs the coordinator named because a release-profile or toolchain change alters the measured binary without touching `crates/`. Rejected: trusting a historical pass because its task is `done`, which lets sign-off rest on a different build (round-1 PA-001). | §19 exit items 1–3, 6, 7 and owner sign-off; §11.9; coordinator decision | spec §19, §11.9, `request` | T21, T22, T24–T27, correction tasks |

## 6. Requirements and Acceptance

| ID | Requirement | Source | Priority | Owner Task | Task(s) | Proof(s) |
| --- | --- | --- | --- | --- | --- | --- |
| REQ-001 | T1–T4 (§11.9) are met by a release build on the reference workstation in the 1600 × 960 client area and recorded with hardware, software and run conditions (MS-1 exit 1; UI check 12). | `spec:docs/specs/canonical/fuzzy-dedupe.md#ms-1-performance-targets` | Must | T25 | T25 | PV-T25-001 |
| REQ-002 | A recorded design review compares the running application with `docs/design/reference/` for each of the 19 review frames (exit 2). | `spec:docs/specs/canonical/fuzzy-dedupe.md#111-authoritative-design-links-and-revision-precedence` | Must | T24 | T24 | PV-T24-001 |
| REQ-003 | PNG exports of the 19 review frames are stored in `docs/design/reference/` with a manifest of node IDs, export scale and checksums, and `REUSE.toml` declares them project-owned and `GPL-3.0-only` (§11.1, §8.5). | `spec:docs/specs/canonical/fuzzy-dedupe.md#111-authoritative-design-links-and-revision-precedence` | Must | T2 | T2 | PV-T2-001 |
| REQ-004 | Numeric layout assertions pass for row heights, column widths, pane widths in 1600 × 960 and 1024 × 600, and token colors (exit 2; §11.3, §11.4, §11.7). | `spec:docs/specs/canonical/fuzzy-dedupe.md#113-layout-measurements-and-visual-hierarchy` | Must | T22 | T22 | PV-T22-001 |
| REQ-005 | `egui_kittest` snapshot tests of the review frames pass in CI in both themes on the GitHub-hosted Ubuntu runner with Mesa lavapipe, against baselines generated there (exit 3; §17.2). | `spec:docs/specs/canonical/fuzzy-dedupe.md#172-test-strategy` | Must | T22 | T22 | PV-T22-002 |
| REQ-006 | A manual pass on GNOME and KDE Plasma (Wayland) covers 100, 125, 150 and 200% scale, the 1024 × 600 minimum, moving between monitors, and KDE shortcut conflicts (exit 6). | `spec:docs/specs/canonical/fuzzy-dedupe.md#19-implementation-plan` | Must | T26 | T26 | PV-T26-001 |
| REQ-007 | Japanese, Arabic and emoji file names render through the font fallback (exit 7; §11.7 Fonts). | `spec:docs/specs/canonical/fuzzy-dedupe.md#117-centralized-visual-tokens-and-reusable-widgets` | Must | T21 | T21 | PV-T21-001 |
| REQ-008 | The MS-1 build contains no code that moves, renames or deletes the user's files and no Trash dependency; writing the application's own preferences and rotating its own logs are allowed (exit 8). Clippy's `disallowed-methods` and `disallowed-types` from the workspace `clippy.toml` and the textual guard both enforce it (C-09). | `spec:docs/specs/canonical/fuzzy-dedupe.md#19-implementation-plan` | Must | T3 | T3 | PV-T3-001, PV-T3-002 |
| REQ-009 | The MS-1 evidence file records every exit-list result and the owner's sign-off (§19 MS-1 exit condition; ADR 0001 Confirmation). | `spec:docs/specs/canonical/fuzzy-dedupe.md#19-implementation-plan` | Must | T27 | T27 | PV-T27-001 |
| REQ-010 | UI check 01 and FR-019: only Findings and Compare are permanent; the seven popups open modally, one at a time, without losing workspace state. | `spec:docs/specs/canonical/fuzzy-dedupe.md#1111-acceptance-checklist-for-the-implementing-agent` | Must | T22 | T22 | PV-T22-003 |
| REQ-011 | UI check 02 and NFR-007: in 1600 × 960 the findings table and the comparison region each occupy at least 80% of their work-area height, with no card dashboard or persistent inspector. | `spec:docs/specs/canonical/fuzzy-dedupe.md#1111-acceptance-checklist-for-the-implementing-agent` | Must | T22 | T22 | PV-T22-004 |
| REQ-012 | UI check 03: compact sizing and `DD/Compact/*` typography match the review frames in both themes while scaling with display settings and zoom, rechecked at 200% zoom. | `spec:docs/specs/canonical/fuzzy-dedupe.md#1111-acceptance-checklist-for-the-implementing-agent` | Must | T24 | T24 | PV-T24-002 |
| REQ-013 | UI check 04: sorting, filtering, collapsing groups and returning from Compare change neither queued membership nor the identity of a targeted file. | `spec:docs/specs/canonical/fuzzy-dedupe.md#1111-acceptance-checklist-for-the-implementing-agent` | Must | T15 | T15 | PV-T15-002 |
| REQ-014 | UI check 05 and FR-013: each member row's label is the §11.4 template for its relation over its actual counterpart ("reference" or the counterpart's file name), its Match and Unique values refer to that file before and after a reference change, and focus and cleanup intent are independently visible. | `spec:docs/specs/canonical/fuzzy-dedupe.md#1111-acceptance-checklist-for-the-implementing-agent` | Must | T13 | T13 | PV-T13-002 |
| REQ-015 | UI check 06 (alignment part) and IR-002: paired diffs stay aligned through insertions, deletions and unequal wrap heights, with correct source line numbers. | `spec:docs/specs/canonical/fuzzy-dedupe.md#1111-acceptance-checklist-for-the-implementing-agent` | Must | T14 | T14 | PV-T14-001 |
| REQ-016 | UI check 07: original text is inspectable and selectable within one pane, comparison is read-only, and no editor, merge or AI control exists. | `spec:docs/specs/canonical/fuzzy-dedupe.md#1111-acceptance-checklist-for-the-implementing-agent` | Must | T15 | T15 | PV-T15-003 |
| REQ-017 | UI check 08 and IR-003: cancel or close leaves drafts uncommitted; saved matching rules affect subsequent scans without relabeling current evidence. | `spec:docs/specs/canonical/fuzzy-dedupe.md#1111-acceptance-checklist-for-the-implementing-agent` | Must | T16 | T16 | PV-T16-001 |
| REQ-018 | UI check 13, NFR-005 and C-016: keyboard-only operation, focus restoration, long paths, Unicode names, display scaling and zoom are tested on GNOME and KDE Plasma (Wayland). | `spec:docs/specs/canonical/fuzzy-dedupe.md#1111-acceptance-checklist-for-the-implementing-agent` | Must | T26 | T26 | PV-T26-002 |
| REQ-019 | UI check 14: missing folders, empty scans, no search matches, decoding errors, stale evidence, scan cancellation and unavailable Trash each have an actionable state without an extra permanent screen. | `spec:docs/specs/canonical/fuzzy-dedupe.md#1111-acceptance-checklist-for-the-implementing-agent` | Must | T22 | T22 | PV-T22-005 |
| REQ-020 | Deterministic fixture scenarios include a result set of exactly 100,000 flattened rows with all groups expanded, plus edge-state scenarios (§19 MS-1 row). | `spec:docs/specs/canonical/fuzzy-dedupe.md#19-implementation-plan` | Must | T4 | T4 | PV-T4-001 |
| REQ-021 | Two deterministic fixture texts of about 5 MB and 100,000 lines each, with about 10% changed lines, plus small pairs whose unique-line counts match the Findings fixtures (§19; §11.9 T2). | `spec:docs/specs/canonical/fuzzy-dedupe.md#19-implementation-plan` | Must | T6 | T6 | PV-T6-001 |
| REQ-022 | A simulated scan job saturates the worker threads, reports progress (folder, files read, groups found, elapsed) and accepts cancellation with acknowledgment and partial coverage (§19; §11.6 Scanning). | `spec:docs/specs/canonical/fuzzy-dedupe.md#116-popup-contracts-and-compact-progress-handling` | Must | T5 | T5 | PV-T5-001 |
| REQ-023 | Compare evidence is a real line diff from `similar`, computed off the UI thread, keyed by snapshot, rules revision and mode, and never shown stale as current (§19; §11.5 rules 1–3, 8–9). | `spec:docs/specs/canonical/fuzzy-dedupe.md#115-comparison-large-aligned-read-only-panes` | Must | T6 | T6 | PV-T6-002 |
| REQ-024 | The cleanup dialog runs through review, running, stopped and finished with simulated outcomes, single-flight confirmation, "Blocked: no Trash", and no permanent-delete option (§19; §11.6 Cleanup). | `spec:docs/specs/canonical/fuzzy-dedupe.md#116-popup-contracts-and-compact-progress-handling` | Must | T18 | T18 | PV-T18-001 |
| REQ-025 | The simulated cleanup run follows the §10.4 item states, stops before a changed or failed item, returns unreached items to `planned`, and touches no filesystem path. | `spec:docs/specs/canonical/fuzzy-dedupe.md#104-state-transitions` | Must | T8 | T8 | PV-T8-001 |
| REQ-026 | Kept, per-file Protect and unique-text acknowledgments are scan-scoped. Each decision binds to its target snapshot, its counterpart snapshot (the counterpart's identity and content) and the matching-rules revision, and a change to any one invalidates that decision; a new scan clears every decision, and folder protection from the scan setup, which is not a decision, persists (DR-008, in-memory part; C-05). | `spec:docs/specs/canonical/fuzzy-dedupe.md#74-data-requirements` | Must | T7 | T7 | PV-T7-003 |
| REQ-027 | Logging writes daily files to the adapter-supplied `$XDG_STATE_HOME/fuzzy-dedupe/logs`, deletes the application's own log files whose date is more than 7 days old at startup and at each daily rollover while keeping recent logs and unrelated files (D-015), defaults to info, honors `RUST_LOG`, and "Open log folder" opens the folder (NFR-009; §18.5). | `spec:docs/specs/canonical/fuzzy-dedupe.md#185-observability` | Should | T12 | T10, T12 | PV-T10-002, PV-T12-003 |
| REQ-028 | The §11.10 keyboard map is implemented, with no single-letter shortcuts, no key that queues to Trash, and underlying shortcuts blocked while a popup is open. | `spec:docs/specs/canonical/fuzzy-dedupe.md#1110-resizing-scaling-and-keyboard-behavior` | Must | T12 | T12 | PV-T12-004 |
| REQ-029 | One theme adapter supplies every §11.7 token in both modes and the eight compact text styles; no text is below 12 units; every token pairing used for text meets 4.5:1. | `spec:docs/specs/canonical/fuzzy-dedupe.md#117-centralized-visual-tokens-and-reusable-widgets` | Must | T11 | T11 | PV-T11-001 |
| REQ-030 | Only the Lucide icons the UI uses are embedded with `LICENSES/ISC.txt`, and the unmodified monochrome Noto Emoji from a pinned release is embedded with its checksum; both have `REUSE.toml` coverage and `THIRD-PARTY-NOTICES.md` entries, and `reuse lint` passes. | `spec:docs/specs/canonical/fuzzy-dedupe.md#183-deployment-flow` | Must | T11 | T11 | PV-T11-002 |
| REQ-031 | The owner-approved §8.3 table names the fontconfig lookup crate and adds rows for `unicode-normalization` and development-only `proptest` before any of them is added. | `spec:docs/specs/canonical/fuzzy-dedupe.md#approved-dependencies` | Must | T1 | T1 | PV-T1-001 |
| REQ-032 | The dialog host gives every popup a title, a reachable close control, visible focus, Esc cancel, a non-destructive Enter default, viewport-constrained size and focus restoration to the invoker (§11.6). | `spec:docs/specs/canonical/fuzzy-dedupe.md#116-popup-contracts-and-compact-progress-handling` | Must | T12 | T12 | PV-T12-005 |
| REQ-033 | Queue identical copies states its scope, rules, file count and size, and queues only byte-identical non-reference, non-protected, non-kept files (§11.4; `27:1868`). | `spec:docs/specs/canonical/fuzzy-dedupe.md#114-findings-table-structure-and-interactions` | Must | T17 | T17 | PV-T17-002 |
| REQ-034 | A release-build measurement mode records eframe `cpu_usage` per painted frame, first paint after a diff is ready, input-to-paint latency and cancellation acknowledgment, and drives scripted T1–T4 scenarios (§11.9). | `spec:docs/specs/canonical/fuzzy-dedupe.md#ms-1-performance-targets` | Must | T23 | T23 | PV-T23-001 |
| REQ-035 | The shell matches §11.2 and §11.3: native title "Fuzzy DeDupe", rail 152 wide that collapses to 48 below 1280, main menu (F10) with its six entries, 26-high status bar, 1024 × 600 minimum window. | `spec:docs/specs/canonical/fuzzy-dedupe.md#112-two-workspaces-supporting-tasks-use-popups` | Must | T12 | T12 | PV-T12-002 |
| REQ-036 | Compare provides the §11.5 toolbar, evidence row with Only-in menus, change row, candidate stepper, Keep both, Queue B for cleanup… and the disabled-with-explanation state when no pair exists. | `spec:docs/specs/canonical/fuzzy-dedupe.md#115-comparison-large-aligned-read-only-panes` | Must | T15 | T15 | PV-T15-004 |
| REQ-037 | Findings provides the §11.4 columns, member labels, filter row and counts, search with clear, Sort and Actions menus, row menu, column collapse below 1280, and the 01a, 01b, 01c, 01e and empty-scan states. | `spec:docs/specs/canonical/fuzzy-dedupe.md#114-findings-table-structure-and-interactions` | Must | T13 | T13 | PV-T13-003 |
| REQ-038 | A design-sample fixture scenario reproduces the sample rows, labels, counts and dialog contents visible in the review frames, as fixture data only (§11.1 "Do not infer new business rules from … sample data"). | `spec:docs/specs/canonical/fuzzy-dedupe.md#111-authoritative-design-links-and-revision-precedence` | Must | T9 | T9 | PV-T9-001 |
| REQ-039 | UI check 09: the checkbox, Space, the row menu, the Compare button and Queue identical copies enforce one reference, protected, kept-counterpart and fuzzy-review policy, and a changed target or kept counterpart invalidates the affected approval. | `spec:docs/specs/canonical/fuzzy-dedupe.md#1111-acceptance-checklist-for-the-implementing-agent` | Must | T22 | T22 | PV-T22-007 |
| REQ-040 | UI check 10: cleanup review states exact targets, kept copies and destination, commit is single-flight, targets without Trash show "Blocked: no Trash" before confirmation, and no permanent deletion exists (simulated cleanup). | `spec:docs/specs/canonical/fuzzy-dedupe.md#1111-acceptance-checklist-for-the-implementing-agent` | Must | T18 | T18 | PV-T18-002 |
| REQ-041 | UI check 11: moved, failed, blocked, stopped and uncertain outcomes stay visible in the popup and the journal view, a stop requires confirming the revised plan, and retry does not replay completed actions (simulated cleanup). | `spec:docs/specs/canonical/fuzzy-dedupe.md#1111-acceptance-checklist-for-the-implementing-agent` | Must | T18 | T18 | PV-T18-003 |
| REQ-042 | The unique-line measure of §10.1 (content-based, original text without terminator, every occurrence counted, moved or repeated lines present in the counterpart not unique, Normalized view applying the scan's rules first and labeled) is computed once and Copy unique lines text is the unique lines in file order, one LF after each. | `spec:docs/specs/canonical/fuzzy-dedupe.md#grouping-policy` | Must | T6 | T6 | PV-T6-003 |
| REQ-043 | UI check 06 (unique-line part): the Findings Unique column, Compare's Only in counts and Review unique text agree on the §17.3 fixtures, and Copy unique lines yields the specified text. | `spec:docs/specs/canonical/fuzzy-dedupe.md#1111-acceptance-checklist-for-the-implementing-agent` | Must | T22 | T22 | PV-T22-008 |
| REQ-044 | Make this the reference removes the new reference from the queue, recomputes every counterpart by the §10.1 rule discarding Compare-chosen counterparts, unqueues each other queued member whose counterpart changed and clears its acknowledgment, keeps the rest, and reports the unqueued count (§10.1). | `spec:docs/specs/canonical/fuzzy-dedupe.md#grouping-policy` | Must | T7 | T7 | PV-T7-004 |
| FR-017 | Kept and Protected are separate states that follow their files; no route queues a protected file; bulk actions skip kept files. | `spec:docs/specs/canonical/fuzzy-dedupe.md#71-functional-requirements` | Must | T7 | T7 | PV-T7-002 |
| FR-018 | Findings is a dominant compact grouped table keyed by stable row identities; filtering, sorting and expansion preserve focus and queued intent. | `spec:docs/specs/canonical/fuzzy-dedupe.md#71-functional-requirements` | Must | T13 | T13 | PV-T13-001 |
| FR-023 | Fuzzy or normalized removal intent requires deliberate review from every entry point; the acknowledgment flow of `44:4079` applies. | `spec:docs/specs/canonical/fuzzy-dedupe.md#71-functional-requirements` | Must | T17 | T17 | PV-T17-001 |
| FR-025 | A group's reference is never queued; a file is queued only while its counterpart stays unqueued ("Counterpart queued"); a file that a queued file was measured against is refused ("Counterpart of a queued file"). | `spec:docs/specs/canonical/fuzzy-dedupe.md#71-functional-requirements` | Must | T7 | T7 | PV-T7-001 |
| FR-026 | Starting a new scan clears the queue and every Kept, per-file Protect and acknowledgment decision; New scan states the count it clears; folder protection persists. | `spec:docs/specs/canonical/fuzzy-dedupe.md#71-functional-requirements` | Must | T16 | T16 | PV-T16-002 |
| FR-027 | Light and dark themes follow the system theme with a System, Light or Dark override in the main menu, recoloring every view without a restart. | `spec:docs/specs/canonical/fuzzy-dedupe.md#71-functional-requirements` | Must | T12 | T12 | PV-T12-001 |
| FR-028 | About shows the GPLv3 Appropriate Legal Notices; View license and Third-party notices replace the About body with the embedded text in a read-only, selectable `DD/Compact/document` view with Back; Source code opens the source location in the browser. | `spec:docs/specs/canonical/fuzzy-dedupe.md#71-functional-requirements` | Must | T19 | T19 | PV-T19-001 |
| IR-001 | Returning from Compare restores all seven Findings view properties. | `spec:docs/specs/canonical/fuzzy-dedupe.md#73-interface-requirements` | Must | T15 | T15 | PV-T15-001 |
| IR-006 | Model-backed selection and copy within one Compare pane return the exact original text with its line endings and hold the T2 frame budget. | `spec:docs/specs/canonical/fuzzy-dedupe.md#73-interface-requirements` | Must | T14 | T14 | PV-T14-002 |
| DR-009 | View preferences (window size and maximized state, not position) and the current scan setup persist in versioned `preferences.toml` at the adapter path and survive a restart; results, queue and decisions do not. | `spec:docs/specs/canonical/fuzzy-dedupe.md#74-data-requirements` | Must | T20 | T20 | PV-T20-001 |
| NFR-008 | Per-user locations, the folder picker, system-font lookup and external opening are reached only through `fuzzy-dedupe-platform`, which alone holds OS-specific code and dependencies. | `spec:docs/specs/canonical/fuzzy-dedupe.md#72-non-functional-requirements` | Must | T10 | T10 | PV-T10-001 |
| NFR-009 | A fixture scan and comparison leave no document text in the log files. | `spec:docs/specs/canonical/fuzzy-dedupe.md#72-non-functional-requirements` | Should | T22 | T22 | PV-T22-006 |
| REQ-045 | MS-1 is complete only after T27's owner-signed evidence file has squash-landed with green `main` runs and a close-out PR has landed with green `main` runs recording those runs, the Appendix B.3 completion report, the §17.3 statuses of the MS-1-verified IDs and a `DEV-` row for each deviation, after which a post-landing handoff record on `main` records the close-out PR's own identifiers and runs (§19; Appendix B.1, B.3, B.4). | `spec:docs/specs/canonical/fuzzy-dedupe.md#appendix-b-agent-implementation-contract` | Must | T28 | T28 | PV-T28-001 |
| REQ-046 | Every durable record the evidence file cites validates the signed application: its C-14 binding equals the signed tree, or EV-007 records a rerun on the signed tree or an equivalence review that names the diff and shows it non-functional (D-016). | `spec:docs/specs/canonical/fuzzy-dedupe.md#19-implementation-plan` | Must | T27 | T27 | PV-T27-002 |

## 7. Verification and Evidence Strategy

- **Authoritative commands:** `rexec -- cargo fmt --all --check`; `rexec -- cargo clippy --workspace --all-targets --locked -- -D warnings`; `rexec -- cargo nextest run --workspace --locked`; `rexec -- cargo test --doc --workspace --locked`; `rexec -- cargo deny check`; `rexec -- cargo audit`; `reuse lint`; the repository's Prettier and markdownlint commands from `AGENTS.md` for changed Markdown; the MS-0 `portability` check and `python3 .github/scripts/check-boundaries.py` (the `boundaries` job). These mirror the `rust.yml` and `reuse.yml` jobs; the UI-test job runs only in GitHub Actions, and `admission` runs only on pushes to `main`, so it is observed after landing and never as PR acceptance. The `clippy` command enforces the workspace `clippy.toml` once T3 lands. Every task ends with these green, and a task is `done` only after its squash commit's `main` runs are green (D-009).
- **Oracles:** spec tables are the oracle for geometry, tokens, labels, keyboard map and popup contents; generator-declared edit scripts are the oracle for diff alignment and unique counts; exhaustive state enumeration is the oracle for queue invariants; byte equality with the generated buffer is the oracle for copy; the reference PNGs are the oracle for the design review; spec §11.9 thresholds are the oracle for T1–T4.
- **Negative controls:** temporary Clippy canaries in fully qualified, imported, aliased, re-exported, multiline and `#[cfg(test)]` forms, and textual canary snippets, for the no-mutation guard; log files on both sides of the 7-day boundary with an unrelated file; each decision kind against each binding component with unchanged-binding controls; an or-later grant injected into About's own summary; a deliberately mismatched token value and a 1-unit layout offset beyond tolerance; queue attempts on reference, protected, kept, counterpart-queued and unacknowledged rows; stale comparison responses; a second confirmation submission; a cross-pane selection attempt; a zero-test snapshot run.
- **Test layers:** unit (domain, policy, generators), deterministic enumeration (queue invariants), contract (request/event revisions), headless `egui_kittest` interaction tests on any machine, `egui_kittest` snapshots on the lavapipe runner only, workstation-only measurement and glyph checks, and manual owner-performed desktop acceptance.
- **External environments:** GitHub-hosted Ubuntu with Mesa lavapipe, in the `rust.yml` UI-test job (T22); the reference workstation with GNOME on Wayland (T21, T24, T25, T26); KDE Plasma on Wayland on Fedora with two monitors at different scales, supplied by the owner (T26); Figma access or the owner for exports (T2).
- **Evidence:** repeatable test output is ephemeral. Workstation, manual, CI-citation and approval-bearing results are durable records `EV-001` to `EV-008` (Appendix C). Each of EV-001 to EV-004 and EV-006 starts with its C-14 binding block, so T27 can prove it describes the signed application (D-016). Evidence never contains user names, host names, home paths or other personal data.
- **Late failure:** a failed integration, measurement, review or manual check blocks that verification task. The executor appends a correction task with `corrects:` and `discovered_from:` against the owning task, states in its `invariants` which evidence records it invalidates (`invalidates: [EV-…]`, or `invalidates: none` with the reason), completes it, and reruns the blocked check. A correction that lands after a record's task is `done` leaves that record stale until T27 renews it or records its equivalence review (D-016). A T1–T4 miss that egui remediation cannot fix goes to the owner, who changes the design or the target in the spec before the plan is revised.

## 8. Execution Summary

| Task | Title | Disposition | Work Type | Phase | Depends On | Requirement(s) | Primary Proof | Parallel / Conflict |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| T1 | Record owner-approved MS-1 dependency rows | active | documentation | P0 | None | REQ-031 | PV-T1-001 | yes / T28 spec file |
| T2 | Export review-frame reference PNGs | active | documentation | P0 | T11 | REQ-003 | PV-T2-001 | yes / T11 REUSE.toml |
| T3 | Add the no-file-mutation guard | active | behavior | P0 | None | REQ-008 | PV-T3-001 | yes / T22 rust.yml |
| T4 | Define domain types, engine contract and fixture scenarios | active | behavior | P1 | None | REQ-020 | PV-T4-001 | yes / owns engine stubs used by T5–T9; D-014 with T6, T10, T11, T21, T22 |
| T5 | Implement the simulated scan job | active | behavior | P1 | T4 | REQ-022 | PV-T5-001 | yes / T4 stub |
| T6 | Implement the comparison service and 5 MB texts | active | behavior | P1 | T4, T1 | REQ-021, REQ-023, REQ-042 | PV-T6-002 | yes / T4 stub and engine manifest; D-014 with T4, T10, T11, T21, T22 |
| T7 | Implement the review and queue policy service | active | behavior | P1 | T4 | FR-025, FR-017, REQ-026, REQ-044 | PV-T7-001 | yes / T4 stub |
| T8 | Implement the simulated cleanup run | active | behavior | P1 | T7, T4 | REQ-025 | PV-T8-001 | yes / T4 stub |
| T9 | Add the design-sample fixture scenario | active | behavior | P1 | T2, T4 | REQ-038 | PV-T9-001 | yes / T4 stub |
| T10 | Implement the MS-1 platform adapters | active | behavior | P1 | T3 | NFR-008, REQ-027 | PV-T10-001 | yes / T21 platform files; D-014 with T4, T6, T11, T21, T22 |
| T11 | Build the theme, fonts and icons | active | behavior | P0 | None | REQ-029, REQ-030 | PV-T11-001 | yes / owns REUSE.toml used by T2 and T22; D-014 with T4, T6, T10, T21, T22 |
| T12 | Build the shell, presentation core and dialog host | active | behavior | P2 | T4, T10, T11 | FR-027, REQ-035, REQ-027, REQ-028, REQ-032 | PV-T12-002 | no / owns app stubs used by T13–T20 and T23 |
| T13 | Build the Findings workspace | active | behavior | P3 | T12, T7 | FR-018, REQ-014, REQ-037 | PV-T13-003 | yes / T12 stubs; T15 state files |
| T14 | Build the paired diff widget | active | behavior | P3 | T12, T6 | REQ-015, IR-006 | PV-T14-001 | yes / T12 stub |
| T15 | Build the Compare workspace and return routing | active | behavior | P3 | T12, T13, T14, T6, T7 | IR-001, REQ-013, REQ-016, REQ-036 | PV-T15-001 | no / T13 state files |
| T16 | Build the Matching rules and New scan popups | active | behavior | P3 | T12, T5, T7, T10 | REQ-017, FR-026 | PV-T16-001 | yes / T12 stub |
| T17 | Build the Review unique text and Queue identical copies popups | active | behavior | P3 | T12, T6, T7 | FR-023, REQ-033 | PV-T17-001 | yes / T12 stub |
| T18 | Build the cleanup dialog | active | behavior | P3 | T12, T8 | REQ-024, REQ-040, REQ-041 | PV-T18-001 | yes / T12 stub |
| T19 | Build the Keyboard shortcuts and About popups | active | behavior | P3 | T12, T10 | FR-028 | PV-T19-001 | yes / T12 stub |
| T20 | Persist preferences | active | behavior | P3 | T12, T13, T15, T16, T10 | DR-009 | PV-T20-001 | yes / T12 stub |
| T21 | Add system font fallback and prove exit item 7 | active | behavior | P3 | T1, T10, T11, T13 | REQ-007 | PV-T21-001 | yes / T10 platform files; D-014 with T4, T6, T10, T11, T22 |
| T22 | Add the UI-test job, layout assertions and integration suite | active | verification | P4 | T3, T9, T11, T13, T15, T16, T17, T18, T19, T20 | REQ-004, REQ-005, REQ-010, REQ-011, REQ-019, NFR-009, REQ-039, REQ-043 | PV-T22-002 | yes / T3 rust.yml, T11 manifest and REUSE.toml; D-014 with T4, T6, T10, T11, T21 |
| T23 | Add the measurement mode and scenario drivers | active | behavior | P4 | T5, T12, T13, T15, T16, T17, T18, T19 | REQ-034 | PV-T23-001 | yes / T12 stub |
| T24 | Record the design review | active | verification | P4 | T2, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14, T15, T16, T17, T18, T19, T20, T21, T22, T23 | REQ-002, REQ-012 | PV-T24-001 | yes / none |
| T25 | Measure T1–T4 on the reference workstation | active | verification | P4 | T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14, T15, T16, T17, T18, T19, T20, T21, T22, T23 | REQ-001 | PV-T25-001 | yes / none |
| T26 | Run the manual GNOME and KDE Plasma pass | active | verification | P4 | T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14, T15, T16, T17, T18, T19, T20, T21, T22, T23 | REQ-006, REQ-018 | PV-T26-001 | yes / none |
| T27 | Assemble the evidence file and obtain owner sign-off | active | verification | P5 | T1, T3, T21, T22, T24, T25, T26 | REQ-009, REQ-046 | PV-T27-001 | no / none |
| T28 | Prove MS-1 on `main` and land the close-out record | active | operational | P5 | T27 | REQ-045 | PV-T28-001 | no / T1 spec file |

## 9. Implementation Tasks

### Phase P0: Gates and external inputs

#### T1: Record owner-approved MS-1 dependency rows

- **disposition:** active
- **outcome:** the §8.3 approved-dependency table names the fontconfig lookup crate (name, exact version, features, license and why it was chosen) and adds rows for `unicode-normalization` (NFC and the Normalized view) and `proptest` (development only), merged through a PR the owner approved
- **work_type:** documentation
- **checkpoint:** one green documentation commit with the required `Plan-*` checkpoint trailers, landed by squash
- **boundary:** cross-task
- **depends_on:** []
- **dependency_reason:** none
- **requirements:** [REQ-031]
- **proof:** [PV-T1-001]
- **source_refs:** [request, spec:docs/specs/canonical/fuzzy-dedupe.md#approved-dependencies, spec:docs/specs/canonical/fuzzy-dedupe.md#117-centralized-visual-tokens-and-reusable-widgets, spec:docs/specs/canonical/fuzzy-dedupe.md#172-test-strategy]
- **consumes:** [§8.3 table at v0.9, cargo deny license allowlist]
- **produces:** [approved fontconfig crate, unicode-normalization row, proptest row, docs/specs/canonical/fuzzy-dedupe.md dependency rows]
- **preserves:** [every other §8.3 row and spec ID unchanged]
- **invariants:** [the PR description lists each candidate crate with license, maintenance status, dlopen versus link-time build needs on CI and the rexec worker, and Linux-only versus portable API; the owner's approval is recorded on the PR before landing]
- **executor_discretion:** [candidate research, table wording, revision-history entry wording]
- **files:** [`docs/specs/canonical/fuzzy-dedupe.md` (modify §8.3 table and revision history only; owner T1)]
- **parallel_safe:** yes
- **conflicts_with:** [T28]
- **supersedes:** []
- **superseded_by:** []
- **evidence:** [ephemeral]
- **recovery:** if the owner declines a row, block the consuming task (T21 for the fontconfig crate, T6 for `unicode-normalization` or `proptest`) with the owner's reason and leave the spec unchanged; if the PR is not approved, close it unmerged and keep T1 blocked
- **acceptance:** PV-T1-001 proves the merged §8.3 table names exactly one fontconfig lookup crate with version and license and holds the `unicode-normalization` and development-only `proptest` rows, and the PR carries the owner's approval before it squash-lands
- **sub-tasks:**
  - **T1.1 INVENTORY** — research fontconfig lookup crates against §8.3, §11.7 and the §17.2 license allowlist, preferring runtime-loaded fontconfig to avoid new build-time system packages; confirm the license and current version of `unicode-normalization` and `proptest`.
  - **T1.2 UPDATE** — edit only the §8.3 rows (fontconfig crate, `unicode-normalization`, development-only `proptest`) and the revision history; open the draft PR with the candidate comparison and ask the owner to choose and approve.
  - **T1.3 VERIFY REFERENCES** — run the repository's spec validation, Prettier and markdownlint commands on the spec file.
  - **T1.4 Verify Task** — after owner approval, record the checkpoint commit with the required `Plan-*` trailers and land with `gh-workflow land --pr N --method squash`; keep the branch.

#### T11: Build the theme, fonts and icons

- **disposition:** active
- **outcome:** `ui/theme.rs` is the single source of every §11.7 token in light and dark, the eight compact text styles on the four embedded Plex files, the 5-unit radius and geometry scale, and zoom from 80% to 200%; egui's bundled fonts are disabled and the embedded Noto Emoji is the emoji fallback; the used Lucide SVGs are embedded with `LICENSES/ISC.txt`; the app manifest declares every approved MS-1 app dependency (eframe, egui, egui_extras, egui_table, development-only egui_kittest, serde, toml, tracing, tracing-subscriber, tracing-appender) at exact pins, each with its C-7 entry for `fuzzy-dedupe-app`
- **work_type:** behavior
- **checkpoint:** one green commit with the required `Plan-*` checkpoint trailers, landed by squash
- **boundary:** cross-task
- **depends_on:** []
- **dependency_reason:** none
- **requirements:** [REQ-029, REQ-030]
- **proof:** [PV-T11-001, PV-T11-002]
- **source_refs:** [spec:docs/specs/canonical/fuzzy-dedupe.md#117-centralized-visual-tokens-and-reusable-widgets, spec:docs/specs/canonical/fuzzy-dedupe.md#approved-dependencies, spec:docs/specs/canonical/fuzzy-dedupe.md#183-deployment-flow, adr:docs/adr/adr-0001-use-egui-for-the-v1-desktop-ui.md#amendments]
- **consumes:** [MS-0 Plex files and notices, approved MS-1 app crate rows, MS-0 C-5 REUSE coverage, MS-0 C-7 boundary policy, Cargo.lock]
- **produces:** [theme adapter, app manifest, REUSE.toml asset annotations]
- **preserves:** [the four Plex files byte-identical; `deny.toml` unchanged and the `deny` job green with egui's default fonts disabled (D-001); every crate root keeps `#![forbid(unsafe_code)]`]
- **invariants:** [no hex color literal outside `theme.rs`; Noto Emoji follows Plex in every family; system fallback families append after Noto Emoji and are supplied by the caller, never looked up inside the theme; T11 owns the root `REUSE.toml` and `THIRD-PARTY-NOTICES.md` for MS-1, and T2 and T22 add their annotations after it; annotations follow MS-0 D-012 by origin, so Lucide icons are class d with the Lucide holder and ISC and Noto Emoji is class d with its holder and OFL-1.1; no annotation glob matches a MS-0 C-5 protected path or `clippy.toml`; GUI crates are declared only by `fuzzy-dedupe-app`]
- **executor_discretion:** [theme API shape, icon set selection by frame inspection, zoom step mechanics within A-004]
- **files:** [`crates/fuzzy-dedupe-app/Cargo.toml` (modify; owner T11), `Cargo.lock` (modify minimal lock delta only, D-014; owner T4), `.github/boundaries.toml` (modify own third-party entries only, C-7; owner T4), `crates/fuzzy-dedupe-app/src/ui/theme.rs` (create; owner T11), `crates/fuzzy-dedupe-app/assets/icons/**` (create; owner T11), `crates/fuzzy-dedupe-app/assets/fonts/NotoEmoji-Regular.ttf` (create; owner T11), `REUSE.toml` (modify; owner T11), `THIRD-PARTY-NOTICES.md` (modify; owner T11), `LICENSES/ISC.txt` (create; owner T11), `crates/fuzzy-dedupe-app/tests/theme_tokens.rs` (test; owner T11)]
- **parallel_safe:** yes
- **conflicts_with:** [T2, T22, T4, T6, T10, T21]
- **supersedes:** []
- **superseded_by:** []
- **evidence:** [ephemeral]
- **recovery:** revert to the last green checkpoint; if no egui_table release supports the newest egui, pin egui to the newest version egui_table and egui_kittest support and record it in the PR
- **acceptance:** PV-T11-001 proves every token in both modes equals the §11.7 table, the eight styles have the stated family, size and line height, no style is below 12 units, and every text and background pairing the theme declares meets 4.5:1 by computed WCAG contrast; PV-T11-002 proves each embedded icon is referenced by the UI inventory, the Noto Emoji file's checksum equals the pinned release and the value in `THIRD-PARTY-NOTICES.md`, egui's default-fonts feature is off in the resolved graph, `reuse lint` passes with the icon and emoji annotations in MS-0 D-012 class d, the notices name the Lucide and Noto Emoji releases with ISC and OFL-1.1, and the `boundaries` job passes with a C-7 entry for each app crate while the reviewed lock diff adds only those crates and their transitive closure
- **sub-tasks:**
  - **T11.1 RED** — add token, style and contrast tests transcribed from §11.7; expected failure: no theme exists.
  - **T11.2 Verify RED** — run `rexec -- cargo nextest run -p fuzzy-dedupe-app --locked -E 'test(theme_tokens)'`.
  - **T11.3 GREEN** — implement the theme, embed fonts and icons, add notices, pin dependencies.
  - **T11.4 Verify GREEN** — rerun targeted tests, `cargo deny check`, `cargo audit` and `reuse lint` (rexec for the cargo commands).
  - **T11.5 REFACTOR** — assess and record none.
  - **T11.6 Verify Task** — run PV-T11-001 and PV-T11-002 and repository gates; before landing, rebase on the current `main`, update `Cargo.lock` minimally without `--locked`, review the lock diff, add the C-7 entries and pass the `--locked` gates and the `boundaries` job against the combined manifests (D-014); commit with the required `Plan-*` trailers.

#### T2: Export review-frame reference PNGs

- **disposition:** active
- **outcome:** `docs/design/reference/` holds one PNG per review frame (19 frames) and a manifest recording frame name, node ID, export scale, pixel size, SHA-256, export date and export route
- **work_type:** documentation
- **checkpoint:** one green documentation commit with the required `Plan-*` checkpoint trailers, landed by squash
- **boundary:** cross-task
- **depends_on:** [T11]
- **dependency_reason:** adds the PNG annotation to the root `REUSE.toml`, which T11 owns as the app manifest and notice aggregator, so T2 serializes after T11
- **requirements:** [REQ-003]
- **proof:** [PV-T2-001]
- **source_refs:** [spec:docs/specs/canonical/fuzzy-dedupe.md#111-authoritative-design-links-and-revision-precedence, spec:docs/specs/canonical/fuzzy-dedupe.md#85-design-constraints, spec:docs/specs/canonical/fuzzy-dedupe.md#183-deployment-flow]
- **consumes:** [Figma file d5tA6ap1tH4cTDaJiwve89, §11.1 review-frame list, app manifest]
- **produces:** [reference PNG set]
- **preserves:** [§11.1 node IDs and inspection date unchanged]
- **invariants:** [the 19 frames are exactly the §11.1 review list (Appendix A C-11); exports are 1× PNG of the whole frame; the repository is public, so the PNGs are published as the spec already chose; `REUSE.toml` declares them project-owned (L3Digital LLC) and `GPL-3.0-only`, MS-0 D-012 class a (§11.1 v0.9), through a path annotation that matches only `docs/design/reference/*.png` and no MS-0 C-5 protected path]
- **executor_discretion:** [file naming scheme, manifest layout]
- **files:** [`docs/design/reference/*.png` (create; owner T2), `docs/design/reference/README.md` (create manifest with SPDX lines; owner T2), `REUSE.toml` (modify PNG annotation only; owner T11)]
- **parallel_safe:** yes
- **conflicts_with:** [T11, T22]
- **supersedes:** []
- **superseded_by:** []
- **evidence:** [ephemeral]
- **recovery:** if Figma MCP cannot produce PNG bytes, use the Figma images REST endpoint with a token the owner authorizes by reference only, or ask the owner to export the frames at 1× from Figma; if a node ID no longer exists, stop and report it as a §11.1 defect to the owner
- **acceptance:** PV-T2-001 proves the directory holds 19 PNGs whose node IDs match the §11.1 review list one to one, each manifest checksum matches its file, the manifest names the export route actually used, and `reuse lint` passes
- **sub-tasks:**
  - **T2.1 INVENTORY** — list the 19 review frames from §11.1 with node IDs; confirm each exists in the file.
  - **T2.2 UPDATE** — export by the first available route (Figma MCP, authorized REST export, owner export); write the manifest and add the `GPL-3.0-only` PNG annotation to `REUSE.toml`.
  - **T2.3 VERIFY REFERENCES** — recompute checksums, compare node IDs with §11.1, run `reuse lint`, Prettier and markdownlint.
  - **T2.4 Verify Task** — land through the D-009 route and record the checkpoint commit with the required `Plan-*` trailers.

#### T3: Add the no-file-mutation guard

- **disposition:** active
- **outcome:** a workspace `clippy.toml` lists the C-09 disallowed methods and types, so the existing `clippy` job (`-D warnings`) rejects every C-09 route outside its allowlist in any import, alias or re-export form; a workspace test, run by the `test` job, keeps that configuration complete and alone, confines `#[expect(clippy::disallowed_*)]` allowances to C-09's allowlist, rejects `allow` forms, textually rejects the C-09 routes in non-test source in the forms C-09 lists, and fails when the lockfile or any manifest names a Trash crate; the `test` job drops nextest's `--no-tests=warn`
- **work_type:** behavior
- **checkpoint:** one green commit with the required `Plan-*` checkpoint trailers, landed by squash
- **boundary:** cross-task
- **depends_on:** []
- **dependency_reason:** none
- **requirements:** [REQ-008]
- **proof:** [PV-T3-001, PV-T3-002]
- **source_refs:** [spec:docs/specs/canonical/fuzzy-dedupe.md#19-implementation-plan, spec:docs/specs/canonical/fuzzy-dedupe.md#85-design-constraints]
- **consumes:** [MS-0 workspace layout, Cargo.lock, rust.yml test job]
- **produces:** [C-09, clippy.toml, rust.yml without no-tests=warn]
- **preserves:** [every other `rust.yml` job unchanged, including the `clippy` command; no manifest change; `Cargo.lock` unchanged]
- **invariants:** [the rule set, allowlist and test-code policy in Appendix A C-09 are binding; `clippy.toml` sits at the workspace root, carries the SPDX pair (MS-0 D-008) and is the only Clippy configuration file in the repository; no `REUSE.toml` annotation matches it; allowances are `expect` with a `reason`, never `allow`; the textual layer is a bounded second layer and the Clippy layer is authoritative for path resolution]
- **executor_discretion:** [textual scanning technique within C-09's supported syntax, test file organization, failure message wording, `clippy.toml` entries beyond C-09's minimum lists]
- **files:** [`clippy.toml` (create; owner T3), `crates/fuzzy-dedupe-app/tests/no_file_mutation.rs` (create; owner T3), `.github/workflows/rust.yml` (modify `test` job flag only; owner T3)]
- **parallel_safe:** yes
- **conflicts_with:** [T22]
- **supersedes:** []
- **superseded_by:** []
- **evidence:** [ephemeral]
- **recovery:** revert to the last green checkpoint; the guard adds no production code
- **acceptance:** PV-T3-001 proves the textual guard passes on the clean workspace; fails on each C-09 textual canary (fully qualified, imported, aliased, re-exported, multiline, inside a `#[cfg(test)]` module under `src/`, `.rename(` method form, `std::process::Command`); accepts the same names inside a string literal or comment and an allowlisted call only at its allowlisted path with its `expect`; fails on an `expect` outside the allowlist, any `allow(clippy::disallowed_methods)` or `allow(clippy::disallowed_types)`, a second Clippy configuration file, a `clippy.toml` missing a C-09 minimum entry, a manifest or workspace lint table that lowers either lint, and a lockfile fixture or manifest containing a `trash` package; and the `test` job runs nextest without `--no-tests=warn` and passes. PV-T3-002 proves `rexec -- cargo clippy --workspace --all-targets --locked -- -D warnings` passes on the clean workspace and fails on each temporary C-09 Clippy canary, including the imported, aliased, re-exported, multiline and `#[cfg(test)]` forms, an allowlisted call without its `expect`, and an `expect` with nothing to fulfil, while a canary at an allowlisted path with its `expect` passes; canaries are reverted and never committed
- **sub-tasks:**
  - **T3.1 RED** — add detector tests that feed each C-09 textual canary and configuration canary and expect detection; expected failure: no detector or `clippy.toml` exists.
  - **T3.2 Verify RED** — run `rexec -- cargo nextest run -p fuzzy-dedupe-app --locked -E 'test(no_file_mutation)'` and confirm the failures are missing detection, not build errors.
  - **T3.2a CONFIGURE** — add `clippy.toml` with the C-09 entries and the SPDX pair; remove `--no-tests=warn` from the `test` job in `rust.yml`, the first MS-1 test now existing.
  - **T3.3 GREEN** — implement the detector, the configuration checks and the workspace scan.
  - **T3.4 Verify GREEN** — rerun the targeted tests, the full nextest gate and the Clippy gate.
  - **T3.5 PROVE REJECTION** — apply each PV-T3-002 Clippy canary as a temporary change, confirm the Clippy gate fails naming `disallowed_methods` or `disallowed_types` (or `unfulfilled_lint_expectations`), revert it, and rerun the clean gate.
  - **T3.6 Verify Task** — run PV-T3-001 and PV-T3-002 and the repository gates; commit with the required `Plan-*` trailers.

### Phase P1: Engine, fixtures and platform

#### T4: Define domain types, engine contract and fixture scenarios

- **disposition:** active
- **outcome:** the domain crate holds the MS-1 types of C-01; the engine exposes the request/event facade of C-02 with stub modules for T5–T8 and declares `crossbeam-channel` and `similar` with their C-7 entries; deterministic scenarios `large` (exactly 100,000 flattened rows with all groups expanded), `mixed`, `issues`, `empty`, `unicode` and `stale` are available, with every counterpart assigned by the §10.1 rule
- **work_type:** behavior
- **checkpoint:** one green commit with the required `Plan-*` checkpoint trailers, landed by squash
- **boundary:** cross-task
- **depends_on:** []
- **dependency_reason:** none
- **requirements:** [REQ-020]
- **proof:** [PV-T4-001]
- **source_refs:** [spec:docs/specs/canonical/fuzzy-dedupe.md#118-recommended-rust-ui-modules-and-state-ownership, spec:docs/specs/canonical/fuzzy-dedupe.md#grouping-policy, spec:docs/specs/canonical/fuzzy-dedupe.md#6-glossary, spec:docs/specs/canonical/fuzzy-dedupe.md#114-findings-table-structure-and-interactions, spec:docs/specs/canonical/fuzzy-dedupe.md#85-design-constraints]
- **consumes:** [MS-0 domain and engine crates, crossbeam-channel row, similar row, MS-0 C-7 boundary policy, Cargo.lock]
- **produces:** [C-01, C-02, engine module stubs]
- **preserves:** [domain depends on no workspace crate and no GUI or OS type; engine does not depend on the app crate; the C-1 edge table and the GUI and OS crate lists in `.github/boundaries.toml` unchanged]
- **invariants:** [IDs are stable for the life of a scan; every group lists its reference first; every member's counterpart is the reference whenever the member has a direct-match edge to it and otherwise the member with its best direct match (§6, §10.1); no group carries a group-wide percentage; each member's declared Unique value is the §10.1 unique-line count against its counterpart; generation is deterministic for a fixed seed without an RNG crate]
- **executor_discretion:** [private helper names, generator algorithm, scenario selection mechanism for developers]
- **files:** [`crates/fuzzy-dedupe-domain/src/**` (create/modify; owner T4), `crates/fuzzy-dedupe-engine/Cargo.toml` (modify; owner T4), `Cargo.lock` (modify minimal lock delta, D-014; owner T4), `.github/boundaries.toml` (modify third-party entries, C-7; owner T4), `crates/fuzzy-dedupe-engine/src/lib.rs` (modify; owner T4), `crates/fuzzy-dedupe-engine/src/service.rs` (create; owner T4), `crates/fuzzy-dedupe-engine/src/fixtures/mod.rs` (create; owner T4), `crates/fuzzy-dedupe-engine/src/fixtures/scenarios.rs` (create; owner T4), `crates/fuzzy-dedupe-engine/src/fixtures/design.rs` (create stub; owner T4), `crates/fuzzy-dedupe-engine/src/scan_sim/mod.rs` (create stub; owner T4), `crates/fuzzy-dedupe-engine/src/compare/mod.rs` (create stub; owner T4), `crates/fuzzy-dedupe-engine/src/review/mod.rs` (create stub; owner T4), `crates/fuzzy-dedupe-engine/src/cleanup_sim/mod.rs` (create stub; owner T4), `crates/fuzzy-dedupe-engine/tests/fixtures_contract.rs` (test; owner T4)]
- **parallel_safe:** yes
- **conflicts_with:** [T5, T6, T7, T8, T9, T10, T11, T21, T22]
- **supersedes:** []
- **superseded_by:** []
- **evidence:** [ephemeral]
- **recovery:** revert to the last green checkpoint; no consumer exists until T4 lands
- **acceptance:** PV-T4-001 proves the `large` scenario expands to exactly 100,000 flattened rows containing every relation type, Mixed groups, protected, kept-eligible and counterpart-chained members; the `mixed` scenario holds an identical pair that is only similar to a protected reference and a containment pair that excludes the reference (FR-013); an independent recomputation of the counterpart rule from each group's edges equals every assigned counterpart; two generations with the same seed are equal; every C-01 invariant holds over all scenarios; the stubs return a typed not-implemented outcome rather than panicking; and the `boundaries` job passes with C-7 entries for `crossbeam-channel` and `similar` declared by `fuzzy-dedupe-engine` while the reviewed lock diff adds only those crates and their transitive closure
- **sub-tasks:**
  - **T4.1 RED** — add contract tests for row count, determinism and C-01 invariants; expected failure: no types or scenarios exist.
  - **T4.2 Verify RED** — run `rexec -- cargo nextest run -p fuzzy-dedupe-engine --locked` and confirm missing-behavior failures.
  - **T4.3 GREEN** — implement C-01, the C-02 facade, the scenarios and the stub modules.
  - **T4.4 Verify GREEN** — rerun targeted tests, clippy and the dependency-direction gate.
  - **T4.5 REFACTOR** — assess and record none.
  - **T4.6 Verify Task** — run PV-T4-001 and repository gates; before landing, rebase on the current `main`, update `Cargo.lock` minimally without `--locked`, review the lock diff, add the C-7 entries and pass the `--locked` gates and the `boundaries` job against the combined manifests (D-014); commit with the required `Plan-*` trailers.

#### T5: Implement the simulated scan job

- **disposition:** active
- **outcome:** `StartScan` snapshots the scan setup and matching rules immutably under a new scan ID, saturates the worker threads, streams coalesced progress, and completes with the selected scenario; `CancelScan` is acknowledged within 1 s with partial coverage
- **work_type:** behavior
- **checkpoint:** one green commit with the required `Plan-*` checkpoint trailers, landed by squash
- **boundary:** cross-task
- **depends_on:** [T4]
- **dependency_reason:** consumes C-01, C-02 and the engine module stubs from T4 and fills the `scan_sim` stub
- **requirements:** [REQ-022]
- **proof:** [PV-T5-001]
- **source_refs:** [spec:docs/specs/canonical/fuzzy-dedupe.md#116-popup-contracts-and-compact-progress-handling, spec:docs/specs/canonical/fuzzy-dedupe.md#119-rendering-asynchronous-work-and-scaling, spec:docs/specs/canonical/fuzzy-dedupe.md#ms-1-performance-targets]
- **consumes:** [C-01, C-02, engine module stubs]
- **produces:** [C-03]
- **preserves:** [C-02 shapes unchanged]
- **invariants:** [one worker per available core; progress events are bounded and coalesced; results from different scan IDs never merge; completion or partial status is emitted only after worker acknowledgment; no filesystem traversal]
- **executor_discretion:** [busy-work design, coalescing interval, thread naming]
- **files:** [`crates/fuzzy-dedupe-engine/src/scan_sim/mod.rs` (modify; owner T4), `crates/fuzzy-dedupe-engine/src/scan_sim/job.rs` (create; owner T5), `crates/fuzzy-dedupe-engine/tests/scan_sim.rs` (test; owner T5)]
- **parallel_safe:** yes
- **conflicts_with:** [T4]
- **supersedes:** []
- **superseded_by:** []
- **evidence:** [ephemeral]
- **recovery:** revert to the last green checkpoint; the stub remains in place
- **acceptance:** PV-T5-001 proves progress counts increase monotonically, cancel acknowledgment arrives within 1 s under saturation in 20 consecutive runs, the cancelled result is marked partial, a stale scan ID's late events are tagged so the consumer can discard them, and the stored setup copy equals the setup at start even after the caller mutates its own copy
- **sub-tasks:**
  - **T5.1 RED** — add tests for progress, cancellation latency, partial coverage and immutability; expected failure: stub returns not implemented.
  - **T5.2 Verify RED** — run `rexec -- cargo nextest run -p fuzzy-dedupe-engine --locked -E 'test(scan_sim)'`.
  - **T5.3 GREEN** — implement the job.
  - **T5.4 Verify GREEN** — rerun targeted and engine tests.
  - **T5.5 REFACTOR** — assess and record none.
  - **T5.6 Verify Task** — run PV-T5-001 and repository gates; commit with the required `Plan-*` trailers.

#### T6: Implement the comparison service and 5 MB texts

- **disposition:** active
- **outcome:** `LoadComparison` returns, off the UI thread, the aligned-row model of C-04 computed with `similar` for any fixture pair, including the two generated 5 MB texts, together with the §10.1 unique-line measure and the unique lines themselves; superseded requests are cancelled or ignored; results are keyed by both snapshots, the rules revision and the view mode
- **work_type:** behavior
- **checkpoint:** one green commit with the required `Plan-*` checkpoint trailers, landed by squash
- **boundary:** cross-task
- **depends_on:** [T4, T1]
- **dependency_reason:** consumes C-01 member identities and declared unique counts, C-02 and the engine module stubs from T4, and the unicode-normalization row and proptest row approved in T1
- **requirements:** [REQ-021, REQ-023, REQ-042]
- **proof:** [PV-T6-001, PV-T6-002, PV-T6-003]
- **source_refs:** [spec:docs/specs/canonical/fuzzy-dedupe.md#115-comparison-large-aligned-read-only-panes, spec:docs/specs/canonical/fuzzy-dedupe.md#ms-1-performance-targets, spec:docs/specs/canonical/fuzzy-dedupe.md#73-interface-requirements]
- **consumes:** [C-01, C-02, engine module stubs, unicode-normalization row, proptest row]
- **produces:** [C-04]
- **preserves:** [C-02 shapes unchanged]
- **invariants:** [the unique-line measure is content-based per §10.1 and is the only source of Only-in counts and unique lines; it is independent of the diff alignment; spacer rows carry no source line number; original line endings are retained in the model; the Normalized view applies Unicode NFC through `unicode-normalization` and then the scan's enabled Ignore options to both sides, and labels its counts as normalized]
- **executor_discretion:** [generator algorithm, inline-change granularity, cache structure]
- **files:** [`crates/fuzzy-dedupe-engine/src/compare/mod.rs` (modify; owner T4), `crates/fuzzy-dedupe-engine/src/compare/texts.rs` (create; owner T6), `crates/fuzzy-dedupe-engine/src/compare/aligned.rs` (create; owner T6), `crates/fuzzy-dedupe-engine/src/compare/normalize.rs` (create; owner T6), `crates/fuzzy-dedupe-engine/Cargo.toml` (modify to add `unicode-normalization` and dev-only `proptest`; owner T4), `Cargo.lock` (modify minimal lock delta only, D-014; owner T4), `.github/boundaries.toml` (modify own third-party entries only, C-7; owner T4), `crates/fuzzy-dedupe-engine/tests/compare.rs` (test; owner T6)]
- **parallel_safe:** yes
- **conflicts_with:** [T4, T10, T11, T21, T22]
- **supersedes:** []
- **superseded_by:** []
- **evidence:** [ephemeral]
- **recovery:** revert to the last green checkpoint; the stub remains in place
- **acceptance:** PV-T6-001 proves each generated large text is 5 MB ± 5% with 100,000 ± 1% lines and 10% ± 1% changed lines, that small pairs include CRLF and mixed line endings and non-NFC input, and that every fixture member's declared Unique value equals the computed unique-line count for its pair; PV-T6-002 proves computed alignment equals the generator's edit script, Normalized mode equals NFC plus the enabled Ignore options under `proptest` properties, a response for a superseded request is never delivered as current, and the work runs on a worker thread; PV-T6-003 proves on the §17.3 UI check 06 fixtures that reordered identical lines count zero despite diff changes, a line repeated in the file and present once in the counterpart counts zero, a repeated absent line counts once per occurrence, CRLF input and a missing final newline change nothing, the normalized count is labeled, and the copy text is the unique lines in file order each followed by one LF; the `boundaries` job passes with C-7 entries for `unicode-normalization` and dev-only `proptest` declared by `fuzzy-dedupe-engine`, and the reviewed lock diff adds only those crates and their transitive closure
- **sub-tasks:**
  - **T6.1 RED** — add tests comparing computed alignment and counts with generator edit scripts, and stale-response tests; expected failure: stub returns not implemented.
  - **T6.2 Verify RED** — run `rexec -- cargo nextest run -p fuzzy-dedupe-engine --locked -E 'test(compare)'`.
  - **T6.3 GREEN** — implement generators, the diff, the aligned model and cancellation.
  - **T6.4 Verify GREEN** — rerun targeted and engine tests.
  - **T6.5 REFACTOR** — assess and record none.
  - **T6.6 Verify Task** — run PV-T6-001 to PV-T6-003 and repository gates; before landing, rebase on the current `main`, update `Cargo.lock` minimally without `--locked`, review the lock diff, add the C-7 entries and pass the `--locked` gates and the `boundaries` job against the combined manifests (D-014); commit with the required `Plan-*` trailers.

#### T7: Implement the review and queue policy service

- **disposition:** active
- **outcome:** the engine review service of C-05 evaluates every queue entry point and owns the scan-scoped decision set, enforcing FR-025 in both queue orders, FR-017, review-first, the C-05 binding and invalidation rule for each decision kind (Kept, per-file Protect, unique-text acknowledgment) and for queue approvals, the §10.1 counterpart rule including counterparts chosen by reviewing in Compare, the ordered Make this the reference transition, and invalidation when a fixture-injected evidence change alters a binding component
- **work_type:** behavior
- **checkpoint:** one green commit with the required `Plan-*` checkpoint trailers, landed by squash
- **boundary:** cross-task
- **depends_on:** [T4]
- **dependency_reason:** consumes the C-01 group, member and edge model, C-02 and the engine module stubs from T4, and fills the `review` stub
- **requirements:** [FR-025, FR-017, REQ-026, REQ-044]
- **proof:** [PV-T7-001, PV-T7-002, PV-T7-003, PV-T7-004]
- **source_refs:** [spec:docs/specs/canonical/fuzzy-dedupe.md#grouping-policy, spec:docs/specs/canonical/fuzzy-dedupe.md#114-findings-table-structure-and-interactions, spec:docs/specs/canonical/fuzzy-dedupe.md#118-recommended-rust-ui-modules-and-state-ownership, spec:docs/specs/canonical/fuzzy-dedupe.md#6-glossary, spec:docs/specs/canonical/fuzzy-dedupe.md#74-data-requirements]
- **consumes:** [C-01, C-02, engine module stubs]
- **produces:** [C-05]
- **preserves:** [C-02 shapes unchanged]
- **invariants:** [C-05 invariants I-1 to I-10 hold after every operation sequence]
- **executor_discretion:** [internal data structures, refusal message wording within §11.4 labels]
- **files:** [`crates/fuzzy-dedupe-engine/src/review/mod.rs` (modify; owner T4), `crates/fuzzy-dedupe-engine/src/review/policy.rs` (create; owner T7), `crates/fuzzy-dedupe-engine/tests/review_policy.rs` (test; owner T7)]
- **parallel_safe:** yes
- **conflicts_with:** [T4]
- **supersedes:** []
- **superseded_by:** []
- **evidence:** [ephemeral]
- **recovery:** revert to the last green checkpoint; the stub remains in place
- **acceptance:** PV-T7-001 proves by exhaustive enumeration that I-1 to I-4 hold for every group shape up to 5 members and every operation sequence up to depth 6, and that in both queue orders queuing a file whose counterpart is queued is refused with "Counterpart queued" and queuing a file that a queued file was measured against is refused with "Counterpart of a queued file", leaving the queue unchanged; PV-T7-002 proves Kept and Protected follow file IDs across sort-independent queries, no operation queues a protected file, and Queue identical copies skips kept, protected, reference and non-byte-identical members; PV-T7-003 proves, for each decision kind (Kept, per-file Protect, acknowledgment) and each binding component (target snapshot, counterpart snapshot, rules revision), that a fixture-injected change of that component alone invalidates that decision and no decision with an unchanged binding; that a counterpart change through Make this the reference or a Compare review invalidates the decisions bound to the old counterpart; that an evidence update repeating the current values, or changing only another member, leaves every decision intact; that a queued target whose acknowledgment or approval is invalidated leaves the queue; that `ClearForNewScan` empties every decision of every kind; and that folder protection from the scan setup survives every binding change and the new scan; PV-T7-004 proves Make this the reference on a queued member removes it from the queue, recomputes every counterpart by the §10.1 rule while discarding Compare-chosen counterparts, unqueues each other queued member whose counterpart changed and clears its acknowledgment, leaves members with an unchanged counterpart untouched, and returns the unqueued count
- **sub-tasks:**
  - **T7.1 RED** — add enumeration and decision-binding tests; expected failure: stub returns not implemented.
  - **T7.2 Verify RED** — run `rexec -- cargo nextest run -p fuzzy-dedupe-engine --locked -E 'test(review_policy)'`.
  - **T7.3 GREEN** — implement the service.
  - **T7.4 Verify GREEN** — rerun targeted and engine tests.
  - **T7.5 REFACTOR** — assess and record none.
  - **T7.6 Verify Task** — run PV-T7-001 to PV-T7-004 and repository gates; commit with the required `Plan-*` trailers.

#### T8: Implement the simulated cleanup run

- **disposition:** active
- **outcome:** `BuildCleanupPlan` builds a plan from the queue with fixture-defined Trash support, `ExecuteApprovedPlan` runs it on a worker over IDs only with scripted outcomes (moved, failed, blocked, stopped, uncertain), `StopCleanup` stops it, and a targeted retry after simulated reconciliation and revalidation never replays completed items, all following C-06
- **work_type:** behavior
- **checkpoint:** one green commit with the required `Plan-*` checkpoint trailers, landed by squash
- **boundary:** cross-task
- **depends_on:** [T7, T4]
- **dependency_reason:** consumes C-05 queued targets and counterparts from T7 and the `cleanup_sim` stub from the T4 engine module stubs
- **requirements:** [REQ-025]
- **proof:** [PV-T8-001]
- **source_refs:** [spec:docs/specs/canonical/fuzzy-dedupe.md#104-state-transitions, spec:docs/specs/canonical/fuzzy-dedupe.md#116-popup-contracts-and-compact-progress-handling]
- **consumes:** [C-05, engine module stubs]
- **produces:** [C-06]
- **preserves:** [the review decision set is read, not changed, except that moved targets are reported]
- **invariants:** [one confirmation produces one execution; a stale precondition stops the run before that file (`stopped`, "Stopped: changed"); a simulated Trash failure stops it after that file's failed move (`failed`, ERR-003); either way the item carries its reason and leaves the plan, and failures count as failed in the summary; a stop ends the authorization; items already moved stay reported as moved; unreached items return to `planned`; there is no `skipped` state; `needs_reconciliation` items are retried only after simulated reconciliation; retry excludes succeeded items; a per-item journal of the run is kept in memory; no filesystem call exists in this module]
- **executor_discretion:** [script format for fixture outcomes, timing of simulated moves]
- **files:** [`crates/fuzzy-dedupe-engine/src/cleanup_sim/mod.rs` (modify; owner T4), `crates/fuzzy-dedupe-engine/src/cleanup_sim/run.rs` (create; owner T8), `crates/fuzzy-dedupe-engine/tests/cleanup_sim.rs` (test; owner T8)]
- **parallel_safe:** yes
- **conflicts_with:** [T4]
- **supersedes:** []
- **superseded_by:** []
- **evidence:** [ephemeral]
- **recovery:** revert to the last green checkpoint; the stub remains in place
- **acceptance:** PV-T8-001 proves every legal transition of C-06 occurs with the right dialog status, an illegal transition is rejected, a second `ExecuteApprovedPlan` with the same authorization is refused, the ERR-003 schedule (a scripted Trash failure mid-run and, separately, a change to a later target) stops the run each time with the failed item `failed` after its move attempt and the changed item `stopped` before it, both recorded with reasons and the rest returned as a revised plan, a blocked item is excluded from the confirmed count, and a retry after reconciliation re-executes no succeeded item
- **sub-tasks:**
  - **T8.1 RED** — add transition-table tests and double-submission tests; expected failure: stub returns not implemented.
  - **T8.2 Verify RED** — run `rexec -- cargo nextest run -p fuzzy-dedupe-engine --locked -E 'test(cleanup_sim)'`.
  - **T8.3 GREEN** — implement the simulation.
  - **T8.4 Verify GREEN** — rerun targeted and engine tests.
  - **T8.5 REFACTOR** — assess and record none.
  - **T8.6 Verify Task** — run PV-T8-001 and repository gates; commit with the required `Plan-*` trailers.

#### T9: Add the design-sample fixture scenario

- **disposition:** active
- **outcome:** a `design` scenario reproduces the sample rows, group labels, member labels, decision states, scan progress counts and cleanup-dialog rows visible in the review frames, and is the default scenario for snapshots and the design review
- **work_type:** behavior
- **checkpoint:** one green commit with the required `Plan-*` checkpoint trailers, landed by squash
- **boundary:** cross-task
- **depends_on:** [T2, T4]
- **dependency_reason:** transcribes sample data from the T2 reference PNG set into C-01 values and fills the `fixtures/design.rs` stub from the T4 engine module stubs
- **requirements:** [REQ-038]
- **proof:** [PV-T9-001]
- **source_refs:** [spec:docs/specs/canonical/fuzzy-dedupe.md#111-authoritative-design-links-and-revision-precedence, spec:docs/specs/canonical/fuzzy-dedupe.md#114-findings-table-structure-and-interactions]
- **consumes:** [reference PNG set, C-01, engine module stubs]
- **produces:** [design scenario]
- **preserves:** [sample values are fixtures only and set no production default (§11.1 Prototype limitation)]
- **invariants:** [C-01 invariants hold; any frame value that would violate a C-01 invariant is reported to the owner, not encoded]
- **executor_discretion:** [data encoding style]
- **files:** [`crates/fuzzy-dedupe-engine/src/fixtures/design.rs` (modify; owner T4), `crates/fuzzy-dedupe-engine/tests/design_scenario.rs` (test; owner T9)]
- **parallel_safe:** yes
- **conflicts_with:** [T4]
- **supersedes:** []
- **superseded_by:** []
- **evidence:** [ephemeral]
- **recovery:** revert to the last green checkpoint; record any frame inconsistency for the owner
- **acceptance:** PV-T9-001 proves a table of expected values transcribed from the frames (group labels and counts, file names, Match and Decision labels, filter counts, scan progress numbers, cleanup rows) equals the scenario's projected values, and the scenario satisfies every C-01 invariant
- **sub-tasks:**
  - **T9.1 RED** — add the expected-value table test; expected failure: the stub scenario is empty.
  - **T9.2 Verify RED** — run `rexec -- cargo nextest run -p fuzzy-dedupe-engine --locked -E 'test(design_scenario)'`.
  - **T9.3 GREEN** — encode the scenario.
  - **T9.4 Verify GREEN** — rerun targeted and engine tests.
  - **T9.5 Verify Task** — run PV-T9-001 and repository gates; commit with the required `Plan-*` trailers.

#### T10: Implement the MS-1 platform adapters

- **disposition:** active
- **outcome:** the platform crate exposes the C-07 traits with Linux implementations for per-user directories (`etcetera`), the folder picker (`rfd`, XDG portal backend), open location (`xdg-open` for a directory or `https` URL), the app-owned file writer and the age-based expiry of the application's own log files (D-015), plus a font-lookup stub that returns no fonts until T21; `etcetera` and `rfd` carry their C-7 entries for `fuzzy-dedupe-platform`
- **work_type:** behavior
- **checkpoint:** one green commit with the required `Plan-*` checkpoint trailers, landed by squash
- **boundary:** cross-task
- **depends_on:** [T3]
- **dependency_reason:** consumes C-09 from T3, the workspace `clippy.toml` and the guard test: T10's allowlisted modules carry `#[expect(clippy::disallowed_*)]` allowances that fail as unfulfilled expectations, and its acceptance runs the guard and the Clippy gate, so T10 starts only once T3 is green on `main`
- **requirements:** [NFR-008, REQ-027]
- **proof:** [PV-T10-001, PV-T10-002]
- **source_refs:** [spec:docs/specs/canonical/fuzzy-dedupe.md#85-design-constraints, spec:docs/specs/canonical/fuzzy-dedupe.md#72-non-functional-requirements, spec:docs/specs/canonical/fuzzy-dedupe.md#approved-dependencies, spec:docs/specs/canonical/fuzzy-dedupe.md#182-configuration]
- **consumes:** [MS-0 platform crate, etcetera row, rfd row, C-09, clippy.toml, MS-0 C-7 boundary policy, Cargo.lock]
- **produces:** [C-07, platform font stub, log expiry]
- **preserves:** [trait signatures encode no inode identity, byte-only path, case-sensitive path or single-Trash assumption]
- **invariants:** [the app-owned writer writes only inside adapter-supplied configuration or state directories and is the only site of the C-09 write, create and `rename` calls; `log_retention.rs` is the only delete site, deletes only regular files matching the roller's file-name pattern in the adapter-resolved log directory whose date is more than 7 days before the injected current date, and never follows a symbolic link; open location is the only process spawn; every allowed call carries its C-09 `expect`; the picker never blocks the UI frame loop; the directory resolver takes an injected environment lookup (a closure from a variable name to an optional `OsString`, as `std::env::var_os` is) and the home directory, and production passes `std::env::var_os` and the home directory from `etcetera`; no test calls `std::env::set_var` or `std::env::remove_var`, which are `unsafe` in edition 2024 and barred by the `unsafe_code` lints]
- **executor_discretion:** [trait method names beyond C-07 semantics, error types]
- **files:** [`crates/fuzzy-dedupe-platform/Cargo.toml` (modify; owner T10), `Cargo.lock` (modify minimal lock delta only, D-014; owner T4), `.github/boundaries.toml` (modify own third-party entries only, C-7; owner T4), `crates/fuzzy-dedupe-platform/src/lib.rs` (modify; owner T10), `crates/fuzzy-dedupe-platform/src/dirs.rs` (create; owner T10), `crates/fuzzy-dedupe-platform/src/picker.rs` (create; owner T10), `crates/fuzzy-dedupe-platform/src/open.rs` (create; owner T10), `crates/fuzzy-dedupe-platform/src/app_files.rs` (create; owner T10), `crates/fuzzy-dedupe-platform/src/log_retention.rs` (create; owner T10), `crates/fuzzy-dedupe-platform/src/fonts.rs` (create stub; owner T10), `crates/fuzzy-dedupe-platform/tests/adapters.rs` (test; owner T10), `crates/fuzzy-dedupe-platform/tests/log_retention.rs` (test; owner T10)]
- **parallel_safe:** yes
- **conflicts_with:** [T21, T4, T6, T11, T22]
- **supersedes:** []
- **superseded_by:** []
- **evidence:** [ephemeral]
- **recovery:** revert to the last green checkpoint; no consumer exists until T10 lands
- **acceptance:** PV-T10-001 proves, through an injected environment map and home directory, that the directories resolve to `fuzzy-dedupe/` and `fuzzy-dedupe/logs` under absolute `XDG_CONFIG_HOME` and `XDG_STATE_HOME` values, fall back to `$HOME/.config` and `$HOME/.local/state` when those are unset, and ignore relative values as the XDG Base Directory specification requires, falling back the same way, the writer refuses a path outside its directories and replaces a file atomically, open location rejects a non-directory non-`https` target, the `portability` and `boundaries` jobs pass with C-7 entries for `etcetera` and `rfd` declared by `fuzzy-dedupe-platform`, the reviewed lock diff adds only those crates and their transitive closure, and the T3 guard and the Clippy gate stay green; PV-T10-002 proves with an injected current date that, in a log directory holding fewer than seven app log files spread over several weeks plus an unrelated file, a non-matching name and a matching name with an unparseable date, expiry deletes exactly the app logs dated more than 7 days earlier, keeps a log dated exactly 7 days earlier and every newer log, keeps the unrelated files, and, after the injected date advances one day (daily rollover), deletes the log that has just crossed the boundary
- **sub-tasks:**
  - **T10.1 RED** — add adapter tests that pass an injected environment map and home directory (set, unset and relative XDG values) and refusal cases, and log-expiry tests with an injected date; expected failure: no adapters exist.
  - **T10.2 Verify RED** — run `rexec -- cargo nextest run -p fuzzy-dedupe-platform --locked`.
  - **T10.3 GREEN** — implement the adapters, the log expiry and the font stub, with each C-09 allowance as an `expect` with a reason.
  - **T10.4 Verify GREEN** — rerun targeted tests, the portability check, the Clippy gate and the guard.
  - **T10.5 REFACTOR** — assess and record none.
  - **T10.6 Verify Task** — run PV-T10-001 and PV-T10-002 and repository gates; before landing, rebase on the current `main`, update `Cargo.lock` minimally without `--locked`, review the lock diff, add the C-7 entries and pass the `--locked` gates and the `boundaries` job against the combined manifests (D-014); commit with the required `Plan-*` trailers.

### Phase P2: Application foundation

#### T12: Build the shell, presentation core and dialog host

- **disposition:** active
- **outcome:** the binary opens the "Fuzzy DeDupe" window with the rail, main menu, shared toolbar items, status bar, workspace switch, dialog host, keyboard dispatcher, logging, theme follow and override, and the §11.8 presentation modules, with stub files for every view, widget and dialog that later tasks fill
- **work_type:** behavior
- **checkpoint:** one green commit with the required `Plan-*` checkpoint trailers, landed by squash
- **boundary:** cross-task
- **depends_on:** [T4, T10, T11]
- **dependency_reason:** consumes C-01 and C-02 from T4, C-07 from T10, and the theme adapter and app manifest from T11
- **requirements:** [FR-027, REQ-035, REQ-027, REQ-028, REQ-032]
- **proof:** [PV-T12-001, PV-T12-002, PV-T12-003, PV-T12-004, PV-T12-005]
- **source_refs:** [spec:docs/specs/canonical/fuzzy-dedupe.md#112-two-workspaces-supporting-tasks-use-popups, spec:docs/specs/canonical/fuzzy-dedupe.md#113-layout-measurements-and-visual-hierarchy, spec:docs/specs/canonical/fuzzy-dedupe.md#116-popup-contracts-and-compact-progress-handling, spec:docs/specs/canonical/fuzzy-dedupe.md#118-recommended-rust-ui-modules-and-state-ownership, spec:docs/specs/canonical/fuzzy-dedupe.md#1110-resizing-scaling-and-keyboard-behavior, spec:docs/specs/canonical/fuzzy-dedupe.md#185-observability]
- **consumes:** [C-01, C-02, C-07, log expiry, theme adapter, app manifest]
- **produces:** [C-12, app module stubs]
- **preserves:** [no widget calls the engine, platform or filesystem while drawing (§5.2)]
- **invariants:** [`WorkspaceState` is exactly Findings or Compare; at most one popup or menu is open; app.rs drains a bounded event batch per frame; events with a non-current origin are discarded; the window cannot shrink below 1024 × 600; `state.rs` defines every §11.8 state owner with its full §11.8 contents and `PreferencesState` with every C-08 field so that only T13 and T15 later edit it; per-dialog draft types live in each dialog file; `commands.rs` dispatches open-location and copy-path intents to C-07 and the clipboard; the app crate holds no delete call, and log expiry runs only through the C-07 log-expiry function, at startup and whenever the roller's date changes, outside the UI pass, with the current date from an injectable clock (D-015)]
- **executor_discretion:** [module internals, toolbar slot mechanism, menu rendering]
- **files:** [`crates/fuzzy-dedupe-app/src/main.rs` (modify; owner T12), `crates/fuzzy-dedupe-app/src/app.rs` (create; owner T12), `crates/fuzzy-dedupe-app/src/ui/mod.rs` (create; owner T12), `crates/fuzzy-dedupe-app/src/ui/shell.rs` (create; owner T12), `crates/fuzzy-dedupe-app/src/ui/widgets/mod.rs` (create; owner T12), `crates/fuzzy-dedupe-app/src/ui/widgets/dialog_host.rs` (create; owner T12), `crates/fuzzy-dedupe-app/src/ui/widgets/status.rs` (create; owner T12), `crates/fuzzy-dedupe-app/src/ui/findings.rs` (create stub; owner T12), `crates/fuzzy-dedupe-app/src/ui/widgets/grouped_table.rs` (create stub; owner T12), `crates/fuzzy-dedupe-app/src/ui/widgets/diff_view.rs` (create stub; owner T12), `crates/fuzzy-dedupe-app/src/ui/compare.rs` (create stub; owner T12), `crates/fuzzy-dedupe-app/src/ui/dialogs/mod.rs` (create; owner T12), `crates/fuzzy-dedupe-app/src/ui/dialogs/matching_rules.rs` (create stub; owner T12), `crates/fuzzy-dedupe-app/src/ui/dialogs/new_scan.rs` (create stub; owner T12), `crates/fuzzy-dedupe-app/src/ui/dialogs/unique_text_review.rs` (create stub; owner T12), `crates/fuzzy-dedupe-app/src/ui/dialogs/queue_identical.rs` (create stub; owner T12), `crates/fuzzy-dedupe-app/src/ui/dialogs/cleanup.rs` (create stub; owner T12), `crates/fuzzy-dedupe-app/src/ui/dialogs/keyboard_shortcuts.rs` (create stub; owner T12), `crates/fuzzy-dedupe-app/src/ui/dialogs/about.rs` (create stub; owner T12), `crates/fuzzy-dedupe-app/src/presentation/mod.rs` (create; owner T12), `crates/fuzzy-dedupe-app/src/presentation/state.rs` (create; owner T12), `crates/fuzzy-dedupe-app/src/presentation/commands.rs` (create; owner T12), `crates/fuzzy-dedupe-app/src/presentation/events.rs` (create; owner T12), `crates/fuzzy-dedupe-app/src/presentation/projections.rs` (create; owner T12), `crates/fuzzy-dedupe-app/src/presentation/preferences.rs` (create stub; owner T12), `crates/fuzzy-dedupe-app/src/measure/mod.rs` (create stub; owner T12), `crates/fuzzy-dedupe-app/tests/shell.rs` (test; owner T12)]
- **parallel_safe:** no
- **conflicts_with:** [T13, T14, T15, T16, T17, T18, T19, T20, T23]
- **supersedes:** []
- **superseded_by:** []
- **evidence:** [ephemeral]
- **recovery:** revert to the last green checkpoint; later app tasks cannot start until T12 lands
- **acceptance:** PV-T12-001 proves a simulated system-theme change and each override value recolor the shell and an open popup in the next frame without restart; PV-T12-002 proves the title, rail widths at 1600 and 1279 units, the six main-menu entries opened by F10 with focus returning to Menu, the 26-unit status bar and the 1024 × 600 minimum; PV-T12-003 proves a log file appears under an overridden state directory, `RUST_LOG=debug` raises the level, startup and a simulated daily rollover of the injected clock each invoke the C-07 log expiry once with the injected current date and outside the UI pass, any file-count cap on the roller is only an extra bound, and Open log folder calls the open-location adapter with that directory; PV-T12-004 proves every global §11.10 chord dispatches its action, a popup blocks them, and no single-letter key is bound; PV-T12-005 proves a second popup request while one is open is refused, Esc cancels, Enter triggers the non-destructive default and focus returns to the invoking control
- **sub-tasks:**
  - **T12.1 RED** — add headless `egui_kittest` interaction tests for PV-T12-001 to PV-T12-005; expected failure: no shell exists.
  - **T12.2 Verify RED** — run `rexec -- cargo nextest run -p fuzzy-dedupe-app --locked -E 'test(shell)'`.
  - **T12.3 GREEN** — implement the shell, presentation core, dialog host, keyboard dispatcher, logging and stubs.
  - **T12.4 Verify GREEN** — rerun targeted tests, the guard and the dependency-direction gate.
  - **T12.5 REFACTOR** — assess and record none.
  - **T12.6 Verify Task** — run PV-T12-001 to PV-T12-005 and repository gates; commit with the required `Plan-*` trailers.

### Phase P3: Workspaces, popups and preferences

#### T13: Build the Findings workspace

- **disposition:** active
- **outcome:** Findings renders the grouped, virtualized `egui_table` over a visible-row projection with stable keys, every §11.4 column, label, menu and interaction, the filter row and search, column collapse below 1280, and the 01a, 01b, 01c, 01e and empty-scan states, emitting review commands for every queue entry point it hosts
- **work_type:** behavior
- **checkpoint:** one green commit with the required `Plan-*` checkpoint trailers, landed by squash
- **boundary:** cross-task
- **depends_on:** [T12, T7]
- **dependency_reason:** fills the T12 app module stubs using C-12 and emits commands to C-05 from T7
- **requirements:** [FR-018, REQ-014, REQ-037]
- **proof:** [PV-T13-001, PV-T13-002, PV-T13-003]
- **source_refs:** [spec:docs/specs/canonical/fuzzy-dedupe.md#114-findings-table-structure-and-interactions, spec:docs/specs/canonical/fuzzy-dedupe.md#113-layout-measurements-and-visual-hierarchy, spec:docs/specs/canonical/fuzzy-dedupe.md#119-rendering-asynchronous-work-and-scaling, spec:docs/specs/canonical/fuzzy-dedupe.md#1110-resizing-scaling-and-keyboard-behavior]
- **consumes:** [C-12, app module stubs, C-05]
- **produces:** [Findings workspace, presentation/state.rs FindingsState return context]
- **preserves:** [projection rebuilt only on data, filter, sort or expansion change, never per frame]
- **invariants:** [focus, queue membership and expansion are keyed by stable IDs; checkbox clicks never open Compare; expanding never changes the queue; groups stay intact when sorted; scores appear on member rows only; Open containing folder emits an open-location intent for the row's folder and a missing fixture folder yields an actionable status message]
- **executor_discretion:** [projection data structure, column-resize persistence mechanics]
- **files:** [`crates/fuzzy-dedupe-app/src/ui/findings.rs` (modify; owner T12), `crates/fuzzy-dedupe-app/src/ui/widgets/grouped_table.rs` (modify; owner T12), `crates/fuzzy-dedupe-app/src/presentation/state.rs` (modify Findings and Queue fields only; owner T12), `crates/fuzzy-dedupe-app/src/presentation/projections.rs` (modify Findings projections; owner T12), `crates/fuzzy-dedupe-app/tests/findings.rs` (test; owner T13)]
- **parallel_safe:** yes
- **conflicts_with:** [T12, T15]
- **supersedes:** []
- **superseded_by:** []
- **evidence:** [ephemeral]
- **recovery:** revert to the last green checkpoint
- **acceptance:** PV-T13-001 proves that after queueing rows, every filter, every sort key, expand all and collapse all leave focus and queued IDs unchanged and a filtered-out queued file is counted ("3 queued, 3 outside the search"); PV-T13-002 proves on the `mixed` and `large` scenarios, before and after Make this the reference, that every member row's label is the §11.4 template for its relation type over its actual counterpart ("reference" when the counterpart is the current reference, otherwise the counterpart's file name, as in "Identical to Notes_copy.txt" and "Contains Notes_copy.txt"), its Unique value refers to that counterpart, and focus and queue indicators toggle independently; PV-T13-003 proves the §11.4 default column widths, the Sort and Actions menu items, the seven row-menu items with Shift+F10 and Menu key, the first item reading "Compare with" and the counterpart's name for a non-reference counterpart, the "Counterpart of a queued file" refusal shown in the Decision column, the status bar reporting how many files Make this the reference unqueued, search clear, the match-type counts rule, the column collapse below 1280 units and each listed state on its scenario
- **sub-tasks:**
  - **T13.1 RED** — add headless interaction tests for PV-T13-001 to PV-T13-003 on the `large`, `issues`, `empty` and `unicode` scenarios; expected failure: the stub view renders nothing.
  - **T13.2 Verify RED** — run `rexec -- cargo nextest run -p fuzzy-dedupe-app --locked -E 'test(findings)'`.
  - **T13.3 GREEN** — implement the view, table, projection and states.
  - **T13.4 Verify GREEN** — rerun targeted and app tests.
  - **T13.5 REFACTOR** — assess and record none.
  - **T13.6 Verify Task** — run PV-T13-001 to PV-T13-003 and repository gates; commit with the required `Plan-*` trailers.

#### T14: Build the paired diff widget

- **disposition:** active
- **outcome:** `diff_view` renders the C-04 model as aligned row pairs with gutters, spacers, wrap alignment to the taller row, horizontal scroll with wrap off, anchor-linked scrolling that can be unlinked, change navigation on both panes, color plus side labels, a layout cache keyed by revision, zoom, wrap width and mode, and model-backed per-pane selection and copy per IR-006
- **work_type:** behavior
- **checkpoint:** one green commit with the required `Plan-*` checkpoint trailers, landed by squash
- **boundary:** cross-task
- **depends_on:** [T12, T6]
- **dependency_reason:** fills the `diff_view.rs` stub from the T12 app module stubs and renders C-04 from T6
- **requirements:** [REQ-015, IR-006]
- **proof:** [PV-T14-001, PV-T14-002]
- **source_refs:** [spec:docs/specs/canonical/fuzzy-dedupe.md#115-comparison-large-aligned-read-only-panes, spec:docs/specs/canonical/fuzzy-dedupe.md#73-interface-requirements, spec:docs/specs/canonical/fuzzy-dedupe.md#119-rendering-asynchronous-work-and-scaling]
- **consumes:** [C-12, app module stubs, C-04]
- **produces:** [diff widget]
- **preserves:** [text is never editable; only visible ranges plus overscan are laid out]
- **invariants:** [selection never crosses panes; copy yields original bytes with original line endings and no line numbers, markers or spacers; Normalized view copies normalized text and says so]
- **executor_discretion:** [widget internals, caret model for Shift+arrow, auto-scroll speed]
- **files:** [`crates/fuzzy-dedupe-app/src/ui/widgets/diff_view.rs` (modify; owner T12), `crates/fuzzy-dedupe-app/tests/diff_view.rs` (test; owner T14)]
- **parallel_safe:** yes
- **conflicts_with:** [T12]
- **supersedes:** []
- **superseded_by:** []
- **evidence:** [ephemeral]
- **recovery:** revert to the last green checkpoint
- **acceptance:** PV-T14-001 proves on generator pairs with one-sided insertions and long lines that paired rows share a top edge with wrap on and off, spacers carry no line number, displayed line numbers equal generator source numbers, displayed Only-in counts equal the C-04 values, and next or previous change lands both panes on the same hunk; PV-T14-002 proves Ctrl+A then copy on a 5 MB fixture returns bytes equal to the generated buffer, drag, Shift+click and Shift+arrow selections copy exactly their span, a drag into the other pane does not extend the selection, and Normalized copy is labeled
- **sub-tasks:**
  - **T14.1 RED** — add widget tests for PV-T14-001 and PV-T14-002; expected failure: the stub renders nothing.
  - **T14.2 Verify RED** — run `rexec -- cargo nextest run -p fuzzy-dedupe-app --locked -E 'test(diff_view)'`.
  - **T14.3 GREEN** — implement the widget.
  - **T14.4 Verify GREEN** — rerun targeted and app tests.
  - **T14.5 REFACTOR** — assess and record none.
  - **T14.6 Verify Task** — run PV-T14-001 and PV-T14-002 and repository gates; commit with the required `Plan-*` trailers.

#### T15: Build the Compare workspace and return routing

- **disposition:** active
- **outcome:** Compare hosts the diff widget with the §11.5 toolbar, evidence row, change row, candidate stepper, Keep both and Queue B for cleanup…, opens from Enter, the toolbar or the rail, stays disabled with an explanation when no pair exists, marks stale evidence, and returns to Findings with all seven IR-001 properties restored
- **work_type:** behavior
- **checkpoint:** one green commit with the required `Plan-*` checkpoint trailers, landed by squash
- **boundary:** cross-task
- **depends_on:** [T12, T13, T14, T6, T7]
- **dependency_reason:** fills the T12 app module stubs, consumes the Findings workspace and the presentation/state.rs FindingsState return context from T13, the diff widget from T14, C-04 from T6 and C-05 from T7
- **requirements:** [IR-001, REQ-013, REQ-016, REQ-036]
- **proof:** [PV-T15-001, PV-T15-002, PV-T15-003, PV-T15-004]
- **source_refs:** [spec:docs/specs/canonical/fuzzy-dedupe.md#115-comparison-large-aligned-read-only-panes, spec:docs/specs/canonical/fuzzy-dedupe.md#112-two-workspaces-supporting-tasks-use-popups, spec:docs/specs/canonical/fuzzy-dedupe.md#73-interface-requirements]
- **consumes:** [app module stubs, Findings workspace, presentation/state.rs FindingsState return context, diff widget, C-04, C-05, C-12]
- **produces:** [Compare workspace, CompareState]
- **preserves:** [Findings state is retained, never rebuilt, while Compare is shown]
- **invariants:** [Queue B identifies its target by ID and path and records the displayed A file as its counterpart (C-05); Keep both changes only the displayed pair; there is no Save, merge, editable cursor or AI control]
- **executor_discretion:** [toolbar overflow mechanics below 1280 units]
- **files:** [`crates/fuzzy-dedupe-app/src/ui/compare.rs` (modify; owner T12), `crates/fuzzy-dedupe-app/src/presentation/state.rs` (modify Compare fields only; owner T12), `crates/fuzzy-dedupe-app/src/presentation/projections.rs` (modify Compare projections; owner T12), `crates/fuzzy-dedupe-app/tests/compare.rs` (test; owner T15)]
- **parallel_safe:** no
- **conflicts_with:** [T12, T13]
- **supersedes:** []
- **superseded_by:** []
- **evidence:** [ephemeral]
- **recovery:** revert to the last green checkpoint
- **acceptance:** PV-T15-001 proves that entering Compare from a filtered, sorted, expanded, resized and scrolled table and returning restores search text, match-type filter, sort, expanded groups, focused row, column widths and scroll position exactly; PV-T15-002 proves queued IDs and targets before and after sort, filter, collapse and a Compare round trip are equal; PV-T15-003 proves keyboard and pointer edit attempts change no text, no edit, merge or AI control exists in the widget tree, and selection and copy follow IR-006 through the workspace; PV-T15-004 proves the §11.5 control set, the no-pair disabled state with its explanation, a stale-evidence indicator on the `stale` scenario, and that Queue B on a counterpart-queued pair is refused with "Counterpart queued"
- **sub-tasks:**
  - **T15.1 RED** — add interaction tests for PV-T15-001 to PV-T15-004; expected failure: the stub view renders nothing.
  - **T15.2 Verify RED** — run `rexec -- cargo nextest run -p fuzzy-dedupe-app --locked -E 'test(compare)'`.
  - **T15.3 GREEN** — implement the workspace and routing.
  - **T15.4 Verify GREEN** — rerun targeted and app tests.
  - **T15.5 REFACTOR** — assess and record none.
  - **T15.6 Verify Task** — run PV-T15-001 to PV-T15-004 and repository gates; commit with the required `Plan-*` trailers.

#### T16: Build the Matching rules and New scan popups

- **disposition:** active
- **outcome:** Matching rules and New scan implement the §11.6 rows with independent drafts, folder add through the picker, per-folder protection, inline folder problems, the collapsible rules section, the queued-count notice, and Start scan, which clears decisions through the review service and launches the simulated scan
- **work_type:** behavior
- **checkpoint:** one green commit with the required `Plan-*` checkpoint trailers, landed by squash
- **boundary:** cross-task
- **depends_on:** [T12, T5, T7, T10]
- **dependency_reason:** fills the T12 app module stubs, starts C-03 from T5, clears decisions through C-05 from T7, and opens the C-07 folder picker from T10
- **requirements:** [REQ-017, FR-026]
- **proof:** [PV-T16-001, PV-T16-002]
- **source_refs:** [spec:docs/specs/canonical/fuzzy-dedupe.md#116-popup-contracts-and-compact-progress-handling, spec:docs/specs/canonical/fuzzy-dedupe.md#73-interface-requirements, spec:docs/specs/canonical/fuzzy-dedupe.md#71-functional-requirements]
- **consumes:** [C-12, app module stubs, C-03, C-05, C-07]
- **produces:** [form popups, scan setup state]
- **preserves:** [removing a folder changes the setup, never a source file; the current scan's stored rules never change]
- **invariants:** [cancel or close commits nothing; minimum similarity is validated against 0–100%; the displayed 95.0% is a fixture value, not a default (OQ-005)]
- **executor_discretion:** [draft structure, folder-problem detection details limited to metadata reads]
- **files:** [`crates/fuzzy-dedupe-app/src/ui/dialogs/matching_rules.rs` (modify; owner T12), `crates/fuzzy-dedupe-app/src/ui/dialogs/new_scan.rs` (modify; owner T12), `crates/fuzzy-dedupe-app/tests/form_popups.rs` (test; owner T16)]
- **parallel_safe:** yes
- **conflicts_with:** [T12]
- **supersedes:** []
- **superseded_by:** []
- **evidence:** [ephemeral]
- **recovery:** revert to the last green checkpoint
- **acceptance:** PV-T16-001 proves that editing then cancelling each popup leaves saved rules and setup unchanged, saving from the gear changes saved rules but not the current scan's stored copy, and the next Start scan stores the saved rules; PV-T16-002 proves New scan states the exact queued count it will clear, Start scan empties the queue and every Kept, per-file Protect and acknowledgment, folder protection stays in the setup, and a missing or nested folder shows its inline problem
- **sub-tasks:**
  - **T16.1 RED** — add popup interaction tests; expected failure: stub dialogs render nothing.
  - **T16.2 Verify RED** — run `rexec -- cargo nextest run -p fuzzy-dedupe-app --locked -E 'test(form_popups)'`.
  - **T16.3 GREEN** — implement both popups with a fake picker in tests.
  - **T16.4 Verify GREEN** — rerun targeted and app tests.
  - **T16.5 REFACTOR** — assess and record none.
  - **T16.6 Verify Task** — run PV-T16-001 and PV-T16-002 and repository gates; commit with the required `Plan-*` trailers.

#### T17: Build the Review unique text and Queue identical copies popups

- **disposition:** active
- **outcome:** Review unique text shows target and counterpart identity, the unique lines, Open comparison, Copy unique lines and the acknowledgment flow of `44:4079`; Queue identical copies states scope, rules, count and size and queues through the review service; every Findings and Compare entry point that needs review opens Review unique text first
- **work_type:** behavior
- **checkpoint:** one green commit with the required `Plan-*` checkpoint trailers, landed by squash
- **boundary:** cross-task
- **depends_on:** [T12, T6, T7]
- **dependency_reason:** fills the T12 app module stubs, shows unique lines from C-04 (T6), and records acknowledgments and bulk queueing through C-05 (T7)
- **requirements:** [FR-023, REQ-033]
- **proof:** [PV-T17-001, PV-T17-002]
- **source_refs:** [spec:docs/specs/canonical/fuzzy-dedupe.md#116-popup-contracts-and-compact-progress-handling, spec:docs/specs/canonical/fuzzy-dedupe.md#114-findings-table-structure-and-interactions, spec:docs/specs/canonical/fuzzy-dedupe.md#71-functional-requirements]
- **consumes:** [C-12, app module stubs, C-04, C-05]
- **produces:** [review popups]
- **preserves:** [an unreviewed score never creates approved removal intent]
- **invariants:** [Queue for cleanup stays enabled; pressed without the acknowledgment it queues nothing, shows the inline prompt and moves focus to the checkbox]
- **executor_discretion:** [unique-line list virtualization]
- **files:** [`crates/fuzzy-dedupe-app/src/ui/dialogs/unique_text_review.rs` (modify; owner T12), `crates/fuzzy-dedupe-app/src/ui/dialogs/queue_identical.rs` (modify; owner T12), `crates/fuzzy-dedupe-app/tests/review_popups.rs` (test; owner T17)]
- **parallel_safe:** yes
- **conflicts_with:** [T12]
- **supersedes:** []
- **superseded_by:** []
- **evidence:** [ephemeral]
- **recovery:** revert to the last green checkpoint
- **acceptance:** PV-T17-001 proves that the checkbox, Space, the row menu and Queue B on a fuzzy or normalized row each open Review unique text, Queue without acknowledgment queues nothing and moves focus to the checkbox, Queue with acknowledgment queues exactly that target against that counterpart, the listed unique lines and their count come from the C-04 measure, and Copy unique lines places exactly the C-04 copy text on the clipboard; PV-T17-002 proves both scopes report the count and binary-unit size the review service will queue and that the resulting queue excludes references, protected, kept and non-byte-identical members
- **sub-tasks:**
  - **T17.1 RED** — add popup interaction tests; expected failure: stub dialogs render nothing.
  - **T17.2 Verify RED** — run `rexec -- cargo nextest run -p fuzzy-dedupe-app --locked -E 'test(review_popups)'`.
  - **T17.3 GREEN** — implement both popups and the entry-point routing through dialog-open commands.
  - **T17.4 Verify GREEN** — rerun targeted and app tests.
  - **T17.5 REFACTOR** — assess and record none.
  - **T17.6 Verify Task** — run PV-T17-001 and PV-T17-002 and repository gates; commit with the required `Plan-*` trailers.

#### T18: Build the cleanup dialog

- **disposition:** active
- **outcome:** one Review cleanup dialog hosts review, running, stopped and finished over the simulated cleanup, with its summary line, virtualized table showing each target and its kept copy, stop rule, destination text, Cancel initial focus, single-flight confirmation, "Blocked: no Trash", revised-plan confirmation, targeted retry and a Show journal view of the simulated per-item outcomes
- **work_type:** behavior
- **checkpoint:** one green commit with the required `Plan-*` checkpoint trailers, landed by squash
- **boundary:** cross-task
- **depends_on:** [T12, T8]
- **dependency_reason:** fills the T12 app module stubs and drives C-06 from T8
- **requirements:** [REQ-024, REQ-040, REQ-041]
- **proof:** [PV-T18-001, PV-T18-002, PV-T18-003]
- **source_refs:** [spec:docs/specs/canonical/fuzzy-dedupe.md#116-popup-contracts-and-compact-progress-handling, spec:docs/specs/canonical/fuzzy-dedupe.md#104-state-transitions]
- **consumes:** [C-12, app module stubs, C-06]
- **produces:** [cleanup dialog]
- **preserves:** [no permanent-delete control or text in any state]
- **invariants:** [Enter never confirms the move; the confirm button is disabled from submission until the run ends; the dialog returns focus to the invoking workspace control]
- **executor_discretion:** [journal view layout inside the dialog]
- **files:** [`crates/fuzzy-dedupe-app/src/ui/dialogs/cleanup.rs` (modify; owner T12), `crates/fuzzy-dedupe-app/tests/cleanup_dialog.rs` (test; owner T18)]
- **parallel_safe:** yes
- **conflicts_with:** [T12]
- **supersedes:** []
- **superseded_by:** []
- **evidence:** [ephemeral]
- **recovery:** revert to the last green checkpoint
- **acceptance:** PV-T18-001 proves each state shows its §11.6 title, statuses and footer text, Cancel has initial focus with "Nothing has been removed.", the finished summary reads "N moved · N failed" with Trash failures counted as failed, and no widget offers permanent deletion; PV-T18-002 proves the review state lists every target with its kept copy and the "system Trash" destination, a blocked target shows "Blocked: no Trash" before confirmation and is excluded from the count, and a double click on "Move N files to Trash" submits once; PV-T18-003 proves on the ERR-003 schedule that the failed file shows "Failed" and the changed file "Stopped: changed", each named with its reason and taken out of the plan, both stay visible in the dialog and its journal view with moved, blocked and uncertain outcomes, the revised plan needs confirmation, and a retry re-executes no moved item
- **sub-tasks:**
  - **T18.1 RED** — add dialog interaction tests per state; expected failure: the stub renders nothing.
  - **T18.2 Verify RED** — run `rexec -- cargo nextest run -p fuzzy-dedupe-app --locked -E 'test(cleanup_dialog)'`.
  - **T18.3 GREEN** — implement the dialog.
  - **T18.4 Verify GREEN** — rerun targeted and app tests.
  - **T18.5 REFACTOR** — assess and record none.
  - **T18.6 Verify Task** — run PV-T18-001 and repository gates; commit with the required `Plan-*` trailers.

#### T19: Build the Keyboard shortcuts and About popups

- **disposition:** active
- **outcome:** Keyboard shortcuts lists the §11.10 map in three columns with key caps and the no-Trash note, opened by F1, Ctrl+? and the main menu; About shows the FR-028 notices, View license and Third-party notices replace the About body with the embedded text in a read-only, selectable `DD/Compact/document` view with Back, and Source code opens the source location in the browser
- **work_type:** behavior
- **checkpoint:** one green commit with the required `Plan-*` checkpoint trailers, landed by squash
- **boundary:** cross-task
- **depends_on:** [T12, T10]
- **dependency_reason:** fills the T12 app module stubs and opens the source URL through C-07 from T10
- **requirements:** [FR-028]
- **proof:** [PV-T19-001]
- **source_refs:** [spec:docs/specs/canonical/fuzzy-dedupe.md#116-popup-contracts-and-compact-progress-handling, spec:docs/specs/canonical/fuzzy-dedupe.md#1110-resizing-scaling-and-keyboard-behavior, spec:docs/specs/canonical/fuzzy-dedupe.md#71-functional-requirements, repo:LICENSE]
- **consumes:** [C-12, app module stubs, C-07]
- **produces:** [info popups]
- **preserves:** [the license text shown is byte-identical to `LICENSE`]
- **invariants:** [About reads "Copyright © 2026 L3Digital LLC", `GPL-3.0-only` and the no-warranty statement; About stays the one open popup while showing either text; the text view is read-only; the no-or-later phrase check covers only the application's own license declaration, package metadata and original notices in About, never the embedded GPL text (which contains "or any later version" at `LICENSE` line 572) or third-party legal texts, which keep their exact-content and selection tests]
- **executor_discretion:** [key-cap rendering, scroll handling inside About]
- **files:** [`crates/fuzzy-dedupe-app/src/ui/dialogs/keyboard_shortcuts.rs` (modify; owner T12), `crates/fuzzy-dedupe-app/src/ui/dialogs/about.rs` (modify; owner T12), `crates/fuzzy-dedupe-app/tests/info_popups.rs` (test; owner T19)]
- **parallel_safe:** yes
- **conflicts_with:** [T12]
- **supersedes:** []
- **superseded_by:** []
- **evidence:** [ephemeral]
- **recovery:** revert to the last green checkpoint
- **acceptance:** PV-T19-001 proves the shortcuts table equals the §11.10 table row for row; About shows the version from the package metadata and each required statement; the phrase check over About's own summary, notices and displayed package license finds no or-later grant; View license replaces the body with text byte-identical to the unchanged `LICENSE` in the Document style, a whole-text selection copies it exactly, and Back restores the body; Third-party notices does the same for `THIRD-PARTY-NOTICES.md`; no second popup opens; and Source code calls the opener adapter with the repository URL
- **sub-tasks:**
  - **T19.1 RED** — add popup tests with a fake open-location adapter; expected failure: stub dialogs render nothing.
  - **T19.2 Verify RED** — run `rexec -- cargo nextest run -p fuzzy-dedupe-app --locked -E 'test(info_popups)'`.
  - **T19.3 GREEN** — implement both popups.
  - **T19.4 Verify GREEN** — rerun targeted and app tests.
  - **T19.5 REFACTOR** — assess and record none.
  - **T19.6 Verify Task** — run PV-T19-001 and repository gates; commit with the required `Plan-*` trailers.

#### T20: Persist preferences

- **disposition:** active
- **outcome:** `preferences.toml` version 1 (C-08) loads at startup from the adapter path, is written through the app-owned writer when a preference changes, and restores window size and maximized state, zoom, column widths, pane split, wrap, linked scroll, rail collapse, theme override and the current scan setup
- **work_type:** behavior
- **checkpoint:** one green commit with the required `Plan-*` checkpoint trailers, landed by squash
- **boundary:** state
- **depends_on:** [T12, T13, T15, T16, T10]
- **dependency_reason:** fills the `preferences.rs` stub from the T12 app module stubs and serializes the fields wired by the Findings workspace (T13), the Compare workspace (T15) and the scan setup state (T16), using C-07 from T10
- **requirements:** [DR-009]
- **proof:** [PV-T20-001]
- **source_refs:** [spec:docs/specs/canonical/fuzzy-dedupe.md#74-data-requirements, spec:docs/specs/canonical/fuzzy-dedupe.md#182-configuration, spec:docs/specs/canonical/fuzzy-dedupe.md#118-recommended-rust-ui-modules-and-state-ownership]
- **consumes:** [app module stubs, Findings workspace, Compare workspace, scan setup state, C-07]
- **produces:** [C-08]
- **preserves:** [eframe persistence stays off; scan results, the queue and decisions are never written to the file]
- **invariants:** [unknown or unreadable files follow C-08 error rules; window position is never stored (DR-009 v0.9)]
- **executor_discretion:** [key names within C-08, write debouncing]
- **files:** [`crates/fuzzy-dedupe-app/src/presentation/preferences.rs` (modify; owner T12), `crates/fuzzy-dedupe-app/tests/preferences.rs` (test; owner T20)]
- **parallel_safe:** yes
- **conflicts_with:** [T12]
- **supersedes:** []
- **superseded_by:** []
- **evidence:** [ephemeral]
- **recovery:** revert to the last green checkpoint; a broken preferences file never prevents startup
- **acceptance:** PV-T20-001 proves that after changing every C-08 field and restarting the harness against the same overridden configuration directory, each field is restored; queued files, decisions and results are absent after restart; a malformed file and a version-2 file both start with defaults and are not overwritten until a preference changes
- **sub-tasks:**
  - **T20.1 RED** — add restart round-trip and error-case tests; expected failure: the stub stores nothing.
  - **T20.2 Verify RED** — run `rexec -- cargo nextest run -p fuzzy-dedupe-app --locked -E 'test(preferences)'`.
  - **T20.3 GREEN** — implement load, save and versioning.
  - **T20.4 Verify GREEN** — rerun targeted tests and the guard.
  - **T20.5 REFACTOR** — assess and record none.
  - **T20.6 Verify Task** — run PV-T20-001 and repository gates; commit with the required `Plan-*` trailers.

#### T21: Add system font fallback and prove exit item 7

- **disposition:** active
- **outcome:** the platform font adapter finds system fallback fonts for scripts Plex lacks (CJK, Arabic, Hebrew, Thai) through the T1-approved fontconfig crate, declared only by `fuzzy-dedupe-platform` with its C-7 entry, the app appends them after Plex and the embedded Noto Emoji at startup, and Japanese, Arabic and emoji file names render on the reference workstation
- **work_type:** behavior
- **checkpoint:** one green commit with the required `Plan-*` checkpoint trailers, landed by squash, carrying EV-001
- **boundary:** cross-task
- **depends_on:** [T1, T10, T11, T13]
- **dependency_reason:** adds the approved fontconfig crate from T1, fills the platform font stub from T10, relies on the theme adapter's embedded Noto Emoji from T11, and captures the Findings workspace from T13 on the `unicode` scenario
- **requirements:** [REQ-007]
- **proof:** [PV-T21-001]
- **source_refs:** [spec:docs/specs/canonical/fuzzy-dedupe.md#117-centralized-visual-tokens-and-reusable-widgets, spec:docs/specs/canonical/fuzzy-dedupe.md#85-design-constraints, spec:docs/specs/canonical/fuzzy-dedupe.md#approved-dependencies]
- **consumes:** [approved fontconfig crate, platform font stub, theme adapter, Findings workspace, MS-0 C-7 boundary policy, Cargo.lock]
- **produces:** [system font fallback, docs/evidence/ms-1/font-fallback.md]
- **preserves:** [snapshot tests keep using embedded fonts only (D-007)]
- **invariants:** [font lookup runs once at startup off the frame path; the fontconfig dependency lives only in the platform crate, as a Linux target-specific dependency if the crate does not build for the portability targets (MS-0 D-011 rule 4 allows that only in platform); the workstation glyph test is ignored by default so rexec and CI gates never depend on system fonts; EV-001 opens with its C-14 binding]
- **executor_discretion:** [fallback selection order beyond CJK, Arabic, Hebrew, Thai and emoji coverage]
- **files:** [`crates/fuzzy-dedupe-platform/Cargo.toml` (modify; owner T10), `Cargo.lock` (modify minimal lock delta only, D-014; owner T4), `.github/boundaries.toml` (modify own third-party entries only, C-7; owner T4), `crates/fuzzy-dedupe-platform/src/fonts.rs` (modify; owner T10), `crates/fuzzy-dedupe-platform/tests/fonts.rs` (test; owner T21), `docs/evidence/ms-1/font-fallback.md` (evidence; owner T21), `docs/evidence/ms-1/font-fallback/**` (evidence captures with `.license` sidecars; owner T21)]
- **parallel_safe:** yes
- **conflicts_with:** [T10, T4, T6, T11, T22]
- **supersedes:** []
- **superseded_by:** []
- **evidence:** [EV-001]
- **recovery:** if a script still lacks glyphs on the workstation, block T21 and report the missing coverage to the owner; never embed another font without a spec change
- **acceptance:** PV-T21-001 proves on the reference workstation that every character of the `unicode` scenario's Japanese and Arabic names has a glyph in a fontconfig-supplied font and every emoji has a glyph in the embedded Noto Emoji, an in-app capture of those rows shows no missing-glyph boxes, and the same check without the adapter fails for the Japanese and Arabic names, recorded in EV-001 with its C-14 binding; the `portability` and `boundaries` jobs pass with the fontconfig crate's C-7 entry, and the reviewed lock diff adds only that crate and its transitive closure
- **sub-tasks:**
  - **T21.1 RED** — add a workstation-only glyph-coverage test; expected failure: the stub supplies no fallback fonts.
  - **T21.2 Verify RED** — run the test locally on the reference workstation under the §3.4 exception.
  - **T21.3 GREEN** — implement the adapter and startup wiring.
  - **T21.4 Verify GREEN** — rerun on the workstation; run the remaining gates through rexec.
  - **T21.5 Verify Task** — write EV-001 with its C-14 binding, the capture and run conditions; run PV-T21-001; before landing, rebase on the current `main`, update `Cargo.lock` minimally without `--locked`, review the lock diff, add the C-7 entries and pass the `--locked` gates and the `boundaries` job against the combined manifests (D-014); commit with the required `Plan-*` trailers.

### Phase P4: Integration and measurement

#### T22: Add the UI-test job, layout assertions and integration suite

- **disposition:** active
- **outcome:** a UI-test job added to `.github/workflows/rust.yml` runs `egui_kittest` snapshots of the 16 review states in both themes on a pinned GitHub-hosted Ubuntu image with Mesa lavapipe; numeric layout and token assertions pass in 1600 × 960 and 1024 × 600; integration tests cover cross-popup behavior, all queue entry points, edge states and log content
- **work_type:** verification
- **checkpoint:** one green commit with the required `Plan-*` checkpoint trailers, landed by squash after the new job passes on the PR, carrying EV-006
- **boundary:** deployment
- **depends_on:** [T3, T9, T11, T13, T15, T16, T17, T18, T19, T20]
- **dependency_reason:** edits rust.yml without no-tests=warn from T3, consumes the design scenario (T9), the app manifest and REUSE.toml asset annotations (T11) for the snapshot feature and baseline coverage, every finished view and popup (T13 and T15 to T19) and C-08 (T20) for the log and restart paths
- **requirements:** [REQ-004, REQ-005, REQ-010, REQ-011, REQ-019, NFR-009, REQ-039, REQ-043]
- **proof:** [PV-T22-001, PV-T22-002, PV-T22-003, PV-T22-004, PV-T22-005, PV-T22-006, PV-T22-007, PV-T22-008]
- **source_refs:** [spec:docs/specs/canonical/fuzzy-dedupe.md#172-test-strategy, spec:docs/specs/canonical/fuzzy-dedupe.md#111-authoritative-design-links-and-revision-precedence, spec:docs/specs/canonical/fuzzy-dedupe.md#113-layout-measurements-and-visual-hierarchy, spec:docs/specs/canonical/fuzzy-dedupe.md#1111-acceptance-checklist-for-the-implementing-agent, spec:docs/specs/canonical/fuzzy-dedupe.md#173-requirement-to-test-traceability]
- **consumes:** [rust.yml without no-tests=warn, design scenario, app manifest, REUSE.toml asset annotations, Findings workspace, Compare workspace, form popups, review popups, cleanup dialog, info popups, C-08]
- **produces:** [C-10 layout assertions, snapshot workflow, integration suite, docs/evidence/ms-1/ci-snapshots.md]
- **preserves:** [the seven MS-0 job IDs and their commands unchanged; only the UI-test job needs a GPU adapter]
- **invariants:** [baselines come only from the snapshot runner; the job fails if zero snapshot tests execute; layout tolerance is ±1 logical unit (A-010); the runner image is pinned to a named Ubuntu version and actions are pinned by commit SHA as in the existing workflows; the snapshot-baseline annotation is project-owned `GPL-3.0-only` (MS-0 D-012 class a) and matches no C-5 protected path; EV-006 opens with its C-14 binding]
- **executor_discretion:** [baseline-update mechanism, test file layout]
- **files:** [`.github/workflows/rust.yml` (modify add UI-test job; owner T3), `crates/fuzzy-dedupe-app/Cargo.toml` (modify snapshot feature only; owner T11), `Cargo.lock` (modify lock delta only if the snapshot feature changes the resolution, D-014; owner T4), `.github/boundaries.toml` (modify only if a direct dependency is added, which T22 does not plan; owner T4), `REUSE.toml` (modify snapshot-baseline annotation only; owner T11), `crates/fuzzy-dedupe-app/tests/snapshots.rs` (create; owner T22), `crates/fuzzy-dedupe-app/tests/snapshots/**` (create baselines; owner T22), `crates/fuzzy-dedupe-app/tests/layout.rs` (create; owner T22), `crates/fuzzy-dedupe-app/tests/integration.rs` (create; owner T22), `docs/evidence/ms-1/ci-snapshots.md` (evidence; owner T22)]
- **parallel_safe:** yes
- **conflicts_with:** [T2, T3, T11, T4, T6, T10, T21]
- **supersedes:** []
- **superseded_by:** []
- **evidence:** [EV-006]
- **recovery:** a failing layout or integration assertion blocks T22 and appends a correction task against the owning view task; runner image or Mesa drift is handled by regenerating baselines in a reviewed PR
- **acceptance:** PV-T22-001 proves the numeric values of C-10 within ±1 unit; PV-T22-002 proves 32 snapshots pass in the UI-test job and a deliberately altered token fails it; PV-T22-003 proves each popup from each workspace leaves workspace state unchanged and a second popup cannot open; PV-T22-004 proves table and paired-region heights are at least 80% of their work areas; PV-T22-005 proves each UI check 14 state shows its actionable message without adding a workspace; PV-T22-006 proves a fixture scan and comparison leave none of the fixture's canary lines in the log directory; PV-T22-007 proves the checkbox, Space, row menu, Compare's Queue B and Queue identical copies give the same outcome, in both queue orders, for reference, protected, kept, counterpart-queued, counterpart-of-a-queued-file and unreviewed fuzzy rows, and an injected target or counterpart change invalidates the affected approval; PV-T22-008 proves on each UI check 06 unique-line fixture that the Findings Unique value, Compare's Only in count and Review unique text's count agree in Original and Normalized views, and Copy unique lines yields the unique lines in file order with one LF after each
- **sub-tasks:**
  - **T22.1 ANCHOR** — record the commit, runner image and Mesa version the baselines target.
  - **T22.2 VERIFY PREREQUISITES** — confirm the `design` scenario, all views and popups, and that the seven MS-0 jobs skip GPU tests.
  - **T22.3 RUN** — add the UI-test job to `rust.yml`, the tests, and baselines generated on the runner with their `REUSE.toml` annotation; run the local suites through rexec.
  - **T22.4 TRIAGE** — route each failure to a correction task against its owner; do not adjust baselines to hide a regression.
  - **T22.5 RERUN** — confirm the job passes on the PR and fails on the negative-control commit, which is then dropped.
  - **T22.6 CAPTURE EVIDENCE** — write EV-006 with its C-14 binding, the passing run URL, runner image, Mesa version and snapshot count; if the lock changed, apply the D-014 landing steps; commit with the required `Plan-*` trailers.

#### T23: Add the measurement mode and scenario drivers

- **disposition:** active
- **outcome:** a release build started in measurement mode drives the scripted T1, T2, T3 and T4 scenarios and prints the C-10 sample streams to standard output
- **work_type:** behavior
- **checkpoint:** one green commit with the required `Plan-*` checkpoint trailers, landed by squash
- **boundary:** cross-task
- **depends_on:** [T5, T12, T13, T15, T16, T17, T18, T19]
- **dependency_reason:** drives C-03 (T5), fills the `measure` stub from the T12 app module stubs, and drives the Findings workspace (T13), the Compare workspace (T15) and every popup (T16 to T19)
- **requirements:** [REQ-034]
- **proof:** [PV-T23-001]
- **source_refs:** [spec:docs/specs/canonical/fuzzy-dedupe.md#ms-1-performance-targets, spec:docs/specs/canonical/fuzzy-dedupe.md#119-rendering-asynchronous-work-and-scaling]
- **consumes:** [C-03, app module stubs, Findings workspace, Compare workspace, form popups, review popups, cleanup dialog, info popups]
- **produces:** [C-10 sample stream]
- **preserves:** [measurement mode is off by default and changes nothing when off]
- **invariants:** [samples go to standard output only (D-010); input injection uses egui raw input, so the same path serves real input]
- **executor_discretion:** [activation mechanism, driver scripting, sample batching]
- **files:** [`crates/fuzzy-dedupe-app/src/measure/mod.rs` (modify; owner T12), `crates/fuzzy-dedupe-app/src/measure/recorder.rs` (create; owner T23), `crates/fuzzy-dedupe-app/src/measure/drivers.rs` (create; owner T23), `crates/fuzzy-dedupe-app/tests/measure.rs` (test; owner T23)]
- **parallel_safe:** yes
- **conflicts_with:** [T12]
- **supersedes:** []
- **superseded_by:** []
- **evidence:** [ephemeral]
- **recovery:** revert to the last green checkpoint
- **acceptance:** PV-T23-001 proves in a headless harness that each scenario emits samples in the C-10 format, cpu_usage samples count painted frames only, a synthetic delayed frame appears as a larger input-to-paint sample, and the mode is inert when not enabled
- **sub-tasks:**
  - **T23.1 RED** — add harness tests for the sample format and inertness; expected failure: the stub emits nothing.
  - **T23.2 Verify RED** — run `rexec -- cargo nextest run -p fuzzy-dedupe-app --locked -E 'test(measure)'`.
  - **T23.3 GREEN** — implement the recorder and drivers.
  - **T23.4 Verify GREEN** — rerun targeted and app tests.
  - **T23.5 Verify Task** — run PV-T23-001 and repository gates; commit with the required `Plan-*` trailers.

#### T24: Record the design review

- **disposition:** active
- **outcome:** EV-002 records, for each of the 19 review frames, the reference PNG beside a capture of the running application on the `design` scenario, findings, and the result, plus the 200% zoom check for UI check 03
- **work_type:** verification
- **checkpoint:** one green evidence commit with the required `Plan-*` checkpoint trailers, landed by squash, carrying EV-002
- **boundary:** cross-task
- **depends_on:** [T2, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14, T15, T16, T17, T18, T19, T20, T21, T22, T23]
- **dependency_reason:** ordering-only: compares against the reference PNG set (T2), after every runtime implementation task (T4–T21 and T23, including preferences from T20 and font fallback from T21) and the C-10 layout assertions and integration suite (T22) have landed, so the captures show the integrated application (D-016)
- **requirements:** [REQ-002, REQ-012]
- **proof:** [PV-T24-001, PV-T24-002]
- **source_refs:** [spec:docs/specs/canonical/fuzzy-dedupe.md#111-authoritative-design-links-and-revision-precedence, spec:docs/specs/canonical/fuzzy-dedupe.md#1111-acceptance-checklist-for-the-implementing-agent]
- **consumes:** [reference PNG set, C-10 layout assertions, design scenario]
- **produces:** [docs/evidence/ms-1/design-review.md]
- **preserves:** [geometry is a reference target, not a reason to break text or scaling (§11.1)]
- **invariants:** [a deviation is either fixed through a correction task or accepted by the owner in writing in EV-002; EV-002 opens with the C-14 binding of the build captured; a correction task that changes rendering lists EV-002 among the records it invalidates]
- **executor_discretion:** [capture method, record layout]
- **files:** [`docs/evidence/ms-1/design-review.md` (evidence; owner T24), `docs/evidence/ms-1/design-review/**` (evidence captures with `.license` sidecars; owner T24)]
- **parallel_safe:** yes
- **conflicts_with:** []
- **supersedes:** []
- **superseded_by:** []
- **evidence:** [EV-002]
- **recovery:** a deviation blocks T24 and appends a correction task against the owning view task, then T24 reruns for the affected frames
- **acceptance:** PV-T24-001 proves EV-002 has one entry per review frame with both images, findings and a pass or owner-accepted result; PV-T24-002 proves the light and dark captures match the compact sizing and typography of the frames and the 200% zoom captures show no clipped content
- **sub-tasks:**
  - **T24.1 ANCHOR** — record the C-14 binding of the commit and build under review.
  - **T24.2 VERIFY PREREQUISITES** — confirm T22 passes and the reference manifest checksums.
  - **T24.3 RUN** — capture each frame state on the reference workstation in both themes and at 200% zoom.
  - **T24.4 TRIAGE** — classify each difference as defect, incidental offset or owner question.
  - **T24.5 RERUN** — after corrections, recapture affected frames.
  - **T24.6 CAPTURE EVIDENCE** — write EV-002 and commit with the required `Plan-*` trailers.

#### T25: Measure T1–T4 on the reference workstation

- **disposition:** active
- **outcome:** EV-003 records T1, T2, T3 and T4 from release builds on the reference workstation in the 1600 × 960 client area, with raw samples, computed percentiles and run conditions, and each target met
- **work_type:** verification
- **checkpoint:** one green evidence commit with the required `Plan-*` checkpoint trailers, landed by squash, carrying EV-003
- **boundary:** cross-task
- **depends_on:** [T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14, T15, T16, T17, T18, T19, T20, T21, T22, T23]
- **dependency_reason:** ordering-only: runs the measurement mode and consumes the C-10 sample stream from T23, on a build that already contains every runtime implementation task (T4–T21, including preferences from T20 and font fallback from T21) and the integration suite (T22), so the measured binary is the integrated application (D-016)
- **requirements:** [REQ-001]
- **proof:** [PV-T25-001]
- **source_refs:** [spec:docs/specs/canonical/fuzzy-dedupe.md#ms-1-performance-targets, adr:docs/adr/adr-0001-use-egui-for-the-v1-desktop-ui.md#amendments]
- **consumes:** [C-10 sample stream]
- **produces:** [docs/evidence/ms-1/performance.md]
- **preserves:** [no threshold changes without an owner decision in the spec]
- **invariants:** [runs execute on the reference workstation only (§3.4 exception); hardware and software are recorded for every run without personal identifiers; EV-003 opens with the C-14 binding of the measured build; a correction task that changes runtime behavior lists EV-003 among the records it invalidates]
- **executor_discretion:** [run count beyond the minimum in C-10, analysis script language]
- **files:** [`docs/evidence/ms-1/performance.md` (evidence; owner T25), `docs/evidence/ms-1/performance/**` (evidence raw samples with `.license` sidecars; owner T25)]
- **parallel_safe:** yes
- **conflicts_with:** []
- **supersedes:** []
- **superseded_by:** []
- **evidence:** [EV-003]
- **recovery:** a miss blocks T25; append a correction task against the owning task for egui remediation, rerun, and if remediation is exhausted ask the owner to change the design or the target (C-015); never weaken a target in the plan
- **acceptance:** PV-T25-001 proves from the raw samples that T1 and T2 cpu_usage p95 ≤ 8 ms and p99 ≤ 16.7 ms with wrap on and off, T2 first paint ≤ 500 ms after diff-ready and copy equal to the selected text, T3 leaves workspace state unchanged for every popup over both workspaces, and T4 input-to-paint p95 ≤ 100 ms with cancellation acknowledged within 1 s
- **sub-tasks:**
  - **T25.1 ANCHOR** — record the C-14 binding, build provenance, hardware and software.
  - **T25.2 VERIFY PREREQUISITES** — confirm release build, 1600 × 960 client area, power and display settings.
  - **T25.3 RUN** — run each scenario the C-10 minimum number of times; redirect samples into the evidence directory.
  - **T25.4 TRIAGE** — compute percentiles; route misses to correction tasks.
  - **T25.5 RERUN** — rerun affected scenarios after remediation.
  - **T25.6 CAPTURE EVIDENCE** — write EV-003 and commit with the required `Plan-*` trailers.

#### T26: Run the manual GNOME and KDE Plasma pass

- **disposition:** active
- **outcome:** EV-004 records an owner-performed or owner-supervised keyboard-only pass on GNOME and on KDE Plasma (Wayland) at 100, 125, 150 and 200% scale, at 1024 × 600, at 80% and 200% zoom, while moving between monitors of different scale, with Japanese, Arabic, emoji and long-path names, the portal folder picker, grayscale side-label check, and a KDE default-shortcut conflict review
- **work_type:** verification
- **checkpoint:** one green evidence commit with the required `Plan-*` checkpoint trailers, landed by squash, carrying EV-004
- **boundary:** cross-task
- **depends_on:** [T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14, T15, T16, T17, T18, T19, T20, T21, T22, T23]
- **dependency_reason:** ordering-only: needs the system font fallback from T21, preferences from T20 and every other runtime implementation task (T4–T19, T23), and the build verified by the integration suite from T22, so the manual pass exercises the integrated application (D-016)
- **requirements:** [REQ-006, REQ-018]
- **proof:** [PV-T26-001, PV-T26-002]
- **source_refs:** [spec:docs/specs/canonical/fuzzy-dedupe.md#19-implementation-plan, spec:docs/specs/canonical/fuzzy-dedupe.md#1110-resizing-scaling-and-keyboard-behavior, spec:docs/specs/canonical/fuzzy-dedupe.md#34-constraints, spec:docs/specs/canonical/fuzzy-dedupe.md#72-non-functional-requirements]
- **consumes:** [system font fallback, integration suite]
- **produces:** [docs/evidence/ms-1/desktop-pass.md]
- **preserves:** [X11 and other desktops are not promised (C-016)]
- **invariants:** [the checklist in C-13 is followed in full on both desktops; a missing environment blocks T26 rather than reducing scope; EV-004 opens with the C-14 binding of the build exercised]
- **executor_discretion:** [record layout, screenshot selection]
- **files:** [`docs/evidence/ms-1/desktop-pass.md` (evidence; owner T26), `docs/evidence/ms-1/desktop-pass/**` (evidence captures with `.license` sidecars; owner T26)]
- **parallel_safe:** yes
- **conflicts_with:** []
- **supersedes:** []
- **superseded_by:** []
- **evidence:** [EV-004]
- **recovery:** a defect blocks T26 and appends a correction task; a KDE shortcut conflict goes to the owner, who decides whether to change the §11.10 map in the spec
- **acceptance:** PV-T26-001 proves EV-004 records a pass for every C-13 item on both desktops with environment versions; PV-T26-002 proves every action was reached by keyboard with visible focus, focus restoration held after each popup, and long and Unicode paths stayed fully accessible through details and copy
- **sub-tasks:**
  - **T26.1 ANCHOR** — record the C-14 binding of the build and both desktop environments.
  - **T26.2 VERIFY PREREQUISITES** — confirm the owner has the KDE Plasma Wayland environment and two monitors with different scales.
  - **T26.3 RUN** — work through C-13 on GNOME, then on KDE Plasma.
  - **T26.4 TRIAGE** — route defects to correction tasks and conflicts to the owner.
  - **T26.5 RERUN** — repeat affected items after corrections.
  - **T26.6 CAPTURE EVIDENCE** — write EV-004 and commit with the required `Plan-*` trailers.

### Phase P5: Acceptance

#### T27: Assemble the evidence file and obtain owner sign-off

- **disposition:** active
- **outcome:** `docs/evidence/ms-1-evidence.md` lists each of the eight exit items with its result, proof reference and supporting record, and the owner's dated sign-off; EV-007 records the freshness review that shows every cited record validates the signed tree (D-016); the PR declares `Supporting:` with the MS-1 governing issue number and squash-lands after the owner signs
- **work_type:** verification
- **checkpoint:** one green evidence commit with the required `Plan-*` checkpoint trailers, carrying EV-005 and EV-007, landed by squash after the owner signs
- **boundary:** cross-task
- **depends_on:** [T1, T3, T21, T22, T24, T25, T26]
- **dependency_reason:** aggregates the dependency rows in docs/specs/canonical/fuzzy-dedupe.md (T1), the C-09 guard result (T3), docs/evidence/ms-1/font-fallback.md (T21), docs/evidence/ms-1/ci-snapshots.md (T22), docs/evidence/ms-1/design-review.md (T24), docs/evidence/ms-1/performance.md (T25) and docs/evidence/ms-1/desktop-pass.md (T26)
- **requirements:** [REQ-009, REQ-046]
- **proof:** [PV-T27-001, PV-T27-002]
- **source_refs:** [spec:docs/specs/canonical/fuzzy-dedupe.md#19-implementation-plan, spec:docs/specs/canonical/fuzzy-dedupe.md#173-requirement-to-test-traceability, spec:docs/specs/canonical/fuzzy-dedupe.md#appendix-b-agent-implementation-contract, adr:docs/adr/adr-0001-use-egui-for-the-v1-desktop-ui.md#amendments]
- **consumes:** [docs/specs/canonical/fuzzy-dedupe.md dependency rows, C-09, docs/evidence/ms-1/font-fallback.md, docs/evidence/ms-1/ci-snapshots.md, docs/evidence/ms-1/design-review.md, docs/evidence/ms-1/performance.md, docs/evidence/ms-1/desktop-pass.md]
- **produces:** [docs/evidence/ms-1-evidence.md, docs/evidence/ms-1/renewals.md]
- **preserves:** [every completed task's evidence file unchanged; renewals are new records in EV-007]
- **invariants:** [no exit item is marked passed without its proof reference; the owner's sign-off is written by the owner or quoted from the owner's PR approval; the signed tree is the T27 PR head, and the T27 PR changes nothing under `crates/` nor `Cargo.lock`, the root `Cargo.toml` or `rust-toolchain.toml`; a record whose C-14 binding differs from the signed tree is cited only through its EV-007 rerun or equivalence review; an equivalence review is allowed only when it names the diff and shows every changed hunk to be documentation, comments or test-only code; workstation and manual reruns follow the §3.4 exception and the owner's participation as their original tasks did]
- **executor_discretion:** [evidence file layout]
- **files:** [`docs/evidence/ms-1-evidence.md` (evidence; owner T27), `docs/evidence/ms-1/renewals.md` (evidence; owner T27), `docs/evidence/ms-1/renewals/**` (evidence raw data and captures with `.license` sidecars; owner T27)]
- **parallel_safe:** no
- **conflicts_with:** []
- **supersedes:** []
- **superseded_by:** []
- **evidence:** [EV-005, EV-007]
- **recovery:** if the owner declines sign-off, record the reasons in the PR, append correction tasks, each listing the records it invalidates, and keep T27 blocked; after the corrections land, redo the freshness review on the new signed tree
- **acceptance:** PV-T27-001 proves the evidence file maps all eight exit items and UI checks 01–08, 12, 13 and 14 to passing proofs with links to EV-001 to EV-004, EV-006 and EV-007, the final no-mutation guard, the Clippy gate and the `cargo tree` checks pass at the signed commit, and the owner's sign-off is present; PV-T27-002 proves that for each of EV-001 to EV-004 and EV-006 the C-14 binding equals the signed tree or EV-007 holds a rerun bound to the signed tree or an equivalence review naming the diff, and that a planted stale binding without either is rejected
- **sub-tasks:**
  - **T27.1 ANCHOR** — fix the commit being signed, record its C-14 binding and rerun every repository gate there.
  - **T27.2 VERIFY PREREQUISITES** — confirm every proof in Appendix B passed and every correction task is done, and collect the records each correction task invalidated.
  - **T27.3 RUN** — compare each record's C-14 binding with the signed tree using `git diff --stat "$VALIDATED_OID" "$SIGNED_OID" -- crates Cargo.lock Cargo.toml rust-toolchain.toml`; for each mismatch rerun the affected checks on the signed tree or write the equivalence review in EV-007; write the evidence file; run the final no-mutation checks.
  - **T27.4 TRIAGE** — resolve any gap through a correction task before requesting sign-off.
  - **T27.5 RERUN** — request owner review and incorporate the owner's sign-off.
  - **T27.6 CAPTURE EVIDENCE** — commit with the required `Plan-*` trailers, land the `Supporting:` PR by squash and request `done` only after its `main` runs are green.

#### T28: Prove MS-1 on `main` and land the close-out record

- **disposition:** active
- **outcome:** T27's squash commit shows green `main` runs; a close-out branch cut from that `main` adds EV-008 (`docs/evidence/ms-1-completion-report.md`: the Appendix B.3 completion report, the `main` run records of T27's squash commit, and the checkpoint OID, PR number and squash OID of T1–T27 and every completed correction task), sets the §17.3 Status cells of the MS-1-verified IDs to point at the evidence file, adds a `DEV-` row for each deviation, and harvests §13 of this plan over the same task set; its PR declares `Final:` with the MS-1 governing issue number and squash-lands with green `main` runs, which completes MS-1; T28's own checkpoint OID, PR number, squash OID and post-landing `main` run results then go to the post-landing handoff record: one handoff-only commit on `main`, made through the agent-handoff skill with the `Workflow-Admission: handoff` trailer, touching only the session-state set (`docs/handoff/state.md`, `docs/handoff/sessions/*.md`, `docs/STATUS.md` and `docs/TODO.md`)
- **work_type:** operational
- **checkpoint:** one green commit on the close-out branch with the required `Plan-*` checkpoint trailers, landed by squash through the `Final:` PR
- **boundary:** deployment
- **depends_on:** [T27]
- **dependency_reason:** consumes docs/evidence/ms-1-evidence.md from T27 and proves the `main` runs of the squash commit that landed it
- **requirements:** [REQ-045]
- **proof:** [PV-T28-001]
- **source_refs:** [spec:docs/specs/canonical/fuzzy-dedupe.md#19-implementation-plan, spec:docs/specs/canonical/fuzzy-dedupe.md#173-requirement-to-test-traceability, spec:docs/specs/canonical/fuzzy-dedupe.md#deviations-log, spec:docs/specs/canonical/fuzzy-dedupe.md#appendix-b-agent-implementation-contract, repo:.agents/skills/github-workflow/references/pr-standard.md, repo:.github/workflows/admission.yml]
- **consumes:** [docs/evidence/ms-1-evidence.md]
- **produces:** [docs/evidence/ms-1-completion-report.md, spec §17.3 MS-1 statuses]
- **preserves:** [spec requirement text unchanged, with only §17.3 Status cells and Deviations Log rows changing; nothing under `crates/` nor `Cargo.lock`, the root `Cargo.toml` or `rust-toolchain.toml` changes, so the signed tree stays the MS-1 tree; every task branch kept until this PR has landed]
- **invariants:** [the close-out branch is cut from the `main` that contains T27's squash commit; every value written before a commit already exists when it is written, so EV-008 and the §13 harvest never name T28's own checkpoint, PR, squash or runs; `admission` is observed only on `main`; a red `main` run keeps MS-1 incomplete and is fixed by a correction task through a new PR, after which PV-T28-001 reruns on the resulting `main` commit; the post-landing handoff record is the last step and records nothing about itself, so repeating or recovering it creates no further self-reference; task branches may be deleted only after that record is on `main`]
- **executor_discretion:** [close-out branch name, report layout beyond the Appendix B.3 fields, Status cell wording]
- **files:** [`docs/evidence/ms-1-completion-report.md` (evidence; owner T28), `docs/specs/canonical/fuzzy-dedupe.md` (modify §17.3 Status cells and Deviations Log only; owner T1), `docs/plans/2026-11-22-ms-1-ui-on-fixtures-plan.md` (modify §13 Close-out harvest only; owner T28)]
- **parallel_safe:** no
- **conflicts_with:** [T1]
- **supersedes:** []
- **superseded_by:** []
- **evidence:** [EV-008]
- **recovery:** before landing, a red check follows §7; after landing, a red `main` run is triaged under §10.3 and fixed forward through a new PR; if T28 is interrupted, `recover` rebuilds completed tasks from the retained branches and `gh run list --commit "$SQUASH_OID"` re-reads the observed runs
- **acceptance:** PV-T28-001 proves that T27's squash commit and the close-out squash commit each have `main` runs of `rust.yml`, `reuse.yml`, `format`, `lint-markdown`, `validate-specs` and `admission` concluding `success` and that `gh-workflow admission --branch main` reports no unadmitted commit; that EV-008 holds every Appendix B.3 field and the checkpoint OID, PR number and squash OID of T1–T27 and every completed correction, and no T28 identifier; that the post-landing handoff record on `main` carries T28's checkpoint OID, PR number, squash OID and `main` run results and touches only the session-state set; that every §17.3 Status cell of an MS-1-verified ID names its proof in the evidence file; that a `DEV-` row exists for each deviation (the Deviations Log is unchanged when there is none); and that the spec gates, Prettier and markdownlint pass on the changed files
- **sub-tasks:**
  - **T28.1 PREFLIGHT** — confirm T27 is `done`, its PR has squash-landed, and the branch for T28 is cut from that `main`.
  - **T28.2 VERIFY** — watch each `main` run on T27's squash commit with one blocking `gh run watch "$RUN_ID" --exit-status`, then run `gh-workflow admission --branch main`; triage any red run under the recovery rule.
  - **T28.3 APPLY** — write EV-008, set the §17.3 Status cells, add each `DEV-` row and harvest §13 of the master plan, covering T1–T27 and completed corrections only.
  - **T28.4 VERIFY REFERENCES** — run `project-standards spec validate --strict`, `spec lint --strict`, Prettier and markdownlint on the changed files.
  - **T28.5 LAND** — commit with the required `Plan-*` trailers (`$T28_CHECKPOINT`), open the `Final:` PR, land it with `gh-workflow land --pr N --method squash` (`$T28_SQUASH`) and watch its `main` runs to green.
  - **T28.6 RECORD** — write the post-landing handoff record on `main` with `$T28_CHECKPOINT`, the PR number, `$T28_SQUASH` and the `main` run results, validate it with `project-standards agent-handoff validate --repo .` and `drift-check --repo .`, and only then request `done` with `--commit "$T28_CHECKPOINT"`.

## 10. Integration, Migration, and Recovery

### 10.1 Integration Sequence

1. Confirm PC-1 to PC-7. Start T1 and T3 in parallel, each in its own worktree and branch, with T4 and T11, which are D-014 tasks and so run and land one at a time in either order; T10, also a D-014 task, starts once T3 is green on `main` and no other D-014 task is in flight; T2 follows T11.
2. After T4: T5 and T7 in parallel, and T6 once T1 has landed and no other D-014 task is in flight; then T8 after T7; T9 after T2 and T4. After T4, T10 and T11: T12.
3. After T12: T13, T14, T16, T17, T18 and T19 in parallel as their engine prerequisites land; T15 after T13 and T14; T20 after T15 and T16; T21 after T1, T10 and T13, outside any other D-014 task.
4. After all views and popups: T22 and T23 in parallel (T22 is a D-014 task); T24, T25 and T26 once every runtime task, T22 and T23 have landed.
5. T27 after T24 to T26, in a `Supporting:` PR the owner signs; then T28 in the `Final:` close-out PR, followed by the post-landing handoff record on `main` (T28.6).

Every step waits for green `main` runs: a dependent task branches from `main` only after its prerequisites' squash commits show green `rust.yml`, `reuse.yml`, `format`, `lint-markdown`, `validate-specs` and `admission` runs (D-009). A D-014 task rebases on the current `main` immediately before landing and repeats the minimal lock update and the `--locked` gates if another D-014 task landed first.

The orchestrator owns the generated checklist and is the only writer of `state`; parallel executors report checkpoint OIDs to it. Every PR declares `Supporting:` with the MS-1 governing issue number under `## Governing work`, except T28's close-out PR, which declares `Final:` with the MS-1 governing issue number. Each PR lands with `gh-workflow land --pr N --method squash` after its own gates pass, and its `main` runs are confirmed green before the task is marked `done`. The executor records each task's checkpoint OID, PR number and squash OID in the generated checklist evidence and keeps every task branch, locally and on origin, until the T28 PR has landed.

### 10.2 Migration / State / Configuration Transition

- Required: no data migration. `preferences.toml` version 1 is new; no earlier user state exists.
- Idempotency: restarting with the same preferences file restores the same view; T20 proves the round trip.
- Point of no return: none. Every task is revertible by reverting its PR's squash commit.
- Rollback / forward repair: revert the squash commit of the faulty PR, then append a correction task. Never rewrite `main` history or delete a task branch, because `recover` finds checkpoint commits through `git log --all`. A revert or correction that changes `crates/`, `Cargo.lock`, the root `Cargo.toml` or `rust-toolchain.toml` after an evidence record was written makes that record stale until T27 renews or reviews it (D-016).

### 10.3 Late Failure and Correction

A failure found by T22, T24, T25, T26 or T27 blocks that task. The executor appends a correction task with `corrects:` naming the owning task and `discovered_from:` naming the blocked task, lists in its `invariants` every evidence record it invalidates (`invalidates: [EV-…]`, judged against C-14: any change to the bound inputs invalidates every record bound to them unless the change is documentation, comments or test-only), writes only files annotated with the owning task as owner or new files, claims `Cargo.lock` and `.github/boundaries.toml` under D-014 if it changes the resolution, runs `sync`, completes the correction, and reruns the blocked verification from its failed step. Records it invalidates whose tasks are already `done` are renewed or reviewed by T27 in EV-007; completed task definitions and their evidence files are never edited. A T1–T4 miss that egui remediation cannot fix is escalated to the owner (C-015); if the owner changes a target or the design, the spec is revised first and this plan follows the pause, revise and replace transaction. A spec defect found during execution pauses the affected task and goes to the owner as an `OQ-` or `DEV-` row (Appendix B of the spec).

## 11. Risks, Assumptions, and Open Questions

### 11.1 Risks

| ID | Risk | Likelihood | Impact | Treatment | Owner / Task |
| --- | --- | --- | --- | --- | --- |
| R-001 | No `egui_table` or `egui_kittest` release matches the newest egui | medium | medium | Pin egui to the newest version both support; record in the PR | executor / T11 |
| R-002 | T1 or T2 frame budget missed with 100,000 rows or whole-file selection on 5 MB | medium | high | Correction tasks within egui; owner decides design or target changes | owner / T25 |
| R-003 | Runner image or Mesa update changes snapshot renders | medium | medium | Pin the runner image, record the Mesa version, regenerate baselines in a reviewed PR | executor / T22 |
| R-004 | The fontconfig crate needs build-time system packages on CI or the rexec worker | medium | medium | Prefer runtime loading in T1's choice; otherwise add packages through an owner-approved change | owner / T1, T21 |
| R-005 | The pinned Noto Emoji release lacks a glyph used by fixture names | low | low | Choose fixture emoji from the font's coverage; report any gap to the owner | executor / T11, T21 |
| R-006 | No KDE Plasma Wayland environment with mixed-scale monitors is available | medium | high | T26 blocks; the owner supplies the environment | owner / T26 |
| R-007 | Figma MCP cannot return PNG bytes | medium | low | Fall back to an authorized REST export or owner export | owner / T2 |
| R-008 | A task branch is deleted before close-out, making its checkpoint unreachable to `recover` | low | medium | Never delete task branches before close-out; `delete_branch_on_merge` stays false; close-out records each OID and PR number | executor / all tasks |
| R-009 | eframe or egui default features pull a license outside the `cargo deny` allowlist | medium | low | Disable default fonts and audit features in T11 | executor / T11 |
| R-010 | Two D-014 tasks resolve `Cargo.lock` differently, or a rebase leaves a stale resolution | medium | medium | Serialize D-014 tasks with `conflicts_with`; rebase on `main`, update minimally, review the lock diff and rerun the `--locked` gates before landing | executor / D-014 tasks |
| R-011 | Late corrections leave workstation or manual evidence describing an earlier build | medium | high | C-14 binding on every record; correction tasks list invalidated records; T27 reruns or records an equivalence review (D-016) | executor / T27 |
| R-012 | A Clippy upgrade changes how `disallowed-methods` paths resolve, silently dropping an entry | low | high | The pinned toolchain fixes Clippy; T3's textual layer and configuration checks remain; PV-T3-002 canaries rerun when the toolchain pin changes | executor / T3 |

### 11.2 Assumptions

| ID | Assumption | Impact if False |
| --- | --- | --- |
| A-001 | MS-0 delivers PC-1 to PC-7 under the names in its plan's produced contracts (MS-0 Appendix A C-1 to C-7, D-008, D-011, D-012). | Tasks touching a missing surface block until MS-0 is corrected. |
| A-003 | Fixture "files" are deterministic in-memory buffers (D-005). | T6 writes fixture files once into the repository instead; no runtime writes are added. |
| A-004 | Zoom steps are 10 percentage points between 80% and 200%. | T11 changes the step constant. |
| A-010 | Numeric layout assertions allow ±1 logical unit for the incidental border offsets §11.3 describes; the application, drawing no window outline, is asserted at 472 units per pane at 1024 × 600 (§11.3 v0.9). | T22 tightens tolerances. |
| A-011 | The executing agent can launch GUI applications in the owner's GNOME Wayland session on the reference workstation; otherwise the owner runs the documented commands for T21, T24 and T25. | Those tasks wait for the owner; acceptance is unchanged. |

### 11.3 Open Questions

| ID | Question | Current Assumption | Blocking? | Owner | Needed By | Status |
| --- | --- | --- | --- | --- | --- | --- |
| PQ-001 | Does exit item 8 forbid the app-owned preferences replace and log retention, or only user-file moves and deletes? | — | No | Owner | Before T10 lands | Answered by SPEC-FZDP v0.9 exit item 8 |
| PQ-002 | DR-009 lists window position, which Wayland clients cannot read or set; what should MS-1 persist on GNOME and KDE Wayland? | — | No | Owner | Before T20 starts | Answered by SPEC-FZDP v0.9 DR-009 |
| PQ-003 | How should About's View license and Third-party notices actions open the texts? | — | No | Owner | Before T19 starts | Answered by SPEC-FZDP v0.9 FR-028 and §11.6 |
| PQ-004 | What copyright and license annotation do the Figma PNG exports and UI captures carry under REUSE? | — | No | Owner | Before T2 lands | Answered by SPEC-FZDP v0.9 §11.1 |
| PQ-005 | If system emoji fonts cannot render in egui, may the build embed egui's bundled emoji font or another one beyond the four Plex files? | — | No | Owner | When T21 reports | Answered by SPEC-FZDP v0.9 §11.7 Fonts |
| PQ-006 | Is a member's default counterpart the reference whenever a direct match exists, or its strongest direct match (§6 Counterpart and §10.1 versus §11.4 row menu)? | — | No | Owner | Before T4 lands | Answered by SPEC-FZDP v0.9 §6 and §10.1 (reference first) |
| PQ-007 | What does the UI show when the user queues a file that is the counterpart of an already queued file? | — | No | Owner | Before T7 starts | Answered by SPEC-FZDP v0.9 FR-025 |
| PQ-008 | How does the UI report a Kept or per-file Protect decision that a binding change invalidates, for example through Make this the reference? Invalidating a per-file Protect makes the file queueable again. | The C-05 outcome lists every invalidated decision; Findings shows the row's resulting state, and the status bar keeps only the §10.1 unqueued-count message; no new UI text is added in MS-1 | No | Owner | Before T13 lands | Open |

## 12. Final Verification

- Every requirement in §6 maps to a completed task whose Appendix B proof passed; no correction task remains open.
- At the signed commit, these pass: `rexec -- cargo fmt --all --check`, `rexec -- cargo clippy --workspace --all-targets --locked -- -D warnings`, `rexec -- cargo nextest run --workspace --locked`, `rexec -- cargo test --doc --workspace --locked`, `rexec -- cargo deny check`, `rexec -- cargo audit`, `reuse lint`, the Markdown checks, `python3 .github/scripts/check-boundaries.py`, and every `rust.yml` job (the seven MS-0 jobs, including `boundaries`, and the UI-test job) and the `reuse.yml` job on the T27 PR. `admission` is checked on `main` only.
- The no-mutation guard and the Clippy gate pass with `clippy.toml` unchanged from T3 except for additions, every `#[expect(clippy::disallowed_*)]` sits at a C-09 allowed location, `rexec -- cargo tree --workspace -i trash` fails because no package matches, `fuzzy-dedupe-fileops` is unchanged from MS-0, and no SPDX line, `REUSE.toml` annotation or Cargo `license` field added by MS-1 declares an `-or-later` expression.
- Every direct dependency in a workspace manifest has a §8.3 row and a C-7 entry for its declaring crate, and `Cargo.lock` differs from MS-0's only by the crates MS-1 added and their transitive closure.
- Every record EV-005 cites validates the signed tree (REQ-046, EV-007).
- `rexec -- cargo tree -p fuzzy-dedupe-domain` lists no workspace crate, and only `fuzzy-dedupe-app` depends on egui or eframe.
- EV-001 to EV-008 exist, contain no personal identifiers, and EV-005 carries the owner's sign-off.
- After T27: its squash commit and T28's each show green `main` runs of `rust.yml`, `reuse.yml`, `format`, `lint-markdown`, `validate-specs` and `admission`; MS-1 is complete only when T28's PR has landed (REQ-045), and the post-landing handoff record then carries T28's checkpoint OID, PR number, squash OID and runs. No committed file names the commit that adds it.
- Every open question in §11.3 is answered or explicitly accepted by the owner in EV-005.

## 13. Close-out

- **Completed:** pending; T28 records the checkpoint OID, PR number and squash OID of T1–T27 and every completed correction here and in EV-008; T28's own identifiers and post-landing runs go to the post-landing handoff record on `main`; task branches are deleted only after that record is on `main`.
- **Decisions / deviations harvested:** pending; implementation deviations become `DEV-` rows in the spec, written by T28 (Appendix B.1).
- **Risks closed / accepted:** pending.
- **Deferred/discovered work filed:** pending.
- **Source/ADR/handoff reconciliation:** pending; T28 sets the §17.3 statuses; after T28 lands, the post-landing handoff record: one handoff-only commit on `main`, made through the agent-handoff skill with the `Workflow-Admission: handoff` trailer, touching only the session-state set (`docs/handoff/state.md`, `docs/handoff/sessions/*.md`, `docs/STATUS.md` and `docs/TODO.md`), carries T28's checkpoint OID, PR number, squash OID and `main` run results and is the last step (T28.6).
- **Scratch teardown:** only after the T28 PR has landed with green `main` runs and the post-landing handoff record is on `main`, EV-001 to EV-008 are on `main` and no irreplaceable note remains in `.project-pipeline/2026-11-22-ms-1-ui-on-fixtures/`.

## Appendix A. Interface and State Contracts

| Contract | Owner / Producer Task | Consumer(s) | Current | Planned / States | Errors / Limits | Compatibility / Invariant | Source |
| --- | --- | --- | --- | --- | --- | --- | --- |
| C-01 Domain results model | T4 | T5–T9, T12–T22 | None | Newtype IDs (`ScanId`, `GroupId`, `FileId`, `RulesRevision`, `RequestId`, `PlanRevision`); `RelationKind` {SameBytes, SameText, SimilarText, Containment}; member `Measurement` {Identical, SameText, Similar per-mille, Containment per-mille with direction InsideCounterpart or ContainsCounterpart}; `Member` {file, name, folder, display path, size bytes, modified, folder-protected, role Reference or Member with counterpart, unique lines, verification HashCandidate or ByteVerified}; `Group` {id, label "G001", members, direct-match edges}; `ScanResults` {scan id, immutable setup and rules copy, groups, issues, coverage Complete, Partial or Empty}; `MatchingRules` {mode, minimum similarity, four Ignore flags, revision}; `ScanSetup` {folders with protect flag, file types}; `ScanProgress`; `PlanItemStatus` (§10.4) | Not-implemented outcome until a module lands | MS-1 provisional: later milestones may extend these types through their own plans; IDs stable within a scan; reference first; counterpart is the reference when a direct edge to it exists, otherwise the best direct match, or a member chosen by review in Compare (§6, §10.1); declared Unique uses the §10.1 measure; no group-wide score | spec §11.8, §10.1, §6 |
| C-02 Engine facade | T4 | T5–T8, T12–T20 | None | Review requests are synchronous calls returning an outcome; `StartScan`, `CancelScan`, `LoadComparison`, `BuildCleanupPlan`, `ExecuteApprovedPlan`, `StopCleanup` are submitted into bounded `crossbeam-channel` queues; the app drains events in bounded batches; every event carries its scan ID, request ID or plan revision | A full request queue returns a busy outcome rather than blocking the UI thread; cancellation is always admitted | No GUI or OS type crosses the facade; unrestricted path-delete callbacks do not exist (§11.8) | spec §11.8, §11.9 |
| C-03 Simulated scan | T5 | T16, T22, T23 | None | Started → Progress* → Completed or Partial (after cancel) | Cancel acknowledged ≤ 1 s | Different scan IDs never merge | spec §11.6, §11.9 |
| C-04 Aligned comparison | T6 | T14, T15, T17, T22 | None | Rows of (left source line or spacer, right source line or spacer, kind Equal, OnlyA, OnlyB, Changed with inline spans); hunk anchors; Only-in-A and Only-in-B counts and the unique lines themselves by the §10.1 unique-line measure, labeled normalized in Normalized mode; copy text in file order with one LF after each line; key (target snapshot, counterpart snapshot, rules revision, mode) | Superseded requests cancelled or ignored; truncated or unreadable previews state what is missing | Unique-line measure is content-based and independent of alignment; one measure feeds Findings, Compare and Review unique text; spacers have no line number | spec §10.1, §11.5, IR-002 |
| C-05 Review service | T7 | T8, T13, T15–T17 | None | Operations: request queue (with origin and counterpart), unqueue, acknowledge, mark kept, protect file, make reference, queue identical copies (scope), clear for new scan, and a fixture-injected evidence update that sets a member's target snapshot, its counterpart's snapshot or the rules revision. Make reference runs in order: remove the new reference from the queue; recompute counterparts by the §10.1 rule, discarding Compare-chosen ones; unqueue each other queued member whose counterpart changed and clear its acknowledgment; return the unqueued count for the status bar. Outcomes: queued, needs review, refused with reason and label (reference, protected, "Counterpart queued", "Counterpart of a queued file", not byte-identical), and the list of decisions the operation invalidated | Refusals never change state | I-1 reference never queued; I-2 protected (folder-protected or with a valid per-file Protect) never queued; I-3 every queued target's recorded counterpart is unqueued; I-4 no group fully queued; I-5 fuzzy or normalized queueing needs a valid acknowledgment; I-6 bulk skips kept; I-7 each Kept, per-file Protect and unique-text acknowledgment decision, and each queue approval, records when made a binding of (target file and its snapshot, counterpart file and its snapshot, rules revision) and is valid only while all three equal the current values; a change to any one, including a different counterpart file, invalidates that decision alone; an invalidated Kept or per-file Protect returns the file to undecided, and a queued target whose acknowledgment or approval is invalidated is unqueued; I-8 counterparts always follow the §10.1 rule except a member the user reviewed the row against in Compare; I-9 clear for new scan removes the queue and every decision of every kind, while folder protection, which belongs to the scan setup and is never a decision, survives it and every binding change; I-10 every outcome lists the decisions it invalidated (PQ-008) | spec §10.1, §11.4, FR-025 (v0.9), DR-008 |
| C-06 Simulated cleanup | T8 | T18, T22 | None | planned → blocked (no Trash) or approved → validating → executing → succeeded, failed or stopped; needs_reconciliation scripted; a stale precondition stops before the file (stopped, "Stopped: changed") and a Trash failure stops after the failed move (failed, ERR-003), each named with its reason and taken out of the plan, failures counted as failed; a stop ends authorization, moved items stay moved, unreached items return to planned; targeted retry after simulated reconciliation and revalidation excludes succeeded items; an in-memory per-item journal backs Show journal | Second execution with one authorization refused | IDs only; no filesystem call; no skipped state | spec §10.4, §11.6 |
| C-07 Platform adapters | T10 (fonts: T21) | T12, T16, T19–T21 | None | Per-user config and state directories; async folder pick returning chosen directories; opener for a folder or web address with the desktop's default application; app-owned writer confined to adapter directories; log expiry taking the current date and deleting only the application's own log files dated more than 7 days earlier in the adapter-resolved log directory, returning the names it deleted (D-015); system fallback fonts returned as font bytes with family hints | Errors are typed and shown as actionable status; no panic on missing portal | Signatures carry no Linux-only identity or path assumption | spec §8.5, NFR-008 |
| C-08 `preferences.toml` | T20 | T12–T16 | None | `version = 1`; window size and maximized state (no position); zoom; theme override System, Light or Dark; rail collapsed; findings column widths by column ID; compare split ratio, wrap, linked scroll; current scan setup (folders with protect flags, file types, saved matching rules) | Unreadable or newer version: start with defaults, log a warning, do not overwrite until a preference changes | Results, queue and decisions never stored | spec DR-009, §18.2 |
| C-09 No-mutation guard | T3 | T10, T12, all tasks | None | **Semantic layer**, the workspace `clippy.toml` enforced by the `clippy` job: `disallowed-methods` lists at least `std::fs::remove_file`, `std::fs::remove_dir`, `std::fs::remove_dir_all`, `std::fs::rename`, `std::fs::hard_link`, `std::fs::soft_link`, `std::fs::set_permissions`, and the write and copy entry points `std::fs::write`, `std::fs::copy`, `std::fs::create_dir`, `std::fs::create_dir_all`, `std::fs::File::create`, `std::fs::File::create_new` and `std::fs::OpenOptions::open`; `disallowed-types` lists at least `std::process::Command`. **Allowlist**, each use an item-level `#[expect(clippy::disallowed_methods, reason = "…")]` or `#[expect(clippy::disallowed_types, reason = "…")]`: `crates/fuzzy-dedupe-platform/src/app_files.rs` for the write, create, open and `rename` entries (the app-owned writer); `crates/fuzzy-dedupe-platform/src/log_retention.rs` for `std::fs::remove_file` only (rotating the application's own logs, which exit item 8 allows, D-015); `crates/fuzzy-dedupe-platform/src/open.rs` for `std::process::Command` only; and integration-test files under `crates/*/tests/` for the write and create entries only, to build fixtures under the test's own `CARGO_TARGET_TMPDIR` directory. No `allow` form, no crate-level allowance under `src/` and no allowance inside a `#[cfg(test)]` module under `src/` is permitted. **Textual layer**, the guard test: strips comments and string literals and joins lines, then flags in every `crates/*/src/**/*.rs` outside the allowlist a listed name reached by a qualified path (`fs::remove_file`), a `use` tree under `std::fs` or `std::process` that imports, renames with `as`, globs or re-exports (`pub use`) a listed item or aliases the module, the `.rename(` method form and `Command::new`, including inside `#[cfg(test)]` modules; flags the delete, rename, link, permission and process names in `crates/*/tests/**`; checks that `clippy.toml` holds the minimum lists and is the only Clippy configuration file, that no manifest or workspace lint table lowers either lint, that every `expect` of either lint sits at an allowlisted location and that no `allow` form exists; and fails on any manifest or `Cargo.lock` package whose name contains `trash` | Canary snippets for each rule and form must be detected. Textual-layer residue: paths assembled by macros and items re-exported under a new name through another workspace crate; the Clippy layer covers both | Scope narrows only by an owner-approved MS-5 plan; `clippy.toml` entries may be added, never removed | spec §19 exit 8 |
| C-10 Measurement and layout targets | T22 (layout), T23 (samples) | T24, T25 | None | Layout: rail 152/48, status 26, toolbar 32, filter row 28, table header 30, body row 28, table 1424 × 822, column widths per §11.4, compare panes 708 at 1600 and 472 at 1024 (A-010), popup sizes per §11.6, token colors per §11.7. Samples: CSV lines `scenario,run,metric,value_ms` with metrics `cpu_usage`, `first_paint`, `input_to_paint`, `cancel_ack`; at least 3 runs of at least 60 s per scenario | Missing or empty sample stream fails the run | Percentiles computed from raw samples, not averages | spec §11.3, §11.9 |
| C-11 Review frames | T2 | T9, T22, T24 | None | Findings `13:648`, Compare `13:1304`, Matching rules `16:735`, New scan `16:806`, Review unique text `16:932`, Review cleanup `16:886`, 01a `28:1399`, 01b `28:2171`, 01e `30:2370`, cleanup running `27:1390`, cleanup finished `27:1533`, 01d `29:2214`, 02b `29:3648`, Findings dark `37:2466`, Compare dark `37:2548`, cleanup dark `37:2732`, main menu open `44:3954`, Keyboard shortcuts `44:3681`, About `44:3858`. Snapshot states: the 16 non-dark frames, each in light and dark (32 snapshots) | A missing node is a §11.1 defect | Node IDs from §11.1 at v0.9 | spec §11.1 |
| C-12 Presentation core | T12 | T13–T20, T23 | None | §11.8 state owners; dialog host API taking a dialog kind and invoker; UiIntent queue drained by `app.rs`; measurement hook stub | Second popup request refused | At most one popup; stale-origin events discarded | spec §11.8 |
| C-13 Manual pass checklist | T26 | T27 | None | Per desktop: scales 100, 125, 150, 200%; 1024 × 600; zoom 80% and 200%; move between mixed-scale monitors; every §11.10 chord; focus restoration after every popup; Japanese, Arabic, emoji and long paths; portal folder picker; grayscale side labels; KDE default shortcut conflicts | Unavailable environment blocks | Same checklist on both desktops | spec §19 exit 6, §11.10, NFR-005 |
| C-14 Evidence binding | T21, T22, T24, T25, T26 (records); T27 (comparison and EV-007) | T27, T28 | None | Each of EV-001 to EV-004 and EV-006 opens with a binding block: `application_commit` (the commit whose build was exercised; the evidence commit may follow it only by changes under `docs/evidence/`), `crates_tree` (`git rev-parse "$C":crates`), `cargo_lock` (`git rev-parse "$C":Cargo.lock`), `cargo_toml` (`git rev-parse "$C":Cargo.toml`), `rust_toolchain` (`git rev-parse "$C":rust-toolchain.toml`), and the build profile and provenance. EV-007 holds one row per cited record: its binding, the signed binding, the verdict (equal, renewed or equivalent), and for renewed the rerun results and raw data bound to the signed tree, or for equivalent the `git diff` range and the classification of every changed hunk | A missing field, or a mismatch with neither a renewal nor an equivalence review, fails PV-T27-002 | An equivalence review covers only documentation, comments or test-only changes; any other change needs a rerun of the affected checks; correction tasks list the records they invalidate (D-016) | spec §19, §11.9; D-016 |

## Appendix B. Requirement-to-Proof Traceability

| Proof ID | Requirement(s) | Task | Method | Oracle | Command / Procedure | Expected Result | Negative Control | Environment | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| PV-T1-001 | REQ-031 | T1 | inspection | §8.3 rule and owner approval on the PR | Review the merged §8.3 diff and the PR approval | Rows for the named fontconfig crate, `unicode-normalization` and development-only `proptest`; owner approval present | A PR merged without owner approval, or a row missing for a crate later added, is rejected | GitHub | ephemeral |
| PV-T2-001 | REQ-003 | T2 | inspection | §11.1 review-frame list (C-11) | Compare manifest node IDs with C-11; `sha256sum -c`; `reuse lint` | 19 files, one per node ID, checksums match | A missing or extra frame fails the node-ID comparison | local | ephemeral |
| PV-T3-001 | REQ-008 | T3 | unit and workspace scan | C-09 textual and configuration rules | `rexec -- cargo nextest run -p fuzzy-dedupe-app --locked -E 'test(no_file_mutation)'` | Clean workspace passes; every textual and configuration canary detected; mentions in strings and comments accepted; `test` job runs without `--no-tests=warn` | Canaries under `src/` in qualified, imported (`use std::fs::remove_file;`), aliased (`use std::fs::rename as mv;`, `use std::process as p;`), re-exported (`pub use std::fs::remove_dir_all;`), multiline and `#[cfg(test)]`-module forms; `Command::new` outside `open.rs`; a delete call in a `tests/` file; an `allow(clippy::disallowed_methods)`; an `expect` outside the allowlist; a `clippy.toml` missing `std::fs::remove_file`; a second `clippy.toml` under a crate; and a lockfile with `trash` each fail | rexec, CI | ephemeral |
| PV-T3-002 | REQ-008 | T3 | static analysis (Clippy) | Clippy `disallowed_methods` and `disallowed_types` with the C-09 `clippy.toml` | `rexec -- cargo clippy --workspace --all-targets --locked -- -D warnings` on the clean tree and on each temporary canary | Clean tree passes; every canary fails naming its lint; a canary at an allowlisted path with its `expect` passes | `std::fs::remove_file` fully qualified in app source; `use std::fs::remove_file as rm;` then `rm(…)`; `use std::fs as f;` then `f::rename(…)`; `pub use std::fs::remove_dir_all as tidy;` in domain called from app; `use std::process::{self as p};` then `p::Command::new(…)`; a call split across lines; the same call in a `#[cfg(test)]` module under `src/`; a call in `log_retention.rs` without its `expect`; and an `expect` with no disallowed call each fail | rexec | ephemeral |
| PV-T4-001 | REQ-020 | T4 | unit and contract | Exact row count 100,000; C-01 invariants | `rexec -- cargo nextest run -p fuzzy-dedupe-engine --locked -E 'test(fixtures_contract)'` | Count, determinism and invariants hold | A generator off by one group fails the exact count | rexec | ephemeral |
| PV-T5-001 | REQ-022 | T5 | concurrency test | §11.6 and T4 cancel bound | `rexec -- cargo nextest run -p fuzzy-dedupe-engine --locked -E 'test(scan_sim)'` | Monotonic progress; cancel ack ≤ 1 s in 20 runs; partial status | A worker that ignores the cancel flag exceeds 1 s and fails | rexec | ephemeral |
| PV-T6-001 | REQ-021 | T6 | unit | Generator edit script and an independent set-membership count of the §10.1 measure | `rexec -- cargo nextest run -p fuzzy-dedupe-engine --locked -E 'test(compare)'` | Size, line and change ratios in range; declared Unique equals computed | A fixture whose declared Unique differs by one fails | rexec | ephemeral |
| PV-T6-002 | REQ-023 | T6 | contract | Generator edit script, independent of `similar` | Same command | Alignment equals the script; stale response never current; work on a worker thread | A response with an older request ID delivered as current fails | rexec | ephemeral |
| PV-T6-003 | REQ-042 | T6 | unit | §10.1 unique-line definition and the §17.3 UI check 06 fixture list | `rexec -- cargo nextest run -p fuzzy-dedupe-engine --locked -E 'test(compare)'` | Each fixture's count and copy text equal the hand-written expected values | Counting diff-removed lines makes reordered identical lines unique and fails; counting distinct texts instead of occurrences fails | rexec | ephemeral |
| PV-T7-001 | FR-025 | T7 | property (exhaustive enumeration) | Invariants I-1 to I-4 | `rexec -- cargo nextest run -p fuzzy-dedupe-engine --locked -E 'test(review_policy)'` | Invariants hold for all shapes ≤ 5 and sequences ≤ depth 6; both queue orders refused with their labels | A policy that checks only the row's own counterpart, not reverse dependents, fails I-3 | rexec | ephemeral |
| PV-T7-004 | REQ-044 | T7 | unit (state machine) | §10.1 Make this the reference order | `rexec -- cargo nextest run -p fuzzy-dedupe-engine --locked -E 'test(review_policy)'` | New reference unqueued; changed-counterpart members unqueued with acknowledgments cleared; others unchanged; count returned | Keeping a Compare-chosen counterpart after the change, or unqueuing a member whose counterpart did not change, fails | rexec | ephemeral |
| PV-T7-002 | FR-017 | T7 | unit | §11.4 bulk rules and §6 Kept and Protected | Same command | Protected never queued; bulk skips kept, protected, reference, non-identical | A bulk queue including a kept file fails | rexec | ephemeral |
| PV-T7-003 | REQ-026 | T7 | unit (state machine) | DR-008 binding rule as C-05 I-7 and I-9 | Same command | For each of Kept, per-file Protect and acknowledgment and each of target snapshot, counterpart snapshot and rules revision, a change to that component alone invalidates exactly that decision; a counterpart change through Make this the reference or Compare invalidates the decisions bound to the old counterpart; unchanged-binding updates keep every decision; a new scan clears every kind; folder protection persists | A decision keyed by target only survives a counterpart-snapshot change and fails; a Kept or Protect that ignores the rules revision fails; clearing folder protection on a new scan or a binding change fails; invalidating a decision whose binding is unchanged fails | rexec | ephemeral |
| PV-T8-001 | REQ-025 | T8 | unit (state machine) | §10.4 table | `rexec -- cargo nextest run -p fuzzy-dedupe-engine --locked -E 'test(cleanup_sim)'` | All legal transitions; illegal rejected; single execution; stop and revised plan | A second submission executing twice fails; a Trash failure reported as "Stopped: changed" or counted as moved fails | rexec | ephemeral |
| PV-T9-001 | REQ-038 | T9 | unit | Values transcribed from the reference PNGs | `rexec -- cargo nextest run -p fuzzy-dedupe-engine --locked -E 'test(design_scenario)'` | Scenario equals the transcribed table | A swapped label fails the comparison | rexec | ephemeral |
| PV-T10-001 | NFR-008 | T10 | unit and gate | XDG specification paths; MS-0 `boundaries` check (D-011, C-7) | `rexec -- cargo nextest run -p fuzzy-dedupe-platform --locked`; `portability` and `boundaries` jobs | Paths resolve for set, unset and relative XDG values through the injected lookup; refusals hold; confinement and C-7 checks pass with the `etcetera` and `rfd` entries | A resolver that honors a relative `XDG_STATE_HOME` fails; a writer target outside the adapter directories is refused; `rfd` declared without its C-7 entry fails `boundaries`; any `set_var` or `remove_var` in a test is rejected in review and by the `unsafe_code` lint | rexec, CI | ephemeral |
| PV-T10-002 | REQ-027 | T10 | unit (injected clock) | NFR-009 "kept for 7 days"; D-015 date rule | `rexec -- cargo nextest run -p fuzzy-dedupe-platform --locked -E 'test(log_retention)'` | With fewer than seven app logs spread over several weeks, those dated more than 7 days before the injected date are deleted; the log dated exactly 7 days earlier and newer ones stay; unrelated, non-matching and unparseable-date files stay; after the injected date advances one day the newly expired log is deleted | A count-only cap keeps a log dated 20 days earlier and fails; an off-by-one rule deletes the 7-day-old log and fails; deleting the unrelated file fails | rexec | ephemeral |
| PV-T11-001 | REQ-029 | T11 | unit | §11.7 tables; WCAG contrast formula | `rexec -- cargo nextest run -p fuzzy-dedupe-app --locked -E 'test(theme_tokens)'` | Exact tokens and styles; all declared pairings ≥ 4.5:1 | `quiet` on `diffASoft` used for line numbers falls below 4.5:1 and must be rejected | rexec | ephemeral |
| PV-T11-002 | REQ-030 | T11 | inspection | REUSE specification; icon inventory; pinned release digests | `reuse lint`; inventory test; `sha256sum` of the Noto Emoji file; `cargo tree -e features -p fuzzy-dedupe-app` | Lint passes; every icon used; digest matches the notice; `default_fonts` absent | An unreferenced icon fails the inventory test; an enabled `default_fonts` feature fails | local, CI | ephemeral |
| PV-T12-001 | FR-027 | T12 | integration (headless egui_kittest) | FR-027 acceptance | `rexec -- cargo nextest run -p fuzzy-dedupe-app --locked -E 'test(shell)'` | Recolor in next frame for system change and each override | A theme applied only at startup fails | rexec | ephemeral |
| PV-T12-002 | REQ-035 | T12 | integration (headless egui_kittest) | §11.2 and §11.3 values | Same command | Title, rail widths, menu entries, status height, minimum size | Rail collapsing at 1280 rather than below it fails | rexec | ephemeral |
| PV-T12-003 | REQ-027 | T12 | integration | NFR-009 and §18.5 | Same command | Log under overridden state dir; `RUST_LOG` honored; log expiry invoked at startup and at the simulated daily rollover with the injected date, outside the UI pass; open-folder call | A log written under the working directory fails; expiry invoked only at startup fails the rollover case | rexec | ephemeral |
| PV-T12-004 | REQ-028 | T12 | integration (headless egui_kittest) | §11.10 table | Same command | Every global chord dispatches; blocked under a popup; no single-letter binding | A shortcut reaching the table while a popup is open fails | rexec | ephemeral |
| PV-T12-005 | REQ-032 | T12 | integration (headless egui_kittest) | §11.6 shared behavior | Same command | Second popup refused; Esc cancels; Enter default; focus restored | Focus left on the popup's last control after close fails | rexec | ephemeral |
| PV-T13-001 | FR-018 | T13 | integration (headless egui_kittest) | Stable-ID rule §11.4 | `rexec -- cargo nextest run -p fuzzy-dedupe-app --locked -E 'test(findings)'` | Focus and queued IDs unchanged across filter, sort, expand and collapse | A row-index-keyed focus moves after sorting and fails | rexec | ephemeral |
| PV-T13-002 | REQ-014 | T13 | integration (headless egui_kittest) | C-01 counterpart and §11.4 label forms | Same command | Every member label names its counterpart; indicators independent | A label naming the reference when the counterpart is another member, or a stale label after a reference change, fails | rexec | ephemeral |
| PV-T13-003 | REQ-037 | T13 | integration (headless egui_kittest) | §11.4 tables and states | Same command | Columns, menus, counts, collapse and states as specified | A Mixed group counted once instead of under each type fails | rexec | ephemeral |
| PV-T14-001 | REQ-015 | T14 | integration (headless widget) | Generator edit script and source numbers | `rexec -- cargo nextest run -p fuzzy-dedupe-app --locked -E 'test(diff_view)'` | Rows share top edges; numbers and counts equal the script; navigation aligned | Applying identical pixel offsets to both panes misaligns after an insertion and fails | rexec | ephemeral |
| PV-T14-002 | IR-006 | T14 | integration (headless widget) | Byte equality with the generated buffer | Same command | Whole-file and span copies equal; no cross-pane extension; Normalized labeled | A copy that includes spacer rows or converts CRLF fails byte equality | rexec | ephemeral |
| PV-T15-001 | IR-001 | T15 | integration (headless egui_kittest) | The seven IR-001 properties captured before entry | `rexec -- cargo nextest run -p fuzzy-dedupe-app --locked -E 'test(compare)'` | All seven equal after return | Scroll position reset to top on return fails | rexec | ephemeral |
| PV-T15-002 | REQ-013 | T15 | integration (headless egui_kittest) | Queued IDs and target paths before the sequence | Same command | Equal after the sequence | A queue rebuilt from visible rows drops filtered-out entries and fails | rexec | ephemeral |
| PV-T15-003 | REQ-016 | T15 | integration (headless egui_kittest) | §11.5 read-only rule; IR-006 | Same command | No text change; no edit, merge or AI control | A focused pane accepting typed characters fails | rexec | ephemeral |
| PV-T15-004 | REQ-036 | T15 | integration (headless egui_kittest) | §11.5 control list | Same command | Controls present; no-pair state explained; stale marked; refusal shown | Queue B on a counterpart-queued pair succeeding fails | rexec | ephemeral |
| PV-T16-001 | REQ-017 | T16 | integration (headless egui_kittest) | IR-003 | `rexec -- cargo nextest run -p fuzzy-dedupe-app --locked -E 'test(form_popups)'` | Cancel commits nothing; save affects only the next scan | Saving that relabels the current scan's stored rules fails | rexec | ephemeral |
| PV-T16-002 | FR-026 | T16 | integration (headless egui_kittest) | FR-026 acceptance | Same command | Count stated; decisions cleared; folder protection kept | A per-file Protect surviving the new scan fails | rexec | ephemeral |
| PV-T17-001 | FR-023 | T17 | integration (headless egui_kittest) | §11.4 review policy; `44:4079` | `rexec -- cargo nextest run -p fuzzy-dedupe-app --locked -E 'test(review_popups)'` | Every entry point routes to review; no queue without acknowledgment | Space on a fuzzy row queuing directly fails | rexec | ephemeral |
| PV-T17-002 | REQ-033 | T17 | integration (headless egui_kittest) | §11.4 bulk rules | Same command | Count and size equal the service result; exclusions hold | Including a containment match fails | rexec | ephemeral |
| PV-T18-001 | REQ-024 | T18 | integration (headless egui_kittest) | §11.6 Cleanup and §10.4 | `rexec -- cargo nextest run -p fuzzy-dedupe-app --locked -E 'test(cleanup_dialog)'` | Each state's text and behavior; one submission | Enter confirming the move fails | rexec | ephemeral |
| PV-T18-002 | REQ-040 | T18 | integration (headless egui_kittest) | UI check 10; §11.6 Cleanup | `rexec -- cargo nextest run -p fuzzy-dedupe-app --locked -E 'test(cleanup_dialog)'` | Targets, kept copies and destination listed; blocked shown before confirmation; one submission | A blocked item counted in "Move N files to Trash" fails | rexec | ephemeral |
| PV-T18-003 | REQ-041 | T18 | integration (headless egui_kittest) | UI check 11; §10.4 | Same command | Every outcome visible in the dialog and journal; revised plan needs confirmation; retry skips moved items | A retry that re-executes a moved item fails | rexec | ephemeral |
| PV-T19-001 | FR-028 | T19 | integration (headless egui_kittest) | FR-028 acceptance; `LICENSE` and `THIRD-PARTY-NOTICES.md` bytes | `rexec -- cargo nextest run -p fuzzy-dedupe-app --locked -E 'test(info_popups)'` | Statements present; no or-later grant in About's own summary, notices or displayed package license; texts equal the files in a Back-able body and copy exactly; URL opened; one popup | An or-later grant injected into About's own summary or the displayed package license fails, while the unchanged `LICENSE`, which contains "or any later version" at line 572, displays and copies and passes because the phrase check excludes the embedded license and third-party texts; a license view differing from `LICENSE` by one byte fails; a second popup opening for the license text fails | rexec | ephemeral |
| PV-T20-001 | DR-009 | T20 | integration round trip | C-08 field list | `rexec -- cargo nextest run -p fuzzy-dedupe-app --locked -E 'test(preferences)'` | All fields restored; excluded data absent; error rules hold | A queue entry restored after restart fails | rexec | ephemeral |
| PV-T21-001 | REQ-007 | T21 | integration (workstation glyph check and capture) | Glyph coverage for each test character | Run the workstation-only glyph test and capture the `unicode` scenario rows locally | Japanese and Arabic covered by fontconfig fonts, emoji by Noto Emoji; no missing-glyph boxes; EV-001 carries its C-14 binding | The same check with the adapter disabled reports missing Japanese and Arabic glyphs | reference workstation | EV-001 |
| PV-T22-001 | REQ-004 | T22 | integration (headless layout) | C-10 values from §11.3, §11.4, §11.7 | `rexec -- cargo nextest run -p fuzzy-dedupe-app --locked -E 'test(layout)'` | All values within ±1 unit | A 2-unit row-height change fails | rexec, CI | ephemeral |
| PV-T22-002 | REQ-005 | T22 | integration (egui_kittest snapshot) | Baselines generated on the lavapipe runner | `rust.yml` UI-test job on the PR | 32 snapshots pass; executed count is 32 | A commit changing one token fails the job; a run executing zero tests fails | GitHub-hosted Ubuntu, Mesa lavapipe | EV-006 |
| PV-T22-003 | REQ-010 | T22 | headless integration | FR-019 and UI check 01 | `rexec -- cargo nextest run -p fuzzy-dedupe-app --locked -E 'test(integration)'` | State unchanged per popup per workspace; no second popup | A popup that resets the table scroll fails | rexec | ephemeral |
| PV-T22-004 | REQ-011 | T22 | integration (headless layout) | NFR-007 80% rule | `rexec -- cargo nextest run -p fuzzy-dedupe-app --locked -E 'test(layout)'` | Both ratios ≥ 0.80 | An added summary card pushing the table below 80% fails | rexec | ephemeral |
| PV-T22-005 | REQ-019 | T22 | headless integration | UI check 14 list | `rexec -- cargo nextest run -p fuzzy-dedupe-app --locked -E 'test(integration)'` | Each state shows an actionable message; workspace count stays two | A blank pane for an unreadable preview fails | rexec | ephemeral |
| PV-T22-006 | NFR-009 | T22 | integration | Fixture canary lines | Same command | No canary line in any log file | Logging the diff payload at debug fails | rexec | ephemeral |
| PV-T22-007 | REQ-039 | T22 | integration (headless egui_kittest) | UI check 09; C-05 invariants | `rexec -- cargo nextest run -p fuzzy-dedupe-app --locked -E 'test(integration)'` | All five entry points give one outcome per row kind; injected change invalidates approval | A Space-key path that bypasses review for a fuzzy row fails | rexec | ephemeral |
| PV-T22-008 | REQ-043 | T22 | integration (headless egui_kittest) | PV-T6-003 expected values | `rexec -- cargo nextest run -p fuzzy-dedupe-app --locked -E 'test(integration)'` | The three surfaces agree in both views; clipboard text equals the expected copy text | A surface that derives its count from diff rows disagrees and fails | rexec | ephemeral |
| PV-T23-001 | REQ-034 | T23 | unit (headless harness) | C-10 sample format | `rexec -- cargo nextest run -p fuzzy-dedupe-app --locked -E 'test(measure)'` | Format valid; painted frames only; delay visible; inert when off | A recorder that samples on idle frames inflates the count and fails | rexec | ephemeral |
| PV-T24-001 | REQ-002 | T24 | manual review | Reference PNGs | Capture each C-11 frame state and compare side by side | 19 entries, each passed or owner-accepted | A frame captured from a different scenario than `design`, or a record without its C-14 binding, is rejected | reference workstation | EV-002 |
| PV-T24-002 | REQ-012 | T24 | manual review | §11.7 styles; reference PNGs | Compare sizing and typography in both themes; repeat at 200% zoom | Match in both themes; no clipping at 200% | Clipped text at 200% zoom fails | reference workstation | EV-002 |
| PV-T25-001 | REQ-001 | T25 | benchmark | §11.9 thresholds | Release build in measurement mode, C-10 runs, percentiles from raw samples | All four targets met | A debug build, a 1280 × 800 window, averages instead of percentiles, or a record without its C-14 binding are rejected | reference workstation | EV-003 |
| PV-T26-001 | REQ-006 | T26 | manual | C-13 checklist | Owner-performed pass on both desktops | Every item passes on both | Recording one desktop only, or a record without its C-14 binding, is rejected | GNOME and KDE Plasma Wayland | EV-004 |
| PV-T26-002 | REQ-018 | T26 | manual | NFR-005 and UI check 13 | Keyboard-only pass within C-13 | Every action reachable; focus restored; paths accessible | An action reachable only by pointer fails | GNOME and KDE Plasma Wayland | EV-004 |
| PV-T27-001 | REQ-009 | T27 | inspection and approval | §19 exit list; Appendix B | Review the evidence file against the exit list; rerun final gates; obtain sign-off | Eight items passed with links; sign-off present | An exit item without a proof link blocks sign-off | GitHub, local | EV-005 |
| PV-T27-002 | REQ-046 | T27 | inspection (binding comparison) | C-14 bindings and `git rev-parse` at the signed commit | Compare each cited record's binding with `git rev-parse "$SIGNED_OID":crates` and the three file object IDs; for each mismatch check EV-007 for a rerun bound to the signed tree or an equivalence review naming the `git diff` range | Every cited record equal, renewed or reviewed as non-functional | A T25 record from before T21's font integration, or a T24 record from before a renderer correction, cited without renewal is rejected; an equivalence review covering a `src/` code change is rejected; a documentation-only diff with a recorded review is accepted | local, reference workstation | EV-007 |
| PV-T28-001 | REQ-045 | T28 | inspection and CI observation | §19 MS-1 completion; Appendix B.1 and B.3; pr-standard | `gh run list --commit "$SQUASH_OID"` for T27's and T28's squash commits; `gh-workflow admission --branch main`; review EV-008 and the spec diff; `project-standards spec validate --strict` | All six workflows succeed on both squash commits; no unadmitted commit; EV-008 holds B.3 fields and the checkpoint OIDs, PR numbers and squash OIDs of T1–T27 and completed corrections; the post-landing handoff record holds T28's checkpoint OID, PR number, squash OID and runs; Status cells and `DEV-` rows complete | Declaring MS-1 complete while a `main` run on T27's squash commit is red, an EV-008 missing a T1–T27 checkpoint OID, an EV-008 or §13 harvest that names a T28 identifier (which cannot exist when it is written), or a missing post-landing record is rejected; rewriting a lost post-landing record from `gh` queries is accepted and names no new commit | GitHub, local | EV-008 |

## Appendix C. Durable Evidence

| Evidence ID | Producing Task | Path | Contents / Provenance | Privacy Exclusions | Retention Reason |
| --- | --- | --- | --- | --- | --- |
| EV-001 | T21 | `docs/evidence/ms-1/font-fallback.md` | C-14 binding, glyph-coverage result, selected fallback families and captures in `docs/evidence/ms-1/font-fallback/`, with commit and environment | No user names, host names or home paths | Exit item 7 needs workstation fonts and cannot be reproduced in CI |
| EV-002 | T24 | `docs/evidence/ms-1/design-review.md` | C-14 binding, per-frame reference and capture pairs in `docs/evidence/ms-1/design-review/`, findings and results | No personal data in captures | Exit item 2 requires a recorded review |
| EV-003 | T25 | `docs/evidence/ms-1/performance.md` | C-14 binding, hardware, software, build provenance, raw CSV samples in `docs/evidence/ms-1/performance/` and computed percentiles | No host names, serial numbers or home paths | Exit item 1 is workstation-only and non-repeatable in CI |
| EV-004 | T26 | `docs/evidence/ms-1/desktop-pass.md` | C-14 binding, checklist results per desktop and scale, environment versions, selected captures in `docs/evidence/ms-1/desktop-pass/` | No personal data or other windows in captures | Exit item 6 is manual and owner-performed |
| EV-006 | T22 | `docs/evidence/ms-1/ci-snapshots.md` | C-14 binding, URL of the passing snapshot run, runner image, Mesa version, snapshot count and the failing negative-control run | None needed | Exit item 3 requires the CI result to be citable after logs expire |
| EV-005 | T27 | `docs/evidence/ms-1-evidence.md` | The MS-1 evidence file: every exit item, its proof, links to EV-001 to EV-004, EV-006 and EV-007, and the owner's sign-off | No personal data beyond the owner's organization name | Required by §19 and ADR 0001 Confirmation |
| EV-007 | T27 | `docs/evidence/ms-1/renewals.md` | Freshness review: per cited record its C-14 binding, the signed binding, the verdict, rerun results with raw data in `docs/evidence/ms-1/renewals/` or the equivalence review with its `git diff` range | No host names, serial numbers, home paths or other personal data | Proves the signed build is the one the cited evidence validated (D-016) |
| EV-008 | T28 | `docs/evidence/ms-1-completion-report.md` | Appendix B.3 completion report; `main` run records of T27's squash commit; the checkpoint OID, PR number and squash OID of T1–T27 and every completed correction; T28's own identifiers go to the post-landing handoff record on `main` | No personal data beyond the owner's organization name | Appendix B.3 gate; squash landing keeps checkpoints only on task branches (D-009) |

## Appendix D. Deferred Work

| Item | Reason Deferred | Follow-up / Reopen Trigger |
| --- | --- | --- |
| UI check 15 | Design-review item, not an MS-1 exit condition (§8.3) | Next design review |
| Narrowing the no-mutation guard for fileops | MS-1 has no file operations | MS-5 plan replaces C-09 scope |
