---
schema_version: '1.1'
id: 'adr-0001-fuzzy-dedupe-use-egui-for-the-v1-desktop-ui'
title: 'ADR 0001: Use egui for the V1 desktop UI'
description: 'Selects egui/eframe as the UI framework for the Fuzzy DeDupe V1 desktop application. Amended 2026-11-22: egui is committed, and Slint is only an owner-decided last resort.'
doc_type: 'adr'
status: 'active'
created: '2026-11-21'
updated: '2026-11-22'
reviewed: '2026-11-21'
owner: 'L3DigitalNet'
consumer: 'agent'
tags:
  - 'gui'
  - 'egui'
  - 'architecture'
aliases:
  - 'gui-framework-decision'
  - 'oq-004'
related:
  - 'SPEC-FZDP'
supersedes: []
superseded_by: null
source:
  - 'https://github.com/emilk/egui'
  - 'https://docs.rs/egui_kittest'
  - 'https://github.com/rerun-io/egui_table'
  - 'https://slint.dev/blog'
  - 'https://github.com/iced-rs/iced'
  - 'https://v2.tauri.app/'
  - 'https://github.com/KDAB/cxx-qt'
confidence: 'medium'
visibility: 'internal'
license: null
project:
  decision_makers:
    - 'L3DigitalNet'
  consulted: []
  informed: []
  amends: []
  amended_by: []
---

# ADR 0001: Use egui for the V1 desktop UI

MADR status: **accepted**.

> **Amended 2026-11-22 (SPEC-FZDP v0.6 gap review, finding I-03).** The owner committed to egui/eframe. The MS-1 prototype no longer decides whether the choice is final, and Slint is no longer a designated fallback that a failed criterion would trigger. The choice of egui with eframe, its scope, and its consequences remain in force. See [Amendments](#amendments).

## Context and Problem Statement

The Fuzzy DeDupe specification ([SPEC-FZDP](../specs/canonical/fuzzy-dedupe.md), open question OQ-004) left the desktop UI framework open. The application is written in Rust (C-002) and must implement the approved compact Figma design (§11). That design has a dominant grouped findings table and large, aligned, read-only side-by-side text panes, with settings, scan setup and cleanup shown as in-window popups.

- Governed concern: which UI framework draws and drives the V1 desktop interface.
- Applies to: the application's presentation layer, the proposed `crates/app` in spec §8.5, for the V1 Linux release (C-014).
- Applies when: code renders the shell, workspaces, tables, comparison panes or popups, or turns user input into application commands.
- Does not apply to: the domain, core, scanner, analyzer, storage and file-operation components, which already must not depend on the UI framework (§8.5). It also excludes the diff algorithm, native file dialogs and the Trash backend, each of which has its own candidate in §8.3.
- Remains undecided: the exact pinned egui version, the renderer backend, the table crate (`egui_extras` or `egui_table`), and the internal design of the aligned diff widget. MS-1 settles these as implementation choices.

Which UI framework should implement the Fuzzy DeDupe V1 desktop interface?

## Decision Drivers

- The approved design is a custom flat visual style, not a native look: specific colors, IBM Plex fonts, 28-unit rows and filter chips (§11.3, §11.7).
- A grouped, virtualized findings table must handle tens of thousands of rows while keeping row identities stable (§11.4, FR-018).
- The Compare panes need aligned read-only text with spacer rows, a line-number gutter, line highlighting and text selection, on files that may be several megabytes (§11.5). No candidate provides this as a ready-made widget.
- The UI must stay responsive while background workers hash and diff files (FR-005, §11.9).
- Headless UI tests with screenshot comparison are needed to check the implementation against the Figma frames at MS-1.
- Assistive-technology support is out of scope for this project by owner decision (2026-11-21). Screen-reader maturity therefore does not count in the comparison.
- The application is distributed under `GPL-3.0-only` (C-012). The spec prefers a single implementation language and recommends against a browser frontend in V1 (§8.1, §8.3).

## Considered Options

- egui with eframe
- Slint
- Iced
- Tauri 2 with a Svelte web frontend
- Qt 6 through CXX-Qt

## Decision Outcome

Chosen option: "egui with eframe", because it keeps the whole interface in Rust and is built for custom-drawn, custom-styled interfaces. Its ecosystem includes a headless test harness with screenshot comparison (`egui_kittest`) and a table crate with expanding rows (`egui_table`). Once accessibility was removed from scope, no other option offset that with an advantage that matters to this application.

This decision governs the UI framework for the Fuzzy DeDupe V1 desktop presentation layer, whenever code renders the interface or turns user input into application commands.

It does not govern the non-UI components, the diff algorithm, file dialogs, the Trash backend, or platforms beyond the V1 Linux release. Those are governed by SPEC-FZDP §8.3 and §8.5 or remain undecided.

The choice is final only once the MS-1 prototype passes (spec §8.3 and §19). The prototype must show all of the following with the approved compact layout:

- smooth navigation of a large grouped table with stable selection;
- aligned, selectable, read-only paired panes on representative multi-megabyte files;
- popup behavior that preserves the underlying workspace;
- a responsive UI while a scan runs.

If egui cannot meet the table or paired-pane criteria, a superseding ADR adopts Slint, the designated fallback.

### Consequences

- Good, because every layer of the application is Rust, with no second UI language, C++ toolchain or bundled web engine.
- Good, because custom drawing suits the flat, compact visual style better than native-widget toolkits.
- Good, because `egui_kittest` supports headless interaction tests and screenshot comparison, which the MS-1 check against Figma needs.
- Good, because the §8.5 boundary confines a later framework change to the presentation crate.
- Bad, because each egui minor release breaks the API (0.35, 0.36 and 0.37 all did in 2026). Versions must be pinned and upgraded deliberately.
- Bad, because the grouped table and the aligned diff panes are custom widgets, including selection across virtualized rows. This would be true for every option considered.
- Neutral, because file dialogs need a separate crate (`rfd`, already proposed in §8.3).

### Confirmation

A change falls within this ADR when it adds or modifies presentation-layer code for the V1 desktop application. Conformance is confirmed when:

- the presentation crate uses egui and eframe at an exactly pinned version, recorded in the committed lockfile;
- no non-UI component depends on egui or eframe, as checked through the dependency graph (`cargo tree`);
- the MS-1 acceptance results are recorded for the criteria named in the Decision Outcome before the choice is treated as final.

### Amendments

> **Amended 2026-11-22 (SPEC-FZDP v0.6 gap review, finding I-03).** The owner said “We will make egui work” and asked that Slint be used only if absolutely necessary. This amendment restates three parts of the accepted record; the rest stays in force.
>
> - **Decision Outcome.** The sentence “The choice is final only once the MS-1 prototype passes” and the fallback paragraph after the criteria list no longer apply. The choice is committed. MS-1 still builds the prototype and measures it against SPEC-FZDP §11.9 (targets T1 to T4) and the MS-1 exit list in §19, which cover the four criteria listed. A missed target is fixed within egui, or the owner changes the design or the target. Slint would be adopted only by a superseding ADR, on explicit owner decision after egui remediation is exhausted. No measurement triggers a switch, and no Slint spike is planned.
> - **Confirmation.** The third check now reads: the MS-1 evidence file records the measured results and the owner's sign-off of the MS-1 exit list. The results inform remediation within egui, not the framework choice.
> - **More Information.** Revisit this decision only by explicit owner decision: for example, when egui remediation is exhausted against a target the owner keeps, when egui's release breakage blocks a needed upgrade, or if accessibility returns to scope.
>
> SPEC-FZDP v0.7 has since chosen `egui_table` and the wgpu renderer, two items this record left undecided. The specification governs them; this ADR does not.

## Pros and Cons of the Options

### egui with eframe

- Good, because it is pure Rust, draws its own widgets, and lets one theme module control colors, fonts and density.
- Good, because 0.34.3 improved text-selection performance for large documents, and `egui_table` supports rows that expand into groups.
- Good, because `egui_kittest` offers headless input, accessibility-tree queries and snapshot testing.
- Bad, because the project warns that upgrades break code, and three breaking minor releases shipped between March and October 2026.
- Bad, because selection across separately laid-out virtualized rows, and wrapped-height alignment, must be built by the application.

### Slint

- Good, because its API is stable, its GPLv3 option fits a `GPL-3.0-only` application, and a Figma plugin can export a selection as an editable Slint project (1.19).
- Good, because it offers read-only selectable text and a `StandardTableView`.
- Bad, because it has no grouped or tree table rows, and virtualization and resizable columns were unconfirmed.
- Bad, because it adds a separate `.slint` UI language, and its testing backend does not follow semantic versioning.

### Iced

- Good, because it is pure Rust with an Elm-style architecture and a headless `iced_test` simulator.
- Bad, because upstream has no built-in table and no selectable text; selectable text is still on the roadmap.
- Bad, because the project calls itself experimental and minor releases are about a year apart.

### Tauri 2 with a Svelte web frontend

- Good, because the browser engine handles text selection, highlighting and CSS layout natively, which suits the diff panes and a precise visual style.
- Bad, because it adds a web frontend stack, which the spec recommends against for V1.
- Bad, because Linux builds depend on the distribution's WebKitGTK. That brings version drift, reported scroll lag, and blank windows on NVIDIA under Wayland.

### Qt 6 through CXX-Qt

- Good, because Qt has mature virtualized table and tree views and a strong modal and focus model.
- Bad, because it adds a C++, CMake and QML toolchain alongside Rust, and its Rust bindings do not cover Qt Widgets.
- Bad, because CXX-Qt changes frequently (9 of 21 releases were breaking), and open-source Qt expects an upgrade about every six months.
- Neutral, because its most mature advantage, Linux screen-reader support, no longer counts now that accessibility is out of scope.

## More Information

The comparison rests on a dated research pass on 2026-11-21 covering egui 0.37.0, Slint 1.19.0, Iced 0.14.0, Tauri 2.12.3 and CXX-Qt 0.10.0 with Qt 6.12. Its confidence was moderate. Version and licensing facts were well sourced; no candidate's behavior on multi-megabyte text was measured, which is why the MS-1 prototype remains the deciding check.

The original v0.2 product specification recommended egui as the leading choice, subject to the same validation check. This ADR turns that recommendation into a decision.

Revisit this decision if the MS-1 prototype fails its criteria, if egui's release breakage blocks a needed upgrade, or if accessibility returns to scope.
