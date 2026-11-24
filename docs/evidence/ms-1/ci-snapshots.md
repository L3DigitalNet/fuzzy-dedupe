<!--
SPDX-FileCopyrightText: 2026 L3Digital LLC
SPDX-License-Identifier: GPL-3.0-only
-->

# MS-1 CI snapshot evidence (EV-006)

## Binding (C-14)

| Field | Value |
| --- | --- |
| `application_commit` | `eecce5e322332e533ee839429cda9d9159dfbe40` |
| `crates_tree` | `a1f4b02f43c4b75ca02c98e7183ae17cac31c419` |
| `cargo_lock` | `5b86f8bade9f841adf89679cac394a98a3245d08` |
| `cargo_toml` | `a94ccf938ee056051158410c860a9cdbea464214` |
| `rust_toolchain` | `e50dfd8fe46b8b05bd33c47b9523b2768270fef7` |
| Build profile | Cargo `test` profile, `fuzzy-dedupe-app` built with `--features snapshot` |
| Provenance | Built and run by the `ui-test` job of `.github/workflows/rust.yml` on GitHub-hosted `ubuntu-24.04` from a clean checkout of `application_commit` (pull request #52), toolchain from `rust-toolchain.toml` (`rustc 1.96.0`) |

The four object IDs are `git rev-parse "$C":crates`, `"$C":Cargo.lock`, `"$C":Cargo.toml` and `"$C":rust-toolchain.toml` with `C` set to `application_commit`. The commit that carries this file follows `application_commit` only by changes under `docs/evidence/`.

## What this proves

PV-T22-002 (REQ-043, REQ-004): the `ui-test` job renders the 16 review frames in both themes, 32 snapshots, and compares each with a baseline produced by the same runner; it passes at `application_commit`, and a deliberately altered design token fails it. The job's own guards hold: it counts exactly 32 snapshot tests before running them, fails if no test runs (`--no-tests=fail`), runs every frame after a failure (`--no-fail-fast`), and uploads the new and diff images as the `ui-snapshots` artifact when it fails.

## Run conditions

| Item | Value |
| --- | --- |
| Runner label | `ubuntu-24.04` (GitHub-hosted, pinned by name) |
| Runner image | `ubuntu24 20261116.133.1` (passing and negative-control runs); `ubuntu24 20261116.376.1` (baseline run) |
| Mesa | `mesa-vulkan-drivers 25.2.8-0ubuntu0.24.04.5`, `libvulkan1 1.3.275.0-1build1` |
| Adapter | `llvmpipe (LLVM 20.1.2, 256 bits)`, driver `Mesa 25.2.8-0ubuntu0.24.04.5 (LLVM 20.1.2)`, selected with `WGPU_BACKEND=vulkan` |
| Fonts | Embedded fonts only: each harness passes an empty font lookup, so no system font reaches a snapshot (D-007) |
| Scenario | `design`, driven through `FuzzyDedupeApp::new`; pointer removed and `input.time` pinned before capture, so the animated scan progress renders the same every run |

## Runs

| Run | Commit | `ui-test` result |
| --- | --- | --- |
| [45347148247](https://github.com/L3DigitalNet/fuzzy-dedupe/actions/runs/45347148247) (baseline generation, `workflow_dispatch`) | tree equal to `application_commit` without the baseline images | 32 `Missing snapshot` failures as designed, 1 passed; artifact `ui-snapshots` held the 32 new images committed as baselines |
| [45354354872](https://github.com/L3DigitalNet/fuzzy-dedupe/actions/runs/45354354872) (job 138308359823) | `application_commit` | **success**: 33 tests run, 33 passed (32 snapshots and the frame-list test) |
| [45356102840](https://github.com/L3DigitalNet/fuzzy-dedupe/actions/runs/45356102840) (job 138314328622) | negative control `b66a45db9f463b23d472612974262e95798d2e79`, then dropped from the branch | **failure**: 33 run, 17 passed, 16 failed |

The negative control changed one token, the light `line` colour, from `0xDD_E3_EB` to `0xD0_D6_DE` in `crates/fuzzy-dedupe-app/src/ui/theme.rs`. Exactly the 16 light frames failed, and the 16 dark frames and the frame-list test passed, which is the expected footprint of a light-only change. Two examples from the log: `28-1399-findings-before-first-scan-light` differed in 7,798 pixels and `44-3858-about-light` in 46,982, both at every comparison threshold from 0.0 to 3.0.

## Frames

The frames are C-11's non-dark review frames (`docs/design/reference/`); the test `the_frames_are_the_non_dark_review_frames` checks the list against that directory. Each renders in light and dark, so the baselines under `crates/fuzzy-dedupe-app/tests/snapshots/` are `<node>-<name>-light.png` and `-dark.png`.

| Frame     | Name                             | Size       |
| --------- | -------------------------------- | ---------- |
| `13:648`  | Findings                         | 1600 × 960 |
| `13:1304` | Compare                          | 1600 × 960 |
| `16:735`  | Matching rules                   | 1600 × 960 |
| `16:806`  | New scan                         | 1600 × 960 |
| `16:886`  | Review cleanup                   | 1600 × 960 |
| `16:932`  | Review unique text               | 1600 × 960 |
| `27:1390` | Cleanup running                  | 1600 × 960 |
| `27:1533` | Cleanup finished                 | 1600 × 960 |
| `28:1399` | Findings before the first scan   | 1600 × 960 |
| `28:2171` | Findings while scanning          | 1600 × 960 |
| `29:2214` | Findings                         | 1024 × 600 |
| `29:3648` | Compare                          | 1024 × 600 |
| `30:2370` | Findings with no search matches  | 1600 × 960 |
| `44:3681` | Keyboard shortcuts               | 1600 × 960 |
| `44:3858` | About                            | 1600 × 960 |
| `44:3954` | Findings with the main menu open | 1600 × 960 |

## Baseline procedure

Baselines come only from the runner. To create or change them: push the branch, let `ui-test` fail, download the `ui-snapshots` artifact, review each image (and its `.diff.png` for a changed baseline), rename each `<name>.new.png` to `<name>.png` under `crates/fuzzy-dedupe-app/tests/snapshots/`, commit, and rerun until all 32 pass. Never commit `.new.png`, `.diff.png` or `.old.png` files (`.gitignore` excludes them) and never set `UPDATE_SNAPSHOTS`. The same procedure is in the header of `crates/fuzzy-dedupe-app/tests/snapshots.rs`.

## Limitations

- The snapshots are regression baselines for this runner and Mesa build, not a design-conformance check; the frame-by-frame comparison with the reference images is EV-002 (T24).
- The `ubuntu-24.04` label floats across image builds. The baselines made on `20261116.376.1` passed on `20261116.133.1` with the same Mesa. A Mesa or image change that moves pixels is handled by regenerating the baselines in a reviewed pull request, never by loosening the comparison.
- A correction task that changes rendering invalidates these baselines and this record (D-016).
