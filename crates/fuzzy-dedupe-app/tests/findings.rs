// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! PV-T13-001 to PV-T13-003 (plan T13): the Findings workspace, driven
//! headless through `egui_kittest` on fixture scenarios.
//!
//! Every test sits in the `findings` module so `-E 'test(findings)'`
//! selects them. A `Rig` stands in for `app.rs`: it adopts any fixture
//! scenario through the real `Engine` facade (the app itself always starts
//! on the design scenario) and runs the app's frame order — focus restore,
//! global chords, menu keys, shell, dialog host, then intent dispatch.
//! The opener is a recording double; nothing opens a portal or spawns.
//!
//! Rows are read from the accessibility tree: each visible cell is a label
//! whose horizontal position falls in its column, so a test can read the
//! row a file name sits on. Expected labels come from an independent
//! oracle: the scenario's results, overridden by the roles a second engine
//! reports for the same review commands, rendered by this file's own copy
//! of the §11.4 templates.

mod findings {
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};

    use egui::accesskit::Role;
    use egui::{Event, Key, Modifiers, Vec2};
    use egui_kittest::Harness;
    use egui_kittest::kittest::{NodeT, Queryable};
    use fuzzy_dedupe_app::presentation::commands::{self, IntentQueue, Services};
    use fuzzy_dedupe_app::presentation::events;
    use fuzzy_dedupe_app::presentation::projections;
    use fuzzy_dedupe_app::presentation::state::{
        ColumnId, DialogKind, PresentationState, RowId, ScanStage, SortKey, WorkspaceState,
    };
    use fuzzy_dedupe_app::ui::widgets::dialog_host;
    use fuzzy_dedupe_app::ui::{shell, theme};
    use fuzzy_dedupe_domain::{
        ContainmentDirection, FileId, Group, Measurement, MemberRole, QueueOrigin, Refusal,
        RelationKind, ScanProgress, ScanResults,
    };
    use fuzzy_dedupe_engine::fixtures::{self, Scenario, ScenarioKind, SeededDecision, design};
    use fuzzy_dedupe_engine::service::{Engine, EngineConfig, ReviewCommand, ReviewStatus};
    use fuzzy_dedupe_platform::dirs::AppDirs;
    use fuzzy_dedupe_platform::fonts::{FontLookup, SystemFontLookup};
    use fuzzy_dedupe_platform::open::{Location, OpenError, OpenHandle, Opener};
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

    /// Records every open; answers with `NotADirectory` when `fail` is set,
    /// as the real adapter does for a fixture folder that does not exist.
    #[derive(Clone, Default)]
    struct OpenRecorder {
        opened: Arc<Mutex<Vec<Location>>>,
        fail: bool,
    }

    impl Opener for OpenRecorder {
        fn open(&self, location: Location, notify: Box<dyn FnOnce() + Send>) -> OpenHandle {
            self.opened.lock().unwrap().push(location.clone());
            notify();
            match (self.fail, location) {
                (true, Location::Folder(path)) => {
                    OpenHandle::ready(Err(OpenError::NotADirectory(path)))
                }
                _ => OpenHandle::ready(Ok(())),
            }
        }
    }

    struct DismissPicker;

    impl FolderPicker for DismissPicker {
        fn pick_folders(&self, _: PickRequest, notify: Box<dyn FnOnce() + Send>) -> PickHandle {
            PickHandle::spawn(|| PickOutcome::Dismissed, notify)
        }
    }

    // ── Rig ──────────────────────────────────────────────────────────────

    struct Rig {
        state: PresentationState,
        services: Services,
        intents: IntentQueue,
        scenario: Option<Arc<Scenario>>,
        /// The harness runs a frame while it is built, before the theme is
        /// installed on its context; the rig draws nothing until then.
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
            }
        }
    }

    type Ui = Harness<'static, Rig>;

    fn dirs() -> AppDirs {
        let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("findings");
        AppDirs::resolve(|_| None, &root.join("home"))
    }

    /// The scenario with its seeded decisions replayed, as `app.rs` does
    /// for the design scenario, through the real review service.
    fn adopt(
        services: &mut Services,
        state: &mut PresentationState,
        kind: ScenarioKind,
    ) -> Arc<Scenario> {
        let engine = &mut services.engine;
        let scan = engine.reserve_scan_id();
        let scenario =
            Arc::new(fixtures::build(kind, scan, fixtures::DEFAULT_SEED).expect("scenario"));
        engine
            .adopt_scenario(Arc::clone(&scenario))
            .expect("adopted");
        events::install_results(state, Arc::clone(&scenario.results), ScanStage::Completed);
        if kind == ScenarioKind::Design {
            let sample = design::sample();
            state.findings.expanded = sample.expanded_groups.iter().copied().collect();
        }
        services.sync_view(state);
        for seed in &scenario.seeded_decisions {
            for command in seed_commands(*seed) {
                let mut outcome = services.engine.review(command.clone());
                if let ReviewStatus::NeedsReview { file, counterpart } = outcome.status {
                    let ack = services
                        .engine
                        .review(ReviewCommand::AcknowledgeUniqueText {
                            file,
                            counterpart,
                            reviewed: None,
                        });
                    events::apply_review(state, &ack);
                    outcome = services.engine.review(command);
                }
                events::apply_review(state, &outcome);
            }
        }
        services.sync_view(state);
        state.status = None;
        state.queue.last_refusal = None;
        scenario
    }

    fn seed_commands(seed: SeededDecision) -> Vec<ReviewCommand> {
        match seed {
            SeededDecision::Kept(file) => vec![ReviewCommand::MarkKept { file, kept: true }],
            SeededDecision::Protect(file) => vec![ReviewCommand::ProtectFile {
                file,
                protected: true,
            }],
            SeededDecision::MakeReference(file) => vec![ReviewCommand::MakeReference { file }],
            SeededDecision::Queue { file, counterpart } => vec![ReviewCommand::RequestQueue {
                file,
                counterpart,
                origin: QueueOrigin::Checkbox,
                reviewed: None,
            }],
        }
    }

    fn launch_with(kind: Option<ScenarioKind>, size: Vec2, opener: OpenRecorder) -> Ui {
        let engine = Engine::new(EngineConfig::default()).expect("engine starts");
        let mut services = Services::new(engine, Box::new(opener), Box::new(DismissPicker), dirs());
        let mut state = PresentationState::default();
        let scenario = kind.map(|kind| adopt(&mut services, &mut state, kind));
        let rig = Rig {
            state,
            services,
            intents: IntentQueue::default(),
            scenario,
            ready: false,
        };
        let mut harness = Harness::builder()
            .with_max_steps(SETTLE_STEPS)
            .with_size(size)
            .build_ui_state(|ui, rig: &mut Rig| rig.frame(ui), rig);
        // The app's own fallback lookup, so non-Latin names have glyphs.
        let fallbacks = SystemFontLookup
            .fallback_fonts()
            .into_iter()
            .map(|font| (font.family, egui::FontData::from_owned(font.data)))
            .collect();
        theme::install(&harness.ctx, fallbacks);
        shell::install_icon_glyphs(&harness.ctx);
        harness.state_mut().ready = true;
        settle(&mut harness);
        harness
    }

    fn launch(kind: ScenarioKind) -> Ui {
        launch_with(
            Some(kind),
            Vec2::new(1600.0, 960.0),
            OpenRecorder::default(),
        )
    }

    /// Enough frames for input, dispatch, the frame that draws the result,
    /// and egui's scroll animation after a row is scrolled into view.
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

    fn results(h: &Ui) -> Arc<ScanResults> {
        Arc::clone(state(h).scan.results.as_ref().expect("results"))
    }

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
        settle(h);
    }

    fn click(h: &mut Ui, label: &str) {
        h.get_by_label(label).click();
        settle(h);
    }

    /// Clicks the button named `label`, where a label of the same text may
    /// also be on screen (the Sort item "Folder" and the Folder column).
    fn press(h: &mut Ui, label: &str) {
        h.get_by_role_and_label(Role::Button, label).click();
        settle(h);
    }

    /// A member named `name`, which the test needs to be the only one.
    fn named(results: &ScanResults, name: &str) -> FileId {
        let found: Vec<FileId> = results
            .groups
            .iter()
            .flat_map(|g| &g.members)
            .filter(|m| m.name == name)
            .map(|m| m.file)
            .collect();
        assert_eq!(found.len(), 1, "exactly one {name}");
        found[0]
    }

    /// Whether `name` is the file name of exactly one member.
    fn unique_name(results: &ScanResults, name: &str) -> bool {
        results
            .groups
            .iter()
            .flat_map(|g| &g.members)
            .filter(|m| m.name == name)
            .count()
            == 1
    }

    /// Focuses the member row of `name` by keyboard, scrolling it into view.
    fn focus_named(h: &mut Ui, name: &str) -> RowId {
        let results = results(h);
        let row = row_of(&results, named(&results, name));
        reveal(h, row);
        row
    }

    fn right_click(h: &mut Ui, label: &str) {
        h.get_by_label(label).click_secondary();
        settle(h);
    }

    fn queued(h: &Ui) -> BTreeSet<FileId> {
        state(h).queue.entries.keys().copied().collect()
    }

    fn expand_all(h: &mut Ui) {
        press(h, "Actions ▾");
        press(h, "Expand all groups");
    }

    fn row_of(results: &ScanResults, file: FileId) -> RowId {
        let group = results.group_of(file).expect("file in results").id;
        RowId::Member { group, file }
    }

    /// Moves row focus to `row` and scrolls it into view through the keys
    /// a user would press.
    fn reveal(h: &mut Ui, row: RowId) {
        // The user puts keyboard focus on the table first, by clicking a
        // row; after a menu closes, focus is back on the menu's toolbar
        // button, which the table's keys must not reach through.
        click_first_row(h);
        state_mut(h).findings.focused = Some(row);
        // One step away and back. Each step must move focus, so the keys
        // demonstrably reached the table; a row at the end steps up first.
        let (away, back) = (Key::ArrowDown, Key::ArrowUp);
        key(h, Modifiers::NONE, away);
        let (away, back) = if state(h).findings.focused == Some(row) {
            key(h, Modifiers::NONE, back);
            (back, away)
        } else {
            (away, back)
        };
        assert_ne!(
            state(h).findings.focused,
            Some(row),
            "{away:?} moved row focus"
        );
        key(h, Modifiers::NONE, back);
        assert_eq!(
            state(h).findings.focused,
            Some(row),
            "{back:?} returned focus to the row"
        );
    }

    /// Clicks the first visible member row's Decision cell, as a user
    /// focusing the table does. Member rows only: a header click would
    /// also expand or collapse its group.
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
                    ]
                    .contains(&v.as_str())
                })
            })
            .min_by(|a, b| a.rect().top().total_cmp(&b.rect().top()))
            .expect("a visible row");
        cell.click();
        settle(h);
    }

    /// The Folder cell's display form, which ends in "/" (frame `13:648`).
    fn folder_cell(folder: &str) -> String {
        if folder.ends_with('/') {
            folder.to_owned()
        } else {
            format!("{folder}/")
        }
    }

    // ── Reading rows ─────────────────────────────────────────────────────

    const COLUMNS: [&str; 10] = [
        "Queue",
        "Group",
        "Match",
        "Unique",
        "File name",
        "Folder",
        "Modified",
        "Size",
        "Protected",
        "Decision",
    ];

    /// Header titles with their left edge, for the columns shown.
    fn header(h: &Ui) -> Vec<(&'static str, f32)> {
        let mut out = Vec::new();
        for title in COLUMNS {
            let top = h
                .query_all_by_role(Role::Label)
                .filter(|n| n.accesskit_node().value().as_deref() == Some(title))
                .map(|n| n.rect())
                .min_by(|a, b| a.top().total_cmp(&b.top()));
            if let Some(rect) = top {
                out.push((title, rect.left() - 8.0));
            }
        }
        out.sort_by(|a, b| a.1.total_cmp(&b.1));
        out
    }

    /// Every visible body row, top to bottom, as column title → cell text.
    /// Icon glyphs (private-use code points) are left out.
    fn rows(h: &Ui) -> Vec<BTreeMap<&'static str, String>> {
        let columns = header(h);
        let header_bottom = h
            .query_all_by_role(Role::Label)
            .filter(|n| n.accesskit_node().value().as_deref() == Some("Decision"))
            .map(|n| n.rect().bottom())
            .fold(f32::INFINITY, f32::min);
        let mut by_line: BTreeMap<i64, BTreeMap<&'static str, String>> = BTreeMap::new();
        for node in h.query_all_by_role(Role::Label) {
            let rect = node.rect();
            let Some(text) = node.accesskit_node().value() else {
                continue;
            };
            if rect.top() <= header_bottom
                || text.chars().all(|c| ('\u{E000}'..='\u{F8FF}').contains(&c))
            {
                continue;
            }
            let Some((title, _)) = columns
                .iter()
                .rev()
                .find(|(_, left)| rect.left() >= *left - 1.0)
            else {
                continue;
            };
            // Rows are 28 high: a 4-unit bucket joins one row's cells and
            // never two rows.
            #[expect(clippy::cast_possible_truncation, reason = "a window-sized coordinate")]
            let line = (rect.center().y / 4.0).round() as i64;
            let cell = by_line.entry(line).or_default().entry(title).or_default();
            if !cell.is_empty() {
                cell.push(' ');
            }
            cell.push_str(&text);
        }
        by_line.into_values().collect()
    }

    /// The visible member row of `file`, by its name and folder. A group
    /// header can show the same name (its title falls back to the
    /// reference's file name), so header rows, which carry "G… (n)" in the
    /// Group column, are skipped.
    fn member_row(h: &Ui, file: FileId) -> BTreeMap<&'static str, String> {
        let results = results(h);
        let member = results
            .group_of(file)
            .and_then(|g| g.member(file))
            .expect("file in results");
        let folder = folder_cell(&member.folder);
        rows(h)
            .into_iter()
            .find(|r| {
                r.get("File name") == Some(&member.name)
                    && r.get("Folder") == Some(&folder)
                    && !r.get("Group").is_some_and(|g| g.contains('('))
            })
            .unwrap_or_else(|| panic!("a visible row for {}", member.name))
    }

    // ── Oracle ───────────────────────────────────────────────────────────

    /// Each member's current role: the results' own, replaced by every
    /// role an outcome of the oracle engine reported.
    struct Oracle {
        engine: Engine,
        roles: BTreeMap<FileId, MemberRole>,
    }

    impl Oracle {
        fn new(scenario: &Arc<Scenario>, results: &ScanResults) -> Self {
            let mut engine = Engine::new(EngineConfig::default()).expect("oracle engine");
            let _ = engine.reserve_scan_id();
            engine
                .adopt_scenario(Arc::clone(scenario))
                .expect("oracle adopts");
            let roles = results
                .groups
                .iter()
                .flat_map(|g| g.members.iter().map(|m| (m.file, m.role)))
                .collect();
            let mut oracle = Self { engine, roles };
            for seed in &scenario.seeded_decisions {
                for command in seed_commands(*seed) {
                    oracle.apply(command);
                }
            }
            oracle
        }

        fn apply(&mut self, command: ReviewCommand) -> ReviewStatus {
            let outcome = self.engine.review(command);
            for row in &outcome.changed {
                self.roles.insert(row.file, row.role);
            }
            outcome.status
        }

        fn reference(&self, group: &Group) -> FileId {
            group
                .members
                .iter()
                .find(|m| matches!(self.roles[&m.file], MemberRole::Reference))
                .expect("a reference")
                .file
        }

        /// The §11.4 Match and Unique cells for `file`.
        fn cells(&self, group: &Group, file: FileId) -> (String, String) {
            let MemberRole::Member(link) = self.roles[&file] else {
                return (String::new(), "—".to_owned());
            };
            let reference = self.reference(group);
            let is_reference = link.counterpart == reference;
            let name = if is_reference {
                "reference".to_owned()
            } else {
                group
                    .member(link.counterpart)
                    .expect("counterpart")
                    .name
                    .clone()
            };
            let matched = match link.measurement {
                None => String::new(),
                Some(Measurement::Identical(_)) => format!("Identical to {name}"),
                Some(Measurement::SameText) => format!("Same text as {name}"),
                Some(Measurement::Similar(p)) => format!("{} similar to {name}", pct(p.get())),
                Some(Measurement::Containment {
                    coverage,
                    direction: ContainmentDirection::InsideCounterpart,
                }) => format!("{} inside {name}", pct(coverage.get())),
                Some(Measurement::Containment { .. }) if is_reference => {
                    "Contains the reference".to_owned()
                }
                Some(Measurement::Containment { .. }) => format!("Contains {name}"),
            };
            let unique = match link.unique_lines {
                0 => "—".to_owned(),
                1 => "1 line".to_owned(),
                n => format!("{} lines", projections::thousands(n)),
            };
            (matched, unique)
        }
    }

    fn pct(permille: u16) -> String {
        if permille.is_multiple_of(10) {
            format!("{}%", permille / 10)
        } else {
            format!("{}.{}%", permille / 10, permille % 10)
        }
    }

    /// Every visible member row's Match and Unique cells equal the oracle's.
    /// Returns how many rows were checked.
    fn assert_visible_labels(h: &Ui, oracle: &Oracle) -> usize {
        let results = results(h);
        let mut checked = 0;
        for row in rows(h) {
            let Some(name) = row.get("File name") else {
                continue;
            };
            let folder = row.get("Folder").cloned().unwrap_or_default();
            let Some(member) = results
                .groups
                .iter()
                .flat_map(|g| g.members.iter().map(move |m| (g, m)))
                .find(|(_, m)| &m.name == name && folder_cell(&m.folder) == folder)
            else {
                continue;
            };
            let (group, member) = member;
            // Header rows show a title, not a file name; they carry a
            // group label in the Group column.
            if row.get("Group").is_some_and(|g| g.contains('(')) {
                continue;
            }
            let (matched, unique) = oracle.cells(group, member.file);
            assert_eq!(
                row.get("Match").cloned().unwrap_or_default(),
                matched,
                "{name} Match"
            );
            assert_eq!(
                row.get("Unique").cloned().unwrap_or_default(),
                unique,
                "{name} Unique"
            );
            checked += 1;
        }
        checked
    }

    /// A member, not the reference, whose Make this the reference changes
    /// the counterpart of at least one other member of its group.
    fn reference_candidate(results: &ScanResults) -> (FileId, FileId) {
        results
            .groups
            .iter()
            .filter(|g| g.members.len() >= 3)
            .find_map(|g| {
                let m = g
                    .members
                    .iter()
                    .find(|m| !m.is_reference() && !m.folder_protected)?;
                let other = g
                    .members
                    .iter()
                    .find(|o| !o.is_reference() && o.file != m.file)?;
                Some((m.file, other.file))
            })
            .expect("a group of three")
    }

    // ── PV-T13-001 ───────────────────────────────────────────────────────

    #[test]
    fn findings_filters_sorts_and_expansion_keep_focus_and_queue() {
        let mut h = launch(ScenarioKind::Design);
        let draft = focus_named(&mut h, "Requirements_draft.txt");
        key(&mut h, Modifiers::NONE, Key::ArrowDown);
        assert_ne!(state(&h).findings.focused, Some(draft));
        click(&mut h, "Requirements_draft.txt");
        let focus = state(&h).findings.focused;
        assert_eq!(focus, Some(draft), "a click focuses the row");
        assert!(
            matches!(focus, Some(RowId::Member { .. })),
            "a click focuses the row"
        );
        let queue = queued(&h);
        assert_eq!(queue.len(), 3, "the design scenario queues three files");

        let check = |h: &Ui, after: &str| {
            assert_eq!(state(h).findings.focused, focus, "focus after {after}");
            assert_eq!(queued(h), queue, "queue after {after}");
        };
        for chip in [
            "Same bytes 42",
            "Same text 16",
            "Similar text 20",
            "Containment 6",
            "All 84",
        ] {
            press(&mut h, chip);
            check(&h, chip);
        }
        let mut current = "Match type";
        for item in [
            "Group size",
            "Total size",
            "Newest change",
            "Folder",
            "Match type",
        ] {
            press(&mut h, &format!("Sort: {current}"));
            press(&mut h, item);
            check(&h, item);
            current = item;
        }
        assert_eq!(state(&h).findings.sort, SortKey::default());
        press(&mut h, "Actions ▾");
        press(&mut h, "Expand all groups");
        check(&h, "Expand all groups");
        assert_eq!(state(&h).findings.expanded.len(), results(&h).groups.len());
        press(&mut h, "Actions ▾");
        press(&mut h, "Collapse all groups");
        check(&h, "Collapse all groups");
        assert!(state(&h).findings.expanded.is_empty());

        // Frame 01e: a search that matches no row hides every queued file.
        h.get_by_label("Search file names or folders").focus();
        h.get_by_label("Search file names or folders")
            .type_text("guide v9");
        settle(&mut h);
        check(&h, "the search");
        assert_eq!(
            projections::status_line(state(&h)).right,
            "3 queued, 3 outside the search"
        );
        assert!(h.query_by_label("No files match “guide v9”").is_some());
        assert!(
            h.query_by_label("All 0").is_some(),
            "counts follow the search"
        );
        press(&mut h, "Clear search and filters");
        assert!(state(&h).findings.search.is_empty());
        assert_eq!(projections::status_line(state(&h)).right, "3 queued");

        // The search field's own clear control.
        state_mut(&mut h).findings.search = "Notes".to_owned();
        settle(&mut h);
        press(&mut h, "Clear search");
        assert!(
            state(&h).findings.search.is_empty(),
            "search clear empties it"
        );
        check(&h, "search clear");
    }

    // ── PV-T13-002 ───────────────────────────────────────────────────────

    fn labels_follow_the_counterpart(kind: ScenarioKind) {
        let mut h = launch(kind);
        expand_all(&mut h);
        let scenario = Arc::clone(h.state().scenario.as_ref().expect("scenario"));
        let results = results(&h);
        let mut oracle = Oracle::new(&scenario, &results);
        assert!(
            assert_visible_labels(&h, &oracle) > 0,
            "{kind:?}: rows were read"
        );

        let (new_reference, _) = reference_candidate(&results);
        reveal(&mut h, row_of(&results, new_reference));
        key(&mut h, Modifiers::SHIFT, Key::F10);
        press(&mut h, "Make this the reference");
        oracle.apply(ReviewCommand::MakeReference {
            file: new_reference,
        });
        assert_eq!(
            member_row(&h, new_reference)
                .get("Decision")
                .map(String::as_str),
            Some("Reference")
        );
        assert!(
            assert_visible_labels(&h, &oracle) > 0,
            "{kind:?}: rows after the change"
        );
    }

    #[test]
    fn findings_member_labels_name_their_counterpart_on_mixed() {
        labels_follow_the_counterpart(ScenarioKind::Mixed);
    }

    #[test]
    fn findings_member_labels_name_their_counterpart_on_large() {
        labels_follow_the_counterpart(ScenarioKind::Large);
    }

    #[test]
    fn findings_focus_and_queue_toggle_independently() {
        let mut h = launch(ScenarioKind::Mixed);
        expand_all(&mut h);
        let results = results(&h);
        let target = results
            .groups
            .iter()
            .flat_map(|g| &g.members)
            .find(|m| {
                !m.is_reference()
                    && !m.folder_protected
                    && unique_name(&results, &m.name)
                    && matches!(m.measurement(), Some(Measurement::Identical(_)))
            })
            .expect("an identical member")
            .clone();
        let other = results
            .groups
            .iter()
            .flat_map(|g| &g.members)
            .find(|m| m.file != target.file && unique_name(&results, &m.name))
            .expect("another file")
            .clone();
        reveal(&mut h, row_of(&results, other.file));
        let focus = state(&h).findings.focused;
        assert_eq!(focus, Some(row_of(&results, other.file)));

        click(&mut h, &format!("Queue {}", target.name));
        assert!(queued(&h).contains(&target.file), "the checkbox queues");
        assert_eq!(
            state(&h).findings.focused,
            focus,
            "the checkbox leaves focus"
        );
        assert_eq!(state(&h).workspace, WorkspaceState::Findings, "no Compare");

        // Focus moves off the target by key, then a click on its name
        // brings it back: the click itself focuses the row.
        reveal(&mut h, row_of(&results, target.file));
        key(&mut h, Modifiers::NONE, Key::ArrowDown);
        assert_ne!(
            state(&h).findings.focused,
            Some(row_of(&results, target.file))
        );
        click(&mut h, &target.name);
        assert_eq!(
            state(&h).findings.focused,
            Some(row_of(&results, target.file))
        );
        assert!(
            queued(&h).contains(&target.file),
            "focusing leaves the queue"
        );

        click(&mut h, &format!("Queue {}", target.name));
        assert!(!queued(&h).contains(&target.file), "the checkbox unqueues");
        assert_eq!(
            state(&h).findings.focused,
            Some(row_of(&results, target.file))
        );
    }

    // ── PV-T13-003 ───────────────────────────────────────────────────────

    #[test]
    fn findings_columns_have_the_reference_widths() {
        let h = launch(ScenarioKind::Design);
        let columns = header(&h);
        let titles: Vec<&str> = columns.iter().map(|(t, _)| *t).collect();
        assert_eq!(titles, COLUMNS);
        for (i, column) in ColumnId::ALL.into_iter().enumerate().take(9) {
            let width = columns[i + 1].1 - columns[i].1;
            assert!(
                (width - column.reference_width()).abs() <= 1.0,
                "{} is {width} wide",
                COLUMNS[i]
            );
        }
    }

    #[test]
    fn findings_columns_collapse_only_below_1280() {
        for (width, shown) in [(1279.0, false), (1280.0, true)] {
            let h = launch_with(
                Some(ScenarioKind::Design),
                Vec2::new(width, 800.0),
                OpenRecorder::default(),
            );
            let titles: Vec<&str> = header(&h).iter().map(|(t, _)| *t).collect();
            // At 1280 the 1408-unit table is wider than the work area and
            // scrolls sideways; `egui_table` draws only the columns in view,
            // so Protected, the right-most of the three, is off screen there.
            for column in ["Size", "Modified"] {
                assert_eq!(titles.contains(&column), shown, "{column} at {width}");
            }
            if !shown {
                assert!(!titles.contains(&"Protected"), "Protected at {width}");
            }
            if !shown {
                // Collapsed, the table fits the work area, so the last
                // column is on screen.
                assert!(titles.contains(&"Decision"), "Decision at {width}");
            }
        }
    }

    #[test]
    fn findings_sort_and_actions_menus_list_their_items() {
        let mut h = launch(ScenarioKind::Design);
        press(&mut h, "Sort: Match type");
        for item in [
            "Match type",
            "Group size",
            "Total size",
            "Newest change",
            "Folder",
        ] {
            assert!(
                h.query_by_role_and_label(Role::Button, item).is_some(),
                "Sort item {item}"
            );
        }
        key(&mut h, Modifiers::NONE, Key::Escape);
        assert!(state(&h).dialog.menu().is_none(), "Esc closes the menu");
        press(&mut h, "Actions ▾");
        for item in [
            "Queue identical copies…",
            "Expand all groups",
            "Collapse all groups",
            "Choose columns…",
        ] {
            assert!(h.query_by_label(item).is_some(), "Actions item {item}");
        }
        press(&mut h, "Queue identical copies…");
        assert_eq!(
            state(&h).dialog.dialog().map(|d| d.kind),
            Some(DialogKind::QueueIdentical)
        );
    }

    const ROW_MENU: [&str; 6] = [
        "Review unique text…",
        "Keep — do not queue",
        "Make this the reference",
        "Protect this file",
        "Open containing folder",
        "Copy path",
    ];

    #[test]
    fn findings_row_menu_opens_by_shift_f10_and_the_menu_key() {
        let mut h = launch(ScenarioKind::Design);
        focus_named(&mut h, "Requirements_draft.txt");
        for (modifiers, chord) in [(Modifiers::SHIFT, Key::F10), (Modifiers::NONE, Key::Menu)] {
            key(&mut h, modifiers, chord);
            assert!(
                h.query_by_label("Compare with reference").is_some(),
                "{chord:?}"
            );
            for item in ROW_MENU {
                assert!(h.query_by_label(item).is_some(), "{chord:?}: {item}");
            }
            key(&mut h, Modifiers::NONE, Key::Escape);
            assert!(state(&h).dialog.menu().is_none());
        }
        // A row measured against another member names it (frame 01).
        focus_named(&mut h, "Requirements_final.txt");
        right_click(&mut h, "Requirements_final.txt");
        assert!(
            h.query_by_label("Compare with Requirements_v2.txt")
                .is_some()
        );
        press(&mut h, "Compare with Requirements_v2.txt");
        assert_eq!(state(&h).workspace, WorkspaceState::Compare);
        let pair = state(&h).compare.pair.expect("pair");
        let results = results(&h);
        let group = results.group_of(pair.target).unwrap();
        assert_eq!(
            group.member(pair.target).unwrap().name,
            "Requirements_final.txt"
        );
        assert_eq!(
            group.member(pair.counterpart).unwrap().name,
            "Requirements_v2.txt"
        );
    }

    #[test]
    fn findings_enter_compares_and_space_queues_through_the_review_service() {
        let mut h = launch(ScenarioKind::Design);
        let row = focus_named(&mut h, "Design_Spec_draft.txt");
        key(&mut h, Modifiers::NONE, Key::Enter);
        assert_eq!(state(&h).workspace, WorkspaceState::Compare);
        assert_eq!(state(&h).compare.return_row, Some(row));
        state_mut(&mut h).workspace = WorkspaceState::Findings;
        settle(&mut h);

        // A Review first row: Space opens Review unique text, queues nothing.
        let before = queued(&h);
        focus_named(&mut h, "Design_Spec_draft.txt");
        key(&mut h, Modifiers::NONE, Key::Space);
        assert_eq!(
            state(&h).dialog.dialog().map(|d| d.kind),
            Some(DialogKind::UniqueTextReview)
        );
        assert_eq!(queued(&h), before);
    }

    #[test]
    fn findings_refusal_and_reference_change_reach_decision_and_status() {
        let mut h = launch(ScenarioKind::Mixed);
        expand_all(&mut h);
        let results = results(&h);
        // A chain: C measured as identical against B, which is not the
        // reference. Queueing C makes B the counterpart of a queued file.
        let (c, b) = results
            .groups
            .iter()
            .flat_map(|g| g.members.iter().map(move |m| (g, m)))
            .find_map(|(g, m)| {
                let link = m.link()?;
                let b = g.member(link.counterpart)?;
                (matches!(link.measurement, Some(Measurement::Identical(_)))
                    && unique_name(&results, &b.name)
                    && !b.is_reference()
                    && !m.folder_protected
                    && !b.folder_protected)
                    .then(|| (m.clone(), b.clone()))
            })
            .expect("an identical chain in mixed");
        reveal(&mut h, row_of(&results, c.file));
        key(&mut h, Modifiers::NONE, Key::Space);
        assert!(
            queued(&h).contains(&c.file),
            "Space queues an identical row: {:?}",
            state(&h).status
        );
        click(&mut h, &format!("Queue {}", b.name));
        assert!(!queued(&h).contains(&b.file));
        assert_eq!(
            state(&h).queue.last_refusal,
            Some((b.file, Refusal::CounterpartOfQueuedFile))
        );
        assert_eq!(
            member_row(&h, b.file).get("Decision").map(String::as_str),
            Some("Counterpart of a queued file")
        );

        // A member whose Make this the reference re-aims C away from B, so
        // C leaves the queue; the oracle engine, holding the same queue,
        // says which member and how many files leave.
        let scenario = Arc::clone(h.state().scenario.as_ref().expect("scenario"));
        let group = results.group_of(c.file).expect("group");
        let (chosen, unqueued) = group
            .members
            .iter()
            .filter(|m| m.file != c.file && !m.is_reference())
            .find_map(|m| {
                let mut oracle = Oracle::new(&scenario, &results);
                oracle.apply(ReviewCommand::RequestQueue {
                    file: c.file,
                    counterpart: b.file,
                    origin: QueueOrigin::Shortcut,
                    reviewed: None,
                });
                match oracle.apply(ReviewCommand::MakeReference { file: m.file }) {
                    ReviewStatus::ReferenceChanged { unqueued } if unqueued > 0 => {
                        Some((m.file, unqueued))
                    }
                    _ => None,
                }
            })
            .expect("a reference change that unqueues C");
        reveal(&mut h, row_of(&results, chosen));
        key(&mut h, Modifiers::SHIFT, Key::F10);
        press(&mut h, "Make this the reference");
        assert!(!queued(&h).contains(&c.file), "C left the queue");
        let files = if unqueued == 1 { "file" } else { "files" };
        assert_eq!(
            state(&h).status.as_ref().map(|s| s.text.clone()),
            Some(format!(
                "Reference changed; {unqueued} {files} left the queue."
            ))
        );
        assert_eq!(
            member_row(&h, chosen).get("Decision").map(String::as_str),
            Some("Reference")
        );
    }

    #[test]
    fn findings_match_type_counts_count_a_mixed_group_under_each_type() {
        let h = launch(ScenarioKind::Large);
        let results = results(&h);
        let mut by_kind: BTreeMap<RelationKind, usize> = BTreeMap::new();
        let mut mixed = 0;
        for group in &results.groups {
            let kinds = group.row_kinds();
            mixed += usize::from(kinds.len() > 1);
            for kind in kinds {
                *by_kind.entry(kind).or_default() += 1;
            }
        }
        assert!(mixed > 0, "large holds Mixed groups");
        let all = format!(
            "All {}",
            projections::thousands(results.groups.len() as u64)
        );
        assert!(h.query_by_label(&all).is_some(), "{all}");
        for (kind, name) in [
            (RelationKind::SameBytes, "Same bytes"),
            (RelationKind::SameText, "Same text"),
            (RelationKind::SimilarText, "Similar text"),
            (RelationKind::Containment, "Containment"),
        ] {
            let label = format!(
                "{name} {}",
                projections::thousands(by_kind.get(&kind).copied().unwrap_or(0) as u64)
            );
            assert!(h.query_by_label(&label).is_some(), "{label}");
        }
    }

    #[test]
    fn findings_states_before_scan_scanning_issues_and_empty() {
        // 01a: before the first scan.
        let mut h = launch_with(None, Vec2::new(1600.0, 960.0), OpenRecorder::default());
        assert!(h.query_by_label("No findings yet").is_some());
        assert!(h.query_by_label("All").is_some(), "chips without counts");
        press(&mut h, "Choose folders to scan");
        assert_eq!(
            state(&h).dialog.dialog().map(|d| d.kind),
            Some(DialogKind::NewScan)
        );

        // 01b: scanning, with Stop scan.
        let mut h = launch(ScenarioKind::Empty);
        let scan = state(&h).scan.scan;
        let progress = ScanProgress {
            current_folder: "~/Documents/Archive/".to_owned(),
            files_read: 8214,
            groups_found: 61,
            elapsed: std::time::Duration::from_secs(42),
        };
        state_mut(&mut h).scan.stage = ScanStage::Running;
        state_mut(&mut h).scan.progress = Some(progress);
        settle(&mut h);
        assert!(h.query_by_label("Scanning…").is_some());
        assert!(
            h.query_by_label("Reading files in ~/Documents/Archive/")
                .is_some()
        );
        assert!(
            h.query_by_label("8,214 files read · 61 groups so far · 0:42 elapsed")
                .is_some()
        );
        press(&mut h, "Stop scan");
        assert_eq!(state(&h).scan.scan, scan);
        assert_eq!(
            state(&h).scan.stage,
            ScanStage::Stopping,
            "Stop requests cancellation"
        );

        // A scan that found no relationships.
        let h = launch(ScenarioKind::Empty);
        assert!(h.query_by_label("No relationships found").is_some());
        assert!(h.query_by_label("No findings yet").is_none());

        // 01c: discovery problems stay accessible.
        let mut h = launch(ScenarioKind::Issues);
        let issues = state(&h).scan.issues.clone();
        assert!(!issues.is_empty());
        let note = format!(
            "{} files could not be read",
            projections::thousands(issues.len() as u64)
        );
        assert!(h.query_by_label_contains(&note).is_some(), "{note}");
        click(&mut h, "Show details");
        for issue in &issues {
            assert!(
                h.query_by_label_contains(&issue.display_path).is_some(),
                "{}",
                issue.display_path
            );
        }
    }

    #[test]
    fn findings_open_containing_folder_opens_the_row_folder_or_reports() {
        for fail in [false, true] {
            let recorder = OpenRecorder {
                fail,
                ..OpenRecorder::default()
            };
            let opened = Arc::clone(&recorder.opened);
            let mut h = launch_with(
                Some(ScenarioKind::Design),
                Vec2::new(1600.0, 960.0),
                recorder,
            );
            focus_named(&mut h, "Requirements_draft.txt");
            key(&mut h, Modifiers::SHIFT, Key::F10);
            press(&mut h, "Open containing folder");
            let results = results(&h);
            let member = results
                .groups
                .iter()
                .flat_map(|g| &g.members)
                .find(|m| m.name == "Requirements_draft.txt")
                .unwrap();
            assert_eq!(
                opened.lock().unwrap().as_slice(),
                [Location::Folder(PathBuf::from(&member.folder))]
            );
            let status = state(&h).status.as_ref().map(|s| s.text.clone());
            if fail {
                let text = status.expect("an actionable status message");
                assert!(text.contains("is not a folder"), "{text}");
            } else {
                assert!(status.is_none());
            }
        }
    }
}
