# MS-0 completion report (EV-002)

GitHub Actions run logs expire, so this file keeps the MS-0 completion report that spec Appendix B.3 requires. It proves the `main` part of exit item 3 (REQ-021) and records REQ-022. EV-001 (`docs/evidence/ms-0-ci-gates.md`) holds the pull-request part.

- Milestone: MS-0, scaffold the governed Cargo workspace
- Pull request: #9, a Standalone PR, squash-landed
- Squash commit on `main`: `0b6dcbc64f1b842b9c222b373e233c50dd583414`, carrying `Workflow-Admission: PR #9`
- Date: 2026-11-22

## Summary of changes and files changed

MS-0 turns the repository into a Cargo workspace of the eight §8.5 crates, with no behavior yet. It adds the pull-request and `main` gates that CI will enforce from now on, the IBM Plex fonts with their license record, and the SPDX and REUSE licensing record. The change is `git diff --stat b0a7516 0b6dcbc`: 36 files changed, 2097 insertions, no deletions.

| Area | Files |
| --- | --- |
| Workspace manifest and toolchain | `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.rexec.toml` |
| Eight crates | `crates/fuzzy-dedupe-{analyzers,app,domain,engine,fileops,platform,scanner,storage}/Cargo.toml`, and one `src/lib.rs` each (`src/main.rs` for the app) |
| Fonts | Four IBM Plex files under `crates/fuzzy-dedupe-app/assets/fonts/` |
| CI | `.github/workflows/rust.yml` (new), `.github/workflows/reuse.yml` (new), `.github/workflows/admission.yml` (two SPDX lines) |
| Boundary check | `.github/scripts/check-boundaries.py`, `.github/boundaries.toml` |
| Dependency policy | `deny.toml` |
| Licensing record | `REUSE.toml`, `LICENSES/Apache-2.0.txt`, `LICENSES/GPL-3.0-only.txt`, `LICENSES/OFL-1.1.txt`, `THIRD-PARTY-NOTICES.md` |
| Evidence | `docs/evidence/ms-0-ci-gates.md` |

## Requirements implemented

The Source column is the spec ID or section the plan maps each requirement to (plan §6). The task receipts named in the Evidence column were ephemeral execution logs under `.project-pipeline/`, which is not committed, so this table summarizes each receipt's observed result. The durable per-task record is each task's checkpoint commit, which carries its `Plan-*` trailers, on the retained branch `feat/ms-0-scaffold-workspace`; the OIDs are listed under "Checkpoint OIDs". EV-001 is `docs/evidence/ms-0-ci-gates.md`.

| Requirement | Source | Proof | Evidence |
| --- | --- | --- | --- |
| REQ-001 | §19 item 1 | PV-T1-001 | T1 receipt: build exit 0, eight packages, one bin target `fuzzy-dedupe` |
| REQ-002 | §19 item 2 | PV-T1-002 | T1 receipt: license, edition, `rust_version`, resolver and `[lints]` inspected |
| REQ-003 | §8.5 | PV-T1-003 | T1 receipt: three failing lint canaries and one passing scoped allow, each with its expected result |
| REQ-004 | §8.5 | PV-T1-004 | T1 receipt: edge set equals Appendix A C-1 |
| REQ-005 | §19 item 6, §8.3 | PV-T1-005 | T1 receipt: eight packages, every `source` null |
| REQ-006 | §19 deliverable, §8.3 | PV-T1-006 | T1 receipt, at the T1 checkpoint only |
| REQ-007 | §17.2 | PV-T2-001 | T2 receipt: `cargo fmt --all --check` and clippy exit 0; EV-001 `fmt`, `clippy` |
| REQ-008 | §17.2, §19 item 1 | PV-T2-002 | T2 receipt: build, nextest and doctests exit 0; EV-001 `test` |
| REQ-009 | §17.2 | PV-T2-003 | T2 receipt, with a license canary; EV-001 `deny` |
| REQ-010 | §17.2 | PV-T2-004 | T2 receipt: `cargo audit` exit 0; EV-001 `audit` |
| REQ-011 | §17.2, NFR-008 | PV-T2-005 | T2 receipt, with a Windows canary; EV-001 `portability` |
| REQ-012 | §8.3, §8.5, §17.2, NFR-008, §19 item 6 | PV-T2-006 | T2 receipt: 25 plan canaries fail under their rule IDs; EV-001 `boundaries` |
| REQ-013 | §17.2; request | PV-T2-007 | T2 receipt: triggers, pins and digests inspected |
| REQ-014 | §19 item 5, §11.7 | PV-T3-001 | T3 receipt: zip and file SHA-256 digests match |
| REQ-015 | §19 item 5, §18.3 | PV-T3-002 | T3 receipt: OFL-1.1 text identical to the release; notice entry complete |
| REQ-016 | §19 item 4, §18.3 | PV-T4-001 | T4 receipt: 26 header-bearing files, none missing a line; header-less canary fails `reuse lint` |
| REQ-017 | §19 item 4, §18.3 | PV-T4-002 | T4 receipt: `LICENSE` digest unchanged; `reuse lint` accepts `LICENSES/` |
| REQ-018 | §19 item 4, §17.2, §18.3 | PV-T4-003 | T4 receipt: `reuse lint` exit 0 and ledger check clean; EV-001 `reuse` |
| REQ-019 | §18.3, C-012 | PV-T4-004 | T4 receipt: no `or-later` match in SPDX lines, `REUSE.toml` or Cargo license fields |
| REQ-020 | §19 item 3, pull-request part | PV-T5-001 | EV-001; T5 receipt |
| REQ-021 | §19 item 3, `main` part | PV-T6-001 | The `main` runs below |
| REQ-022 | Appendix B.1, B.3; §17.3 | PV-T6-002 | This report, with the NFR-008 Status cell in `docs/specs/canonical/fuzzy-dedupe.md` |

## Tests added or changed

None. The workspace has no behavior yet, so there is nothing to test (D-006), and nextest runs with `--no-tests=warn` until the first test exists. Gate canaries stand in for tests. Each canary is a deliberate violation that was run as a negative control, shown to fail on its own expected lint, rule ID or check, and then reverted:

- PV-T1-003: three failing canaries (an `unsafe` block under an allow in domain, the same block without an allow in platform, and a pedantic-only violation in engine) and the scoped allow that compiles in platform.
- PV-T2-003: a `GPL-3.0-or-later` license on one crate, which `cargo deny check licenses` rejects.
- PV-T2-005: a `std::os::unix` import in domain, which the Windows check rejects.
- PV-T2-006: 25 plan canaries against the boundary script, each failing under only its expected rule ID, with every positive control passing.
- PV-T4-001 and PV-T4-003: a header-less Rust file that `reuse lint` rejects, and a mislabeled template that only the origin-ledger check catches.

No canary remains in the tree.

## Deviations

The spec has no deviation from MS-0, so there are no `DEV-` rows and the Deviations Log is unchanged. These plan-level decisions were recorded during execution. Each stays within the spec.

| Decision | Authority |
| --- | --- |
| `rust.yml` runs `cargo deny --locked check` instead of C-3's literal `cargo deny check`. `--locked` is a cargo-deny global option, and T2 requires it wherever the tool accepts it. | T2 `--locked` invariant |
| cargo-deny `[bans] wildcards` is left at the 0.22 default (allow). T1's versionless internal path dependencies fail `wildcards = "deny"`, and `allow-wildcard-paths` applies only to `publish = false` crates, which the plan rules out. | T2 executor discretion |
| `.gitignore` is class a (2026 L3Digital LLC, GPL-3.0-only). Lines 1-24 come from github/gitignore's Rust template, which is CC0-1.0 and needs no notice. A `REUSE.toml` comment says so. | Orchestrator, under D-012 |
| `.codex/config.toml` is class c, annotated with no header. It configures the agent harness rather than the build or CI, and holds a managed key set. | Orchestrator, under D-008 |
| `LICENSES/` holds the canonical SPDX texts from `reuse download`, not the upstream license files. This applies to `LICENSES/Apache-2.0.txt`, whose upstream holder line was confirmed at v5.29.0. | T4 ledger preparation |
| T2 ran as two worker legs (gates; boundaries) with one checkpoint, and T3 ran concurrently with them on disjoint claims. Integration was serial, T3 before T2. | Orchestrator; §8 asks only for serial commits |

## Known limitations and open questions

- D-011's boundary scan is lexical, so two cases escape it. A macro that assembles an OS path from tokens is covered by the `portability` job for domain and platform, and by review in the other six crates. An OS item that `fuzzy-dedupe-platform` re-exports or exposes in its public API, then used by a dependent crate such as app, is covered by review of platform's public API.
- The aliased `cfg!` macro (`use core::cfg as c;` then `c!(unix)`) is not caught, because D-011 rule 5a lists only the literal `cfg(`, `cfg!(` and `cfg_attr(`. It is accepted residue. `cfg!` evaluates to a bool and cannot remove code, so both branches still compile on every target, and an OS-API path in either branch still fails rule 5b. A renamed `cfg` used as an attribute fails with "cannot find attribute `c`" (checked with rustc 1.96), so it cannot compile code conditionally.
- `LICENSES/GPL-3.0-only.txt` uses the SPDX canonical layout and is not byte-identical to `LICENSE`. PV-T4-002 requires only that `LICENSE` matches the gnu.org digest and that `reuse lint` accepts the `LICENSES/` text, and both hold.
- The class-a `REUSE.toml` globs may need rechecking after a standards reconcile drops new files into the folders they cover.
- Required status checks on `main` remain an owner repository setting. `main` is unprotected.
- Open `OQ-` items: none for MS-0.

Work deferred to later milestones is listed in plan Appendix D.

## Documentation deliverables

- `THIRD-PARTY-NOTICES.md`, with the IBM Plex entry.
- `docs/evidence/ms-0-ci-gates.md` (EV-001).
- `docs/evidence/ms-0-completion-report.md` (this file, EV-002).

The §18.7 deliverables fall due in later milestones.

## Checkpoint OIDs

All five are on the retained branch `feat/ms-0-scaffold-workspace`. The harvest commit adds T6's.

| Task | Checkpoint                                 |
| ---- | ------------------------------------------ |
| T1   | `29fc803ab90a2bf3deddfca4e2fd4bfc62c5a7f9` |
| T3   | `7287227d89fe25eed55fdcfe7c8a37bb73e5b044` |
| T2   | `ee33e4b34e0e6752518079d231f58f175efa605c` |
| T4   | `d5e15de03a512d1042940ae8bf39f39e12e10936` |
| T5   | `f5f73af749901572b3ef0d217541fbd9eeffcd63` |

T3 is listed before T2 because it was integrated first.

## Landed head beside the T5 checkpoint

| Item                  | OID                                        |
| --------------------- | ------------------------------------------ |
| PR #9 head at landing | `f5f73af749901572b3ef0d217541fbd9eeffcd63` |
| T5 checkpoint         | `f5f73af749901572b3ef0d217541fbd9eeffcd63` |

The two are equal, so no D-013 handoff commits sit between the checkpoint and the head. After `land`, `git diff origin/main f5f73af` was empty.

## `main` runs on the squash commit

Source: `gh run list --repo L3DigitalNet/fuzzy-dedupe --commit 0b6dcbc64f1b842b9c222b373e233c50dd583414 --json databaseId,workflowName,conclusion,headSha`. Every `headSha` equals the squash OID, and each run was confirmed with `gh run watch --exit-status`, which exited 0.

| Workflow                  | Run ID      | Conclusion |
| ------------------------- | ----------- | ---------- |
| Rust                      | 45066522708 | success    |
| REUSE                     | 45066522715 | success    |
| Format                    | 45066522955 | success    |
| Lint Markdown             | 45066522932 | success    |
| Validate project specs    | 45066522936 | success    |
| GitHub Workflow admission | 45066522706 | success    |

## Admission result

`gh-workflow admission --branch main` reads the local `main` ref. Run with local `main` at `b419d7d48bce4f34e10d5382f6bc9105fc22d629`, after the close-out PR, it exited 0 and reported 14 commits since `e8b32886101d14fd7da344e2a7fc5862d04c1255`: 10 pull request, 4 handoff and 0 unadmitted. That range includes this squash, `0b6dcbc`. The `admission` workflow run on `0b6dcbc` itself, 45066522706, also concluded `success`.

Correction, 2026-11-22: this paragraph first quoted a run made while local `main` was still at `b0a75162182c408ee2897e8f27e90f8f85b51f62`. That run reported 12 commits (8 pull request, 4 handoff, 0 unadmitted), but its range ended before the squash, so it did not prove the squash was admitted.
