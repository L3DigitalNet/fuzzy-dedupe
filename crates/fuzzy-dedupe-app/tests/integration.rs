// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! PV-T22-003, PV-T22-005, PV-T22-006, PV-T22-007 and PV-T22-008 (plan
//! T22): MS-1's cross-cutting behavior, driven headless through
//! `egui_kittest` over the whole shell.
//!
//! Every test sits in the `integration` module and is named
//! `integration_…`, so `-E 'test(integration)'` selects exactly these.
//!
//! The rig runs `FuzzyDedupeApp`'s frame order over the public seams, as
//! `tests/compare.rs` does: drain engine events into `events::apply`, poll
//! the services, the dialog host's frame start, global chords, menu keys,
//! the shell, the dialog host, then the frame's intents with their
//! follow-ups. It is not `FuzzyDedupeApp` itself because the app always
//! adopts the design scenario, and these proofs also need `empty`,
//! `issues`, `stale`, `large` and the hand-built UI check 06 scenario.
//! Engine work arrives from worker threads, so every wait on it is a
//! bounded loop of at most 500 frames 10 ms apart.
//!
//! Observable outcomes are read where the user reads them: accessible
//! labels in the drawn UI, the clipboard output, or the projections the
//! Findings cells draw (`projections::decision_label`, `unique_label`).
//! Queue membership is read from the published validation view mirrored
//! into `QueueState`, the single queue authority (plan D-003).

mod integration {
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use egui::accesskit::Role;
    use egui::{Event, Key, Modifiers, OutputCommand, Vec2};
    use egui_kittest::Harness;
    use egui_kittest::kittest::{NodeT as _, Queryable};
    use fuzzy_dedupe_app::app::{adopt_scenario, log_subscriber};
    use fuzzy_dedupe_app::presentation::commands::{
        self, IntentQueue, Services, UiIntent, comparison_request,
    };
    use fuzzy_dedupe_app::presentation::events;
    use fuzzy_dedupe_app::presentation::projections;
    use fuzzy_dedupe_app::presentation::state::{
        CleanupPhase, CompareState, ComparisonSlot, DialogKind, DialogSubject, FindingsState,
        MenuKind, PresentationState, QueueState, RowId, ScanStage, UniqueTextSubject,
        WorkspaceState,
    };
    use fuzzy_dedupe_app::ui::compare as compare_view;
    use fuzzy_dedupe_app::ui::dialogs::{new_scan, unique_text_review};
    use fuzzy_dedupe_app::ui::widgets::dialog_host;
    use fuzzy_dedupe_app::ui::{glyphs, shell, theme};
    use fuzzy_dedupe_domain::{
        CompareMode, CounterpartBasis, CounterpartLink, Coverage, EvidenceUpdate, FileId, Group,
        GroupId, IgnoreOptions, IssueKind, MatchEdge, MatchMode, MatchingRules, Measurement,
        Member, MemberRole, Permille, QueueOrigin, QueueScope, RulesRevision, ScanFolder,
        ScanResults, ScanSetup, SnapshotId,
    };
    use fuzzy_dedupe_engine::cleanup_sim::{CleanupScript, ScriptedOutcome};
    use fuzzy_dedupe_engine::fixtures::{
        self, FileContent, FixtureContents, Scenario, ScenarioKind, design,
    };
    use fuzzy_dedupe_engine::service::{
        Engine, EngineConfig, EngineRequest, ReviewCommand, ReviewStatus, StartScan,
    };
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

    /// A fresh home under the test's own target directory, unique per run
    /// because tests may not delete what an earlier run left behind.
    fn dirs_for(name: &str) -> AppDirs {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after 1970")
            .as_nanos();
        let root: PathBuf = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("integration")
            .join(format!("{name}-{nanos}"));
        let state = root.join("state");
        AppDirs::resolve(
            move |var| (var == "XDG_STATE_HOME").then(|| state.clone().into_os_string()),
            &root.join("home"),
        )
    }

    // ── The rig ──────────────────────────────────────────────────────────

    struct Rig {
        state: PresentationState,
        services: Services,
        intents: IntentQueue,
        /// The harness runs a frame while it is built, before the theme is
        /// installed; the rig draws nothing until then.
        ready: bool,
    }

    impl Rig {
        /// `FuzzyDedupeApp::run_logic` then `draw`, minus preferences,
        /// log expiry and measurement, which none of these proofs reads.
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

        /// A review command applied the way `commands::review` mirrors it,
        /// for test setup only: no popup opens on `NeedsReview`.
        fn review(&mut self, command: ReviewCommand) -> ReviewStatus {
            let outcome = self.services.engine.review(command);
            self.services.sync_view(&mut self.state);
            events::apply_review(&mut self.state, &outcome);
            outcome.status
        }

        /// Queues `file` against `counterpart` with the acknowledgment the
        /// policy asks for, as a user who reviewed it would; setup only.
        fn queue_reviewed(&mut self, file: FileId, counterpart: FileId) {
            let request = ReviewCommand::RequestQueue {
                file,
                counterpart,
                origin: QueueOrigin::Checkbox,
                reviewed: None,
            };
            if let ReviewStatus::NeedsReview { .. } = self.review(request.clone()) {
                self.review(ReviewCommand::AcknowledgeUniqueText {
                    file,
                    counterpart,
                    reviewed: None,
                });
                self.review(request);
            }
            assert!(self.state.queue.is_queued(file), "setup queued {file:?}");
        }
    }

    type Ui = Harness<'static, Rig>;

    fn build(services: Services, state: PresentationState) -> Ui {
        let rig = Rig {
            state,
            services,
            intents: IntentQueue::default(),
            ready: false,
        };
        let mut harness = Harness::builder()
            .with_max_steps(SETTLE_STEPS)
            .with_size(Vec2::new(1600.0, 960.0))
            .build_ui_state(|ui, rig: &mut Rig| rig.frame(ui), rig);
        theme::install(&harness.ctx, Vec::new());
        // After the theme, so every icon draws on the first frame.
        glyphs::install(&harness.ctx);
        harness.state_mut().ready = true;
        settle(&mut harness);
        harness
    }

    fn services(dirs: AppDirs) -> Services {
        let engine = Engine::new(EngineConfig::default()).expect("engine starts");
        Services::new(engine, Box::new(NoOpener), Box::new(DismissPicker), dirs)
    }

    /// `kind` adopted and seeded through `app::adopt_scenario`, the app's
    /// own startup path.
    fn launch(kind: ScenarioKind) -> Ui {
        launch_in(kind, dirs_for(kind.name()))
    }

    fn launch_in(kind: ScenarioKind, dirs: AppDirs) -> Ui {
        let mut services = services(dirs);
        let mut state = PresentationState::default();
        adopt_scenario(&mut services, &mut state, kind);
        build(services, state)
    }

    /// A hand-built scenario, adopted through the facade and installed as
    /// `app::adopt_scenario` installs one; it has no seeded decisions.
    fn launch_custom(build_scenario: impl FnOnce(fuzzy_dedupe_domain::ScanId) -> Scenario) -> Ui {
        let mut services = services(dirs_for("custom"));
        let scenario = Arc::new(build_scenario(services.engine.reserve_scan_id()));
        services
            .engine
            .adopt_scenario(Arc::clone(&scenario))
            .expect("fresh scan ID");
        let mut state = PresentationState::default();
        events::install_results(
            &mut state,
            Arc::clone(&scenario.results),
            ScanStage::Completed,
        );
        state.findings.expanded = scenario.results.groups.iter().map(|g| g.id).collect();
        services.sync_view(&mut state);
        build(services, state)
    }

    /// Enough frames for input, dispatch, the frame that draws the result,
    /// glyph registration and egui's scroll animation. Fixed steps rather
    /// than `run()`: the scanning view's progress bar animates for as long
    /// as a scan runs, so `run()` could never settle there.
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
        settle(h);
    }

    /// Runs frames until `done` holds, at most 500 × 10 ms, for state that
    /// arrives from the engine's worker threads.
    #[track_caller]
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

    fn button(h: &mut Ui, label: &str) {
        h.get_by_role_and_label(Role::Button, label).click();
        settle(h);
    }

    /// Clicks the first button named `label` outside the rail, where the
    /// same name is on screen twice (New scan in the rail and in a state
    /// message).
    fn button_outside_rail(h: &mut Ui, label: &str) {
        let rail = h.get_by_label(shell::RAIL_NAME).rect();
        h.query_all_by_role_and_label(Role::Button, label)
            .find(|n| !rail.contains_rect(n.rect()))
            .unwrap_or_else(|| panic!("a {label} button outside the rail"))
            .click();
        settle(h);
    }

    /// Opens Review unique text from Compare the way a user reaches it:
    /// Compare has no top-level Review unique text… control, only the item
    /// in the evidence row's "Only in B ▾" menu (spec §11.5), whose label
    /// carries the count, so it is found by its prefix.
    fn review_from_only_in_b(h: &mut Ui) {
        let menu = h
            .query_all_by_role(Role::Button)
            .filter_map(|n| n.accesskit_node().label())
            .find(|l| l.starts_with("Only in B: "))
            .expect("the Only in B ▾ menu button");
        button(h, &menu);
        button(h, compare_view::REVIEW_UNIQUE_NAME);
    }

    fn enabled(h: &Ui, role: Role, label: &str) -> bool {
        h.query_by_role_and_label(role, label)
            .is_some_and(|n| !n.accesskit_node().is_disabled())
    }

    fn has_label(h: &Ui, text: &str) -> bool {
        h.query_all_by_label(text).next().is_some()
    }

    #[track_caller]
    fn assert_label(h: &Ui, text: &str) {
        if !has_label(h, text) {
            let labels: Vec<String> = h
                .query_all_by(|_| true)
                .filter_map(|n| {
                    let node = n.accesskit_node();
                    node.label().or_else(|| node.value())
                })
                .collect();
            panic!("no node labelled {text:?}; labels: {labels:#?}");
        }
    }

    fn results(h: &Ui) -> Arc<ScanResults> {
        state(h).scan.results.clone().expect("results")
    }

    fn file_named(h: &Ui, name: &str) -> FileId {
        let found: Vec<FileId> = results(h)
            .groups
            .iter()
            .flat_map(|g| &g.members)
            .filter(|m| m.name == name)
            .map(|m| m.file)
            .collect();
        assert_eq!(found.len(), 1, "exactly one {name}");
        found[0]
    }

    fn member(h: &Ui, file: FileId) -> Member {
        results(h)
            .group_of(file)
            .and_then(|g| g.member(file))
            .expect("in the results")
            .clone()
    }

    fn row_of(h: &Ui, file: FileId) -> RowId {
        let group = results(h).group_of(file).expect("a member").id;
        RowId::Member { group, file }
    }

    /// The row's counterpart as the review service recorded it, `None` for
    /// the reference.
    fn counterpart_of(h: &Ui, file: FileId) -> Option<FileId> {
        let results = results(h);
        let group = results.group_of(file).expect("a member");
        let member = group.member(file).expect("a member");
        projections::member_view(group, &state(h).queue, member).counterpart
    }

    /// The Decision cell's text for `file`, from the function the cell
    /// draws with (`grouped_table`).
    fn decision(h: &Ui, file: FileId) -> &'static str {
        let results = results(h);
        let group = results.group_of(file).expect("a member");
        let member = group.member(file).expect("a member");
        let queue = &state(h).queue;
        projections::decision_label(
            member,
            &projections::member_view(group, queue, member),
            queue,
        )
    }

    fn queued(h: &Ui) -> BTreeSet<FileId> {
        state(h).queue.entries.keys().copied().collect()
    }

    fn open_dialog(h: &Ui) -> Option<DialogKind> {
        state(h).dialog.dialog().map(|d| d.kind)
    }

    /// Opens Compare on `target` against `counterpart`, as Enter on the row
    /// does, and waits for the comparison to be drawn.
    fn open_compare(h: &mut Ui, target: FileId, counterpart: FileId) {
        let row = row_of(h, target);
        push(
            h,
            UiIntent::OpenCompare {
                target,
                counterpart,
                return_row: Some(row),
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
    fn design_pair() -> (FileId, FileId) {
        let pair = design::sample().last_compared.expect("design pair");
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

    /// The labels of every node inside the navigation rail: its workspace
    /// entries, New scan and Menu.
    fn rail_entries(h: &Ui) -> Vec<String> {
        let rail = h.get_by_label(shell::RAIL_NAME).rect();
        let mut labels: Vec<String> = h
            .query_all_by_role(Role::Button)
            .filter(|n| rail.contains_rect(n.rect()))
            .filter_map(|n| n.accesskit_node().label())
            .collect();
        labels.sort();
        labels
    }

    /// Clicks the first visible member row's Decision cell, as a user
    /// focusing the table does (the pattern of `tests/findings.rs`).
    fn click_first_row(h: &mut Ui) {
        let header_bottom = h
            .query_all_by_role(Role::Label)
            .filter(|n| n.accesskit_node().value().as_deref() == Some("Decision"))
            .map(|n| n.rect().bottom())
            .fold(f32::INFINITY, f32::min);
        let cell = h
            .query_all_by_role(Role::Label)
            .filter(|n| n.rect().top() > header_bottom + 2.0)
            .filter(|n| {
                n.accesskit_node().value().is_some_and(|v| {
                    [
                        "Reference",
                        "Unreviewed",
                        "Review first",
                        "Kept",
                        "Queued",
                        "Protected",
                        "Counterpart queued",
                        "Counterpart of a queued file",
                    ]
                    .contains(&v.as_str())
                })
            })
            .min_by(|a, b| a.rect().top().total_cmp(&b.rect().top()))
            .expect("a visible row");
        cell.click();
        settle(h);
    }

    /// Moves row focus to `row` and scrolls it into view through the keys
    /// a user would press, with keyboard focus on the table, so Space and
    /// Shift+F10 reach it.
    fn reveal(h: &mut Ui, row: RowId) {
        click_first_row(h);
        state_mut(h).findings.focused = Some(row);
        // One step away and back: each step must move focus, so the keys
        // demonstrably reached the table; a row at the end steps up first.
        key(h, Modifiers::NONE, Key::ArrowDown);
        let back = if state(h).findings.focused == Some(row) {
            key(h, Modifiers::NONE, Key::ArrowUp);
            Key::ArrowDown
        } else {
            Key::ArrowUp
        };
        assert_ne!(state(h).findings.focused, Some(row), "the keys moved focus");
        key(h, Modifiers::NONE, back);
        assert_eq!(state(h).findings.focused, Some(row), "focus is on the row");
    }

    // ── PV-T22-003: popups keep workspace state; one popup at a time ─────

    /// Everything a popup must leave as it found it: which workspace shows,
    /// the Findings and Compare view state, and the queue.
    ///
    /// The comparison slot is cleared before comparing: by design one slot
    /// serves Compare and Review unique text (`ComparisonSlot` documents
    /// the rejected two-slot alternative), so a popup may take it, and
    /// Compare re-requests its own pair from `pinned` when it draws again.
    #[derive(Debug, PartialEq)]
    struct WorkspaceView {
        workspace: WorkspaceState,
        findings: FindingsState,
        compare: CompareState,
        queue: QueueState,
    }

    fn workspace_view(h: &Ui) -> WorkspaceView {
        let s = state(h);
        WorkspaceView {
            workspace: s.workspace,
            findings: s.findings.clone(),
            compare: CompareState {
                comparison: ComparisonSlot::default(),
                ..s.compare.clone()
            },
            queue: s.queue.clone(),
        }
    }

    /// The subject each popup needs: the design pair for Review unique
    /// text, every byte-identical group for Queue identical copies.
    fn subject_for(h: &Ui, kind: DialogKind) -> DialogSubject {
        match kind {
            DialogKind::UniqueTextReview => {
                let (target, counterpart) = design_pair();
                unique_subject(h, target, counterpart)
            }
            DialogKind::QueueIdentical => {
                DialogSubject::QueueIdentical(QueueScope::AllByteIdenticalGroups)
            }
            _ => DialogSubject::None,
        }
    }

    /// Review unique text's subject for `target` against `counterpart`, as
    /// the row menu builds it.
    fn unique_subject(h: &Ui, target: FileId, counterpart: FileId) -> DialogSubject {
        let request = comparison_request(state(h), target, counterpart, CompareMode::Original)
            .expect("the pair is in the results");
        DialogSubject::UniqueText(UniqueTextSubject {
            scan: request.scan,
            target,
            counterpart,
            target_snapshot: request.target_snapshot,
            counterpart_snapshot: request.counterpart_snapshot,
            rules: request.rules,
            reviewed: None,
            origin: QueueOrigin::RowMenu,
        })
    }

    fn request_popup(h: &mut Ui, kind: DialogKind) {
        let subject = subject_for(h, kind);
        push(
            h,
            UiIntent::OpenDialog {
                kind,
                subject,
                invoker: None,
            },
        );
    }

    /// The design scenario with every group expanded and the table
    /// scrolled away from the top, so a popup that reset the expansion or
    /// the scroll offset would show in the comparison.
    fn findings_with_view_state() -> Ui {
        let mut h = launch(ScenarioKind::Design);
        let groups: BTreeSet<GroupId> = results(&h).groups.iter().map(|g| g.id).collect();
        let last = results(&h)
            .groups
            .iter()
            .flat_map(|g| g.members.iter().map(move |m| (g.id, m.file)))
            .last()
            .expect("a member");
        let findings = &mut state_mut(&mut h).findings;
        findings.expanded = groups;
        findings.search.clear();
        findings.focused = Some(RowId::Member {
            group: last.0,
            file: last.1,
        });
        findings.reveal_focus = true;
        findings.revision += 1;
        settle(&mut h);
        assert!(
            state(&h).findings.scroll_offset > 0.0,
            "the table scrolled to the last row"
        );
        h
    }

    fn popup_round_trip(h: &mut Ui, workspace: WorkspaceState) {
        for kind in DialogKind::ALL {
            let before = workspace_view(h);
            request_popup(h, kind);
            assert_eq!(open_dialog(h), Some(kind), "{workspace:?}: {kind:?} opens");
            assert_eq!(state(h).workspace, workspace, "{kind:?} adds no workspace");
            key(h, Modifiers::NONE, Key::Escape);
            assert_eq!(open_dialog(h), None, "{workspace:?}: Esc closes {kind:?}");
            if workspace == WorkspaceState::Compare {
                // Compare asks for its pinned pair again once the slot was
                // taken; let that answer land before comparing.
                run_until(h, "Compare's own comparison again", |s| {
                    s.compare.comparison.aligned().is_some()
                });
            }
            assert_eq!(
                workspace_view(h),
                before,
                "{workspace:?}: {kind:?} changed workspace state"
            );
        }
    }

    #[test]
    fn integration_each_popup_from_findings_leaves_workspace_state_unchanged() {
        let mut h = findings_with_view_state();
        popup_round_trip(&mut h, WorkspaceState::Findings);
    }

    #[test]
    fn integration_each_popup_from_compare_leaves_workspace_state_unchanged() {
        let mut h = launch(ScenarioKind::Design);
        let (target, counterpart) = design_pair();
        open_compare(&mut h, target, counterpart);
        // Non-default Compare view state a popup could reset.
        key(&mut h, Modifiers::ALT, Key::Z);
        key(&mut h, Modifiers::ALT, Key::ArrowDown);
        // Wrap starts on (frame `13:1304`), so Alt+Z leaves it off.
        assert!(!state(&h).compare.wrap, "wrap off, against the default");
        popup_round_trip(&mut h, WorkspaceState::Compare);
    }

    #[test]
    fn integration_a_second_popup_or_menu_cannot_open_over_a_popup() {
        for workspace in [WorkspaceState::Findings, WorkspaceState::Compare] {
            let mut h = launch(ScenarioKind::Design);
            if workspace == WorkspaceState::Compare {
                let (target, counterpart) = design_pair();
                open_compare(&mut h, target, counterpart);
            }
            for first in DialogKind::ALL {
                request_popup(&mut h, first);
                assert_eq!(open_dialog(&h), Some(first));
                for second in DialogKind::ALL {
                    request_popup(&mut h, second);
                    assert_eq!(
                        open_dialog(&h),
                        Some(first),
                        "{workspace:?}: {second:?} opened over {first:?}"
                    );
                }
                push(
                    &mut h,
                    UiIntent::ToggleMenu {
                        kind: MenuKind::Main,
                        invoker: None,
                    },
                );
                // Global chords are blocked under a popup, too.
                key(&mut h, Modifiers::CTRL, Key::N);
                key(&mut h, Modifiers::NONE, Key::F10);
                assert_eq!(state(&h).dialog.menu(), None, "no menu over {first:?}");
                assert_eq!(open_dialog(&h), Some(first), "{first:?} still open");
                key(&mut h, Modifiers::NONE, Key::Escape);
                assert_eq!(open_dialog(&h), None);
            }
        }
    }

    // ── PV-T22-005: the UI check 14 states ───────────────────────────────

    /// The rail before a state is driven: its two workspace entries, New
    /// scan and Menu. A state that added a screen would add an entry.
    fn baseline_rail(h: &Ui) -> Vec<String> {
        let rail = rail_entries(h);
        assert_eq!(
            rail.iter()
                .filter(|l| ["Findings", "Compare"].contains(&l.as_str()))
                .count(),
            2,
            "two workspace entries: {rail:?}"
        );
        rail
    }

    #[track_caller]
    fn assert_no_extra_screen(h: &Ui, rail: &[String]) {
        assert_eq!(rail_entries(h), rail, "the rail's entries are unchanged");
        assert!(matches!(
            state(h).workspace,
            WorkspaceState::Findings | WorkspaceState::Compare
        ));
    }

    #[test]
    fn integration_missing_folder_shows_its_remedy_in_new_scan() {
        let mut h = launch(ScenarioKind::Design);
        let rail = baseline_rail(&h);
        let missing = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("integration")
            .join("never-created-folder");
        assert!(!missing.exists());
        state_mut(&mut h).preferences.scan_setup = ScanSetup {
            folders: vec![ScanFolder {
                path: missing.clone(),
                protect: false,
            }],
            file_types: vec!["txt".to_owned()],
        };
        key(&mut h, Modifiers::CTRL, Key::N);
        assert_eq!(open_dialog(&h), Some(DialogKind::NewScan));
        assert_label(&h, new_scan::MISSING_FOLDER);
        // Start scan refuses with the remedy instead of starting a scan.
        let scan = state(&h).scan.clone();
        button(&mut h, "Start scan");
        assert_eq!(open_dialog(&h), Some(DialogKind::NewScan));
        assert_label(&h, "Remove or replace the folder that was not found.");
        assert_eq!(state(&h).scan, scan, "no scan started");
        // The remedy is a control in the same popup.
        let remove = format!("Remove {}/", missing.display());
        assert!(enabled(&h, Role::Button, &remove), "{remove}");
        assert_no_extra_screen(&h, &rail);
    }

    #[test]
    fn integration_empty_scan_offers_a_new_scan() {
        let mut h = launch(ScenarioKind::Empty);
        let rail = baseline_rail(&h);
        assert_label(&h, "No relationships found");
        assert_label(
            &h,
            "The scan finished and found no identical or similar text files.",
        );
        button_outside_rail(&mut h, "New scan");
        assert_eq!(
            open_dialog(&h),
            Some(DialogKind::NewScan),
            "the action acts"
        );
        assert_no_extra_screen(&h, &rail);
    }

    #[test]
    fn integration_no_search_matches_offers_to_clear_the_search() {
        let mut h = launch(ScenarioKind::Design);
        let rail = baseline_rail(&h);
        let field = fuzzy_dedupe_app::ui::findings::SEARCH_NAME;
        h.get_by_role_and_label(Role::TextInput, field).focus();
        settle(&mut h);
        h.get_by_role_and_label(Role::TextInput, field)
            .type_text("zz-no-such-file");
        settle(&mut h);
        assert_label(&h, "No files match “zz-no-such-file”");
        button(&mut h, "Clear search and filters");
        assert!(state(&h).findings.search.is_empty(), "the action acts");
        assert!(!has_label(&h, "No files match “zz-no-such-file”"));
        assert_no_extra_screen(&h, &rail);
    }

    #[test]
    fn integration_decoding_errors_and_missing_folders_stay_listed() {
        let mut h = launch(ScenarioKind::Issues);
        let rail = baseline_rail(&h);
        let issues = state(&h).scan.issues.clone();
        for kind in [IssueKind::DecodingFailed, IssueKind::MissingFolder] {
            assert!(issues.iter().any(|i| i.kind == kind), "{kind:?} in issues");
        }
        let note = format!(
            "{} files could not be read",
            projections::thousands(issues.len() as u64)
        );
        assert!(h.query_by_label_contains(&note).is_some(), "{note}");
        h.get_by_label("Show details").click();
        settle(&mut h);
        for issue in &issues {
            let line = format!(
                "{}  {}",
                fuzzy_dedupe_app::ui::findings::issue_text(issue.kind),
                issue.display_path
            );
            assert_label(&h, &line);
        }
        assert_no_extra_screen(&h, &rail);
    }

    #[test]
    fn integration_stale_evidence_offers_to_compare_again() {
        let mut h = launch(ScenarioKind::Design);
        let rail = baseline_rail(&h);
        let (target, counterpart) = design_pair();
        open_compare(&mut h, target, counterpart);
        assert!(!has_label(&h, compare_view::STALE_BADGE));
        let snapshot = state(&h).queue.snapshot(target).expect("a snapshot");
        push(
            &mut h,
            UiIntent::Review {
                command: ReviewCommand::UpdateEvidence(EvidenceUpdate::FileSnapshot {
                    file: target,
                    snapshot: SnapshotId::new(snapshot.get() + 1_000),
                }),
                invoker: None,
            },
        );
        assert_label(&h, compare_view::STALE_BADGE);
        assert!(
            enabled(&h, Role::Button, compare_view::COMPARE_AGAIN_NAME),
            "Compare again is offered"
        );
        assert!(
            !enabled(&h, Role::Button, compare_view::QUEUE_B_NAME),
            "no queueing on stale evidence"
        );
        assert_no_extra_screen(&h, &rail);
    }

    #[test]
    fn integration_scan_cancellation_reports_partial_coverage() {
        let mut h = launch(ScenarioKind::Design);
        let rail = baseline_rail(&h);
        let results = results(&h);
        push(
            &mut h,
            UiIntent::Submit(EngineRequest::StartScan(StartScan {
                setup: results.setup.clone(),
                rules: results.rules,
                scenario: ScenarioKind::Large,
                seed: fixtures::DEFAULT_SEED,
            })),
        );
        run_until(&mut h, "the scanning view", |s| {
            s.scan.stage == ScanStage::Running
        });
        button(&mut h, "Stop scan");
        run_until(&mut h, "the partial result", |s| {
            s.scan.stage == ScanStage::Partial
        });
        assert!(
            h.query_by_label_contains("partial: stopped before every file was read")
                .is_some(),
            "the status line states the coverage"
        );
        assert_no_extra_screen(&h, &rail);
    }

    #[test]
    fn integration_unavailable_trash_is_blocked_before_confirmation() {
        let mut h = launch(ScenarioKind::Stale);
        let rail = baseline_rail(&h);
        // The stale scenario scripts one target onto a volume without
        // Trash; its seeded Protect is cleared first, as a user would.
        let scenario = fixtures::build(
            ScenarioKind::Stale,
            fuzzy_dedupe_domain::ScanId::new(1),
            fixtures::DEFAULT_SEED,
        )
        .expect("fixture");
        let no_trash = CleanupScript::for_scenario(&scenario)
            .entries()
            .find(|(_, o)| *o == ScriptedOutcome::NoTrash)
            .expect("a scripted no-Trash target")
            .0;
        h.state_mut().review(ReviewCommand::ProtectFile {
            file: no_trash,
            protected: false,
        });
        let counterpart = counterpart_of(&h, no_trash).expect("a member");
        h.state_mut().queue_reviewed(no_trash, counterpart);
        settle(&mut h);
        key(&mut h, Modifiers::CTRL, Key::Enter);
        run_until(&mut h, "the plan", |s| {
            s.operation.phase == CleanupPhase::Review
        });
        assert_eq!(open_dialog(&h), Some(DialogKind::Cleanup));
        assert_label(&h, "Blocked: no Trash");
        let folder = format!("{}/", member(&h, no_trash).folder);
        assert_label(
            &h,
            &format!("{folder} is on a volume without a Trash folder."),
        );
        assert_no_extra_screen(&h, &rail);
    }

    // ── PV-T22-006: no fixture text in the logs ──────────────────────────

    /// Every line of every fixture file of `kind` long enough that finding
    /// it in a log can only mean document text was logged. Short lines
    /// ("", "---", a lone word) could match ordinary log vocabulary.
    fn canary_lines(kind: ScenarioKind) -> Vec<String> {
        let scenario = fixtures::build(
            kind,
            fuzzy_dedupe_domain::ScanId::new(1),
            fixtures::DEFAULT_SEED,
        )
        .expect("fixture");
        let mut lines = BTreeSet::new();
        for member in scenario.results.groups.iter().flat_map(|g| &g.members) {
            let bytes = scenario.contents.render(member.file).expect("content");
            for line in String::from_utf8_lossy(&bytes).lines() {
                let line = line.trim();
                if line.chars().count() >= 16 {
                    lines.insert(line.to_owned());
                }
            }
        }
        lines.into_iter().collect()
    }

    /// The text of every file in the log directory. Read-only.
    fn log_text(dirs: &AppDirs) -> String {
        let mut text = String::new();
        for entry in std::fs::read_dir(dirs.log_dir()).expect("the log directory exists") {
            let path = entry.expect("a directory entry").path();
            text.push_str(&std::fs::read_to_string(&path).expect("a readable log file"));
        }
        text
    }

    /// Installs the app's own log subscriber at debug, writing into a fresh
    /// `dirs`, as the global default.
    ///
    /// Global, not thread-scoped: engine workers log from their own
    /// threads, which a thread-local default would never see. nextest runs
    /// each test in its own process, so the one global default per process
    /// is this test's.
    fn install_logging(name: &str) -> (AppDirs, tracing_appender::non_blocking::WorkerGuard) {
        let dirs = dirs_for(name);
        // Debug, above the info default: document text logged at debug
        // would still break NFR-009 under `RUST_LOG=debug`.
        let setup = log_subscriber(&dirs, Some("debug")).expect("logging starts");
        tracing::subscriber::set_global_default(setup.subscriber)
            .expect("the only subscriber in this process (nextest: one process per test)");
        (dirs, setup.guard)
    }

    #[test]
    fn integration_a_fixture_scan_and_comparison_log_no_document_text() {
        let (dirs, guard) = install_logging("logs");
        let canaries = canary_lines(ScenarioKind::Design);
        assert!(canaries.len() >= 20, "enough canaries: {}", canaries.len());

        let mut h = launch_in(ScenarioKind::Design, dirs.clone());
        let previous = state(&h).scan.scan;
        let results = results(&h);
        push(
            &mut h,
            UiIntent::Submit(EngineRequest::StartScan(StartScan {
                setup: results.setup.clone(),
                rules: results.rules,
                scenario: ScenarioKind::Design,
                seed: fixtures::DEFAULT_SEED,
            })),
        );
        run_until(&mut h, "the fixture scan", |s| {
            s.scan.stage == ScanStage::Completed && s.scan.scan != previous
        });
        let target = file_named(&h, "Design_Spec_draft.txt");
        let counterpart = counterpart_of(&h, target).expect("a member");
        open_compare(&mut h, target, counterpart);
        button(&mut h, compare_view::NORMALIZED_NAME);
        run_until(&mut h, "the Normalized comparison", |s| {
            s.compare
                .comparison
                .aligned()
                .is_some_and(|a| a.mode == CompareMode::Normalized)
        });
        review_from_only_in_b(&mut h);
        assert_eq!(open_dialog(&h), Some(DialogKind::UniqueTextReview));
        // Positive control: the subscriber really writes to this directory.
        tracing::info!("integration log marker 7f3a");
        drop(h);
        // Dropping the guard flushes the non-blocking writer.
        drop(guard);

        let logged = log_text(&dirs);
        assert!(logged.contains("integration log marker 7f3a"), "{logged}");
        let leaked: Vec<&String> = canaries
            .iter()
            .filter(|c| logged.contains(c.as_str()))
            .collect();
        assert!(leaked.is_empty(), "document text in the logs: {leaked:#?}");
    }

    /// The negative control for the search above: a canary line logged at
    /// debug is found, so an empty result is not a blind search.
    #[test]
    fn integration_a_logged_canary_line_is_detected() {
        let (dirs, guard) = install_logging("logs-control");
        let canaries = canary_lines(ScenarioKind::Design);
        tracing::debug!(payload = %canaries[0], "diff payload");
        drop(guard);
        assert!(log_text(&dirs).contains(canaries[0].as_str()));
    }

    // ── PV-T22-007: every queue entry point, one policy ──────────────────

    /// The UI check 09 row kinds, each built on the design scenario.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum RowKind {
        Reference,
        Protected,
        Kept,
        /// FR-025 first order: the counterpart was queued first.
        CounterpartQueued,
        /// FR-025 reversed order: the dependent row was queued first, and
        /// the target is its counterpart.
        CounterpartOfQueuedFile,
        UnreviewedFuzzy,
    }

    const ROW_KINDS: [RowKind; 6] = [
        RowKind::Reference,
        RowKind::Protected,
        RowKind::Kept,
        RowKind::CounterpartQueued,
        RowKind::CounterpartOfQueuedFile,
        RowKind::UnreviewedFuzzy,
    ];

    /// The four single-file entry points. Queue identical copies, the bulk
    /// fifth, has its own test below.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Entry {
        Checkbox,
        Space,
        /// Row menu → Review unique text… → Queue for cleanup: the row
        /// menu's only queue route.
        RowMenu,
        /// Compare's Queue B for cleanup….
        CompareQueueB,
    }

    const ENTRIES: [Entry; 4] = [
        Entry::Checkbox,
        Entry::Space,
        Entry::RowMenu,
        Entry::CompareQueueB,
    ];

    /// The design scenario arranged so `kind` exists, and its target row.
    ///
    /// The FR-025 pair is G002's `Requirements_final.txt`, whose counterpart
    /// is `Requirements_v2.txt` (spec §10.1 example). Its seeded per-file
    /// Protect is cleared first so either file can be queued.
    fn row_kind(kind: RowKind) -> (Ui, FileId) {
        let mut h = launch(ScenarioKind::Design);
        let v2 = file_named(&h, "Requirements_v2.txt");
        let last = file_named(&h, "Requirements_final.txt");
        let unprotect_v2 = |h: &mut Ui| {
            h.state_mut().review(ReviewCommand::ProtectFile {
                file: v2,
                protected: false,
            });
            assert!(!state(h).queue.is_protected(v2) && !member(h, v2).folder_protected);
            assert_eq!(counterpart_of(h, last), Some(v2), "final names v2");
        };
        let target = match kind {
            RowKind::Reference => file_named(&h, "Design_Spec.txt"),
            RowKind::Protected => v2,
            RowKind::Kept => {
                // Same bytes as its reference, so only the Kept decision
                // keeps Queue identical copies from taking it.
                let file = file_named(&h, "Release_Notes_copy.txt");
                h.state_mut()
                    .review(ReviewCommand::RemoveFromQueue { file });
                h.state_mut()
                    .review(ReviewCommand::MarkKept { file, kept: true });
                file
            }
            RowKind::CounterpartQueued => {
                unprotect_v2(&mut h);
                let counterpart = counterpart_of(&h, v2).expect("a member");
                h.state_mut().queue_reviewed(v2, counterpart);
                last
            }
            RowKind::CounterpartOfQueuedFile => {
                unprotect_v2(&mut h);
                h.state_mut().queue_reviewed(last, v2);
                v2
            }
            RowKind::UnreviewedFuzzy => file_named(&h, "Design_Spec_draft.txt"),
        };
        settle(&mut h);
        let before = match kind {
            RowKind::Reference => "Reference",
            RowKind::Protected => "Protected",
            RowKind::Kept => "Kept",
            RowKind::CounterpartQueued => "Counterpart queued",
            RowKind::CounterpartOfQueuedFile | RowKind::UnreviewedFuzzy => "Review first",
        };
        assert_eq!(decision(&h, target), before, "{kind:?} set up");
        (h, target)
    }

    /// What an entry point did, read after its whole flow.
    #[derive(Clone, Debug, PartialEq, Eq)]
    struct Outcome {
        queued: bool,
        decision: &'static str,
        queue: BTreeSet<FileId>,
    }

    /// Triggers `entry` on `target`. `false` when the control is absent:
    /// a reference row has no checkbox, no enabled Review unique text…
    /// item and no Compare pair naming it as B.
    fn trigger(h: &mut Ui, entry: Entry, target: FileId) -> bool {
        let row = row_of(h, target);
        match entry {
            Entry::Checkbox => {
                reveal(h, row);
                let name = format!("Queue {}", member(h, target).name);
                if h.query_by_role_and_label(Role::CheckBox, &name).is_none() {
                    return false;
                }
                h.get_by_role_and_label(Role::CheckBox, &name).click();
                settle(h);
            }
            Entry::Space => {
                reveal(h, row);
                key(h, Modifiers::NONE, Key::Space);
            }
            Entry::RowMenu => {
                reveal(h, row);
                key(h, Modifiers::SHIFT, Key::F10);
                assert_eq!(state(h).dialog.menu(), Some(MenuKind::Row(row)));
                let item = fuzzy_dedupe_app::ui::findings::ROW_MENU_ITEMS[0];
                if !enabled(h, Role::Button, item) {
                    key(h, Modifiers::NONE, Key::Escape);
                    return false;
                }
                button(h, item);
                assert_eq!(open_dialog(h), Some(DialogKind::UniqueTextReview));
            }
            Entry::CompareQueueB => {
                let Some(counterpart) = counterpart_of(h, target) else {
                    return false;
                };
                open_compare(h, target, counterpart);
                if enabled(h, Role::Button, compare_view::QUEUE_B_NAME) {
                    button(h, compare_view::QUEUE_B_NAME);
                }
            }
        }
        true
    }

    /// Finishes a flow that opened Review unique text the way a user who
    /// reviews would: Queue for cleanup without the acknowledgment must
    /// queue nothing, then with it the request goes ahead.
    fn finish_review(h: &mut Ui, target: FileId) -> bool {
        if open_dialog(h) != Some(DialogKind::UniqueTextReview) {
            return false;
        }
        run_until(h, "the unique lines", move |s| {
            s.compare
                .comparison
                .key
                .is_some_and(|k| k.target == target && k.mode == CompareMode::Original)
                && s.compare.comparison.result.is_some()
        });
        button(h, "Queue for cleanup");
        assert!(
            !state(h).queue.is_queued(target),
            "nothing queued unacknowledged"
        );
        // The `44:4079` prompt is the proof the press reached the button:
        // without it, "nothing queued" also holds for a click that landed
        // nowhere.
        assert_label(h, unique_text_review::PROMPT);
        h.get_by_role_and_label(Role::CheckBox, unique_text_review::ACKNOWLEDGMENT)
            .click();
        settle(h);
        button(h, "Queue for cleanup");
        true
    }

    /// One entry point's whole flow on a fresh arrangement of `kind`;
    /// `None` when the entry point has no control for the row.
    fn attempt(kind: RowKind, entry: Entry) -> Option<Outcome> {
        let (mut h, target) = row_kind(kind);
        if !trigger(&mut h, entry, target) {
            assert!(
                kind == RowKind::Reference
                    || (kind == RowKind::Protected && entry == Entry::Checkbox),
                "{entry:?} absent for {kind:?}"
            );
            return None;
        }
        let queued_at_once = state(&h).queue.is_queued(target);
        let reviewed = finish_review(&mut h, target);
        if kind == RowKind::UnreviewedFuzzy {
            // The fuzzy-review policy: no route queues it before the
            // acknowledgment, and every route offers the review.
            assert!(!queued_at_once, "{entry:?} queued an unreviewed fuzzy row");
            assert!(reviewed, "{entry:?} opened Review unique text");
        }
        if state(&h).workspace == WorkspaceState::Compare {
            push(&mut h, UiIntent::SwitchWorkspace(WorkspaceState::Findings));
        }
        Some(Outcome {
            queued: state(&h).queue.is_queued(target),
            decision: decision(&h, target),
            queue: queued(&h),
        })
    }

    /// The outcome every entry point must give: refused rows stay out of
    /// the queue under their own label; a Kept row and a reviewed fuzzy row
    /// are queued, because each entry point is a deliberate single-file
    /// choice (spec §6 Kept: "the user can change the decision").
    const fn expected(kind: RowKind) -> (bool, &'static str) {
        match kind {
            RowKind::Reference => (false, "Reference"),
            RowKind::Protected => (false, "Protected"),
            RowKind::Kept | RowKind::UnreviewedFuzzy => (true, "Queued"),
            RowKind::CounterpartQueued => (false, "Counterpart queued"),
            RowKind::CounterpartOfQueuedFile => (false, "Counterpart of a queued file"),
        }
    }

    fn entry_points_agree(kind: RowKind) {
        let outcomes: Vec<(Entry, Outcome)> = ENTRIES
            .iter()
            .filter_map(|&entry| attempt(kind, entry).map(|o| (entry, o)))
            .collect();
        let available: Vec<Entry> = outcomes.iter().map(|(e, _)| *e).collect();
        if kind == RowKind::Reference {
            assert_eq!(available, [Entry::Space], "only Space reaches a reference");
        } else if kind == RowKind::Protected {
            // Frame `13:648` (T40 Q9): a protected row shows no queue
            // checkbox; every other entry point still reaches it and refuses.
            assert_eq!(
                available,
                [Entry::Space, Entry::RowMenu, Entry::CompareQueueB],
                "{kind:?}: every control but the checkbox present"
            );
        } else {
            assert_eq!(available, ENTRIES, "{kind:?}: every control present");
        }
        let (queued, label) = expected(kind);
        let first = &outcomes[0].1;
        assert_eq!((first.queued, first.decision), (queued, label), "{kind:?}");
        for (entry, outcome) in &outcomes {
            assert_eq!(
                outcome, first,
                "{kind:?}: {entry:?} differs from {:?}",
                outcomes[0].0
            );
        }
    }

    #[test]
    fn integration_queue_entry_points_agree_on_a_reference_row() {
        entry_points_agree(RowKind::Reference);
    }

    #[test]
    fn integration_queue_entry_points_agree_on_a_protected_row() {
        entry_points_agree(RowKind::Protected);
    }

    #[test]
    fn integration_queue_entry_points_agree_on_a_kept_row() {
        entry_points_agree(RowKind::Kept);
    }

    #[test]
    fn integration_queue_entry_points_agree_on_a_counterpart_queued_row() {
        entry_points_agree(RowKind::CounterpartQueued);
    }

    #[test]
    fn integration_queue_entry_points_agree_on_a_counterpart_of_a_queued_file() {
        entry_points_agree(RowKind::CounterpartOfQueuedFile);
    }

    #[test]
    fn integration_queue_entry_points_agree_on_an_unreviewed_fuzzy_row() {
        entry_points_agree(RowKind::UnreviewedFuzzy);
    }

    /// Runs Queue identical copies over every byte-identical group through
    /// Actions ▾, and returns the files it added.
    fn queue_identical_copies(h: &mut Ui) -> BTreeSet<FileId> {
        let before = queued(h);
        button(h, fuzzy_dedupe_app::ui::findings::ACTIONS_NAME);
        button(h, fuzzy_dedupe_app::ui::findings::ACTIONS_ITEMS[0]);
        assert_eq!(open_dialog(h), Some(DialogKind::QueueIdentical));
        let label = h
            .query_all_by_role(Role::Button)
            .filter_map(|n| n.accesskit_node().label())
            .find(|l| {
                l.strip_prefix("Queue ")
                    .is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_digit()))
            })
            .expect("the Queue N files button");
        button(h, &label);
        assert_eq!(open_dialog(h), None);
        queued(h).difference(&before).copied().collect()
    }

    /// The bulk entry point never queues any of the six row kinds, in both
    /// FR-025 orders, while it does queue the eligible copies it finds.
    /// For a Kept row this is the bulk rule itself (FR-017: bulk actions
    /// skip kept files), where the single-file entry points queue it.
    #[test]
    fn integration_queue_identical_copies_never_queues_a_policy_row() {
        for kind in ROW_KINDS {
            let (mut h, target) = row_kind(kind);
            let label = decision(&h, target);
            let added = queue_identical_copies(&mut h);
            assert!(!added.is_empty(), "{kind:?}: the bulk action queued copies");
            assert!(!state(&h).queue.is_queued(target), "{kind:?} was queued");
            assert_eq!(decision(&h, target), label, "{kind:?} keeps its state");
            for file in &added {
                let m = member(&h, *file);
                let link = m.link().expect("a member");
                let group = results(&h).group_of(*file).expect("a member").clone();
                assert!(
                    matches!(
                        group
                            .edge_between(*file, link.counterpart)
                            .map(|e| e.measurement),
                        Some(Measurement::Identical(_))
                    ),
                    "{kind:?}: {} is byte-identical",
                    m.name
                );
            }
        }
    }

    /// Which binding component a test changes.
    #[derive(Clone, Copy, Debug)]
    enum Change {
        Target,
        Counterpart,
    }

    /// Injects a new snapshot for `file` through the review service, as the
    /// fixture evidence changes do (`Scenario::evidence_changes`).
    fn change_snapshot(h: &mut Ui, file: FileId) {
        let snapshot = state(h).queue.snapshot(file).expect("a snapshot");
        push(
            h,
            UiIntent::Review {
                command: ReviewCommand::UpdateEvidence(EvidenceUpdate::FileSnapshot {
                    file,
                    snapshot: SnapshotId::new(snapshot.get() + 1_000),
                }),
                invoker: None,
            },
        );
    }

    #[track_caller]
    fn assert_invalidated(h: &mut Ui, target: FileId, change: Change, why: &str) {
        let counterpart = counterpart_of(h, target).expect("a member");
        let others: BTreeSet<FileId> = queued(h).into_iter().filter(|f| *f != target).collect();
        assert!(state(h).queue.is_queued(target), "{why}: queued first");
        change_snapshot(
            h,
            match change {
                Change::Target => target,
                Change::Counterpart => counterpart,
            },
        );
        assert!(
            !state(h).queue.is_queued(target),
            "{why}: {change:?} change unqueued it"
        );
        assert_eq!(queued(h), others, "{why}: other approvals are untouched");
    }

    #[test]
    fn integration_a_changed_target_or_counterpart_invalidates_the_approval() {
        for change in [Change::Target, Change::Counterpart] {
            for entry in ENTRIES {
                let (mut h, target) = row_kind(RowKind::UnreviewedFuzzy);
                assert!(trigger(&mut h, entry, target));
                assert!(finish_review(&mut h, target));
                if state(&h).workspace == WorkspaceState::Compare {
                    push(&mut h, UiIntent::SwitchWorkspace(WorkspaceState::Findings));
                }
                assert_invalidated(&mut h, target, change, &format!("{entry:?}"));
                assert_eq!(
                    decision(&h, target),
                    "Review first",
                    "{entry:?}: back to review"
                );
            }
            let mut h = launch(ScenarioKind::Design);
            let added = queue_identical_copies(&mut h);
            let target = *added.iter().next().expect("a bulk-queued file");
            assert_invalidated(&mut h, target, change, "Queue identical copies");
        }
    }

    // ── PV-T22-008: the three unique-line surfaces agree ─────────────────

    /// The §17.3 UI check 06 unique-line fixtures, as (target, counterpart)
    /// texts with the hand-written unique lines of the target in file
    /// order. They are the PV-T6-003 expected values of the engine's
    /// `tests/compare.rs` `CASES`, restated here because test files share
    /// no module. Every counterpart has no line the target lacks.
    struct UniqueCase {
        name: &'static str,
        target: &'static str,
        counterpart: &'static str,
        unique: &'static [&'static str],
    }

    const UNIQUE_CASES: &[UniqueCase] = &[
        UniqueCase {
            name: "reordered identical lines",
            target: "alpha\nbeta\ngamma\ndelta\n",
            counterpart: "delta\ngamma\nalpha\nbeta\n",
            unique: &[],
        },
        UniqueCase {
            name: "repeated in the file, present once in the counterpart",
            target: "alpha\nalpha\nalpha\nbeta\n",
            counterpart: "alpha\nbeta\n",
            unique: &[],
        },
        UniqueCase {
            name: "repeated absent line",
            target: "alpha\nomega\nbeta\nomega\nomega\n",
            counterpart: "alpha\nbeta\n",
            unique: &["omega", "omega", "omega"],
        },
        UniqueCase {
            name: "CRLF input",
            target: "alpha\r\nbeta\r\ngamma\r\n",
            counterpart: "alpha\nbeta\ngamma\n",
            unique: &[],
        },
        UniqueCase {
            name: "missing final newline",
            target: "alpha\nbeta\ngamma",
            counterpart: "alpha\nbeta\ngamma\n",
            unique: &[],
        },
        UniqueCase {
            name: "file order of unique lines",
            target: "zeta\nalpha\nmid\nbeta\nzeta\n",
            counterpart: "beta\nalpha\n",
            unique: &["zeta", "mid", "zeta"],
        },
    ];

    /// File IDs of case `index`: (counterpart, the group's reference;
    /// target, its member).
    fn case_files(index: usize) -> (FileId, FileId) {
        let base = 2 * u64::try_from(index).expect("small");
        (FileId::new(base + 1), FileId::new(base + 2))
    }

    /// One Similar group per case, the counterpart its reference. The
    /// member's Findings Unique value is the hand-written count, not one
    /// computed from the text, so the Findings surface is an independent
    /// oracle for the two computed ones.
    ///
    /// Its `kind` is `Large` for want of a dedicated variant (the C-01
    /// enum is frozen, plan D-004), as `texts::large_pair_scenario` does;
    /// nothing here reads the kind.
    fn unique_line_scenario(scan: fuzzy_dedupe_domain::ScanId) -> Scenario {
        let folder = "/home/fixture/Unique lines";
        let similar = Measurement::Similar(Permille::new(900).expect("in range"));
        let mut contents = FixtureContents::default();
        let mut groups = Vec::new();
        for (index, case) in UNIQUE_CASES.iter().enumerate() {
            let (counterpart, target) = case_files(index);
            let unique = case.unique.len() as u64;
            let file = |file: FileId, name: String, text: &str, role| Member {
                file,
                display_path: format!("{folder}/{name}"),
                name,
                folder: folder.to_owned(),
                size_bytes: text.len() as u64,
                modified: SystemTime::UNIX_EPOCH,
                folder_protected: false,
                snapshot: SnapshotId::new(file.get()),
                role,
            };
            contents.insert(
                counterpart,
                FileContent::Bytes(Arc::from(case.counterpart.as_bytes())),
            );
            contents.insert(
                target,
                FileContent::Bytes(Arc::from(case.target.as_bytes())),
            );
            groups.push(Group {
                id: GroupId::new(counterpart.get()),
                label: format!("G{:03}", index + 1),
                members: vec![
                    file(
                        counterpart,
                        format!("Case{}_counterpart.txt", index + 1),
                        case.counterpart,
                        MemberRole::Reference,
                    ),
                    file(
                        target,
                        format!("Case{}_target.txt", index + 1),
                        case.target,
                        MemberRole::Member(CounterpartLink {
                            counterpart,
                            basis: CounterpartBasis::Rule,
                            measurement: Some(similar.mirrored()),
                            unique_lines: unique,
                        }),
                    ),
                ],
                edges: vec![MatchEdge {
                    a: counterpart,
                    b: target,
                    measurement: similar,
                    unique_lines_a: 0,
                    unique_lines_b: unique,
                }],
            });
        }
        Scenario {
            kind: ScenarioKind::Large,
            seed: 0,
            results: Arc::new(ScanResults {
                scan,
                setup: ScanSetup {
                    folders: vec![ScanFolder {
                        path: PathBuf::from(folder),
                        protect: false,
                    }],
                    file_types: vec!["txt".to_owned()],
                },
                rules: MatchingRules {
                    mode: MatchMode::ExactAndFuzzy,
                    minimum_similarity: Permille::new(800).expect("in range"),
                    ignore: IgnoreOptions {
                        whitespace: false,
                        line_endings: true,
                        letter_case: false,
                        punctuation: false,
                    },
                    revision: RulesRevision::new(1),
                },
                groups,
                issues: Vec::new(),
                coverage: Coverage::Complete,
            }),
            contents,
            seeded_decisions: Vec::new(),
            evidence_changes: Vec::new(),
        }
    }

    fn lines(n: u64) -> &'static str {
        if n == 1 { "line" } else { "lines" }
    }

    /// Opens Review unique text from Compare, checks its count and its
    /// Copy unique lines text, and closes it again.
    fn check_review(h: &mut Ui, target: FileId, case: &UniqueCase) {
        let n = case.unique.len() as u64;
        review_from_only_in_b(h);
        assert_eq!(
            open_dialog(h),
            Some(DialogKind::UniqueTextReview),
            "{}",
            case.name
        );
        run_until(h, "Review unique text's lines", move |s| {
            s.compare.comparison.aligned().is_some_and(|a| {
                a.mode == CompareMode::Original
                    && s.compare.comparison.key.is_some_and(|k| k.target == target)
            })
        });
        let headline = match n {
            0 => "No lines exist only in this file.".to_owned(),
            1 => "1 line exists only in this file.".to_owned(),
            n => format!("{n} lines exist only in this file."),
        };
        assert_label(h, &headline);
        h.get_by_role_and_label(Role::Button, "Copy unique lines")
            .click();
        h.step();
        let want: String = case.unique.iter().flat_map(|l| [*l, "\n"]).collect();
        let got = copied(h);
        assert_eq!(
            got.last().map_or("", String::as_str),
            want,
            "{}: copy text",
            case.name
        );
        key(h, Modifiers::NONE, Key::Escape);
        assert_eq!(open_dialog(h), None);
    }

    #[test]
    fn integration_unique_line_surfaces_agree_on_the_ui_check_06_fixtures() {
        let mut h = launch_custom(unique_line_scenario);
        for (index, case) in UNIQUE_CASES.iter().enumerate() {
            let (counterpart, target) = case_files(index);
            let n = case.unique.len() as u64;

            // Findings: the Unique cell's text, from the function it draws.
            let results = results(&h);
            let group = results.group_of(target).expect("a member");
            let view = projections::member_view(
                group,
                &state(&h).queue,
                group.member(target).expect("a member"),
            );
            assert_eq!(view.counterpart, Some(counterpart), "{}", case.name);
            assert_eq!(view.unique, Some(n), "{}: Findings Unique", case.name);
            let cell = if n == 0 {
                "—".to_owned()
            } else {
                format!("{n} {}", lines(n))
            };
            assert_eq!(projections::unique_label(&view), cell, "{}", case.name);

            // Compare, Original then Normalized, with Review from each.
            open_compare(&mut h, target, counterpart);
            assert_eq!(
                state(&h)
                    .compare
                    .comparison
                    .aligned()
                    .expect("ready")
                    .only_in_b
                    .count(),
                n,
                "{}: Only in B",
                case.name
            );
            assert_label(&h, &format!("Only in B: {n} {} ▾", lines(n)));
            check_review(&mut h, target, case);

            button(&mut h, compare_view::NORMALIZED_NAME);
            run_until(&mut h, "the Normalized comparison", |s| {
                s.compare
                    .comparison
                    .aligned()
                    .is_some_and(|a| a.mode == CompareMode::Normalized)
            });
            assert_eq!(
                state(&h)
                    .compare
                    .comparison
                    .aligned()
                    .expect("ready")
                    .only_in_b
                    .count(),
                n,
                "{}: Only in B (normalized)",
                case.name
            );
            assert_label(&h, &format!("Only in B: {n} {} (normalized) ▾", lines(n)));
            check_review(&mut h, target, case);

            // Back to Original for the next case's Compare.
            button(&mut h, compare_view::ORIGINAL_NAME);
            push(&mut h, UiIntent::SwitchWorkspace(WorkspaceState::Findings));
        }
    }
}
