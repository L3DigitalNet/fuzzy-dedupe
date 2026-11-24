// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! The UI root (spec §11.8): window options, logging, engine startup,
//! bounded event ingestion and the per-frame order.
//!
//! Frame order. eframe calls [`FuzzyDedupeApp::run_logic`] (through
//! `App::logic`) before each UI pass; it never draws. It runs log expiry
//! when the injected clock's date changes, records a window resize into
//! the preferences, drains at most [`EVENT_BATCH`] engine events, collects
//! finished opens and folder picks, mirrors a newer validation view, writes
//! changed preferences (throttled) and reports to the measurement hook.
//! Then [`FuzzyDedupeApp::draw`] (through `App::ui`) restores focus,
//! applies the theme and zoom preferences, reads the global chords, draws
//! the shell and then the dialog host, and only after the whole UI pass
//! dispatches the frame's intents, plus the bounded follow-ups their
//! dispatch queues (plan D-008). Drawing therefore never calls the engine,
//! a platform adapter or the filesystem (spec §5.2).
//!
//! Logging (spec §18.5, NFR-009, plan D-015): daily files from
//! `tracing-appender` in the platform-resolved `logs/` directory, at info
//! unless `RUST_LOG` says otherwise. The roller has no file-count cap; age
//! expiry is the retention rule and runs only through the C-07
//! [`expire_logs`], at startup and at each change of date, outside the UI
//! pass. This crate holds no delete call.
//!
//! Cross-file contract with `fuzzy-dedupe-platform/src/log_retention.rs`:
//! the roller's prefix, suffix and daily UTC rotation must produce
//! [`log_file_name`] exactly, and expiry must be given the date in the same
//! UTC time base ([`LogDate::today_utc`]). Any other naming makes every log
//! "not ours" to the expiry, so logs would silently never expire;
//! `tests/shell.rs` pins the file name.

use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::{Context, ThemePreference, Vec2};
use fuzzy_dedupe_domain::QueueOrigin;
use fuzzy_dedupe_engine::fixtures::{self, ScenarioKind, SeededDecision};
use fuzzy_dedupe_engine::service::{Engine, EngineConfig, ReviewCommand, ReviewStatus};
use fuzzy_dedupe_platform::app_files::{AppFileError, AppFiles};
use fuzzy_dedupe_platform::dirs::AppDirs;
use fuzzy_dedupe_platform::fonts::{FontLookup, SystemFontLookup};
use fuzzy_dedupe_platform::log_retention::{
    LOG_FILE_PREFIX, LOG_FILE_SUFFIX, LogDate, expire_logs,
};
use fuzzy_dedupe_platform::open::{Location, OpenError, OpenHandle, Opener, SystemOpener};
use fuzzy_dedupe_platform::picker::{FolderPicker, PortalPicker};
use tracing_subscriber::filter::{LevelFilter, Targets};
use tracing_subscriber::layer::{Layer as _, SubscriberExt as _};

use crate::measure::{FrameHook, FrameReport};
use crate::presentation::commands::{self, IntentQueue, Services};
use crate::presentation::events;
use crate::presentation::preferences;
use crate::presentation::state::{
    PreferencesState, PresentationState, ScanStage, ThemeOverride, TimeOfDay, WindowPreference,
};
use crate::ui::theme::{self, Zoom};
use crate::ui::widgets::dialog_host;
use crate::ui::{glyphs, shell};

/// The native window title (spec §11.3); the app draws no title bar itself.
pub const APP_TITLE: &str = "Fuzzy DeDupe";
/// The window cannot shrink below this (spec §11.3, frames 01d and 02b).
pub const MIN_WINDOW_SIZE: Vec2 = Vec2::new(1024.0, 600.0);
/// The reference client area, the size of a first start (spec §11.3).
pub const DEFAULT_WINDOW_SIZE: Vec2 = Vec2::new(1600.0, 960.0);
/// Engine events taken per frame at most. A burst larger than this spreads
/// over later frames instead of stalling one (spec §11.9).
pub const EVENT_BATCH: usize = 256;

/// The native window: title, default size and minimum size.
#[must_use]
pub fn viewport() -> egui::ViewportBuilder {
    egui::ViewportBuilder::default()
        .with_title(APP_TITLE)
        .with_inner_size(DEFAULT_WINDOW_SIZE)
        .with_min_inner_size(MIN_WINDOW_SIZE)
}

/// [`viewport`] with the remembered size and maximized state, never below
/// the minimum (DR-009; no position is remembered).
#[must_use]
pub fn viewport_for(preferences: &PreferencesState) -> egui::ViewportBuilder {
    match preferences.window {
        Some(window) => viewport()
            .with_inner_size(Vec2::new(window.width, window.height).max(MIN_WINDOW_SIZE))
            .with_maximized(window.maximized),
        None => viewport(),
    }
}

/// eframe's options: [`viewport`] with eframe's own persistence left off,
/// because `preferences.toml` is the only persisted state (DR-009).
#[must_use]
pub fn native_options() -> eframe::NativeOptions {
    native_options_for(&PreferencesState::default())
}

/// [`native_options`] restoring the window from `preferences`.
#[must_use]
pub fn native_options_for(preferences: &PreferencesState) -> eframe::NativeOptions {
    eframe::NativeOptions {
        viewport: viewport_for(preferences),
        persist_window: false,
        ..Default::default()
    }
}

/// The current date in the log roller's time base. Injected so tests can
/// move the date across a rollover.
pub trait Clock {
    fn today(&self) -> LogDate;
}

/// Today's UTC date, matching `tracing-appender`'s daily file names.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn today(&self) -> LogDate {
        LogDate::today_utc()
    }
}

/// One log-expiry pass for the given current date.
pub type LogExpiry = Box<dyn FnMut(LogDate)>;

/// Expiry through the C-07 [`expire_logs`]. Never panics: an
/// [`fuzzy_dedupe_platform::log_retention::ExpiryError`] or a file that
/// could not be deleted is logged and retried at the next pass.
#[must_use]
pub fn platform_log_expiry(dirs: AppDirs) -> LogExpiry {
    Box::new(move |today| match expire_logs(&dirs, today) {
        Ok(report) => {
            for (name, err) in &report.failed {
                tracing::warn!(file = %name, %err, "expired log not deleted");
            }
            if !report.deleted.is_empty() {
                tracing::info!(deleted = ?report.deleted, "expired logs deleted");
            }
        }
        Err(err) => tracing::warn!(%err, "log expiry skipped"),
    })
}

/// Why logging could not be set up.
#[derive(Debug)]
pub enum LogSetupError {
    /// The log directory could not be created through the app-owned writer.
    Directory(AppFileError),
    /// The roller could not open its file.
    Roller(tracing_appender::rolling::InitError),
}

impl std::fmt::Display for LogSetupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Directory(err) => write!(f, "log directory: {err}"),
            Self::Roller(err) => write!(f, "log file: {err}"),
        }
    }
}

impl std::error::Error for LogSetupError {}

/// The level filter for a `RUST_LOG` value: info when unset, empty or
/// unparseable (the last also reported in the returned flag), otherwise the
/// value in `tracing-subscriber`'s target-directive syntax, such as `debug`
/// or `fuzzy_dedupe_engine=trace`.
fn level_filter(rust_log: Option<&str>) -> (Targets, bool) {
    let default = Targets::new().with_default(LevelFilter::INFO);
    match rust_log.map(str::trim).filter(|v| !v.is_empty()) {
        None => (default, true),
        Some(value) => match value.parse::<Targets>() {
            Ok(targets) => (targets, true),
            Err(_) => (default, false),
        },
    }
}

/// A configured log subscriber and what owns its writer.
pub struct LogSetup<S> {
    pub subscriber: S,
    /// Keep alive for the program's life: dropping it flushes and stops the
    /// writer thread.
    pub guard: tracing_appender::non_blocking::WorkerGuard,
    /// The `RUST_LOG` value when it could not be parsed and info applies
    /// instead; the caller logs it once the subscriber is installed.
    pub ignored_rust_log: Option<String>,
}

/// A subscriber writing daily files into `dirs`' log directory, filtered by
/// `rust_log` (the `RUST_LOG` value, passed in rather than read so tests
/// never touch the process environment).
///
/// # Errors
///
/// [`LogSetupError`] when the directory or the file cannot be opened.
pub fn log_subscriber(
    dirs: &AppDirs,
    rust_log: Option<&str>,
) -> Result<LogSetup<impl tracing::Subscriber + Send + Sync + 'static>, LogSetupError> {
    AppFiles::new(dirs)
        .ensure_dir(dirs.log_dir())
        .map_err(LogSetupError::Directory)?;
    // No `max_log_files`: a count cap would delete by count, not age, and
    // intermittent use makes a few daily files span far more than 7 days
    // (plan D-015). Expiry by age is `expire_logs`' job alone.
    let roller = tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix(LOG_FILE_PREFIX)
        .filename_suffix(LOG_FILE_SUFFIX)
        .build(dirs.log_dir())
        .map_err(LogSetupError::Roller)?;
    let (writer, guard) = tracing_appender::non_blocking(roller);
    let (filter, parsed) = level_filter(rust_log);
    let layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_writer(writer);
    Ok(LogSetup {
        subscriber: tracing_subscriber::registry().with(layer.with_filter(filter)),
        guard,
        ignored_rust_log: (!parsed).then(|| rust_log.unwrap_or_default().to_owned()),
    })
}

/// An opener for when the system opener's worker could not start: every
/// open answers at once with the start-up failure, shown as status.
struct UnavailableOpener(String);

impl Opener for UnavailableOpener {
    fn open(&self, _location: Location, notify: Box<dyn FnOnce() + Send>) -> OpenHandle {
        notify();
        OpenHandle::ready(Err(OpenError::WorkerUnavailable(std::io::Error::other(
            self.0.clone(),
        ))))
    }
}

/// Everything the app is built from. Tests substitute each adapter.
pub struct AppDeps {
    pub dirs: AppDirs,
    pub opener: Box<dyn Opener>,
    pub picker: Box<dyn FolderPicker>,
    pub fonts: Box<dyn FontLookup>,
    pub clock: Box<dyn Clock>,
    pub log_expiry: LogExpiry,
    pub engine: EngineConfig,
    pub measure: Box<dyn FrameHook>,
}

impl AppDeps {
    /// The production adapters for `dirs`.
    #[must_use]
    pub fn system(dirs: AppDirs) -> Self {
        let opener: Box<dyn Opener> = match SystemOpener::start() {
            Ok(opener) => Box::new(opener),
            Err(err) => {
                tracing::warn!(%err, "opener unavailable");
                Box::new(UnavailableOpener(err.to_string()))
            }
        };
        Self {
            log_expiry: platform_log_expiry(dirs.clone()),
            dirs,
            opener,
            picker: Box::new(PortalPicker),
            fonts: Box::new(SystemFontLookup),
            clock: Box::new(SystemClock),
            engine: EngineConfig::default(),
            measure: crate::measure::configured(),
        }
    }
}

/// The application: presentation state plus the services it reaches.
pub struct FuzzyDedupeApp {
    pub state: PresentationState,
    services: Services,
    intents: IntentQueue,
    clock: Box<dyn Clock>,
    log_expiry: LogExpiry,
    /// The date expiry last ran for; a different date is a rollover.
    expiry_date: LogDate,
    measure: Box<dyn FrameHook>,
    intents_last_frame: usize,
    /// eframe's CPU time for the previous frame, for the measurement hook.
    cpu_usage: Option<f32>,
    /// The preferences as last written (or loaded); a difference is a
    /// change to write.
    saved_preferences: PreferencesState,
    last_preferences_write: Option<Instant>,
    /// The window size and maximized state egui last reported.
    observed_window: Option<(Vec2, bool)>,
}

/// Preference writes happen at most this often while a value keeps
/// changing (a window drag resizes every frame), and once more on exit.
pub const PREFERENCES_WRITE_INTERVAL: Duration = Duration::from_secs(1);

/// Rounds of follow-up intents dispatched per frame: an intent's dispatch
/// may queue more (a popup's `open` asking for its comparison), which may
/// in principle queue more; the bound stops a cycle from stalling a frame.
const FOLLOWUP_ROUNDS: usize = 4;

impl FuzzyDedupeApp {
    /// Installs the theme on `ctx`, starts the engine on the startup
    /// scenario (`measure::startup_scenario`, the design scenario unless a
    /// measurement run chose another), and runs the startup log expiry.
    ///
    /// # Errors
    ///
    /// The operating system's error when the engine thread cannot start.
    pub fn new(ctx: &Context, deps: AppDeps) -> std::io::Result<Self> {
        let AppDeps {
            dirs,
            opener,
            picker,
            fonts,
            clock,
            mut log_expiry,
            engine,
            measure,
        } = deps;

        let today = clock.today();
        log_expiry(today);

        let fallbacks = fonts
            .fallback_fonts()
            .into_iter()
            .map(|font| (font.family, egui::FontData::from_owned(font.data)))
            .collect();
        theme::install(ctx, fallbacks);
        // Before the first pass, so every icon draws on the first frame.
        glyphs::install(ctx);

        let mut state = PresentationState {
            preferences: preferences::load(&dirs),
            ..PresentationState::default()
        };
        let saved_preferences = state.preferences.clone();
        let mut services = Services::new(Engine::new(engine)?, opener, picker, dirs);
        adopt_scenario(
            &mut services,
            &mut state,
            crate::measure::startup_scenario(),
        );

        Ok(Self {
            state,
            services,
            intents: IntentQueue::default(),
            clock,
            log_expiry,
            expiry_date: today,
            measure,
            intents_last_frame: 0,
            cpu_usage: None,
            saved_preferences,
            last_preferences_write: None,
            observed_window: None,
        })
    }

    /// Work outside the UI pass; draws nothing (eframe `App::logic`).
    pub fn run_logic(&mut self, ctx: &Context) {
        let today = self.clock.today();
        if today != self.expiry_date {
            self.expiry_date = today;
            (self.log_expiry)(today);
        }

        record_window(ctx, &mut self.observed_window, &mut self.state);

        let batch = self.services.engine.drain_events(EVENT_BATCH);
        let drained = batch.len();
        for event in batch {
            events::apply(&mut self.state, event);
        }
        // After the drain: a completed scan's results were installed into
        // the review service by `drain_events`, and its new view is mirrored.
        self.services.poll(&mut self.state);
        if drained == EVENT_BATCH {
            // More may be waiting; take them on the next frame, not this one.
            ctx.request_repaint();
        }

        self.write_preferences(false);
        self.measure.on_frame(FrameReport {
            events_drained: drained,
            intents_dispatched: self.intents_last_frame,
            cpu_usage: self.cpu_usage,
        });
    }

    /// One UI pass, then the dispatch of its intents (eframe `App::ui`).
    pub fn draw(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        dialog_host::begin_frame(&ctx, &mut self.state.dialog);
        apply_preferences(&ctx, &self.state);

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
        self.intents_last_frame = intents.len();
        let mut rounds = 0;
        while !intents.is_empty() && rounds < FOLLOWUP_ROUNDS {
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
            rounds += 1;
            ctx.request_repaint();
        }
        if !intents.is_empty() {
            tracing::warn!(
                dropped = intents.len(),
                "follow-up intents beyond the bound"
            );
        }
    }

    /// Writes changed preferences through T20's `preferences::store`, at
    /// most once per [`PREFERENCES_WRITE_INTERVAL`] unless `force`. A failed
    /// write is logged and retried on the next change (C-08: a failure is
    /// never fatal).
    fn write_preferences(&mut self, force: bool) {
        if self.state.preferences == self.saved_preferences {
            return;
        }
        let due = self
            .last_preferences_write
            .is_none_or(|at| at.elapsed() >= PREFERENCES_WRITE_INTERVAL);
        if !force && !due {
            return;
        }
        self.last_preferences_write = Some(Instant::now());
        let files = AppFiles::new(&self.services.dirs);
        match preferences::store(&files, &self.services.dirs, &self.state.preferences) {
            Ok(()) => self.saved_preferences = self.state.preferences.clone(),
            Err(err) => tracing::warn!(%err, "preferences not written"),
        }
    }

    /// Queues `intent` as if a view had pushed it this frame; it is
    /// dispatched after the next UI pass. For drivers (T23) and tests.
    pub fn push_intent(&mut self, intent: crate::presentation::commands::UiIntent) {
        self.intents.push(intent);
    }

    /// The application services, for inspection.
    #[must_use]
    pub const fn services(&self) -> &Services {
        &self.services
    }
}

impl eframe::App for FuzzyDedupeApp {
    fn logic(&mut self, ctx: &Context, frame: &mut eframe::Frame) {
        self.cpu_usage = frame.info().cpu_usage;
        self.run_logic(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.draw(ui);
    }

    fn raw_input_hook(&mut self, ctx: &Context, raw_input: &mut egui::RawInput) {
        self.measure.raw_input(ctx, raw_input);
    }

    fn on_exit(&mut self) {
        self.write_preferences(true);
    }
}

/// Copies the window's size and maximized state into the preferences when
/// they change (DR-009; never its position). A minimized or not yet
/// reported window leaves them as they are.
///
/// The first report only seeds `observed`: the window opening at its
/// restored or default size is not a change, and C-08 forbids rewriting an
/// unreadable or newer `preferences.toml` until the user changes something.
fn record_window(
    ctx: &Context,
    observed: &mut Option<(Vec2, bool)>,
    state: &mut PresentationState,
) {
    let (size, maximized, minimized) = ctx.input(|i| {
        let viewport = i.viewport();
        (
            viewport.inner_rect.map(|r| r.size()),
            viewport.maximized,
            viewport.minimized,
        )
    });
    let (Some(size), Some(maximized)) = (size, maximized) else {
        return;
    };
    if minimized == Some(true) {
        return;
    }
    let previous = observed.replace((size, maximized));
    if previous.is_none_or(|seen| seen == (size, maximized)) {
        return;
    }
    let current = state.preferences.window;
    // A maximized window keeps the size it would restore to.
    let (width, height) = match current {
        Some(previous) if maximized => (previous.width, previous.height),
        _ => (size.x, size.y),
    };
    let next = WindowPreference {
        width,
        height,
        maximized,
    };
    if current != Some(next) {
        state.preferences.window = Some(next);
    }
}

/// Applies the theme override and zoom preferences, only when they differ
/// from what egui already holds. A system theme change needs no call here:
/// with [`ThemePreference::System`] egui reads it from each frame's input.
fn apply_preferences(ctx: &Context, state: &PresentationState) {
    let wanted = match state.preferences.theme {
        ThemeOverride::System => ThemePreference::System,
        ThemeOverride::Light => ThemePreference::Light,
        ThemeOverride::Dark => ThemePreference::Dark,
    };
    if ctx.options(|o| o.theme_preference) != wanted {
        ctx.set_theme(wanted);
    }
    let zoom: Zoom = state.preferences.zoom;
    if (ctx.zoom_factor() - zoom.factor()).abs() > f32::EPSILON {
        theme::apply_zoom(ctx, zoom);
    }
}

/// Makes `kind` the current scenario through the facade and replays its
/// seeded decisions through the review service, so the shell starts on the
/// review frames' state for the design scenario. Real scans start from New
/// scan (T16).
///
/// Public so tests can adopt a scenario other than the startup one through
/// this same path; the app itself calls it once, from [`FuzzyDedupeApp::new`].
pub fn adopt_scenario(services: &mut Services, state: &mut PresentationState, kind: ScenarioKind) {
    let engine = &mut services.engine;
    let scan = engine.reserve_scan_id();
    let scenario = match fixtures::build(kind, scan, fixtures::DEFAULT_SEED) {
        Ok(scenario) => Arc::new(scenario),
        Err(missing) => {
            tracing::warn!(?missing, ?kind, "scenario unavailable");
            return;
        }
    };
    if let Err(err) = engine.adopt_scenario(Arc::clone(&scenario)) {
        tracing::warn!(?err, "scenario refused");
        return;
    }
    events::install_results(state, Arc::clone(&scenario.results), ScanStage::Completed);
    // Group titles are design-scenario fixture data (spec §11.1, frame
    // `13:648`), installed here only, after `install_results` has cleared
    // any earlier ones; every other scenario and every scan shows each
    // header's reference name.
    if kind == ScenarioKind::Design {
        let sample = fixtures::design::sample();
        state.scan.files_scanned = sample.files_scanned;
        state.scan.finished_at = Some(TimeOfDay {
            hour: sample.finished_at.hour,
            minute: sample.finished_at.minute,
        });
        state.findings.expanded = sample.expanded_groups.iter().copied().collect();
        state.findings.group_titles = sample.group_titles;
    }
    services.sync_view(state);

    for seed in &scenario.seeded_decisions {
        for command in seed_commands(*seed) {
            let mut outcome = services.engine.review(command.clone());
            if let (
                ReviewCommand::RequestQueue { .. },
                ReviewStatus::NeedsReview { file, counterpart },
            ) = (&command, outcome.status)
            {
                let ack = ReviewCommand::AcknowledgeUniqueText {
                    file,
                    counterpart,
                    reviewed: None,
                };
                let acked = services.engine.review(ack);
                events::apply_review(state, &acked);
                outcome = services.engine.review(command);
            }
            if let ReviewStatus::Refused { file, refusal } = outcome.status {
                tracing::warn!(?file, ?refusal, "seeded decision refused");
            }
            events::apply_review(state, &outcome);
        }
    }
    services.sync_view(state);
    state.status = None;
    state.queue.last_refusal = None;
}

/// The review command a seed replays as (fixtures `SeededDecision`).
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

/// The binary's entry: logging, then the window. Logging that cannot start
/// is reported on stderr and the app runs without a log file rather than
/// not at all.
///
/// # Errors
///
/// eframe's error when the window or renderer cannot start.
pub fn run() -> eframe::Result {
    let dirs = AppDirs::from_system();
    // Held until `run` returns, so the log writer flushes on exit.
    let mut log_guard = None;
    if let Ok(dirs) = &dirs {
        let rust_log = std::env::var("RUST_LOG").ok();
        match log_subscriber(dirs, rust_log.as_deref()) {
            Ok(setup) => {
                if tracing::subscriber::set_global_default(setup.subscriber).is_ok() {
                    log_guard = Some(setup.guard);
                }
                if let Some(value) = setup.ignored_rust_log {
                    tracing::warn!(rust_log = %value, "RUST_LOG not understood; logging at info");
                }
            }
            Err(err) => eprintln!("fuzzy-dedupe: logging disabled: {err}"),
        }
    }
    let dirs = match dirs {
        Ok(dirs) => dirs,
        Err(err) => {
            eprintln!("fuzzy-dedupe: {err}");
            return Err(eframe::Error::AppCreation(Box::new(err)));
        }
    };
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "starting");
    let result = eframe::run_native(
        APP_TITLE,
        native_options_for(&preferences::load(&dirs)),
        Box::new(move |cc| {
            let app = FuzzyDedupeApp::new(&cc.egui_ctx, AppDeps::system(dirs))?;
            Ok(Box::new(app))
        }),
    );
    drop(log_guard);
    result
}
