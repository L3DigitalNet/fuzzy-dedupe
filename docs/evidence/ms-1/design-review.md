<!--
SPDX-FileCopyrightText: 2026 L3Digital LLC
SPDX-License-Identifier: GPL-3.0-only
-->

# MS-1 design review (EV-002)

## Binding (C-14)

| Field | Value |
| --- | --- |
| `application_commit` | `23b3562c78d691c804574847c17994f6ddd02767` |
| `crates_tree` | `5b0a0a5d418ebf186b55544e844624a1bf75f95c` |
| `cargo_lock` | `5b86f8bade9f841adf89679cac394a98a3245d08` |
| `cargo_toml` | `a94ccf938ee056051158410c860a9cdbea464214` |
| `rust_toolchain` | `e50dfd8fe46b8b05bd33c47b9523b2768270fef7` |
| Build profile | `dev` (unoptimized with debug info) |
| Provenance | Built and run locally on the reference workstation under the plan §3 reference-workstation exception; `rustc 1.96.0 (ac68faa20 2026-05-25)`, `cargo 1.96.0 (30a34c682 2026-05-25)` |

The four object IDs are `git rev-parse "$C":crates`, `"$C":Cargo.lock`, `"$C":Cargo.toml` and `"$C":rust-toolchain.toml` with `C` set to `application_commit`.

- The captures were rendered from T43's checkpoint `84f10e3c149f19ed41abb5190c8874531c631531`, whose tree equals `application_commit`'s.
- The commit that carries this file follows `application_commit` only by changes under `docs/evidence/`.

## What this proves

- **Exit item 2:** the design review is recorded.
- **PV-T24-001 (REQ-002):** each of the 19 review frames has one entry below, with its reference PNG, the captures of the running application on the `design` scenario, the findings and a result. Every result is a pass.
- **PV-T24-002 (REQ-012):**
  - the light and dark captures match the frames' compact sizing and typography;
  - the 13 dark captures without a dark reference use only §11.7 dark tokens;
  - the 16 captures at 200% zoom show no clipped text, no overlapping controls and nothing off-canvas.

## Method

- **References:** the 19 PNGs in [`docs/design/reference/`](../../design/reference/). Their SHA-256 checksums were verified against that directory's `README.md` before each review round.
- **Captures:** 48 renders of the whole application (`FuzzyDedupeApp::new`) on the `design` scenario, listed with checksums, run conditions and geometry files in [`design-review/manifest.md`](design-review/manifest.md).
  - The 16 non-dark review states, each in the light and dark themes at 100%, and in the light theme at 200%.
  - Rendered off-screen on the reference workstation's integrated Intel Arc GPU (Vulkan, Mesa 26.2.7) under KDE Plasma 6.8.3 on Wayland, with the production system-font lookup.
  - The frame states are those of `crates/fuzzy-dedupe-app/tests/snapshots.rs`.
  - Two full runs were byte-identical.
- **Review:** an independent reviewer compared each reference with its capture side by side, and measured sizes, insets and colours from the pixels and from the accessibility geometry beside each capture. The orchestrator spot-checked each correction. Round 5 was a delta review: it proved that only the captures T43 affects had changed.
- **Classification:** each difference was classed as a defect, an incidental difference or an owner question, under the spec's precedence (§11.1): owner decisions and safety, then the §11 compact-UI contract, then the frames. Geometry is a reference target, not a reason to break text or scaling, and no business rule is inferred from sample data or decoration.
- **Defects:** every defect went to a correction task, and the affected frames were captured and reviewed again.

## Review history

| Round | Application | Result | Correction |
| --- | --- | --- | --- |
| 1 | After T22 and T38 | Deviations in most entries; collapse step 3 missing; capture-rig flaws (an 8-unit margin, the New scan and Scanning states) | T40 |
| 2 | After T40 | Table scrollbar not drawn; stepper hit targets about 4 wide; chips at 512 points; Group elided at 200%; trailing "/" dropped; pinned time hid the progress segment | T41 |
| 3 | After T41 | All T41 items fixed; 200% clean. Queued rows not tinted; Match mode control 22 high; "· protected" elided at 200% | T42 |
| 4 | After T42 | All T42 items fixed; no regression; 200% and dark clean. Unique line counts drawn muted where the frames draw amber | T43 |
| 5 | After T43 (this record) | Only the six captures showing a Unique count changed; the counts are amber and "—" muted; every other capture is byte-identical to round 4 and keeps its verdict. Every entry passes | — |

## Rulings behind the incidental differences

These differences are not defects. Each follows a ruling recorded during MS-1 execution, or the spec's precedence.

- **Rulings already in force before this review (T37, T22, T40):**
  - **Group order:** the frames show a group order that the application's grouping does not produce. The captures follow the application's order and scroll to the focused row, so some frames' top groups are out of view.
  - **Table and empty states:** the grouped table keeps a 24-unit bottom margin, while empty and scanning states fill to the 12-unit inset.
  - **Compare change row:** the row is 28 units, with its 30-high Previous and Next controls overhanging by 1, as the frame draws them.
  - **Columns at 1024 × 600:** only File name and Folder flex (§11.4). Frame 29:2214 also narrows Match, Unique and Decision, a frame-over-spec difference.
  - **512 points (1024 × 600 at 200%):** the Findings table scrolls sideways (line 869 forbids clipping, not scrolling).
  - **Second shrink stage at 800 points:** follows the frame precedent.
  - **Review cleanup:** the per-row × is not drawn (the spec governs). Its popup is 960 × 423 in every state, the single size §11.6 gives, where the running and finished frames draw 441 and 405.
  - **Workspace label:** the "Review cleanup (N)" label shortens only when the workspace needs the room (Q6), so Compare at 1024 keeps the full label.
  - **Search glyph:** Compare is disabled without a pair (§11.2, Q8), and a clear (×) glyph replaces the search glyph when there is text.
  - **Before the first scan:** chips and the caption are muted, and Menu stays enabled (Q7).
- **Rulings made during this review (T24 to T42):**
  - **Compact frame differences:** chip padding differs between frames 13:648 and 30:2370 (Q1); the cleanup finished state is synthesised by the rig (Q2); Compare opens on the first change (Q4); highlight granularity.
  - **Scan progress stays indeterminate:** the engine reports no total, and frame 28:2171's "of about 12,600 files" is sample data.
  - **Chip padding:** chips keep the frame's 6-unit padding where the row fits, and compress to 4 where it does not, so they stay inside the inset at 512 points.
  - **The "· protected" marker is never elided** (safety precedence). The folder shortens first.
  - **Keyboard shortcuts rows** follow the §11.10 shortcut map, not the frame's list (§11.6).
  - **Popup sizes** follow §11.6 where a frame is a few units off, for example About at 387 against the frame's 383.
  - **Sample data:** status-bar wording and counts are sample data.

## Entries

Each entry pairs the reference frame with the capture in the frame's theme at 100%. The light and dark captures of every state, and the 200% captures, are listed in the manifest.

| Entry | Frame | Reference | Capture | Result | Findings |
| --- | --- | --- | --- | --- | --- |
| 01 Findings | `13:648` | [13-648-findings.png](../../design/reference/13-648-findings.png) | [13-648-findings-light.png](design-review/13-648-findings-light.png) | Pass with incidentals | Sizes match by pixel: rail 152, toolbar 32, search 340, chips 26, header 30, rows 28, a 24-unit table margin, status bar 26. The table scrollbar shows at rest (T41). Queued rows are tinted `code` (T42). Unique counts are amber and "—" is muted (T43). Incidental: group order and scroll to the focused row; chip padding (Q1); status wording (sample data). |
| 02 Compare | `13:1304` | [13-1304-compare.png](../../design/reference/13-1304-compare.png) | [13-1304-compare-light.png](design-review/13-1304-compare-light.png) | Pass with incidentals | Panes 708 wide with an 8-unit gap; identity header 54; text rows 22; stepper arrows 24 × 30 (T41). Incidental: opens on the first change (Q4); highlight granularity. |
| Matching rules | `16:735` | [16-735-matching-rules.png](../../design/reference/16-735-matching-rules.png) | [16-735-matching-rules-light.png](design-review/16-735-matching-rules-light.png) | Pass with incidentals | Popup 420 × 421; inset 16; Match mode control 30 high with a centred label (T42); similarity field 95. Incidental: two match modes, not the frame's four (the spec governs); content follows the 16-unit inset. |
| New scan | `16:806` | [16-806-new-scan.png](../../design/reference/16-806-new-scan.png) | [16-806-new-scan-light.png](design-review/16-806-new-scan-light.png) | Pass with incidentals | Popup 640 × 528; folder rows 36; + Add folder 112 × 30. Incidental: the folders are the capture rig's; the file-type list differs (sample data, and an empty list means all plain-text files, spec 0.13). |
| Review unique text | `16:932` | [16-932-review-unique-text.png](../../design/reference/16-932-review-unique-text.png) | [16-932-review-unique-text-light.png](design-review/16-932-review-unique-text-light.png) | Pass with incidentals | Popup 600 × 422; folders keep their trailing "/" (T41). Incidental: content follows the 16-unit inset. |
| Review cleanup | `16:886` | [16-886-review-cleanup.png](../../design/reference/16-886-review-cleanup.png) | [16-886-review-cleanup-light.png](design-review/16-886-review-cleanup-light.png) | Pass with incidentals | Popup 960 × 423; a 28-unit header band; rows 51; "· protected" never elided (T42). Incidental: no per-row × (the spec governs). |
| Cleanup running | `27:1390` | [27-1390-cleanup-running.png](../../design/reference/27-1390-cleanup-running.png) | [27-1390-cleanup-running-light.png](design-review/27-1390-cleanup-running-light.png) | Pass with incidentals | As Review cleanup; Stop reachable. Incidental: 423 high against the frame's 441 (§11.6). |
| Cleanup finished | `27:1533` | [27-1533-cleanup-finished.png](../../design/reference/27-1533-cleanup-finished.png) | [27-1533-cleanup-finished-light.png](design-review/27-1533-cleanup-finished-light.png) | Pass with incidentals | As Review cleanup. Incidental: 423 against 405 (§11.6); the finished state is synthesised by the rig (Q2). |
| 01a Before first scan | `28:1399` | [28-1399-findings-before-first-scan.png](../../design/reference/28-1399-findings-before-first-scan.png) | [28-1399-findings-before-first-scan-light.png](design-review/28-1399-findings-before-first-scan-light.png) | Pass with incidentals | Header row present; the empty state fills to the 12 inset (T37); rail Compare disabled. Incidental: chips and caption muted, Menu enabled (Q7); disabled ink tint (§11.7 has no disabled token). |
| 01b Scanning | `28:2171` | [28-2171-findings-scanning.png](../../design/reference/28-2171-findings-scanning.png) | [28-2171-findings-scanning-light.png](design-review/28-2171-findings-scanning-light.png) | Pass with incidentals | Track 360 × 6 with the segment mid-track (T41); folder keeps its trailing "/". Incidental: progress stays indeterminate (the total is sample data); status wording. |
| 01e No search matches | `30:2370` | [30-2370-findings-no-search-matches.png](../../design/reference/30-2370-findings-no-search-matches.png) | [30-2370-findings-no-search-matches-light.png](design-review/30-2370-findings-no-search-matches-light.png) | Pass with incidentals | "3 queued, 3 outside the search". Incidental: Compare disabled without a pair (Q8); × clear glyph replaces the search glyph. |
| 01d Findings 1024 × 600 | `29:2214` | [29-2214-findings-1024x600.png](../../design/reference/29-2214-findings-1024x600.png) | [29-2214-findings-1024x600-light.png](design-review/29-2214-findings-1024x600-light.png) | Pass with incidentals | Rail 48; Size, Modified and Protected hidden; queued rows tinted (T42); scrollbar at rest (T41). Only "—" is in view in the Unique column; the counted rows are scrolled out of view, and T43's test covers their colour. Incidental: Match, Unique and Decision narrower in the frame (§11.4: only File name and Folder flex); caption and full label shown (Q6); Compare disabled (Q8). |
| 02b Compare 1024 × 600 | `29:3648` | [29-3648-compare-1024x600.png](../../design/reference/29-3648-compare-1024x600.png) | [29-3648-compare-1024x600-light.png](design-review/29-3648-compare-1024x600-light.png) | Pass with incidentals | Panes 472; word wrap (T40). Incidental: "Review cleanup (3)" keeps its full label because it fits (Q6). |
| 01 Findings, dark | `37:2466` | [37-2466-findings-dark.png](../../design/reference/37-2466-findings-dark.png) | [13-648-findings-dark.png](design-review/13-648-findings-dark.png) | Pass with incidentals | Dark tokens match. Unique counts are amber dark and "—" muted (T43). Otherwise as 01 Findings. |
| 02 Compare, dark | `37:2548` | [37-2548-compare-dark.png](../../design/reference/37-2548-compare-dark.png) | [13-1304-compare-dark.png](design-review/13-1304-compare-dark.png) | Pass with incidentals | Dark tokens match. As 02 Compare. |
| Review cleanup, dark | `37:2732` | [37-2732-review-cleanup-dark.png](../../design/reference/37-2732-review-cleanup-dark.png) | [16-886-review-cleanup-dark.png](design-review/16-886-review-cleanup-dark.png) | Pass with incidentals | Dark tokens match. As Review cleanup. |
| 01f Main menu | `44:3954` | [44-3954-findings-main-menu-open.png](../../design/reference/44-3954-findings-main-menu-open.png) | [44-3954-findings-main-menu-open-light.png](design-review/44-3954-findings-main-menu-open-light.png) | Pass | Menu item pitch 28; Appearance, Zoom, "Keyboard shortcuts…", "Open log folder", About, Quit Ctrl+Q. Unique counts are amber and "—" is muted (T43); the menu sits over the table. |
| Keyboard shortcuts | `44:3681` | [44-3681-keyboard-shortcuts.png](../../design/reference/44-3681-keyboard-shortcuts.png) | [44-3681-keyboard-shortcuts-light.png](design-review/44-3681-keyboard-shortcuts-light.png) | Pass with incidentals | Popup 960 × 494; one cap per chord; the F1 row. Incidental: rows follow the §11.10 map (§11.6). |
| About | `44:3858` | [44-3858-about.png](../../design/reference/44-3858-about.png) | [44-3858-about-light.png](design-review/44-3858-about-light.png) | Pass with incidentals | Incidental: 480 × 387 (§11.6) against the frame's 383. |

## Dark theme without a reference

Thirteen dark captures have no dark frame: Matching rules, New scan, Review unique text, both cleanup progress states, Before first scan, Scanning, No search matches, both 1024 × 600 frames, the main menu, Keyboard shortcuts and About. Each was swept for light-theme colours.

- **What was found:** their dominant colours are all §11.7 dark tokens: canvas, surface, rail, code, line, primarySoft, ink, primary, muted, amberSoft, redSoft, diffA and diffB. Popups dim the backdrop. Amber, red and green appear only where the state calls for them.
- **What was not found:** no light-theme token (#FFFFFF, #F4F6F9, #EBEFF5, #FAFBFD, #DDE3EB, #325CC9, #AA353B, #236C54, #8B5614, #FCECEE, #FFF2DC, #E9EFFD) appears within ±2. The only exceptions are scattered anti-aliased glyph pixels.

## 200% zoom (UI check 03)

Each capture keeps the window's pixel size at 2 pixels per point: 800 × 480 points, or 512 × 300 for the 1024 × 600 frames. All 16 were viewed. None has clipped text, overlapping controls or a control off the canvas. Content that runs past an edge either scrolls inside its own area or, at 512 points, is the Findings table's ruled sideways scroll.

| Capture | Result | Findings |
| --- | --- | --- |
| [13-648-findings-zoom200.png](design-review/13-648-findings-zoom200.png) | No clipping | Rail 48; Size, Modified and Protected hidden; Group labels whole (T41); other cells shorten with an ellipsis (second shrink stage); scrollbar visible. |
| [13-1304-compare-zoom200.png](design-review/13-1304-compare-zoom200.png) | No clipping | The group label and pair leave the toolbar, as frame 29:3648 does; both names stay in the identity headers and status bar; stepper 24 × 30 points. |
| [16-735-matching-rules-zoom200.png](design-review/16-735-matching-rules-zoom200.png) | No clipping | Nothing past the inset; Match mode control 30 high. |
| [16-806-new-scan-zoom200.png](design-review/16-806-new-scan-zoom200.png) | No clipping | The body scrolls under the pinned footer (§11.10). |
| [16-886-review-cleanup-zoom200.png](design-review/16-886-review-cleanup-zoom200.png) | No clipping | Cells shorten with an ellipsis; "· protected" stays whole (T42). |
| [16-932-review-unique-text-zoom200.png](design-review/16-932-review-unique-text-zoom200.png) | No clipping | Trailing "/" kept. |
| [27-1390-cleanup-running-zoom200.png](design-review/27-1390-cleanup-running-zoom200.png) | No clipping | "· protected" stays whole; Stop reachable. |
| [27-1533-cleanup-finished-zoom200.png](design-review/27-1533-cleanup-finished-zoom200.png) | No clipping | "· protected" stays whole. |
| [28-1399-findings-before-first-scan-zoom200.png](design-review/28-1399-findings-before-first-scan-zoom200.png) | No clipping | No issues. |
| [28-2171-findings-scanning-zoom200.png](design-review/28-2171-findings-scanning-zoom200.png) | No clipping | Segment mid-track; trailing "/" kept. |
| [29-2214-findings-1024x600-zoom200.png](design-review/29-2214-findings-1024x600-zoom200.png) | No clipping | 512 points: chips inside the inset (T41); Compare and Sort in Actions; the table scrolls sideways (ruled); status bar shortens. |
| [29-3648-compare-1024x600-zoom200.png](design-review/29-3648-compare-1024x600-zoom200.png) | No clipping | Keep both and Queue B in the ⋯ menu; identity rows and status bar shorten. |
| [30-2370-findings-no-search-matches-zoom200.png](design-review/30-2370-findings-no-search-matches-zoom200.png) | No clipping | No issues. |
| [44-3681-keyboard-shortcuts-zoom200.png](design-review/44-3681-keyboard-shortcuts-zoom200.png) | No clipping | The body scrolls; Close stays pinned. |
| [44-3858-about-zoom200.png](design-review/44-3858-about-zoom200.png) | No clipping | No issues. |
| [44-3954-findings-main-menu-open-zoom200.png](design-review/44-3954-findings-main-menu-open-zoom200.png) | No clipping | The menu sits over the table at 200%. |

## Owner-accepted items

None. Every difference is a pass or an incidental difference under a recorded ruling. The owner signs this record at T27.

## Not covered by this record

Still captures cannot show keyboard or pointer behaviour. These items belong to T26's desktop pass (`desktop-pass.md`) and to the automated suites:

- reaching actions moved into the Actions, More actions and ⋯ menus by keyboard (PV-T40-001);
- the full-path tooltip on keyboard focus in Compare and Review cleanup;
- keyboard scrolling of the Compare panes, the Keyboard shortcuts body and the Findings table's sideways scroll at 512 points;
- live animation of the indeterminate scan progress.

The automated geometry overlap check flags three frames in every round (13:648, 16:806, 44:3954). Each flag was confirmed a false positive against the pixels:

- 13:648: a row scrolled under the sticky header keeps its unclipped accessibility rect.
- 16:806: the New scan section toggle contains its own label.
- 44:3954: egui exposes no container node for the menu drawn over the table.
