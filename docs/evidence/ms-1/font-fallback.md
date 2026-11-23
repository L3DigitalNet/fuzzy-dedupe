<!--
SPDX-FileCopyrightText: 2026 L3Digital LLC
SPDX-License-Identifier: GPL-3.0-only
-->

# MS-1 font fallback evidence (EV-001)

## Binding (C-14)

| Field | Value |
| --- | --- |
| `application_commit` | `5aaf2c8ed201b8d80c43c01d60e18f2d7e93d939` |
| `crates_tree` | `b4e1cee9439831dd7897c849d5e817267840f9e1` |
| `cargo_lock` | `5b86f8bade9f841adf89679cac394a98a3245d08` |
| `cargo_toml` | `a94ccf938ee056051158410c860a9cdbea464214` |
| `rust_toolchain` | `e50dfd8fe46b8b05bd33c47b9523b2768270fef7` |
| Build profile | `dev` (Cargo `test` and `dev` profiles, unoptimized with debug info) |
| Provenance | Built locally on the reference workstation from a clean checkout of `application_commit` under the plan §3.4 reference-workstation exception; toolchain `rustc 1.96.0 (ac68faa20 2026-05-25)`, `cargo 1.96.0 (30a34c682 2026-05-25)` |

The four object IDs are `git rev-parse "$C":crates`, `"$C":Cargo.lock`, `"$C":Cargo.toml` and `"$C":rust-toolchain.toml` with `C` set to `application_commit`. The commit that carries this file follows `application_commit` only by changes under `docs/evidence/`.

## What this proves

Exit item 7 (REQ-007, PV-T21-001): the `unicode` scenario's Japanese, Arabic and emoji file names render with no missing-glyph boxes. Specifically:

1. Every character of the scenario's Japanese and Arabic folder segments and file names has a glyph in IBM Plex Sans or in a font the fontconfig adapter supplied, and every character Plex lacks has one in a fontconfig font.
2. Every emoji in the emoji file name has a glyph in the embedded Noto Emoji.
3. An in-app capture of those Findings rows shows no missing-glyph boxes.
4. The negative control: the same check without the adapter reports missing glyphs in every Japanese and Arabic name.

## Run conditions

- Date: 2026-11-23.
- Machine: the reference workstation (plan §11.9): Intel Core Ultra 7 155H with integrated Intel Arc graphics (Mesa, Xe kernel driver), 62 GiB of memory.
- OS: Fedora Linux 45. The session was Wayland with `XDG_CURRENT_DESKTOP=KDE`. The glyph check does not depend on the desktop, and the capture renders off-screen.
- fontconfig: 2.18.3 (`fontconfig-2.18.3-2.fc45`), loaded at run time through the crate's `dlopen` feature.

## Selected fallback families

The adapter returned these, in this order. Each is face 0 of its file, which is the face egui loads.

| Script (fontconfig `lang`) | Family | Package | File |
| --- | --- | --- | --- |
| Japanese (`ja`) | Noto Sans CJK JP | `google-noto-sans-cjk-vf-fonts-2.004-12.fc45` | `/usr/share/fonts/google-noto-sans-cjk-vf-fonts/NotoSansCJK-VF.ttc` (32,682,580 bytes) |
| Arabic (`ar`) | Noto Sans Arabic | `google-noto-sans-arabic-vf-fonts-20260601-3.fc45` | `/usr/share/fonts/google-noto-vf/NotoSansArabic[wght].ttf` (239,324 bytes) |
| Hebrew (`he`) | DejaVu Sans | `dejavu-sans-fonts-2.37-30.fc45` | `/usr/share/fonts/dejavu-sans-fonts/DejaVuSans.ttf` (759,720 bytes) |
| Thai (`th`) | Noto Sans Thai | `google-noto-sans-thai-vf-fonts-20260601-3.fc45` | 31,912 bytes, in `/usr/share/fonts/google-noto-vf/` |

## Glyph coverage

Command, run from the root of a clean checkout of `application_commit`:

```bash
cargo test --locked -p fuzzy-dedupe-platform --test fonts -- --include-ignored --nocapture
```

Result: exit 0, 4 passed, 0 failed.

| Test | Result | Proves |
| --- | --- | --- |
| `fonts_system_fallbacks_cover_japanese_and_arabic_names` (ignored by default) | pass | Item 1, and that the adapter's family keys are unique |
| `fonts_embedded_noto_emoji_covers_the_emoji_names` | pass | Item 2 |
| `fonts_without_the_adapter_japanese_and_arabic_lack_glyphs` | pass | Item 4 |
| `fonts_cmap_reader_answers_known_glyphs` | pass | The coverage reader on known answers (format 4 and format 12 subtables) |

Coverage is read from each font's own `cmap` table at face 0 of the bytes the adapter returns, so it checks what egui receives. It does not rely on fontconfig's coverage claims.

Before the adapter existed (the T10 stub, same test file), the ignored test failed with exit 101 and `the adapter supplied no fallback fonts`.

The negative control printed these missing-glyph counts with the embedded faces alone:

| Name                                                | Missing glyphs |
| --------------------------------------------------- | -------------- |
| 文書                                                | 2              |
| 会議                                                | 2              |
| 議事録_ミーティング.txt                             | 9              |
| プロジェクト                                        | 6              |
| 二〇二六年度                                        | 6              |
| 要件定義                                            | 4              |
| レビュー済み                                        | 6              |
| 最終版                                              | 3              |
| アーカイブ                                          | 5              |
| 長いフォルダー名の例                                | 10             |
| 非常に長いファイル名の例_要件定義書_最終版_確定.txt | 22             |
| أرشيف                                               | 5              |
| تقرير_الاجتماع.txt                                  | 13             |
| نسخة.txt                                            | 4              |

## In-app captures

Kind: an `egui_kittest` wgpu render on the workstation GPU, not a screenshot of the launched application. It renders the Findings workspace (shell, Findings view and dialog host, as `crates/fuzzy-dedupe-app/tests/findings.rs` drives them) on the `unicode` scenario at the default seed, at 1600 × 960, with every group expanded. Fonts are installed exactly as `FuzzyDedupeApp::new` installs them: `theme::install` with the adapter's fonts, then the icon glyphs.

| File | Fonts | SHA-256 |
| --- | --- | --- |
| [`findings-unicode-with-fallback.png`](font-fallback/findings-unicode-with-fallback.png) | Plex, Noto Emoji, then the adapter's four fallbacks | `b4b2aef511a72abb652c56cd9f5eccbaefb1ac1867a1c6f46c750ac5748f5909` |
| [`findings-unicode-without-fallback.png`](font-fallback/findings-unicode-without-fallback.png) | Plex and Noto Emoji only (negative control) | `66e50ea3d8368b91d929039bb0bf0d760778f42d506ca1ca01210cb1bc273a2a` |

The with-fallback render ran under `egui_kittest`'s default missing-glyph policy, which panics on the first character no installed font can draw. It completed, so no row drew a missing-glyph box. Inspecting the image confirms it: the Japanese names and folders, the Arabic names (joined and laid out right to left) and the 📝 and ✅ emoji all render as glyphs.

The negative-control render allows missing glyphs so that it can be captured. Every Japanese and Arabic character shows as a box, while the emoji, Latin text and `Café` still render.

Folder paths in the captures are fixture data (`/home/fixture/…`), not paths on the workstation.

## Limitations

- The capture is an off-screen wgpu render through the app's own UI code, not a screenshot of the launched binary, which always starts on the design scenario.
- The CJK fallback is a variable font loaded at its default instance, so Japanese text in the semibold group-header rows renders at regular weight.
- Which fonts are selected depends on the installed packages. A workstation without a font for a script degrades to missing-glyph boxes for that script only.

## Capture program

A scratch binary crate outside the repository with path dependencies on the checkout's `fuzzy-dedupe-app`, `-engine`, `-platform` and `-domain`, plus `egui =0.37.0`, `egui_kittest =0.37.0` (feature `wgpu`) and `image 0.25.10` (feature `png`). It used a copy of the checkout's `Cargo.lock` and `rust-toolchain.toml`, and ran as `cargo build && ./target/debug/t21-capture <output-dir>`.

```rust
//! Scratch capture for EV-001: renders the Findings workspace on the
//! `unicode` scenario with every group expanded, through egui_kittest's
//! wgpu renderer, once with the platform adapter's fallbacks and once
//! without. Mirrors the Rig in crates/fuzzy-dedupe-app/tests/findings.rs.
use std::path::Path;
use std::sync::Arc;

use egui::Vec2;
use egui_kittest::Harness;
use fuzzy_dedupe_app::presentation::commands::{self, IntentQueue, Services};
use fuzzy_dedupe_app::presentation::events;
use fuzzy_dedupe_app::presentation::state::{PresentationState, ScanStage};
use fuzzy_dedupe_app::ui::widgets::dialog_host;
use fuzzy_dedupe_app::ui::{shell, theme};
use fuzzy_dedupe_engine::fixtures::{self, ScenarioKind};
use fuzzy_dedupe_engine::service::{Engine, EngineConfig};
use fuzzy_dedupe_platform::dirs::AppDirs;
use fuzzy_dedupe_platform::fonts::{FontLookup, SystemFontLookup};
use fuzzy_dedupe_platform::open::{Location, OpenHandle, Opener};
use fuzzy_dedupe_platform::picker::{FolderPicker, PickHandle, PickOutcome, PickRequest};

struct NoOpen;
impl Opener for NoOpen {
    fn open(&self, _: Location, notify: Box<dyn FnOnce() + Send>) -> OpenHandle {
        notify();
        OpenHandle::ready(Ok(()))
    }
}
struct NoPick;
impl FolderPicker for NoPick {
    fn pick_folders(&self, _: PickRequest, notify: Box<dyn FnOnce() + Send>) -> PickHandle {
        PickHandle::spawn(|| PickOutcome::Dismissed, notify)
    }
}

struct Rig {
    state: PresentationState,
    services: Services,
    intents: IntentQueue,
    ready: bool,
}

impl Rig {
    fn frame(&mut self, ui: &mut egui::Ui) {
        if !self.ready {
            return;
        }
        let ctx = ui.ctx().clone();
        self.services.poll(&mut self.state);
        dialog_host::begin_frame(&ctx, &mut self.state.dialog);
        commands::read_global_chords(&ctx, &self.state, Some(shell::menu_entry_id()), &mut self.intents);
        dialog_host::handle_menu_keys(&ctx, &mut self.state.dialog);
        shell::show(ui, &mut self.state, &mut self.intents);
        dialog_host::show(&ctx, &mut self.state, &mut self.intents);
        let mut intents = self.intents.drain();
        for _ in 0..4 {
            if intents.is_empty() {
                break;
            }
            let mut followups = IntentQueue::default();
            for intent in intents {
                commands::dispatch(intent, &mut self.state, &mut self.services, &ctx, &mut followups);
            }
            intents = followups.drain();
        }
    }
}

fn capture(with_adapter: bool, out: &Path) {
    let dirs = AppDirs::resolve(|_| None, Path::new("/tmp/t21-capture/home"));
    let engine = Engine::new(EngineConfig::default()).expect("engine");
    let mut services = Services::new(engine, Box::new(NoOpen), Box::new(NoPick), dirs);
    let mut state = PresentationState::default();
    let scan = services.engine.reserve_scan_id();
    let scenario = Arc::new(fixtures::build(ScenarioKind::Unicode, scan, fixtures::DEFAULT_SEED).expect("scenario"));
    services.engine.adopt_scenario(Arc::clone(&scenario)).expect("adopt");
    events::install_results(&mut state, Arc::clone(&scenario.results), ScanStage::Completed);
    state.findings.expanded = scenario.results.groups.iter().map(|g| g.id).collect();
    services.sync_view(&mut state);
    state.status = None;
    let rig = Rig { state, services, intents: IntentQueue::default(), ready: false };
    let mut builder = Harness::builder().with_size(Vec2::new(1600.0, 960.0)).wgpu();
    // With the adapter, kittest's default policy panics on any missing glyph,
    // so a completed render is itself the no-tofu proof. Without it, tofu is
    // allowed so the negative control can be captured.
    if !with_adapter {
        builder = builder.allow_missing_glyphs();
    }
    let mut harness = builder
        .build_ui_state(|ui, rig: &mut Rig| rig.frame(ui), rig);
    let fallbacks: Vec<_> = if with_adapter { SystemFontLookup.fallback_fonts() } else { Vec::new() };
    for f in &fallbacks {
        println!("fallback family: {}", f.family);
    }
    let fallbacks = fallbacks
        .into_iter()
        .map(|font| (font.family, egui::FontData::from_owned(font.data)))
        .collect();
    theme::install(&harness.ctx, fallbacks);
    shell::install_icon_glyphs(&harness.ctx);
    harness.state_mut().ready = true;
    for _ in 0..24 {
        harness.step();
    }
    let image = harness.render().expect("render");
    image.save(out).expect("save png");
    println!("wrote {}", out.display());
}

fn main() {
    let dir = std::env::args().nth(1).expect("output dir");
    let dir = Path::new(&dir);
    capture(true, &dir.join("findings-unicode-with-fallback.png"));
    capture(false, &dir.join("findings-unicode-without-fallback.png"));
}
```
