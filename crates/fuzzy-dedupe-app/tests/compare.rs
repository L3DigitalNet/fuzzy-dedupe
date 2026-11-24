// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! PV-T15-001 to PV-T15-004 (plan T15): the Compare workspace and the
//! return to Findings, driven headless through `egui_kittest`.
//!
//! Every test sits in the `compare_workspace` module, so
//! `-E 'test(compare_workspace)'` selects exactly these. The plan's
//! `test(compare)` filter also selects T35's `compare_orientation` tests,
//! which Compare must keep green as well.
//!
//! The rig runs the app's own frame order on a scenario adopted through
//! `app::adopt_scenario`, so any fixture scenario can be loaded (the app
//! itself always starts on `design`): engine events drained and applied,
//! focus restore, global chords, menu keys, the shell, the dialog host,
//! then intent dispatch with follow-ups. Engine work arrives from worker
//! threads, so every wait on it is a bounded `run_until`.
//!
//! The strings looked for are the spec's own (§11.5) and the frames'
//! (`13:1304`); they are literals here rather than the view's constants,
//! so a renamed control fails these tests instead of following silently.

mod compare_workspace {
    use std::path::Path;
    use std::time::Duration;

    use egui::accesskit::Role;
    use egui::{Event, Key, Modifiers, OutputCommand, Vec2};
    use egui_kittest::Harness;
    use egui_kittest::kittest::{NodeT as _, Queryable};
    use fuzzy_dedupe_app::app::adopt_scenario;
    use fuzzy_dedupe_app::presentation::commands::{self, IntentQueue, Services, UiIntent};
    use fuzzy_dedupe_app::presentation::events;
    use fuzzy_dedupe_app::presentation::state::{
        ColumnId, MenuKind, PresentationState, RowId, SortKey, WorkspaceState,
    };
    use fuzzy_dedupe_app::ui::widgets::dialog_host;
    use fuzzy_dedupe_app::ui::{glyphs, shell, theme};
    use fuzzy_dedupe_domain::{
        FileId, GroupId, MemberDecision, QueueOrigin, Refusal, RelationKind, ScanResults,
    };
    use fuzzy_dedupe_engine::compare::AlignedComparison;
    use fuzzy_dedupe_engine::fixtures::{self, ScenarioKind, design};
    use fuzzy_dedupe_engine::service::{Engine, EngineConfig, EngineRequest, ReviewCommand};
    use fuzzy_dedupe_platform::dirs::AppDirs;
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

    // ── Doubles ──────────────────────────────────────────────────────────

    struct NoOpener;

    impl Opener for NoOpener {
        fn open(&self, _: Location, notify: Box<dyn FnOnce() + Send>) -> OpenHandle {
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

    // ── The rig ──────────────────────────────────────────────────────────

    struct Rig {
        state: PresentationState,
        services: Services,
        intents: IntentQueue,
        /// The harness runs a frame while it is built, before the theme is
        /// installed; the rig draws nothing until then.
        ready: bool,
        /// Stands in for an engine whose request queue is full: each
        /// `LoadComparison` is refused as `SubmitError::Busy` would be (not
        /// admitted, so the slot is unchanged) and counted. The engine
        /// facade has no injectable busy state, and its dispatcher drains
        /// the job channel too fast to hold it full from a test.
        busy: bool,
        refused: usize,
    }

    impl Rig {
        /// `FuzzyDedupeApp::run_logic` then `draw`, minus preferences,
        /// logs and measurement, which Compare does not touch.
        fn frame(&mut self, ui: &mut egui::Ui) {
            if !self.ready {
                return;
            }
            let ctx = ui.ctx().clone();
            for event in self.services.engine.drain_events(256) {
                events::apply(&mut self.state, event);
            }
            self.services.poll(&mut self.state);
            dialog_host::begin_frame(&ctx, &mut self.state.dialog);
            commands::read_global_chords(
                &ctx,
                &self.state,
                Some(shell::menu_entry_id()),
                &mut self.intents,
            );
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
                    if self.busy
                        && matches!(intent, UiIntent::Submit(EngineRequest::LoadComparison(_)))
                    {
                        self.refused += 1;
                        continue;
                    }
                    commands::dispatch(
                        intent,
                        &mut self.state,
                        &mut self.services,
                        &ctx,
                        &mut followups,
                    );
                }
                intents = followups.drain();
                ctx.request_repaint();
            }
        }
    }

    type Ui = Harness<'static, Rig>;

    fn launch(kind: ScenarioKind, size: Vec2) -> Ui {
        launch_with_dt(kind, size, None)
    }

    /// [`launch`] with kittest's step length set, for tests that count
    /// frames against egui's clock; the default step is 0.25 s.
    fn launch_with_dt(kind: ScenarioKind, size: Vec2, step_dt: Option<f32>) -> Ui {
        let engine = Engine::new(EngineConfig::default()).expect("engine starts");
        let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("compare_workspace");
        let mut services = Services::new(
            engine,
            Box::new(NoOpener),
            Box::new(DismissPicker),
            AppDirs::resolve(|_| None, &root.join("home")),
        );
        let mut state = PresentationState::default();
        adopt_scenario(&mut services, &mut state, kind);
        let rig = Rig {
            state,
            services,
            intents: IntentQueue::default(),
            ready: false,
            busy: false,
            refused: 0,
        };
        let mut builder = Harness::builder()
            .with_max_steps(SETTLE_STEPS)
            .with_size(size);
        if let Some(dt) = step_dt {
            builder = builder.with_step_dt(dt);
        }
        let mut harness = builder.build_ui_state(|ui, rig: &mut Rig| rig.frame(ui), rig);
        theme::install(&harness.ctx, Vec::new());
        glyphs::install(&harness.ctx);
        harness.state_mut().ready = true;
        settle(&mut harness);
        harness
    }

    fn launch_design() -> Ui {
        launch(ScenarioKind::Design, Vec2::new(1600.0, 960.0))
    }

    /// Enough frames for input, dispatch, the frame that draws the result,
    /// glyph registration and egui's scroll animation.
    fn settle(h: &mut Ui) {
        for _ in 0..24 {
            h.step();
        }
    }

    fn state(h: &Ui) -> &PresentationState {
        &h.state().state
    }

    fn state_mut(h: &mut Ui) -> &mut PresentationState {
        &mut h.state_mut().state
    }

    fn push(h: &mut Ui, intent: UiIntent) {
        h.state_mut().intents.push(intent);
    }

    /// Runs frames until `done` holds, at most 500 × 10 ms, for state that
    /// arrives from the engine's worker threads.
    fn run_until(h: &mut Ui, what: &str, done: impl Fn(&PresentationState) -> bool) {
        for _ in 0..500 {
            h.step();
            if done(state(h)) {
                settle(h);
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("timed out waiting for {what}");
    }

    /// Presses and releases `key` in exactly one frame, then settles.
    fn key(h: &mut Ui, modifiers: Modifiers, key: Key) {
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
        settle(h);
    }

    fn click(h: &mut Ui, role: Role, label: &str) {
        h.get_by_role_and_label(role, label).click();
        settle(h);
    }

    fn button(h: &mut Ui, label: &str) {
        click(h, Role::Button, label);
    }

    fn enabled(h: &Ui, role: Role, label: &str) -> bool {
        !h.get_by_role_and_label(role, label)
            .accesskit_node()
            .is_disabled()
    }

    fn results(h: &Ui) -> std::sync::Arc<ScanResults> {
        state(h).scan.results.clone().expect("results")
    }

    fn file_named(results: &ScanResults, name: &str) -> FileId {
        results
            .groups
            .iter()
            .flat_map(|g| g.members.iter())
            .find(|m| m.name == name)
            .unwrap_or_else(|| panic!("{name} in the scenario"))
            .file
    }

    fn group_of(results: &ScanResults, file: FileId) -> GroupId {
        results.group_of(file).expect("a member").id
    }

    fn aligned(h: &Ui) -> AlignedComparison {
        state(h)
            .compare
            .comparison
            .aligned()
            .expect("comparison ready")
            .clone()
    }

    /// Opens Compare on `target` against `counterpart`, as Enter on a row
    /// does, and waits for the comparison to be drawn.
    fn open(h: &mut Ui, target: FileId, counterpart: FileId) {
        let group = group_of(&results(h), target);
        push(
            h,
            UiIntent::OpenCompare {
                target,
                counterpart,
                return_row: Some(RowId::Member {
                    group,
                    file: target,
                }),
            },
        );
        run_until(h, "Compare's comparison", move |s| {
            s.workspace == WorkspaceState::Compare
                && s.compare.comparison.aligned().is_some()
                && s.compare
                    .comparison
                    .key
                    .is_some_and(|k| (k.target, k.counterpart) == (target, counterpart))
        });
    }

    /// The design pair of frame `13:1304`: (draft, the target; reference,
    /// its counterpart).
    fn open_design_pair(h: &mut Ui) -> (FileId, FileId) {
        let pair = design::sample().last_compared.expect("design pair");
        open(h, pair.b, pair.a);
        (pair.b, pair.a)
    }

    fn copied(h: &Ui) -> Vec<String> {
        h.output()
            .platform_output
            .commands
            .iter()
            .filter_map(|c| match c {
                OutputCommand::CopyText(text) => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// The pinned identity row whose name starts with `prefix`.
    fn identity(h: &Ui, prefix: &str) {
        let owned = prefix.to_owned();
        h.query_by(move |n| {
            let named = |text: Option<String>| text.is_some_and(|t| t.starts_with(owned.as_str()));
            named(n.label()) || named(n.value())
        })
        .unwrap_or_else(|| panic!("an identity row named {prefix}…"));
    }

    fn only_in(letter: &str, count: u64) -> String {
        let unit = if count == 1 { "line" } else { "lines" };
        format!("Only in {letter}: {count} {unit} ▾")
    }

    // ── PV-T15-001: the seven return-context properties ──────────────────

    /// The seven IR-001 properties, compared exactly (the scroll offset by
    /// its bits: the round trip must give back the very value recorded).
    #[derive(Debug, PartialEq)]
    struct ReturnContext {
        search: String,
        match_filter: Option<RelationKind>,
        sort: SortKey,
        expanded: std::collections::BTreeSet<GroupId>,
        focused: Option<RowId>,
        column_widths: std::collections::BTreeMap<ColumnId, f32>,
        scroll_bits: u32,
    }

    fn return_context(h: &Ui) -> ReturnContext {
        let f = &state(h).findings;
        ReturnContext {
            search: f.search.clone(),
            match_filter: f.match_filter,
            sort: f.sort,
            expanded: f.expanded.clone(),
            focused: f.focused,
            column_widths: f.column_widths.clone(),
            scroll_bits: f.scroll_offset.to_bits(),
        }
    }

    /// The header of the pair's group reads this once Compare has shown it
    /// (frame `13:648`); it is the one label a round trip is meant to add.
    const LAST_COMPARED: &str = "Last compared";

    /// Every visible label with its top edge, except [`LAST_COMPARED`]: the
    /// drawn table, which the scroll offset moves.
    fn table_layout(h: &Ui) -> Vec<(String, u32)> {
        let mut out: Vec<(String, u32)> = h
            .query_all_by_role(Role::Label)
            .filter_map(|n| {
                n.accesskit_node()
                    .value()
                    .filter(|v| v.as_str() != LAST_COMPARED)
                    .map(|v| (v.clone(), n.rect().top().to_bits()))
            })
            .collect();
        out.sort();
        out
    }

    /// Presses and releases the primary button at `node`'s centre over two
    /// frames, as a pointer does; the diff widget reads the press origin.
    /// Row 12 is used because opening Compare scrolls the panes to the
    /// first change with three rows of context, so the first rows are laid
    /// out above the view and a press there misses the pane.
    fn press_node(h: &mut Ui, author: &str) {
        let pos = h
            .query_by(|n| n.author_id() == Some(author))
            .unwrap_or_else(|| panic!("{author} is drawn"))
            .rect()
            .center();
        for pressed in [true, false] {
            h.event(Event::PointerMoved(pos));
            h.event(Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            });
            h.step();
        }
        settle(h);
    }

    /// A short window on the design scenario with the table filtered,
    /// sorted, fully expanded, resized and scrolled to its end, which also
    /// focuses the last row; returns once Compare shows that row.
    fn enter_from_a_prepared_table() -> (Ui, ReturnContext, Vec<(String, u32)>) {
        let mut h = launch(ScenarioKind::Design, Vec2::new(1600.0, 640.0));
        let groups = results(&h).groups.iter().map(|g| g.id).collect();
        {
            let f = &mut state_mut(&mut h).findings;
            f.match_filter = Some(RelationKind::SimilarText);
            "Documents".clone_into(&mut f.search);
            f.sort = SortKey::GroupSize;
            f.expanded = groups;
            f.column_widths.insert(ColumnId::Folder, 231.0);
            f.column_widths.insert(ColumnId::FileName, 302.5);
        }
        settle(&mut h);
        // A member row takes focus by pointer; End then scrolls the table
        // and moves focus to its last row.
        let projection = state(&h).findings.projection.clone().expect("projection");
        let first_member = projection
            .rows
            .iter()
            .find_map(|row| match row.id {
                RowId::Member { file, .. } => Some(file),
                RowId::Group(_) => None,
            })
            .expect("a member row");
        let name = results(&h)
            .group_of(first_member)
            .and_then(|g| g.member(first_member))
            .expect("member")
            .name
            .clone();
        h.query_all_by_label(&name)
            .min_by(|a, b| a.rect().top().total_cmp(&b.rect().top()))
            .expect("row drawn")
            .click();
        settle(&mut h);
        key(&mut h, Modifiers::NONE, Key::End);
        let before = return_context(&h);
        assert!(
            f32::from_bits(before.scroll_bits) > 0.0,
            "the table scrolled"
        );
        assert!(
            matches!(before.focused, Some(RowId::Member { .. })),
            "End focused a member row: {:?}",
            before.focused
        );
        let drawn = table_layout(&h);

        key(&mut h, Modifiers::NONE, Key::Enter);
        run_until(&mut h, "Compare from Enter", |s| {
            s.workspace == WorkspaceState::Compare && s.compare.comparison.aligned().is_some()
        });
        let Some(RowId::Member { file, .. }) = before.focused else {
            unreachable!()
        };
        assert_eq!(state(&h).compare.pair.expect("pair").target, file);
        (h, before, drawn)
    }

    #[test]
    fn compare_workspace_esc_returns_with_all_seven_properties_restored() {
        let (mut h, before, drawn) = enter_from_a_prepared_table();
        assert_eq!(return_context(&h), before, "untouched while Compare shows");

        key(&mut h, Modifiers::NONE, Key::Escape);
        assert_eq!(state(&h).workspace, WorkspaceState::Findings);
        assert_eq!(return_context(&h), before);
        assert_eq!(table_layout(&h), drawn, "the table shows the same rows");
        h.get_by_label(LAST_COMPARED);
    }

    #[test]
    fn compare_workspace_back_button_returns_with_all_seven_properties_restored() {
        let (mut h, before, drawn) = enter_from_a_prepared_table();
        button(&mut h, "‹ Findings");
        assert_eq!(state(&h).workspace, WorkspaceState::Findings);
        assert_eq!(return_context(&h), before);
        assert_eq!(table_layout(&h), drawn);
    }

    // ── PV-T15-002: queued IDs and targets survive view changes ──────────

    /// Each queued target by ID with its recorded counterpart and its path.
    fn queued(h: &Ui) -> Vec<(FileId, FileId, String)> {
        let results = results(h);
        state(h)
            .queue
            .entries
            .iter()
            .map(|(file, entry)| {
                let path = results
                    .group_of(*file)
                    .and_then(|g| g.member(*file))
                    .expect("a queued member")
                    .display_path
                    .clone();
                (*file, entry.counterpart, path)
            })
            .collect()
    }

    #[test]
    fn compare_workspace_queue_is_unchanged_by_sort_filter_collapse_and_a_round_trip() {
        let mut h = launch_design();
        let before = queued(&h);
        assert!(before.len() >= 2, "the design scenario queues files");

        // Sort through the toolbar menu, filter to a type that hides some
        // queued rows, collapse every group.
        button(&mut h, "Sort: Match type");
        button(&mut h, "Group size");
        assert_eq!(state(&h).findings.sort, SortKey::GroupSize);
        state_mut(&mut h).findings.match_filter = Some(RelationKind::SimilarText);
        settle(&mut h);
        state_mut(&mut h).findings.expanded.clear();
        settle(&mut h);
        assert_eq!(queued(&h), before);

        let (target, _) = open_design_pair(&mut h);
        assert_eq!(queued(&h), before);
        key(&mut h, Modifiers::NONE, Key::Escape);
        assert_eq!(state(&h).workspace, WorkspaceState::Findings);
        assert_eq!(queued(&h), before, "same IDs, counterparts and paths");
        assert!(!state(&h).queue.is_queued(target));
    }

    // ── PV-T15-003: read-only panes, selection and copy ──────────────────

    /// Labels of controls that must not exist anywhere in Compare (§11.5:
    /// no Save, merge arrows, editable cursor or AI control).
    const FORBIDDEN: [&str; 9] = [
        "save", "merge", "edit", "apply", "accept", "revert", "ai", "→", "←",
    ];

    #[test]
    fn compare_workspace_edit_attempts_change_no_text_and_no_edit_control_exists() {
        let mut h = launch_design();
        open_design_pair(&mut h);
        let model = aligned(&h);
        let text = (
            model.a.original.text.to_string(),
            model.b.original.text.to_string(),
        );

        // No text input of any kind, and no editing, merge or AI control.
        for role in [Role::TextInput, Role::MultilineTextInput] {
            assert_eq!(h.query_all_by_role(role).count(), 0, "{role:?}");
        }
        for node in h.query_all_by_role(Role::Button) {
            let label = node.accesskit_node().label().unwrap_or_default();
            let words: Vec<String> = label
                .to_lowercase()
                .split(|c: char| !c.is_alphanumeric() && c != '→' && c != '←')
                .map(str::to_owned)
                .collect();
            for word in FORBIDDEN {
                assert!(
                    !words.iter().any(|w| w == word),
                    "button {label:?} looks like an edit control"
                );
            }
        }

        // Pointer into pane B, then typing, deleting, pasting and cutting.
        press_node(&mut h, "diff_view:b:12");
        h.input_mut().events.push(Event::Text("typed".to_owned()));
        h.input_mut().events.push(Event::Paste("pasted".to_owned()));
        h.input_mut().events.push(Event::Cut);
        h.step();
        for k in [Key::Backspace, Key::Delete, Key::Enter, Key::Tab] {
            key(&mut h, Modifiers::NONE, k);
        }
        key(&mut h, Modifiers::COMMAND, Key::V);
        key(&mut h, Modifiers::COMMAND, Key::X);
        settle(&mut h);

        let after = aligned(&h);
        assert_eq!(
            (
                after.a.original.text.to_string(),
                after.b.original.text.to_string()
            ),
            text,
            "no text changed"
        );
        assert_eq!(state(&h).workspace, WorkspaceState::Compare);
    }

    #[test]
    fn compare_workspace_select_all_and_copy_return_one_panes_original_text() {
        let mut h = launch_design();
        open_design_pair(&mut h);
        let model = aligned(&h);

        press_node(&mut h, "diff_view:b:12");
        h.key_press_modifiers(Modifiers::COMMAND, Key::A);
        h.step();
        h.event(Event::Copy);
        h.step();
        assert_eq!(copied(&h), vec![model.b.original.text.to_string()]);

        // F6 moves to pane A; its selection and copy are A's alone.
        h.key_press(Key::F6);
        h.step();
        h.key_press_modifiers(Modifiers::COMMAND, Key::A);
        h.step();
        h.event(Event::Copy);
        h.step();
        assert_eq!(copied(&h), vec![model.a.original.text.to_string()]);
    }

    // ── PV-T15-004: controls, no pair, stale evidence, refusal ───────────

    #[test]
    fn compare_workspace_shows_the_spec_control_set() {
        let mut h = launch_design();
        let (target, counterpart) = open_design_pair(&mut h);
        let model = aligned(&h);
        let results = results(&h);
        let a = results
            .group_of(counterpart)
            .and_then(|g| g.member(counterpart))
            .expect("reference");
        let b = results
            .group_of(target)
            .and_then(|g| g.member(target))
            .expect("draft");

        // Toolbar.
        for name in [
            "‹ Findings",
            "Previous candidate",
            "Next candidate",
            "Keep both",
            "Queue B for cleanup…",
            "Review cleanup (3)",
        ] {
            h.get_by_role_and_label(Role::Button, name);
        }
        h.get_by_label("G004");
        h.get_by_label("Design_Spec.txt  ↔  Design_Spec_draft.txt");
        h.get_by_label("Candidate 1 of 2");

        // Evidence row: A is the counterpart, B the target (T35).
        h.get_by_label("Similar text");
        h.get_by_label("97.9% similar");
        h.get_by_role_and_label(Role::Button, &only_in("A", model.only_in_a.count()));
        h.get_by_role_and_label(Role::Button, &only_in("B", model.only_in_b.count()));
        h.get_by_label("Read-only · similarity is not deletion safety");

        // Change row.
        let hunks = model.hunks.len();
        h.get_by_label(&format!("Change 1 of {hunks}"));
        h.get_by_role_and_label(Role::Button, "‹ Previous");
        h.get_by_role_and_label(Role::Button, "Next ›");
        h.get_by_role_and_label(Role::CheckBox, "Wrap");
        h.get_by_role_and_label(Role::CheckBox, "Linked scroll");
        h.get_by_role_and_label(Role::Button, "Original");
        h.get_by_role_and_label(Role::Button, "Normalized");

        // Pinned file identity above each pane (frame `13:1410`), one node
        // per row: side and role, name, path, size and date.
        identity(
            &h,
            &format!("A · Reference: {}, {}, ", a.name, a.display_path),
        );
        identity(
            &h,
            &format!("B · Review first: {}, {}, ", b.name, b.display_path),
        );

        // The change row moves the widget's active change.
        button(&mut h, "Next ›");
        assert_eq!(state(&h).compare.active_change, 1.min(hunks - 1));
        key(&mut h, Modifiers::ALT, Key::ArrowUp);
        assert_eq!(state(&h).compare.active_change, 0);
        key(&mut h, Modifiers::ALT, Key::ArrowDown);
        assert_eq!(state(&h).compare.active_change, 1.min(hunks - 1));

        // Wrap and Linked scroll persist as preferences (§11.5 rule 5).
        let wrap = state(&h).compare.wrap;
        click(&mut h, Role::CheckBox, "Wrap");
        assert_eq!(state(&h).compare.wrap, !wrap);
        assert_eq!(state(&h).preferences.compare_wrap, !wrap);
        key(&mut h, Modifiers::ALT, Key::L);
        assert!(!state(&h).compare.linked_scroll);
        assert!(!state(&h).preferences.compare_linked_scroll);

        // Normalized view requests its own comparison and labels counts.
        button(&mut h, "Normalized");
        run_until(&mut h, "the Normalized comparison", |s| {
            s.compare
                .comparison
                .aligned()
                .is_some_and(|a| a.mode == fuzzy_dedupe_domain::CompareMode::Normalized)
        });
        let normalized = aligned(&h);
        let count = normalized.only_in_b.count();
        let unit = if count == 1 { "line" } else { "lines" };
        h.get_by_role_and_label(
            Role::Button,
            &format!("Only in B: {count} {unit} (normalized) ▾"),
        );
    }

    #[test]
    fn compare_workspace_candidate_stepper_re_aims_the_pair() {
        let mut h = launch_design();
        let (target, counterpart) = open_design_pair(&mut h);
        let rev = file_named(&results(&h), "Design_Spec_rev.txt");
        assert!(!enabled(&h, Role::Button, "Previous candidate"));

        button(&mut h, "Next candidate");
        run_until(&mut h, "the next candidate's comparison", move |s| {
            s.compare
                .comparison
                .aligned()
                .is_some_and(|a| a.a.file == rev)
        });
        let pair = state(&h).compare.pair.expect("pair");
        assert_eq!((pair.target, pair.counterpart), (target, rev));
        h.get_by_label("Candidate 2 of 2");
        h.get_by_label("Design_Spec_rev.txt  ↔  Design_Spec_draft.txt");
        assert!(!enabled(&h, Role::Button, "Next candidate"));

        key(&mut h, Modifiers::ALT, Key::ArrowLeft);
        run_until(&mut h, "the first candidate again", move |s| {
            s.compare
                .comparison
                .aligned()
                .is_some_and(|a| a.a.file == counterpart)
        });
        assert_eq!(
            state(&h).compare.pair.expect("pair").counterpart,
            counterpart
        );
        h.get_by_label("Candidate 1 of 2");
    }

    #[test]
    fn compare_workspace_only_in_menus_go_through_the_overlay_slot() {
        let mut h = launch_design();
        open_design_pair(&mut h);
        let model = aligned(&h);

        button(&mut h, &only_in("B", model.only_in_b.count()));
        assert_eq!(state(&h).dialog.menu(), Some(MenuKind::OnlyIn { b: true }));
        for item in [
            "Jump to first line only in B",
            "Copy those lines",
            "Review unique text…",
        ] {
            h.get_by_role_and_label(Role::Button, item);
        }
        h.get_by_role_and_label(Role::Button, "Copy those lines")
            .click();
        h.step();
        assert_eq!(copied(&h), vec![model.copy_only_in_b().text]);
        settle(&mut h);
        assert_eq!(state(&h).dialog.menu(), None);

        // Esc closes the menu, not Compare.
        button(&mut h, &only_in("A", model.only_in_a.count()));
        assert_eq!(state(&h).dialog.menu(), Some(MenuKind::OnlyIn { b: false }));
        key(&mut h, Modifiers::NONE, Key::Escape);
        assert_eq!(state(&h).dialog.menu(), None);
        assert_eq!(state(&h).workspace, WorkspaceState::Compare);
    }

    #[test]
    fn compare_workspace_without_a_pair_is_disabled_with_an_explanation() {
        let mut h = launch_design();
        assert_eq!(state(&h).findings.focused, None);
        // The rail entry, not the Findings toolbar's Compare action, which
        // has the same name and sits further right.
        h.query_all_by_role_and_label(Role::Button, "Compare")
            .min_by(|a, b| a.rect().left().total_cmp(&b.rect().left()))
            .expect("the rail's Compare entry")
            .click();
        settle(&mut h);
        assert_eq!(state(&h).workspace, WorkspaceState::Compare);
        assert_eq!(state(&h).compare.pair, None);
        h.get_by_label("No pair to compare");
        h.get_by_label(
            "Compare shows a file beside the file it is measured against. In Findings, focus a file and press Enter or choose Compare.",
        );
        assert!(!enabled(&h, Role::Button, "Keep both"));
        assert!(!enabled(&h, Role::Button, "Queue B for cleanup…"));
        assert!(h.query_by_label("Candidate 1 of 2").is_none());

        // After a pair was shown, the rail and Ctrl+2 return to it, the
        // last valid pair (§11.2), with its change position.
        let (target, counterpart) = open_design_pair(&mut h);
        button(&mut h, "Next ›");
        let active = state(&h).compare.active_change;
        key(&mut h, Modifiers::NONE, Key::Escape);
        key(&mut h, Modifiers::CTRL, Key::Num2);
        assert_eq!(state(&h).workspace, WorkspaceState::Compare);
        let shown = state(&h).compare.pair.expect("the last pair");
        assert_eq!((shown.target, shown.counterpart), (target, counterpart));
        assert_eq!(state(&h).compare.active_change, active);
        assert!(enabled(&h, Role::Button, "Queue B for cleanup…"));
    }

    fn focus(h: &mut Ui, row: RowId) {
        state_mut(h).findings.focused = Some(row);
        settle(h);
    }

    fn member_row(h: &Ui, name: &str) -> (RowId, FileId) {
        let results = results(h);
        let file = file_named(&results, name);
        let group = group_of(&results, file);
        (RowId::Member { group, file }, file)
    }

    fn shown_pair(h: &Ui) -> Option<(FileId, FileId)> {
        state(h).compare.pair.map(|p| (p.target, p.counterpart))
    }

    /// The rail's Compare entry (the Findings toolbar action has the same
    /// name and sits further right).
    fn rail_compare(h: &mut Ui) {
        h.query_all_by_role_and_label(Role::Button, "Compare")
            .min_by(|a, b| a.rect().left().total_cmp(&b.rect().left()))
            .expect("the rail's Compare entry")
            .click();
        settle(h);
    }

    /// §11.2: the rail and Ctrl+2 show the focused row against its
    /// counterpart, else the last valid pair, else the disabled state.
    #[test]
    fn compare_workspace_rail_and_ctrl_2_follow_the_focused_row() {
        let mut h = launch_design();
        let pair = design::sample().last_compared.expect("design pair");
        let group = group_of(&results(&h), pair.b);

        // Neither a focused member nor a pair: disabled.
        focus(&mut h, RowId::Group(group));
        key(&mut h, Modifiers::CTRL, Key::Num2);
        assert_eq!(state(&h).workspace, WorkspaceState::Compare);
        assert_eq!(shown_pair(&h), None);
        h.get_by_label("No pair to compare");

        // The rail with the draft focused: the draft against its reference.
        key(&mut h, Modifiers::CTRL, Key::Num1);
        let (draft_row, draft) = member_row(&h, "Design_Spec_draft.txt");
        focus(&mut h, draft_row);
        rail_compare(&mut h);
        run_until(&mut h, "the draft's comparison", |s| {
            s.compare.comparison.aligned().is_some()
        });
        assert_eq!(shown_pair(&h), Some((draft, pair.a)));
        assert_eq!(state(&h).compare.return_row, Some(draft_row));

        // Ctrl+2 with another member focused: that member.
        key(&mut h, Modifiers::CTRL, Key::Num1);
        let (rev_row, rev) = member_row(&h, "Design_Spec_rev.txt");
        focus(&mut h, rev_row);
        key(&mut h, Modifiers::CTRL, Key::Num2);
        run_until(&mut h, "the revision's comparison", move |s| {
            s.compare
                .comparison
                .aligned()
                .is_some_and(|a| a.b.file == rev)
        });
        assert_eq!(shown_pair(&h), Some((rev, pair.a)));

        // A group row focused: the last valid pair stays.
        key(&mut h, Modifiers::CTRL, Key::Num1);
        focus(&mut h, RowId::Group(group));
        rail_compare(&mut h);
        assert_eq!(state(&h).workspace, WorkspaceState::Compare);
        assert_eq!(shown_pair(&h), Some((rev, pair.a)));
    }

    /// A request the engine does not admit (Busy) is asked again at most
    /// every 250 ms of egui time, not every frame, and is admitted once the
    /// engine has room.
    #[test]
    fn compare_workspace_a_busy_engine_is_not_asked_every_frame() {
        // 60 frames per second, so frames and egui time can be told apart.
        let mut h = launch_with_dt(
            ScenarioKind::Design,
            Vec2::new(1600.0, 960.0),
            Some(1.0 / 60.0),
        );
        open_design_pair(&mut h);
        h.state_mut().busy = true;
        button(&mut h, "Normalized");
        for _ in 0..36 {
            h.step();
        }
        // 24 + 36 frames are one second at 60 frames per second: the
        // switch's own request plus at most four retries.
        let refused = h.state().refused;
        assert!(
            (1..=5).contains(&refused),
            "asked {refused} times in 60 frames"
        );

        h.state_mut().busy = false;
        run_until(&mut h, "the Normalized comparison", |s| {
            s.compare
                .comparison
                .aligned()
                .is_some_and(|a| a.mode == fuzzy_dedupe_domain::CompareMode::Normalized)
        });
    }

    #[test]
    fn compare_workspace_marks_stale_evidence_on_the_stale_scenario() {
        let mut h = launch(ScenarioKind::Stale, Vec2::new(1600.0, 960.0));
        let results = results(&h);
        let review = file_named(&results, "Proposal_review.txt");
        let proposal = file_named(&results, "Proposal.txt");
        open(&mut h, review, proposal);
        assert!(h.query_by_label("Stale evidence").is_none());

        // The scenario's first injected change: the counterpart's snapshot.
        let scenario = fixtures::build(ScenarioKind::Stale, results.scan, fixtures::DEFAULT_SEED)
            .expect("stale scenario");
        let change = scenario.evidence_changes[0];
        push(
            &mut h,
            UiIntent::Review {
                command: ReviewCommand::UpdateEvidence(change),
                invoker: None,
            },
        );
        settle(&mut h);
        h.get_by_label("Stale evidence");
        h.get_by_label("A changed since this comparison");
        h.get_by_role_and_label(Role::Button, "Compare again");
        assert!(!enabled(&h, Role::Button, "Queue B for cleanup…"));
        assert!(!enabled(&h, Role::Button, "Keep both"));
    }

    #[test]
    fn compare_workspace_queue_b_on_a_counterpart_queued_pair_is_refused() {
        let mut h = launch(ScenarioKind::Stale, Vec2::new(1600.0, 960.0));
        let results = results(&h);
        // The exact group: Invoice.txt (reference), a queued copy, and a
        // backup copy shown against the queued one.
        let group = results
            .groups
            .iter()
            .find(|g| g.members.iter().all(|m| m.name == "Invoice.txt"))
            .expect("the Invoice group");
        let queued = group
            .members
            .iter()
            .find(|m| state(&h).queue.is_queued(m.file))
            .expect("a queued copy")
            .file;
        let reference = state(&h).queue.reference(group).expect("reference");
        let backup = group
            .members
            .iter()
            .find(|m| m.file != queued && m.file != reference)
            .expect("the backup copy")
            .file;
        open(&mut h, backup, queued);
        let entries = state(&h).queue.entries.clone();

        button(&mut h, "Queue B for cleanup…");
        assert_eq!(
            state(&h).queue.last_refusal,
            Some((backup, Refusal::CounterpartQueued))
        );
        assert_eq!(state(&h).queue.entries, entries, "nothing queued");
        assert_eq!(
            state(&h).status.as_ref().map(|s| s.text.as_str()),
            Some("Counterpart queued")
        );
        identity(&h, "B · Counterpart queued: Invoice.txt, ");
    }

    #[test]
    fn compare_workspace_queue_b_and_keep_both_name_the_displayed_pair() {
        let mut h = launch_design();
        let (target, counterpart) = open_design_pair(&mut h);
        let request = state(&h).compare.comparison.request;
        assert!(request.is_some());

        // Queue B on the unreviewed draft needs Review unique text, for
        // exactly the displayed target against A, bound to this comparison.
        button(&mut h, "Queue B for cleanup…");
        let dialog = state(&h).dialog.dialog().expect("a popup").clone();
        let fuzzy_dedupe_app::presentation::state::DialogSubject::UniqueText(subject) =
            dialog.subject
        else {
            panic!("Review unique text, not {:?}", dialog.subject);
        };
        assert_eq!(
            (
                subject.target,
                subject.counterpart,
                subject.reviewed,
                subject.origin
            ),
            (target, counterpart, request, QueueOrigin::Compare)
        );
        key(&mut h, Modifiers::NONE, Key::Escape);
        assert!(!state(&h).dialog.is_open());
        run_until(&mut h, "Compare's comparison back", |s| {
            s.compare.comparison.aligned().is_some()
                && s.compare.comparison.key.is_some_and(|k| k.target == target)
        });

        // Keep both changes only the displayed pair.
        let rev = file_named(&results(&h), "Design_Spec_rev.txt");
        let rev_before = state(&h).queue.decisions.get(&rev).copied();
        button(&mut h, "Keep both");
        let decisions = &state(&h).queue.decisions;
        assert_eq!(decisions.get(&target), Some(&MemberDecision::Kept));
        assert_eq!(decisions.get(&rev).copied(), rev_before);
        assert!(!state(&h).queue.is_queued(target));
        assert!(!state(&h).queue.is_queued(counterpart));
    }
}
