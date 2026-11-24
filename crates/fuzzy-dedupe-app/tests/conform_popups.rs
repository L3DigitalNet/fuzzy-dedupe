// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! PV-T40-001, leg P2 (plan T40, items P6 to P9): the small popups conform
//! to their review frames and to the owner's scan-setup decision, measured
//! on the running application in a headless `egui_kittest` harness.
//!
//! Every test sits in the `conform_popups` module and is named `conform_…`,
//! so `-E 'test(conform_)'` selects them with the other T40 legs' suites.
//!
//! The rig is `tests/layout.rs`'s, copied because test files share no
//! module: the whole app through `FuzzyDedupeApp::new` on the design
//! scenario, drawn into the full content rect rather than kittest's 8-unit
//! outer margin, each frame running `run_logic` then `draw` as eframe does.
//! Zoom and theme go through the preferences, which the app applies to the
//! context itself (`app::apply_preferences`), so 200% on a 1600 × 960
//! window gives the UI 800 × 480 points and on 1024 × 600 gives 512 × 300.
//!
//! Geometry comes from the accessibility tree (widget bounds, in points)
//! and from the frame's painted shapes, never from a rendered image, so no
//! GPU is needed. Key caps and checkbox squares are painted, not widgets;
//! they are found as painted rectangles inside the widget that owns them.
//! Expected values are the spec's and the frames' numbers, written here as
//! literals with their source, never the app's constants. Tolerance is
//! ±1 logical unit (A-010).
//!
//! Requirements: fresh preference and log directories under
//! `CARGO_TARGET_TMPDIR`, unique per run, which the app itself may write
//! into; New scan's folder check reads only `CARGO_TARGET_TMPDIR`'s own
//! metadata, which cargo guarantees exists.

mod conform_popups {
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use egui::accesskit::Role;
    use egui::{Color32, Key, Modifiers, Rect, Vec2};
    use egui_kittest::Harness;
    use egui_kittest::kittest::{NodeT, Queryable};
    use fuzzy_dedupe_app::app::{AppDeps, Clock, FuzzyDedupeApp};
    use fuzzy_dedupe_app::measure::NoMeasurement;
    use fuzzy_dedupe_app::presentation::commands::UiIntent;
    use fuzzy_dedupe_app::presentation::state::{
        DialogKind, DialogSubject, PresentationState, ThemeOverride,
    };
    use fuzzy_dedupe_app::ui::dialogs::unique_text_review;
    use fuzzy_dedupe_app::ui::glyphs;
    use fuzzy_dedupe_app::ui::theme::{self, Mode, Token, Zoom};
    use fuzzy_dedupe_domain::{MemberRole, QueueOrigin, ScanFolder, ScanSetup};
    use fuzzy_dedupe_engine::service::{EngineConfig, ReviewCommand};
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

    /// A-010: numeric layout assertions allow ±1 logical unit.
    const TOLERANCE: f32 = 1.0;

    /// The largest window the spec draws and the smallest it supports
    /// (spec §11.3).
    const SIZES: [Vec2; 2] = [Vec2::new(1600.0, 960.0), Vec2::new(1024.0, 600.0)];
    /// The default and the largest in-app zoom (spec §11.10).
    const ZOOMS: [u16; 2] = [100, 200];
    const THEMES: [ThemeOverride; 2] = [ThemeOverride::Light, ThemeOverride::Dark];

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
            LogDate::new(2026, 11, 24).expect("valid date")
        }
    }

    // ── Harness ──────────────────────────────────────────────────────────

    struct Slot {
        app: Option<FuzzyDedupeApp>,
    }

    type Shell = Harness<'static, Slot>;

    /// A fresh home under the test's own target directory, unique per run
    /// because tests may not delete what an earlier run left behind.
    fn dirs_for(name: &str) -> AppDirs {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after 1970")
            .as_nanos();
        let root: PathBuf = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("conform_popups")
            .join(format!("{name}-{nanos}"));
        let state = root.join("state");
        AppDirs::resolve(
            move |var| (var == "XDG_STATE_HOME").then(|| state.clone().into_os_string()),
            &root.join("home"),
        )
    }

    fn launch(name: &str, size: Vec2) -> Shell {
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
            .with_size(size)
            .build_ui_state(
                |ui, slot: &mut Slot| {
                    if let Some(app) = slot.app.as_mut() {
                        app.run_logic(ui.ctx());
                        // kittest's `build_ui_state` hands over a Ui inside an
                        // 8-unit outer margin; eframe gives the app the whole
                        // client area, so draw into a child spanning the
                        // content rect, as `tests/layout.rs` does.
                        let full = ui.ctx().content_rect();
                        let mut window = ui.new_child(egui::UiBuilder::new().max_rect(full));
                        app.draw(&mut window);
                    }
                },
                Slot { app: None },
            );
        let app = FuzzyDedupeApp::new(&harness.ctx, deps).expect("engine starts");
        glyphs::install(&harness.ctx);
        harness.state_mut().app = Some(app);
        harness.run();
        run_until(&mut harness, "the design scenario's results", |s| {
            s.scan.results.is_some()
        });
        harness.run();
        harness
    }

    fn state(h: &Shell) -> &PresentationState {
        &h.state().app.as_ref().expect("app").state
    }

    fn state_mut(h: &mut Shell) -> &mut PresentationState {
        &mut h.state_mut().app.as_mut().expect("app").state
    }

    fn push(h: &mut Shell, intent: UiIntent) {
        h.state_mut().app.as_mut().expect("app").push_intent(intent);
        h.run();
    }

    /// Runs frames until `done` holds, for engine work on worker threads.
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

    /// Applies a zoom and theme through the preferences, as the View menu
    /// and the zoom chords do, and lets the app re-lay out under them.
    fn view(h: &mut Shell, zoom: u16, theme: ThemeOverride) {
        let prefs = &mut state_mut(h).preferences;
        prefs.zoom = Zoom::from_percent(zoom).expect("on the zoom grid");
        prefs.theme = theme;
        h.run();
        h.run();
    }

    fn open_popup(h: &mut Shell, kind: DialogKind) {
        push(
            h,
            UiIntent::OpenDialog {
                kind,
                subject: DialogSubject::None,
                invoker: None,
            },
        );
        h.run();
        assert_eq!(state(h).dialog.dialog().map(|d| d.kind), Some(kind));
    }

    /// Review unique text opens from a queue request that needs review:
    /// the design scenario's fuzzy row against its counterpart, as
    /// `tests/layout.rs` opens it.
    fn open_unique_text(h: &mut Shell) {
        let results = state(h).scan.results.clone().expect("results");
        let member = results
            .groups
            .iter()
            .flat_map(|g| g.members.iter())
            .find(|m| m.name == "Design_Spec_draft.txt")
            .expect("the design scenario's fuzzy row");
        let MemberRole::Member(link) = member.role else {
            panic!("the fuzzy row is a member");
        };
        push(
            h,
            UiIntent::Review {
                command: ReviewCommand::RequestQueue {
                    file: member.file,
                    counterpart: link.counterpart,
                    origin: QueueOrigin::Shortcut,
                    reviewed: None,
                },
                invoker: None,
            },
        );
        run_until(h, "Review unique text", |s| {
            s.dialog.dialog().map(|d| d.kind) == Some(DialogKind::UniqueTextReview)
        });
        h.run();
    }

    fn close_popup(h: &mut Shell) {
        h.key_press(Key::Escape);
        h.run();
        assert!(state(h).dialog.dialog().is_none(), "the popup closed");
    }

    // ── Measuring ────────────────────────────────────────────────────────

    fn within(actual: f32, expected: f32) -> bool {
        (actual - expected).abs() <= TOLERANCE
    }

    fn color(h: &Shell, token: Token) -> Color32 {
        let mode = match h.ctx.theme() {
            egui::Theme::Light => Mode::Light,
            egui::Theme::Dark => Mode::Dark,
        };
        theme::palette(mode).color(token)
    }

    /// Every rectangle this frame painted, nested shape lists included.
    fn painted_rects(h: &Shell) -> Vec<egui::epaint::RectShape> {
        fn walk(shape: &egui::Shape, out: &mut Vec<egui::epaint::RectShape>) {
            match shape {
                egui::Shape::Rect(rect) => out.push(rect.clone()),
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

    /// The painted popup: the area of egui's top modal layer.
    fn popup_rect(h: &Shell) -> Rect {
        h.ctx.memory(|memory| {
            let layer = memory.top_modal_layer().expect("a popup is drawn");
            memory.area_rect(layer.id).expect("the popup's area")
        })
    }

    fn contains(outer: Rect, inner: Rect) -> bool {
        outer.expand(TOLERANCE).contains_rect(inner)
    }

    /// Whether two rects overlap by more than the A-010 tolerance, so
    /// shared borders and sub-unit rounding are not counted.
    fn overlap(a: Rect, b: Rect) -> bool {
        let i = a.intersect(b);
        i.width() > TOLERANCE && i.height() > TOLERANCE
    }

    // ── P6: Keyboard shortcuts ───────────────────────────────────────────

    /// Spec §11.10's keyboard map, plus frame `44:3681`'s Keyboard shortcuts
    /// row, as row action and the number of key caps the row draws: one
    /// cap per chord (spec §11.6). The chord counts are read off the spec's
    /// Keys cells ("Arrow keys, Page Up/Down, Home/End" is three chords,
    /// "Menu key or Shift+F10" two), so a row drawn as one long cap fails.
    const SHORTCUT_ROWS: [(&str, usize); 24] = [
        ("New scan", 1),
        ("Findings / Compare", 2),
        ("Matching rules", 1),
        ("Open Review cleanup", 1),
        ("Zoom in / zoom out / reset zoom", 3),
        ("Main menu", 1),
        ("Keyboard shortcuts", 2),
        ("Quit", 1),
        ("Search; Esc clears it", 1),
        ("Move row focus", 3),
        ("Expand / collapse a group", 2),
        ("Compare the row against its counterpart", 1),
        (
            "Queue or unqueue; opens Review unique text when review is needed",
            1,
        ),
        ("Row menu", 2),
        ("Copy path", 1),
        ("Next / previous change", 2),
        ("Next / previous candidate", 2),
        ("Switch pane", 1),
        ("Toggle wrap / toggle linked scroll", 2),
        ("Select the whole file in the pane / copy", 2),
        ("Back to Findings", 1),
        ("Move focus", 2),
        ("Cancel", 1),
        ("Default action, never destructive", 1),
    ];

    /// The rectangles painted on the popup's layer and above it: every
    /// shape after the dialog host's frame, whose fill rectangle is the
    /// popup's area. Shapes come out in layer order, so this leaves out the
    /// workspace under the modal, whose own code-token fields would
    /// otherwise pass for key caps where the popup covers them at 200%.
    fn popup_layer_rects(h: &Shell) -> Vec<egui::epaint::RectShape> {
        let popup = popup_rect(h);
        let all = painted_rects(h);
        let frame = all
            .iter()
            .position(|r| {
                (r.rect.min - popup.min).length() <= 2.0 && (r.rect.max - popup.max).length() <= 2.0
            })
            .expect("the dialog host's frame");
        all.into_iter().skip(frame + 1).collect()
    }

    /// Checks every shortcut row at the current size, zoom and theme:
    /// the row exists, draws one cap per chord, and every cap lies inside
    /// its row (whose width is its column's), inside the popup, and clear
    /// of the row's other caps. Each row is scrolled into the body's view
    /// first, because egui paints nothing outside a scroll area's clip.
    /// Returns the failures, so one run reports all of them.
    fn shortcut_failures(h: &mut Shell, combo: &str) -> Vec<String> {
        let mut failures = Vec::new();
        for (action, chords) in SHORTCUT_ROWS {
            let prefix = format!("{action}: ");
            let find = |h: &Shell| {
                h.query_all_by_role(Role::ListItem)
                    .filter_map(|n| n.accesskit_node().label().map(|l| (l, n.rect())))
                    .find(|(l, _)| l.starts_with(&prefix))
            };
            if find(h).is_none() {
                failures.push(format!("{combo}: no row for {action:?}"));
                continue;
            }
            h.query_all_by_role(Role::ListItem)
                .find(|n| {
                    n.accesskit_node()
                        .label()
                        .is_some_and(|l| l.starts_with(&prefix))
                })
                .expect("the row")
                .scroll_to_me();
            h.run();
            let (name, row) = find(h).expect("the row");
            let popup = popup_rect(h);
            let code = color(h, Token::Code);
            // Caps are the popup's only rectangles in the code token: the
            // `DD/Compact/Key cap` fill.
            let caps: Vec<Rect> = popup_layer_rects(h)
                .into_iter()
                .filter(|r| r.fill == code && row.contains(r.rect.center()))
                .map(|r| r.rect)
                .collect();
            if caps.len() != chords {
                failures.push(format!(
                    "{combo}: {name:?} draws {} caps, expected one per chord ({chords})",
                    caps.len()
                ));
            }
            for (i, cap) in caps.iter().enumerate() {
                if !contains(row, *cap) {
                    failures.push(format!(
                        "{combo}: a cap of {name:?} at {cap:?} runs past its column {row:?}"
                    ));
                }
                if !contains(popup, *cap) {
                    failures.push(format!("{combo}: a cap of {name:?} leaves the popup"));
                }
                for other in &caps[i + 1..] {
                    if overlap(*cap, *other) {
                        failures.push(format!("{combo}: caps of {name:?} intersect"));
                    }
                }
            }
        }
        failures
    }

    #[test]
    fn conform_shortcuts_one_cap_per_chord_inside_its_column() {
        let mut failures = Vec::new();
        for size in SIZES {
            let mut h = launch(&format!("shortcuts-{}", size.x), size);
            for zoom in ZOOMS {
                for theme in THEMES {
                    view(&mut h, zoom, theme);
                    open_popup(&mut h, DialogKind::KeyboardShortcuts);
                    let combo = format!("{}×{} at {zoom}% {theme:?}", size.x, size.y);
                    failures.extend(shortcut_failures(&mut h, &combo));
                    close_popup(&mut h);
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[test]
    fn conform_shortcuts_list_the_shortcuts_window_row() {
        let mut h = launch("shortcuts-row", SIZES[0]);
        open_popup(&mut h, DialogKind::KeyboardShortcuts);
        // Frame `44:3681` draws "Keyboard shortcuts  F1  Ctrl+?" in the
        // Global column; spec §11.10 binds both chords to the window.
        assert!(
            h.query_by_role_and_label(Role::ListItem, "Keyboard shortcuts: F1 / Ctrl+?")
                .is_some(),
            "the Keyboard shortcuts row is listed"
        );
    }

    // ── P7: Matching rules ───────────────────────────────────────────────

    /// The similarity value box: the smallest surface-filled rectangle
    /// around the "Minimum similarity" text input.
    fn similarity_box(h: &Shell) -> Rect {
        let input = h
            .query_all_by_label("Minimum similarity")
            .find(|n| n.accesskit_node().role() == Role::TextInput)
            .expect("the similarity input")
            .rect();
        let surface = color(h, Token::Surface);
        painted_rects(h)
            .into_iter()
            .filter(|r| r.fill == surface && r.rect.contains(input.center()))
            .map(|r| r.rect)
            .min_by(|a, b| a.area().total_cmp(&b.area()))
            .expect("the value box around the input")
    }

    #[test]
    fn conform_matching_rules_similarity_field_is_95_wide() {
        let mut failures = Vec::new();
        for size in SIZES {
            let mut h = launch(&format!("rules-{}", size.x), size);
            for zoom in ZOOMS {
                view(&mut h, zoom, ThemeOverride::Light);
                open_popup(&mut h, DialogKind::MatchingRules);
                let field = similarity_box(&h);
                // Frame `16:735` draws the value box about 95 wide and 30
                // high (the §11.3 control height).
                if !within(field.width(), 95.0) || !within(field.height(), 30.0) {
                    failures.push(format!(
                        "{}×{} at {zoom}%: similarity box {} × {}, expected 95 × 30 ±{TOLERANCE}",
                        size.x,
                        size.y,
                        field.width(),
                        field.height()
                    ));
                }
                close_popup(&mut h);
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    // ── P8: Review unique text ───────────────────────────────────────────

    /// T41 item 5, frame `16:932`: both identities show their folder with
    /// the trailing "/" the Findings Folder column shows.
    #[test]
    fn conform_unique_text_folders_keep_the_trailing_slash() {
        let mut h = launch("unique-slash", SIZES[0]);
        open_unique_text(&mut h);
        let results = state(&h).scan.results.clone().expect("results");
        let target = results
            .groups
            .iter()
            .flat_map(|g| g.members.iter())
            .find(|m| m.name == "Design_Spec_draft.txt")
            .expect("the design scenario's fuzzy row");
        let MemberRole::Member(link) = target.role else {
            panic!("the fuzzy row is a member");
        };
        let counterpart = results
            .groups
            .iter()
            .flat_map(|g| g.members.iter())
            .find(|m| m.file == link.counterpart)
            .expect("the counterpart");
        for (who, folder) in [
            ("target", &target.folder),
            ("counterpart", &counterpart.folder),
        ] {
            let shown = format!("{}/", folder.trim_end_matches('/'));
            assert!(
                h.query_by_label(&shown).is_some(),
                "the {who}'s folder does not show as {shown:?}"
            );
        }
    }

    /// The acknowledgment's painted square: the filled square rectangle
    /// at the left of the checkbox widget. A focus ring has no fill, so it
    /// is never taken for the box.
    fn acknowledgment_box(h: &Shell) -> egui::epaint::RectShape {
        let widget = h
            .get_by_role_and_label(Role::CheckBox, unique_text_review::ACKNOWLEDGMENT)
            .rect();
        painted_rects(h)
            .into_iter()
            .filter(|r| {
                r.fill != Color32::TRANSPARENT
                    && contains(widget, r.rect)
                    && (r.rect.width() - r.rect.height()).abs() < 0.5
            })
            .min_by(|a, b| a.rect.left().total_cmp(&b.rect.left()))
            .expect("the acknowledgment's square")
    }

    #[test]
    fn conform_unique_text_acknowledgment_box_is_16_units() {
        let mut failures = Vec::new();
        let mut h = launch("ack", SIZES[0]);
        for zoom in ZOOMS {
            view(&mut h, zoom, ThemeOverride::Light);
            open_unique_text(&mut h);
            let square = acknowledgment_box(&h).rect;
            // Spec §11.7: the checkbox is 16 units.
            if !within(square.width(), 16.0) || !within(square.height(), 16.0) {
                failures.push(format!(
                    "at {zoom}%: acknowledgment box {} × {}, expected 16 × 16 ±{TOLERANCE}",
                    square.width(),
                    square.height()
                ));
            }
            close_popup(&mut h);
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[test]
    fn conform_unique_text_prompt_still_marks_the_box_red() {
        let mut h = launch("ack-prompt", SIZES[0]);
        open_unique_text(&mut h);
        h.query_all_by_role_and_label(Role::Button, "Queue for cleanup")
            .max_by(|a, b| a.rect().top().total_cmp(&b.rect().top()))
            .expect("the footer's Queue for cleanup")
            .click();
        h.run();
        h.run();
        assert!(
            h.query_by_label_contains(unique_text_review::PROMPT)
                .is_some(),
            "the 44:4079 prompt shows"
        );
        let square = acknowledgment_box(&h);
        assert_eq!(
            square.stroke.color,
            color(&h, Token::Red),
            "frame 44:4079 draws the unacknowledged box in the red token"
        );
    }

    // ── P9: New scan file types ──────────────────────────────────────────

    /// A setup with one folder that exists (the test's own target
    /// directory, so New scan's folder check passes) and `types`.
    fn setup_with(types: &[&str]) -> ScanSetup {
        ScanSetup {
            folders: vec![ScanFolder {
                path: PathBuf::from(env!("CARGO_TARGET_TMPDIR")),
                protect: false,
            }],
            file_types: types.iter().map(|t| (*t).to_owned()).collect(),
        }
    }

    fn start_scan(h: &mut Shell) {
        h.query_all_by_role_and_label(Role::Button, "Start scan")
            .max_by(|a, b| a.rect().top().total_cmp(&b.rect().top()))
            .expect("the footer's Start scan")
            .click();
        // Fixed steps, not `run()`: a running scan repaints continuously.
        for _ in 0..4 {
            h.step();
        }
    }

    /// After Start scan: the popup closed, the committed setup kept the
    /// empty list, and the scan the engine ran carries it too (its results
    /// hold the request's setup, `scan_sim::job::finish`).
    fn assert_started_with_no_filter(h: &mut Shell) {
        assert!(
            state(h).dialog.dialog().is_none(),
            "Start scan closed the popup; it showed {:?}",
            h.query_by_label_contains("file type")
                .and_then(|n| n.accesskit_node().label())
        );
        assert!(
            state(h).preferences.scan_setup.file_types.is_empty(),
            "the committed setup keeps the empty list"
        );
        let folder = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
        run_until(h, "the new scan's results", |s| {
            s.scan
                .results
                .as_ref()
                .is_some_and(|r| r.setup.folders.first().is_some_and(|f| f.path == folder))
        });
        let results = state(h).scan.results.clone().expect("results");
        assert!(
            results.setup.file_types.is_empty(),
            "the scan ran with no extension filter"
        );
    }

    #[test]
    fn conform_new_scan_fresh_setup_selects_all_plain_text_files() {
        let mut h = launch("new-scan-fresh", SIZES[0]);
        state_mut(&mut h).preferences.scan_setup = setup_with(&[]);
        open_popup(&mut h, DialogKind::NewScan);
        // Owner decision 2026-11-24 (spec 0.13): an empty list is every
        // plain-text file (C-013), shown as such, and Start needs no type.
        assert!(
            h.query_by_label("All plain-text files").is_some(),
            "the summary reads All plain-text files"
        );
        start_scan(&mut h);
        assert_started_with_no_filter(&mut h);
    }

    #[test]
    fn conform_new_scan_clearing_file_types_returns_to_all_plain_text_files() {
        let mut h = launch("new-scan-clear", SIZES[0]);
        state_mut(&mut h).preferences.scan_setup = setup_with(&["txt", "md"]);
        open_popup(&mut h, DialogKind::NewScan);
        assert!(h.query_by_label("All plain-text files").is_none());

        h.get_by_role_and_label(Role::Button, "Edit…").click();
        h.run();
        let input = h.get_by_role_and_label(Role::TextInput, "File types");
        input.focus();
        h.run();
        h.key_press_modifiers(Modifiers::COMMAND, Key::A);
        h.key_press(Key::Backspace);
        h.run();
        h.get_by_role_and_label(Role::Button, "Done").click();
        h.run();

        assert!(
            h.query_by_label("All plain-text files").is_some(),
            "clearing the field returns to All plain-text files"
        );
        start_scan(&mut h);
        assert_started_with_no_filter(&mut h);
    }
}
