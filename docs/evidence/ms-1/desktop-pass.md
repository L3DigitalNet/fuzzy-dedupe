<!--
SPDX-FileCopyrightText: 2026 L3Digital LLC
SPDX-License-Identifier: GPL-3.0-only
-->

# MS-1 desktop pass evidence (EV-004)

This is the fill-in record for plan task T26: a keyboard-only manual pass on GNOME and on KDE Plasma, both on Wayland. Nothing below is filled yet. Work through it in the order of the plan's T26 sub-tasks: bind the build, record both environments, run the GNOME table, run the KDE Plasma table, triage, and close.

## Binding (C-14)

Fill this block first, before any row. Every result in this file belongs to the build named here; if a correction changes it, see [Reruns](#reruns).

| Field                | Value     |
| -------------------- | --------- |
| `application_commit` | _to fill_ |
| `crates_tree`        | _to fill_ |
| `cargo_lock`         | _to fill_ |
| `cargo_toml`         | _to fill_ |
| `rust_toolchain`     | _to fill_ |
| Build profile        | _to fill_ |
| Provenance           | _to fill_ |

Set `C` to `application_commit`, then:

```bash
git rev-parse "$C" "$C":crates "$C":Cargo.lock "$C":Cargo.toml "$C":rust-toolchain.toml
```

The five lines print `application_commit`, `crates_tree`, `cargo_lock`, `cargo_toml` and `rust_toolchain`, in that order. Provenance says where and how the binary was built, from a clean checkout of `application_commit` (`git status --porcelain` prints nothing), with the output of `rustc --version` and `cargo --version`. The commit that carries this file follows `application_commit` only by changes under `docs/evidence/`.

## What this proves

Exit item 6 in §19 of the spec (REQ-006, PV-T26-001): on GNOME and on KDE Plasma (Wayland) every item of the plan's C-13 checklist holds, with the environment versions recorded. The pass covers display scales of 100, 125, 150 and 200%, the 1024 × 600 minimum window, in-app zoom of 80% and 200%, moving between monitors of different scale, every chord in §11.10, focus restoration after every popup, Japanese, Arabic, emoji and long-path names, the portal folder picker, side labels in grayscale, and, on KDE Plasma, conflicts with Plasma's default shortcuts.

PV-T26-002 (REQ-018, UI check 13, NFR-005, C-016): every action was reached by keyboard with visible focus, focus restoration held after each popup, and long and Unicode paths stayed fully accessible through details and copy.

A row passes only when you saw it hold in the build named above. Write `pass`, `fail`, or `n/a` followed by the reason. Two more results exist: `blocked` for a row the app does not let you reach (for example, a launch route that does not work), and `deviation` for a row you did differently on the virtual machine. A row you could not exercise as written is not `n/a`: write what you did instead in its notes, and for the GNOME virtual machine use [the deviations table](#gnome-rows-the-virtual-machine-cannot-exercise-faithfully). A missing environment blocks T26; it does not shrink the checklist.

## Run conditions

- Date: _to fill_.
- Performed by: _to fill_ (owner, or owner-supervised).
- Keyboard layout in use: _to fill_. The chord `Ctrl+?` depends on it.
- Input: keyboard only for every row. Use the pointer only to set up the environment (display scale, monitor arrangement, window size) and to read the screen.

### Environments

Record the same fields for both. The commands are suggestions; use whatever shows the value.

| Field | GNOME | KDE Plasma |
| --- | --- | --- |
| Machine | Fedora Workstation virtual machine supplied by the owner | The reference workstation |
| OS and release | _to fill_ (`cat /etc/os-release`) | _to fill_ (`cat /etc/os-release`) |
| Desktop and version | _to fill_ (`gnome-shell --version`) | _to fill_ (`plasmashell --version`) |
| Session type | _to fill_ (`echo $XDG_SESSION_TYPE`; must be `wayland`) | _to fill_ (`echo $XDG_SESSION_TYPE`; must be `wayland`) |
| Portal packages | _to fill_ (`rpm -q xdg-desktop-portal xdg-desktop-portal-gnome`) | _to fill_ (`rpm -q xdg-desktop-portal xdg-desktop-portal-kde`) |
| Monitors | _to fill_: how many, and their sizes | _to fill_ (`kscreen-doctor -o`): two monitors at different scales |
| Scales available | _to fill_: which of 100, 125, 150 and 200% the desktop offered | _to fill_: which of 100, 125, 150 and 200% the desktop offered |
| How the build was obtained | _to fill_: built in the VM, or copied from where it was built | _to fill_: built on the workstation, or pulled from where it was built |
| Binary SHA-256 | _to fill_ (`sha256sum` of the binary) | _to fill_ (`sha256sum` of the binary) |

If the two binaries have different SHA-256 values, say why in the notes of the Binding block. Both must come from `application_commit`.

## Launching the app for the pass

Build the release binary from a clean checkout of `application_commit`, then run it:

```bash
cargo build --locked --release -p fuzzy-dedupe-app
target/release/fuzzy-dedupe
```

The binary is `fuzzy-dedupe` (`crates/fuzzy-dedupe-app/Cargo.toml`). Record in Provenance if you use a different profile.

**Prerequisite: T39 and T40 on `main`.** Choosing a scenario at launch comes from correction task T39, and T40 makes the views conform to the review frames at every size and zoom, including keyboard access to full paths. `application_commit` must contain both, or the scenario rows below cannot be performed and the layout rows test superseded behaviour.

**Choosing the scenario.** Set `FUZZY_DEDUPE_SCENARIO` to a scenario name on a normal launch. The app starts on that fixture scenario, and New scan replays it:

```bash
FUZZY_DEDUPE_SCENARIO=unicode target/release/fuzzy-dedupe
```

- Names: `large`, `mixed`, `issues`, `empty`, `unicode`, `stale`, `design`.
- Unset: the app starts on `design`.
- An unknown name prints one line to standard error listing the valid names, and the app starts on `design`.
- `FUZZY_DEDUPE_MEASURE` wins if both are set. Leave it unset for this pass: measurement mode scripts its own input and quits.
- Preferences behave normally, so zoom and window size carry over between launches. Scans are simulated (C-03), so adding real folders does not put their names in Findings. The portal folder picker shows your real folders.

Which scenario each part of the pass uses:

| Scenario | Use it for |
| --- | --- |
| `design` | The default. Scales, zoom, the chords, focus restoration, keyboard reach and grayscale, unless a row names another scenario. |
| `unicode` | UN01 to UN04 and LP01 to LP04: Japanese, Arabic, emoji and decomposed-accent names, a long path and mixed line endings. |
| `large` | WN01 and ZM200 repeated with a long table, and KB01 on a table that scrolls. |
| `issues` | KB01's discovery-problem details link. |
| `empty` | ES01, the scan that found no relationship. |
| `stale` | FR05 repeated, to see Review cleanup with stale evidence. |
| `mixed` | Not required by a row. Use it if a row fails on `design` and you want a smaller case. |

## What each row asks

Each row is one thing to check. Every row is keyboard-only. When a row fails, write what you saw, then add it to [Triage](#triage). The chord rows (CH01 to CH23) follow the §11.10 keyboard map row by row; where a row names several chords, press each one and name any that fail. Row IDs are the same on both desktops.

"Visible focus" means you can tell which control has keyboard focus at all times. At each scale and zoom, check that nothing is clipped or overlapped and that every action stays reachable.

## GNOME checklist

GNOME on Wayland, in the Fedora Workstation virtual machine. Rows that the virtual machine cannot show faithfully go in the deviations table below, not in a quiet `pass`.

| ID | Check | Result | Notes |
| --- | --- | --- | --- |
| SC100 | Display scale 100%: Findings, Compare and one popup are readable, nothing is clipped or overlapped, and every action is reachable with visible focus. |  |  |
| SC125 | Display scale 125%: Findings, Compare and one popup are readable, nothing is clipped or overlapped, and every action is reachable with visible focus. |  |  |
| SC150 | Display scale 150%: Findings, Compare and one popup are readable, nothing is clipped or overlapped, and every action is reachable with visible focus. |  |  |
| SC200 | Display scale 200%: Findings, Compare and one popup are readable, nothing is clipped or overlapped, and every action is reachable with visible focus. |  |  |
| WN01 | Window at the 1024 × 600 minimum, on `design` and again on `large` (note the scale): Findings, Compare and every popup stay usable, popup bodies scroll, and each popup's action footer stays visible (§11.3 collapse order, §11.10). |  |  |
| ZM80 | In-app zoom 80% (`Ctrl+-` to the floor): rows and controls shrink, text stays readable, nothing is clipped. |  |  |
| ZM200 | In-app zoom 200% (`Ctrl+=` to the ceiling), on `design` and again on `large`: rows and controls grow, popups fit the window with body scrolling and a visible footer, and every action stays reachable. |  |  |
| ZM00 | `Ctrl+0` resets zoom, and the zoom level you set before closing the app is the one it opens with next time. |  |  |
| MM01 | Move the window between two monitors of different scale, in both directions, with Findings showing and again with a popup open: the window re-renders crisply at the new scale, text and controls keep their proportions, and focus is not lost. |  |  |
| CH01 | Global, `Ctrl+N`: New scan. |  |  |
| CH02 | Global, `Ctrl+1` / `Ctrl+2`: Findings / Compare. |  |  |
| CH03 | Global, `Ctrl+,`: Matching rules. |  |  |
| CH04 | Global, `Ctrl+Enter`: Open Review cleanup. |  |  |
| CH05 | Global, `Ctrl+=` / `Ctrl+-` / `Ctrl+0`: Zoom in / zoom out / reset zoom. |  |  |
| CH06 | Global, `F10`: Main menu. |  |  |
| CH07 | Global, `Ctrl+Q`: Quit. |  |  |
| CH08 | Findings, `Ctrl+F`: Search; `Esc` clears it. |  |  |
| CH09 | Findings, Arrow keys, `Page Up`/`Page Down`, `Home`/`End`: Move row focus. |  |  |
| CH10 | Findings, `Right` / `Left`: Expand / collapse a group. |  |  |
| CH11 | Findings, `Enter`: Compare the row against its counterpart. |  |  |
| CH12 | Findings, `Space`: Queue or unqueue; opens Review unique text when review is needed. |  |  |
| CH13 | Findings, Menu key or `Shift+F10`: Row menu. |  |  |
| CH14 | Findings, `Ctrl+Shift+C`: Copy path. |  |  |
| CH15 | Compare, `Alt+Down` / `Alt+Up`: Next / previous change. |  |  |
| CH16 | Compare, `Alt+Right` / `Alt+Left`: Next / previous candidate. |  |  |
| CH17 | Compare, `F6`: Switch pane. |  |  |
| CH18 | Compare, `Alt+Z` / `Alt+L`: Toggle wrap / toggle linked scroll. |  |  |
| CH19 | Compare, `Ctrl+A` / `Ctrl+C`: Select the whole file in the pane / copy. |  |  |
| CH20 | Compare, `Esc`: Back to Findings. |  |  |
| CH21 | Popups, `Tab` / `Shift+Tab`: Move focus. |  |  |
| CH22 | Popups, `Esc`: Cancel. |  |  |
| CH23 | Popups, `Enter`: Default action, never destructive. |  |  |
| CH24 | `F1` and `Ctrl+?` each open the Keyboard shortcuts window, and it lists the map above. |  |  |
| CH25 | Tooltips show each control's shortcut (hover or focus a control that has one). |  |  |
| CH26 | Modal shortcut blocking: with each popup open, the global and workspace chords do not act on the workspace behind it. Name any chord that does. |  |  |
| FR01 | New scan (open with `Ctrl+N`): close with `Esc` and again with the popup's own button. Focus returns to the control or row that had it before, and no command reaches the table behind. |  |  |
| FR02 | Matching rules (open with `Ctrl+,`): close with `Esc` and again with the popup's own button. Focus returns to the control or row that had it before, and no command reaches the table behind. |  |  |
| FR03 | Review unique text (open with `Space` on a row that needs review, or Review unique text… in the row menu (Menu key or `Shift+F10`)): close with `Esc` and again with the popup's own button. Focus returns to the control or row that had it before, and no command reaches the table behind. |  |  |
| FR04 | Queue identical copies (open with `Tab` to Actions ▾ in the Findings toolbar, `Enter`, then the first item, Queue identical copies…): close with `Esc` and again with the popup's own button. Focus returns to the control or row that had it before, and no command reaches the table behind. |  |  |
| FR05 | Review cleanup, on `design` and again on `stale` (open with `Ctrl+Enter`): close with `Esc` and again with the popup's own button. Focus returns to the control or row that had it before, and no command reaches the table behind. |  |  |
| FR06 | Keyboard shortcuts (open with `F1`): close with `Esc` and again with the popup's own button. Focus returns to the control or row that had it before, and no command reaches the table behind. |  |  |
| FR07 | About (open with `F10`, then About Fuzzy DeDupe in the main menu): close with `Esc` and again with the popup's own button. Focus returns to the control or row that had it before, and no command reaches the table behind. |  |  |
| UN01 | On `unicode`: Japanese names (`unicode` scenario: `文書/会議/議事録_ミーティング.txt`): every character shows a glyph, no missing-glyph boxes, in Findings and in Compare. |  |  |
| UN02 | On `unicode`: Arabic names (`أرشيف/تقرير_الاجتماع.txt`, `نسخة.txt`): every character shows a glyph, joined and right-to-left, in Findings and in Compare. |  |  |
| UN03 | On `unicode`: emoji names (`📝 notes ✅.txt`): both emoji show a glyph, and the decomposed-accent name (`Cafe` followed by U+0301, then `menu.txt`, with a space between) shows the accent on the `e`. |  |  |
| UN04 | On `unicode`: long path (the seven-folder path starting with `プロジェクト`): the row stays laid out, nothing overflows the window, and the full path is visible somewhere without the pointer. |  |  |
| PK01 | Portal folder picker: in New scan (`Ctrl+N`), `Tab` to + Add folder and press `Enter` or `Space`. The desktop's own picker (titled Add folders to scan) opens, a folder can be chosen with the keyboard alone, it is added to the list, and focus returns to New scan. |  |  |
| GS01 | Compare in grayscale (record how you made it grayscale): added, removed and changed lines are told apart by their side labels, without color. |  |  |
| KB01 | Findings, on `design`: every action is reachable by keyboard and shows visible focus: search (`Ctrl+F`), Actions ▾ and its four items (Queue identical copies…, Expand all groups, Collapse all groups, Choose columns…), the Sort button (labelled Sort: and the current order) and its menu, the relationship filter chips, Compare, Review cleanup, row focus and expansion, the row menu and its six items (Review unique text…, Keep, Make this the reference, Protect this file, Open containing folder, Copy path). Actions ▾ and Sort need a scan with results. On `issues`, the Show details or Hide details link under the chips also takes focus and opens the discovery-problem list. |  |  |
| ES01 | On `empty`: the No relationships found message appears, its New scan button takes keyboard focus with a visible ring, and `Enter` opens New scan. Focus returns to the button when the popup closes. |  |  |
| KB02 | Compare: every action (changes, candidates, pane switch, wrap, linked scroll, select and copy, back) is reachable by keyboard and shows visible focus. |  |  |
| KB03 | Every popup: every control, including its footer actions, is reachable with `Tab` and `Shift+Tab` and shows visible focus. |  |  |
| KB04 | The rail, the main menu and its entries (Appearance, Zoom, Keyboard shortcuts, Open log folder, About, Quit) are reachable by keyboard and show visible focus. |  |  |
| LP01 | On `unicode`, long path, full path without the pointer: the Folder column and Compare's pinned header truncate long paths, and the app has no separate details view that the source shows. Queue the file and open Review cleanup (`Ctrl+Enter`), which lists full paths. Record which surface shows the whole path by keyboard alone. If none does, mark `fail` (§11.10: hover alone is insufficient) and add it to Triage. |  |  |
| LP02 | On `unicode`, long path, copy: `Ctrl+Shift+C` copies the full path; paste it into a text editor and compare it with the details view. |  |  |
| LP03 | On `unicode`, Unicode names, full path without the pointer: the same surfaces as LP01 show every Japanese, Arabic and emoji character of the full path, in order. |  |  |
| LP04 | On `unicode`, Unicode names, copy: `Ctrl+Shift+C` copies each full path with every character intact when pasted into a text editor. |  |  |

### GNOME rows the virtual machine cannot exercise faithfully

A virtual machine may offer one monitor, fewer scales, or a software-drawn display. Where a row above could not be done as written, list it here with exactly what you did instead and your decision. The row stays in the table above, with `deviation` in its result and a pointer to this table. A row with an open disposition keeps T26 blocked.

| Row | What the row asks | What was done instead | Owner disposition |
| --- | ----------------- | --------------------- | ----------------- |
|     |                   |                       |                   |
|     |                   |                       |                   |
|     |                   |                       |                   |

## KDE Plasma checklist

KDE Plasma on Wayland, on the reference workstation with two monitors at different scales. The first part is the same checklist as GNOME. The rows from KX01 on are the Plasma default-shortcut conflict review (KDE only): a conflict goes to the owner, who decides whether to change the §11.10 map in the spec.

Method for rows KX01 on: in Plasma's System Settings, open Keyboard, then Shortcuts, and search each chord. Note the Plasma action, if any, that already owns it. Then press the chord in the app. It conflicts if Plasma acts on it or the app does not receive it.

| ID | Check | Result | Notes |
| --- | --- | --- | --- |
| SC100 | Display scale 100%: Findings, Compare and one popup are readable, nothing is clipped or overlapped, and every action is reachable with visible focus. |  |  |
| SC125 | Display scale 125%: Findings, Compare and one popup are readable, nothing is clipped or overlapped, and every action is reachable with visible focus. |  |  |
| SC150 | Display scale 150%: Findings, Compare and one popup are readable, nothing is clipped or overlapped, and every action is reachable with visible focus. |  |  |
| SC200 | Display scale 200%: Findings, Compare and one popup are readable, nothing is clipped or overlapped, and every action is reachable with visible focus. |  |  |
| WN01 | Window at the 1024 × 600 minimum, on `design` and again on `large` (note the scale): Findings, Compare and every popup stay usable, popup bodies scroll, and each popup's action footer stays visible (§11.3 collapse order, §11.10). |  |  |
| ZM80 | In-app zoom 80% (`Ctrl+-` to the floor): rows and controls shrink, text stays readable, nothing is clipped. |  |  |
| ZM200 | In-app zoom 200% (`Ctrl+=` to the ceiling), on `design` and again on `large`: rows and controls grow, popups fit the window with body scrolling and a visible footer, and every action stays reachable. |  |  |
| ZM00 | `Ctrl+0` resets zoom, and the zoom level you set before closing the app is the one it opens with next time. |  |  |
| MM01 | Move the window between two monitors of different scale, in both directions, with Findings showing and again with a popup open: the window re-renders crisply at the new scale, text and controls keep their proportions, and focus is not lost. |  |  |
| CH01 | Global, `Ctrl+N`: New scan. |  |  |
| CH02 | Global, `Ctrl+1` / `Ctrl+2`: Findings / Compare. |  |  |
| CH03 | Global, `Ctrl+,`: Matching rules. |  |  |
| CH04 | Global, `Ctrl+Enter`: Open Review cleanup. |  |  |
| CH05 | Global, `Ctrl+=` / `Ctrl+-` / `Ctrl+0`: Zoom in / zoom out / reset zoom. |  |  |
| CH06 | Global, `F10`: Main menu. |  |  |
| CH07 | Global, `Ctrl+Q`: Quit. |  |  |
| CH08 | Findings, `Ctrl+F`: Search; `Esc` clears it. |  |  |
| CH09 | Findings, Arrow keys, `Page Up`/`Page Down`, `Home`/`End`: Move row focus. |  |  |
| CH10 | Findings, `Right` / `Left`: Expand / collapse a group. |  |  |
| CH11 | Findings, `Enter`: Compare the row against its counterpart. |  |  |
| CH12 | Findings, `Space`: Queue or unqueue; opens Review unique text when review is needed. |  |  |
| CH13 | Findings, Menu key or `Shift+F10`: Row menu. |  |  |
| CH14 | Findings, `Ctrl+Shift+C`: Copy path. |  |  |
| CH15 | Compare, `Alt+Down` / `Alt+Up`: Next / previous change. |  |  |
| CH16 | Compare, `Alt+Right` / `Alt+Left`: Next / previous candidate. |  |  |
| CH17 | Compare, `F6`: Switch pane. |  |  |
| CH18 | Compare, `Alt+Z` / `Alt+L`: Toggle wrap / toggle linked scroll. |  |  |
| CH19 | Compare, `Ctrl+A` / `Ctrl+C`: Select the whole file in the pane / copy. |  |  |
| CH20 | Compare, `Esc`: Back to Findings. |  |  |
| CH21 | Popups, `Tab` / `Shift+Tab`: Move focus. |  |  |
| CH22 | Popups, `Esc`: Cancel. |  |  |
| CH23 | Popups, `Enter`: Default action, never destructive. |  |  |
| CH24 | `F1` and `Ctrl+?` each open the Keyboard shortcuts window, and it lists the map above. |  |  |
| CH25 | Tooltips show each control's shortcut (hover or focus a control that has one). |  |  |
| CH26 | Modal shortcut blocking: with each popup open, the global and workspace chords do not act on the workspace behind it. Name any chord that does. |  |  |
| FR01 | New scan (open with `Ctrl+N`): close with `Esc` and again with the popup's own button. Focus returns to the control or row that had it before, and no command reaches the table behind. |  |  |
| FR02 | Matching rules (open with `Ctrl+,`): close with `Esc` and again with the popup's own button. Focus returns to the control or row that had it before, and no command reaches the table behind. |  |  |
| FR03 | Review unique text (open with `Space` on a row that needs review, or Review unique text… in the row menu (Menu key or `Shift+F10`)): close with `Esc` and again with the popup's own button. Focus returns to the control or row that had it before, and no command reaches the table behind. |  |  |
| FR04 | Queue identical copies (open with `Tab` to Actions ▾ in the Findings toolbar, `Enter`, then the first item, Queue identical copies…): close with `Esc` and again with the popup's own button. Focus returns to the control or row that had it before, and no command reaches the table behind. |  |  |
| FR05 | Review cleanup, on `design` and again on `stale` (open with `Ctrl+Enter`): close with `Esc` and again with the popup's own button. Focus returns to the control or row that had it before, and no command reaches the table behind. |  |  |
| FR06 | Keyboard shortcuts (open with `F1`): close with `Esc` and again with the popup's own button. Focus returns to the control or row that had it before, and no command reaches the table behind. |  |  |
| FR07 | About (open with `F10`, then About Fuzzy DeDupe in the main menu): close with `Esc` and again with the popup's own button. Focus returns to the control or row that had it before, and no command reaches the table behind. |  |  |
| UN01 | On `unicode`: Japanese names (`unicode` scenario: `文書/会議/議事録_ミーティング.txt`): every character shows a glyph, no missing-glyph boxes, in Findings and in Compare. |  |  |
| UN02 | On `unicode`: Arabic names (`أرشيف/تقرير_الاجتماع.txt`, `نسخة.txt`): every character shows a glyph, joined and right-to-left, in Findings and in Compare. |  |  |
| UN03 | On `unicode`: emoji names (`📝 notes ✅.txt`): both emoji show a glyph, and the decomposed-accent name (`Cafe` followed by U+0301, then `menu.txt`, with a space between) shows the accent on the `e`. |  |  |
| UN04 | On `unicode`: long path (the seven-folder path starting with `プロジェクト`): the row stays laid out, nothing overflows the window, and the full path is visible somewhere without the pointer. |  |  |
| PK01 | Portal folder picker: in New scan (`Ctrl+N`), `Tab` to + Add folder and press `Enter` or `Space`. The desktop's own picker (titled Add folders to scan) opens, a folder can be chosen with the keyboard alone, it is added to the list, and focus returns to New scan. |  |  |
| GS01 | Compare in grayscale (record how you made it grayscale): added, removed and changed lines are told apart by their side labels, without color. |  |  |
| KB01 | Findings, on `design`: every action is reachable by keyboard and shows visible focus: search (`Ctrl+F`), Actions ▾ and its four items (Queue identical copies…, Expand all groups, Collapse all groups, Choose columns…), the Sort button (labelled Sort: and the current order) and its menu, the relationship filter chips, Compare, Review cleanup, row focus and expansion, the row menu and its six items (Review unique text…, Keep, Make this the reference, Protect this file, Open containing folder, Copy path). Actions ▾ and Sort need a scan with results. On `issues`, the Show details or Hide details link under the chips also takes focus and opens the discovery-problem list. |  |  |
| ES01 | On `empty`: the No relationships found message appears, its New scan button takes keyboard focus with a visible ring, and `Enter` opens New scan. Focus returns to the button when the popup closes. |  |  |
| KB02 | Compare: every action (changes, candidates, pane switch, wrap, linked scroll, select and copy, back) is reachable by keyboard and shows visible focus. |  |  |
| KB03 | Every popup: every control, including its footer actions, is reachable with `Tab` and `Shift+Tab` and shows visible focus. |  |  |
| KB04 | The rail, the main menu and its entries (Appearance, Zoom, Keyboard shortcuts, Open log folder, About, Quit) are reachable by keyboard and show visible focus. |  |  |
| LP01 | On `unicode`, long path, full path without the pointer: the Folder column and Compare's pinned header truncate long paths, and the app has no separate details view that the source shows. Queue the file and open Review cleanup (`Ctrl+Enter`), which lists full paths. Record which surface shows the whole path by keyboard alone. If none does, mark `fail` (§11.10: hover alone is insufficient) and add it to Triage. |  |  |
| LP02 | On `unicode`, long path, copy: `Ctrl+Shift+C` copies the full path; paste it into a text editor and compare it with the details view. |  |  |
| LP03 | On `unicode`, Unicode names, full path without the pointer: the same surfaces as LP01 show every Japanese, Arabic and emoji character of the full path, in order. |  |  |
| LP04 | On `unicode`, Unicode names, copy: `Ctrl+Shift+C` copies each full path with every character intact when pasted into a text editor. |  |  |
| KX01 | Global, `Ctrl+N`: no Plasma default shortcut takes this chord from the app. Name the Plasma action if one does. |  |  |
| KX02 | Global, `Ctrl+1` / `Ctrl+2`: no Plasma default shortcut takes this chord from the app. Name the Plasma action if one does. |  |  |
| KX03 | Global, `Ctrl+,`: no Plasma default shortcut takes this chord from the app. Name the Plasma action if one does. |  |  |
| KX04 | Global, `Ctrl+Enter`: no Plasma default shortcut takes this chord from the app. Name the Plasma action if one does. |  |  |
| KX05 | Global, `Ctrl+=` / `Ctrl+-` / `Ctrl+0`: no Plasma default shortcut takes this chord from the app. Name the Plasma action if one does. |  |  |
| KX06 | Global, `F10`: no Plasma default shortcut takes this chord from the app. Name the Plasma action if one does. |  |  |
| KX07 | Global, `Ctrl+Q`: no Plasma default shortcut takes this chord from the app. Name the Plasma action if one does. |  |  |
| KX08 | Findings, `Ctrl+F`: no Plasma default shortcut takes this chord from the app. Name the Plasma action if one does. |  |  |
| KX09 | Findings, Arrow keys, `Page Up`/`Page Down`, `Home`/`End`: no Plasma default shortcut takes this chord from the app. Name the Plasma action if one does. |  |  |
| KX10 | Findings, `Right` / `Left`: no Plasma default shortcut takes this chord from the app. Name the Plasma action if one does. |  |  |
| KX11 | Findings, `Enter`: no Plasma default shortcut takes this chord from the app. Name the Plasma action if one does. |  |  |
| KX12 | Findings, `Space`: no Plasma default shortcut takes this chord from the app. Name the Plasma action if one does. |  |  |
| KX13 | Findings, Menu key or `Shift+F10`: no Plasma default shortcut takes this chord from the app. Name the Plasma action if one does. |  |  |
| KX14 | Findings, `Ctrl+Shift+C`: no Plasma default shortcut takes this chord from the app. Name the Plasma action if one does. |  |  |
| KX15 | Compare, `Alt+Down` / `Alt+Up`: no Plasma default shortcut takes this chord from the app. Name the Plasma action if one does. |  |  |
| KX16 | Compare, `Alt+Right` / `Alt+Left`: no Plasma default shortcut takes this chord from the app. Name the Plasma action if one does. |  |  |
| KX17 | Compare, `F6`: no Plasma default shortcut takes this chord from the app. Name the Plasma action if one does. |  |  |
| KX18 | Compare, `Alt+Z` / `Alt+L`: no Plasma default shortcut takes this chord from the app. Name the Plasma action if one does. |  |  |
| KX19 | Compare, `Ctrl+A` / `Ctrl+C`: no Plasma default shortcut takes this chord from the app. Name the Plasma action if one does. |  |  |
| KX20 | Compare, `Esc`: no Plasma default shortcut takes this chord from the app. Name the Plasma action if one does. |  |  |
| KX21 | Popups, `Tab` / `Shift+Tab`: no Plasma default shortcut takes this chord from the app. Name the Plasma action if one does. |  |  |
| KX22 | Popups, `Esc`: no Plasma default shortcut takes this chord from the app. Name the Plasma action if one does. |  |  |
| KX23 | Popups, `Enter`: no Plasma default shortcut takes this chord from the app. Name the Plasma action if one does. |  |  |
| KX24 | `F1` and `Ctrl+?`: no Plasma default shortcut takes either chord from the app. |  |  |

## Triage

Every `fail`, `blocked` and `deviation` result gets a line here. A defect blocks T26 and appends a correction task. A Plasma shortcut conflict is not a code defect: it goes to the owner, who decides whether to change the §11.10 map in the spec.

| ID  | Desktop | Row | What happened | Correction task | Invalidates | Owner decision |
| --- | ------- | --- | ------------- | --------------- | ----------- | -------------- |
|     |         |     |               |                 |             |                |
|     |         |     |               |                 |             |                |
|     |         |     |               |                 |             |                |

`Correction task` is the appended task ID. `Invalidates` lists the evidence records the correction invalidates (`EV-…`), judged against C-14. `Owner decision` is the owner's call on a conflict, a deviation or a blocked row, with its date.

### Reruns

After a correction lands, rerun the affected rows and record them here. If the correction changed `crates/`, `Cargo.lock`, the root `Cargo.toml` or `rust-toolchain.toml`, the binding changed too: record the new `application_commit` and object IDs here, and rerun every row the correction could touch.

| Row | Desktop | Build (`application_commit`) | Result | Notes |
| --- | ------- | ---------------------------- | ------ | ----- |
|     |         |                              |        |       |
|     |         |                              |        |       |

## Result

| Desktop    | pass | fail | n/a | blocked | deviation |
| ---------- | ---- | ---- | --- | ------- | --------- |
| GNOME      |      |      |     |         |           |
| KDE Plasma |      |      |     |         |           |

- PV-T26-001 (every C-13 item recorded with environment versions): _to fill_.
- PV-T26-002 (keyboard reach and visible focus, focus restoration, long and Unicode paths): _to fill_.
- Open defects, conflicts and dispositions: _to fill_.
- Closing result, `pass` only when no row is `fail`, `blocked` or `deviation` without a recorded owner decision: _to fill_.
