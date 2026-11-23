// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! PV-T33-001 to PV-T33-005 (plan T33): the presentation hub mirrors what
//! the review service reports, and views read shared state.
//!
//! Every test sits in the `presentation_hub` module so
//! `-E 'test(presentation_hub)'` selects them. Each leg of T33 keeps its
//! tests and helpers in its own submodule.

mod presentation_hub {
    /// Step bound for every `Harness::run` in both submodules, in place of kittest's
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

    // ── Leg A: PV-T33-001 (recorded roles) and PV-T33-002 (forecast) ─────

    /// Review histories run against the real C-05 service
    /// (`fuzzy_dedupe_engine::review::ReviewService`), mirrored into a
    /// `PresentationState` exactly as `commands::review` does: apply, then
    /// mirror the published validation view, then `events::apply_review`.
    /// Driving the service directly, rather than through the engine facade,
    /// lets a test supply Compare evidence (`ReviewedPair`) without running
    /// a comparison job per step; the facade adds only the resolution of a
    /// request ID to that pair.
    ///
    /// Histories come from a hand-rolled seeded generator (plan D-011), so
    /// a failure is replayed by its scenario, seed and step, which every
    /// assertion message names.
    mod roles_and_forecast {
        use std::collections::{BTreeMap, BTreeSet};
        use std::path::Path;
        use std::sync::Arc;

        use egui::Vec2;
        use egui::accesskit::Role;
        use egui_kittest::Harness;
        use egui_kittest::kittest::{NodeT as _, Queryable};
        use fuzzy_dedupe_app::presentation::commands::{self, IntentQueue, Services, UiIntent};
        use fuzzy_dedupe_app::presentation::events;
        use fuzzy_dedupe_app::presentation::projections;
        use fuzzy_dedupe_app::presentation::state::{
            DialogKind, DialogSubject, IntendedAction, PresentationState, QueueEntry, ScanStage,
            Severity,
        };
        use fuzzy_dedupe_app::ui::dialogs::queue_identical;
        use fuzzy_dedupe_app::ui::theme;
        use fuzzy_dedupe_app::ui::widgets::dialog_host;
        use fuzzy_dedupe_domain::{
            CompareMode, FileId, Group, Measurement, MemberDecision, MemberRole, QueueOrigin,
            QueueScope, RequestId, ReviewedPair, ScanId, ScanResults, ValidationView, Verification,
        };
        use fuzzy_dedupe_engine::fixtures::{self, Scenario, ScenarioKind, SeededDecision};
        use fuzzy_dedupe_engine::review::{
            ReviewCommand, ReviewOutcome, ReviewService, ReviewStatus,
        };
        use fuzzy_dedupe_engine::service::{Engine, EngineConfig};
        use fuzzy_dedupe_platform::dirs::AppDirs;
        use fuzzy_dedupe_platform::open::{Location, OpenHandle, Opener};
        use fuzzy_dedupe_platform::picker::{FolderPicker, PickHandle, PickOutcome, PickRequest};

        // ── Seeded generator ─────────────────────────────────────────────

        /// `SplitMix64`: small, fast, and fully determined by its seed.
        struct Rng(u64);

        impl Rng {
            fn next(&mut self) -> u64 {
                self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
                let mut z = self.0;
                z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
                z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
                z ^ (z >> 31)
            }

            fn below(&mut self, n: usize) -> usize {
                usize::try_from(self.next() % n as u64).expect("fits")
            }

            fn chance(&mut self, percent: u64) -> bool {
                self.next() % 100 < percent
            }
        }

        // ── A review history mirrored into presentation ──────────────────

        struct History {
            service: ReviewService,
            state: PresentationState,
            results: Arc<ScanResults>,
            /// Each file's role: the results' own, replaced by every role an
            /// outcome reported. The test's independent record, never read
            /// from presentation.
            roles: BTreeMap<FileId, MemberRole>,
            /// Who to blame in an assertion message.
            at: String,
        }

        fn scenario(kind: ScenarioKind, scan: u64) -> Scenario {
            fixtures::build(kind, ScanId::new(scan), fixtures::DEFAULT_SEED).expect("scenario")
        }

        fn seed_commands(seed: SeededDecision) -> Vec<ReviewCommand> {
            match seed {
                SeededDecision::Kept(file) => vec![ReviewCommand::MarkKept { file, kept: true }],
                SeededDecision::Protect(file) => vec![ReviewCommand::ProtectFile {
                    file,
                    protected: true,
                }],
                SeededDecision::MakeReference(file) => {
                    vec![ReviewCommand::MakeReference { file }]
                }
                SeededDecision::Queue { file, counterpart } => {
                    vec![ReviewCommand::RequestQueue {
                        file,
                        counterpart,
                        origin: QueueOrigin::Checkbox,
                        reviewed: None,
                    }]
                }
            }
        }

        impl History {
            /// `kind` loaded, with its seeded decisions replayed as
            /// `app.rs` replays them.
            fn new(kind: ScenarioKind) -> Self {
                let scenario = scenario(kind, 1);
                let results = Arc::clone(&scenario.results);
                let mut service = ReviewService::new();
                service.load_scan(Arc::clone(&results));
                let mut state = PresentationState::default();
                events::install_results(&mut state, Arc::clone(&results), ScanStage::Completed);
                let roles = role_map(&results);
                let mut h = Self {
                    service,
                    state,
                    results,
                    roles,
                    at: format!("{kind:?} seeds"),
                };
                h.sync();
                for seed in &scenario.seeded_decisions {
                    for command in seed_commands(*seed) {
                        if let ReviewStatus::NeedsReview { file, counterpart } =
                            h.run(command.clone(), None).status
                        {
                            h.run(
                                ReviewCommand::AcknowledgeUniqueText {
                                    file,
                                    counterpart,
                                    reviewed: None,
                                },
                                None,
                            );
                            h.run(command, None);
                        }
                    }
                }
                h
            }

            fn view(&self) -> ValidationView {
                self.service
                    .validation_view()
                    .expect("implemented")
                    .expect("a scan is loaded")
            }

            fn sync(&mut self) {
                let view = self.view();
                events::sync_view(&mut self.state, Arc::new(view));
            }

            /// One command, mirrored in `commands::review`'s order, with the
            /// PV-T33-001 checks after it.
            fn run(
                &mut self,
                command: ReviewCommand,
                reviewed: Option<ReviewedPair>,
            ) -> ReviewOutcome {
                let what = format!("{command:?}");
                let outcome = self.service.apply(command, reviewed);
                self.sync();
                events::apply_review(&mut self.state, &outcome);
                for row in &outcome.changed {
                    self.roles.insert(row.file, row.role);
                }
                self.check_roles(&outcome, &what);
                outcome
            }

            /// After every outcome: the read model holds each reported
            /// role, and every member's projected counterpart and evidence
            /// is the one its current role names.
            fn check_roles(&self, outcome: &ReviewOutcome, command: &str) {
                let at = &self.at;
                for row in &outcome.changed {
                    let member = self.member(row.file);
                    assert_eq!(
                        self.state.queue.role(member),
                        row.role,
                        "{at}: {command} reported {:?}'s role, the read model holds another",
                        row.file
                    );
                }
                for group in &self.results.groups {
                    for m in &group.members {
                        let view = projections::member_view(group, &self.state.queue, m);
                        let (counterpart, measurement, unique) = match self.roles[&m.file] {
                            MemberRole::Reference => (None, None, None),
                            MemberRole::Member(link) => (
                                Some(link.counterpart),
                                link.measurement,
                                Some(link.unique_lines),
                            ),
                        };
                        assert_eq!(
                            view.counterpart, counterpart,
                            "{at}: after {command}, {:?}'s Findings counterpart",
                            m.file
                        );
                        if counterpart.is_some() {
                            assert_eq!(
                                (view.measurement, view.unique),
                                (measurement, unique),
                                "{at}: after {command}, {:?}'s Match and Unique evidence",
                                m.file
                            );
                        }
                    }
                }
            }

            fn member(&self, file: FileId) -> &fuzzy_dedupe_domain::Member {
                self.group_of(file).member(file).expect("member")
            }

            fn group_of(&self, file: FileId) -> &Group {
                self.results.group_of(file).expect("file in a group")
            }

            fn counterpart(&self, file: FileId) -> Option<FileId> {
                match self.roles[&file] {
                    MemberRole::Reference => None,
                    MemberRole::Member(link) => Some(link.counterpart),
                }
            }

            /// Compare evidence for the pair as it is now, as the engine
            /// would resolve it from a fresh comparison.
            fn evidence(&self, target: FileId, counterpart: FileId, unique: u64) -> ReviewedPair {
                let view = self.view();
                ReviewedPair {
                    scan: view.scan,
                    target,
                    target_snapshot: view.snapshots[&target],
                    counterpart,
                    counterpart_snapshot: view.snapshots[&counterpart],
                    rules: view.rules,
                    mode: CompareMode::Original,
                    unique_lines: unique,
                }
            }

            /// A Compare review that re-aims `file` onto `onto` without
            /// queueing it: the Review unique text acknowledgment sent from
            /// Compare.
            fn re_aim(&mut self, file: FileId, onto: FileId, unique: u64) -> ReviewOutcome {
                let pair = self.evidence(file, onto, unique);
                self.run(
                    ReviewCommand::AcknowledgeUniqueText {
                        file,
                        counterpart: onto,
                        reviewed: Some(RequestId::new(1)),
                    },
                    Some(pair),
                )
            }
        }

        fn role_map(results: &ScanResults) -> BTreeMap<FileId, MemberRole> {
            results
                .groups
                .iter()
                .flat_map(|g| g.members.iter().map(|m| (m.file, m.role)))
                .collect()
        }

        // ── The popup's forecast and the service's answer ────────────────

        /// What the popup states for `scope`, read through the popup's own
        /// forecast entry point over the presentation read model.
        struct Stated {
            files: usize,
            set: BTreeSet<FileId>,
            bytes: u64,
        }

        fn stated(state: &PresentationState, scope: &QueueScope) -> Stated {
            let results = state.scan.results.as_deref().expect("results");
            let forecast = queue_identical::forecast(results, scope, &state.queue);
            Stated {
                files: forecast.files.len(),
                set: forecast.files.iter().copied().collect(),
                bytes: forecast.bytes,
            }
        }

        fn size_of(results: &ScanResults, files: &BTreeSet<FileId>) -> u64 {
            results
                .groups
                .iter()
                .flat_map(|g| g.members.iter())
                .filter(|m| files.contains(&m.file))
                .map(|m| m.size_bytes)
                .sum()
        }

        /// Runs Queue identical copies over `scope` and checks the popup's
        /// forecast, made immediately before, against the service's
        /// `Queued { files }` and the files the published view newly
        /// queues.
        fn bulk(h: &mut History, scope: QueueScope) -> usize {
            let stated = stated(&h.state, &scope);
            let before: BTreeSet<FileId> = h.view().queue.iter().map(|q| q.target).collect();
            let outcome = h.run(ReviewCommand::QueueIdenticalCopies { scope }, None);
            let after: BTreeSet<FileId> = h.view().queue.iter().map(|q| q.target).collect();
            let added: BTreeSet<FileId> = after.difference(&before).copied().collect();
            let at = &h.at;
            assert_eq!(
                outcome.status,
                ReviewStatus::Queued {
                    files: stated.files
                },
                "{at}: the popup's count against the service's"
            );
            assert_eq!(stated.set, added, "{at}: the popup's file set");
            assert_eq!(
                stated.bytes,
                size_of(&h.results, &added),
                "{at}: the popup's total size"
            );
            added.len()
        }

        // ── The generated histories (PV-T33-002 part 1) ──────────────────

        /// Scenarios the histories run on: the design scenario and the
        /// small edge scenarios. `Large` is left out: its 100,000 rows
        /// would make each step's full-table check cost seconds, and it
        /// adds no rule the others lack.
        const SCENARIOS: [ScenarioKind; 4] = [
            ScenarioKind::Design,
            ScenarioKind::Mixed,
            ScenarioKind::Stale,
            ScenarioKind::Unicode,
        ];
        const SEEDS_PER_SCENARIO: u64 = 60;
        const STEPS: usize = 48;

        /// A queue request from a Findings control against the row's current
        /// counterpart, acknowledged and repeated, most of the time, when it
        /// needs Review unique text first.
        fn queue_request(h: &mut History, rng: &mut Rng, file: FileId, other: FileId) {
            let counterpart = h.counterpart(file).unwrap_or(other);
            let origin = [
                QueueOrigin::Checkbox,
                QueueOrigin::RowMenu,
                QueueOrigin::Shortcut,
            ][rng.below(3)];
            let request = ReviewCommand::RequestQueue {
                file,
                counterpart,
                origin,
                reviewed: None,
            };
            if let ReviewStatus::NeedsReview { file, counterpart } =
                h.run(request.clone(), None).status
                && rng.chance(60)
            {
                h.run(
                    ReviewCommand::AcknowledgeUniqueText {
                        file,
                        counterpart,
                        reviewed: None,
                    },
                    None,
                );
                h.run(request, None);
            }
        }

        /// One random step on `h`. Members are picked from groups of two or
        /// more so every command has a row and a second member to aim at.
        fn step(h: &mut History, rng: &mut Rng, evidence: &[fuzzy_dedupe_domain::EvidenceUpdate]) {
            let groups: Vec<usize> = (0..h.results.groups.len())
                .filter(|&i| h.results.groups[i].members.len() >= 2)
                .collect();
            let group = &h.results.groups[groups[rng.below(groups.len())]];
            let file = group.members[rng.below(group.members.len())].file;
            let others: Vec<FileId> = group
                .members
                .iter()
                .map(|m| m.file)
                .filter(|f| *f != file)
                .collect();
            let other = others[rng.below(others.len())];
            let group_id = group.id;
            let unique = rng.next() % 3;
            match rng.below(14) {
                0 | 1 => queue_request(h, rng, file, other),
                2 => {
                    let pair = h.evidence(file, other, unique);
                    h.run(
                        ReviewCommand::RequestQueue {
                            file,
                            counterpart: other,
                            origin: QueueOrigin::Compare,
                            reviewed: Some(RequestId::new(1)),
                        },
                        Some(pair),
                    );
                }
                3 | 4 => {
                    h.re_aim(file, other, unique);
                }
                5 => {
                    let counterpart = h.counterpart(file).unwrap_or(other);
                    h.run(
                        ReviewCommand::AcknowledgeUniqueText {
                            file,
                            counterpart,
                            reviewed: None,
                        },
                        None,
                    );
                }
                6 => {
                    let pair = h.evidence(file, other, unique);
                    h.run(
                        ReviewCommand::KeepBoth {
                            file,
                            counterpart: other,
                            reviewed: RequestId::new(1),
                        },
                        Some(pair),
                    );
                }
                7 => {
                    let kept = rng.chance(70);
                    h.run(ReviewCommand::MarkKept { file, kept }, None);
                }
                8 => {
                    let protected = rng.chance(70);
                    h.run(ReviewCommand::ProtectFile { file, protected }, None);
                }
                9 => {
                    h.run(ReviewCommand::MakeReference { file }, None);
                }
                10 => {
                    h.run(ReviewCommand::RemoveFromQueue { file }, None);
                }
                11 => {
                    bulk(h, QueueScope::AllByteIdenticalGroups);
                }
                12 => {
                    let ids = h.results.groups.iter().map(|g| g.id);
                    let ids = ids.filter(|id| *id == group_id || rng.chance(50));
                    bulk(h, QueueScope::Groups(ids.collect()));
                }
                _ => {
                    if evidence.is_empty() {
                        bulk(h, QueueScope::AllByteIdenticalGroups);
                    } else {
                        let update = evidence[rng.below(evidence.len())];
                        h.run(ReviewCommand::UpdateEvidence(update), None);
                    }
                }
            }
        }

        #[test]
        fn presentation_hub_forecast_equals_what_the_service_queues_over_seeded_histories() {
            let mut histories = 0;
            let mut bulk_commands = 0;
            for kind in SCENARIOS {
                let evidence = scenario(kind, 1).evidence_changes;
                for seed in 0..SEEDS_PER_SCENARIO {
                    let mut h = History::new(kind);
                    let mut rng = Rng(seed ^ 0x7333_A000);
                    for n in 0..STEPS {
                        h.at = format!("{kind:?} seed {seed} step {n}");
                        step(&mut h, &mut rng, &evidence);
                    }
                    // Every history ends with both scopes, so none passes
                    // without its forecast being checked.
                    h.at = format!("{kind:?} seed {seed} step {STEPS}");
                    bulk(&mut h, QueueScope::AllByteIdenticalGroups);
                    let shown: Vec<_> = h.results.groups.iter().step_by(2).map(|g| g.id).collect();
                    h.at = format!("{kind:?} seed {seed} step {}", STEPS + 1);
                    bulk(&mut h, QueueScope::Groups(shown));
                    histories += 1;
                    bulk_commands += 2;
                }
            }
            assert!(histories >= 200, "{histories} histories");
            assert!(bulk_commands >= 400);
        }

        // ── Targeted cases (PV-T33-001) ──────────────────────────────────

        /// An unqueued, unprotected, unkept member `x` measured against its
        /// group's reference, and a second member `y`, not the reference.
        /// With `exact`, `x` is a byte-verified copy of the reference:
        /// re-aiming it onto `y` is the history the read model's old guess
        /// over-counted. With `scored`, `y` has a direct edge to `x`, so the
        /// re-aimed Match label has a score to show.
        fn re_aim_case(h: &History, exact: bool, scored: bool) -> Option<(FileId, FileId)> {
            let queue = &h.state.queue;
            h.results.groups.iter().find_map(|g| {
                let reference = g.reference()?.file;
                g.members.iter().find_map(|x| {
                    let free = !queue.is_queued(x.file)
                        && !queue.is_protected(x.file)
                        && queue.decisions.get(&x.file) != Some(&MemberDecision::Kept);
                    let copy = g.edge_between(x.file, reference).is_some_and(|e| {
                        e.measurement == Measurement::Identical(Verification::ByteVerified)
                    });
                    if !(free && (copy || !exact) && h.counterpart(x.file) == Some(reference)) {
                        return None;
                    }
                    g.members
                        .iter()
                        .find(|y| {
                            y.file != reference
                                && y.file != x.file
                                && !queue.is_queued(y.file)
                                && (!scored || g.edge_between(x.file, y.file).is_some())
                        })
                        .map(|y| (x.file, y.file))
                })
            })
        }

        /// The first scenario, in [`SCENARIOS`] order, holding a case.
        fn with_re_aim_case(exact: bool, scored: bool) -> (History, FileId, FileId) {
            SCENARIOS
                .into_iter()
                .find_map(|kind| {
                    let h = History::new(kind);
                    let (x, y) = re_aim_case(&h, exact, scored)?;
                    Some((h, x, y))
                })
                .expect("a scenario with an exact copy and a second member")
        }

        #[test]
        fn presentation_hub_a_compare_re_aim_on_an_unqueued_row_names_the_member() {
            let (mut h, x, y) = with_re_aim_case(false, true);
            h.at = "re-aim label".to_owned();
            let outcome = h.re_aim(x, y, 0);
            assert_eq!(outcome.status, ReviewStatus::Applied);
            assert!(!h.state.queue.is_queued(x), "the re-aim queued nothing");
            let group = h.group_of(x);
            let member = group.member(x).expect("x");
            let view = projections::member_view(group, &h.state.queue, member);
            assert_eq!(view.counterpart, Some(y));
            let y_name = &group.member(y).expect("y").name;
            let label = projections::match_label(group, &view);
            assert!(
                label.ends_with(y_name.as_str()) && !label.contains("reference"),
                "the label names {y_name}: {label:?}"
            );
            assert_eq!(
                projections::compare_item_label(group, &view),
                format!("Compare with {y_name}")
            );
        }

        #[test]
        fn presentation_hub_make_this_the_reference_moves_the_anchor_and_relabels() {
            let mut h = History::new(ScenarioKind::Mixed);
            h.at = "mixed make-reference".to_owned();
            let (group_index, new_reference) = h
                .results
                .groups
                .iter()
                .enumerate()
                .filter(|(_, g)| g.members.len() >= 3)
                .find_map(|(i, g)| {
                    let reference = g.reference()?.file;
                    g.members
                        .iter()
                        .find(|m| m.file != reference)
                        .map(|m| (i, m.file))
                })
                .expect("a group of three");
            let before = h.roles.clone();
            let outcome = h.run(
                ReviewCommand::MakeReference {
                    file: new_reference,
                },
                None,
            );
            assert!(matches!(
                outcome.status,
                ReviewStatus::ReferenceChanged { .. }
            ));
            let group = &h.results.groups[group_index];
            assert_eq!(
                projections::current_reference(group, &h.state.queue),
                Some(new_reference)
            );
            let relabelled = group
                .members
                .iter()
                .filter(|m| before[&m.file] != h.roles[&m.file])
                .count();
            assert!(relabelled >= 2, "the anchor and a counterpart changed");
            for m in &group.members {
                let view = projections::member_view(group, &h.state.queue, m);
                assert_eq!(view.reference, new_reference);
                assert_eq!(view.counterpart, h.counterpart(m.file), "{:?}", m.file);
            }
        }

        #[test]
        fn presentation_hub_a_new_scan_resets_recorded_roles_to_its_results() {
            let (mut h, x, y) = with_re_aim_case(true, false);
            h.at = "new scan".to_owned();
            h.re_aim(x, y, 0);
            assert_eq!(h.counterpart(x), Some(y), "the re-aim moved x");
            let fresh = scenario(ScenarioKind::Design, 2);
            events::install_results(
                &mut h.state,
                Arc::clone(&fresh.results),
                ScanStage::Completed,
            );
            let fresh_roles = role_map(&fresh.results);
            for group in &fresh.results.groups {
                for m in &group.members {
                    assert_eq!(h.state.queue.role(m), fresh_roles[&m.file]);
                    let view = projections::member_view(group, &h.state.queue, m);
                    assert_eq!(view.counterpart, m.counterpart(), "{:?}", m.file);
                }
            }
        }

        /// No Findings projection derives a role from decisions or queue
        /// entries: corrupting both leaves every member view unchanged.
        #[test]
        fn presentation_hub_findings_roles_ignore_decisions_and_queue_entries() {
            let mut h = History::new(ScenarioKind::Mixed);
            h.at = "mixed tamper".to_owned();
            let views = |state: &PresentationState, results: &ScanResults| {
                results
                    .groups
                    .iter()
                    .flat_map(|g| {
                        g.members
                            .iter()
                            .map(|m| projections::member_view(g, &state.queue, m))
                    })
                    .collect::<Vec<_>>()
            };
            let results = Arc::clone(&h.results);
            let honest = views(&h.state, &results);
            for group in &results.groups {
                let (Some(first), Some(last)) = (group.members.first(), group.members.last())
                else {
                    continue;
                };
                // Every member claims to be the reference, and each is
                // "queued" against the group's last member.
                for m in &group.members {
                    h.state
                        .queue
                        .decisions
                        .insert(m.file, MemberDecision::Reference);
                    h.state.queue.entries.insert(
                        m.file,
                        QueueEntry {
                            counterpart: if m.file == last.file {
                                first.file
                            } else {
                                last.file
                            },
                            action: IntendedAction::MoveToTrash,
                            acknowledgment: None,
                        },
                    );
                }
            }
            assert_eq!(views(&h.state, &results), honest);
        }

        // ── The popup on screen (PV-T33-002 part 2) ──────────────────────

        struct Popup {
            state: PresentationState,
            intents: IntentQueue,
        }

        fn popup_frame(ctx: &egui::Context, p: &mut Popup) {
            dialog_host::begin_frame(ctx, &mut p.state.dialog);
            dialog_host::show(ctx, &mut p.state, &mut p.intents);
        }

        fn label_of(node: &egui_kittest::Node<'_>) -> String {
            let node = node.accesskit_node();
            node.label()
                .filter(|label| !label.is_empty())
                .or_else(|| node.value())
                .unwrap_or_default()
        }

        /// After a re-aim history, the popup shows the forecast's count and
        /// size, its Queue N files button names that N, and the command it
        /// sends queues exactly that.
        #[test]
        fn presentation_hub_the_popup_shows_the_forecast_and_queues_it() {
            let (mut h, x, y) = with_re_aim_case(true, false);
            h.at = "popup".to_owned();
            h.re_aim(x, y, 0);
            let expected = stated(&h.state, &QueueScope::AllByteIdenticalGroups);
            assert!(
                !expected.set.contains(&x),
                "x, re-aimed off its reference, is not forecast"
            );
            assert!(expected.files > 0, "something to queue");

            let mut harness = Harness::builder()
                .with_max_steps(super::SETTLE_STEPS)
                .with_size(Vec2::new(1600.0, 960.0))
                .build_ui_state(
                    |ui, p: &mut Popup| popup_frame(&ui.ctx().clone(), p),
                    Popup {
                        state: h.state.clone(),
                        intents: IntentQueue::default(),
                    },
                );
            theme::install(&harness.ctx, Vec::new());
            let mut intents = IntentQueue::default();
            dialog_host::open(
                &mut harness.state_mut().state,
                DialogKind::QueueIdentical,
                DialogSubject::QueueIdentical(QueueScope::AllByteIdenticalGroups),
                None,
                &mut intents,
            )
            .expect("opens");
            harness.run();
            let summary = label_of(&harness.get_by_label_contains("will be queued"));
            let files = |n: usize| {
                if n == 1 {
                    "1 file".to_owned()
                } else {
                    format!("{n} files")
                }
            };
            assert!(
                summary.starts_with(&files(expected.files)),
                "the popup states the count: {summary}"
            );
            assert!(
                summary.ends_with(&queue_identical::binary_size(expected.bytes)),
                "the popup states the size: {summary}"
            );
            let button = format!("Queue {}", files(expected.files));
            harness.get_by_role_and_label(Role::Button, &button).click();
            harness.run();
            let sent: Vec<UiIntent> = harness.state_mut().intents.drain();
            let [UiIntent::Review { command, .. }] = sent.as_slice() else {
                panic!("one review intent, got {sent:?}");
            };
            let before: BTreeSet<FileId> = h.view().queue.iter().map(|q| q.target).collect();
            let outcome = h.run(command.clone(), None);
            assert_eq!(
                outcome.status,
                ReviewStatus::Queued {
                    files: expected.files
                }
            );
            let added: BTreeSet<FileId> = h
                .view()
                .queue
                .iter()
                .map(|q| q.target)
                .filter(|f| !before.contains(f))
                .collect();
            assert_eq!(added, expected.set);
        }

        // ── No eligibility rule in the app crate (PV-T33-002 part 4) ─────

        /// Every `.rs` file under `dir`, recursively, with its code lines:
        /// comment lines are dropped, so prose naming a term is not a hit.
        fn sources(dir: &Path, out: &mut Vec<(String, Vec<String>)>) {
            let entries = std::fs::read_dir(dir)
                .unwrap_or_else(|err| panic!("read {}: {err}", dir.display()));
            for entry in entries {
                let path = entry.expect("directory entry").path();
                if path.is_dir() {
                    sources(&path, out);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    let text = std::fs::read_to_string(&path)
                        .unwrap_or_else(|err| panic!("read {}: {err}", path.display()));
                    let code = text
                        .lines()
                        .filter(|l| !l.trim_start().starts_with("//"))
                        .map(str::to_owned)
                        .collect();
                    out.push((path.display().to_string(), code));
                }
            }
        }

        /// The terms only the Queue identical copies rule needs: whether an
        /// exact match was byte-verified, and how a counterpart was chosen
        /// or would be recomputed. The forecast reads them in the engine;
        /// none may appear in app code, where they could only rebuild a
        /// copy of the rule.
        const RULE_TERMS: [&str; 4] = [
            "Verification",
            "CounterpartBasis",
            "rule_counterpart",
            "ReviewedInCompare",
        ];

        #[test]
        fn presentation_hub_the_app_crate_holds_no_eligibility_condition() {
            let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
            let mut files = Vec::new();
            sources(&src, &mut files);
            assert!(files.len() > 10, "the scan read the crate");
            let mut forecast_calls = Vec::new();
            for (path, code) in &files {
                for (n, line) in code.iter().enumerate() {
                    for term in RULE_TERMS {
                        assert!(
                            !line.contains(term),
                            "{path}: code line {n} names {term}: {line}"
                        );
                    }
                    if line.contains("eligibility::queue_identical")
                        || line.contains("forecast::queue_identical")
                    {
                        forecast_calls.push(path.clone());
                    }
                }
            }
            assert_eq!(forecast_calls.len(), 1, "one call site: {forecast_calls:?}");
            assert!(
                forecast_calls[0].ends_with("ui/dialogs/queue_identical.rs"),
                "the popup calls the forecast: {forecast_calls:?}"
            );
        }

        // ── The bulk status line (PV-T33-002 part 3) ─────────────────────

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

        /// The real facade and dispatch, as `app.rs` wires them.
        fn rig() -> (PresentationState, Services) {
            let mut engine = Engine::new(EngineConfig::default()).expect("engine starts");
            let scan = engine.reserve_scan_id();
            let scenario = Arc::new(
                fixtures::build(ScenarioKind::Design, scan, fixtures::DEFAULT_SEED)
                    .expect("scenario"),
            );
            engine
                .adopt_scenario(Arc::clone(&scenario))
                .expect("adopted");
            let mut state = PresentationState::default();
            events::install_results(
                &mut state,
                Arc::clone(&scenario.results),
                ScanStage::Completed,
            );
            let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("presentation_hub");
            let mut services = Services::new(
                engine,
                Box::new(NoOpener),
                Box::new(DismissPicker),
                AppDirs::resolve(|_| None, &root.join("home")),
            );
            services.sync_view(&mut state);
            (state, services)
        }

        fn dispatch(
            state: &mut PresentationState,
            services: &mut Services,
            command: ReviewCommand,
        ) {
            let ctx = egui::Context::default();
            let mut followups = IntentQueue::default();
            commands::dispatch(
                UiIntent::Review {
                    command,
                    invoker: None,
                },
                state,
                services,
                &ctx,
                &mut followups,
            );
        }

        #[test]
        fn presentation_hub_a_bulk_result_sets_one_status_line_stating_n() {
            let (mut state, mut services) = rig();
            let expected = stated(&state, &QueueScope::AllByteIdenticalGroups).files;
            assert!(expected > 1, "the design scenario has copies to queue");
            dispatch(
                &mut state,
                &mut services,
                ReviewCommand::QueueIdenticalCopies {
                    scope: QueueScope::AllByteIdenticalGroups,
                },
            );
            assert_eq!(state.queue.queued_count(), expected);
            let status = state.status.clone().expect("a status line");
            assert_eq!(status.severity, Severity::Info);
            assert_eq!(status.text, format!("{expected} files queued."));

            // Nothing left to queue: a bulk result of 0 states nothing new.
            state.status = None;
            dispatch(
                &mut state,
                &mut services,
                ReviewCommand::QueueIdenticalCopies {
                    scope: QueueScope::AllByteIdenticalGroups,
                },
            );
            assert_eq!(state.status, None);
        }

        /// A single-file queue result still sets no status line.
        #[test]
        fn presentation_hub_a_single_queue_result_keeps_its_status_behavior() {
            let (mut state, mut services) = rig();
            let results = Arc::clone(state.scan.results.as_ref().expect("results"));
            // An unqueued exact copy measured against its reference, which
            // queues directly: no review is needed for Same bytes.
            let (file, counterpart) = results
                .groups
                .iter()
                .find_map(|g| {
                    let reference = projections::current_reference(g, &state.queue)?;
                    g.members.iter().find_map(|m| {
                        let exact = g.edge_between(m.file, reference).is_some_and(|e| {
                            e.measurement == Measurement::Identical(Verification::ByteVerified)
                        });
                        let view = projections::member_view(g, &state.queue, m);
                        (exact
                            && view.counterpart == Some(reference)
                            && !state.queue.is_queued(m.file)
                            && !state.queue.is_protected(m.file)
                            && !m.folder_protected)
                            .then_some((m.file, reference))
                    })
                })
                .expect("an exact copy of its reference");
            dispatch(
                &mut state,
                &mut services,
                ReviewCommand::RequestQueue {
                    file,
                    counterpart,
                    origin: QueueOrigin::Checkbox,
                    reviewed: None,
                },
            );
            assert!(state.queue.is_queued(file));
            assert_eq!(state.status, None);
        }
    }

    // ── Leg B: PV-T33-003 (menus), PV-T33-004 (titles), PV-T33-005 (scroll)

    /// The Findings view drawn headless through `egui_kittest`, on a
    /// scenario adopted through the app's own `app::adopt_scenario`, in the
    /// app's frame order: focus restore, global chords, menu keys, shell,
    /// dialog host, then intent dispatch.
    mod views {
        use std::path::Path;

        use egui::accesskit::Role;
        use egui::{Event, Key, Modifiers, PointerButton, Vec2};
        use egui_kittest::Harness;
        use egui_kittest::kittest::{NodeT as _, Queryable};
        use fuzzy_dedupe_app::app::adopt_scenario;
        use fuzzy_dedupe_app::presentation::commands::{self, IntentQueue, Services};
        use fuzzy_dedupe_app::presentation::projections;
        use fuzzy_dedupe_app::presentation::state::{
            MenuKind, PresentationState, RowId, WorkspaceState,
        };
        use fuzzy_dedupe_app::ui::findings::{actions_button_id, sort_button_id};
        use fuzzy_dedupe_app::ui::widgets::dialog_host;
        use fuzzy_dedupe_app::ui::{shell, theme};
        use fuzzy_dedupe_engine::fixtures::{ScenarioKind, design};
        use fuzzy_dedupe_engine::service::{Engine, EngineConfig};
        use fuzzy_dedupe_platform::dirs::AppDirs;
        use fuzzy_dedupe_platform::open::{Location, OpenHandle, Opener};
        use fuzzy_dedupe_platform::picker::{FolderPicker, PickHandle, PickOutcome, PickRequest};

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

        struct Rig {
            state: PresentationState,
            services: Services,
            intents: IntentQueue,
            /// The harness runs a frame while it is built, before the theme
            /// is installed; the rig draws nothing until then.
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

        fn launch(kind: ScenarioKind, size: Vec2) -> Ui {
            let engine = Engine::new(EngineConfig::default()).expect("engine starts");
            let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("presentation_hub_views");
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
            };
            let mut harness = Harness::builder()
                .with_max_steps(super::SETTLE_STEPS)
                .with_size(size)
                .build_ui_state(|ui, rig: &mut Rig| rig.frame(ui), rig);
            theme::install(&harness.ctx, Vec::new());
            shell::install_icon_glyphs(&harness.ctx);
            harness.state_mut().ready = true;
            settle(&mut harness);
            harness
        }

        fn launch_design() -> Ui {
            launch(ScenarioKind::Design, Vec2::new(1600.0, 960.0))
        }

        /// Enough frames for input, dispatch, the frame that draws the
        /// result, and egui's scroll animation.
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

        /// Presses and releases `key` in exactly one frame.
        fn key_once(h: &mut Ui, modifiers: Modifiers, key: Key) {
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

        fn key(h: &mut Ui, modifiers: Modifiers, key: Key) {
            key_once(h, modifiers, key);
            settle(h);
        }

        fn press(h: &mut Ui, label: &str) {
            h.get_by_role_and_label(Role::Button, label).click();
            settle(h);
        }

        fn button_shown(h: &Ui, label: &str) -> bool {
            h.query_by_role_and_label(Role::Button, label).is_some()
        }

        /// Items only the named menu has, so a drawn menu is identified.
        const SORT_ONLY: [&str; 3] = ["Group size", "Total size", "Newest change"];
        const ACTIONS_ONLY: [&str; 3] = [
            "Expand all groups",
            "Collapse all groups",
            "Choose columns…",
        ];
        const ROW_ONLY: [&str; 3] = [
            "Keep — do not queue",
            "Make this the reference",
            "Copy path",
        ];

        fn shows_all(h: &Ui, items: &[&str]) -> bool {
            items.iter().all(|item| button_shown(h, item))
        }

        fn shows_none(h: &Ui, items: &[&str]) -> bool {
            items.iter().all(|item| !button_shown(h, item))
        }

        /// The first member row measured against a counterpart, with its
        /// file name: a reference's name is also its header's default
        /// title, so clicking it by name could hit the header instead.
        fn first_counterpart_row(h: &Ui) -> (RowId, String) {
            let results = state(h).scan.results.clone().expect("results");
            let queue = &state(h).queue;
            let projection = state(h).findings.projection.clone().expect("projection");
            projection
                .rows
                .iter()
                .find_map(|row| {
                    let RowId::Member { group, file } = row.id else {
                        return None;
                    };
                    let group = results.groups.iter().find(|g| g.id == group)?;
                    let member = group.member(file)?;
                    projections::member_view(group, queue, member)
                        .counterpart
                        .map(|_| (row.id, member.name.clone()))
                })
                .expect("a counterpart row on screen")
        }

        /// The topmost drawn label named `name`.
        fn topmost<'h>(h: &'h Ui, name: &'h str) -> egui_kittest::Node<'h> {
            h.query_all_by_label(name)
                .min_by(|a, b| a.rect().top().total_cmp(&b.rect().top()))
                .expect("the row is drawn")
        }

        /// The first member row of the projection, which a row menu names.
        fn first_member(h: &Ui) -> RowId {
            let projection = state(h).findings.projection.clone().expect("projection");
            projection
                .rows
                .iter()
                .map(|row| row.id)
                .find(|id| matches!(id, RowId::Member { .. }))
                .expect("a member row on screen")
        }

        // ── Menus through the shared overlay slot (PV-T33-003) ───────────

        /// Sort ▾ and Actions ▾ open and close through the overlay slot, and
        /// neither leaves a trace in `FindingsState`: the view holds no copy
        /// of which menu is open.
        #[test]
        fn presentation_hub_toolbar_menus_open_through_the_overlay_slot() {
            let mut h = launch_design();
            let before = state(&h).findings.clone();

            press(&mut h, "Sort: Match type");
            assert_eq!(state(&h).dialog.menu(), Some(MenuKind::Sort));
            assert!(shows_all(&h, &SORT_ONLY), "Sort ▾ is drawn");
            assert_eq!(
                state(&h).findings,
                before,
                "opening Sort ▾ changes no Findings state"
            );
            key(&mut h, Modifiers::NONE, Key::Escape);
            assert_eq!(state(&h).dialog.menu(), None);
            assert!(shows_none(&h, &SORT_ONLY));

            press(&mut h, "Actions ▾");
            assert_eq!(state(&h).dialog.menu(), Some(MenuKind::Actions));
            assert!(shows_all(&h, &ACTIONS_ONLY), "Actions ▾ is drawn");
            assert_eq!(
                state(&h).findings,
                before,
                "opening Actions ▾ changes no Findings state"
            );
        }

        /// The row menu opens through the slot from a right-click, and a
        /// press outside it closes the slot, after which the view no longer
        /// draws it.
        #[test]
        fn presentation_hub_the_row_menu_opens_through_the_slot_and_closes_outside() {
            let mut h = launch_design();
            let (row, name) = first_counterpart_row(&h);
            topmost(&h, &name).click_secondary();
            settle(&mut h);
            assert_eq!(state(&h).dialog.menu(), Some(MenuKind::Row(row)));
            assert!(shows_all(&h, &ROW_ONLY), "the row menu is drawn");

            // A press on the filter row's statement, outside the menu.
            let outside = h
                .get_by_label(fuzzy_dedupe_app::ui::findings::MEASURED_NOTE)
                .rect()
                .center();
            h.input_mut().events.push(Event::PointerMoved(outside));
            h.input_mut().events.push(Event::PointerButton {
                pos: outside,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            });
            h.step();
            assert_eq!(state(&h).dialog.menu(), None, "the press closed the slot");
            h.input_mut().events.push(Event::PointerButton {
                pos: outside,
                button: PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::NONE,
            });
            h.step();
            assert!(shows_none(&h, &ROW_ONLY), "closed on the next frame");
        }

        /// Esc, handled by the dialog host before the view draws, closes the
        /// menu, and the view draws it no longer from that very frame.
        #[test]
        fn presentation_hub_a_menu_the_host_closes_with_esc_is_seen_closed() {
            let mut h = launch_design();
            press(&mut h, "Sort: Match type");
            assert!(shows_all(&h, &SORT_ONLY));
            key_once(&mut h, Modifiers::NONE, Key::Escape);
            assert_eq!(state(&h).dialog.menu(), None);
            h.step();
            assert!(shows_none(&h, &SORT_ONLY), "closed on the next frame");
        }

        /// A menu the host opens without the view asking for it is drawn,
        /// and a menu the host swaps for another is drawn as the new one:
        /// the view reads the open menu only from the overlay slot.
        #[test]
        fn presentation_hub_the_view_draws_the_menu_the_slot_holds() {
            let mut h = launch_design();
            let before = state(&h).findings.clone();

            let row = first_member(&h);
            dialog_host::open_menu(
                &mut state_mut(&mut h).dialog,
                MenuKind::Row(row),
                Some(fuzzy_dedupe_app::ui::widgets::grouped_table::table_id()),
            )
            .expect("slot free");
            settle(&mut h);
            assert!(
                shows_all(&h, &ROW_ONLY),
                "a row menu the host opened is drawn"
            );
            dialog_host::close(&mut state_mut(&mut h).dialog);
            h.step();
            assert!(shows_none(&h, &ROW_ONLY), "a host-side close is seen");

            // The view asks for Sort ▾; before input is ever enabled again
            // the host closes it and opens Actions ▾ in its place.
            press(&mut h, "Sort: Match type");
            assert!(shows_all(&h, &SORT_ONLY));
            let dialog = &mut state_mut(&mut h).dialog;
            dialog_host::close(dialog);
            dialog_host::open_menu(dialog, MenuKind::Actions, Some(actions_button_id()))
                .expect("slot free");
            settle(&mut h);
            assert!(shows_all(&h, &ACTIONS_ONLY), "the slot's menu is drawn");
            assert!(shows_none(&h, &SORT_ONLY), "no stale Sort ▾");

            let dialog = &mut state_mut(&mut h).dialog;
            dialog_host::close(dialog);
            dialog_host::open_menu(dialog, MenuKind::Sort, Some(sort_button_id()))
                .expect("slot free");
            settle(&mut h);
            assert!(shows_all(&h, &SORT_ONLY), "the slot's menu is drawn");
            assert!(shows_none(&h, &ACTIONS_ONLY), "no stale Actions ▾");
            assert_eq!(
                state(&h).findings,
                before,
                "host-side menus change no Findings state"
            );
        }

        // ── Group titles (PV-T33-004) ────────────────────────────────────

        /// Every visible label's text.
        fn labels(h: &Ui) -> Vec<String> {
            h.query_all_by_role(Role::Label)
                .filter_map(|n| n.accesskit_node().value())
                .collect()
        }

        fn launch_collapsed(kind: ScenarioKind) -> Ui {
            let mut h = launch_design_sized(kind);
            state_mut(&mut h).findings.expanded.clear();
            settle(&mut h);
            h
        }

        fn launch_design_sized(kind: ScenarioKind) -> Ui {
            launch(kind, Vec2::new(1600.0, 960.0))
        }

        /// The labels drawn once `group`'s header row is scrolled into
        /// view, through the table's own reveal of the focused row.
        fn labels_with_header_of(h: &mut Ui, group: fuzzy_dedupe_domain::GroupId) -> Vec<String> {
            let findings = &mut state_mut(h).findings;
            findings.focused = Some(RowId::Group(group));
            findings.reveal_focus = true;
            settle(h);
            labels(h)
        }

        #[test]
        fn presentation_hub_design_headers_show_the_sample_titles() {
            let mut h = launch_collapsed(ScenarioKind::Design);
            let sample = design::sample();
            assert!(!sample.group_titles.is_empty(), "frame 13:648 has titles");
            assert_eq!(state(&h).findings.group_titles, sample.group_titles);
            let results = state(&h).scan.results.clone().expect("results");
            for (group, title) in &sample.group_titles {
                let group = results
                    .groups
                    .iter()
                    .find(|g| g.id == *group)
                    .expect("a titled group is in the results");
                let reference = projections::current_reference(group, &state(&h).queue)
                    .and_then(|r| group.member(r))
                    .map(|m| m.name.clone())
                    .unwrap_or_default();
                assert_ne!(*title, reference, "the title differs from the default");
                let shown = labels_with_header_of(&mut h, group.id);
                assert!(
                    shown.iter().any(|l| l == title),
                    "{} shows its title {title:?}",
                    group.label
                );
            }
        }

        #[test]
        fn presentation_hub_other_scenarios_keep_the_default_titles() {
            let mut h = launch_collapsed(ScenarioKind::Mixed);
            assert!(state(&h).findings.group_titles.is_empty());
            let results = state(&h).scan.results.clone().expect("results");
            for group in &results.groups {
                let reference = projections::current_reference(group, &state(&h).queue)
                    .and_then(|r| group.member(r))
                    .map(|m| m.name.clone())
                    .expect("a reference");
                let shown = labels_with_header_of(&mut h, group.id);
                assert!(
                    shown.contains(&reference),
                    "{} shows its reference {reference:?}",
                    group.label
                );
            }

            // Adopting another scenario over the design one drops its titles.
            let mut state = PresentationState::default();
            let engine = Engine::new(EngineConfig::default()).expect("engine starts");
            let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("presentation_hub_titles");
            let mut services = Services::new(
                engine,
                Box::new(NoOpener),
                Box::new(DismissPicker),
                AppDirs::resolve(|_| None, &root.join("home")),
            );
            adopt_scenario(&mut services, &mut state, ScenarioKind::Design);
            assert!(!state.findings.group_titles.is_empty());
            adopt_scenario(&mut services, &mut state, ScenarioKind::Mixed);
            assert!(state.findings.group_titles.is_empty());
        }

        // ── The recorded scroll offset (PV-T33-005) ──────────────────────

        /// Every visible label with its top edge: the drawn table, which a
        /// scroll offset moves.
        fn layout(h: &Ui) -> Vec<(String, f32)> {
            let mut out: Vec<(String, f32)> = h
                .query_all_by_role(Role::Label)
                .filter_map(|n| {
                    n.accesskit_node()
                        .value()
                        .map(|v| (v.clone(), n.rect().top()))
                })
                .collect();
            out.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
            out
        }

        /// The design scenario with every group expanded in a short window,
        /// so the table scrolls; the table holds keyboard focus.
        fn launch_scrollable() -> Ui {
            let mut h = launch(ScenarioKind::Design, Vec2::new(1600.0, 640.0));
            let groups = state(&h)
                .scan
                .results
                .as_ref()
                .expect("results")
                .groups
                .iter()
                .map(|g| g.id)
                .collect();
            state_mut(&mut h).findings.expanded = groups;
            settle(&mut h);
            let (_, name) = first_counterpart_row(&h);
            topmost(&h, &name).click();
            settle(&mut h);
            h
        }

        /// The recorded offset's bits: the round trip must give back the
        /// very value recorded, so it is compared exactly.
        fn offset_bits(h: &Ui) -> u32 {
            state(h).findings.scroll_offset.to_bits()
        }

        /// One frame away in Compare and back: the frame after the switch
        /// is the first restored frame.
        fn round_trip(h: &mut Ui, restore: Option<f32>) {
            state_mut(h).workspace = WorkspaceState::Compare;
            settle(h);
            if let Some(offset) = restore {
                state_mut(h).findings.scroll_offset = offset;
            }
            state_mut(h).workspace = WorkspaceState::Findings;
            h.step();
        }

        #[test]
        fn presentation_hub_scrolling_records_the_offset_and_a_round_trip_keeps_it() {
            let mut h = launch_scrollable();
            assert_eq!(offset_bits(&h), 0.0_f32.to_bits());
            key(&mut h, Modifiers::NONE, Key::End);
            let offset = state(&h).findings.scroll_offset;
            assert!(offset > 0.0, "End scrolled the table: {offset}");
            let drawn = layout(&h);

            round_trip(&mut h, None);
            assert_eq!(
                layout(&h),
                drawn,
                "the first restored frame is at the offset"
            );
            settle(&mut h);
            assert_eq!(offset_bits(&h), offset.to_bits(), "exact round trip");
            assert_eq!(layout(&h), drawn);
        }

        /// An offset written while the table is hidden, other than the one
        /// it last showed, is applied when the view is restored, and only
        /// once: scrolling afterwards is not undone. It shows from the frame
        /// after the first restored one, because `egui_table`'s scroll area
        /// is animated and places its content before it reaches a new
        /// offset (`grouped_table::ScrollProbe`); the unchanged round trip
        /// above is exact on the first frame.
        #[test]
        fn presentation_hub_a_restored_view_applies_its_recorded_offset_once() {
            let mut h = launch_scrollable();
            key(&mut h, Modifiers::NONE, Key::End);
            let recorded = state(&h).findings.scroll_offset;
            assert!(recorded > 0.0, "End scrolled the table: {recorded}");
            let drawn = layout(&h);
            key(&mut h, Modifiers::NONE, Key::Home);
            assert_eq!(offset_bits(&h), 0.0_f32.to_bits(), "Home scrolled back");
            assert_ne!(layout(&h), drawn);

            round_trip(&mut h, Some(recorded));
            h.step();
            assert_eq!(
                layout(&h),
                drawn,
                "the second restored frame shows the written offset"
            );
            settle(&mut h);
            assert_eq!(offset_bits(&h), recorded.to_bits(), "exact restore");
            assert_eq!(layout(&h), drawn);

            key(&mut h, Modifiers::NONE, Key::Home);
            settle(&mut h);
            assert_eq!(
                offset_bits(&h),
                0.0_f32.to_bits(),
                "the offset is not forced after the restore"
            );
        }

        // ── A completed scan after the design scenario (PV-T33-004) ──────

        struct FixedClock;

        impl fuzzy_dedupe_app::app::Clock for FixedClock {
            fn today(&self) -> fuzzy_dedupe_platform::log_retention::LogDate {
                fuzzy_dedupe_platform::log_retention::LogDate::new(2026, 11, 23)
                    .expect("valid date")
            }
        }

        struct AppSlot {
            app: Option<fuzzy_dedupe_app::app::FuzzyDedupeApp>,
        }

        type AppUi = Harness<'static, AppSlot>;

        fn app_state(h: &AppUi) -> &PresentationState {
            &h.state().app.as_ref().expect("app").state
        }

        fn app_state_mut(h: &mut AppUi) -> &mut PresentationState {
            &mut h.state_mut().app.as_mut().expect("app").state
        }

        fn app_steps(h: &mut AppUi, n: usize) {
            for _ in 0..n {
                h.step();
            }
        }

        /// The whole app, which adopts the design scenario at startup and
        /// drains engine events each frame (`run_logic`), so a scan started
        /// from New scan completes through the real event path.
        fn launch_app() -> AppUi {
            let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("presentation_hub_scan");
            let deps = fuzzy_dedupe_app::app::AppDeps {
                dirs: AppDirs::resolve(|_| None, &root.join("home")),
                opener: Box::new(NoOpener),
                picker: Box::new(DismissPicker),
                fonts: Box::new(fuzzy_dedupe_platform::fonts::SystemFontLookup),
                clock: Box::new(FixedClock),
                log_expiry: Box::new(|_| {}),
                engine: EngineConfig::default(),
                measure: Box::new(fuzzy_dedupe_app::measure::NoMeasurement),
            };
            let mut h = Harness::builder()
                .with_max_steps(super::SETTLE_STEPS)
                .with_size(Vec2::new(1600.0, 960.0))
                .build_ui_state(
                    |ui, slot: &mut AppSlot| {
                        if let Some(app) = slot.app.as_mut() {
                            app.run_logic(ui.ctx());
                            app.draw(ui);
                        }
                    },
                    AppSlot { app: None },
                );
            let app =
                fuzzy_dedupe_app::app::FuzzyDedupeApp::new(&h.ctx, deps).expect("engine starts");
            h.state_mut().app = Some(app);
            app_steps(&mut h, 24);
            h
        }

        /// A scan started from New scan after the design scenario was
        /// adopted: its group IDs match the design ones, and its headers
        /// must still show their default titles, because the titles are
        /// adoption-time fixture data (§11.1), not part of any scan.
        #[test]
        fn presentation_hub_a_completed_scan_shows_default_titles() {
            let mut h = launch_app();
            assert!(
                !app_state(&h).findings.group_titles.is_empty(),
                "the adopted design scenario shows its titles"
            );
            let previous = app_state(&h).scan.scan;
            // A folder that exists wherever the test runs; the simulated
            // scan reads no files (plan D-005).
            let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
            app_state_mut(&mut h).preferences.scan_setup = fuzzy_dedupe_domain::ScanSetup {
                folders: vec![fuzzy_dedupe_domain::ScanFolder {
                    path: folder,
                    protect: false,
                }],
                file_types: vec!["txt".to_owned(), "md".to_owned()],
            };

            for pressed in [true, false] {
                h.input_mut().events.push(Event::Key {
                    key: Key::N,
                    physical_key: None,
                    pressed,
                    repeat: false,
                    modifiers: Modifiers::CTRL,
                });
            }
            app_steps(&mut h, 24);
            h.get_by_role_and_label(Role::Button, "Start scan")
                .scroll_to_me();
            app_steps(&mut h, 24);
            h.get_by_role_and_label(Role::Button, "Start scan").click();
            let mut done = false;
            for _ in 0..500 {
                h.step();
                let s = app_state(&h);
                if s.scan.stage == fuzzy_dedupe_app::presentation::state::ScanStage::Completed
                    && s.scan.scan != previous
                {
                    done = true;
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            assert!(done, "the new scan completed");
            app_state_mut(&mut h).findings.expanded.clear();
            app_steps(&mut h, 24);

            let results = app_state(&h).scan.results.clone().expect("results");
            assert!(!results.groups.is_empty(), "the scan found groups");
            for group in &results.groups {
                let reference = projections::current_reference(group, &app_state(&h).queue)
                    .and_then(|r| group.member(r))
                    .map(|m| m.name.clone())
                    .expect("a reference");
                let findings = &mut app_state_mut(&mut h).findings;
                findings.focused = Some(RowId::Group(group.id));
                findings.reveal_focus = true;
                app_steps(&mut h, 24);
                let shown: Vec<String> = h
                    .query_all_by_role(Role::Label)
                    .filter_map(|n| n.accesskit_node().value())
                    .collect();
                assert_eq!(
                    projections::group_title(group, &app_state(&h).findings, &app_state(&h).queue),
                    reference,
                    "{} has its default title",
                    group.label
                );
                assert!(
                    shown.contains(&reference),
                    "{} shows its default title {reference:?}",
                    group.label
                );
            }
        }
    }
}
