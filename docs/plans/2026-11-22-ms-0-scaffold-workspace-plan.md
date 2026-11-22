---
plan_format: 3
title: 'MS-0: Scaffold the Cargo workspace Implementation Plan'
slug: 'ms-0-scaffold-workspace'
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

# MS-0: Scaffold the Cargo workspace Implementation Plan

> **Definition, not state.** Authoring drafts live in `.project-pipeline/2026-11-22-ms-0-scaffold-workspace/authoring/`; generated execution status and evidence pointers live in `.project-pipeline/2026-11-22-ms-0-scaffold-workspace/execution/`.

## 1. Objective

Fuzzy DeDupe has a specification, an ADR and a pinned toolchain, but no buildable code, no Rust CI and no licensing record. MS-0 turns the repository into an empty but fully governed Rust workspace. When it is done, a developer or agent starting MS-1 finds the eight §8.5 crates building with the agreed dependency direction, lint policy and binary target. Every applicable §17.2 gate runs on each pull request, every file has a machine-checkable copyright and license, and the four IBM Plex font files sit in place with their checksums and notices. The workspace contains no product behavior and no third-party crate.

The dominant invariant is that the scaffold encodes the spec's rules mechanically, so later milestones inherit them instead of re-deciding them. The rules covered are dependency direction, `unsafe` policy, OS confinement, the license policy and the approved-dependency rule. Each rule is enforced by a gate or check that a deliberately wrong change fails.

## 2. Authority and Source Map

| Source | Source Role | Authority / Use | Version / Date | Affected Plan Surface |
| --- | --- | --- | --- | --- |
| `request` | normative | Owner authorization of MS-0/MS-1 planning (2026-11-22); MS-0 boundary; rexec and PR landing constraints; no MS-1 content | 2026-11-22 | §§1–13 |
| `spec:docs/specs/canonical/fuzzy-dedupe.md#19-implementation-plan` | normative | MS-0 deliverable row and the six-item MS-0 exit list | SPEC-FZDP v0.9 | §3, §6, all tasks |
| `spec:docs/specs/canonical/fuzzy-dedupe.md#85-design-constraints` | normative | Workspace layout, crate names, dependency direction, the `unsafe` lint split (workspace `deny`, crate-root `forbid` except platform), OS confinement | v0.9 | T1, T2, Appendix A |
| `spec:docs/specs/canonical/fuzzy-dedupe.md#83-design-decisions` | normative | Approved-dependency rule and table, and the definition of a direct dependency (third-party crates only, dev and build included; inter-workspace edges excluded); Foundation row (edition 2024, resolver 3, `rust-version` 1.96, committed lockfile) | v0.9 | T1, T2 |
| `spec:docs/specs/canonical/fuzzy-dedupe.md#172-test-strategy` | normative | Development-tooling gates, allowed-license list with the eight workspace crates as `GPL-3.0-only` exceptions, portability check, CI rules | v0.9 | T2, T4, T5 |
| `spec:docs/specs/canonical/fuzzy-dedupe.md#183-deployment-flow` | normative | `GPL-3.0-only`; SPDX pair on every project-owned source file (code, and build or CI configuration that can carry comments); `REUSE.toml` for files that cannot carry headers, including Markdown; unmodified `LICENSE`; `THIRD-PARTY-NOTICES.md` | v0.9 | T3, T4 |
| `spec:docs/specs/canonical/fuzzy-dedupe.md#117-centralized-visual-tokens-and-reusable-widgets` | normative | Fonts paragraph: four unmodified IBM Plex files from pinned per-family releases with checksums, OFL-1.1 text, notice (Noto Emoji and disabling egui's default fonts are MS-1) | v0.9 | T3 |
| `spec:docs/specs/canonical/fuzzy-dedupe.md#72-non-functional-requirements` | normative | NFR-008 portability and CI-enforced OS confinement | v0.9 | T2 |
| `spec:docs/specs/canonical/fuzzy-dedupe.md#34-constraints` | normative | C-012 (`GPL-3.0-only`), C-014 (Linux V1, portable later), C-015 (egui committed) | v0.9 | §3 |
| `spec:docs/specs/canonical/fuzzy-dedupe.md#appendix-b-agent-implementation-contract` | normative | No unagreed dependencies, `DEV-`/`OQ-` routing, keeping §17.3 current, the B.3 completion report, the B.4 session handoff records (D-013) | v0.9 | §3.3, §3.4, §12, §13, T6 |
| `spec:docs/specs/canonical/fuzzy-dedupe.md#173-requirement-to-test-traceability` | normative | NFR-008 traceability row, whose Status T6 updates for MS-0's part | v0.9 | T6 |
| `spec:docs/specs/canonical/fuzzy-dedupe.md#deviations-log` | normative | Where any MS-0 implementation deviation is recorded as a `DEV-` row | v0.9 | T6 |
| `adr:docs/adr/adr-0001-use-egui-for-the-v1-desktop-ui.md` | decision | egui/eframe is committed; only `fuzzy-dedupe-app` may depend on it (no egui in MS-0) | amended 2026-11-22 (amendment note maps the old crate names) | §3.2 |
| `repo:rust-toolchain.toml` | current-state evidence | Toolchain pin 1.96.0 with rustfmt, clippy, rust-analyzer | plans-PR base | T1, T2 |
| `repo:.rexec.toml` | operational evidence | rexec tool pins and digests for nextest 0.9.152, cargo-deny 0.22.0, cargo-audit 0.23.0; worker uses its own rustup | plans-PR base | T2, §7 |
| `repo:.github/workflows/admission.yml` | current-state evidence | Precedent for SHA-pinned `actions/checkout`, `ubuntu-latest`, `contents: read` | plans-PR base | T2 |
| `repo:.standards/lock.toml` | current-state evidence | Which tracked files are standards-managed (whole-file or block) or create-only scaffolds, the package release, and each copied file's source content digest, which T4 uses to classify origin (D-012) | plans-PR base | T4 |
| `external:L3DigitalNet/project-standards LICENSE at the locked release` | current-state evidence | Upstream license (Apache-2.0, observed 2026-11-22 through the GitHub API) and holder of the files the standards package copies into this repository (A-005) | release recorded in `.standards/lock.toml` | T4 |
| `repo:.agents/skills/github-workflow/references/pr-standard.md` | operational evidence | Admission classes, draft PR, `## Governing work`, `land` | package 1.10 | §10, T5 |
| `repo:docs/handoff/conventions.md` | current-state evidence | C1: `figma-*` skills are installed locally and never tracked | plans-PR base | T4 |
| `repo:CLAUDE.md` | operational evidence | Prettier and markdownlint commands over tracked Markdown/JSON/YAML | plans-PR base | T3, T4, T5 |
| `repo:LICENSE` | current-state evidence | Full GPLv3 text; SHA-256 `3972dc9744f6499f0f9b2dbf76696f2ae7ad8af9b23dde66d6af86c9dfb36986`, equal to `https://www.gnu.org/licenses/gpl-3.0.txt` fetched 2026-11-22 | plans-PR base | T4 |
| `external:IBM/plex releases @ibm/plex-sans@1.3.6 and @ibm/plex-mono@2.5.0` | decision | Font pins accepted by the owner 2026-11-22 (OQ-P03) and observed file digests (A-004) | observed 2026-11-22 | T3 |

Conflict precedence is user direction, then the spec, then ADR 0001 within its UI scope, then repository evidence. No material conflict remains. Spec v0.9 states the lint split (D-001), the workspace-crate license exceptions (D-007), and the definitions of direct dependency and project-owned source file that D-008 and REQ-005 apply. It also lists LICENSES/ as the texts of licenses in use and pins IBM Plex per family. ADR 0001's amendment note maps its older `crates/app` and `core` names to `fuzzy-dedupe-app` and `fuzzy-dedupe-engine`. The plan's remaining judgments are D-006, the nextest no-tests flag, and D-011, the boundary check's detection set. Both are tooling detail rather than spec scope. D-012 classifies licensing by each file's origin, using upstream and lock evidence rather than a judgment.

## 3. Scope, Boundaries, and Constraints

### 3.1 In Scope

- A Cargo virtual workspace with exactly the eight §8.5 crates, inherited package metadata and lints, the Appendix A dependency edges, the `fuzzy-dedupe` binary, and a committed `Cargo.lock`.
- CI that runs on pull requests and pushes to `main` for the §17.2 gates that apply to an empty scaffold: format, Clippy pedantic, build with nextest and doctests, `cargo deny`, `cargo audit`, Windows and macOS `cargo check` of the domain and platform crates, the workspace boundary check (D-011), and `reuse lint`.
- `deny.toml`, `REUSE.toml`, `LICENSES/`, `THIRD-PARTY-NOTICES.md`, and inline SPDX headers on every project-owned source file (D-008), including the three pre-existing ones.
- The four IBM Plex font files, unmodified, with recorded checksums and the OFL-1.1 text.
- Landing the result through one draft pull request and `gh-workflow land`, then proving every applicable gate on `main` and recording the result, the §17.3 NFR-008 status and any `DEV-` row in a close-out pull request (T6).

### 3.2 Out of Scope and Deferred

- Any product behavior, UI code, fixtures, egui/eframe or other third-party crates, engine logic, `docs/design/reference/`, Lucide icons, `LICENSES/ISC.txt`, and the egui_kittest/lavapipe UI-test job. These belong to MS-1 or later.
- `tests/`, `benches/` and `packaging/` from the §8.5 layout. They are created when a milestone first needs them.
- Framework comparison or Slint contingency work (C-015). This is a permanent non-goal of this plan.
- Edits to the spec beyond T6's close-out record, which is the §17.3 NFR-008 Status cell and any `DEV-` row (Appendix B.1 assigns both to the implementer). Also out of scope: edits to ADRs, standards-managed files, `.gitignore`, or `.claude/settings.json`. The `figma-*` skills are untracked by owner decision (OQ-P01) and must not be re-added. Spec defects and scope questions go to the owner as `OQ-` rows (§11.3).
- Every SPEC-FZDP requirement outside the §3.5 map's MS-0 rows. §19 assigns it to MS-1 to MS-6, and those milestones' plans own it.
- Branch protection or required status checks. These are an owner setting, not an MS-0 deliverable (Appendix D).

### 3.3 Ownership and Preserved Behavior

| Boundary | Owner / Responsibility |
| --- | --- |
| Plan owns | Workspace manifests and placeholder crate roots, `Cargo.lock`, `deny.toml`, `.github/workflows/rust.yml`, `.github/scripts/check-boundaries.py`, `.github/boundaries.toml`, `.github/workflows/reuse.yml`, `REUSE.toml`, `LICENSES/`, `THIRD-PARTY-NOTICES.md`, `crates/fuzzy-dedupe-app/assets/fonts/`, `docs/evidence/ms-0-ci-gates.md`, `docs/evidence/ms-0-completion-report.md`; at T6 only, the spec's §17.3 NFR-008 Status cell and Deviations Log |
| Depends on | GitHub-hosted `ubuntu-latest` runners with preinstalled rustup and pipx; the `IBM/plex` release assets; crates.io and the RustSec advisory database at gate time; PyPI for `reuse` |
| Does not own | `L3DigitalNet/project-standards` reusable workflows and managed files; locally installed `figma-*` skills (untracked); the spec and ADRs; handoff documents (updated only by D-013 session-end commits and at close-out, both through the agent-handoff skill) |
| Must preserve | **Unchanged set**, byte-identical to the branch base on the topic branch: `LICENSE`, `.gitignore`, every standards-managed file in `.standards/lock.toml` (including `format.yml`, `lint-markdown.yml` and `validate-specs.yml`, so drift checks stay clean), the spec, the ADRs and every handoff document outside the session-state set. **Session-state set**: `docs/handoff/state.md`, `docs/handoff/sessions/*.md`, `docs/STATUS.md` and `docs/TODO.md` change on the topic or close-out branch only through D-013 handoff-only commits. **SPDX-delta set**: `rust-toolchain.toml`, `.rexec.toml` and `.github/workflows/admission.yml` differ from the base only by T4's two SPDX comment lines, and their triggers and settings are unchanged. All four existing workflows stay green. The owner's uncommitted `.claude/settings.json` change is never staged |

### 3.4 Constraints and Authorization

- Heavy CPU work runs as `rexec -- cargo …`, or as `rexec --shell '…'` only when shell syntax is needed. Exit codes 20–25 are rexec's own failures and are never reported as a gate result. Only `--pull` paths return. Generate `Cargo.lock` in the local tree, where `cargo generate-lockfile` is a cheap metadata step, so that remote `--locked` runs verify it. Git mutations and cheap single-file checks (`git`, `sha256sum`, `grep`, Prettier or markdownlint on named files, `reuse lint`) run locally.
- No direct dependency, including a dev-dependency or build-dependency, is added to any crate. A task that finds it needs one blocks. It resumes only after an owner-approved pull request adds the §8.3 table row (spec §8.3 and Appendix B.2).
- `.github/boundaries.toml` mirrors spec §8.3 and Appendix A C-1. It is not the approval record. A change that adds a direct dependency adds its §8.3 row and its policy entry in the same pull request. A change that adds a workspace edge keeps C-1's direction rules.
- New CI uses only GitHub-owned actions (`actions/checkout`, pinned by full commit SHA as `admission.yml` does). Third-party actions are not introduced, because they would be dependencies not agreed with the owner (Appendix B.2). See D-004.
- Every new comment-capable file carries `SPDX-FileCopyrightText: 2026 L3Digital LLC` and `SPDX-License-Identifier: GPL-3.0-only`. No declaration anywhere uses `GPL-3.0-or-later` (§18.3).
- Markdown, JSON and YAML files MS-0 adds must pass the Prettier and markdownlint commands in `CLAUDE.md`.
- Execution starts from a `main` that already contains the plans PR: this promoted master, spec v0.9, the `.project-pipeline/` and `.agents/skills/figma-*/` ignore rules, and the untracked Figma skills. All task commits go on one topic branch cut from that `main` (for example `feat/ms-0-scaffold-workspace`). Pushing that branch, opening the draft PR, `land`, and T6's close-out branch and pull request are part of execution once it is authorized. The plan grants nothing beyond that.

### 3.5 Specification Coverage Map

This map gives every SPEC-FZDP requirement a milestone disposition. The `REQ-nnn` IDs in §6 are plan-local, because the plan engine requires that form. Each one's Source column names the spec ID or section it implements.

| Spec requirement or section | MS-0 disposition | Plan IDs |
| --- | --- | --- |
| §19 MS-0 row and exit items 1–6 | Owned in full | REQ-001–REQ-022 |
| §8.5 design constraints | Owned for the scaffold: the eight crates under `crates/`, the binary name, dependency direction, the lint split and OS confinement. Deferred: `tests/`, `benches/` and `packaging/` (first milestone that needs them) and every adapter (MS-1 onward) | REQ-001–REQ-004, REQ-012 |
| §8.3 approved dependencies and the Foundation row | Owned: no third-party crate at MS-0, and from MS-0 on the boundary policy rejects any direct dependency without an entry | REQ-002, REQ-005, REQ-006, REQ-012 |
| §17.2 test strategy | Owned for the gates that apply to an empty workspace. Deferred: the UI-test job with lavapipe (MS-1), and the unit, property, integration and safety suites (each milestone that adds behavior) | REQ-007–REQ-013, REQ-018 |
| §18.3 licensing and notices | Owned for every file present at MS-0. Deferred: the release notice inventory and corresponding-source packaging (MS-6) | REQ-014–REQ-019 |
| §11.7 Fonts paragraph | Split. MS-0 owns the four IBM Plex files, their checksums, the OFL-1.1 text and the notice entry. MS-1 owns Noto Emoji, egui font configuration, disabling egui's default fonts and script rendering | REQ-014, REQ-015 |
| NFR-008 portability | Split. MS-0 owns CI confinement and the Windows and macOS checks. Linux adapter suites arrive with each adapter (MS-1, MS-2, MS-5). T6 records MS-0's part in §17.3 | REQ-011, REQ-012, REQ-022 |
| C-012, C-014, C-015 | Preserved: `GPL-3.0-only` with no `-or-later`, Linux first with portable shared crates, and egui committed with no fallback work | REQ-002, REQ-011, REQ-019 |
| Appendix B.1–B.3 | Owned for MS-0: `OQ-` and `DEV-` routing, keeping §17.3 current, and the completion report | REQ-022 |
| All other requirements: every FR-, DR- and ERR- requirement, the remaining NFRs, the UI checks, and §9–§13 behavior | Deferred by §19 to MS-1–MS-6 and owned by those plans. MS-0 adds no behavior and must not preclude any of them. Later plans build on C-1's direction rules and the C-3, C-5 and C-7 extension points | — |

## 4. Current State and Target State

### 4.1 Current State

After the plans PR, the repository has no Cargo manifest, Rust source, `Cargo.lock`, `deny.toml`, `REUSE.toml`, `LICENSES/` or `THIRD-PARTY-NOTICES.md`. `rust-toolchain.toml` pins 1.96.0 (minimal profile with rustfmt, clippy, rust-analyzer). `.rexec.toml` pins the three gate binaries by release-asset digest: `cargo-nextest-0.9.152-x86_64-unknown-linux-gnu.tar.gz`, `cargo-deny-0.22.0-x86_64-unknown-linux-musl.tar.gz` and `cargo-audit-x86_64-unknown-linux-gnu-v0.23.0.tgz`. `LICENSE` is the canonical GPLv3 text. CI consists of `format.yml`, `lint-markdown.yml` and `validate-specs.yml`, which are standards-managed callers of `project-standards@v5`, plus the project-owned `admission.yml` on pushes to `main`. `.standards/lock.toml` lists the standards-managed files. The `.agents/skills/figma-*` skills are untracked and gitignored, because their upstream (`figma/mcp-server-guide`) grants no license, so `reuse lint` never sees them. `main` has no branch protection. Every prior PR landed as a squash commit carrying `Workflow-Admission: PR #N`.

### 4.2 Target State

`cargo build --workspace --locked` produces the `fuzzy-dedupe` binary from eight placeholder crates whose manifests inherit license, edition, `rust-version` and lints. `Cargo.lock` lists only those eight packages. `rust.yml` and `reuse.yml` run every applicable gate on each PR and on `main`, and each gate has been shown to reject a deliberately wrong change. The `boundaries` job enforces dependency direction, approved dependencies and OS confinement from the policy in `.github/boundaries.toml`. `reuse lint` passes over the whole tree. The fonts are present and verifiably unmodified. MS-1 can then add egui crates (with their §8.3 rows and C-7 policy entries), the UI-test job, icons and the ISC license through the extension points in Appendix A without restructuring any of this.

### 4.3 Delta Summary

| Area | Current | Target | Must Preserve |
| --- | --- | --- | --- |
| Build | No workspace | Eight crates, binary `fuzzy-dedupe`, committed lockfile | Toolchain pin 1.96.0 |
| CI | Four workflows (format, Markdown, specs, admission) | Adds `rust.yml` (seven jobs), the boundary script and policy, and `reuse.yml` (one job) | Existing workflows green; only `admission.yml` gains SPDX comment lines |
| Licensing | `LICENSE` only | SPDX headers, `REUSE.toml`, `LICENSES/`, `THIRD-PARTY-NOTICES.md` | `LICENSE` byte-identical; managed files unedited |
| Assets | None | Four IBM Plex TTF files | Upstream bytes unmodified |

## 5. Change Surface and Architecture

### 5.1 Components and Ownership

| Component / Surface | Current Responsibility | Planned Responsibility | Paths / Contracts | Owning Task |
| --- | --- | --- | --- | --- |
| Workspace and crates | None | Eight placeholder crates, inherited metadata and lints, dependency edges | `Cargo.toml`, `crates/*/`, `Cargo.lock`; Appendix A C-1, C-2 | T1 |
| Rust gates | None | Seven CI jobs, the boundary check, and dependency policy | `.github/workflows/rust.yml`, `.github/scripts/check-boundaries.py`, `.github/boundaries.toml`, `deny.toml`; Appendix A C-3, C-6, C-7 | T2 |
| Font assets | None | Four upstream TTF files, OFL text, notice entry | `crates/fuzzy-dedupe-app/assets/fonts/`, `LICENSES/OFL-1.1.txt`, `THIRD-PARTY-NOTICES.md`; Appendix A C-4 | T3 |
| Licensing record | `LICENSE` | REUSE coverage of every tracked file and its CI job | `REUSE.toml`, `LICENSES/GPL-3.0-only.txt`, `.github/workflows/reuse.yml`; Appendix A C-5 | T4 |
| Integrated proof | None | All gates green on the PR head, recorded durably | Draft PR; `docs/evidence/ms-0-ci-gates.md` (EV-001) | T5 |
| Landing and `main` proof | None | Squash landing, gates green on `main`, completion report, §17.3 status | Squash commit; `docs/evidence/ms-0-completion-report.md` (EV-002); spec §17.3 NFR-008 Status and Deviations Log | T6 |

### 5.2 Change-Surface Matrix

| Surface | Applies? | Invariant / Required Change | Proof | Task |
| --- | --- | --- | --- | --- |
| Behavior | no | No product behavior; the binary's `main` does nothing | PV-T1-001 | T1 |
| Architecture / dependency direction | yes | Appendix A C-1 edges; domain depends on no workspace crate; nothing depends on the app; the `boundaries` job keeps both true | PV-T1-004, PV-T2-006 | T1, T2 |
| Public / cross-task interface | yes | CI file and job IDs, font paths, REUSE extension rule consumed by MS-1 | PV-T2-007, PV-T3-001, PV-T4-003 | T2, T3, T4 |
| Data / state | no | No persistent data | — | — |
| Configuration | yes | Workspace lints, `deny.toml`, `REUSE.toml` | PV-T1-003, PV-T2-003, PV-T4-003 | T1, T2, T4 |
| Security / trust | yes | `unsafe` forbidden outside platform; advisories and sources gated; no new third-party actions | PV-T1-003, PV-T2-003, PV-T2-004, PV-T2-007 | T1, T2 |
| Compatibility / migration | yes | Domain and platform check for Windows and macOS; OS-conditional code, OS API paths, OS-specific crates and target-specific dependencies confined to platform | PV-T2-005, PV-T2-006 | T2 |
| Operations / deployment | yes | CI runs on PRs and `main`; squash landing; gates proven on the squash commit | PV-T5-001, PV-T6-001 | T5, T6 |
| Documentation | yes | `THIRD-PARTY-NOTICES.md`; spec §17.3 NFR-008 Status and any `DEV-` row at close-out; no `docs/usage.md` change, because no harness exists yet (§8.5) | PV-T3-002, PV-T6-002 | T3, T6 |
| Durable evidence | yes | EV-001 keeps the PR-head gate results and EV-002 the `main` results and the Appendix B.3 report after GitHub's run-log retention expires | PV-T5-001, PV-T6-001, PV-T6-002 | T5, T6 |

### 5.3 Binding Decisions

| ID | Decision | Rationale | Source | Affected Task(s) |
| --- | --- | --- | --- | --- |
| D-001 | Workspace `[workspace.lints.rust]` sets `unsafe_code = "deny"`. Every crate declares only `[lints] workspace = true`. Each crate root except `fuzzy-dedupe-platform` adds `#![forbid(unsafe_code)]`. `clippy::pedantic` is `warn` (priority below individual lints), and any selective allow is listed in the workspace manifest with a reason comment. | Cargo rejects any lint override beside `workspace = true` ("cannot override `workspace.lints` in `lints`", verified with cargo 1.96.0 on 2026-11-22). A workspace-level `forbid` cannot be relaxed for platform. Rejected: workspace `forbid` (platform could not carry a scoped allow) and per-crate lint tables (breaks "every crate inherits"). Reopen if Cargo adds per-member lint overrides. | spec §8.5 (v0.9 states this split) | T1 |
| D-002 | The dependency edges are exactly Appendix A C-1. Edges to `fuzzy-dedupe-platform` from scanner and fileops are added by the milestone whose adapter they consume (MS-2 file identity, MS-5 Trash). | §8.2 draws app→engine→{scanner, analyzers, fileops}→storage and engine→storage. Domain holds the shared types. MS-1's four adapters are UI-initiated (app→platform). Rejected: pre-declaring every plausible platform edge, because an edge with no consumed contract proves nothing. | spec §8.2, §8.5 | T1 |
| D-003 | CI is two new workflow files with stable job IDs (Appendix A C-3). Triggers are `pull_request`, `push` to `main` and `workflow_dispatch`, with `permissions: contents: read`, on `ubuntu-latest`. | Matches the existing per-concern workflow naming and the public-repository runner rule. MS-1 adds its UI job to `rust.yml`. | request; repo `.github/workflows/` | T2, T4 |
| D-004 | The toolchain comes from `rust-toolchain.toml` through the runner's preinstalled rustup, not a toolchain action. nextest, cargo-deny and cargo-audit are installed from the same release assets, with the same SHA-256 digests, that `.rexec.toml` pins, and the install fails on a digest mismatch. `reuse` is version 6.3.0 from PyPI. | One version authority for workstation, worker and CI. Rejected: `dtolnay/rust-toolchain`, `taiki-e/install-action` and `fsfe/reuse-action`, which are third-party dependencies not agreed with the owner. Reopen if the owner approves an action. | `.rexec.toml`; spec §17.2, App. B.2 | T2, T4 |
| D-005 | The portability targets are `x86_64-pc-windows-msvc` and `aarch64-apple-darwin`. CI adds them with `rustup target add`, and they are not listed in `rust-toolchain.toml`. | §17.2 asks for "a Windows and a macOS target". Rejected: listing the targets in `rust-toolchain.toml`, which would add two standard-library downloads to every workstation and worker install. | spec §17.2 | T2 |
| D-006 | nextest runs with `--no-tests=warn` until the workspace has its first test. | nextest 0.9.152 defaults to failing with exit 4 when no tests exist (verified). Rejected: placeholder tests, which assert nothing, and `--no-tests=pass`, which is silent. The first milestone to add a test removes the flag (Appendix D). | spec §17.2 | T2 |
| D-007 | `deny.toml` allows exactly the ten §17.2 licenses for third-party crates and adds a `GPL-3.0-only` exception naming each of the eight workspace crates. Sources are limited to crates.io. OFL-1.1 is not in the cargo-deny allow list, because MS-0 has no font crate and the font assets are governed through `REUSE.toml`. | Spec v0.9 §17.2 names the eight crates as exceptions; cargo-deny checks workspace crates too. Rejected: adding `GPL-3.0-only` globally, which would widen third-party policy, and `publish = false` with private-crate ignore, which takes a publishing decision the spec does not make. | spec §17.2 | T2 |
| D-008 | Inline SPDX pairs go on every project-owned source file, which spec v0.9 §18.3 defines as code and build or CI configuration that can carry comments. That means every file MS-0 creates of that kind (`*.rs`, every `Cargo.toml`, `deny.toml`, `REUSE.toml`, `.github/scripts/check-boundaries.py`, `.github/boundaries.toml`, both new workflows) and the three pre-existing ones (`rust-toolchain.toml`, `.rexec.toml`, `.github/workflows/admission.yml`), which T4 edits. Everything else is covered by a path-scoped `REUSE.toml` annotation whose holder and license follow the file's origin (D-012): Markdown, JSON, binaries, fonts, `Cargo.lock`, `.gitignore` and `.editorconfig` (VCS and editor settings, not build or CI), `.standards/**` (control-plane input and output; editing it changes `lock.toml` digests), and standards-managed files. No annotation glob may match `*.rs`, `Cargo.toml`, `deny.toml`, `.github/scripts/**`, `.github/boundaries.toml` or any header-bearing workflow. | This follows the v0.9 definition, and the no-catch-all rule keeps a missing header a `reuse lint` failure. Rejected: headers on managed files (reconcile would flag drift) and a `**` fallback (would mask missing headers). The `.gitignore` and `.standards/config.toml` classification is a plan reading; reopen if the owner counts them as build configuration. | spec §18.3 (v0.9), §19 item 4 | T1, T2, T4 |
| D-009 | Fonts are the upstream `fonts/complete/ttf/` files under their upstream names in `crates/fuzzy-dedupe-app/assets/fonts/`. `LICENSES/OFL-1.1.txt` is the release's `LICENSE.txt`, verbatim (it keeps IBM's copyright and Reserved Font Name line). | §8.5 places assets in the app crate. The four files named in §11.7 are static weights. Both families ship a byte-identical `LICENSE.txt` (SHA-256 `7e6b2818…07da`). Rejected: variable fonts, because the spec names four weight files. | spec §8.5, §11.7 | T3 |
| D-010 | Landing is one topic branch with one checkpoint commit per task, one Standalone draft PR, and `gh-workflow land --pr N --method squash`. | A 2026-11-22 probe in a scratch clone showed that a merge-commit landing leaves the introduced commits unadmitted (`GHW-ADMISSION-MISSING`), which would turn `admission.yml` red on `main`. Rebase has the same defect. All seven prior PRs squash-landed. The `Plan-*` checkpoints therefore live only on the topic branch. The branch must not be deleted, locally or on `origin`, until close-out is done, because `recover` finds checkpoints with `git log --all` (the repository has `delete_branch_on_merge: false`). T6 lands the PR, proves the gates on the squash commit, and records the result on a close-out branch that lands through its own Standalone PR. That PR also carries the §13 harvest with the checkpoint OIDs. | pr-standard.md; repo history | T5, T6, §10 |
| D-011 | The `boundaries` job runs `python3 .github/scripts/check-boundaries.py`. The script uses only the Python 3.11+ standard library (`tomllib` reads the policy), so it adds no dependency. It reads `cargo metadata --no-deps --format-version 1`, which needs no network or lockfile change, plus the tracked `*.rs` files, and checks them against `.github/boundaries.toml` (C-7). It fails on each of the following: (1) a workspace edge of any dependency kind that the policy's C-1 table does not list for that crate; (2) a direct third-party dependency of any kind (normal, dev, build or target-specific), matched by package name so that a `package =` alias is caught, which the policy's third-party table does not list for the declaring crate; (3) a GUI crate from the policy list (at least `egui`, `eframe`, `egui_*` and `winit`) declared outside `fuzzy-dedupe-app`; (4) an OS-specific crate from the policy list (at least `libc`, `nix`, `rustix`, `windows`, `windows-sys`, `winapi`, `core-foundation` and `objc2`), or any target-specific dependency, declared outside `fuzzy-dedupe-platform`; (5) in a tracked `*.rs` file outside platform, after the script strips comments, string literals (raw strings included) and char literals (not lifetimes), flattens every `use` declaration, across lines, into full paths (braces at any depth, `self`, `as` aliases and globs), collapses whitespace inside the remaining non-`use` paths, and compares raw identifiers without their `r#` prefix, any of the following, each reported as R5: (5a) a `cfg(`, `cfg!(` or `cfg_attr(` predicate naming `target_os`, `target_family`, `target_vendor`, `target_env`, `unix` or `windows`; (5b) any flattened `use` path, or any `::`-path elsewhere (expression, type or attribute), with an `os` segment whose next segment is not `raw`, whatever precedes the segment. That includes a path ending at `os`, its `as` and `{self as …}` renames and a glob, so `use std::os;`, `use std::os as host;`, `use std::os::{self as host};`, `use std::os::*;`, `use s::os as host;`, `use os as host;` after `use std::*;`, and `os::unix::…` in an expression all fail. Only `os::raw` and its children are portable; (5c) an alias of the `std` or `core` root, with or without a leading `::`: `use std as …`, `use core as …`, `use ::std as …`, `use std::{self as …}` (which flattens to the same form), `extern crate std as …` or `extern crate core as …`; (5d) a `use` tree the script cannot parse, which fails as an unsupported form; (5e) any `include!` invocation, bare or path-qualified (`std::include!`, and likewise `core::include!`), and any `use` whose flattened path ends at `include`, renamed or not, so `use std::include as inc;` cannot hide a later `inc!`. The project has no need to include Rust source across crates, and `include_str!` and `include_bytes!`, which include data rather than code, stay allowed. Because 5b keys on the `os` segment itself and 5c removes the only rename above it, rule 5 depends on neither a list of OS names nor alias resolution. It catches `std::os::unix::…`, `std::{os::unix::…}`, `std::os::{unix::…}` and `std::{os::{unix::…}}` on one line or several, and the chained `use std as s;` / `use s::os as host;` / `use host::unix::…`, while `std::os::raw` stays portable and a mention inside a comment or string is ignored. Known false positive: a project item named `os` (a module, function, constant or crate) used in a path outside platform fails under 5b and must be renamed; field access such as `self.os` is not a path segment and is not matched. The rules carry the IDs R1 to R5; every failure line begins with its rule ID and names the crate and file. The script accepts `--policy PATH`, defaulting to `.github/boundaries.toml`, so placement canaries can run against a temporary policy fixture. The local proof and CI run the same script. | NFR-008 requires CI-enforced confinement, and §8.3 and §8.5 make dependency direction and approval rules that the scaffold must enforce, not merely inspect once. Rejected: grep over `Cargo.toml` text, which misses aliases and dotted tables; a ninth `xtask` crate, which breaks the eight-crate rule; cargo-deny `[bans]` wrappers, which also govern transitive third-party edges and would need every upstream wrapper listed; and a third-party action. Known residue: the source scan is lexical, so two cases escape it. The first is a macro that assembles an OS path from tokens; the portability job covers that for domain and platform, and review covers it in the other six crates. The second is an OS item that `fuzzy-dedupe-platform` exposes through its own public API, by `pub use` or in a public signature, and that a dependent crate such as app then uses; review of platform's public API covers that, because the platform adapter contracts expose portable types, and the scan alone does not. Rejected for rule 5: matching the contiguous `os::unix` text, which ordinary nested `use` trees evade (round-1b PA-001); and matching an `os` segment only when an OS name or glob follows it, or a `use` ending at `std::os` or `core::os`, which a chained root alias evades (`use std as s; use s::os as host;`, round-3 PA-001). The segment rule and the root-alias ban fail closed without name resolution, which the script does not have. Reopen if a crate outside platform needs an item named `os` or a `std` root alias. | spec §8.3, §8.5, §17.2, NFR-008 | T2, T4 |
| D-012 | `REUSE.toml` annotations follow each file's origin, not its standards update policy. (a) Original project files are `2026 L3Digital LLC`, `GPL-3.0-only`. (b) Files copied from `L3DigitalNet/project-standards` keep the upstream holder and license (A-005). That covers whole-file managed files, and create-only scaffolds whose SHA-256 still equals the source content digest in `.standards/lock.toml`. (c) Block-managed files, and create-only scaffolds the project has since edited, carry both holders with `GPL-3.0-only AND Apache-2.0`. (d) Third-party assets keep their upstream holder and license (IBM Plex: OFL-1.1). T4.1 builds the origin ledger by comparing every create-only file's digest with the lock, and confirms the upstream license and holder from the `LICENSE` file at the locked release. | "Create-only" says who owns later updates, not who wrote the file, so it proves neither authorship nor permission to replace the upstream notice (§18.3 requires applicable notices to be kept). Rejected: classing every create-only scaffold as original project material. Reopen if the owner states that L3DigitalNet and L3Digital LLC are one holder relicensing its own files, which would reduce (b) and (c) to (a). | spec §18.3; `.standards/lock.toml` | T4 |
| D-013 | Session-end handoff (spec Appendix B.4). A session that ends with MS-0 work started and T6 not closed ends with one handoff-only commit, made through the repository's agent-handoff skill, on the topic branch (or on the close-out branch once T6 has cut it). The commit carries no `Plan-*` trailers, touches only the session-state set (`docs/handoff/state.md`, `docs/handoff/sessions/*.md`, `docs/STATUS.md` and `docs/TODO.md`) and keeps their managed blocks, and records the current milestone (MS-0), the REQ IDs done and in progress, unresolved `OQ-` and `DEV-` items, and the retained topic branch (and the close-out branch once it exists). Before the session ends it passes `project-standards agent-handoff validate --repo . --since "$SESSION_START_OID"` and `project-standards agent-handoff drift-check --repo .`. `recover` ignores it, because it has no `Plan-Id`. A D-013 commit after T5's checkpoint is expected and does not invalidate it: T6.1 accepts a PR head that descends from the latest valid T5 checkpoint (or its last correction) through D-013 commits only, and §12's checks run on that actual head. The final milestone harvest stays in the close-out PR, and the close-out PR's own number, squash OID and `main` run results go to the post-landing handoff record (D-013): one handoff-only commit on `main`, made through the agent-handoff skill with the `Workflow-Admission: handoff` trailer, that touches only a `docs/handoff/sessions/` row, `docs/handoff/state.md` and `docs/STATUS.md`; that record is the last step, and repeating or recovering it creates no further self-reference. Rejected: holding every handoff document byte-identical until close-out, which makes a multi-session execution break either this plan or Appendix B.4 (round-1b PA-002); and recording session state in checkpoint commits, which would mix task proof with session notes. | spec Appendix B.4; coordinator decision | spec Appendix B, `request` | all tasks, §12, §13 |

## 6. Requirements and Acceptance

`REQ-nnn` IDs are plan-local, and the Source column names the spec ID or section each implements (§3.5). Exit item 3 is split. REQ-020 is the proof on the PR head (T5). REQ-021, "passes on `main`", is proven by T6 on the squash commit, because it can only be observed after the squash lands. MS-0 is not complete until REQ-021 holds.

| ID | Requirement | Source | Priority | Owner Task | Task(s) | Proof(s) |
| --- | --- | --- | --- | --- | --- | --- |
| REQ-001 | `cargo build --workspace --locked` succeeds with exactly the eight §8.5 crates, and `fuzzy-dedupe-app` builds the binary `fuzzy-dedupe` | §19 item 1 | Must | T1 | T1 | PV-T1-001 |
| REQ-002 | The workspace manifest sets license `GPL-3.0-only`, edition 2024, resolver 3 and `rust-version` 1.96, and every crate inherits them and `[lints] workspace = true` | §19 item 2 | Must | T1 | T1 | PV-T1-002 |
| REQ-003 | `unsafe_code` is effectively `forbid` in seven crates and `deny` in `fuzzy-dedupe-platform`, and `clippy::pedantic` warns workspace-wide | §8.5 | Must | T1 | T1 | PV-T1-003 |
| REQ-004 | Workspace dependency edges equal Appendix A C-1: domain depends on no workspace crate, and no crate depends on `fuzzy-dedupe-app` | §8.5 | Must | T1 | T1 | PV-T1-004 |
| REQ-005 | The workspace has no direct or transitive third-party crate, so every direct dependency (third-party, per the §8.3 definition; inter-workspace edges excluded) trivially has a §8.3 row | §19 item 6, §8.3 | Must | T1 | T1 | PV-T1-005 |
| REQ-006 | `Cargo.lock` is committed and `rust-toolchain.toml` still pins 1.96.0 | §19 deliverable, §8.3 | Must | T1 | T1 | PV-T1-006 |
| REQ-007 | CI runs `cargo fmt --all --check` and `cargo clippy --workspace --all-targets --locked -- -D warnings` | §17.2 | Must | T2 | T2 | PV-T2-001 |
| REQ-008 | CI runs `cargo build --workspace --locked`, `cargo nextest run` and `cargo test --doc` | §17.2, §19 item 1 | Must | T2 | T2 | PV-T2-002 |
| REQ-009 | CI runs `cargo deny check` (licenses, advisories, bans, sources) with the §17.2 allowed-license list | §17.2 | Must | T2 | T2 | PV-T2-003 |
| REQ-010 | CI runs `cargo audit` against RustSec | §17.2 | Must | T2 | T2 | PV-T2-004 |
| REQ-011 | CI runs `cargo check` of `fuzzy-dedupe-domain` and `fuzzy-dedupe-platform` for one Windows and one macOS target | §17.2, NFR-008 | Must | T2 | T2 | PV-T2-005 |
| REQ-012 | The `boundaries` CI job (D-011) fails on a workspace edge outside C-1, a direct third-party dependency with no policy entry, a GUI crate outside the app, an OS-specific crate or target-specific dependency outside platform, and, in source outside platform, OS-conditional compilation, a path through an `os` module other than `os::raw` and its children (in any nested, aliased or expression form), an alias of the `std` or `core` root, or a `use` tree it cannot parse, or an `include!` of Rust source, each reported under its rule ID R1 to R5 | §8.3, §8.5, §17.2, NFR-008, §19 item 6 | Must | T2 | T2 | PV-T2-006 |
| REQ-013 | CI uses the pinned toolchain, `--locked`, tool versions and digests equal to `.rexec.toml`, and no third-party action | §17.2; request | Should | T2 | T2 | PV-T2-007 |
| REQ-014 | The four IBM Plex files (Sans Regular, Medium, SemiBold; Mono Regular) are present unmodified from pinned releases, with recorded checksums | §19 item 5, §11.7 | Must | T3 | T3 | PV-T3-001 |
| REQ-015 | The OFL-1.1 text ships beside the fonts, and `THIRD-PARTY-NOTICES.md` has an IBM Plex entry | §19 item 5, §18.3 | Must | T3 | T3 | PV-T3-002 |
| REQ-016 | Every project-owned source file (D-008) present at MS-0, created or pre-existing, carries the two SPDX lines | §19 item 4, §18.3 | Must | T4 | T4 | PV-T4-001 |
| REQ-017 | `LICENSE` holds the unmodified GPLv3 text, and `LICENSES/GPL-3.0-only.txt` exists | §19 item 4, §18.3 | Must | T4 | T4 | PV-T4-002 |
| REQ-018 | `REUSE.toml` declares each header-less file's copyright and license by origin (D-012), `reuse lint` passes, and CI runs it | §19 item 4, §17.2, §18.3 | Must | T4 | T4 | PV-T4-003 |
| REQ-019 | No SPDX line, `REUSE.toml` annotation or Cargo `license` field declares an `-or-later` expression | §18.3, C-012 | Must | T4 | T4 | PV-T4-004 |
| REQ-020 | Every `rust.yml` and `reuse.yml` job, and every job of the three PR-triggered existing workflows (`format`, `lint-markdown`, `validate-specs`), runs on the draft PR and passes on its head. `admission` runs only on pushes to `main` and is REQ-021's | §19 item 3 (pull-request part) | Must | T5 | T5 | PV-T5-001 |
| REQ-021 | On the squash commit on `main`, every `rust.yml`, `reuse.yml`, `format`, `lint-markdown`, `validate-specs` and `admission` run concludes `success`, and `gh-workflow admission --branch main` reports no unadmitted commit | §19 item 3 (`main` part) | Must | T6 | T6 | PV-T6-001 |
| REQ-022 | EV-002 holds the Appendix B.3 completion report, the spec's §17.3 NFR-008 Status records MS-0's part, and every implementation deviation has a `DEV-` row | Appendix B.1, B.3; §17.3 | Must | T6 | T6 | PV-T6-002 |

## 7. Verification and Evidence Strategy

- **Authoritative commands:** the §17.2 commands in Appendix B, run locally through rexec before commit and on GitHub Actions at T5. Use `cargo metadata --format-version 1 --locked` (with `--no-deps` where noted) for manifest inspection, `reuse lint` (6.3.0) for licensing, `sha256sum` for byte identity, and the `CLAUDE.md` Prettier and markdownlint commands for Markdown, JSON and YAML.
- **Oracles:** the spec's exit list and §8.5 rules, Appendix A C-1 for edges, upstream release digests and the gnu.org GPLv3 digest for byte identity, and the gate tools' own verdicts on deliberately wrong inputs.
- **Negative controls:** consequential proofs (PV-T1-003, PV-T2-003, PV-T2-005, PV-T2-006, PV-T3-001, PV-T4-001, PV-T4-003) each run a temporary canary that must fail the gate. Canaries are never committed. After each proof, `git status` shows only the task's declared files.
- **Test layers:** static inspection, compile-time policy, dependency policy, licensing lint and CI integration. There are no unit tests, because there is no behavior (D-006).
- **External environments:** the rexec worker for cargo work, GitHub-hosted `ubuntu-latest` for T5 and T6, and network access to crates.io, RustSec, PyPI and GitHub releases.
- **Evidence:** task-level proof is ephemeral, and verbose output goes to `.project-pipeline/2026-11-22-ms-0-scaffold-workspace/execution/logs/`. The durable records are EV-001 (PR-head gate results, T5) and EV-002 (`main` results and the Appendix B.3 report, T6), because Actions run logs expire. Font digests are a deliverable in `THIRD-PARTY-NOTICES.md`, not evidence.
- **Late failure:** a T5 or Final Verification failure blocks the verifying step. The executor appends a correction task with `corrects:` and `discovered_from:`, completes it on the same branch, and reruns from the failed gate. A red `main` run after landing follows T6's recovery rule and §10.3.

## 8. Execution Summary

| Task | Title | Disposition | Work Type | Phase | Depends On | Requirement(s) | Primary Proof | Parallel / Conflict |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| T1 | Create the eight-crate workspace | active | configuration | P1 | None | REQ-001–REQ-006 | PV-T1-003 | no / none |
| T2 | Add the Rust CI gates, boundary check and dependency policy | active | configuration | P2 | T1 | REQ-007–REQ-013 | PV-T2-006 | no / none |
| T3 | Add the IBM Plex font assets and notices | active | configuration | P2 | T1 | REQ-014–REQ-015 | PV-T3-001 | no / none |
| T4 | Establish REUSE licensing coverage and its CI job | active | configuration | P3 | T1, T2, T3 | REQ-016–REQ-019 | PV-T4-003 | no / none |
| T5 | Prove every gate on the draft pull request | active | verification | P4 | T2, T4 | REQ-020 | PV-T5-001 | no / none |
| T6 | Land MS-0 and prove it on `main` | active | operational | P5 | T5 | REQ-021–REQ-022 | PV-T6-001 | no / none |

T2 and T3 have disjoint file claims, but one executor commits them serially on a single topic branch.

## 9. Implementation Tasks

### Phase P1: Workspace

#### T1: Create the eight-crate workspace

- **disposition:** active
- **outcome:** A virtual workspace whose eight placeholder crates build, inherit metadata and lints, follow Appendix A C-1 edges, and produce the `fuzzy-dedupe` binary, with a committed lockfile and no third-party crate.
- **work_type:** configuration
- **checkpoint:** one green commit on the topic branch with the required `Plan-*` checkpoint trailers
- **boundary:** cross-task
- **depends_on:** []
- **dependency_reason:** none
- **requirements:** [REQ-001, REQ-002, REQ-003, REQ-004, REQ-005, REQ-006]
- **proof:** [PV-T1-001, PV-T1-002, PV-T1-003, PV-T1-004, PV-T1-005, PV-T1-006]
- **source_refs:** [spec:docs/specs/canonical/fuzzy-dedupe.md#85-design-constraints, spec:docs/specs/canonical/fuzzy-dedupe.md#83-design-decisions, spec:docs/specs/canonical/fuzzy-dedupe.md#19-implementation-plan, repo:rust-toolchain.toml]
- **consumes:** [toolchain pin 1.96.0]
- **produces:** [Appendix A C-1 crate set and edges, Appendix A C-2 lint contract, `Cargo.lock`]
- **preserves:** [every pre-existing tracked file byte-identical, including `rust-toolchain.toml`]
- **invariants:** [D-001, D-002, no `[dependencies]`/`[dev-dependencies]`/`[build-dependencies]` entry outside the workspace, every created file carries the SPDX pair (D-008), no `publish` key]
- **executor_discretion:** [workspace `version` value (one pre-1.0 value in `[workspace.package]`), whether internal edges go through `[workspace.dependencies]`, crate-root doc comments stating each crate's §8.5 role, pedantic selective allows (none expected)]
- **files:** [`Cargo.toml` (create; owner T1), `Cargo.lock` (create; owner T1), `crates/fuzzy-dedupe-domain/Cargo.toml` and `src/lib.rs` (create; owner T1), `crates/fuzzy-dedupe-engine/Cargo.toml` and `src/lib.rs` (create; owner T1), `crates/fuzzy-dedupe-scanner/Cargo.toml` and `src/lib.rs` (create; owner T1), `crates/fuzzy-dedupe-analyzers/Cargo.toml` and `src/lib.rs` (create; owner T1), `crates/fuzzy-dedupe-storage/Cargo.toml` and `src/lib.rs` (create; owner T1), `crates/fuzzy-dedupe-fileops/Cargo.toml` and `src/lib.rs` (create; owner T1), `crates/fuzzy-dedupe-platform/Cargo.toml` and `src/lib.rs` (create; owner T1), `crates/fuzzy-dedupe-app/Cargo.toml` and `src/main.rs` (create; owner T1)]
- **parallel_safe:** no
- **conflicts_with:** []
- **supersedes:** []
- **superseded_by:** []
- **evidence:** [ephemeral]
- **recovery:** Nothing outside the declared paths changes. On failure, remove the uncommitted `Cargo.toml`, `Cargo.lock` and `crates/`, return to the branch base, and retry. Never stage `.claude/settings.json`.
- **acceptance:** PV-T1-001 shows the build and binary. PV-T1-002 shows inherited metadata. PV-T1-003 shows that the `unsafe` and pedantic canaries are rejected where required and accepted only in platform. PV-T1-004 shows edges equal to C-1. PV-T1-005 shows exactly eight packages, all workspace members. PV-T1-006 shows the lockfile tracked and the toolchain pin unchanged.
- **sub-tasks:**
  - **T1.1 PRECHECK** — confirm the branch base contains this master and the `.project-pipeline/` ignore rule, that no `Cargo.toml` exists, and that `rust-toolchain.toml` still pins `channel = "1.96.0"`.
  - **T1.2 APPLY** — create the manifests and crate roots per Appendix A C-1 and C-2, then run `cargo generate-lockfile` locally.
  - **T1.3 VERIFY** — run PV-T1-001, PV-T1-002, PV-T1-004, PV-T1-005 and PV-T1-006 through rexec where they compile.
  - **T1.4 PROVE POLICY** — run the PV-T1-003 canaries, revert them, and rerun Clippy clean.
  - **T1.5 Verify Task** — confirm the diff holds only T1 files, then commit with the `Plan-*` trailers.

### Phase P2: Gates and assets

#### T2: Add the Rust CI gates, boundary check and dependency policy

- **disposition:** active
- **outcome:** `rust.yml` runs the seven Appendix A C-3 jobs on PRs and `main`, `deny.toml` encodes D-007, and the boundary script and policy encode D-011 and C-7. Each job's command passes locally, and each consequential job rejects its canary.
- **work_type:** configuration
- **checkpoint:** one green commit on the topic branch with the required `Plan-*` checkpoint trailers
- **boundary:** deployment
- **depends_on:** [T1]
- **dependency_reason:** consumes the workspace `Cargo.toml`, the eight crate names and `Cargo.lock` produced by T1, which every gate and the `deny.toml` exceptions run against
- **requirements:** [REQ-007, REQ-008, REQ-009, REQ-010, REQ-011, REQ-012, REQ-013]
- **proof:** [PV-T2-001, PV-T2-002, PV-T2-003, PV-T2-004, PV-T2-005, PV-T2-006, PV-T2-007]
- **source_refs:** [spec:docs/specs/canonical/fuzzy-dedupe.md#172-test-strategy, spec:docs/specs/canonical/fuzzy-dedupe.md#85-design-constraints, spec:docs/specs/canonical/fuzzy-dedupe.md#83-design-decisions, spec:docs/specs/canonical/fuzzy-dedupe.md#72-non-functional-requirements, repo:.rexec.toml, repo:.github/workflows/admission.yml]
- **consumes:** [Appendix A C-1 crate set, `Cargo.lock`, `.rexec.toml` tool pins]
- **produces:** [Appendix A C-3 rust.yml job contract, Appendix A C-6 dependency policy, Appendix A C-7 boundary policy, `deny.toml`]
- **preserves:** [the four existing workflows, `.rexec.toml` and `Cargo.lock` byte-identical at T2's checkpoint]
- **invariants:** [D-003, D-004, D-005, D-006, D-007, D-011, every command uses `--locked` where the tool accepts it, the policy's edge table equals C-1 and its third-party table is empty, the CI step and the local proof run the same script, the script and the policy carry the SPDX pair, the R3 and R4 canaries run only against a temporary `--policy` fixture under `.project-pipeline/2026-11-22-ms-0-scaffold-workspace/execution/` that is never committed]
- **executor_discretion:** [job display names, step layout, concurrency group, cargo-deny `[bans]` and `[advisories]` settings beyond D-007, the policy's TOML layout, GUI and OS crate names beyond D-011's minimum lists, the script's internal structure and messages]
- **files:** [`.github/workflows/rust.yml` (create; owner T2), `deny.toml` (create; owner T2), `.github/scripts/check-boundaries.py` (create; owner T2), `.github/boundaries.toml` (create; owner T2)]
- **parallel_safe:** no
- **conflicts_with:** []
- **supersedes:** []
- **superseded_by:** []
- **evidence:** [ephemeral]
- **recovery:** The four files are new. On failure, delete the uncommitted `rust.yml`, `deny.toml`, `check-boundaries.py` and `boundaries.toml`. A canary left in a crate is reverted with `git restore` on that path before any commit.
- **acceptance:** PV-T2-001, PV-T2-002 and PV-T2-004 pass locally. PV-T2-003 rejects the license canary. PV-T2-005 rejects the Unix-API canary. PV-T2-006 rejects every boundary canary with exactly its expected rule ID (R3 and R4 against the fixture policy) and accepts every positive control. PV-T2-007 shows the triggers, pins and digests. `rust.yml` passes the `CLAUDE.md` Prettier check.
- **sub-tasks:**
  - **T2.1 PRECHECK** — confirm T1 is checkpointed, and read the three asset names and digests from `.rexec.toml`.
  - **T2.2 APPLY** — write `deny.toml` (D-007), `.github/boundaries.toml` (C-7), `.github/scripts/check-boundaries.py` (D-011) and `rust.yml` (C-3, D-003 to D-006), each with the SPDX pair.
  - **T2.3 VERIFY** — run PV-T2-001, PV-T2-002, PV-T2-004 and PV-T2-007.
  - **T2.4 PROVE REJECTION** — run the PV-T2-003, PV-T2-005 and PV-T2-006 canaries one at a time (the R3 and R4 ones with `--policy` pointing at the temporary fixture), check each reports exactly its expected rule ID, revert each, delete the fixture, and rerun the clean gate.
  - **T2.5 Verify Task** — confirm the diff holds only T2 files, then commit with the `Plan-*` trailers.

#### T3: Add the IBM Plex font assets and notices

- **disposition:** active
- **outcome:** The four pinned IBM Plex TTF files sit unmodified in the app crate's asset directory, and the OFL-1.1 text and a third-party notice entry record their origin and checksums.
- **work_type:** configuration
- **checkpoint:** one green commit on the topic branch with the required `Plan-*` checkpoint trailers
- **boundary:** cross-task
- **depends_on:** [T1]
- **dependency_reason:** consumes the `crates/fuzzy-dedupe-app/` package directory created by T1, which holds `assets/`
- **requirements:** [REQ-014, REQ-015]
- **proof:** [PV-T3-001, PV-T3-002]
- **source_refs:** [spec:docs/specs/canonical/fuzzy-dedupe.md#117-centralized-visual-tokens-and-reusable-widgets, spec:docs/specs/canonical/fuzzy-dedupe.md#183-deployment-flow, spec:docs/specs/canonical/fuzzy-dedupe.md#85-design-constraints]
- **consumes:** [Appendix A C-1 crate set and edges, release assets named in A-004 (owner-accepted pins)]
- **produces:** [Appendix A C-4 font asset paths, `LICENSES/OFL-1.1.txt`]
- **preserves:** [upstream file bytes, `cargo build --workspace --locked` still green]
- **invariants:** [D-009, each release zip's SHA-256 equals its GitHub asset digest before extraction, nothing outside the four TTF files and `LICENSE.txt` is copied from the zips, downloads are unpacked in a fresh directory outside the repository]
- **executor_discretion:** [download tooling, `THIRD-PARTY-NOTICES.md` layout beyond the required fields]
- **files:** [`crates/fuzzy-dedupe-app/assets/fonts/IBMPlexSans-Regular.ttf` (create; owner T3), `crates/fuzzy-dedupe-app/assets/fonts/IBMPlexSans-Medium.ttf` (create; owner T3), `crates/fuzzy-dedupe-app/assets/fonts/IBMPlexSans-SemiBold.ttf` (create; owner T3), `crates/fuzzy-dedupe-app/assets/fonts/IBMPlexMono-Regular.ttf` (create; owner T3), `LICENSES/OFL-1.1.txt` (create; owner T3), `THIRD-PARTY-NOTICES.md` (create; owner T3)]
- **parallel_safe:** no
- **conflicts_with:** []
- **supersedes:** []
- **superseded_by:** []
- **evidence:** [ephemeral]
- **recovery:** Delete the uncommitted files. If a zip digest differs from the GitHub asset digest or from A-004, stop and block the task. Do not substitute another release without an owner decision.
- **acceptance:** PV-T3-001 shows each file's SHA-256 equal to the A-004 value and to the value recorded in `THIRD-PARTY-NOTICES.md`. PV-T3-002 shows that `LICENSES/OFL-1.1.txt` is byte-identical to the release `LICENSE.txt`, that the notice entry carries the fields in C-4, and that the Prettier and markdownlint checks pass on `THIRD-PARTY-NOTICES.md`.
- **sub-tasks:**
  - **T3.1 PRECHECK** — use the A-004 pins (accepted 2026-11-22; the owner may override before T3 starts), download both zips into a fresh temporary directory, and compare their digests with the GitHub asset digests and A-004.
  - **T3.2 APPLY** — copy the four TTF files and `LICENSE.txt` per D-009, then write the `THIRD-PARTY-NOTICES.md` entry (C-4).
  - **T3.3 VERIFY** — run PV-T3-001 and PV-T3-002, and rerun `rexec -- cargo build --workspace --locked`.
  - **T3.4 Verify Task** — confirm the diff holds only T3 files, then commit with the `Plan-*` trailers.

### Phase P3: Licensing record

#### T4: Establish REUSE licensing coverage and its CI job

- **disposition:** active
- **outcome:** Every tracked file has a truthful, machine-readable copyright and license, `reuse lint` passes over the whole tree, a missing header on a new source file fails it, and `reuse.yml` runs it on PRs and `main`.
- **work_type:** configuration
- **checkpoint:** one green commit on the topic branch with the required `Plan-*` checkpoint trailers
- **boundary:** cross-task
- **depends_on:** [T1, T2, T3]
- **dependency_reason:** consumes T1's crate sources, manifests and `Cargo.lock`, T2's `deny.toml`, `rust.yml`, boundary script and policy, and T3's fonts, `LICENSES/OFL-1.1.txt` and `THIRD-PARTY-NOTICES.md`, which form the complete tree `REUSE.toml` must cover
- **requirements:** [REQ-016, REQ-017, REQ-018, REQ-019]
- **proof:** [PV-T4-001, PV-T4-002, PV-T4-003, PV-T4-004]
- **source_refs:** [spec:docs/specs/canonical/fuzzy-dedupe.md#183-deployment-flow, spec:docs/specs/canonical/fuzzy-dedupe.md#172-test-strategy, repo:.standards/lock.toml, external:L3DigitalNet/project-standards LICENSE at the locked release, repo:docs/handoff/conventions.md, repo:LICENSE, repo:rust-toolchain.toml, repo:.rexec.toml, repo:.github/workflows/admission.yml]
- **consumes:** [Appendix A C-1 crate set and edges, `Cargo.lock`, Appendix A C-3 rust.yml job contract, `deny.toml`, `.github/boundaries.toml` and `.github/scripts/check-boundaries.py` (Appendix A C-7), Appendix A C-4 font asset paths, `LICENSES/OFL-1.1.txt`, `.standards/lock.toml` managed-path list]
- **produces:** [Appendix A C-5 REUSE coverage contract, Appendix A C-3 reuse.yml job contract]
- **preserves:** [`LICENSE`, every standards-managed file, `.gitignore` and `.claude/settings.json` byte-identical; the three header-receiving files change only by the two SPDX comment lines]
- **invariants:** [D-008, D-012, every annotation states a holder and license supported by the origin ledger (C-5), `LICENSES/` holds only licenses some file uses]
- **executor_discretion:** [annotation grouping and order, `precedence` values consistent with D-008, how `reuse` is invoked (`uvx` locally, `pipx run` in CI) at version 6.3.0]
- **files:** [`REUSE.toml` (create; owner T4), `LICENSES/GPL-3.0-only.txt` (create; owner T4), `LICENSES/Apache-2.0.txt` (create; owner T4), `.github/workflows/reuse.yml` (create; owner T4), `rust-toolchain.toml` (modify, SPDX lines only; owner T4), `.rexec.toml` (modify, SPDX lines only; owner T4), `.github/workflows/admission.yml` (modify, SPDX lines only; owner T4)]
- **parallel_safe:** no
- **conflicts_with:** []
- **supersedes:** []
- **superseded_by:** []
- **evidence:** [ephemeral]
- **recovery:** Delete the new files and `git restore` the three header-receiving files on failure. If an uncovered file's origin or true license cannot be established from evidence, block with that path. Never annotate a guessed license. If the upstream `LICENSE` at the locked release differs from A-005, use what the release states and record it in the ledger.
- **acceptance:** PV-T4-001 shows that the header-less canary fails `reuse lint`. PV-T4-002 shows both GPL texts byte-correct. PV-T4-003 shows `reuse lint` passing, no annotation glob matching a protected path, and every annotation agreeing with the origin ledger. PV-T4-004 shows no `-or-later` declaration. `reuse.yml` passes the Prettier check.
- **sub-tasks:**
  - **T4.1 PRECHECK** — confirm `git ls-files '.agents/skills/figma-*'` prints nothing and `git check-ignore` covers that path. Then build the origin ledger in the execution log. It classifies every tracked file as header-bearing (D-008) or as one of D-012's classes. For each create-only file, compare its SHA-256 with the source content digest in `.standards/lock.toml`. Read the upstream license and holder from the `L3DigitalNet/project-standards` `LICENSE` at the locked release.
  - **T4.2 APPLY** — add the SPDX pair to the three pre-existing files, then write `LICENSES/GPL-3.0-only.txt`, `LICENSES/Apache-2.0.txt` (A-005), `REUSE.toml` (each annotation commented with its D-012 class) and `reuse.yml`.
  - **T4.3 VERIFY** — run PV-T4-002, PV-T4-003 and PV-T4-004.
  - **T4.4 PROVE REJECTION** — run the PV-T4-001 and PV-T4-003 canaries, revert them, and rerun `reuse lint` and the ledger comparison clean.
  - **T4.5 Verify Task** — confirm the diff holds only T4 files, then commit with the `Plan-*` trailers.

### Phase P4: Integrated proof

#### T5: Prove every gate on the draft pull request

- **disposition:** active
- **outcome:** A Standalone draft PR from the topic branch shows every `rust.yml` and `reuse.yml` job, and every job of `format`, `lint-markdown` and `validate-specs`, passing on its head commit.
- **work_type:** verification
- **checkpoint:** one green commit adding EV-001, with the required `Plan-*` checkpoint trailers
- **boundary:** deployment
- **depends_on:** [T2, T4]
- **dependency_reason:** consumes `.github/workflows/rust.yml` from T2 and `.github/workflows/reuse.yml` from T4, the two workflows whose runs this task proves
- **requirements:** [REQ-020]
- **proof:** [PV-T5-001]
- **source_refs:** [repo:.agents/skills/github-workflow/references/pr-standard.md, spec:docs/specs/canonical/fuzzy-dedupe.md#19-implementation-plan]
- **consumes:** [Appendix A C-3 rust.yml job contract, Appendix A C-3 reuse.yml job contract]
- **produces:** [draft PR number, EV-001 CI gate record]
- **preserves:** [PR stays draft; no `ready`, `merge` or `land` in this task]
- **invariants:** [the PR body uses the four ready-contract sections with `Standalone` and a `Change risk:` line (R2 Moderate unless the executor judges otherwise and says why), one blocking `gh pr checks N --watch --fail-fast` call per run with no poll loop]
- **executor_discretion:** [PR title and body prose]
- **files:** [`docs/evidence/ms-0-ci-gates.md` (create; owner T5)]
- **parallel_safe:** no
- **conflicts_with:** []
- **supersedes:** []
- **superseded_by:** []
- **evidence:** [EV-001]
- **recovery:** A red job blocks T5. Append a correction task that `corrects:` the task owning the failing file, with `discovered_from: [T5]`, complete it, push, and rerun PV-T5-001 against the new head.
- **acceptance:** PV-T5-001 shows that, on the PR head commit, all seven `rust.yml` jobs, the `reuse` job, and every job of `format`, `lint-markdown` and `validate-specs` concluded `success`. `admission` is not expected on the PR. EV-001 records that head OID, the PR number, each run ID and per-job conclusion, and the gate tool versions. EV-001 passes the Prettier and markdownlint checks.
- **sub-tasks:**
  - **T5.1 ANCHOR** — push the topic branch and open the Standalone draft PR with `gh pr create --draft --body-file`.
  - **T5.2 RUN** — watch the PR checks with one blocking call and triage any failure under the recovery rule.
  - **T5.3 CAPTURE EVIDENCE** — write EV-001 from the observed runs.
  - **T5.4 Verify Task** — run the Prettier and markdownlint checks on EV-001, commit with the `Plan-*` trailers, push, and confirm that the new head (documentation-only delta) is green.

### Phase P5: Landing and `main` proof

#### T6: Land MS-0 and prove it on `main`

- **disposition:** active
- **outcome:** The verified PR is squash-landed, and every applicable gate passes on the squash commit on `main`. A close-out branch then records the `main` results and the Appendix B.3 completion report (EV-002), the §17.3 NFR-008 Status, and any `DEV-` row. MS-0 is complete, and MS-1 may start, only after the close-out PR (§10.1 step 4) lands.
- **work_type:** operational
- **checkpoint:** one green commit on the close-out branch adding EV-002 and the spec close-out record, with the required `Plan-*` checkpoint trailers
- **boundary:** deployment
- **depends_on:** [T5]
- **dependency_reason:** consumes the draft PR number and verified head from T5, which this task lands and whose squash commit it proves
- **requirements:** [REQ-021, REQ-022]
- **proof:** [PV-T6-001, PV-T6-002]
- **source_refs:** [spec:docs/specs/canonical/fuzzy-dedupe.md#19-implementation-plan, spec:docs/specs/canonical/fuzzy-dedupe.md#173-requirement-to-test-traceability, spec:docs/specs/canonical/fuzzy-dedupe.md#deviations-log, spec:docs/specs/canonical/fuzzy-dedupe.md#appendix-b-agent-implementation-contract, repo:.agents/skills/github-workflow/references/pr-standard.md, repo:.github/workflows/admission.yml]
- **consumes:** [draft PR number, EV-001 CI gate record]
- **produces:** [squash commit OID, EV-002 completion report, spec §17.3 NFR-008 Status]
- **preserves:** [the topic branch and its checkpoint commits, locally and on `origin`; the spec everywhere except the NFR-008 Status cell and the Deviations Log]
- **invariants:** [§12's pull-request checks pass on the actual PR head before `land`; the latest valid T5 checkpoint, or its last correction checkpoint, is an ancestor of that head and every later commit is a D-013 handoff-only commit touching only the session-state set; `land --method squash` only; a red `main` run on the squash commit keeps T6 in progress and MS-1 blocked; the close-out branch is cut from the `main` that contains the squash commit; nothing written before a commit names that commit, so EV-002 at T6's checkpoint names T1–T5 checkpoints and the harvest commit adds T6's]
- **executor_discretion:** [close-out branch name, the wording of the NFR-008 Status cell, EV-002 layout beyond the required fields]
- **files:** [`docs/evidence/ms-0-completion-report.md` (create; owner T6), `docs/specs/canonical/fuzzy-dedupe.md` (modify, §17.3 NFR-008 Status cell and Deviations Log only; owner T6)]
- **parallel_safe:** no
- **conflicts_with:** []
- **supersedes:** []
- **superseded_by:** []
- **evidence:** [EV-002]
- **recovery:** Before `land`, a red check follows §7. After `land`, a red run on the squash commit is triaged under §10.3. A tree defect gets a correction task that `corrects:` the owning task, with `discovered_from: [T6]`, landed through a new Standalone PR. An `admission` failure is a landing defect, repaired as `pr-standard.md` directs. A time-varying input (advisory, runner image, upstream reusable workflow) gets a forward fix through a new PR. After any fix, PV-T6-001 reruns on the new `main` commit. If T6 is interrupted, `recover` rebuilds T1–T5 from the retained topic branch, and `gh run list --commit "$SQUASH_OID"` re-reads the observed runs.
- **acceptance:** PV-T6-001 shows every listed `main` run on the squash commit concluded `success`, and `gh-workflow admission --branch main` reported zero unadmitted commits. PV-T6-002 shows EV-002 with every Appendix B.3 field, both run records, the landed PR head OID next to the T5 checkpoint OID it descends from, and only identifiers that existed before the commit that wrote them. It also shows the NFR-008 Status cell naming MS-0's part, and a `DEV-` row for each deviation (the Deviations Log is unchanged when there is none). The spec gates pass, and Prettier and markdownlint pass on both changed files.
- **sub-tasks:**
  - **T6.1 PREFLIGHT** — confirm T5 is checkpointed and that the latest valid T5 checkpoint, or its last correction checkpoint (`$T5_CHECKPOINT`), is an ancestor of the current PR head (`git merge-base --is-ancestor "$T5_CHECKPOINT" HEAD`). Every commit listed by `git log --format=%H "$T5_CHECKPOINT"..HEAD` must be a D-013 handoff-only commit with no `Plan-*` trailers, and `git diff --name-only "$T5_CHECKPOINT" HEAD` must list only session-state-set paths; any other intervening change blocks T6.1 and gets a correction task. Confirm that §12's pull-request checks pass on that actual head, and record the head OID for EV-002 next to `$T5_CHECKPOINT`.
  - **T6.2 APPLY** — land: run `.agents/skills/github-workflow/bin/gh-workflow land --pr N --method squash` and record the squash OID it reports. Keep the topic branch.
  - **T6.3 VERIFY** — watch each `main` run on the squash commit with one blocking `gh run watch "$RUN_ID" --exit-status`, then run `gh-workflow admission --branch main`. Triage any red run under the recovery rule.
  - **T6.4 CAPTURE EVIDENCE** — cut the close-out branch from the new `main`. Write EV-002 with the T1–T5 checkpoint OIDs, the landed head OID, the MS-0 PR number, its squash OID and its `main` run results, update the NFR-008 Status cell, and add a `DEV-` row for each deviation. Then run `project-standards spec validate --strict` and `spec lint --strict` and the Markdown checks.
  - **T6.5 Verify Task** — confirm the diff holds only T6 files, then commit with the `Plan-*` trailers.

## 10. Integration, Migration, and Recovery

### 10.1 Integration Sequence

1. Merge the plans PR. After it lands, cut the topic branch from `main`, run `generate`, and execute T1 → (T2, T3 serially) → T4 → T5. Each task is a local rexec-verified checkpoint.
2. Push and open the draft PR at T5. Before then, pushing is optional and changes nothing.
3. T6: once T6.1 confirms the PR head descends from the latest valid T5 checkpoint through D-013 commits only and §12's pull-request checks pass on that head, land with `gh-workflow land --pr N --method squash` (D-010). Then prove every gate on the squash commit, and record the result on a close-out branch cut from the new `main`.
4. Close-out (§13): add the harvest to the close-out branch as a second commit, recording only identifiers that already exist: the T1–T6 checkpoint OIDs, the MS-0 PR number, its squash OID and its `main` run results. Then open its Standalone PR, land it with `gh-workflow land --method squash`, and confirm its `main` runs. MS-0 is complete, and MS-1 may start, only after this PR lands with green `main` runs. Keep the topic and close-out branches, locally and on `origin`, until then.
5. Post-landing record, the last step: write the post-landing handoff record (D-013): one handoff-only commit on `main`, made through the agent-handoff skill with the `Workflow-Admission: handoff` trailer, that touches only a `docs/handoff/sessions/` row, `docs/handoff/state.md` and `docs/STATUS.md`, carrying the close-out PR's number, its squash OID and its `main` run results. It records nothing about itself; if it is lost, it is rewritten from `gh run list` and `gh pr view`.

Any session that ends between the start of T1 and the landing of the close-out PR ends with a D-013 handoff-only commit on the current branch.

### 10.2 Migration / State / Configuration Transition

- Required: no data migration. The only state transition is adding new files.
- Idempotency: every task creates new paths only, and rerunning a gate is side-effect free.
- Point of no return: `land`. Before it, abandoning means closing the draft PR. After it, a defect is fixed by a new governed PR, and MS-0 stays incomplete until T6 shows `main` green. The squash is never force-reverted.
- Rollback / forward repair: before landing, use correction tasks on the branch. After landing, revert the squash commit through a PR only if `main` gates are red and a forward fix is not quick.

### 10.3 Late Failure and Correction

A red gate at T5 or in §12's pull-request checks follows §7's correction rule. After `land`, a red run on the squash commit keeps MS-0 incomplete and MS-1 blocked, and T6's recovery rule applies. The squash tree equals the verified head, so most red `main` runs have one of four causes. `admission` runs only on pushes to `main`, so its first run comes after landing. The others are a new RustSec advisory, a runner image change, and an upstream reusable workflow change. A tree defect is still possible, because no check ran the `main` push event before landing. Every cause is fixed through a new Standalone PR, after which T6 reruns PV-T6-001 on the resulting `main` commit. Completed task definitions are never edited; a fix is an appended correction task.

## 11. Risks, Assumptions, and Open Questions

### 11.1 Risks

| ID | Risk | Likelihood | Impact | Treatment | Owner / Task |
| --- | --- | --- | --- | --- | --- |
| R-001 | A new RustSec advisory or a cargo-deny default change turns a gate red without a code change | low | medium | No dependencies at MS-0; treat as a §10.3 post-land fix | executor / T2 |
| R-002 | `reuse` 6.3.0's unpinned Python dependencies change behavior in CI | low | low | Pin the `reuse` version; a CI-only failure blocks T5 and gets a correction task | executor / T4 |
| R-003 | The MS-1 plan assumes different job IDs, font paths, or LICENSES contents | medium | medium | Appendix A is the produced contract; the orchestrator reconciles both drafts before promotion | orchestrator / T2, T3, T4 |
| R-004 | Prettier reformats the new workflow YAML and `THIRD-PARTY-NOTICES.md` | medium | low | Run the `CLAUDE.md` Prettier check on those files inside T2, T3 and T4 | executor / T2, T3, T4 |

### 11.2 Assumptions

| ID | Assumption | Impact if False |
| --- | --- | --- |
| A-004 | Pins (accepted 2026-11-22, OQ-P03): `@ibm/plex-sans@1.3.6` (zip SHA-256 `a52bd405930040a6bfdb5c8aff67f1b7205538999986e2186d8f8895ca6a5de1`) and `@ibm/plex-mono@2.5.0` (zip `6d23f01257663d8cc49a0d64c22ced630b79e0e2a0ac08a0da86e9a38bbc481c`). Expected file SHA-256: Sans-Regular `b47ebd72b5ce53b60f909033ff84d6fc12358cc147e2981f8c321ad3f36246b6`, Sans-Medium `a494d5a69fa094f7071a9ffbc98e86c85337d1a29f3fc1ff7c2e1c46161ddf27`, Sans-SemiBold `c51119b25f4a296da2b66edf3201b9aa1735091ed7efc1fca0313346f3851a4d`, Mono-Regular `7c6fbddca4b700be918f5f6183d9bd4464fa427fe435f0b480d77fe2bb8c5a43`. | If the owner picks other releases, T3 uses them and records their digests instead |
| A-005 | `L3DigitalNet/project-standards`, the source of every standards-managed and create-only file here, is licensed Apache-2.0 (observed 2026-11-22 through the GitHub API) with L3DigitalNet as holder. T4.1 confirms both from the `LICENSE` at the locked release. D-012 applies them to class (b) and (c) files. | T4 uses whatever the locked release states. If the owner says the two holders are one entity relicensing its own files, D-012 reopens |
| A-006 | The app→platform edge is the only platform edge MS-1 needs (D-002) | MS-1 adds any other edge it needs within the C-1 direction rules |

### 11.3 Open Questions

| ID | Question | Current Assumption | Blocking? | Owner | Needed By | Status |
| --- | --- | --- | --- | --- | --- | --- |
| OQ-P01 | How does `REUSE.toml` cover the vendored `.agents/skills/figma-*` files, whose upstream grants no license? | **Answered 2026-11-22 by the owner:** they are no longer tracked. The plans PR removes them from the index and gitignores `.agents/skills/figma-*/`, and convention C1 now says they are installed locally only. `reuse lint` skips gitignored files, so T4 needs no Figma annotation. T4.1 confirms that no `figma-*` path is tracked. | No | owner | Answered | Answered |
| OQ-P02 | Do the spec gaps found while planning need amendments: the lint split, workspace-crate license exceptions, ISC in `LICENSES/`, per-family font pins, and the definitions of source file and direct dependency? | **Answered by spec v0.9:** §8.5 states the lint split (D-001), §17.2 lists the eight crates as `GPL-3.0-only` exceptions (D-007), §8.5 lists the licenses in use, §11.7 pins per family, §8.3 defines direct dependency (REQ-005), and §18.3 defines project-owned source file (D-008). | No | spec owner | Answered | Answered |
| OQ-P03 | Which IBM Plex releases are pinned? | **Answered:** the A-004 pins `@ibm/plex-sans@1.3.6` and `@ibm/plex-mono@2.5.0`, accepted 2026-11-22. The owner may override before T3. | No | owner | Answered | Answered |

## 12. Final Verification

Before T6 lands the PR (the pull-request checks):

- Reconcile REQ-001–REQ-020 against completed tasks and passing Appendix B rows. Rows marked checkpoint-only (PV-T1-006) are reconciled by their result at that checkpoint and are not rerun on the final tree; the check below replaces PV-T1-006 there. Confirm that every checkpoint commit carries valid `Plan-*` trailers and that no canary survives in the tree.
- `cargo metadata` shows exactly eight workspace packages and no others. On the topic branch, a diff against the base shows every §3.3 unchanged-set file byte-identical. Session-state-set files may differ only through D-013 commits: every commit on the branch that touches one of them is handoff-only, with no `Plan-*` trailers and no other path, and no other commit touches them. The diff also shows each SPDX-delta-set file changed only by the two SPDX comment lines. `rust-toolchain.toml` still sets `channel = "1.96.0"`, `components = ["rustfmt", "clippy", "rust-analyzer"]` and `profile = "minimal"`, and `rexec -- rustup show active-toolchain` reports 1.96.0. This pin and SPDX-delta check is PV-T1-006's final-tree replacement and fails if the channel, profile or components change.
- PR CI on the actual head is green across every job in PV-T5-001. Rerun PV-T5-001 if commits followed T5. T6.1's ancestry check passes: the head descends from the latest valid T5 checkpoint (or its last correction) through D-013 handoff-only commits only.
- Run the Prettier and markdownlint commands from `CLAUDE.md` over the full tracked scope, then run `project-standards agent-handoff validate --repo .` and `drift-check --repo .` to confirm managed files are untouched.

After landing:

- REQ-021 and REQ-022 hold (T6: PV-T6-001, PV-T6-002). "Passes on `main`" (exit item 3) is REQ-021.
- The Appendix B.3 completion report is EV-002. It records the summary of changes and files changed, each REQ with its spec ID and proof, the tests added or changed (none; D-006), deviations and their approval status, known limitations and open questions, and the documentation delivered (`THIRD-PARTY-NOTICES.md`; the §18.7 deliverables fall due later). It also records the T1–T5 checkpoint OIDs (the harvest commit adds T6's), the landed head OID next to the T5 checkpoint OID, the MS-0 PR number, the squash OID and every `main` run with its conclusion. The close-out PR's own identifiers go to the post-landing handoff record (D-013), never into a file that PR commits.

## 13. Close-out

- **Completed:** pending. The harvest commit records the T1–T6 checkpoint OIDs, the MS-0 PR number, its squash OID and its `main` run results; the close-out PR's number, squash OID and `main` runs go to the post-landing handoff record (§10.1 step 5).
- **Decisions / deviations harvested:** pending. Deviations are `DEV-` rows in the spec (T6). Scope questions go to the owner as `OQ-` rows, not into the plan.
- **Risks closed / accepted:** pending.
- **Deferred/discovered work filed:** pending. Appendix D items are carried to the MS-1 plan or the handoff.
- **Source/ADR/handoff reconciliation:** pending. Sessions that end before close-out leave D-013 handoff-only commits; they do not replace this harvest. T6 records the NFR-008 status and any `DEV-` row in the spec. The close-out PR records the MS-0 status and the Appendix A contracts in the handoff through the agent-handoff skill. It also lists here the task checkpoint OIDs, the MS-0 PR number and its squash OID, because squash landing keeps the checkpoints only on their branches (D-010); the close-out PR's number and squash OID go to the post-landing handoff record.
- **Scratch teardown:** only after the close-out PR lands with green `main` runs and the post-landing handoff record is on `main`. The topic and close-out branches may be deleted only after that, since they alone carry the `Plan-*` checkpoints.

## Appendix A. Interface and State Contracts

| Contract | Owner / Producer Task | Consumer(s) | Current | Planned / States | Errors / Limits | Compatibility / Invariant | Source |
| --- | --- | --- | --- | --- | --- | --- | --- |
| C-1 Crate set and edges | T1 | T2 (C-7 edge table), T4, MS-1+ | None | Binding block below | No edge to `fuzzy-dedupe-app`; domain has none | Later milestones may add edges only within these direction rules | spec §8.2, §8.5 |
| C-2 Lint contract | T1 | all crates, MS-1+ | None | D-001 | A new crate must inherit and, unless it is platform, carry the crate-root `forbid` | Selective pedantic allows live only in the workspace manifest | spec §8.5 |
| C-3 CI jobs | T2 (`rust.yml`), T4 (`reuse.yml`) | T5, MS-1 | Four workflows | Binding block below | Job IDs are stable; renaming one is a contract change | MS-1 adds its UI-test job to `rust.yml`, removes `--no-tests=warn` with its first test, and adds a workspace `clippy.toml` that the `clippy` job reads without a command change | spec §17.2 |
| C-4 Font assets | T3 | MS-1 font loading | None | `crates/fuzzy-dedupe-app/assets/fonts/{IBMPlexSans-Regular,IBMPlexSans-Medium,IBMPlexSans-SemiBold,IBMPlexMono-Regular}.ttf` | Bytes never modified; replacement only as a reviewed pin change updating digests | The notice entry records family, release tag, source URL, the copyright line, OFL-1.1, Reserved Font Name "Plex", and each file's path and SHA-256 | spec §11.7, §18.3 |
| C-5 REUSE coverage | T4 | MS-1+ | None | D-008 and D-012, with the T4.1 origin ledger | No annotation glob matches `*.rs`, `Cargo.toml`, `deny.toml`, `.github/scripts/**`, `.github/boundaries.toml`, `rust.yml`, `reuse.yml`, `admission.yml`, any workflow a later milestone creates, or header-bearing configuration a later milestone adds (MS-1: `clippy.toml`); standards-managed workflows are annotated by exact path | Whoever adds a new third-party asset class adds its annotation and `LICENSES/` text in the same change (MS-1: Lucide icons with ISC) | spec §18.3 |
| C-6 Dependency policy | T2 | MS-1+ | None | D-007 | Unknown registries and git sources denied | Adding a crate also needs its §8.3 row and its C-7 entry; `deny.toml` is not the approval record | spec §8.3, §17.2 |
| C-7 Boundary policy | T2 | T4, MS-1+ | None | `.github/boundaries.toml`: the C-1 edge table, a third-party table mapping each direct dependency to the crates allowed to declare it (empty at MS-0), the GUI crate list (app only) and the OS crate list (platform only); checked by `check-boundaries.py` (D-011), which reads it by default and any other policy file through `--policy` | A dependency or edge missing from the policy fails `boundaries` | A change adding a direct dependency adds its §8.3 row and its entry here in the same PR, because the policy is not the approval record. A new edge keeps C-1's direction rules | spec §8.3, §8.5, NFR-008 |

**Binding** — C-1 edges (workspace path dependencies only):

```text
fuzzy-dedupe-domain     -> (none)
fuzzy-dedupe-platform   -> domain
fuzzy-dedupe-storage    -> domain
fuzzy-dedupe-scanner    -> domain, storage
fuzzy-dedupe-analyzers  -> domain, storage
fuzzy-dedupe-fileops    -> domain, storage
fuzzy-dedupe-engine     -> domain, storage, scanner, analyzers, fileops
fuzzy-dedupe-app        -> domain, engine, platform   [[bin]] name = "fuzzy-dedupe"
```

**Binding** — C-3 workflows and job IDs:

```text
.github/workflows/rust.yml   (T2)
  fmt             cargo fmt --all --check
  clippy          cargo clippy --workspace --all-targets --locked -- -D warnings
  test            cargo build --workspace --locked
                  cargo nextest run --workspace --locked --no-tests=warn
                  cargo test --doc --workspace --locked
  deny            cargo deny check
  audit           cargo audit
  portability     cargo check --locked -p fuzzy-dedupe-domain -p fuzzy-dedupe-platform
                    --target x86_64-pc-windows-msvc, then --target aarch64-apple-darwin
  boundaries      python3 .github/scripts/check-boundaries.py
                    rules R1–R5 of D-011 against .github/boundaries.toml (C-7;
                    --policy PATH overrides it for canaries)
.github/workflows/reuse.yml  (T4)
  reuse           reuse lint   (reuse 6.3.0)
```

## Appendix B. Requirement-to-Proof Traceability

| Proof ID | Requirement(s) | Task | Method | Oracle | Command / Procedure | Expected Result | Negative Control | Environment | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| PV-T1-001 | REQ-001 | T1 | build | §19 item 1 | `rexec -- cargo build --workspace --locked`; `cargo metadata --no-deps` target list | Exit 0; eight packages named per C-1; `fuzzy-dedupe-app` has one bin target `fuzzy-dedupe` | — | rexec worker | ephemeral |
| PV-T1-002 | REQ-002 | T1 | inspection | §19 item 2 | `cargo metadata --no-deps`; read the root manifest | Every package reports license `GPL-3.0-only`, edition `2024`, rust_version `1.96`; root has `resolver = "3"`; each crate manifest has `[lints] workspace = true` and nothing else under `[lints]` | — | local | ephemeral |
| PV-T1-003 | REQ-003 | T1 | static check | §8.5 lint sentence via D-001 | Temporary canaries, each followed by `rexec -- cargo clippy --workspace --all-targets --locked -- -D warnings` | A scoped allow on an `unsafe` block compiles only in platform; the clean tree passes | Fails for: an `unsafe` block under `#[allow(unsafe_code)]` in domain (forbid), the same block without the allow in platform (deny), and a pedantic-only violation such as `must_use_candidate` in engine. Each diagnostic must name the expected lint. A canary that fails on another lint, such as `unused_unsafe` or `missing_safety_doc`, is not a valid control | rexec worker | ephemeral |
| PV-T1-004 | REQ-004 | T1 | inspection | Appendix A C-1 | `cargo metadata --no-deps` dependency lists per package | Edge set equals C-1 exactly; no package lists `fuzzy-dedupe-app` | — | local | ephemeral |
| PV-T1-005 | REQ-005 | T1 | inspection | §8.3 table (no MS-0 rows) | `cargo metadata --format-version 1 --locked` | `packages` has exactly eight entries, all with `source` null | — | local | ephemeral |
| PV-T1-006 | REQ-006 | T1 | inspection (T1 checkpoint only) | Branch-base toolchain file | At T1's checkpoint: `git ls-files Cargo.lock`; `git diff --exit-code "$(git merge-base HEAD main)" -- rust-toolchain.toml` | `Cargo.lock` tracked; toolchain file unchanged at T1's checkpoint | Not rerun on the final tree, where T4's two SPDX lines are intended; §12's pin and SPDX-delta check replaces it there and fails if the channel, profile or components change | local | ephemeral |
| PV-T2-001 | REQ-007 | T2 | static check | §17.2 | `rexec -- cargo fmt --all --check`; `rexec -- cargo clippy --workspace --all-targets --locked -- -D warnings` | Both exit 0 | — | rexec worker | ephemeral |
| PV-T2-002 | REQ-008 | T2 | static check | §17.2 | The three `test` commands in C-3 through rexec | All exit 0; nextest warns that there are no tests | — | rexec worker | ephemeral |
| PV-T2-003 | REQ-009 | T2 | static check | §17.2 allowed list, D-007 | `rexec -- cargo deny check` | Exit 0 on the clean tree | Temporarily set one crate's license to `GPL-3.0-or-later`: `cargo deny check licenses` fails | rexec worker | ephemeral |
| PV-T2-004 | REQ-010 | T2 | static check | RustSec database | `rexec -- cargo audit` | Exit 0 with zero vulnerabilities | — | rexec worker | ephemeral |
| PV-T2-005 | REQ-011 | T2 | build | §17.2, NFR-008 | `rexec --shell` adding both D-005 targets and running the C-3 `portability` checks | Both targets exit 0 | A temporary `use std::os::unix::ffi::OsStrExt;` in domain fails the Windows check | rexec worker | ephemeral |
| PV-T2-006 | REQ-012 | T2 | static check | NFR-008, §8.3, D-011, C-7; each canary's expected rule ID, so removing rule 3 or rule 4 fails its placement control | `python3 .github/scripts/check-boundaries.py`, locally; placement canaries with `python3 .github/scripts/check-boundaries.py --policy "$FIXTURE"`, where `$FIXTURE` is a temporary policy under `.project-pipeline/2026-11-22-ms-0-scaffold-workspace/execution/` that is never committed | Exit 0 on the clean tree; each canary fails with failure lines carrying only its expected rule ID; each positive control exits 0 | Each canary must fail with the script's own rule ID and message, not a Cargo or compiler error. Real policy: R5 for `#[cfg(unix)]` in domain, `cfg!(target_os = "linux")` in app, `use std::os::unix::ffi::OsStrExt;` in app, and in scanner `use std::{os::unix::fs::MetadataExt};`, `use std::os::{unix::fs::MetadataExt};`, `use std::{os::{unix::fs::MetadataExt}};`, the three-line form `use std::os::{` / `unix::fs::MetadataExt,` / `};`, `use std::os::*;`, `use std::os as host;` with `use host::unix::fs::MetadataExt;`, `use std::os::{self as host};` with the same second `use`, `use std::os;`, `use std as s;` with `use s::os as host;` and `use host::unix::fs::MetadataExt;`, `use std as s;` with `use s::os::{self as host};` and the same third `use`, `use std as s;` on its own, `extern crate std as s;`, `use std::*;` with `use os as host;` and `use host::unix::fs::MetadataExt;`, `use std::r#os::unix::fs::MetadataExt;`, `include!("../../fuzzy-dedupe-platform/src/lib.rs");`, and `use std::include as inc;`; R1 for a `fuzzy-dedupe-storage` path dependency in platform (a non-cyclic edge outside C-1); R2 for unapproved `serde` under `[dependencies]` in domain. Fixture policy, which approves the canary crate for its declaring crate so that only the placement rule can fire: R3 for `egui`, approved for engine, under `[dev-dependencies]` in engine; R4 for `libc`, approved for scanner, under `[dependencies]` in scanner; R4 for `libc`, approved for storage, declared as `sys = { package = "libc", version = "0.2" }` in storage; R4 for `libc`, approved for scanner, under a `[target.'cfg(windows)'.dependencies]` table in scanner. Positive controls: in platform, `#[cfg(target_os = "linux")]`, `use std::os::unix::ffi::OsStrExt;` and every scanner R5 canary above (the nested, multi-line, glob, OS-module alias, chained-alias, root-alias, `extern crate` and raw-identifier forms) pass; `use std::os::raw::c_char;` and `use std::os::{raw::c_char};` pass in scanner; `std::os::unix` inside a comment or a string literal in scanner passes; `self.os` field access in scanner passes; `include_str!` and `include_bytes!` in scanner pass; under the fixture, `egui` approved for and declared in app and `libc` approved for and declared in platform pass | local | ephemeral |
| PV-T2-007 | REQ-013 | T2 | inspection | `.rexec.toml`, D-003, D-004 | Read `rust.yml` | Triggers `pull_request`, `push` to `main` and `workflow_dispatch`; only `actions/*` actions, SHA-pinned; tool assets and digests equal `.rexec.toml`; no toolchain action; Prettier check passes | — | local | ephemeral |
| PV-T3-001 | REQ-014 | T3 | inspection | GitHub asset digests and A-004 file digests | `sha256sum` of both zips, before extraction, and of the four committed files | Every digest equals its A-004 value and the value recorded in `THIRD-PARTY-NOTICES.md` | A one-byte change to a copy of any font changes its digest, so the comparison detects modification | local | ephemeral |
| PV-T3-002 | REQ-015 | T3 | inspection | Release `LICENSE.txt`, C-4 fields | `cmp` against the release `LICENSE.txt`; read the notice; `CLAUDE.md` Prettier and markdownlint on `THIRD-PARTY-NOTICES.md` | Identical bytes; every C-4 field present; both checks pass | — | local | ephemeral |
| PV-T4-001 | REQ-016 | T4 | static check | D-008, §18.3 | `grep -L` for both SPDX lines over the files D-008 names; then a canary | No file lacks either line. The three pre-existing files differ from the branch base only by the two SPDX lines, and `rexec config show` still loads `.rexec.toml` | A new header-less `crates/fuzzy-dedupe-domain/src/canary.rs` makes `reuse lint` fail, proving no glob covers Rust sources | local | ephemeral |
| PV-T4-002 | REQ-017 | T4 | inspection | gnu.org GPLv3 digest `3972dc97…6986` | `sha256sum LICENSE LICENSES/GPL-3.0-only.txt`; `reuse lint` | `LICENSE` matches the digest; the GPL text in `LICENSES/` is accepted by `reuse lint` | — | local | ephemeral |
| PV-T4-003 | REQ-018 | T4 | static check | REUSE specification via `reuse` 6.3.0; C-5; D-012 origin ledger | `uvx reuse@6.3.0 lint`; compare every `REUSE.toml` path and annotation with the C-5 protected set and the origin ledger | Exit 0; zero missing or unused licenses; no annotation matches a protected path; every annotation's holder and license equal its ledger class, checked at least for one unchanged create-only scaffold (for example `docs/adr/adr.template.md`, class b), one edited create-only document (for example `docs/handoff/state.md`, class c), one block-managed file (`CLAUDE.md`, class c) and one original project document (the spec, class a); `reuse.yml` runs the same version on the C-3 triggers | Temporarily annotating the unchanged `docs/adr/adr.template.md` as `GPL-3.0-only` under L3Digital LLC fails the ledger comparison, even though `reuse lint` alone accepts it | local | ephemeral |
| PV-T4-004 | REQ-019 | T4 | inspection | §18.3 | `git grep` for `or-later` in SPDX lines, `REUSE.toml` and Cargo `license` fields | No match | — | local | ephemeral |
| PV-T5-001 | REQ-020 | T5 | CI integration | Each job's conclusion | `gh pr checks N --watch --fail-fast` on the head | Every C-3 job, and every job of `format`, `lint-markdown` and `validate-specs`, concludes `success`; `admission` is absent, because it runs only on `main` | — | GitHub-hosted `ubuntu-latest` | EV-001 |
| PV-T6-001 | REQ-021 | T6 | CI integration | Each `main` run's conclusion on the squash commit | `gh run list --commit "$SQUASH_OID"`, then `gh run watch "$RUN_ID" --exit-status` per run; `.agents/skills/github-workflow/bin/gh-workflow admission --branch main` | `rust.yml`, `reuse.yml`, `format`, `lint-markdown`, `validate-specs` and `admission` each conclude `success` on the squash OID; admission reports zero unadmitted commits | A run on any other commit does not count toward the result | GitHub-hosted `ubuntu-latest` | EV-002 |
| PV-T6-002 | REQ-022 | T6 | inspection | Spec Appendix B.3 field list, §17.3, Deviations Log | Read EV-002 against B.3; read the NFR-008 row and the Deviations Log; `project-standards spec validate --strict` and `spec lint --strict`; `CLAUDE.md` Prettier and markdownlint on both changed files | Every B.3 field present; the landed head OID beside the T5 checkpoint OID; no identifier of the commit that writes it; NFR-008 Status names MS-0's part; one `DEV-` row per deviation; all checks pass | — | local | EV-002 |

## Appendix C. Durable Evidence

| Evidence ID | Producing Task | Path | Contents / Provenance | Privacy Exclusions | Retention Reason |
| --- | --- | --- | --- | --- | --- |
| EV-001 | T5 | `docs/evidence/ms-0-ci-gates.md` | Verified PR head OID, PR number, each workflow run ID with per-job conclusion, gate tool versions and date, transcribed from `gh pr checks` and run metadata | No tokens, log bodies or environment dumps | Proves the pull-request part of exit item 3 on the verified head after Actions logs expire |
| EV-002 | T6 | `docs/evidence/ms-0-completion-report.md` | The Appendix B.3 completion report (§12 lists its fields), the T1–T5 checkpoint OIDs (T6's added by the harvest commit), the landed PR head OID beside the T5 checkpoint OID, the MS-0 PR number, the squash OID, each `main` run ID with its conclusion, and the admission result, transcribed from `gh run list` and `gh-workflow admission` | No tokens, log bodies or environment dumps | Proves the `main` part of exit item 3 and is the completion report Appendix B.3 requires |

## Appendix D. Deferred Work

| Item | Reason Deferred | Follow-up / Reopen Trigger |
| --- | --- | --- |
| Remove `--no-tests=warn` | No tests exist at MS-0 (D-006) | The first change that adds a test, expected in MS-1 |
| UI-test job (egui_kittest, lavapipe) in `rust.yml` | MS-1 scope | MS-1 plan, through C-3 |
| Lucide icons, `LICENSES/ISC.txt` and their `REUSE.toml` annotation | `reuse lint` rejects an unused license; icons are MS-1 | MS-1 plan, through C-5 |
| Platform edges from scanner (MS-2) and fileops (MS-5) | No adapter contract exists yet (D-002) | The milestone that adds the adapter |
| `tests/`, `benches/`, `packaging/` | Not in the MS-0 exit list | First milestone needing them |
| Required status checks on `main` | Owner repository setting; `main` is unprotected | Owner decision |
| C-7 policy entries and §8.3 rows for MS-1's crates | No third-party crate at MS-0 | MS-1 adds each with its §8.3 row in the same PR |
