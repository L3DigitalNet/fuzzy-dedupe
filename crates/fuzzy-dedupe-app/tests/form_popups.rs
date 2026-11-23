// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! PV-T16-001 and PV-T16-002 (plan T16): the Matching rules and New scan
//! popups, driven headless through `egui_kittest` on the real engine facade
//! and the design scenario.
//!
//! Every test sits in the `form_popups` module so `-E 'test(form_popups)'`
//! selects them. Frames run `run_logic` then `draw`, as eframe does; a test
//! that must observe the state between Start scan's dispatch and the
//! scan's first event turns `run_logic` off, so no engine event is drained
//! in between. The folder picker is a fake that answers from a worker
//! thread through a real `PickHandle`; no test opens a portal. Folder paths
//! are this crate's own source directories, which exist wherever the test
//! runs, so no test creates a directory.

mod form_popups {
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};
    use std::time::{SystemTime, UNIX_EPOCH};

    use egui::accesskit::{Role, Toggled};
    use egui::{Event, Key, Modifiers, Vec2};
    use egui_kittest::Harness;
    use egui_kittest::kittest::{NodeT, Queryable};
    use fuzzy_dedupe_app::app::{AppDeps, Clock, FuzzyDedupeApp};
    use fuzzy_dedupe_app::measure::NoMeasurement;
    use fuzzy_dedupe_app::presentation::commands::{IntentQueue, UiIntent};
    use fuzzy_dedupe_app::presentation::state::{
        CompareState, DialogKind, DialogSubject, FindingsState, OperationState, PresentationState,
        QueueState, ScanStage,
    };
    use fuzzy_dedupe_app::ui::dialogs::{DialogCx, DialogResponse, new_scan};
    use fuzzy_dedupe_domain::{
        MatchMode, MatchingRules, MemberDecision, Permille, ScanFolder, ScanSetup,
    };
    use fuzzy_dedupe_engine::service::{EngineConfig, EngineRequest, ReviewCommand};
    use fuzzy_dedupe_platform::dirs::AppDirs;
    use fuzzy_dedupe_platform::fonts::SystemFontLookup;
    use fuzzy_dedupe_platform::log_retention::LogDate;
    use fuzzy_dedupe_platform::open::{Location, OpenHandle, Opener};
    use fuzzy_dedupe_platform::picker::{FolderPicker, PickHandle, PickOutcome, PickRequest};

    // ── Doubles ──────────────────────────────────────────────────────────

    struct NoOpener;

    impl Opener for NoOpener {
        fn open(&self, _: Location, notify: Box<dyn FnOnce() + Send>) -> OpenHandle {
            notify();
            OpenHandle::ready(Ok(()))
        }
    }

    /// Answers every pick with `answer` and records each request.
    #[derive(Clone, Default)]
    struct FakePicker {
        answer: Arc<Mutex<Vec<PathBuf>>>,
        requests: Arc<Mutex<Vec<PickRequest>>>,
    }

    impl FolderPicker for FakePicker {
        fn pick_folders(
            &self,
            request: PickRequest,
            notify: Box<dyn FnOnce() + Send>,
        ) -> PickHandle {
            self.requests.lock().unwrap().push(request);
            let folders = self.answer.lock().unwrap().clone();
            PickHandle::spawn(move || PickOutcome::Chosen(folders), notify)
        }
    }

    struct FixedClock;

    impl Clock for FixedClock {
        fn today(&self) -> LogDate {
            LogDate::new(2026, 11, 23).expect("valid date")
        }
    }

    // ── Harness ─────────────────────────────────────────────────────────

    struct Slot {
        app: Option<FuzzyDedupeApp>,
        logic: bool,
    }

    type Shell = Harness<'static, Slot>;

    fn dirs_for(name: &str) -> AppDirs {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after 1970")
            .as_nanos();
        let root = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("form_popups")
            .join(format!("{name}-{nanos}"));
        let state = root.join("state");
        AppDirs::resolve(
            move |var| (var == "XDG_STATE_HOME").then(|| state.clone().into_os_string()),
            &root.join("home"),
        )
    }

    fn launch(name: &str) -> (Shell, FakePicker) {
        let picker = FakePicker::default();
        let deps = AppDeps {
            dirs: dirs_for(name),
            opener: Box::new(NoOpener),
            picker: Box::new(picker.clone()),
            fonts: Box::new(SystemFontLookup),
            clock: Box::new(FixedClock),
            log_expiry: Box::new(|_| {}),
            engine: EngineConfig::default(),
            measure: Box::new(NoMeasurement),
        };
        let mut harness = Harness::builder()
            .with_size(Vec2::new(1600.0, 960.0))
            .build_ui_state(
                |ui, slot: &mut Slot| {
                    if let Some(app) = slot.app.as_mut() {
                        if slot.logic {
                            app.run_logic(ui.ctx());
                        }
                        app.draw(ui);
                    }
                },
                Slot {
                    app: None,
                    logic: true,
                },
            );
        let app = FuzzyDedupeApp::new(&harness.ctx, deps).expect("engine starts");
        harness.state_mut().app = Some(app);
        harness.run();
        (harness, picker)
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
        for pressed in [true, false] {
            h.input_mut().events.push(Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers,
            });
        }
        h.step();
    }

    fn press(h: &mut Shell, key: Key) {
        chord(h, Modifiers::NONE, key);
    }

    /// Opens `kind` through its global chord (Ctrl+, or Ctrl+N).
    fn open(h: &mut Shell, kind: DialogKind) {
        let key = match kind {
            DialogKind::MatchingRules => Key::Comma,
            DialogKind::NewScan => Key::N,
            other => panic!("{other:?} is not a form popup"),
        };
        chord(h, Modifiers::CTRL, key);
        h.run();
        assert_eq!(open_dialog(h), Some(kind));
    }

    /// Clicks the control, first scrolling it into the popup's view: the
    /// expanded New scan content is taller than the popup and scrolls.
    fn click(h: &mut Shell, role: Role, label: &str) {
        h.get_by_role_and_label(role, label).scroll_to_me();
        h.run();
        h.get_by_role_and_label(role, label).click();
        h.run();
    }

    fn checked(h: &Shell, label: &str) -> bool {
        h.get_by_role_and_label(Role::CheckBox, label)
            .accesskit_node()
            .toggled()
            == Some(Toggled::True)
    }

    /// The minimum-similarity field, by the label it is labelled by.
    fn similarity(h: &Shell) -> egui_kittest::Node<'_> {
        h.get_by_role_and_label(Role::TextInput, "Minimum similarity")
    }

    /// Replaces the minimum-similarity field's text with `text`.
    fn type_similarity(h: &mut Shell, text: &str) {
        similarity(h).focus();
        h.run();
        let select_all = Modifiers {
            ctrl: true,
            command: true,
            ..Modifiers::NONE
        };
        chord(h, select_all, Key::A);
        similarity(h).type_text(text);
        h.run();
    }

    /// Runs frames until `done` holds, for engine and picker work on
    /// worker threads.
    fn run_until(h: &mut Shell, what: &str, done: impl Fn(&PresentationState) -> bool) {
        for _ in 0..500 {
            h.step();
            if done(state(h)) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("timed out waiting for {what}");
    }

    /// Folders that exist wherever the test runs: this crate and two of its
    /// own subdirectories, `src/ui` nested inside `src`.
    fn crate_dir(relative: &str) -> PathBuf {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        if relative.is_empty() {
            root.to_path_buf()
        } else {
            root.join(relative)
        }
    }

    /// The display form the popup uses: the path with one trailing slash,
    /// as in frame `16:806`.
    fn shown(path: &Path) -> String {
        let text = path.display().to_string();
        if text.ends_with('/') {
            text
        } else {
            format!("{text}/")
        }
    }

    fn setup(folders: &[(PathBuf, bool)]) -> ScanSetup {
        ScanSetup {
            folders: folders
                .iter()
                .map(|(path, protect)| ScanFolder {
                    path: path.clone(),
                    protect: *protect,
                })
                .collect(),
            file_types: vec!["txt".to_owned(), "md".to_owned()],
        }
    }

    fn design_rules(h: &Shell) -> MatchingRules {
        state(h).scan.rules.expect("the design scan's stored rules")
    }

    // ── PV-T16-001: drafts, cancel, save, next scan ─────────────────────

    #[test]
    fn form_popups_matching_rules_starts_from_the_current_scans_rules() {
        let (mut h, _p) = launch("rules-initial");
        assert_eq!(state(&h).preferences.saved_rules, None);
        open(&mut h, DialogKind::MatchingRules);
        // The design scan's stored copy, a fixture value (OQ-005).
        assert_eq!(similarity(&h).value().as_deref(), Some("95.0"));
        assert!(checked(&h, "Ignore whitespace differences"));
        assert!(checked(&h, "Ignore line-ending differences"));
        assert!(!checked(&h, "Ignore letter case"));
        assert!(!checked(&h, "Ignore punctuation"));
        h.get_by_label("Changes apply to the next scan.");
        h.get_by_label("Existing results keep their original matching rules.");
    }

    #[test]
    fn form_popups_cancel_close_and_esc_commit_no_matching_rules_edit() {
        let (mut h, _p) = launch("rules-cancel");
        let scan_rules = design_rules(&h);
        let before = state(&h).preferences.clone();
        let dismissals: [fn(&mut Shell); 3] = [
            |h| click(h, Role::Button, "Cancel"),
            |h| click(h, Role::Button, "Close"),
            |h| {
                press(h, Key::Escape);
                h.run();
            },
        ];
        for dismiss in dismissals {
            open(&mut h, DialogKind::MatchingRules);
            click(&mut h, Role::Button, "Decrease minimum similarity");
            click(&mut h, Role::CheckBox, "Ignore letter case");
            assert_eq!(similarity(&h).value().as_deref(), Some("94.0"));
            assert!(checked(&h, "Ignore letter case"), "the draft changed");
            dismiss(&mut h);
            assert_eq!(open_dialog(&h), None);
            assert_eq!(state(&h).preferences, before, "nothing committed");
            assert_eq!(state(&h).scan.rules, Some(scan_rules));
        }
        // A fresh draft each time: the edits above are gone.
        open(&mut h, DialogKind::MatchingRules);
        assert_eq!(similarity(&h).value().as_deref(), Some("95.0"));
        assert!(!checked(&h, "Ignore letter case"));
    }

    #[test]
    fn form_popups_cancel_and_close_commit_no_new_scan_edit() {
        let (mut h, picker) = launch("scan-cancel");
        let folders = [(crate_dir("src"), false), (crate_dir("tests"), true)];
        state_mut(&mut h).preferences.scan_setup = setup(&folders);
        *picker.answer.lock().unwrap() = vec![crate_dir("assets")];
        let before = state(&h).clone();

        for dismiss in ["Cancel", "Close"] {
            open(&mut h, DialogKind::NewScan);
            let src = shown(&folders[0].0);
            click(&mut h, Role::CheckBox, &format!("Protect files in {src}"));
            click(
                &mut h,
                Role::Button,
                &format!("Remove {}", shown(&folders[1].0)),
            );
            click(&mut h, Role::Button, "+ Add folder");
            let added = format!("Remove {}", shown(&crate_dir("assets")));
            for _ in 0..100 {
                h.run();
                if h.query_by_role_and_label(Role::Button, &added).is_some() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            assert!(
                h.query_by_role_and_label(Role::Button, &added).is_some(),
                "the picked folder joined the draft"
            );
            click(&mut h, Role::Button, "Matching rules section");
            click(&mut h, Role::Button, "Decrease minimum similarity");
            click(&mut h, Role::Button, dismiss);

            assert_eq!(open_dialog(&h), None);
            let after = state(&h);
            assert_eq!(after.preferences, before.preferences, "{dismiss}");
            assert_eq!(after.scan, before.scan, "{dismiss}: no scan started");
            assert_eq!(after.queue, before.queue, "{dismiss}: nothing cleared");
        }
        assert_eq!(picker.requests.lock().unwrap().len(), 2);
    }

    #[test]
    fn form_popups_saving_from_the_gear_changes_only_the_saved_rules() {
        let (mut h, _p) = launch("rules-save");
        let scan_rules = design_rules(&h);
        let results = state(&h).scan.results.clone().expect("results");

        h.get_by_role_and_label(Role::Button, "Matching rules")
            .click();
        h.run();
        assert_eq!(open_dialog(&h), Some(DialogKind::MatchingRules));
        type_similarity(&mut h, "90.5");
        click(&mut h, Role::CheckBox, "Ignore punctuation");
        click(&mut h, Role::Button, "Save rules");

        assert_eq!(open_dialog(&h), None);
        let saved = state(&h).preferences.saved_rules.expect("rules saved");
        assert_eq!(saved.minimum_similarity, Permille::new(905).unwrap());
        assert!(saved.ignore.punctuation);
        assert_eq!(saved.mode, scan_rules.mode);
        assert_ne!(saved.revision, scan_rules.revision, "a new saved version");
        // The current scan's stored copy, and its evidence, are untouched.
        assert_eq!(state(&h).scan.rules, Some(scan_rules));
        assert_eq!(results.rules, scan_rules);
        assert_eq!(
            state(&h).scan.results.as_ref().expect("results").rules,
            scan_rules
        );
        // Focus returns to the gear.
        assert!(
            h.get_by_role_and_label(Role::Button, "Matching rules")
                .is_focused()
        );

        // The next Matching rules draft starts from the saved rules.
        open(&mut h, DialogKind::MatchingRules);
        assert_eq!(similarity(&h).value().as_deref(), Some("90.5"));
        assert!(checked(&h, "Ignore punctuation"));
    }

    #[test]
    fn form_popups_the_next_start_scan_stores_the_saved_rules() {
        let (mut h, _p) = launch("rules-next-scan");
        let previous = state(&h).scan.results.clone().expect("results");
        let previous_rules = previous.rules;
        state_mut(&mut h).preferences.scan_setup = setup(&[(crate_dir("src"), false)]);

        open(&mut h, DialogKind::MatchingRules);
        type_similarity(&mut h, "80");
        click(&mut h, Role::Button, "Save rules");
        let saved = state(&h).preferences.saved_rules.expect("saved");
        assert_eq!(saved.minimum_similarity, Permille::new(800).unwrap());

        open(&mut h, DialogKind::NewScan);
        h.get_by_label_contains("80% similar");
        click(&mut h, Role::Button, "Start scan");
        assert_eq!(open_dialog(&h), None);
        assert_eq!(state(&h).scan.rules, Some(saved), "admitted with them");
        run_until(&mut h, "the new scan's results", |s| {
            s.scan.stage == ScanStage::Completed
                && s.scan
                    .results
                    .as_ref()
                    .is_some_and(|r| r.scan != previous.scan)
        });
        let results = state(&h).scan.results.clone().expect("results");
        assert_eq!(results.rules, saved, "the new scan stored the saved rules");
        assert_eq!(
            previous.rules, previous_rules,
            "old evidence kept its rules"
        );
        assert_eq!(state(&h).preferences.saved_rules, Some(saved));
    }

    #[test]
    fn form_popups_minimum_similarity_is_validated_against_0_to_100() {
        let (mut h, _p) = launch("rules-validate");
        open(&mut h, DialogKind::MatchingRules);
        for bad in ["100.1", "-1", "abc", "", "95.05"] {
            type_similarity(&mut h, bad);
            click(&mut h, Role::Button, "Save rules");
            assert_eq!(open_dialog(&h), Some(DialogKind::MatchingRules), "{bad:?}");
            h.get_by_label("Enter a minimum similarity from 0 to 100%, to one decimal place.");
            assert_eq!(state(&h).preferences.saved_rules, None, "{bad:?}");
            // Enter, the default action, refuses the same way.
            press(&mut h, Key::Enter);
            h.run();
            assert_eq!(open_dialog(&h), Some(DialogKind::MatchingRules), "{bad:?}");
            assert_eq!(state(&h).preferences.saved_rules, None, "{bad:?}");
        }
        for (good, permille) in [("0", 0), ("100", 1000), ("100.0", 1000), ("12.3", 123)] {
            if open_dialog(&h).is_none() {
                open(&mut h, DialogKind::MatchingRules);
            }
            type_similarity(&mut h, good);
            click(&mut h, Role::Button, "Save rules");
            assert_eq!(open_dialog(&h), None, "{good:?}");
            let saved = state(&h).preferences.saved_rules.expect("saved");
            assert_eq!(saved.minimum_similarity.get(), permille, "{good:?}");
        }
        // The steps stop at the bounds instead of leaving the range.
        open(&mut h, DialogKind::MatchingRules);
        type_similarity(&mut h, "99.5");
        for _ in 0..2 {
            click(&mut h, Role::Button, "Increase minimum similarity");
            assert_eq!(similarity(&h).value().as_deref(), Some("100.0"));
        }
        type_similarity(&mut h, "0.5");
        for _ in 0..2 {
            click(&mut h, Role::Button, "Decrease minimum similarity");
            assert_eq!(similarity(&h).value().as_deref(), Some("0.0"));
        }
    }

    #[test]
    fn form_popups_match_mode_offers_the_two_spec_modes() {
        let (mut h, _p) = launch("rules-mode");
        open(&mut h, DialogKind::MatchingRules);
        h.get_by_role_and_label(Role::ComboBox, "Match mode")
            .click();
        h.run();
        click(&mut h, Role::Button, "Exact copies only");
        click(&mut h, Role::Button, "Save rules");
        let saved = state(&h).preferences.saved_rules.expect("saved");
        assert_eq!(saved.mode, MatchMode::ExactOnly);
    }

    // ── PV-T16-002: the new scan's clear, count and folder problems ─────

    #[test]
    fn form_popups_new_scan_states_the_exact_queued_count() {
        let (mut h, _p) = launch("scan-count");
        assert_eq!(state(&h).queue.queued_count(), 3, "design scenario");
        open(&mut h, DialogKind::NewScan);
        h.get_by_label("Starting a scan replaces the current results.");
        h.get_by_label("3 queued files will be cleared from the cleanup queue.");
        press(&mut h, Key::Escape);
        h.run();

        // One fewer queued file changes the stated count with it.
        let file = *state(&h).queue.entries.keys().next().expect("queued");
        h.state_mut()
            .app
            .as_mut()
            .expect("app")
            .push_intent(UiIntent::Review {
                command: ReviewCommand::RemoveFromQueue { file },
                invoker: None,
            });
        h.run();
        assert_eq!(state(&h).queue.queued_count(), 2);
        open(&mut h, DialogKind::NewScan);
        h.get_by_label("2 queued files will be cleared from the cleanup queue.");
    }

    #[test]
    fn form_popups_start_scan_clears_every_decision_and_keeps_folder_protection() {
        let (mut h, _p) = launch("scan-clear");
        let folders = [(crate_dir("src"), true), (crate_dir("tests"), false)];
        state_mut(&mut h).preferences.scan_setup = setup(&folders);
        let previous = state(&h).scan.scan;
        let decisions = &state(&h).queue.decisions;
        assert!(decisions.values().any(|d| *d == MemberDecision::Kept));
        assert!(decisions.values().any(|d| *d == MemberDecision::Queued));
        let view = state(&h).queue.view.clone().expect("view");
        let folder_protected: Vec<_> = state(&h)
            .scan
            .results
            .as_ref()
            .expect("results")
            .groups
            .iter()
            .flat_map(|g| g.members.iter())
            .filter(|m| m.folder_protected)
            .map(|m| m.file)
            .collect();
        assert!(
            view.protected.len() > folder_protected.len(),
            "the design scenario holds a per-file Protect"
        );

        open(&mut h, DialogKind::NewScan);
        // No engine event may be drained between the clear and this check.
        h.state_mut().logic = false;
        click(&mut h, Role::Button, "Start scan");
        assert_eq!(open_dialog(&h), None, "Start scan closed the popup");

        let s = state(&h);
        assert_ne!(s.scan.scan, previous, "a new scan was admitted");
        assert_eq!(s.scan.stage, ScanStage::Running);
        assert!(s.queue.entries.is_empty(), "the queue is empty");
        let view = s.queue.view.clone().expect("view");
        assert!(view.queue.is_empty(), "the engine's queue is empty");
        let mut protected: Vec<_> = view.protected.iter().copied().collect();
        let mut expected = folder_protected.clone();
        protected.sort();
        expected.sort();
        assert_eq!(protected, expected, "only folder protection survives");
        for (file, decision) in &s.queue.decisions {
            assert!(
                !matches!(decision, MemberDecision::Kept | MemberDecision::Queued),
                "{file:?} kept {decision:?}"
            );
            if *decision == MemberDecision::Protected {
                assert!(folder_protected.contains(file), "{file:?} per-file Protect");
            }
        }
        // Folder protection belongs to the setup and stays there.
        let stored = s.scan.setup.clone().expect("the new scan's setup");
        assert_eq!(stored, setup(&folders));
        assert_eq!(s.preferences.scan_setup, setup(&folders));

        h.state_mut().logic = true;
        run_until(&mut h, "the new scan's results", |s| {
            s.scan.stage == ScanStage::Completed && s.scan.scan != previous
        });
        let s = state(&h);
        assert!(s.queue.entries.is_empty());
        assert!(
            !s.queue
                .decisions
                .values()
                .any(|d| matches!(d, MemberDecision::Kept | MemberDecision::Queued))
        );
        assert_eq!(
            s.scan.results.as_ref().expect("results").setup,
            setup(&folders)
        );
    }

    #[test]
    fn form_popups_start_scan_sends_close_clear_then_start_in_that_order() {
        let (h, _p) = launch("scan-order");
        let s = state(&h);
        let mut preferences = s.preferences.clone();
        preferences.scan_setup = setup(&[(crate_dir("src"), true)]);
        let mut intents = IntentQueue::default();
        let mut picked = None;
        let subject = DialogSubject::None;
        let (findings, compare, operation) = (
            FindingsState::default(),
            CompareState::default(),
            OperationState::default(),
        );
        let queue: QueueState = s.queue.clone();
        let mut cx = DialogCx {
            subject: &subject,
            findings: &findings,
            compare: &compare,
            queue: &queue,
            scan: &s.scan,
            operation: &operation,
            preferences: &preferences,
            picked_folders: &mut picked,
            intents: &mut intents,
        };
        let mut draft = new_scan::open(&mut cx);
        assert_eq!(new_scan::submit(&mut draft, &mut cx), DialogResponse::Close);
        let sent = intents.drain();
        let clear = sent
            .iter()
            .position(|i| {
                matches!(
                    i,
                    UiIntent::Review {
                        command: ReviewCommand::ClearForNewScan,
                        ..
                    }
                )
            })
            .expect("the clear-for-new-scan command");
        let start = sent
            .iter()
            .position(|i| matches!(i, UiIntent::Submit(EngineRequest::StartScan(_))))
            .expect("StartScan");
        assert!(clear < start, "clear before start: {sent:?}");
        assert!(
            sent.contains(&UiIntent::SetScanSetup(setup(&[(crate_dir("src"), true)]))),
            "the setup is committed: {sent:?}"
        );
        let UiIntent::Submit(EngineRequest::StartScan(request)) = &sent[start] else {
            unreachable!()
        };
        assert_eq!(request.setup, setup(&[(crate_dir("src"), true)]));
        assert_eq!(Some(request.rules), s.scan.rules);
    }

    #[test]
    fn form_popups_missing_and_nested_folders_show_their_inline_problem() {
        let (mut h, picker) = launch("scan-problems");
        let missing = crate_dir("no-such-folder-for-form-popups");
        state_mut(&mut h).preferences.scan_setup =
            setup(&[(crate_dir("src"), true), (missing.clone(), false)]);
        *picker.answer.lock().unwrap() = vec![crate_dir("src/ui")];

        open(&mut h, DialogKind::NewScan);
        h.get_by_label("Folder not found. Remove it or choose another folder.");
        assert!(
            h.query_by_label_contains("scanned once").is_none(),
            "no nested folder yet"
        );

        click(&mut h, Role::Button, "+ Add folder");
        let nested = format!("Inside {} — scanned once", shown(&crate_dir("src")));
        for _ in 0..100 {
            h.run();
            if h.query_by_label(&nested).is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        h.get_by_label(&nested);
        // Inside a protected folder, its files are protected through it.
        let inner = format!("Protect files in {}", shown(&crate_dir("src/ui")));
        assert!(checked(&h, &inner));

        // A missing folder blocks Start scan; removing it unblocks it.
        let before = state(&h).scan.clone();
        click(&mut h, Role::Button, "Start scan");
        assert_eq!(open_dialog(&h), Some(DialogKind::NewScan));
        assert_eq!(state(&h).scan, before, "no scan started");
        click(&mut h, Role::Button, &format!("Remove {}", shown(&missing)));
        assert!(
            h.query_by_label("Folder not found. Remove it or choose another folder.")
                .is_none()
        );
        click(&mut h, Role::Button, "Start scan");
        assert_eq!(open_dialog(&h), None);
        let committed = &state(&h).preferences.scan_setup;
        assert_eq!(
            committed.folders,
            vec![
                ScanFolder {
                    path: crate_dir("src"),
                    protect: true
                },
                ScanFolder {
                    path: crate_dir("src/ui"),
                    protect: false
                },
            ],
            "removing a folder changed only the setup"
        );
        assert!(crate_dir("").join("src").is_dir(), "no source file touched");
    }
}
