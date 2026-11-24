// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! PV-T40-001, leg P1 (plan T40, items P1 to P5): the Review cleanup popup
//! conforms to frame `16:886` at 1600 × 960 and 1024 × 600, each at zoom
//! 100% and 200%, in light and dark, driven headless through
//! `egui_kittest`.
//!
//! Every test is named `conform_cleanup_…`, so `-E 'test(conform_)'`
//! selects them with the other legs' conformance tests.
//!
//! The rig runs the app's frame order over T12's public seams, as
//! `tests/cleanup_dialog.rs` does, because the app always adopts the design
//! scenario and the full-path case needs the unicode one. It differs in two
//! ways that make it the window as drawn: it draws into a child spanning
//! the whole content rect, as `tests/layout.rs` does, not inside kittest's
//! 8-unit outer margin; and it applies the theme and zoom preferences the
//! way `app.rs` does, through `theme::apply_zoom`, so 200% on a
//! 1600 × 960 window leaves 800 × 480 points and on 1024 × 600, 512 × 300.
//!
//! Geometry comes from accesskit, whose bounds are the unclipped widget
//! rects, and, for text painted inside a button, from the frame's text
//! shapes. Comparisons allow the ±1-unit layout tolerance (A-010).
//! Expected values are the frame's numbers written as literals: 51-unit
//! rows and a 28-unit header band (`16:886`, spec §11.6).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use egui::accesskit::Role;
use egui::{Event, Key, Modifiers, Rect, Vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT as _, Queryable};
use fuzzy_dedupe_app::presentation::commands::{self, IntentQueue, Services, UiIntent};
use fuzzy_dedupe_app::presentation::events;
use fuzzy_dedupe_app::presentation::state::{
    CleanupPhase, DialogKind, DialogSubject, PresentationState, ScanStage, ThemeOverride,
};
use fuzzy_dedupe_app::ui::glyphs;
use fuzzy_dedupe_app::ui::theme::{self, Zoom};
use fuzzy_dedupe_app::ui::widgets::dialog_host;
use fuzzy_dedupe_domain::{FileId, MemberRole, QueueOrigin};
use fuzzy_dedupe_engine::fixtures::{self, Scenario, ScenarioKind, SeededDecision};
use fuzzy_dedupe_engine::service::{Engine, EngineConfig, ReviewCommand, ReviewStatus};
use fuzzy_dedupe_platform::dirs::AppDirs;
use fuzzy_dedupe_platform::open::{OpenHandle, Opener};
use fuzzy_dedupe_platform::picker::{FolderPicker, PickHandle, PickOutcome, PickRequest};

/// Step bound for every `Harness::run` here, in place of kittest's default
/// of 4.
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
/// Outer bound on any wait for engine events.
const TIMEOUT: Duration = Duration::from_secs(30);
const INVOKER: &str = "Open cleanup";
const HEADERS: [&str; 6] = ["File", "Folder", "Match", "Kept copy", "Size", "Status"];

/// The four supported size and zoom combinations, each in both modes.
fn combinations() -> Vec<(Vec2, u16, ThemeOverride)> {
    let mut out = Vec::new();
    for size in [Vec2::new(1600.0, 960.0), Vec2::new(1024.0, 600.0)] {
        for zoom in [100, 200] {
            for theme in [ThemeOverride::Light, ThemeOverride::Dark] {
                out.push((size, zoom, theme));
            }
        }
    }
    out
}

// ── Doubles ──────────────────────────────────────────────────────────────

struct NoOpener;

impl Opener for NoOpener {
    fn open(
        &self,
        _: fuzzy_dedupe_platform::open::Location,
        notify: Box<dyn FnOnce() + Send>,
    ) -> OpenHandle {
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

/// Directories under the test's own target directory; nothing is written
/// there, but `Services` needs a resolved set.
fn dirs(name: &str) -> AppDirs {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after 1970")
        .as_nanos();
    let root: PathBuf = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("conform_cleanup")
        .join(format!("{name}-{nanos}"));
    AppDirs::resolve(|_| None, &root.join("home"))
}

// ── The rig ──────────────────────────────────────────────────────────────

struct Rig {
    state: PresentationState,
    services: Services,
    scenario: Arc<Scenario>,
    /// Unset until the theme is installed: the harness draws a first frame
    /// on build, before the compact font families exist.
    ready: bool,
}

impl Rig {
    fn frame(&mut self, ui: &mut egui::Ui) {
        if !self.ready {
            return;
        }
        let ctx = ui.ctx().clone();
        // The preferences, applied as `app.rs`'s `apply_preferences` does.
        let wanted = match self.state.preferences.theme {
            ThemeOverride::System => egui::ThemePreference::System,
            ThemeOverride::Light => egui::ThemePreference::Light,
            ThemeOverride::Dark => egui::ThemePreference::Dark,
        };
        if ctx.options(|o| o.theme_preference) != wanted {
            ctx.set_theme(wanted);
        }
        let zoom = self.state.preferences.zoom;
        if (ctx.zoom_factor() - zoom.factor()).abs() > f32::EPSILON {
            theme::apply_zoom(&ctx, zoom);
        }
        for event in self.services.engine.drain_events(256) {
            events::apply(&mut self.state, event);
        }
        self.services.poll(&mut self.state);
        dialog_host::begin_frame(&ctx, &mut self.state.dialog);
        // The whole content rect, as eframe hands `App::ui` the client area;
        // kittest's own Ui sits inside an 8-unit margin.
        let full = ctx.content_rect();
        let mut window = ui.new_child(egui::UiBuilder::new().max_rect(full));
        let mut intents = IntentQueue::default();
        let invoker = window.button(INVOKER);
        if invoker.clicked() {
            intents.push(UiIntent::OpenDialog {
                kind: DialogKind::Cleanup,
                subject: DialogSubject::None,
                invoker: Some(invoker.id),
            });
        }
        dialog_host::show(&ctx, &mut self.state, &mut intents);
        let mut batch = intents.drain();
        for _ in 0..4 {
            if batch.is_empty() {
                break;
            }
            let mut followups = IntentQueue::default();
            for intent in batch {
                commands::dispatch(
                    intent,
                    &mut self.state,
                    &mut self.services,
                    &ctx,
                    &mut followups,
                );
            }
            batch = followups.drain();
        }
    }

    fn review(&mut self, command: ReviewCommand) {
        let shown = format!("{command:?}");
        let outcome = self.services.engine.review(command);
        assert!(
            !matches!(outcome.status, ReviewStatus::Refused { .. }),
            "{shown} answered {:?}",
            outcome.status
        );
        events::apply_review(&mut self.state, &outcome);
        self.services.sync_view(&mut self.state);
    }

    /// Queues `file` against `counterpart`, acknowledging unique text where
    /// the policy asks, as a user would.
    fn queue_against(&mut self, file: FileId, counterpart: FileId) {
        let request = ReviewCommand::RequestQueue {
            file,
            counterpart,
            origin: QueueOrigin::Checkbox,
            reviewed: None,
        };
        let outcome = self.services.engine.review(request.clone());
        events::apply_review(&mut self.state, &outcome);
        if let ReviewStatus::NeedsReview { .. } = outcome.status {
            self.review(ReviewCommand::AcknowledgeUniqueText {
                file,
                counterpart,
                reviewed: None,
            });
            self.review(request);
        }
        self.services.sync_view(&mut self.state);
    }

    fn member(&self, file: FileId) -> &fuzzy_dedupe_domain::Member {
        self.scenario
            .results
            .group_of(file)
            .and_then(|g| g.member(file))
            .expect("in the results")
    }
}

type Dialog = Harness<'static, Rig>;

/// The rig on `kind` at `size`, `zoom` and `theme`, adopted and seeded as
/// `app.rs` does it.
fn launch(
    name: &str,
    kind: ScenarioKind,
    (size, zoom, theme): (Vec2, u16, ThemeOverride),
) -> Dialog {
    let mut engine = Engine::new(EngineConfig::default()).expect("engine starts");
    let scan = engine.reserve_scan_id();
    let scenario = Arc::new(fixtures::build(kind, scan, fixtures::DEFAULT_SEED).expect("fixture"));
    engine
        .adopt_scenario(Arc::clone(&scenario))
        .expect("fresh scan");
    let mut rig = Rig {
        state: PresentationState::default(),
        services: Services::new(
            engine,
            Box::new(NoOpener),
            Box::new(DismissPicker),
            dirs(name),
        ),
        scenario: Arc::clone(&scenario),
        ready: false,
    };
    rig.state.preferences.zoom = Zoom::from_percent(zoom).expect("on the zoom grid");
    rig.state.preferences.theme = theme;
    events::install_results(
        &mut rig.state,
        Arc::clone(&scenario.results),
        ScanStage::Completed,
    );
    rig.services.sync_view(&mut rig.state);
    for seed in scenario.seeded_decisions.clone() {
        match seed {
            SeededDecision::Kept(file) => rig.review(ReviewCommand::MarkKept { file, kept: true }),
            SeededDecision::Protect(file) => rig.review(ReviewCommand::ProtectFile {
                file,
                protected: true,
            }),
            SeededDecision::MakeReference(file) => {
                rig.review(ReviewCommand::MakeReference { file });
            }
            SeededDecision::Queue { file, counterpart } => rig.queue_against(file, counterpart),
        }
    }
    let mut builder = Harness::builder();
    if kind == ScenarioKind::Unicode {
        // The full-path case asserts the accessible text of Japanese and
        // Arabic names, not their rasterization, and the workstation's
        // fallback fonts are not part of this rig; glyph coverage is
        // `tests/glyph_registry.rs`'s concern.
        builder = builder.allow_missing_glyphs();
    }
    let mut harness = builder
        .with_size(size)
        .with_max_steps(SETTLE_STEPS)
        .build_ui_state(|ui, rig: &mut Rig| rig.frame(ui), rig);
    theme::install(&harness.ctx, Vec::new());
    glyphs::install(&harness.ctx);
    harness.state_mut().ready = true;
    harness.run();
    harness
}

/// Opens the popup and waits for its plan, then settles the frame that
/// draws it.
fn open_cleanup(h: &mut Dialog) {
    h.get_by_role_and_label(Role::Button, INVOKER).click();
    h.run();
    let deadline = Instant::now() + TIMEOUT;
    while h.state().state.operation.phase != CleanupPhase::Review {
        assert!(Instant::now() < deadline, "timed out waiting for the plan");
        h.step();
        std::thread::yield_now();
    }
    h.run();
}

fn rect_of(h: &Dialog, label: &str) -> Rect {
    h.get_by_label(label).rect()
}

fn near(actual: f32, expected: f32) -> bool {
    (actual - expected).abs() <= TOLERANCE
}

fn what(size: Vec2, zoom: u16, theme: ThemeOverride) -> String {
    format!("{} × {} at {zoom}% ({theme:?})", size.x, size.y)
}

/// The queued targets' file names, in plan order (the table's row order).
fn target_names(h: &Dialog) -> Vec<String> {
    let rig = h.state();
    rig.state
        .operation
        .items
        .iter()
        .map(|item| rig.member(item.target).name.clone())
        .collect()
}

/// The popup body's horizontal extent: from the summary line's left edge,
/// at the content inset, to the title row's × control, whose right edge is
/// the content inset's right edge.
fn body_x(h: &Dialog) -> (f32, f32) {
    let summary = h
        .query_all_by(|node| {
            node.label()
                .or_else(|| node.value())
                .is_some_and(|l| l.contains("files queued"))
        })
        .next()
        .expect("the summary line")
        .rect();
    let close = h.get_by_role_and_label(Role::Button, "Close").rect();
    (summary.left(), close.right())
}

// ── P1 and P2: 51-unit rows, centred content, a 28-unit header band ─────

#[test]
fn conform_cleanup_rows_are_51_high_under_a_28_unit_header_band() {
    for combination in combinations() {
        let (size, zoom, theme) = combination;
        let at = what(size, zoom, theme);
        let mut h = launch("rows", ScenarioKind::Design, combination);
        open_cleanup(&mut h);
        let names = target_names(&h);
        assert!(names.len() >= 3, "{at}: the design plan has three rows");

        // Row pitch, read from the File cells: a row that lost its height
        // packs the next one closer than 51 (T24: about 33).
        let centres: Vec<f32> = names
            .iter()
            .map(|name| rect_of(&h, name).center().y)
            .collect();
        for pair in centres.windows(2) {
            let pitch = pair[1] - pair[0];
            assert!(
                near(pitch, 51.0),
                "{at}: row pitch {pitch}, expected 51 ±{TOLERANCE} (frame 16:886)"
            );
        }

        // Each row's content is centred in it, single-line cells and the
        // two-line Kept copy cell alike.
        let rows: Vec<Rect> = h
            .query_all_by_role(Role::Row)
            .map(|node| node.rect())
            .collect();
        assert_eq!(rows.len(), names.len(), "{at}: one focusable row per item");
        for (row, name) in rows.iter().zip(&names) {
            assert!(near(row.height(), 51.0), "{at}: row {} high", row.height());
            let file = rect_of(&h, name).center().y;
            assert!(
                near(file, row.center().y),
                "{at}: {name} centred at {file}, row centre {}",
                row.center().y
            );
        }
        let detail = h
            .query_all_by_label("~/Documents/Projects/ · protected")
            .next()
            .expect("the first kept copy's folder line")
            .rect();
        let kept = h
            .query_all_by_label("Project_Plan.txt")
            .map(|n| n.rect())
            .find(|r| r.bottom() <= detail.top() + TOLERANCE && rows[0].contains(r.center()))
            .expect("the first kept copy's name line");
        let block = f32::midpoint(kept.top(), detail.bottom());
        assert!(
            near(block, rows[0].center().y),
            "{at}: the Kept copy block centred at {block}, row centre {}",
            rows[0].center().y
        );

        // The header band is 28 high and ends where the first row begins,
        // so its titles centre 14 above that row's top.
        for title in HEADERS {
            let y = rect_of(&h, title).center().y;
            assert!(
                near(y, rows[0].top() - 14.0),
                "{at}: header {title:?} centred at {y}, expected {} (28-unit band)",
                rows[0].top() - 14.0
            );
        }
    }
}

// ── P3: footer labels centred ────────────────────────────────────────────

/// Where the frame painted `text`, from its text shapes: a button's label
/// is not an accesskit node of its own.
fn painted_text(h: &Dialog, text: &str) -> Rect {
    fn walk(shape: &egui::Shape, text: &str, out: &mut Option<Rect>) {
        match shape {
            egui::Shape::Text(shape) if shape.galley.text() == text => {
                *out = Some(shape.visual_bounding_rect());
            }
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, text, out)),
            _ => {}
        }
    }
    let mut out = None;
    for clipped in &h.output().shapes {
        walk(&clipped.shape, text, &mut out);
    }
    out.unwrap_or_else(|| panic!("{text:?} was not painted"))
}

#[test]
fn conform_cleanup_footer_labels_are_centred_in_their_buttons() {
    for combination in combinations() {
        let (size, zoom, theme) = combination;
        let at = what(size, zoom, theme);
        let mut h = launch("footer", ScenarioKind::Design, combination);
        open_cleanup(&mut h);
        let count = h.state().state.operation.confirmed_count();
        let confirm = format!("Move {count} files to Trash");
        for label in ["Cancel", confirm.as_str()] {
            let button = h.get_by_role_and_label(Role::Button, label).rect();
            assert!(
                near(button.height(), 30.0),
                "{at}: {label} is {} high",
                button.height()
            );
            let text = painted_text(&h, label).center().y;
            assert!(
                near(text, button.center().y),
                "{at}: {label:?} text centred at {text}, button centre {}",
                button.center().y
            );
        }
        let note = rect_of(&h, "Nothing has been removed.").center().y;
        let cancel = h
            .get_by_role_and_label(Role::Button, "Cancel")
            .rect()
            .center()
            .y;
        assert!(
            near(note, cancel),
            "{at}: the footer note centred at {note}, the buttons at {cancel}"
        );
    }
}

// ── P4: columns fit the body or scroll inside it ─────────────────────────

#[test]
fn conform_cleanup_columns_fit_the_body_or_scroll_inside_it() {
    let mut scrolled = 0;
    for combination in combinations() {
        let (size, zoom, theme) = combination;
        let at = what(size, zoom, theme);
        let mut h = launch("columns", ScenarioKind::Design, combination);
        open_cleanup(&mut h);
        let (left, right) = body_x(&h);
        let headers: Vec<Rect> = HEADERS.iter().map(|t| rect_of(&h, t)).collect();
        for (i, a) in headers.iter().enumerate() {
            for (j, b) in headers.iter().enumerate().skip(i + 1) {
                assert!(
                    !a.intersects(*b),
                    "{at}: headers {:?} and {:?} overlap",
                    HEADERS[i],
                    HEADERS[j]
                );
            }
        }
        assert!(
            headers[0].left() >= left - TOLERANCE,
            "{at}: File starts at {}, left of the body at {left}",
            headers[0].left()
        );
        let inside = |r: &Rect| r.left() >= left - TOLERANCE && r.right() <= right + TOLERANCE;
        if headers.iter().all(inside) {
            continue;
        }
        // Not every column fits: the table must scroll sideways inside the
        // body, so the last column can be brought in without leaving it.
        scrolled += 1;
        h.get_by_label("Status").scroll_to_me();
        h.run();
        let status = rect_of(&h, "Status");
        assert!(
            inside(&status),
            "{at}: Status at {:?} after scrolling, body spans {left}..{right}",
            status.x_range()
        );
        let ready = h
            .query_all_by_label("Ready")
            .next()
            .expect("a Ready status cell")
            .rect();
        assert!(
            inside(&ready),
            "{at}: the Status cells scroll with their header ({:?})",
            ready.x_range()
        );
    }
    // 512 × 300 points (1024 × 600 at 200%) leaves the table about 444
    // units against the 652 its minimum columns need, so the scrolling
    // branch above is exercised, not vacuous.
    assert!(scrolled > 0, "no combination needed the table to scroll");
}

// ── P5: the full path on keyboard focus ──────────────────────────────────

fn press(h: &mut Dialog, key: Key) {
    for pressed in [true, false] {
        h.input_mut().events.push(Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: Modifiers::NONE,
        });
    }
    h.run();
}

#[test]
fn conform_cleanup_keyboard_focus_on_a_row_shows_its_full_paths() {
    for combination in combinations() {
        let (size, zoom, theme) = combination;
        let at = what(size, zoom, theme);
        let mut h = launch("full-path", ScenarioKind::Unicode, combination);
        // Queue the long-path group's non-reference member, so the long
        // path is in the plan as a target or as the kept copy.
        let (file, counterpart, long) = {
            let results = &h.state().scenario.results;
            let long = results
                .groups
                .iter()
                .flat_map(|g| g.members.iter())
                .find(|m| m.display_path.contains("長いフォルダー名の例"))
                .expect("the unicode scenario's long path");
            let group = results.group_of(long.file).expect("its group");
            let target = group
                .members
                .iter()
                .find(|m| m.role != MemberRole::Reference)
                .expect("a non-reference member");
            (
                target.file,
                target.counterpart().expect("a counterpart"),
                long.display_path.clone(),
            )
        };
        h.state_mut().queue_against(file, counterpart);
        open_cleanup(&mut h);
        assert!(
            h.query_all_by_label(&long).next().is_none(),
            "{at}: the full path shows before any row has focus"
        );

        // Tab, never the pointer, until a row has focus.
        let mut focused = false;
        for _ in 0..8 {
            press(&mut h, Key::Tab);
            if h.query_all_by_role(Role::Row).any(|n| n.is_focused()) {
                focused = true;
                break;
            }
        }
        assert!(focused, "{at}: Tab never reaches a table row");
        assert!(
            h.query_all_by_label(&long).next().is_some(),
            "{at}: the focused row does not show the full path {long:?}"
        );
    }
}

// ── Row keys: every row reachable, not only the drawn ones ──────────────

/// The design plan plus every other member the engine lets the rig queue
/// against its counterpart, so the plan runs well past the table's six
/// visible rows and the virtualised table leaves most rows undrawn.
fn queue_many(h: &mut Dialog) {
    let candidates: Vec<(FileId, FileId)> = h
        .state()
        .scenario
        .results
        .groups
        .iter()
        .flat_map(|g| g.members.iter())
        .filter(|m| m.role != MemberRole::Reference && !m.folder_protected)
        .filter_map(|m| Some((m.file, m.counterpart()?)))
        .collect();
    let rig = h.state_mut();
    for (file, counterpart) in candidates {
        let request = ReviewCommand::RequestQueue {
            file,
            counterpart,
            origin: QueueOrigin::Checkbox,
            reviewed: None,
        };
        let outcome = rig.services.engine.review(request.clone());
        events::apply_review(&mut rig.state, &outcome);
        if let ReviewStatus::NeedsReview { .. } = outcome.status {
            let ack = rig
                .services
                .engine
                .review(ReviewCommand::AcknowledgeUniqueText {
                    file,
                    counterpart,
                    reviewed: None,
                });
            events::apply_review(&mut rig.state, &ack);
            let again = rig.services.engine.review(request);
            events::apply_review(&mut rig.state, &again);
        }
    }
    rig.services.sync_view(&mut rig.state);
}

/// Each plan row's accessible label, in table order, as `cleanup.rs`
/// writes it: "<target path>, kept copy <kept path>".
fn row_labels(h: &Dialog) -> Vec<String> {
    let rig = h.state();
    rig.state
        .operation
        .items
        .iter()
        .map(|item| {
            format!(
                "{}, kept copy {}",
                rig.member(item.target).display_path,
                rig.member(item.counterpart).display_path
            )
        })
        .collect()
}

/// The table position of the focused row, if a row has focus.
fn focused_row(h: &Dialog, labels: &[String]) -> Option<usize> {
    let label = h
        .query_all_by_role(Role::Row)
        .find(egui_kittest::Node::is_focused)?
        .accesskit_node()
        .label()?;
    labels.iter().position(|l| *l == label)
}

#[test]
fn conform_cleanup_row_keys_reach_every_row_of_a_long_plan() {
    for combination in [
        (Vec2::new(1600.0, 960.0), 100, ThemeOverride::Light),
        (Vec2::new(1024.0, 600.0), 200, ThemeOverride::Light),
    ] {
        let (size, zoom, theme) = combination;
        let at = what(size, zoom, theme);
        let mut h = launch("row-keys", ScenarioKind::Design, combination);
        queue_many(&mut h);
        open_cleanup(&mut h);
        let labels = row_labels(&h);
        let last = labels.len() - 1;
        assert!(
            labels.len() >= 12,
            "{at}: the plan has {} rows, too few to leave rows undrawn",
            labels.len()
        );
        assert!(
            h.query_all_by_role(Role::Row).count() < labels.len(),
            "{at}: every row is drawn, so the case is vacuous"
        );

        // Tab lands on the first visible row, the first row here.
        let mut tabs = 0;
        while focused_row(&h, &labels).is_none() {
            tabs += 1;
            assert!(tabs <= 8, "{at}: Tab never reaches a table row");
            press(&mut h, Key::Tab);
        }
        assert_eq!(
            focused_row(&h, &labels),
            Some(0),
            "{at}: Tab lands on row 0"
        );

        // End reaches the last row: focused, inside the visible body, with
        // its full path shown.
        press(&mut h, Key::End);
        h.run();
        assert_eq!(
            focused_row(&h, &labels),
            Some(last),
            "{at}: End did not move focus to the last of {} rows",
            labels.len()
        );
        let row = h
            .query_all_by_role(Role::Row)
            .find(egui_kittest::Node::is_focused)
            .expect("the focused row")
            .rect();
        let title = h.get_by_role_and_label(Role::Button, "Close").rect();
        let footer = h.get_by_role_and_label(Role::Button, "Cancel").rect();
        assert!(
            row.top() >= title.bottom() - TOLERANCE && row.bottom() <= footer.top() + TOLERANCE,
            "{at}: the last row at {:?} is outside the body between {} and {}",
            row.y_range(),
            title.bottom(),
            footer.top()
        );
        let item = h.state().state.operation.items[last];
        let path = h.state().member(item.target).display_path.clone();
        assert!(
            h.query_all_by_label(&path).next().is_some(),
            "{at}: the last row's full path {path:?} does not show"
        );

        // Home returns to the first row; Page Down then moves by a page of
        // the table's viewport, six 51-unit rows in 306 units (frame
        // `16:886`'s six visible rows), even where the popup body shows
        // fewer of them.
        press(&mut h, Key::Home);
        h.run();
        assert_eq!(focused_row(&h, &labels), Some(0), "{at}: Home");
        press(&mut h, Key::PageDown);
        h.run();
        let paged = focused_row(&h, &labels).expect("a focused row after Page Down");
        assert!(
            (5..=6).contains(&paged),
            "{at}: Page Down moved from row 0 to row {paged}"
        );
        press(&mut h, Key::ArrowDown);
        h.run();
        assert_eq!(focused_row(&h, &labels), Some(paged + 1), "{at}: Down");
        press(&mut h, Key::ArrowUp);
        h.run();
        assert_eq!(focused_row(&h, &labels), Some(paged), "{at}: Up");
        press(&mut h, Key::PageUp);
        h.run();
        assert_eq!(focused_row(&h, &labels), Some(0), "{at}: Page Up");
        assert!(
            h.state().state.dialog.is_open(),
            "{at}: the row keys never close the popup"
        );
    }
}
