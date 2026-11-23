// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! PV-T14-001 and PV-T14-002 (plan T14): the paired diff widget, driven
//! headless through `egui_kittest`.
//!
//! Every test sits in the `diff_view` module so `-E 'test(diff_view)'`
//! selects them. Comparisons come from the real `Engine` facade: a
//! scenario holding the generated texts is adopted and `LoadComparison`
//! computes the C-04 model, exactly as Compare receives it.
//!
//! The oracle is never `similar`'s output. Each pair is built from an edit
//! script whose lines are all distinct except kept lines and their copies,
//! so the script alone fixes every row, source line number and Only-in
//! count; the widget's display is compared against that. Copies are
//! compared byte for byte against slices of the generated buffers.
//!
//! Cross-file contract with `src/ui/widgets/diff_view.rs`: each visible row
//! of each pane is one accessibility node whose author ID is
//! `diff_view:a:<row>` or `diff_view:b:<row>` (zero-based aligned row),
//! whose label is the displayed source line number (absent on a spacer),
//! and whose bounds are the row's text cell: full row height, starting at
//! the first character column. The caption and notice strings below are
//! the widget's own text.

mod diff_view {
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use egui::accesskit::Role;
    use egui::{Event, Key, Modifiers, OutputCommand, PointerButton, Pos2, Rect, Vec2};
    use egui_kittest::Harness;
    use egui_kittest::kittest::{NodeT, Queryable};
    use fuzzy_dedupe_app::presentation::state::{
        ComparePair, CompareState, ComparisonSlot, PaneSelection, TextPosition,
    };
    use fuzzy_dedupe_app::ui::theme::{self, CompactStyle};
    use fuzzy_dedupe_app::ui::widgets::diff_view;
    use fuzzy_dedupe_domain::{CompareMode, FileId, GroupId, SnapshotId};
    use fuzzy_dedupe_engine::compare::ComparisonRequest;
    use fuzzy_dedupe_engine::compare::texts::{self, EditOp, LargePair};
    use fuzzy_dedupe_engine::fixtures::{DEFAULT_SEED, FileContent, FixtureContents};
    use fuzzy_dedupe_engine::service::{Engine, EngineConfig, EngineEvent, EngineRequest, Origin};

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

    /// The Document style's row height in units.
    const ROW: f32 = 22.0;

    // ── Generated pairs ──────────────────────────────────────────────────

    /// A pair built from `script`, A first.
    struct Pair {
        a: String,
        b: String,
        script: Vec<EditOp>,
    }

    /// Far longer than a pane at 1600 units, so wrap gives it several
    /// visual lines; the id prefix keeps it distinct from every other line.
    fn long(prefix: &str) -> String {
        format!("{prefix}{}", " lorem ipsum dolor".repeat(30))
    }

    /// Builds both texts from `script`, every line ended by `ending`. A kept
    /// line has the same text in both files; every other line is unique to
    /// its file, so the script is the only possible line diff. `long_a` and
    /// `long_b` name zero-based source lines made long.
    fn generate(script: &[EditOp], ending: &str, long_a: &[usize], long_b: &[usize]) -> Pair {
        let (mut a_lines, mut b_lines) = (Vec::new(), Vec::new());
        for op in script {
            match *op {
                EditOp::Keep(n) => {
                    for _ in 0..n {
                        let text = format!("kept line {} of A", a_lines.len());
                        b_lines.push(text.clone());
                        a_lines.push(text);
                    }
                }
                EditOp::Replace(n) | EditOp::Delete(n) | EditOp::Insert(n) => {
                    if !matches!(op, EditOp::Insert(_)) {
                        for _ in 0..n {
                            a_lines.push(format!("line {} only in A", a_lines.len()));
                        }
                    }
                    if !matches!(op, EditOp::Delete(_)) {
                        for _ in 0..n {
                            b_lines.push(format!("line {} only in B", b_lines.len()));
                        }
                    }
                }
            }
        }
        let join = |lines: Vec<String>, longs: &[usize]| {
            let mut out = String::new();
            for (index, line) in lines.into_iter().enumerate() {
                out.push_str(&if longs.contains(&index) {
                    long(&line)
                } else {
                    line
                });
                out.push_str(ending);
            }
            out
        };
        Pair {
            a: join(a_lines, long_a),
            b: join(b_lines, long_b),
            script: script.to_vec(),
        }
    }

    /// The rows the script implies, as displayed one-based source line
    /// numbers; `None` is a spacer. A Replace pairs its lines one to one.
    fn expected_rows(script: &[EditOp]) -> Vec<(Option<usize>, Option<usize>)> {
        let (mut a, mut b) = (0, 0);
        let mut rows = Vec::new();
        for op in script {
            let (n, has_a, has_b) = match *op {
                EditOp::Keep(n) | EditOp::Replace(n) => (n, true, true),
                EditOp::Delete(n) => (n, true, false),
                EditOp::Insert(n) => (n, false, true),
            };
            for _ in 0..n {
                let left = has_a.then(|| {
                    a += 1;
                    a
                });
                let right = has_b.then(|| {
                    b += 1;
                    b
                });
                rows.push((left, right));
            }
        }
        rows
    }

    /// Lines only in A and only in B by the script: every non-kept line.
    fn script_unique(script: &[EditOp]) -> (u64, u64) {
        script.iter().fold((0, 0), |(a, b), op| match *op {
            EditOp::Keep(_) => (a, b),
            EditOp::Replace(n) => (a + n as u64, b + n as u64),
            EditOp::Delete(n) => (a + n as u64, b),
            EditOp::Insert(n) => (a, b + n as u64),
        })
    }

    /// One-sided insertions and deletions, a Replace whose B line is long,
    /// a long B-only insertion and a Replace whose A line is long, each
    /// hunk separated by kept lines.
    fn mixed_script() -> Vec<EditOp> {
        use EditOp::{Delete, Insert, Keep, Replace};
        vec![
            Keep(2),
            Insert(2),
            Keep(2),
            Replace(1),
            Keep(1),
            Delete(2),
            Keep(2),
            Insert(1),
            Keep(1),
            Replace(1),
            Keep(2),
        ]
    }

    /// B line 6 (row 6) is the first Replace's, B line 10 (row 12) the
    /// long insertion and A line 11 (row 14) the last Replace's, all
    /// zero-based.
    fn mixed_pair(ending: &str) -> Pair {
        generate(&mixed_script(), ending, &[11], &[6, 10])
    }

    // ── Engine ───────────────────────────────────────────────────────────

    /// Computes the C-04 comparison of `a` against `b` through the real
    /// engine and returns Compare's state holding it, as `events.rs` fills
    /// it. File 1 is the target (A), file 2 the counterpart (B).
    fn load(a: &str, b: &str, mode: CompareMode) -> CompareState {
        let mut engine = Engine::new(EngineConfig::default()).expect("engine starts");
        let scan = engine.reserve_scan_id();
        let mut scenario = texts::large_pair_scenario(scan, DEFAULT_SEED);
        let (target, counterpart) = (FileId::new(1), FileId::new(2));
        let mut contents = FixtureContents::default();
        contents.insert(target, FileContent::Bytes(Arc::from(a.as_bytes())));
        contents.insert(counterpart, FileContent::Bytes(Arc::from(b.as_bytes())));
        scenario.contents = contents;
        let rules = scenario.results.rules.revision;
        engine
            .adopt_scenario(Arc::new(scenario))
            .expect("fresh scan ID");
        let key = ComparisonRequest {
            scan,
            target,
            counterpart,
            target_snapshot: SnapshotId::new(1),
            counterpart_snapshot: SnapshotId::new(2),
            rules,
            mode,
        };
        let Ok(Origin::Request(request)) = engine.submit(EngineRequest::LoadComparison(key)) else {
            panic!("comparison not admitted");
        };
        let deadline = Instant::now() + Duration::from_mins(2);
        let result = loop {
            let found = engine
                .drain_events(64)
                .into_iter()
                .find_map(|event| match event {
                    EngineEvent::Comparison {
                        request: id,
                        comparison,
                    } if id == request => Some(comparison),
                    _ => None,
                });
            if let Some(found) = found {
                break found;
            }
            assert!(Instant::now() < deadline, "no comparison within 120 s");
            std::thread::sleep(Duration::from_millis(5));
        };
        CompareState {
            pair: Some(ComparePair {
                group: GroupId::new(1),
                target,
                counterpart,
            }),
            target_snapshot: Some(key.target_snapshot),
            counterpart_snapshot: Some(key.counterpart_snapshot),
            comparison: ComparisonSlot {
                request: Some(request),
                key: Some(key),
                result: Some(result),
            },
            mode,
            ..CompareState::default()
        }
    }

    // ── Harness ──────────────────────────────────────────────────────────

    /// The harness state: Compare's state, drawn only once the theme is
    /// installed, because the harness runs a frame while it is built and
    /// the shipped text styles do not exist before `theme::install`.
    struct Slot {
        compare: CompareState,
        themed: bool,
    }

    type View = Harness<'static, Slot>;

    /// The widget alone in a 1600 x 800 area, with the shipped fonts and
    /// styles, so character widths are IBM Plex Mono's.
    fn view(compare: CompareState) -> View {
        let mut harness = Harness::builder()
            .with_max_steps(SETTLE_STEPS)
            .with_size(Vec2::new(1600.0, 800.0))
            .build_ui_state(
                |ui, slot: &mut Slot| {
                    if slot.themed {
                        diff_view::show(ui, &mut slot.compare, true);
                    }
                },
                Slot {
                    compare,
                    themed: false,
                },
            );
        theme::install(&harness.ctx, Vec::new());
        harness.state_mut().themed = true;
        harness.run();
        harness
    }

    /// The node of `row` in pane `side` ('a' or 'b'), if it is laid out.
    fn cell(harness: &View, side: char, row: usize) -> Option<Rect> {
        node_label(harness, side, row).map(|(rect, _)| rect)
    }

    fn node_label(harness: &View, side: char, row: usize) -> Option<(Rect, Option<String>)> {
        let id = format!("diff_view:{side}:{row}");
        harness
            .query_by(move |node| node.author_id() == Some(id.as_str()))
            .map(|node| {
                let label = node.accesskit_node().label().filter(|l| !l.is_empty());
                (node.rect(), label)
            })
    }

    /// The first row whose top edges disagree between the panes, if any.
    /// This is the alignment check; the negative control feeds it a
    /// layout that applies identical pixel offsets to unaligned documents.
    fn first_misaligned(tops: &[(usize, f32, f32)]) -> Option<usize> {
        tops.iter()
            .find(|(_, a, b)| (a - b).abs() > 0.01)
            .map(|(row, _, _)| *row)
    }

    /// Every laid-out row in `rows` with both panes' top edges.
    fn laid_out_tops(harness: &View, rows: std::ops::Range<usize>) -> Vec<(usize, f32, f32)> {
        rows.filter_map(|row| {
            let a = cell(harness, 'a', row)?;
            let b = cell(harness, 'b', row)?;
            Some((row, a.top(), b.top()))
        })
        .collect()
    }

    fn copied(harness: &View) -> Vec<String> {
        harness
            .output()
            .platform_output
            .commands
            .iter()
            .filter_map(|command| match command {
                OutputCommand::CopyText(text) => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// Sends the platform's Copy event (what egui-winit turns Ctrl+C into)
    /// and returns the one text copied.
    fn copy(harness: &mut View) -> String {
        harness.event(Event::Copy);
        harness.step();
        let mut texts = copied(harness);
        assert_eq!(texts.len(), 1, "exactly one copy per Copy event");
        texts.remove(0)
    }

    fn select_all(harness: &mut View) {
        harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
        harness.step();
    }

    /// A small count as units; every count here is a row or column of a
    /// generated pair.
    fn units(n: usize) -> f32 {
        f32::from(u16::try_from(n).expect("a small count"))
    }

    fn char_width(harness: &View) -> f32 {
        harness
            .ctx
            .fonts_mut(|fonts| fonts.glyph_width(&CompactStyle::Document.font_id(), 'x'))
    }

    /// The point inside `column` (zero-based character boundary) on the
    /// first visual line of `row` in pane `side`.
    fn point(harness: &View, side: char, row: usize, column: usize) -> Pos2 {
        let rect = cell(harness, side, row).expect("row laid out");
        let x = rect.left() + (units(column) + 0.3) * char_width(harness);
        Pos2::new(x, rect.top() + ROW / 2.0)
    }

    fn button(harness: &mut View, pos: Pos2, pressed: bool, modifiers: Modifiers) {
        harness.event(Event::ModifiersChanged(modifiers));
        harness.event(Event::PointerMoved(pos));
        harness.event(Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers,
        });
        harness.event(Event::ModifiersChanged(Modifiers::NONE));
        harness.step();
    }

    fn click(harness: &mut View, pos: Pos2, modifiers: Modifiers) {
        button(harness, pos, true, modifiers);
        button(harness, pos, false, modifiers);
    }

    fn drag(harness: &mut View, from: Pos2, to: Pos2) {
        button(harness, from, true, Modifiers::NONE);
        for step in 1..=4u8 {
            let t = f32::from(step) / 4.0;
            harness.event(Event::PointerMoved(from + (to - from) * t));
            harness.step();
        }
        button(harness, to, false, Modifiers::NONE);
    }

    /// Byte offset of (zero-based line, character column) in `text`, split
    /// on LF with any CR kept as part of the terminator.
    fn offset(text: &str, line: usize, column: usize) -> usize {
        let start: usize = text.split_inclusive('\n').take(line).map(str::len).sum();
        let body = text[start..].split_inclusive('\n').next().unwrap_or("");
        start
            + body
                .char_indices()
                .nth(column)
                .map_or(body.trim_end_matches(['\r', '\n']).len(), |(i, _)| i)
    }

    fn line_len(text: &str, line: usize) -> usize {
        text.split_inclusive('\n')
            .nth(line)
            .map_or(0, |l| l.trim_end_matches(['\r', '\n']).chars().count())
    }

    // ── PV-T14-001: alignment, numbers, counts, navigation ───────────────

    #[test]
    fn diff_view_paired_rows_share_top_edges_with_wrap_off_and_on() {
        let pair = mixed_pair("\n");
        let expected = expected_rows(&pair.script);
        let mut harness = view(load(&pair.a, &pair.b, CompareMode::Original));

        for wrap in [false, true] {
            harness.state_mut().compare.wrap = wrap;
            harness.run();
            let tops = laid_out_tops(&harness, 0..expected.len());
            assert_eq!(
                tops.len(),
                expected.len(),
                "every row laid out (wrap {wrap})"
            );
            assert_eq!(first_misaligned(&tops), None, "rows aligned (wrap {wrap})");
            for (row, (a_no, b_no)) in expected.iter().enumerate() {
                let (a_rect, a_label) = node_label(&harness, 'a', row).expect("A row");
                let (b_rect, b_label) = node_label(&harness, 'b', row).expect("B row");
                assert_eq!(a_label, a_no.map(|n| n.to_string()), "A number, row {row}");
                assert_eq!(b_label, b_no.map(|n| n.to_string()), "B number, row {row}");
                assert!(
                    (a_rect.height() - b_rect.height()).abs() < 0.01,
                    "paired heights equal, row {row}"
                );
                if !wrap {
                    assert!((a_rect.height() - ROW).abs() < 0.01, "one line, row {row}");
                }
            }
            // Rows 6 (long B), 12 (long B insertion, A spacer) and 14 (long
            // A) are the taller rows once wrapped; both sides take that height.
            for row in [6, 12, 14] {
                let height = cell(&harness, 'a', row).expect("row").height();
                if wrap {
                    assert!(height >= 2.0 * ROW, "row {row} wraps to the taller side");
                } else {
                    assert!((height - ROW).abs() < 0.01, "row {row} one line unwrapped");
                }
            }
        }

        // Negative control: each file laid out as its own document, with
        // the same pixel offset applied to both, must fail the check.
        let first_top = cell(&harness, 'a', 0).expect("row 0").top();
        let naive: Vec<(usize, f32, f32)> = expected
            .iter()
            .enumerate()
            .filter_map(|(row, (a, b))| {
                let (a, b) = ((*a)?, (*b)?);
                let top = |line: usize| first_top + units(line - 1) * ROW;
                Some((row, top(a), top(b)))
            })
            .collect();
        assert_eq!(
            first_misaligned(&naive),
            Some(4),
            "identical pixel offsets misalign right after the insertion"
        );

        // Read-only: nothing in the widget is a text input.
        assert!(harness.query_by_role(Role::TextInput).is_none());
        assert!(harness.query_by_role(Role::MultilineTextInput).is_none());
    }

    #[test]
    fn diff_view_only_in_counts_equal_the_model_and_the_script() {
        let pair = mixed_pair("\r\n");
        let (unique_a, unique_b) = script_unique(&pair.script);
        for (mode, suffix) in [
            (CompareMode::Original, ""),
            (CompareMode::Normalized, " (normalized)"),
        ] {
            let state = load(&pair.a, &pair.b, mode);
            let aligned = state.comparison.aligned().expect("ready");
            assert_eq!(aligned.only_in_a.count(), unique_a);
            assert_eq!(aligned.only_in_b.count(), unique_b);
            let harness = view(state);
            harness.get_by_label(&format!("Only in A: {unique_a} lines{suffix}"));
            harness.get_by_label(&format!("Only in B: {unique_b} lines{suffix}"));
        }
    }

    /// Per hunk of a large pair's script: its first aligned row and the
    /// displayed numbers there.
    fn hunk_starts(script: &[EditOp]) -> Vec<(usize, Option<usize>, Option<usize>)> {
        let (mut row, mut a, mut b) = (0, 0, 0);
        let mut starts = Vec::new();
        for op in script {
            let (n, da, db) = match *op {
                EditOp::Keep(n) | EditOp::Replace(n) => (n, n, n),
                EditOp::Delete(n) => (n, n, 0),
                EditOp::Insert(n) => (n, 0, n),
            };
            if !matches!(op, EditOp::Keep(_)) {
                starts.push((row, (da > 0).then_some(a + 1), (db > 0).then_some(b + 1)));
            }
            row += n;
            a += da;
            b += db;
        }
        starts
    }

    #[test]
    fn diff_view_change_navigation_lands_both_panes_on_the_same_hunk() {
        let large: LargePair = texts::large_pair(DEFAULT_SEED);
        let starts = hunk_starts(&large.script);
        let rows: usize = large
            .script
            .iter()
            .map(|op| match *op {
                EditOp::Keep(n) | EditOp::Replace(n) | EditOp::Delete(n) | EditOp::Insert(n) => n,
            })
            .sum();
        let state = load(&large.a, &large.b, CompareMode::Original);
        assert_eq!(
            state.comparison.aligned().expect("ready").hunks.len(),
            starts.len()
        );
        let mut harness = view(state);
        let viewport = harness.ctx.content_rect();

        let land = |harness: &mut View, change: usize| {
            harness.state_mut().compare.active_change = change;
            harness.run();
            let (row, a_no, b_no) = starts[change];
            let (a_rect, a_label) = node_label(harness, 'a', row).expect("A hunk row laid out");
            let (b_rect, b_label) = node_label(harness, 'b', row).expect("B hunk row laid out");
            assert!(
                (a_rect.top() - b_rect.top()).abs() < 0.01,
                "change {change} aligned"
            );
            assert!(viewport.contains_rect(a_rect), "change {change} visible");
            assert_eq!(
                a_label,
                a_no.map(|n| n.to_string()),
                "change {change} A number"
            );
            assert_eq!(
                b_label,
                b_no.map(|n| n.to_string()),
                "change {change} B number"
            );
            // Only the visible rows plus overscan are laid out.
            let window: Vec<usize> = (row.saturating_sub(60)..(row + 60).min(rows))
                .filter(|r| cell(harness, 'a', *r).is_some())
                .collect();
            assert!(
                window.len() < 60,
                "virtualized: {} rows laid out",
                window.len()
            );
            let tops = laid_out_tops(harness, row.saturating_sub(60)..(row + 60).min(rows));
            assert_eq!(first_misaligned(&tops), None, "scrolled rows aligned");
        };

        for change in [1, 2, 700, 699, 1] {
            land(&mut harness, change);
        }

        // Unlinked, the panes scroll apart; navigation still lands both.
        harness.state_mut().compare.linked_scroll = false;
        harness.run();
        let (row, _, _) = starts[1];
        let before = cell(&harness, 'b', row).expect("B row");
        let at = point(&harness, 'a', row, 0);
        click(&mut harness, at, Modifiers::NONE);
        harness.key_press(Key::PageDown);
        harness.run();
        assert!(cell(&harness, 'a', row).is_none(), "A scrolled away");
        assert_eq!(cell(&harness, 'b', row), Some(before), "B did not move");
        land(&mut harness, 3);
    }

    // ── PV-T14-002: selection and copy ───────────────────────────────────

    #[test]
    fn diff_view_select_all_copies_the_whole_5_mb_buffer() {
        let large = texts::large_pair(DEFAULT_SEED);
        assert!(large.a.len() >= 4_900_000, "a 5 MB fixture");
        let mut harness = view(load(&large.a, &large.b, CompareMode::Original));
        select_all(&mut harness);
        assert!(
            copy(&mut harness).as_bytes() == large.a.as_bytes(),
            "A copied exactly"
        );
        harness.key_press(Key::F6);
        harness.step();
        select_all(&mut harness);
        assert!(
            copy(&mut harness).as_bytes() == large.b.as_bytes(),
            "B copied exactly"
        );
    }

    #[test]
    fn diff_view_select_all_keeps_crlf_and_skips_spacers() {
        let pair = mixed_pair("\r\n");
        let mut harness = view(load(&pair.a, &pair.b, CompareMode::Original));
        select_all(&mut harness);
        let text = copy(&mut harness);
        assert!(text.contains("\r\n"));
        assert_eq!(
            text, pair.a,
            "original bytes, no numbers, markers or spacers"
        );
    }

    #[test]
    fn diff_view_drag_shift_click_and_shift_arrow_copy_exactly_their_span() {
        let pair = mixed_pair("\r\n");
        let mut harness = view(load(&pair.a, &pair.b, CompareMode::Original));

        // Rows 1 to 7 in A cross the two spacer rows of the B insertion.
        // Row 1 is A line 1, row 7 is A line 5.
        let (from, to) = (point(&harness, 'a', 1, 2), point(&harness, 'a', 7, 4));
        let span = &pair.a[offset(&pair.a, 1, 2)..offset(&pair.a, 5, 4)];
        drag(&mut harness, from, to);
        assert_eq!(copy(&mut harness), span, "drag");

        let at = point(&harness, 'a', 0, 0);
        click(&mut harness, at, Modifiers::NONE);
        click(&mut harness, from, Modifiers::NONE);
        click(&mut harness, to, Modifiers::SHIFT);
        assert_eq!(copy(&mut harness), span, "Shift+click");

        // Shift+Right three times, then Shift+Down: from row 0 column 1
        // to row 1 column 4.
        let at = point(&harness, 'a', 0, 1);
        click(&mut harness, at, Modifiers::NONE);
        for _ in 0..3 {
            harness.key_press_modifiers(Modifiers::SHIFT, Key::ArrowRight);
        }
        harness.key_press_modifiers(Modifiers::SHIFT, Key::ArrowDown);
        harness.step();
        let span = &pair.a[offset(&pair.a, 0, 1)..offset(&pair.a, 1, 4)];
        assert_eq!(copy(&mut harness), span, "Shift+arrow");

        // Shift+Down from A line 1 (row 1) skips the spacers at rows 2 and
        // 3 and lands on A line 2 (row 4).
        let at = point(&harness, 'a', 1, 1);
        click(&mut harness, at, Modifiers::NONE);
        harness.key_press_modifiers(Modifiers::SHIFT, Key::ArrowDown);
        harness.step();
        let span = &pair.a[offset(&pair.a, 1, 1)..offset(&pair.a, 2, 1)];
        assert_eq!(copy(&mut harness), span, "Shift+Down over spacers");

        // The same in pane B: rows 4 to 7 are B lines 4 to 7.
        let (from, to) = (point(&harness, 'b', 4, 0), point(&harness, 'b', 7, 3));
        drag(&mut harness, from, to);
        let span = &pair.b[offset(&pair.b, 4, 0)..offset(&pair.b, 7, 3)];
        assert_eq!(copy(&mut harness), span, "drag in B");
    }

    #[test]
    fn diff_view_drag_into_the_other_pane_does_not_extend_the_selection() {
        let pair = mixed_pair("\n");
        let mut harness = view(load(&pair.a, &pair.b, CompareMode::Original));
        let before_b = harness.state().compare.selection[1];
        assert_eq!(before_b, PaneSelection::default());

        // From A row 4 (A line 2) into pane B at row 10 (A line 8).
        let (from, to) = (point(&harness, 'a', 4, 2), point(&harness, 'b', 10, 3));
        drag(&mut harness, from, to);
        let selection = harness.state().compare.selection;
        assert_eq!(selection[1], before_b, "pane B untouched");
        assert_eq!(
            selection[0].anchor,
            Some(TextPosition { row: 4, column: 2 })
        );
        let cursor = selection[0].cursor.expect("A cursor");
        assert_eq!(cursor.row, 10, "the pointer's row, clamped into pane A");
        let end = offset(&pair.a, 8, line_len(&pair.a, 8));
        let span = &pair.a[offset(&pair.a, 2, 2)..end];
        let text = copy(&mut harness);
        assert_eq!(text, span, "copy stays within pane A");
        assert!(!text.contains("only in B"));
    }

    #[test]
    fn diff_view_normalized_copy_gives_normalized_text_and_says_so() {
        let pair = mixed_pair("\r\n");
        let state = load(&pair.a, &pair.b, CompareMode::Normalized);
        let normalized = state
            .comparison
            .aligned()
            .and_then(|c| c.a.normalized.as_ref())
            .map(|text| text.text.to_string())
            .expect("normalized text");
        assert_ne!(normalized, pair.a, "the scenario ignores line endings");
        let mut harness = view(state);
        assert_eq!(
            harness
                .query_all_by_label(diff_view::NORMALIZED_NOTICE)
                .count(),
            2,
            "both panes say the view copies normalized text"
        );
        select_all(&mut harness);
        assert_eq!(copy(&mut harness), normalized);
        harness.run();
        harness.get_by_label("Copied normalized text from A");

        let mut original = view(load(&pair.a, &pair.b, CompareMode::Original));
        select_all(&mut original);
        assert_eq!(copy(&mut original), pair.a);
        original.run();
        assert!(original.query_by_label_contains("normalized").is_none());
    }
}
