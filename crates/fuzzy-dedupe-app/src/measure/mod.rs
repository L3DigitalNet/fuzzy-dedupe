// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! The measurement hook (plan C-12, C-10). Stub until plan T23, which adds
//! the measurement mode and scenario drivers in `recorder.rs` and
//! `drivers.rs` and fills [`configured`], [`startup_scenario`] and its own
//! [`FrameHook`] in this file.
//!
//! What `app.rs` gives the hook, and when (all outside the UI pass):
//!
//! - [`FrameHook::raw_input`] from eframe's `App::raw_input_hook`, before
//!   each pass: drivers inject scripted input here, through the same egui
//!   raw input real input uses;
//! - [`FrameHook::on_frame`] once per frame from `App::logic`, with what the
//!   frame ingested and eframe's CPU time for the previous frame.
//!
//! Both must return promptly and never block the UI thread. Samples go to
//! standard output only (plan D-010).

use fuzzy_dedupe_engine::fixtures::ScenarioKind;

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

/// Receives the frame events above.
pub trait FrameHook {
    /// May edit the raw input of the coming pass. Default: leaves it.
    fn raw_input(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        let _ = (ctx, raw);
    }

    fn on_frame(&mut self, report: FrameReport);
}

/// The hook when measurement is off: does nothing.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoMeasurement;

impl FrameHook for NoMeasurement {
    fn on_frame(&mut self, report: FrameReport) {
        let _ = report;
    }
}

/// The production hook: the measurement mode's recorder when that mode is
/// enabled, otherwise [`NoMeasurement`]. Stub: always [`NoMeasurement`].
#[must_use]
pub fn configured() -> Box<dyn FrameHook> {
    Box::new(NoMeasurement)
}

/// The scenario the app adopts at startup. A measurement run picks its
/// scenario (T1 `large`, T2 the 5 MB pair); otherwise the design scenario.
/// Stub: always the design scenario.
#[must_use]
pub fn startup_scenario() -> ScenarioKind {
    ScenarioKind::Design
}
