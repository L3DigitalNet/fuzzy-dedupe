// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! PV-T17-001 and PV-T17-002 (plan T17): the Review unique text and Queue
//! identical copies popups, driven headless through `egui_kittest`.
//!
//! Every test sits in the `review_popups` module so `-E 'test(review_popups)'`
//! selects them. Most run the whole app on the design scenario, as
//! `tests/shell.rs` does: `run_logic` then `draw` per frame, with test
//! doubles for every platform adapter. The bulk-queue exclusion test needs
//! scenarios the app does not start on (an exact copy that is only a hash
//! candidate), so it drives the real engine facade on generated scenarios
//! through T12's seams instead: `Services`, `dialog_host` and `dispatch`,
//! in `app.rs`'s frame order.
//!
//! The Findings checkbox, Space and row menu (T13) and Compare's Queue B
//! (T15) are not drawn on this base, so each entry point is exercised as
//! the `UiIntent::Review` request that control sends, with its own
//! `QueueOrigin`. What T17 owns is everything after that request.
//!
//! Every expected value comes from an independent source: unique lines from
//! the C-04 result and from `measure_unique` over the compared texts, and
//! bulk counts and sizes from the queue the review service actually built.

mod review_popups {
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};

    use egui::accesskit::Role;
    use egui::{Event, Key, Modifiers, OutputCommand, Vec2};
    use egui_kittest::Harness;
    use egui_kittest::kittest::{NodeT as _, Queryable};
    use fuzzy_dedupe_app::app::{AppDeps, Clock, FuzzyDedupeApp};
    use fuzzy_dedupe_app::measure::NoMeasurement;
    use fuzzy_dedupe_app::presentation::commands::{self, IntentQueue, Services, UiIntent};
    use fuzzy_dedupe_app::presentation::events;
    use fuzzy_dedupe_app::presentation::state::{
        DialogKind, DialogSubject, PresentationState, QueueEntry, QueueState, ScanStage,
        UniqueTextSubject, WorkspaceState,
    };
    use fuzzy_dedupe_app::ui::widgets::dialog_host;
    use fuzzy_dedupe_app::ui::{glyphs, theme};
    use fuzzy_dedupe_domain::{
        CompareMode, EvidenceUpdate, FileId, Group, IgnoreOptions, Measurement, MemberRole,
        QueueOrigin, QueueScope, RequestId, ScanResults, SnapshotId, Verification,
    };
    use fuzzy_dedupe_engine::compare::{AlignedComparison, measure_unique};
    use fuzzy_dedupe_engine::fixtures::{self, ScenarioKind};
    use fuzzy_dedupe_engine::service::{Engine, EngineConfig, ReviewCommand};
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

    struct FixedClock;

    impl Clock for FixedClock {
        fn today(&self) -> LogDate {
            LogDate::new(2026, 11, 23).expect("valid date")
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
            .join("review_popups")
            .join(format!("{name}-{nanos}"));
        let state = root.join("state");
        AppDirs::resolve(
            move |var| (var == "XDG_STATE_HOME").then(|| state.clone().into_os_string()),
            &root.join("home"),
        )
    }

    // ── The whole app on the design scenario ─────────────────────────────

    type Shell = Harness<'static, Option<FuzzyDedupeApp>>;

    fn launch(name: &str) -> Shell {
        let deps = AppDeps {
            dirs: dirs_for(name),
            opener: Box::new(NoOpener),
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
                |ui, app: &mut Option<FuzzyDedupeApp>| {
                    if let Some(app) = app.as_mut() {
                        app.run_logic(ui.ctx());
                        app.draw(ui);
                    }
                },
                None,
            );
        let app = FuzzyDedupeApp::new(&harness.ctx, deps).expect("engine starts");
        *harness.state_mut() = Some(app);
        harness.run();
        harness
    }

    fn state(h: &Shell) -> &PresentationState {
        &h.state().as_ref().expect("app").state
    }

    fn push(h: &mut Shell, intent: UiIntent) {
        h.state_mut().as_mut().expect("app").push_intent(intent);
    }

    fn press(h: &mut Shell, key: Key) {
        for pressed in [true, false] {
            h.input_mut().events.push(Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: Modifiers::NONE,
            });
        }
        h.step();
    }

    /// Runs frames until `done` holds, for engine work on worker threads.
    fn run_until<S>(h: &mut Harness<'static, S>, what: &str, done: impl Fn(&S) -> bool) {
        for _ in 0..500 {
            h.step();
            if done(h.state()) {
                h.run();
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("timed out waiting for {what}");
    }

    fn open_kind(h: &Shell) -> Option<DialogKind> {
        state(h).dialog.dialog().map(|d| d.kind)
    }

    /// A node's accessible text: a button's label, or a text label's value
    /// (egui exposes label text as the value, which kittest also matches).
    fn label_of(node: &egui_kittest::Node<'_>) -> String {
        let node = node.accesskit_node();
        node.label()
            .filter(|label| !label.is_empty())
            .or_else(|| node.value())
            .unwrap_or_default()
    }

    /// The design scenario's fuzzy row of frame `16:932`: it needs review
    /// against its counterpart, the reference.
    const FUZZY_ROW: &str = "Design_Spec_draft.txt";

    fn fuzzy_pair(h: &Shell) -> (FileId, FileId) {
        let results = state(h).scan.results.clone().expect("results");
        let member = results
            .groups
            .iter()
            .flat_map(|g| g.members.iter())
            .find(|m| m.name == FUZZY_ROW)
            .expect("the fuzzy row in the design scenario");
        let MemberRole::Member(link) = member.role else {
            panic!("{FUZZY_ROW} is a reference");
        };
        (member.file, link.counterpart)
    }

    fn request_queue(
        h: &mut Shell,
        origin: QueueOrigin,
        reviewed: Option<RequestId>,
    ) -> (FileId, FileId) {
        let (file, counterpart) = fuzzy_pair(h);
        push(
            h,
            UiIntent::Review {
                command: ReviewCommand::RequestQueue {
                    file,
                    counterpart,
                    origin,
                    reviewed,
                },
                invoker: None,
            },
        );
        h.run();
        (file, counterpart)
    }

    fn subject(h: &Shell) -> UniqueTextSubject {
        let dialog = state(h).dialog.dialog().expect("a popup is open");
        assert_eq!(dialog.kind, DialogKind::UniqueTextReview);
        match dialog.subject {
            DialogSubject::UniqueText(subject) => subject,
            ref other => panic!("Review unique text about {other:?}"),
        }
    }

    /// Opens Review unique text from a Space request on the fuzzy row and
    /// waits until its comparison is ready.
    fn open_review(name: &str) -> (Shell, UniqueTextSubject) {
        let mut h = launch(name);
        request_queue(&mut h, QueueOrigin::Shortcut, None);
        let subject = subject(&h);
        let key = subject.comparison(CompareMode::Original);
        run_until(&mut h, "the popup's comparison", move |app| {
            let slot = &app.as_ref().expect("app").state.compare.comparison;
            slot.matches(&key) && slot.aligned().is_some()
        });
        (h, subject)
    }

    fn aligned(h: &Shell) -> AlignedComparison {
        state(h)
            .compare
            .comparison
            .aligned()
            .expect("comparison ready")
            .clone()
    }

    fn checkbox(h: &Shell) -> egui_kittest::Node<'_> {
        h.get_by_role_and_label(Role::CheckBox, "I reviewed the unique text in this file.")
    }

    const PROMPT: &str =
        "Check this box to confirm you reviewed the unique text. Nothing was queued.";

    // ── PV-T17-001: Review unique text ───────────────────────────────────

    #[test]
    fn review_popups_every_queue_entry_point_on_a_fuzzy_row_opens_review_unique_text() {
        for origin in [
            QueueOrigin::Checkbox,
            QueueOrigin::Shortcut,
            QueueOrigin::RowMenu,
        ] {
            let mut h = launch("entry");
            let (file, counterpart) = request_queue(&mut h, origin, None);
            let subject = subject(&h);
            assert_eq!(
                (subject.target, subject.counterpart, subject.origin),
                (file, counterpart, origin)
            );
            assert!(!state(&h).queue.entries.contains_key(&file), "{origin:?}");
            // The popup draws its pair, not an empty body.
            h.get_by_label(FUZZY_ROW);
            h.get_by_role_and_label(Role::Button, "Queue for cleanup");
        }

        // Queue B in Compare sends the reviewed comparison with its request.
        let mut h = launch("entry-compare");
        let (file, counterpart) = fuzzy_pair(&h);
        push(
            &mut h,
            UiIntent::OpenCompare {
                target: file,
                counterpart,
                return_row: None,
            },
        );
        run_until(&mut h, "Compare's comparison", |app| {
            let s = &app.as_ref().expect("app").state;
            s.compare.comparison.aligned().is_some()
        });
        let reviewed = state(&h).compare.comparison.request;
        assert!(reviewed.is_some());
        request_queue(&mut h, QueueOrigin::Compare, reviewed);
        let subject = subject(&h);
        assert_eq!(
            (subject.target, subject.counterpart, subject.reviewed),
            (file, counterpart, reviewed)
        );
        assert!(!state(&h).queue.entries.contains_key(&file));
        h.get_by_label(FUZZY_ROW);
    }

    #[test]
    fn review_popups_queue_without_acknowledgment_queues_nothing_and_focuses_the_checkbox() {
        let (mut h, subject) = open_review("no-ack");
        let before = state(&h).queue.clone();
        assert!(h.query_by_label(PROMPT).is_none(), "no prompt before Queue");

        h.get_by_role_and_label(Role::Button, "Queue for cleanup")
            .click();
        h.run();
        assert_eq!(open_kind(&h), Some(DialogKind::UniqueTextReview));
        assert_eq!(state(&h).queue, before, "nothing was queued");
        assert!(!state(&h).queue.entries.contains_key(&subject.target));
        h.get_by_label(PROMPT);
        assert!(checkbox(&h).is_focused(), "focus moved to the checkbox");

        // Enter runs the same action: still nothing queued, focus stays.
        h.get_by_role_and_label(Role::Button, "Copy unique lines")
            .focus();
        h.run();
        press(&mut h, Key::Enter);
        h.run();
        assert_eq!(open_kind(&h), Some(DialogKind::UniqueTextReview));
        assert_eq!(state(&h).queue, before);
        assert!(checkbox(&h).is_focused());
    }

    #[test]
    fn review_popups_queue_with_acknowledgment_queues_that_target_against_that_counterpart() {
        let (mut h, subject) = open_review("ack");
        let before = state(&h).queue.entries.clone();

        checkbox(&h).click();
        h.run();
        assert_eq!(checkbox(&h).accesskit_node().toggled(), Some(true.into()));
        h.get_by_role_and_label(Role::Button, "Queue for cleanup")
            .click();
        h.run();

        assert_eq!(open_kind(&h), None, "the popup closed");
        let after = &state(&h).queue.entries;
        let added: Vec<(&FileId, &QueueEntry)> = after
            .iter()
            .filter(|(file, _)| !before.contains_key(file))
            .collect();
        assert_eq!(added.len(), 1, "exactly one file entered the queue");
        let (target, entry) = added[0];
        assert_eq!(*target, subject.target);
        assert_eq!(entry.counterpart, subject.counterpart);
        let binding = entry.acknowledgment.expect("bound approval");
        assert_eq!(
            (binding.target_snapshot, binding.counterpart_snapshot),
            (subject.target_snapshot, subject.counterpart_snapshot),
            "bound to the displayed snapshots"
        );
        assert!(
            before
                .iter()
                .all(|(file, entry)| after.get(file) == Some(entry))
        );
    }

    #[test]
    fn review_popups_a_changed_snapshot_blocks_the_acknowledged_queue() {
        let (mut h, subject) = open_review("stale");
        // The target changes on disk after the popup showed its lines: the
        // acknowledgment would bind to text the user never saw.
        push(
            &mut h,
            UiIntent::Review {
                command: ReviewCommand::UpdateEvidence(EvidenceUpdate::FileSnapshot {
                    file: subject.target,
                    snapshot: SnapshotId::new(subject.target_snapshot.get() + 1_000),
                }),
                invoker: None,
            },
        );
        h.run();
        let before = state(&h).queue.clone();
        checkbox(&h).click();
        h.run();
        h.get_by_role_and_label(Role::Button, "Queue for cleanup")
            .click();
        h.run();
        assert_eq!(open_kind(&h), Some(DialogKind::UniqueTextReview));
        assert_eq!(
            state(&h).queue.entries,
            before.entries,
            "nothing was queued"
        );
        h.get_by_label_contains("changed since this review opened");
    }

    #[test]
    fn review_popups_unique_lines_and_their_count_come_from_the_c04_measure() {
        let (h, subject) = open_review("lines");
        let aligned = aligned(&h);
        assert_eq!(aligned.b.file, subject.target, "side B is the target");

        // The same §10.1 measure, recomputed from the compared texts.
        let (only_b, _) = measure_unique(
            &aligned.b.original.text,
            &aligned.a.original.text,
            CompareMode::Original,
            IgnoreOptions {
                whitespace: false,
                line_endings: false,
                letter_case: false,
                punctuation: false,
            },
        );
        assert_eq!(only_b.lines, aligned.only_in_b.lines);
        let count = aligned.only_in_b.count();
        assert!(count > 0, "the fuzzy row has unique lines");

        // The Findings Unique value of the row agrees (one measure, C-04).
        let results = state(&h).scan.results.clone().expect("results");
        let member = results
            .groups
            .iter()
            .find_map(|g| g.member(subject.target))
            .expect("target");
        assert_eq!(member.link().expect("member").unique_lines, count);

        let headline = if count == 1 {
            "1 line exists only in this file.".to_owned()
        } else {
            format!("{count} lines exist only in this file.")
        };
        h.get_by_label(&headline);
        for &line in &aligned.only_in_b.lines {
            let text = aligned.b.original.line(line).expect("line");
            h.get_by_label(&format!("Line {}: {text}", line + 1));
        }
    }

    #[test]
    fn review_popups_copy_unique_lines_places_exactly_the_c04_copy_text() {
        let (mut h, _) = open_review("copy");
        let expected = aligned(&h).copy_only_in_b().text;
        assert!(expected.ends_with('\n'));
        h.get_by_role_and_label(Role::Button, "Copy unique lines")
            .click();
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
        assert_eq!(copied, vec![&expected]);
        h.run();
        assert_eq!(open_kind(&h), Some(DialogKind::UniqueTextReview));
    }

    #[test]
    fn review_popups_open_comparison_closes_the_popup_and_shows_the_pair() {
        let (mut h, subject) = open_review("open-compare");
        h.get_by_role_and_label(Role::Button, "Open comparison")
            .click();
        h.run();
        assert_eq!(open_kind(&h), None);
        let s = state(&h);
        assert_eq!(s.workspace, WorkspaceState::Compare);
        let pair = s.compare.pair.expect("pair");
        assert_eq!(
            (pair.target, pair.counterpart),
            (subject.target, subject.counterpart)
        );
        assert!(!s.queue.entries.contains_key(&subject.target));
    }

    // ── PV-T17-002: Queue identical copies ───────────────────────────────

    /// The §11.4 Size format: binary units, one decimal, rounded to nearest.
    /// Written independently of the popup's own formatter.
    fn binary_size(bytes: u64) -> String {
        if bytes < 1024 {
            return format!("{bytes} B");
        }
        let mut unit = 1024_u64;
        for name in ["KiB", "MiB", "GiB"] {
            let tenths = (u128::from(bytes) * 10 + u128::from(unit) / 2) / u128::from(unit);
            if tenths < 10_240 || name == "GiB" {
                return format!("{}.{} {name}", tenths / 10, tenths % 10);
            }
            unit *= 1024;
        }
        unreachable!()
    }

    fn files(n: usize) -> String {
        if n == 1 {
            "1 file".to_owned()
        } else {
            format!("{n} files")
        }
    }

    fn groups(n: usize) -> String {
        if n == 1 {
            "1 group".to_owned()
        } else {
            format!("{n} groups")
        }
    }

    /// What the popup must have said, given what the service queued.
    fn expected_summary(results: &ScanResults, added: &BTreeSet<FileId>) -> String {
        let mut bytes = 0;
        let mut in_groups = BTreeSet::new();
        for group in &results.groups {
            for m in &group.members {
                if added.contains(&m.file) {
                    bytes += m.size_bytes;
                    in_groups.insert(group.id);
                }
            }
        }
        format!(
            "{} in {} will be queued · {}",
            files(added.len()),
            groups(in_groups.len()),
            binary_size(bytes)
        )
    }

    fn group_of(results: &ScanResults, file: FileId) -> &Group {
        results
            .groups
            .iter()
            .find(|g| g.member(file).is_some())
            .expect("file in a group")
    }

    /// Whether `file` is its group's reference now, by the role the review
    /// service last reported for it, else the scan's (the design scenario
    /// replays Make this the reference, so the scan's own reference is not
    /// enough).
    fn is_reference(results: &ScanResults, queue: &QueueState, file: FileId) -> bool {
        group_of(results, file)
            .member(file)
            .is_some_and(|m| matches!(queue.role(m), MemberRole::Reference))
    }

    /// Asserts the §11.4 bulk rules on what the service queued: never the
    /// reference, a protected or a kept file, only a byte-verified exact
    /// copy queued against its reference.
    fn assert_bulk_rules(
        results: &ScanResults,
        added: &BTreeSet<FileId>,
        queue: &QueueState,
        protected: &BTreeSet<FileId>,
        kept: &BTreeSet<FileId>,
    ) {
        for &file in added {
            assert!(
                !is_reference(results, queue, file),
                "a reference was queued"
            );
            assert!(!protected.contains(&file), "a protected file was queued");
            assert!(!kept.contains(&file), "a kept file was queued");
            let counterpart = queue.entries[&file].counterpart;
            assert!(
                is_reference(results, queue, counterpart),
                "queued against its reference"
            );
            let edge = group_of(results, file)
                .edge_between(file, counterpart)
                .expect("a direct edge to the reference");
            assert_eq!(
                edge.measurement,
                Measurement::Identical(Verification::ByteVerified),
                "only byte-verified exact copies"
            );
        }
    }

    fn summary_label(h: &Harness<'static, impl Sized>) -> String {
        label_of(&h.get_by_label_contains("will be queued"))
    }

    /// Exact copies of their reference the bulk action would take, one per
    /// group, by the scan's own evidence.
    fn exact_copies(h: &Shell) -> Vec<FileId> {
        let results = state(h).scan.results.clone().expect("results");
        let queued = &state(h).queue.entries;
        results
            .groups
            .iter()
            .filter_map(|g| {
                let reference = g.reference()?.file;
                g.members.iter().find_map(|m| {
                    let exact = g.edge_between(m.file, reference).is_some_and(|e| {
                        e.measurement == Measurement::Identical(Verification::ByteVerified)
                    });
                    (exact && m.file != reference && !queued.contains_key(&m.file))
                        .then_some(m.file)
                })
            })
            .collect()
    }

    fn review(h: &mut Shell, command: ReviewCommand) {
        push(
            h,
            UiIntent::Review {
                command,
                invoker: None,
            },
        );
    }

    /// Opens Queue identical copies over the design scenario after marking
    /// one exact copy Kept and protecting another, picks the scope, presses
    /// Queue N files, and checks what the popup stated against what the
    /// review service queued.
    fn bulk_run(filtered: bool) {
        let mut h = launch("bulk");
        let results = state(&h).scan.results.clone().expect("results");
        let candidates = exact_copies(&h);
        assert!(candidates.len() >= 4, "design has exact copies to queue");
        let (kept, protect) = (candidates[0], candidates[1]);
        review(
            &mut h,
            ReviewCommand::MarkKept {
                file: kept,
                kept: true,
            },
        );
        review(
            &mut h,
            ReviewCommand::ProtectFile {
                file: protect,
                protected: true,
            },
        );
        h.run();

        // "Shown by the current filter": every other group, plus the kept
        // and protected files' groups, so both exclusions are in both scopes.
        let shown: Vec<_> = results
            .groups
            .iter()
            .enumerate()
            .filter(|(i, g)| i % 2 == 0 || g.member(kept).is_some() || g.member(protect).is_some())
            .map(|(_, g)| g.id)
            .collect();
        push(
            &mut h,
            UiIntent::OpenDialog {
                kind: DialogKind::QueueIdentical,
                subject: DialogSubject::QueueIdentical(QueueScope::Groups(shown.clone())),
                invoker: None,
            },
        );
        h.run();
        assert_eq!(open_kind(&h), Some(DialogKind::QueueIdentical));
        if filtered {
            h.get_by_label_contains("Only groups shown by the current filter")
                .click();
            h.run();
        }
        let summary = summary_label(&h);
        let button = h
            .get_all_by_role(Role::Button)
            .map(|b| label_of(&b))
            .find(|l| l.starts_with("Queue "))
            .expect("the Queue N files button");

        let before = state(&h).queue.clone();
        let protected = before.view.as_ref().expect("view").protected.clone();
        assert!(protected.contains(&protect));
        h.get_by_role_and_label(Role::Button, &button).click();
        h.run();
        assert_eq!(open_kind(&h), None, "the popup closed");

        let after = state(&h).queue.clone();
        let added: BTreeSet<FileId> = after
            .entries
            .keys()
            .filter(|f| !before.entries.contains_key(f))
            .copied()
            .collect();
        assert!(!added.is_empty(), "the service queued exact copies");
        assert_eq!(summary, expected_summary(&results, &added));
        assert_eq!(button, format!("Queue {}", files(added.len())));
        if filtered {
            let shown: BTreeSet<_> = shown.iter().copied().collect();
            assert!(
                added
                    .iter()
                    .all(|f| shown.contains(&group_of(&results, *f).id)),
                "only the filter's groups"
            );
        }
        assert_bulk_rules(
            &results,
            &added,
            &after,
            &protected,
            &BTreeSet::from([kept]),
        );
        assert!(!added.contains(&kept) && !added.contains(&protect));
    }

    #[test]
    fn review_popups_queue_identical_reports_what_the_service_queues_for_all_groups() {
        bulk_run(false);
    }

    #[test]
    fn review_popups_queue_identical_reports_what_the_service_queues_for_the_filter() {
        bulk_run(true);
    }

    // ── PV-T17-002 on generated scenarios, through T12's seams ───────────

    struct Mini {
        state: PresentationState,
        services: Services,
        intents: IntentQueue,
    }

    /// `app.rs`'s frame order for a popup alone: focus restore, the dialog
    /// host, then the frame's intents and their bounded follow-ups.
    fn mini_frame(ctx: &egui::Context, m: &mut Mini) {
        m.services.poll(&mut m.state);
        dialog_host::begin_frame(ctx, &mut m.state.dialog);
        dialog_host::show(ctx, &mut m.state, &mut m.intents);
        let mut intents = m.intents.drain();
        for _ in 0..4 {
            let mut followups = IntentQueue::default();
            for intent in intents {
                commands::dispatch(intent, &mut m.state, &mut m.services, ctx, &mut followups);
            }
            intents = followups.drain();
        }
    }

    fn mini(name: &str, kind: ScenarioKind, seed: u64) -> Harness<'static, Mini> {
        let mut engine = Engine::new(EngineConfig::default()).expect("engine starts");
        let scan = engine.reserve_scan_id();
        let scenario = Arc::new(fixtures::build(kind, scan, seed).expect("scenario"));
        engine
            .adopt_scenario(Arc::clone(&scenario))
            .expect("adopted");
        let mut state = PresentationState::default();
        events::install_results(
            &mut state,
            Arc::clone(&scenario.results),
            ScanStage::Completed,
        );
        let mut services = Services::new(
            engine,
            Box::new(NoOpener),
            Box::new(DismissPicker),
            dirs_for(name),
        );
        services.sync_view(&mut state);
        let harness = Harness::builder()
            .with_max_steps(SETTLE_STEPS)
            .with_size(Vec2::new(1600.0, 960.0))
            .build_ui_state(
                |ui, m: &mut Mini| mini_frame(&ui.ctx().clone(), m),
                Mini {
                    state,
                    services,
                    intents: IntentQueue::default(),
                },
            );
        theme::install(&harness.ctx, Vec::new());
        // Built without `FuzzyDedupeApp::new`, so the registry is installed
        // here, before the first pass, as the app installs it.
        glyphs::install(&harness.ctx);
        harness
    }

    /// Exact copies of their reference, split by verification.
    fn reference_copies(results: &ScanResults) -> (Vec<FileId>, Vec<FileId>) {
        let (mut verified, mut candidates) = (Vec::new(), Vec::new());
        for g in &results.groups {
            let Some(reference) = g.reference() else {
                continue;
            };
            for m in &g.members {
                match g
                    .edge_between(m.file, reference.file)
                    .map(|e| e.measurement)
                {
                    Some(Measurement::Identical(Verification::ByteVerified)) => {
                        verified.push(m.file);
                    }
                    Some(Measurement::Identical(Verification::HashCandidate)) => {
                        candidates.push(m.file);
                    }
                    _ => {}
                }
            }
        }
        (verified, candidates)
    }

    #[test]
    fn review_popups_queue_identical_never_takes_a_hash_candidate_or_a_non_reference_copy() {
        // The generator makes one exact copy in four a hash candidate only;
        // take the first seed whose scenario has both kinds of reference
        // copy, so the case is never vacuous. Deterministic per seed.
        let (kind, seed) = (0..64)
            .flat_map(|seed| {
                [
                    ScenarioKind::Mixed,
                    ScenarioKind::Stale,
                    ScenarioKind::Unicode,
                ]
                .map(|kind| (kind, seed))
            })
            .find(|&(kind, seed)| {
                let s = fixtures::build(kind, fuzzy_dedupe_domain::ScanId::new(1), seed)
                    .expect("scenario");
                let (verified, candidates) = reference_copies(&s.results);
                !verified.is_empty() && !candidates.is_empty()
            })
            .expect("a seed with both verified and hash-candidate copies");

        let mut h = mini("generated", kind, seed);
        h.run();
        let results = h.state().state.scan.results.clone().expect("results");
        let (_, candidates) = reference_copies(&results);
        let mut intents = IntentQueue::default();
        dialog_host::open(
            &mut h.state_mut().state,
            DialogKind::QueueIdentical,
            DialogSubject::QueueIdentical(QueueScope::AllByteIdenticalGroups),
            None,
            &mut intents,
        )
        .expect("opens");
        h.run();
        let summary = summary_label(&h);
        let before = h.state().state.queue.clone();
        press_enter(&mut h);
        h.run();
        assert!(!h.state().state.dialog.is_open(), "Enter queued and closed");

        let added: BTreeSet<FileId> = h
            .state()
            .state
            .queue
            .entries
            .keys()
            .filter(|f| !before.entries.contains_key(f))
            .copied()
            .collect();
        assert!(!added.is_empty());
        assert_eq!(summary, expected_summary(&results, &added));
        assert!(candidates.iter().all(|f| !added.contains(f)));
        let protected = before.view.as_ref().expect("view").protected.clone();
        let after = h.state().state.queue.clone();
        assert_bulk_rules(&results, &added, &after, &protected, &BTreeSet::new());
    }

    fn press_enter(h: &mut Harness<'static, Mini>) {
        for pressed in [true, false] {
            h.input_mut().events.push(Event::Key {
                key: Key::Enter,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: Modifiers::NONE,
            });
        }
        h.step();
    }

    /// The comparison request built through T12's helper is the one the
    /// popup's subject asks for: both come from the current snapshots.
    #[test]
    fn review_popups_the_subject_comparison_matches_the_request_helper() {
        let mut h = launch("helper");
        request_queue(&mut h, QueueOrigin::RowMenu, None);
        let subject = subject(&h);
        let built = commands::comparison_request(
            state(&h),
            subject.target,
            subject.counterpart,
            CompareMode::Original,
        )
        .expect("request");
        assert_eq!(built, subject.comparison(CompareMode::Original));
        assert!(
            state(&h).compare.comparison.matches(&built),
            "the popup asked for its pair's comparison"
        );
    }
}
