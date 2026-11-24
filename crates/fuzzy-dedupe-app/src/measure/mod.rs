// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! The measurement mode (plan T23, REQ-034; C-10 samples, C-12 hook): a
//! release build started with [`MODE_VAR`] set drives one scripted spec
//! §11.9 scenario (T1 to T4) and prints its C-10 sample stream,
//! `scenario,run,metric,value_ms`, to standard output (plan D-010), then
//! quits. Without the variable the mode is off and the app is unchanged:
//! [`configured`] returns [`NoMeasurement`] and [`startup`] the design
//! scenario.
//!
//! Activation: `FUZZY_DEDUPE_MEASURE=<scenario>[,run=<n>][,secs=<n>]`, for
//! example `FUZZY_DEDUPE_MEASURE=t2,run=3`. The scenario is `t1` to `t4`;
//! `run` (default 1) is the run number the stream records; `secs`
//! (default 60, the C-10 minimum) is the run's length. An unparseable value
//! leaves the mode off and says why on standard error. An environment
//! variable rather than a command-line flag: the binary parses no
//! arguments, and the variable is read here, inside the measure seam,
//! without touching `main.rs` or `app.rs`.
//!
//! What `app.rs` gives the hook, and when (all outside the UI pass):
//!
//! - [`FrameHook::raw_input`] from eframe's `App::raw_input_hook`, before
//!   each pass: drivers inject scripted input here, through the same egui
//!   raw input real input uses;
//! - [`FrameHook::on_events`] once per frame from `App::logic`, after the
//!   event drain: which comparison and scan events it held;
//! - [`FrameHook::on_frame`] next, with what the frame ingested and
//!   eframe's CPU time for the previous frame;
//! - [`FrameHook::take_intents`] right after `on_frame`: the hook's intents
//!   join the frame's queue and are dispatched after its UI pass.
//!
//! All must return promptly and never block the UI thread.
//!
//! Cross-file contract with `app.rs`: `run_logic` reports
//! [`FrameHook::on_events`] (only events `events::apply` installed), then
//! [`FrameHook::on_frame`], then queues [`FrameHook::take_intents`] for
//! dispatch after the coming UI pass; `FuzzyDedupeApp::new` adopts
//! [`startup`] through [`StartupScenario::build`]. A change to the order or
//! to which events count changes what the samples mean. While
//! [`FrameHook::isolates_preferences`] holds, `FuzzyDedupeApp::with_startup`
//! starts on the default preferences and `write_preferences` writes none,
//! so a run neither depends on nor changes the user's `preferences.toml`.

pub mod drivers;
pub mod recorder;

use std::io::Write;
use std::sync::OnceLock;
use std::time::Duration;

use egui::{Rect, Vec2, ViewportId};
use fuzzy_dedupe_domain::ScanId;
use fuzzy_dedupe_engine::compare::texts;
use fuzzy_dedupe_engine::fixtures::{self, Scenario, ScenarioKind};
use fuzzy_dedupe_engine::service::NotImplemented;

use crate::presentation::commands::UiIntent;
use drivers::Driver;
use recorder::{MeasureClock, MonotonicClock, Recorder};

/// The variable that turns the measurement mode on.
pub const MODE_VAR: &str = "FUZZY_DEDUPE_MEASURE";

/// A run's length unless `secs` says otherwise: C-10's 60 s minimum.
pub const DEFAULT_RUN_LENGTH: Duration = Duration::from_mins(1);

/// How a scan ended, as drained from its final event.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ScanEnd {
    Completed,
    /// The workers acknowledged a cancellation (`ScanEvent::Partial`).
    Partial,
}

/// What one frame did, as `app.rs` reports it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FrameReport {
    /// Engine events drained this frame (the bounded batch's actual size).
    pub events_drained: usize,
    /// Intents dispatched after the previous UI pass.
    pub intents_dispatched: usize,
    /// eframe's `cpu_usage` for the previous frame, in seconds; `None`
    /// before the first painted frame and in headless tests.
    pub cpu_usage: Option<f32>,
}

/// Which of this frame's drained engine events the measurement needs to
/// see, as `app.rs` reports them through [`FrameHook::on_events`]. Only
/// events for the current scan or request count; a stale one discarded by
/// `events::apply` is not reported. Every field defaults to "nothing
/// happened", so a frame that reports nothing is truthful.
///
/// A separate struct rather than more [`FrameReport`] fields: `app.rs`
/// builds its report with a struct literal, and a new field would break
/// that build before the app is wired to fill it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameEvents {
    /// A comparison result for the current request was installed, so this
    /// frame's pass draws it (`EngineEvent::Comparison`).
    pub comparison_ready: bool,
    /// `ScanEvent::Started` for the current scan.
    pub scan_started: Option<ScanId>,
    /// The current scan's final event: `Completed` or `Partial`.
    pub scan_ended: Option<ScanEnd>,
}

impl FrameEvents {
    /// Adds `other`, one more event of the same frame, to `self`.
    pub fn merge(&mut self, other: Self) {
        self.comparison_ready |= other.comparison_ready;
        self.scan_started = other.scan_started.or(self.scan_started);
        self.scan_ended = other.scan_ended.or(self.scan_ended);
    }
}

/// Receives the frame events above.
pub trait FrameHook {
    /// May edit the raw input of the coming pass. Default: leaves it.
    fn raw_input(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        let _ = (ctx, raw);
    }

    /// This frame's drained events of interest. Called after the drain and
    /// before [`FrameHook::on_frame`] of the same frame. Default: ignores.
    fn on_events(&mut self, events: FrameEvents) {
        let _ = events;
    }

    fn on_frame(&mut self, report: FrameReport);

    /// Intents for the app to dispatch this frame. Default: none.
    fn take_intents(&mut self) -> Vec<UiIntent> {
        Vec::new()
    }

    /// Whether the app must leave `preferences.toml` alone: neither load it
    /// at startup (it starts on `PreferencesState::default()`) nor write it,
    /// throttled or on exit. Read by `FuzzyDedupeApp::with_startup` and
    /// `write_preferences` in `app.rs`. Default: `false`, the normal app.
    fn isolates_preferences(&self) -> bool {
        false
    }
}

/// The hook when measurement is off: does nothing.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoMeasurement;

impl FrameHook for NoMeasurement {
    fn on_frame(&mut self, report: FrameReport) {
        let _ = report;
    }
}

/// The spec §11.9 scenarios.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MeasureScenario {
    /// Findings over 100,000 rows: continuous scroll and key repeat.
    T1,
    /// Compare on the 5 MB pair: scroll, wrap on and off, select and copy.
    T2,
    /// Every popup opened and closed over both workspaces.
    T3,
    /// Input and cancellation while a simulated scan saturates the workers.
    T4,
}

impl MeasureScenario {
    /// The stream's `scenario` column.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::T1 => "T1",
            Self::T2 => "T2",
            Self::T3 => "T3",
            Self::T4 => "T4",
        }
    }

    /// The data the app starts on for this scenario.
    #[must_use]
    pub const fn startup(self) -> StartupScenario {
        match self {
            Self::T1 => StartupScenario::Fixture(ScenarioKind::Large),
            Self::T2 => StartupScenario::LargePair,
            Self::T3 | Self::T4 => StartupScenario::Fixture(ScenarioKind::Design),
        }
    }
}

/// One measurement run's settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MeasureConfig {
    pub scenario: MeasureScenario,
    /// At least 1.
    pub run: u32,
    pub length: Duration,
}

/// Why a [`MODE_VAR`] value was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModeError {
    NotUnicode,
    UnknownScenario(String),
    BadOption(String),
}

impl std::fmt::Display for ModeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotUnicode => write!(f, "{MODE_VAR} is not valid Unicode"),
            Self::UnknownScenario(s) => write!(f, "{MODE_VAR}: unknown scenario {s:?} (t1 to t4)"),
            Self::BadOption(s) => write!(
                f,
                "{MODE_VAR}: bad option {s:?} (run=<n> or secs=<n>, n at least 1)"
            ),
        }
    }
}

impl std::error::Error for ModeError {}

/// Parses a [`MODE_VAR`] value. `None` and an empty or all-blank value mean
/// the mode is off. Pure, so tests never touch the process environment.
///
/// # Errors
///
/// [`ModeError`] for an unknown scenario or a malformed option.
pub fn parse_mode(value: Option<&str>) -> Result<Option<MeasureConfig>, ModeError> {
    let Some(value) = value.map(str::trim).filter(|v| !v.is_empty()) else {
        return Ok(None);
    };
    let mut parts = value.split(',').map(str::trim);
    let name = parts.next().unwrap_or_default();
    let scenario = match name.to_ascii_lowercase().as_str() {
        "t1" => MeasureScenario::T1,
        "t2" => MeasureScenario::T2,
        "t3" => MeasureScenario::T3,
        "t4" => MeasureScenario::T4,
        _ => return Err(ModeError::UnknownScenario(name.to_owned())),
    };
    let mut config = MeasureConfig {
        scenario,
        run: 1,
        length: DEFAULT_RUN_LENGTH,
    };
    for part in parts {
        let bad = || ModeError::BadOption(part.to_owned());
        let (key, number) = part.split_once('=').ok_or_else(bad)?;
        let number: u32 = number
            .trim()
            .parse()
            .ok()
            .filter(|n| *n >= 1)
            .ok_or_else(bad)?;
        match key.trim() {
            "run" => config.run = number,
            "secs" => config.length = Duration::from_secs(u64::from(number)),
            _ => return Err(bad()),
        }
    }
    Ok(Some(config))
}

/// The process's mode, read and parsed once: [`configured`] and [`startup`]
/// must agree, and a refused value is reported once, not per call.
fn mode() -> Option<MeasureConfig> {
    static MODE: OnceLock<Option<MeasureConfig>> = OnceLock::new();
    *MODE.get_or_init(|| {
        let value = match std::env::var(MODE_VAR) {
            Ok(value) => Some(value),
            Err(std::env::VarError::NotPresent) => None,
            Err(std::env::VarError::NotUnicode(_)) => {
                eprintln!("fuzzy-dedupe: measurement off: {}", ModeError::NotUnicode);
                return None;
            }
        };
        parse_mode(value.as_deref()).unwrap_or_else(|err| {
            eprintln!("fuzzy-dedupe: measurement off: {err}");
            None
        })
    })
}

/// The scenario data the app starts on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StartupScenario {
    /// A C-03 fixture scenario, as `fixtures::build` makes it.
    Fixture(ScenarioKind),
    /// The T2 5 MB pair, `compare::texts::large_pair_scenario`, which
    /// `fixtures::build` never returns.
    LargePair,
}

impl StartupScenario {
    /// The kind a simulated scan from New scan replays on this data. The
    /// large pair reports `Large`, the kind `large_pair_scenario` itself
    /// carries for want of a dedicated variant.
    #[must_use]
    pub const fn kind(self) -> ScenarioKind {
        match self {
            Self::Fixture(kind) => kind,
            Self::LargePair => ScenarioKind::Large,
        }
    }

    /// Builds the scenario for `scan`, which the caller has reserved.
    ///
    /// # Errors
    ///
    /// [`NotImplemented`] where `fixtures::build` reports it.
    pub fn build(self, scan: ScanId, seed: u64) -> Result<Scenario, NotImplemented> {
        match self {
            Self::Fixture(kind) => fixtures::build(kind, scan, seed),
            Self::LargePair => Ok(texts::large_pair_scenario(scan, seed)),
        }
    }
}

/// The startup data for `mode`: the scenario's own when measuring,
/// otherwise the design scenario.
#[must_use]
pub fn startup_for(mode: Option<&MeasureConfig>) -> StartupScenario {
    mode.map_or(StartupScenario::Fixture(ScenarioKind::Design), |m| {
        m.scenario.startup()
    })
}

/// The data the app starts on in this process.
#[must_use]
pub fn startup() -> StartupScenario {
    startup_for(mode().as_ref())
}

/// The scenario kind New scan replays: [`startup`]'s kind. The app adopts
/// [`startup`] itself, which for T2 is the large pair, not this kind.
#[must_use]
pub fn startup_scenario() -> ScenarioKind {
    startup().kind()
}

/// Whether measurement mode is configured for this process. Exactly when
/// it is, [`configured`] returns the measurement hook, whose
/// [`FrameHook::isolates_preferences`] is `true`; `app::run` reads this
/// before the window exists, when no hook has been built yet.
#[must_use]
pub fn active() -> bool {
    mode().is_some()
}

/// The production hook: the measurement mode's recorder on standard output
/// when that mode is enabled, otherwise [`NoMeasurement`].
#[must_use]
pub fn configured() -> Box<dyn FrameHook> {
    hook_for(
        mode(),
        Box::new(MonotonicClock::start()),
        Box::new(std::io::stdout()),
    )
}

/// The hook for `mode`, timing with `clock` and writing to `sink`. `None`
/// gives [`NoMeasurement`], which never touches `sink`.
#[must_use]
pub fn hook_for(
    mode: Option<MeasureConfig>,
    clock: Box<dyn MeasureClock>,
    sink: Box<dyn Write>,
) -> Box<dyn FrameHook> {
    match mode {
        Some(mode) => Box::new(Measurement::new(mode, clock, sink)),
        None => Box::new(NoMeasurement),
    }
}

/// The measurement hook: one driver and one recorder on one clock.
pub struct Measurement {
    clock: Box<dyn MeasureClock>,
    recorder: Recorder,
    driver: Driver,
    /// Reported by `on_events`, consumed by the same frame's `on_frame`.
    events: FrameEvents,
}

impl Measurement {
    #[must_use]
    pub fn new(mode: MeasureConfig, clock: Box<dyn MeasureClock>, sink: Box<dyn Write>) -> Self {
        let recorder = Recorder::new(mode.scenario.label(), mode.run, sink, clock.now());
        Self {
            clock,
            recorder,
            driver: Driver::new(mode.scenario, mode.length),
            events: FrameEvents::default(),
        }
    }
}

/// The content rectangle before the window reports one: the §11.3
/// reference client area.
const FALLBACK_SCREEN: Vec2 = Vec2::new(1600.0, 960.0);

impl FrameHook for Measurement {
    fn raw_input(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        let now = self.clock.now();
        // Read outside any pass, so name the viewport: the root is the only
        // one this app draws.
        self.recorder
            .frame_start(now, ctx.cumulative_frame_nr_for(ViewportId::ROOT));
        if self.driver.finished() {
            return;
        }
        let screen = raw
            .screen_rect
            .unwrap_or_else(|| Rect::from_min_size(egui::Pos2::ZERO, FALLBACK_SCREEN));
        for injection in self.driver.inject(now, screen) {
            raw.events.extend(injection.events);
            self.recorder.input(injection.at);
        }
        // §11.9 measures CPU time "under continuous repaint": without this
        // an idle window paints nothing and the stream thins to the frames
        // the script's own input caused.
        ctx.request_repaint();
    }

    fn on_events(&mut self, events: FrameEvents) {
        self.events = events;
    }

    fn on_frame(&mut self, report: FrameReport) {
        let now = self.clock.now();
        let events = std::mem::take(&mut self.events);
        self.recorder.report(now, &report, &events);
        self.driver.observe(now, &events);
    }

    fn take_intents(&mut self) -> Vec<UiIntent> {
        let now = self.clock.now();
        let (intents, cancels) = self.driver.take_intents(now);
        if cancels {
            self.recorder.cancel_requested(now);
        }
        if self.driver.finished() {
            // Quit is next; eframe may exit before dropping the hook.
            self.recorder.flush();
        }
        intents
    }

    /// A run must not rewrite the owner's file (T2's Alt+Z toggles would,
    /// through the throttled write), and must not start from whatever the
    /// owner last saved, or two runs of one scenario would differ in wrap,
    /// zoom or split before the script has done anything.
    fn isolates_preferences(&self) -> bool {
        true
    }
}
