// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! PV-T23-001 (plan T23): the measurement mode's C-10 stream, its
//! painted-frame rule, delay visibility and inertness, proven headless.
//!
//! Every test sits in the `measure` module so `-E 'test(measure)'` selects
//! them. The hook is driven the way `app.rs` drives it, around a real egui
//! pass in an `egui_kittest` harness: `raw_input` before the pass, then
//! `on_frame` and `take_intents`. A small app double answers the hook's
//! intents with the reports the engine's events would produce (a
//! comparison three frames after Compare opens, Started one frame after
//! `StartScan`, Partial five frames after `CancelScan`). Time is a fake clock
//! the test advances, so a delayed frame costs no real sleep.
//!
//! The `end_to_end` module runs each script on the real app and engine
//! instead: the hook is the app's own `AppDeps::measure`, raw input goes
//! through eframe's `App::raw_input_hook`, and frames run `run_logic` then
//! `draw` as eframe does. It proves what the doubles cannot: that each
//! script reaches the widgets it targets, that T2 opens the 5 MB pair and
//! samples `first_paint`, and that T4's cancellation yields `cancel_ack`.

mod measure {
    use std::cell::{Cell, RefCell};
    use std::collections::BTreeSet;
    use std::io::Write;
    use std::rc::Rc;
    use std::time::Duration;

    use egui::Vec2;
    use egui_kittest::Harness;
    use fuzzy_dedupe_app::measure::recorder::{HEADER, MeasureClock, Metric};
    use fuzzy_dedupe_app::measure::{
        FrameEvents, FrameHook, FrameReport, MODE_VAR, MeasureConfig, MeasureScenario, ModeError,
        NoMeasurement, ScanEnd, StartupScenario, hook_for, parse_mode, startup_for,
    };
    use fuzzy_dedupe_app::presentation::commands::UiIntent;
    use fuzzy_dedupe_app::ui::theme;
    use fuzzy_dedupe_domain::ScanId;
    use fuzzy_dedupe_engine::fixtures::ScenarioKind;
    use fuzzy_dedupe_engine::service::EngineRequest;

    /// Step bound for every `Harness::run` here, in place of kittest's
    /// default of 4. Each harness test file under `tests/` declares its own
    /// copy, because they share no module; keep the copies equal (PR #40).
    const SETTLE_STEPS: u64 = 16;

    /// The simulated frame interval: 10 ms keeps a 12 s run at 1,200
    /// frames, and every unloaded sample is exactly one interval.
    const FRAME: Duration = Duration::from_millis(10);

    // ── Doubles ──────────────────────────────────────────────────────────

    #[derive(Clone, Default)]
    struct FakeClock(Rc<Cell<Duration>>);

    impl MeasureClock for FakeClock {
        fn now(&self) -> Duration {
            self.0.get()
        }
    }

    impl FakeClock {
        fn advance(&self, by: Duration) {
            self.0.set(self.0.get() + by);
        }
    }

    #[derive(Clone, Default)]
    struct Sink(Rc<RefCell<Vec<u8>>>);

    impl Write for Sink {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.borrow_mut().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Sink {
        fn text(&self) -> String {
            String::from_utf8(self.0.borrow().clone()).expect("UTF-8 stream")
        }
    }

    /// What the app double reports, `frames` frames from now.
    enum Answer {
        Comparison,
        Started(ScanId),
        Ended(ScanEnd),
    }

    struct Sim {
        harness: Harness<'static, bool>,
        hook: Box<dyn FrameHook>,
        clock: FakeClock,
        sink: Sink,
        frame_index: u64,
        answers: Vec<(u64, Answer)>,
        intents: Vec<UiIntent>,
        /// The `cpu_usage` eframe holds: the last painted frame's, kept
        /// through logic-only ticks.
        cpu_usage: Option<f32>,
        painted: u32,
    }

    impl Sim {
        fn new(mode: Option<MeasureConfig>) -> Self {
            let clock = FakeClock::default();
            let sink = Sink::default();
            // Text waits for the app's fonts: egui's bundled ones are
            // compiled out (`default_fonts` off), and the builder runs a
            // first pass before `theme::install` can reach its context.
            let mut harness = Harness::builder()
                .with_max_steps(SETTLE_STEPS)
                .with_size(Vec2::new(1600.0, 960.0))
                .build_ui_state(
                    |ui, fonts_ready: &mut bool| {
                        if *fonts_ready {
                            ui.label("measured");
                        }
                    },
                    false,
                );
            theme::install(&harness.ctx, Vec::new());
            *harness.state_mut() = true;
            let hook = hook_for(mode, Box::new(clock.clone()), Box::new(sink.clone()));
            Self {
                harness,
                hook,
                clock,
                sink,
                frame_index: 0,
                answers: Vec::new(),
                intents: Vec::new(),
                cpu_usage: None,
                painted: 0,
            }
        }

        fn measuring(scenario: MeasureScenario, secs: u64) -> Self {
            Self::new(Some(MeasureConfig {
                scenario,
                run: 2,
                length: Duration::from_secs(secs),
            }))
        }

        /// One frame as eframe runs it: the hook's raw input, a full pass
        /// (`painted`) or a logic-only tick, the report, the intents. The
        /// frame then lasts `FRAME + delay`.
        fn frame(&mut self, painted: bool, delay: Duration) {
            let ctx = self.harness.ctx.clone();
            self.hook.raw_input(&ctx, self.harness.input_mut());
            if painted {
                self.harness.step();
            } else {
                // eframe's `update_logic_only`: no pass; the input waits
                // for the next one.
                let raw = self.harness.input_mut().clone();
                let _output = ctx.run_logic(&raw, |_| {});
            }
            let mut events = FrameEvents::default();
            let now = self.frame_index;
            self.answers.retain(|(due, answer)| {
                if *due != now {
                    return true;
                }
                match answer {
                    Answer::Comparison => events.comparison_ready = true,
                    Answer::Started(scan) => events.scan_started = Some(*scan),
                    Answer::Ended(end) => events.scan_ended = Some(*end),
                }
                false
            });
            self.hook.on_events(events);
            self.hook.on_frame(FrameReport {
                cpu_usage: self.cpu_usage,
                ..FrameReport::default()
            });
            for intent in self.hook.take_intents() {
                let answer = match &intent {
                    UiIntent::OpenCompare { .. } => Some((3, Answer::Comparison)),
                    UiIntent::Submit(EngineRequest::StartScan(_)) => {
                        Some((1, Answer::Started(ScanId::new(7))))
                    }
                    UiIntent::Submit(EngineRequest::CancelScan(_)) => {
                        Some((5, Answer::Ended(ScanEnd::Partial)))
                    }
                    _ => None,
                };
                if let Some((after, answer)) = answer {
                    self.answers.push((now + after, answer));
                }
                self.intents.push(intent);
            }
            if painted {
                // Distinct per painted frame, so a repeated sample shows.
                self.painted += 1;
                self.cpu_usage = Some(0.001 * f32::from(u16::try_from(self.painted).unwrap()));
            }
            self.frame_index += 1;
            self.clock.advance(FRAME + delay);
        }

        /// The whole stream: drops the hook first, so the recorder's final
        /// flush (on drop) has run and nothing sits in its buffer.
        fn stream(&mut self) -> String {
            self.hook = Box::new(NoMeasurement);
            self.sink.text()
        }

        fn frames(&mut self, count: u64) {
            for _ in 0..count {
                self.frame(true, Duration::ZERO);
            }
        }
    }

    /// One parsed stream line.
    #[derive(Debug)]
    struct Sample {
        scenario: String,
        run: u32,
        metric: String,
        value_ms: f64,
    }

    /// Parses `stream` strictly as C-10: the header, then lines of exactly
    /// four fields, a known metric, and a non-negative millisecond value
    /// with three decimals.
    fn parse(stream: &str) -> Vec<Sample> {
        let mut lines = stream.lines();
        assert_eq!(lines.next(), Some(HEADER), "header first");
        assert!(stream.ends_with('\n'), "every line terminated");
        let metrics: Vec<&str> = Metric::ALL.iter().map(|m| m.name()).collect();
        lines
            .map(|line| {
                let fields: Vec<&str> = line.split(',').collect();
                assert_eq!(fields.len(), 4, "four fields: {line:?}");
                assert!(metrics.contains(&fields[2]), "known metric: {line:?}");
                let (whole, decimals) = fields[3].split_once('.').expect("decimal point");
                assert!(
                    !whole.is_empty()
                        && whole.bytes().all(|b| b.is_ascii_digit())
                        && decimals.len() == 3
                        && decimals.bytes().all(|b| b.is_ascii_digit()),
                    "value_ms as milliseconds with three decimals: {line:?}"
                );
                Sample {
                    scenario: fields[0].to_owned(),
                    run: fields[1].parse().expect("run number"),
                    metric: fields[2].to_owned(),
                    value_ms: fields[3].parse().expect("value"),
                }
            })
            .collect()
    }

    fn values(samples: &[Sample], metric: Metric) -> Vec<f64> {
        samples
            .iter()
            .filter(|s| s.metric == metric.name())
            .map(|s| s.value_ms)
            .collect()
    }

    // ── Activation ───────────────────────────────────────────────────────

    #[test]
    fn measure_mode_parses_scenario_run_and_length() {
        assert_eq!(MODE_VAR, "FUZZY_DEDUPE_MEASURE");
        assert_eq!(
            parse_mode(Some("t2")),
            Ok(Some(MeasureConfig {
                scenario: MeasureScenario::T2,
                run: 1,
                length: Duration::from_mins(1),
            }))
        );
        assert_eq!(
            parse_mode(Some(" T4, run=3 ,secs=90 ")),
            Ok(Some(MeasureConfig {
                scenario: MeasureScenario::T4,
                run: 3,
                length: Duration::from_secs(90),
            }))
        );
        assert_eq!(
            parse_mode(Some("t5")),
            Err(ModeError::UnknownScenario("t5".to_owned()))
        );
        for bad in ["t1,run=0", "t1,run", "t1,secs=x", "t1,speed=2"] {
            assert!(
                matches!(parse_mode(Some(bad)), Err(ModeError::BadOption(_))),
                "{bad}"
            );
        }
        assert_eq!(
            startup_for(Some(&parse_mode(Some("t1")).unwrap().unwrap())),
            StartupScenario::Fixture(ScenarioKind::Large)
        );
        assert_eq!(
            startup_for(Some(&parse_mode(Some("t2")).unwrap().unwrap())),
            StartupScenario::LargePair
        );
    }

    #[test]
    fn measure_mode_is_inert_when_not_enabled() {
        assert_eq!(parse_mode(None), Ok(None));
        assert_eq!(parse_mode(Some("  ")), Ok(None));
        assert_eq!(
            startup_for(None),
            StartupScenario::Fixture(ScenarioKind::Design)
        );
        assert_eq!(startup_for(None).kind(), ScenarioKind::Design);

        let mut sim = Sim::new(None);
        for _ in 0..200 {
            let ctx = sim.harness.ctx.clone();
            // `RawInput` has no `PartialEq`; its `Debug` form shows every
            // field, events included.
            let before = format!("{:?}", sim.harness.input_mut());
            sim.hook.raw_input(&ctx, sim.harness.input_mut());
            let after = format!("{:?}", sim.harness.input_mut());
            assert_eq!(after, before, "raw input untouched");
            sim.harness.step();
            sim.hook.on_events(FrameEvents {
                comparison_ready: true,
                scan_started: Some(ScanId::new(1)),
                scan_ended: Some(ScanEnd::Partial),
            });
            sim.hook.on_frame(FrameReport {
                cpu_usage: Some(0.004),
                ..FrameReport::default()
            });
            assert!(sim.hook.take_intents().is_empty(), "no intents");
            sim.clock.advance(FRAME);
        }
        assert_eq!(sim.sink.text(), "", "nothing written, not even a header");
        // No repaint is requested, so the UI settles within the bound.
        sim.harness.run();
    }

    // ── PV-T23-001: format per scenario ──────────────────────────────────

    #[test]
    fn measure_each_scenario_emits_c10_samples() {
        let expected: [(MeasureScenario, &[Metric]); 4] = [
            (
                MeasureScenario::T1,
                &[Metric::CpuUsage, Metric::InputToPaint],
            ),
            (
                MeasureScenario::T2,
                &[Metric::CpuUsage, Metric::FirstPaint, Metric::InputToPaint],
            ),
            (
                MeasureScenario::T3,
                &[Metric::CpuUsage, Metric::InputToPaint],
            ),
            (
                MeasureScenario::T4,
                &[Metric::CpuUsage, Metric::InputToPaint, Metric::CancelAck],
            ),
        ];
        for (scenario, metrics) in expected {
            let mut sim = Sim::measuring(scenario, 12);
            // 12 s of 10 ms frames, then a second more: the script quits at
            // 12 s and must stay quiet after.
            sim.frames(1_300);
            let samples = parse(&sim.stream());
            let label = scenario.label();
            assert!(
                samples.iter().all(|s| s.scenario == label && s.run == 2),
                "{label}: scenario and run columns"
            );
            let seen: BTreeSet<&str> = samples.iter().map(|s| s.metric.as_str()).collect();
            let wanted: BTreeSet<&str> = metrics.iter().map(|m| m.name()).collect();
            assert_eq!(seen, wanted, "{label}: metrics");
            // Inputs fall due between frame starts, so each waits for the
            // next frame and is painted one frame later: between one and two
            // frame intervals.
            for value in values(&samples, Metric::InputToPaint) {
                assert!((10.0..20.0).contains(&value), "{label}: {value}");
            }
            assert_eq!(
                sim.intents.iter().filter(|i| **i == UiIntent::Quit).count(),
                1,
                "{label}: quits once at the end"
            );
            assert_eq!(sim.intents.last(), Some(&UiIntent::Quit), "{label}");
            match scenario {
                MeasureScenario::T2 => {
                    // Drained at one frame start, painted by the next.
                    assert_eq!(values(&samples, Metric::FirstPaint), [10.0]);
                }
                MeasureScenario::T4 => {
                    let acks = values(&samples, Metric::CancelAck);
                    // A scan cycle: 2 s running, the ack 50 ms later, then
                    // 1 s idle; 12 s holds at least three.
                    assert!(acks.len() >= 3, "T4: {} acknowledgments", acks.len());
                    assert!(acks.iter().all(|v| (v - 50.0).abs() < 1e-9), "{acks:?}");
                }
                _ => {}
            }
        }
    }

    // ── PV-T23-001: painted frames only ──────────────────────────────────

    /// Logic-only ticks (a minimized or occluded window) leave eframe's
    /// `cpu_usage` at the last painted frame's value. A recorder that
    /// sampled every report would repeat it once per tick.
    #[test]
    fn measure_cpu_usage_counts_painted_frames_only() {
        let mut sim = Sim::measuring(MeasureScenario::T1, 600);
        let mut painted = 0;
        for i in 0..300 {
            // Two logic-only ticks after every third painted frame.
            let paint = i % 5 < 3;
            sim.frame(paint, Duration::ZERO);
            painted += u32::from(paint);
        }
        // One more frame start closes the last painted frame.
        sim.frame(true, Duration::ZERO);
        let samples = parse(&sim.stream());
        let cpu = values(&samples, Metric::CpuUsage);
        assert_eq!(
            cpu.len(),
            usize::try_from(painted).unwrap(),
            "one sample per painted frame"
        );
        let distinct: BTreeSet<String> = cpu.iter().map(|v| format!("{v:.3}")).collect();
        assert_eq!(distinct.len(), cpu.len(), "no stale value sampled twice");
    }

    // ── PV-T23-001: a delayed frame is visible ───────────────────────────

    #[test]
    fn measure_delayed_frame_shows_as_larger_input_to_paint() {
        // T4 injects a key every 100 ms from 0.5 s on.
        let mut sim = Sim::measuring(MeasureScenario::T4, 600);
        sim.frames(110);
        // The frame starting at 1.1 s takes its key and then 150 ms more.
        sim.frame(true, Duration::from_millis(150));
        sim.frames(50);
        let samples = parse(&sim.stream());
        let latency = values(&samples, Metric::InputToPaint);
        let slow: Vec<f64> = latency.iter().copied().filter(|v| *v > 10.0).collect();
        // The delayed frame's own key, then the key that fell due during
        // the delay (1.2 s, injected at 1.26 s, painted at 1.27 s).
        assert_eq!(slow, [160.0, 70.0], "all samples: {latency:?}");
        assert!(latency.len() >= 10, "{latency:?}");
    }

    // ── End to end, on the real app ──────────────────────────────────────

    mod end_to_end {
        use std::collections::{BTreeSet, HashSet};
        use std::path::Path;
        use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

        use egui::{OutputCommand, Vec2};
        use egui_kittest::Harness;
        use fuzzy_dedupe_app::app::{AppDeps, Clock, FuzzyDedupeApp};
        use fuzzy_dedupe_app::measure::drivers::{self, PopupOpener};
        use fuzzy_dedupe_app::measure::recorder::{MeasureClock, Metric, MonotonicClock};
        use fuzzy_dedupe_app::measure::{MeasureConfig, MeasureScenario, hook_for};
        use fuzzy_dedupe_app::presentation::commands::{self, UiIntent};
        use fuzzy_dedupe_app::presentation::preferences;
        use fuzzy_dedupe_app::presentation::state::{
            DialogKind, DialogSubject, MenuKind, PreferencesState, PresentationState, ScanStage,
            WorkspaceState,
        };
        use fuzzy_dedupe_app::ui::theme::Zoom;
        use fuzzy_dedupe_domain::{CompareMode, QueueScope, RelationKind};
        use fuzzy_dedupe_engine::compare::texts;
        use fuzzy_dedupe_engine::fixtures::DEFAULT_SEED;
        use fuzzy_dedupe_engine::service::EngineConfig;
        use fuzzy_dedupe_platform::app_files::AppFiles;
        use fuzzy_dedupe_platform::dirs::AppDirs;
        use fuzzy_dedupe_platform::fonts::SystemFontLookup;
        use fuzzy_dedupe_platform::log_retention::LogDate;
        use fuzzy_dedupe_platform::open::{Location, OpenHandle, Opener};
        use fuzzy_dedupe_platform::picker::{FolderPicker, PickHandle, PickOutcome, PickRequest};

        use super::{FRAME, FakeClock, SETTLE_STEPS, Sink, parse, values};

        /// Real engine work (the 5 MB diff, the simulated scan) runs on
        /// other threads in wall time; a debug build diffs 5 MB slowly.
        const ENGINE_DEADLINE: Duration = Duration::from_mins(2);

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

        struct FixedDate;

        impl Clock for FixedDate {
            fn today(&self) -> LogDate {
                LogDate::new(2026, 11, 23).expect("valid date")
            }
        }

        fn dirs_for(name: &str) -> AppDirs {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock after 1970")
                .as_nanos();
            let root = Path::new(env!("CARGO_TARGET_TMPDIR"))
                .join("measure")
                .join(format!("{name}-{nanos}"));
            let state = root.join("state");
            AppDirs::resolve(
                move |var| (var == "XDG_STATE_HOME").then(|| state.clone().into_os_string()),
                &root.join("home"),
            )
        }

        struct Run {
            harness: Harness<'static, Option<FuzzyDedupeApp>>,
            sink: Sink,
        }

        impl Run {
            /// The app as `FuzzyDedupeApp::new` builds it in a process
            /// started with `scenario`'s mode: that scenario's startup data
            /// and its measurement hook, on `clock`.
            fn launch(scenario: MeasureScenario, clock: Box<dyn MeasureClock>) -> Self {
                Self::launch_in(scenario, clock, dirs_for(scenario.label()))
            }

            /// [`Run::launch`] on `dirs`, for tests that place files there.
            fn launch_in(
                scenario: MeasureScenario,
                clock: Box<dyn MeasureClock>,
                dirs: AppDirs,
            ) -> Self {
                let mode = MeasureConfig {
                    scenario,
                    run: 1,
                    length: Duration::from_mins(10),
                };
                let sink = Sink::default();
                let deps = AppDeps {
                    dirs,
                    opener: Box::new(NoOpener),
                    picker: Box::new(DismissPicker),
                    fonts: Box::new(SystemFontLookup),
                    clock: Box::new(FixedDate),
                    log_expiry: Box::new(|_| {}),
                    engine: EngineConfig::default(),
                    measure: hook_for(Some(mode), clock, Box::new(sink.clone())),
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
                let app = FuzzyDedupeApp::with_startup(&harness.ctx, deps, scenario.startup())
                    .expect("engine starts");
                *harness.state_mut() = Some(app);
                Self { harness, sink }
            }

            fn state(&self) -> &PresentationState {
                &self.harness.state().as_ref().expect("app").state
            }

            /// One frame as eframe runs it: the app's raw-input hook (the
            /// measurement hook's injection), then the pass. Never
            /// `Harness::run`: the hook asks for continuous repaint.
            fn frame(&mut self) {
                let ctx = self.harness.ctx.clone();
                let mut raw = std::mem::take(self.harness.input_mut());
                let app = self.harness.state_mut().as_mut().expect("app");
                eframe::App::raw_input_hook(app, &ctx, &mut raw);
                *self.harness.input_mut() = raw;
                self.harness.step();
            }

            fn copies(&self) -> Vec<usize> {
                self.harness
                    .output()
                    .platform_output
                    .commands
                    .iter()
                    .filter_map(|c| match c {
                        OutputCommand::CopyText(text) => Some(text.len()),
                        _ => None,
                    })
                    .collect()
            }

            /// The whole stream, after dropping the app and with it the
            /// recorder, whose drop flushes.
            fn stream(&mut self) -> String {
                *self.harness.state_mut() = None;
                self.sink.text()
            }
        }

        /// Runs fake-clock frames until script time `until`.
        fn run_until(
            run: &mut Run,
            clock: &FakeClock,
            until: Duration,
            mut each: impl FnMut(&Run),
        ) {
            while clock.now() < until {
                run.frame();
                each(run);
                clock.advance(FRAME);
            }
        }

        #[test]
        fn measure_t1_script_focuses_a_row_scrolls_and_moves_the_focus() {
            let clock = FakeClock::default();
            let mut run = Run::launch(MeasureScenario::T1, Box::new(clock.clone()));
            // The click at 0.5 s focuses a row; the wheel phase runs to 4 s.
            run_until(&mut run, &clock, Duration::from_millis(3_900), |_| {});
            let focused = run.state().findings.focused;
            assert!(focused.is_some(), "the click focused a row");
            let scrolled = run.state().findings.scroll_offset;
            assert!(scrolled > 0.0, "the wheel scrolled the table: {scrolled}");
            // Key repeat (Arrow Down) from 4 s to 8 s.
            run_until(&mut run, &clock, Duration::from_millis(7_900), |_| {});
            assert_ne!(
                run.state().findings.focused,
                focused,
                "key repeat moved the focus"
            );
            let samples = parse(&run.stream());
            assert!(!values(&samples, Metric::InputToPaint).is_empty());
        }

        #[test]
        fn measure_t2_script_opens_the_5_mb_pair_toggles_wrap_and_copies_a_pane() {
            let clock = FakeClock::default();
            let mut run = Run::launch(MeasureScenario::T2, Box::new(clock.clone()));
            // Script time stands still while the engine diffs, so the
            // first-paint frame is exactly one frame interval long.
            let deadline = Instant::now() + ENGINE_DEADLINE;
            loop {
                run.frame();
                let ready = run.state().compare.comparison.result.is_some();
                if ready || clock.now() < Duration::from_millis(600) {
                    clock.advance(FRAME);
                }
                if ready {
                    break;
                }
                assert!(Instant::now() < deadline, "the 5 MB comparison arrived");
                std::thread::sleep(Duration::from_millis(5));
            }
            assert_eq!(run.state().workspace, WorkspaceState::Compare);
            let pair = texts::large_pair(DEFAULT_SEED);
            let panes = [pair.a.len(), pair.b.len()];
            let mut wraps = Vec::new();
            let mut copies = Vec::new();
            run_until(&mut run, &clock, Duration::from_millis(8_500), |run| {
                let wrap = run.state().compare.wrap;
                if wraps.last() != Some(&wrap) {
                    wraps.push(wrap);
                }
                copies.extend(run.copies());
            });
            assert_eq!(wraps, [false, true, false], "Alt+Z at 4 s and at 8 s");
            assert_eq!(copies.len(), 1, "one copy at 4 s: {copies:?}");
            assert!(
                panes.contains(&copies[0]),
                "a whole pane copied: {copies:?}"
            );
            let samples = parse(&run.stream());
            assert_eq!(values(&samples, Metric::FirstPaint), [10.0]);
        }

        /// The intent-opened popups' subjects name the startup data as the
        /// app holds it, as the menus that open them would.
        fn assert_subjects_name_the_startup_data(
            state: &PresentationState,
            subjects: &[(DialogKind, DialogSubject)],
        ) {
            let results = state.scan.results.as_ref().expect("startup results");
            for (kind, subject) in subjects {
                match subject {
                    DialogSubject::UniqueText(subject) => {
                        assert_eq!(*kind, DialogKind::UniqueTextReview);
                        let group = results.group_of(subject.target).expect("target in a group");
                        let link = group
                            .member(subject.target)
                            .and_then(|m| m.link())
                            .expect("target is a member");
                        assert_eq!(link.counterpart, subject.counterpart);
                        assert!(link.unique_lines > 0, "a pair with unique text");
                        assert_eq!(
                            Some(subject.comparison(CompareMode::Original)),
                            commands::comparison_request(
                                state,
                                subject.target,
                                subject.counterpart,
                                CompareMode::Original,
                            ),
                            "scan, snapshots and rules are the app's own"
                        );
                    }
                    DialogSubject::QueueIdentical(QueueScope::Groups(groups)) => {
                        assert_eq!(*kind, DialogKind::QueueIdentical);
                        assert!(!groups.is_empty());
                        for id in groups {
                            let group = results.groups.iter().find(|g| g.id == *id);
                            assert!(
                                group.is_some_and(|g| g
                                    .row_kinds()
                                    .contains(&RelationKind::SameBytes)),
                                "a group with identical copies: {id:?}"
                            );
                        }
                    }
                    DialogSubject::QueueIdentical(QueueScope::AllByteIdenticalGroups) => {
                        assert_eq!(*kind, DialogKind::QueueIdentical);
                    }
                    DialogSubject::None => assert!(subject.fits(*kind), "{kind:?}"),
                }
            }
        }

        #[test]
        fn measure_t3_script_opens_and_closes_every_popup_on_both_workspaces() {
            // The script's list must be every popup the app has: the
            // exhaustive chain in `drivers` against the state's own list.
            let popups = drivers::t3_popups();
            assert_eq!(
                popups.iter().copied().collect::<HashSet<_>>(),
                DialogKind::ALL.into_iter().collect::<HashSet<_>>(),
                "T3 covers every DialogKind"
            );
            assert_eq!(popups.len(), DialogKind::ALL.len(), "each once");

            let clock = FakeClock::default();
            let mut run = Run::launch(MeasureScenario::T3, Box::new(clock.clone()));
            run.frame();
            clock.advance(FRAME);
            let queue_before = run.state().queue.clone();
            let results_before = run.state().scan.results.clone();
            let mut opened = BTreeSet::new();
            let mut subjects = Vec::new();
            let mut open_for = 0_u32;
            let mut longest = 0_u32;
            // Per workspace: its switch, an open and an Esc per popup, and
            // two F10 presses; 400 ms steps from 0.5 s, plus a margin that
            // ends before the script's second lap opens anything.
            let steps = 2 * (1 + 2 * popups.len() + 2);
            let lap = Duration::from_millis(500 + 400 * u64::try_from(steps).unwrap());
            run_until(&mut run, &clock, lap + Duration::from_millis(200), |run| {
                let state = run.state();
                let overlay = state
                    .dialog
                    .dialog()
                    .map(|d| format!("{:?}", d.kind))
                    .or_else(|| state.dialog.menu().map(|m| format!("{m:?} menu")));
                if let Some(dialog) = state.dialog.dialog()
                    && open_for == 0
                {
                    subjects.push((dialog.kind, dialog.subject.clone()));
                }
                if let Some(overlay) = overlay {
                    opened.insert((format!("{:?}", state.workspace), overlay));
                    open_for += 1;
                    longest = longest.max(open_for);
                } else {
                    open_for = 0;
                }
            });
            let mut expected = BTreeSet::new();
            for workspace in [WorkspaceState::Findings, WorkspaceState::Compare] {
                for kind in &popups {
                    expected.insert((format!("{workspace:?}"), format!("{kind:?}")));
                }
                expected.insert((
                    format!("{workspace:?}"),
                    format!("{:?} menu", MenuKind::Main),
                ));
            }
            assert_eq!(opened, expected, "each popup on each workspace");
            // One step is 40 frames: an overlay the next step failed to
            // close would stay open for 80 or more.
            assert!(
                longest <= 41,
                "every overlay closed by the next step: {longest}"
            );
            assert!(run.state().dialog.dialog().is_none() && run.state().dialog.menu().is_none());

            let state = run.state();
            assert_subjects_name_the_startup_data(state, &subjects);
            let intent_opened: HashSet<DialogKind> = subjects
                .iter()
                .filter(|(kind, _)| drivers::popup_opener(*kind) == PopupOpener::Intent)
                .map(|(kind, _)| *kind)
                .collect();
            assert_eq!(
                intent_opened,
                [
                    DialogKind::About,
                    DialogKind::QueueIdentical,
                    DialogKind::UniqueTextReview
                ]
                .into_iter()
                .collect(),
                "the popups without a chord opened through their intents"
            );
            // Opening and closing commits nothing: no queue entry, decision
            // or acknowledgment, and the same results.
            assert_eq!(state.queue, queue_before, "review state unchanged");
            assert_eq!(state.scan.results, results_before, "results unchanged");
        }

        /// A measurement run neither reads nor writes the user's
        /// `preferences.toml`: it starts on the defaults, so runs are
        /// reproducible, and T2's Alt+Z toggles leave the file alone.
        #[test]
        fn measure_preferences_isolates_a_run_from_the_saved_file() {
            let dirs = dirs_for("isolated");
            let saved = PreferencesState {
                compare_wrap: true,
                zoom: Zoom::from_percent(150).expect("on the zoom grid"),
                ..PreferencesState::default()
            };
            preferences::store(&AppFiles::new(&dirs), &dirs, &saved).expect("fixture written");
            let path = dirs.config_dir().join(preferences::FILE_NAME);
            let before = std::fs::read(&path).expect("fixture readable");

            let clock = FakeClock::default();
            let mut run = Run::launch_in(MeasureScenario::T2, Box::new(clock.clone()), dirs);
            assert_eq!(
                run.state().preferences,
                PreferencesState::default(),
                "starts on the defaults, not the saved wrap and zoom"
            );
            let deadline = Instant::now() + ENGINE_DEADLINE;
            loop {
                run.frame();
                let ready = run.state().compare.comparison.result.is_some();
                if ready || clock.now() < Duration::from_millis(600) {
                    clock.advance(FRAME);
                }
                if ready {
                    break;
                }
                assert!(Instant::now() < deadline, "the 5 MB comparison arrived");
                std::thread::sleep(Duration::from_millis(5));
            }
            let mut wraps = Vec::new();
            run_until(&mut run, &clock, Duration::from_millis(8_500), |run| {
                let wrap = run.state().preferences.compare_wrap;
                if wraps.last() != Some(&wrap) {
                    wraps.push(wrap);
                }
            });
            assert_eq!(wraps, [false, true, false], "the Alt+Z toggles ran");
            let app = run.harness.state_mut().as_mut().expect("app");
            eframe::App::on_exit(app);
            assert_eq!(
                std::fs::read(&path).expect("file still there"),
                before,
                "preferences.toml byte-identical"
            );
        }

        /// The window is built before any hook exists, so `app::run` picks
        /// its size through `launch_preferences`; a saved size must not
        /// reach a measurement run's window.
        #[test]
        fn measure_preferences_launch_window_ignores_the_saved_file() {
            use fuzzy_dedupe_app::app::{launch_preferences, native_options, native_options_for};
            use fuzzy_dedupe_app::presentation::state::WindowPreference;

            let dirs = dirs_for("launch-window");
            let window = WindowPreference {
                width: 1440.0,
                height: 870.0,
                maximized: false,
            };
            let saved = PreferencesState {
                window: Some(window),
                ..PreferencesState::default()
            };
            preferences::store(&AppFiles::new(&dirs), &dirs, &saved).expect("fixture written");

            assert_eq!(
                launch_preferences(&dirs, true),
                PreferencesState::default(),
                "measurement mode starts on the defaults"
            );
            assert_eq!(
                launch_preferences(&dirs, false).window,
                Some(window),
                "an ordinary launch restores the saved size"
            );
            assert_eq!(
                native_options_for(&launch_preferences(&dirs, true))
                    .viewport
                    .inner_size,
                native_options().viewport.inner_size,
                "the measurement window opens at the reference size"
            );
        }

        #[test]
        fn measure_preferences_run_without_a_file_creates_none() {
            let dirs = dirs_for("no-file");
            let path = dirs.config_dir().join(preferences::FILE_NAME);
            let clock = FakeClock::default();
            let mut run = Run::launch_in(MeasureScenario::T3, Box::new(clock.clone()), dirs);
            run_until(&mut run, &clock, Duration::from_millis(300), |_| {});
            // A preference change the app would otherwise write at once
            // (the first write is never throttled) and again on exit.
            let app = run.harness.state_mut().as_mut().expect("app");
            app.push_intent(UiIntent::ZoomIn);
            run_until(&mut run, &clock, Duration::from_millis(600), |_| {});
            assert_ne!(run.state().preferences, PreferencesState::default());
            let app = run.harness.state_mut().as_mut().expect("app");
            eframe::App::on_exit(app);
            assert!(!path.exists(), "no preferences.toml created");
        }

        #[test]
        fn measure_t4_script_starts_a_scan_and_its_cancel_yields_cancel_ack() {
            // Wall time: the scan's pace and its cancellation are real.
            let mut run = Run::launch(MeasureScenario::T4, Box::new(MonotonicClock::start()));
            let deadline = Instant::now() + ENGINE_DEADLINE;
            let mut stages = Vec::new();
            loop {
                run.frame();
                let stage = run.state().scan.stage;
                if stages.last() != Some(&stage) {
                    stages.push(stage);
                }
                if run.sink.text().contains(",cancel_ack,") {
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "no cancel_ack; stages {stages:?}"
                );
                std::thread::sleep(Duration::from_millis(2));
            }
            assert!(stages.contains(&ScanStage::Running), "{stages:?}");
            let samples = parse(&run.stream());
            let acks = values(&samples, Metric::CancelAck);
            assert_eq!(acks.len(), 1, "{acks:?}");
            assert!(!values(&samples, Metric::InputToPaint).is_empty());
        }
    }
}
