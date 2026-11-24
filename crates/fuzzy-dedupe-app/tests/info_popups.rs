// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! PV-T19-001 (plan T19): the Keyboard shortcuts and About popups, driven
//! headless through `egui_kittest` on the full app.
//!
//! Every test sits in the `info_popups` module so `-E 'test(info_popups)'`
//! selects them. The opener is a recording double: no test starts a browser.
//! Expected texts come from their sources of truth, never from a copy kept
//! here: the §11.10 table is parsed out of the canonical spec, the license
//! and notices are the repository files, and version, license and address
//! are the package metadata Cargo sets for this crate.

mod info_popups {
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};
    use std::time::{SystemTime, UNIX_EPOCH};

    use egui::accesskit::Role;
    use egui::{Event, Key, Modifiers, OutputCommand, Vec2};
    use egui_kittest::Harness;
    use egui_kittest::kittest::{NodeT, Queryable};
    use fuzzy_dedupe_app::app::{AppDeps, Clock, FuzzyDedupeApp};
    use fuzzy_dedupe_app::measure::NoMeasurement;
    use fuzzy_dedupe_app::presentation::commands::{
        IntentQueue, SHORTCUT_TABLE, ShortcutRow, ShortcutScope,
    };
    use fuzzy_dedupe_app::presentation::state::{DialogKind, DialogSubject, PresentationState};
    use fuzzy_dedupe_app::ui::shell::{MAIN_MENU_ENTRIES, MENU_ENTRY_NAME};
    use fuzzy_dedupe_app::ui::theme::CompactStyle;
    use fuzzy_dedupe_app::ui::widgets::dialog_host;
    use fuzzy_dedupe_engine::service::EngineConfig;
    use fuzzy_dedupe_platform::dirs::AppDirs;
    use fuzzy_dedupe_platform::fonts::SystemFontLookup;
    use fuzzy_dedupe_platform::log_retention::LogDate;
    use fuzzy_dedupe_platform::open::{Location, OpenHandle, Opener};
    use fuzzy_dedupe_platform::picker::{FolderPicker, PickHandle, PickOutcome, PickRequest};

    /// Step bound for every `Harness::run` here, in place of kittest's
    /// default of 4.
    ///
    /// A result from another thread (engine work, the folder picker, the
    /// opener) may arrive part-way through a run, and the widgets it draws for
    /// the first time each ask egui for up to two more frames (`widget_style`).
    /// On slow CI runners that overran 4 steps (PR #39, "would have settled
    /// after 6 steps"). 16 covers it with margin. A continuous repaint, from an
    /// animation or a repaint loop, still exhausts 16, so `run()` keeps
    /// catching runaway repaints.
    ///
    /// Each harness test file under `tests/` declares its own copy, because
    /// they share no module; keep the copies equal.
    const SETTLE_STEPS: u64 = 16;

    const SPEC: &str = include_str!("../../../docs/specs/canonical/fuzzy-dedupe.md");
    const LICENSE: &str = include_str!("../../../LICENSE");
    const NOTICES: &str = include_str!("../../../THIRD-PARTY-NOTICES.md");

    // ── Doubles and harness ─────────────────────────────────────────────

    #[derive(Clone, Default)]
    struct OpenRecorder(Arc<Mutex<Vec<Location>>>);

    impl Opener for OpenRecorder {
        fn open(&self, location: Location, notify: Box<dyn FnOnce() + Send>) -> OpenHandle {
            self.0.lock().unwrap().push(location);
            notify();
            OpenHandle::ready(Ok(()))
        }
    }

    struct DismissPicker;

    impl FolderPicker for DismissPicker {
        fn pick_folders(&self, _: PickRequest, notify: Box<dyn FnOnce() + Send>) -> PickHandle {
            PickHandle::spawn(|| PickOutcome::Dismissed, notify)
        }
    }

    struct FixedClock;

    impl Clock for FixedClock {
        fn today(&self) -> LogDate {
            LogDate::new(2026, 11, 23).expect("valid date")
        }
    }

    /// A fresh home under the test target directory, unique per run because
    /// tests may not delete what an earlier run left behind.
    fn dirs_for(name: &str) -> AppDirs {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after 1970")
            .as_nanos();
        let root: PathBuf = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("info_popups")
            .join(format!("{name}-{nanos}"));
        let state = root.join("state");
        AppDirs::resolve(
            move |var| (var == "XDG_STATE_HOME").then(|| state.clone().into_os_string()),
            &root.join("home"),
        )
    }

    struct Slot {
        app: Option<FuzzyDedupeApp>,
    }

    type Shell = Harness<'static, Slot>;

    /// The app at the 1600 × 960 reference size, built on the harness's own
    /// context before its first frame, as eframe builds it.
    fn launch(name: &str) -> (Shell, OpenRecorder) {
        let opens = OpenRecorder::default();
        let deps = AppDeps {
            dirs: dirs_for(name),
            opener: Box::new(opens.clone()),
            picker: Box::new(DismissPicker),
            fonts: Box::new(SystemFontLookup),
            clock: Box::new(FixedClock),
            log_expiry: Box::new(|_| {}),
            engine: EngineConfig::default(),
            measure: Box::new(NoMeasurement),
        };
        let mut harness = Harness::builder()
            .with_max_steps(SETTLE_STEPS)
            .with_size(Vec2::new(1600.0, 960.0))
            .build_ui_state(
                |ui, slot: &mut Slot| {
                    if let Some(app) = slot.app.as_mut() {
                        app.run_logic(ui.ctx());
                        app.draw(ui);
                    }
                },
                Slot { app: None },
            );
        let app = FuzzyDedupeApp::new(&harness.ctx, deps).expect("engine starts");
        harness.state_mut().app = Some(app);
        harness.run();
        (harness, opens)
    }

    fn state(h: &Shell) -> &PresentationState {
        &h.state().app.as_ref().expect("app").state
    }

    fn state_mut(h: &mut Shell) -> &mut PresentationState {
        &mut h.state_mut().app.as_mut().expect("app").state
    }

    fn open_dialog(h: &Shell) -> Option<DialogKind> {
        state(h).dialog.dialog().map(|d| d.kind)
    }

    fn chord(h: &mut Shell, modifiers: Modifiers, key: Key) {
        h.key_press_modifiers(modifiers, key);
        h.run();
    }

    fn open_from_main_menu(h: &mut Shell, entry: &str) {
        h.get_by_role_and_label(Role::Button, MENU_ENTRY_NAME)
            .click();
        h.run();
        h.get_by_role_and_label(Role::Button, entry).click();
        h.run();
    }

    fn open_about(h: &mut Shell) {
        open_from_main_menu(h, MAIN_MENU_ENTRIES[4]);
        assert_eq!(open_dialog(h), Some(DialogKind::About));
    }

    fn button(h: &Shell, label: &str) -> bool {
        h.query_by_role_and_label(Role::Button, label).is_some()
    }

    /// The footer Close. The host's title-row × is also named Close and is
    /// drawn first, so the footer button is the last of the two.
    fn footer_close(h: &Shell) -> egui_kittest::Node<'_> {
        let mut all: Vec<_> = h.get_all_by_role_and_label(Role::Button, "Close").collect();
        assert_eq!(all.len(), 2, "the title × and the footer Close");
        all.pop().expect("footer Close")
    }

    /// Every text shape this frame painted, nested shape lists included.
    fn texts(h: &Shell) -> Vec<egui::epaint::TextShape> {
        fn walk(shape: &egui::Shape, out: &mut Vec<egui::epaint::TextShape>) {
            match shape {
                egui::Shape::Text(text) => out.push(text.clone()),
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, out)),
                _ => {}
            }
        }
        let mut out = Vec::new();
        for clipped in &h.output().shapes {
            walk(&clipped.shape, &mut out);
        }
        out
    }

    /// Whether this frame painted exactly `text` as one galley in `style`.
    fn painted_in(h: &Shell, text: &str, style: CompactStyle) -> bool {
        texts(h).iter().any(|shape| {
            let job = &shape.galley.job;
            job.text == text
                && !job.sections.is_empty()
                && job
                    .sections
                    .iter()
                    .all(|s| s.format.font_id == style.font_id())
        })
    }

    /// Every accessible label and value in the tree this frame.
    fn all_strings(h: &Shell) -> Vec<String> {
        let root = h.root();
        root.children_recursive()
            .flat_map(|node| {
                let node = node.accesskit_node();
                [node.label(), node.value()]
            })
            .flatten()
            .collect()
    }

    // ── Keyboard shortcuts ──────────────────────────────────────────────

    /// The §11.10 Scope | Keys | Action table, parsed from the spec.
    fn spec_keyboard_map() -> Vec<(String, String, String)> {
        let section = SPEC.split("\n### 11.10 ").nth(1).expect("spec has §11.10");
        let table = section
            .split("| Scope | Keys | Action |")
            .nth(1)
            .expect("§11.10 has the keyboard map");
        table
            .lines()
            .skip(2) // the rest of the header line, then the delimiter row
            .take_while(|line| line.starts_with('|'))
            .map(|line| {
                let cells: Vec<&str> = line.trim_matches('|').split(" | ").map(str::trim).collect();
                assert_eq!(cells.len(), 3, "{line}");
                (cells[0].into(), cells[1].into(), cells[2].into())
            })
            .collect()
    }

    fn scope_name(scope: ShortcutScope) -> &'static str {
        match scope {
            ShortcutScope::Global => "Global",
            ShortcutScope::Findings => "Findings",
            ShortcutScope::Compare => "Compare",
            ShortcutScope::Popups => "Popups",
        }
    }

    /// The accessible name of one displayed row: its action, then its keys.
    fn row_name(row: &ShortcutRow) -> String {
        format!("{}: {}", row.action, row.keys)
    }

    #[test]
    fn info_popups_shortcuts_open_from_f1_ctrl_question_and_the_main_menu() {
        let (mut h, _) = launch("shortcuts-open");
        for how in ["F1", "Ctrl+?", "main menu"] {
            match how {
                "F1" => chord(&mut h, Modifiers::NONE, Key::F1),
                "Ctrl+?" => chord(&mut h, Modifiers::CTRL, Key::Questionmark),
                _ => open_from_main_menu(&mut h, MAIN_MENU_ENTRIES[2]),
            }
            assert_eq!(
                open_dialog(&h),
                Some(DialogKind::KeyboardShortcuts),
                "{how}"
            );
            assert!(
                h.query_by_role_and_label(Role::ListItem, &row_name(&SHORTCUT_TABLE[0]))
                    .is_some(),
                "{how}: the map is shown"
            );
            footer_close(&h).click();
            h.run();
            assert_eq!(open_dialog(&h), None, "{how}: Close closes it");
        }
    }

    #[test]
    fn info_popups_shortcuts_list_the_spec_table_row_for_row() {
        let spec = spec_keyboard_map();
        let table: Vec<(String, String, String)> = SHORTCUT_TABLE
            .iter()
            .map(|r| (scope_name(r.scope).into(), r.keys.into(), r.action.into()))
            .collect();
        assert_eq!(table, spec, "SHORTCUT_TABLE is the §11.10 table");

        let (mut h, _) = launch("shortcuts-rows");
        chord(&mut h, Modifiers::NONE, Key::F1);
        let shown: Vec<String> = h
            .get_all_by_role(Role::ListItem)
            .map(|node| node.accesskit_node().label().unwrap_or_default())
            .collect();
        let mut expected: Vec<String> = SHORTCUT_TABLE.iter().map(row_name).collect();
        // T40 P6: frame `44:3681`'s shortcuts-window row, after Main menu.
        let main_menu = expected
            .iter()
            .position(|r| r.starts_with("Main menu: "))
            .expect("the Main menu row");
        expected.insert(main_menu + 1, "Keyboard shortcuts: F1 / Ctrl+?".to_owned());
        assert_eq!(
            shown, expected,
            "one displayed row per §11.10 row, in order, plus the frame's shortcuts-window row"
        );

        for heading in ["Global", "Findings", "Compare", "Popups"] {
            h.get_by_role_and_label(Role::Heading, heading);
        }
        // One key cap per chord, in the path style (Figma `DD/Compact/Key cap`).
        // §11.10 separates chords with " / ", ", " and " or " (T40 P6).
        for row in SHORTCUT_TABLE {
            for chord in row
                .keys
                .split(" / ")
                .flat_map(|k| k.split(", "))
                .flat_map(|k| k.split(" or "))
            {
                assert!(
                    painted_in(&h, chord, CompactStyle::PathDetail),
                    "key cap {chord:?} of {:?}",
                    row.action
                );
            }
        }
        h.get_by_label("No shortcut moves a file to Trash, and none is a single letter.");
    }

    // ── About ───────────────────────────────────────────────────────────

    const COPYRIGHT: &str = "Copyright © 2026 L3Digital LLC";

    #[test]
    fn info_popups_about_shows_identity_version_and_legal_notices() {
        let (mut h, _) = launch("about-notices");
        open_about(&mut h);
        h.get_by_label("Fuzzy DeDupe");
        h.get_by_label(&format!("Version {}", env!("CARGO_PKG_VERSION")));
        h.get_by_label(COPYRIGHT);
        let license = h.get_by_label_contains("GNU General Public License");
        // A `Role::Label` node carries its text as the AccessKit value.
        let license = license.accesskit_node().value().unwrap_or_default();
        assert_eq!(env!("CARGO_PKG_LICENSE"), "GPL-3.0-only");
        assert!(
            license.contains(&format!("({})", env!("CARGO_PKG_LICENSE"))),
            "displays the package license: {license}"
        );
        h.get_by_label_contains("ABSOLUTELY NO WARRANTY");
        for action in ["View license", "Third-party notices", "Source code"] {
            assert!(button(&h, action), "{action}");
        }
    }

    /// The no-or-later rule covers About's own summary, notices and the
    /// displayed package license. It deliberately does not cover the
    /// embedded GPL text, which says "or any later version" (`LICENSE` line
    /// 572), nor third-party legal texts: those are checked for exact
    /// content instead.
    #[test]
    fn info_popups_about_grants_no_or_later_license() {
        let (mut h, _) = launch("about-phrases");
        open_about(&mut h);
        let mut checked: Vec<String> = all_strings(&h);
        assert!(
            checked.iter().any(|s| s == COPYRIGHT),
            "About's summary is in scope"
        );
        checked.push(env!("CARGO_PKG_LICENSE").to_owned());
        for text in &checked {
            let lower = text.to_lowercase();
            for phrase in [
                "or later",
                "or-later",
                "later version",
                "gpl-3.0+",
                "gplv3+",
            ] {
                assert!(!lower.contains(phrase), "{phrase:?} in {text:?}");
            }
        }
    }

    /// Opens `action`'s text view and checks it, then Back.
    fn check_text_view(name: &str, action: &str, expected: &str) {
        let (mut h, _) = launch(name);
        open_about(&mut h);
        h.get_by_role_and_label(Role::Button, action).click();
        h.run();

        // About stays the one open popup, its body replaced.
        assert_eq!(open_dialog(&h), Some(DialogKind::About));
        h.get_by_label(DialogKind::About.title());
        assert!(
            h.query_by_label(COPYRIGHT).is_none(),
            "the body was replaced"
        );
        assert!(!button(&h, "View license"), "the body was replaced");
        assert!(button(&h, "Back"));

        // Read-only, in the Document style, byte-identical to the file.
        let views: Vec<_> = h.get_all_by_role(Role::MultilineTextInput).collect();
        assert_eq!(views.len(), 1, "one text view");
        assert!(views[0].accesskit_node().is_read_only(), "read-only");
        assert!(
            painted_in(&h, expected, CompactStyle::Document),
            "{action}: the whole text in the Document style"
        );
        let doc = texts(&h)
            .into_iter()
            .find(|t| t.galley.job.text == expected)
            .expect("text view galley");
        assert!(
            doc.galley
                .job
                .sections
                .iter()
                .all(|s| s.format.line_height == Some(CompactStyle::Document.line_height())),
            "Document line height"
        );

        // Typing changes nothing.
        views[0].focus();
        h.run();
        h.get_by_role(Role::MultilineTextInput).type_text("x");
        h.run();
        assert!(
            painted_in(&h, expected, CompactStyle::Document),
            "unchanged by typing"
        );

        // Selecting the whole text and copying copies it exactly.
        let mut select_all = Modifiers::CTRL;
        select_all.command = true;
        h.key_press_modifiers(select_all, Key::A);
        h.step();
        h.input_mut().events.push(Event::Copy);
        h.step();
        let copied: Vec<&String> = h
            .output()
            .platform_output
            .commands
            .iter()
            .filter_map(|c| match c {
                OutputCommand::CopyText(text) => Some(text),
                _ => None,
            })
            .collect();
        assert_eq!(copied.len(), 1, "{action}: one copy");
        assert!(copied[0] == expected, "{action}: copied text is exact");
        h.run();

        // No second popup opens over it.
        chord(&mut h, Modifiers::NONE, Key::F1);
        chord(&mut h, Modifiers::NONE, Key::F10);
        let mut intents = IntentQueue::default();
        let refused = dialog_host::open(
            state_mut(&mut h),
            DialogKind::KeyboardShortcuts,
            DialogSubject::None,
            None,
            &mut intents,
        );
        assert_eq!(refused, Err(dialog_host::Refused::OverlayOpen));
        h.run();
        assert_eq!(open_dialog(&h), Some(DialogKind::About));
        assert_eq!(state(&h).dialog.menu(), None);

        // Back restores the body.
        h.get_by_role_and_label(Role::Button, "Back").click();
        h.run();
        assert_eq!(open_dialog(&h), Some(DialogKind::About));
        h.get_by_label(COPYRIGHT);
        assert!(button(&h, action));
        assert!(h.query_by_role(Role::MultilineTextInput).is_none());
        assert!(
            h.get_by_role_and_label(Role::Button, action).is_focused(),
            "focus returns to {action}"
        );
    }

    #[test]
    fn info_popups_view_license_shows_the_unchanged_license_and_back_restores() {
        check_text_view("about-license", "View license", LICENSE);
    }

    #[test]
    fn info_popups_third_party_notices_show_the_notices_and_back_restores() {
        check_text_view("about-notices-text", "Third-party notices", NOTICES);
    }

    #[test]
    fn info_popups_source_code_opens_the_repository_address() {
        let address = env!("CARGO_PKG_REPOSITORY");
        assert!(address.starts_with("https://"), "{address:?}");
        let (mut h, opens) = launch("about-source");
        open_about(&mut h);
        h.get_by_role_and_label(Role::Button, "Source code").click();
        h.run();
        assert_eq!(
            *opens.0.lock().unwrap(),
            vec![Location::WebAddress(address.to_owned())]
        );
        assert_eq!(open_dialog(&h), Some(DialogKind::About), "About stays open");
        footer_close(&h).click();
        h.run();
        assert_eq!(open_dialog(&h), None);
    }
}
