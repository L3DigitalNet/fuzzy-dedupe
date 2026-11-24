// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! The C-10 sample recorder: turns the frame hook's calls into CSV lines
//! `scenario,run,metric,value_ms` on one sink, standard output in
//! production (plan D-010).
//!
//! What counts as painted. The recorder never sees a paint call. It reads
//! egui's completed-frame counter (`Context::cumulative_frame_nr_for`) at
//! each frame start: the counter advances once per full UI pass and not on
//! eframe's logic-only ticks (`update_logic_only`, run while the window is
//! minimized or occluded), which paint nothing. A frame start that sees the
//! counter moved therefore closes a painted frame, and only then:
//!
//! - the next report's `cpu_usage` is that painted frame's time and is
//!   sampled once. eframe leaves `frame.info().cpu_usage` at the last
//!   painted value through logic-only ticks, so sampling every report would
//!   repeat a stale value and inflate the count (the PV-T23-001 falsifier);
//! - every input injected before that pass, and the first paint of a
//!   comparison drained before it, resolve as `input_to_paint` and
//!   `first_paint`, measured to this frame start. The frame start follows
//!   the previous frame's present, so the sample includes the paint itself.
//!
//! Timing comes from an injected [`MeasureClock`], so tests can delay a
//! frame without sleeping.
//!
//! Failure policy: a write error (a closed pipe, a full disk behind the
//! redirect) is reported once on standard error and the recorder goes
//! quiet. It never panics, because a measurement run must not take the UI
//! thread down with it.

use std::io::{BufWriter, Write};
use std::time::{Duration, Instant};

use super::{FrameEvents, FrameReport, ScanEnd};

/// The first line of every stream, naming the C-10 columns.
pub const HEADER: &str = "scenario,run,metric,value_ms";

/// Buffered lines are flushed at least this often. Lines are written per
/// frame into a buffer and reach the sink in batches, so a frame pays for
/// a memory copy rather than a write system call.
pub const FLUSH_INTERVAL: Duration = Duration::from_millis(250);

/// Time since an arbitrary fixed origin. Only differences are used.
pub trait MeasureClock {
    fn now(&self) -> Duration;
}

/// The wall clock: monotonic time since the clock was made.
#[derive(Clone, Copy, Debug)]
pub struct MonotonicClock(Instant);

impl MonotonicClock {
    #[must_use]
    pub fn start() -> Self {
        Self(Instant::now())
    }
}

impl MeasureClock for MonotonicClock {
    fn now(&self) -> Duration {
        self.0.elapsed()
    }
}

/// The C-10 metrics.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Metric {
    /// eframe's CPU time for one painted frame.
    CpuUsage,
    /// A drained comparison to the end of the frame that painted it.
    FirstPaint,
    /// A scripted input's due time to the end of the frame that painted it.
    InputToPaint,
    /// A cancellation request to the drained partial-scan event.
    CancelAck,
}

impl Metric {
    pub const ALL: [Self; 4] = [
        Self::CpuUsage,
        Self::FirstPaint,
        Self::InputToPaint,
        Self::CancelAck,
    ];

    /// The name in the stream's `metric` column.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::CpuUsage => "cpu_usage",
            Self::FirstPaint => "first_paint",
            Self::InputToPaint => "input_to_paint",
            Self::CancelAck => "cancel_ack",
        }
    }
}

/// One stream line, without its line feed: milliseconds with three decimals
/// (microsecond resolution), so percentiles come from the raw values.
#[must_use]
pub fn format_sample(scenario: &str, run: u32, metric: Metric, value: Duration) -> String {
    format!(
        "{scenario},{run},{},{:.3}",
        metric.name(),
        value.as_secs_f64() * 1000.0
    )
}

/// The recorder for one run of one scenario.
pub struct Recorder {
    scenario: &'static str,
    run: u32,
    sink: Option<BufWriter<Box<dyn Write>>>,
    last_flush: Duration,
    /// The completed-frame counter at the previous frame start.
    frames_seen: Option<u64>,
    /// A painted frame closed and its `cpu_usage` is not sampled yet.
    cpu_pending: bool,
    /// Injected inputs not yet painted: due time and the counter value
    /// their pass will move past.
    inputs: Vec<(Duration, u64)>,
    /// A drained comparison not yet painted, as for `inputs`.
    first_paint: Option<(Duration, u64)>,
    cancel_at: Option<Duration>,
}

impl Recorder {
    /// Writes [`HEADER`] at once, so even a run that measures nothing
    /// leaves a recognizable, empty stream rather than no output.
    #[must_use]
    pub fn new(scenario: &'static str, run: u32, sink: Box<dyn Write>, now: Duration) -> Self {
        let mut recorder = Self {
            scenario,
            run,
            sink: Some(BufWriter::new(sink)),
            last_flush: now,
            frames_seen: None,
            cpu_pending: false,
            inputs: Vec::new(),
            first_paint: None,
            cancel_at: None,
        };
        recorder.line(HEADER);
        recorder
    }

    /// A frame is starting; `frames` is egui's completed-frame counter.
    pub fn frame_start(&mut self, now: Duration, frames: u64) {
        let painted = self.frames_seen.is_some_and(|seen| frames > seen);
        self.frames_seen = Some(frames);
        if !painted {
            return;
        }
        self.cpu_pending = true;
        let mut resolved = Vec::new();
        self.inputs.retain(|&(at, before)| {
            let done = frames > before;
            if done {
                resolved.push(at);
            }
            !done
        });
        for at in resolved {
            self.sample(Metric::InputToPaint, now.saturating_sub(at));
        }
        if let Some((at, before)) = self.first_paint
            && frames > before
        {
            self.first_paint = None;
            self.sample(Metric::FirstPaint, now.saturating_sub(at));
        }
        if now.saturating_sub(self.last_flush) >= FLUSH_INTERVAL {
            self.flush();
            self.last_flush = now;
        }
    }

    /// An input due at `at` was injected into the coming pass.
    pub fn input(&mut self, at: Duration) {
        if let Some(frames) = self.frames_seen {
            self.inputs.push((at, frames));
        }
    }

    /// The app's report and events for the frame that started last.
    pub fn report(&mut self, now: Duration, report: &FrameReport, events: &FrameEvents) {
        if self.cpu_pending
            && let Some(seconds) = report.cpu_usage
        {
            self.cpu_pending = false;
            self.sample(
                Metric::CpuUsage,
                Duration::try_from_secs_f32(seconds).unwrap_or_default(),
            );
        }
        // The frame that drained the comparison draws it in this same pass.
        if events.comparison_ready
            && let Some(frames) = self.frames_seen
        {
            self.first_paint = Some((now, frames));
        }
        if let Some(end) = events.scan_ended {
            if let (ScanEnd::Partial, Some(at)) = (end, self.cancel_at) {
                self.sample(Metric::CancelAck, now.saturating_sub(at));
            }
            self.cancel_at = None;
        }
    }

    /// A scan cancellation left the hook at `now`.
    pub fn cancel_requested(&mut self, now: Duration) {
        self.cancel_at = Some(now);
    }

    /// Pushes buffered lines to the sink.
    pub fn flush(&mut self) {
        if let Some(sink) = self.sink.as_mut()
            && let Err(err) = sink.flush()
        {
            self.fail(&err);
        }
    }

    fn sample(&mut self, metric: Metric, value: Duration) {
        let line = format_sample(self.scenario, self.run, metric, value);
        self.line(&line);
    }

    fn line(&mut self, line: &str) {
        if let Some(sink) = self.sink.as_mut()
            && let Err(err) = writeln!(sink, "{line}")
        {
            self.fail(&err);
        }
    }

    fn fail(&mut self, err: &std::io::Error) {
        // Dropping the writer would flush again into the same error; the
        // buffer is discarded with it, which loses nothing the sink could
        // still take.
        if let Some(sink) = self.sink.take() {
            let (_inner, _unflushed) = sink.into_parts();
        }
        eprintln!("fuzzy-dedupe: measurement output stopped: {err}");
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.flush();
    }
}
