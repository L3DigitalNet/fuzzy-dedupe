// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! The scripted spec §11.9 scenarios T1 to T4, as time-driven scripts.
//!
//! A driver produces two things and nothing else:
//!
//! - egui raw input events (wheel, key repeat, chords), injected through
//!   the hook's `raw_input`, the path real input takes (plan T23
//!   invariant). Each input has a due time on a fixed cadence; a frame that
//!   starts late injects everything that fell due meanwhile, with its
//!   original due time, so a late frame shows as a larger `input_to_paint`
//!   sample instead of hiding the wait;
//! - UI intents for what raw input cannot reach without a person: opening
//!   the T2 pair, opening T3's popups that have no global chord, and
//!   starting and cancelling T4's simulated scan. The portal folder picker
//!   cannot be scripted, and a New scan from the popup would change the
//!   scan setup preference, so T4 submits `StartScan` directly with its own
//!   setup and rules.
//!
//! T3's intent-opened popups (About, Queue identical copies, Review unique
//! text) give no `input_to_paint` sample on opening: an intent skips raw
//! input, so there is no input due time to measure from. Their Esc closes
//! are raw input and are sampled like every other step.
//!
//! Every script ends after its configured duration by asking the app to
//! quit, so an operator's redirect captures exactly one run.
//!
//! The scripts read only what the frame events carry. They do not check
//! that the app reacted (that a key moved the focus, say): the samples
//! measure frames whatever the app did with the input. The `end_to_end`
//! tests in `tests/measure.rs` prove each script reaches its targets, so
//! a script change that misses one fails there, not silently in a run.

use std::path::PathBuf;
use std::time::Duration;

use egui::{Event, Key, Modifiers, MouseWheelSource, MouseWheelUnit, Pos2, TouchPhase, Vec2};
use fuzzy_dedupe_domain::{
    FileId, IgnoreOptions, MatchMode, MatchingRules, Permille, QueueOrigin, QueueScope,
    RelationKind, RulesRevision, ScanFolder, ScanId, ScanSetup,
};
use fuzzy_dedupe_engine::fixtures::{self, ScenarioKind};
use fuzzy_dedupe_engine::service::{EngineRequest, ReviewCommand, StartScan};

use super::{FrameEvents, MeasureScenario};
use crate::presentation::commands::UiIntent;
use crate::presentation::state::{DialogKind, DialogSubject, UniqueTextSubject};

/// Inputs injected into one frame at most. A long stall would otherwise
/// replay seconds of key repeat into one pass; the rest is skipped, and the
/// injected ones already carry the stall in their samples.
pub const MAX_INPUTS_PER_FRAME: usize = 4;

/// Wait before the first scripted action, so startup work (scenario
/// adoption, font upload) is not measured as input latency.
pub const LEAD_IN: Duration = Duration::from_millis(500);

/// T1 and T2 alternate their input kind this often.
const PHASE: Duration = Duration::from_secs(4);
/// One wheel notch per display frame at 60 Hz.
const WHEEL_EVERY: Duration = Duration::from_millis(16);
/// A typical desktop key-repeat rate, 30 Hz.
const REPEAT_EVERY: Duration = Duration::from_millis(33);
/// T3: one open or close step this often.
const POPUP_STEP_EVERY: Duration = Duration::from_millis(400);
/// T4: a key press this often while the scan runs.
const T4_KEY_EVERY: Duration = Duration::from_millis(100);
/// T4: cancel this long after the scan reported Started. The `large` scan
/// runs for about four seconds (`scan_sim`), so two seconds in it is still
/// saturating the workers.
const T4_CANCEL_AFTER: Duration = Duration::from_secs(2);
/// T4: idle time between one scan's end and the next start.
const T4_RESTART_AFTER: Duration = Duration::from_secs(1);

/// The T2 pair's files, as `compare::texts::large_pair_scenario` numbers
/// them: A (1) is the reference, B (2) the member shown against it.
pub const T2_TARGET: FileId = FileId::new(2);
pub const T2_COUNTERPART: FileId = FileId::new(1);

/// The scan the app's startup data carries in a measurement run. Cross-file
/// contract with `adopt_startup` in `app.rs`: it reserves the startup scan
/// as the first `Engine::reserve_scan_id` call on a fresh engine, whose
/// counter starts at 1. T3's subjects name this scan; the `end_to_end` T3
/// test fails if they stop matching the scan the app holds.
const STARTUP_SCAN: ScanId = ScanId::new(1);

/// One T3 step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum T3Step {
    Key(Modifiers, Key),
    /// Push `kind`'s `OpenDialog` intent ([`PopupOpener::Intent`]).
    Open(DialogKind),
}

/// T3's steps: on Findings, then on Compare, each of [`t3_popups`] opened
/// and closed with Esc, then the main menu. F10 toggles the main menu, so
/// it closes itself.
fn t3_steps() -> Vec<T3Step> {
    let popups = t3_popups();
    let mut steps = Vec::with_capacity(2 * (3 + 2 * popups.len()));
    for workspace in [Key::Num1, Key::Num2] {
        steps.push(T3Step::Key(Modifiers::CTRL, workspace));
        for &kind in &popups {
            steps.push(match popup_opener(kind) {
                PopupOpener::Chord(modifiers, key) => T3Step::Key(modifiers, key),
                PopupOpener::Intent => T3Step::Open(kind),
            });
            steps.push(T3Step::Key(Modifiers::NONE, Key::Escape));
        }
        steps.push(T3Step::Key(Modifiers::NONE, Key::F10));
        steps.push(T3Step::Key(Modifiers::NONE, Key::F10));
    }
    steps
}

/// The subjects T3's intent-opened popups need, taken from the startup
/// data the way the menus that open them take them from the results.
#[derive(Clone, Debug, PartialEq, Eq)]
struct T3Subjects {
    /// A member with unique lines (Unique > 0) against its counterpart, as
    /// the row menu's Review unique text builds it.
    unique_text: UniqueTextSubject,
    /// A group with identical copies.
    identical: QueueScope,
}

impl T3Subjects {
    /// `None` when the startup data has no such pair or group; T3 then
    /// pushes no intent for that popup, and the `end_to_end` T3 test fails.
    fn from_startup() -> Option<Self> {
        let scenario = MeasureScenario::T3
            .startup()
            .build(STARTUP_SCAN, fixtures::DEFAULT_SEED)
            .ok()?;
        let results = &scenario.results;
        let unique_text = results.groups.iter().find_map(|group| {
            group.members.iter().find_map(|member| {
                let link = member.link().filter(|link| link.unique_lines > 0)?;
                let counterpart = group.member(link.counterpart)?;
                Some(UniqueTextSubject {
                    scan: results.scan,
                    target: member.file,
                    counterpart: counterpart.file,
                    target_snapshot: member.snapshot,
                    counterpart_snapshot: counterpart.snapshot,
                    rules: results.rules.revision,
                    reviewed: None,
                    origin: QueueOrigin::RowMenu,
                })
            })
        })?;
        let identical = results
            .groups
            .iter()
            .find(|group| group.row_kinds().contains(&RelationKind::SameBytes))?;
        Some(Self {
            unique_text,
            identical: QueueScope::Groups(vec![identical.id]),
        })
    }

    /// `kind`'s `OpenDialog` intent. No invoker: nothing on screen opened
    /// it, so closing it returns focus nowhere.
    fn open(&self, kind: DialogKind) -> UiIntent {
        let subject = match kind {
            DialogKind::UniqueTextReview => DialogSubject::UniqueText(self.unique_text),
            DialogKind::QueueIdentical => DialogSubject::QueueIdentical(self.identical.clone()),
            DialogKind::MatchingRules
            | DialogKind::NewScan
            | DialogKind::Cleanup
            | DialogKind::KeyboardShortcuts
            | DialogKind::About => DialogSubject::None,
        };
        UiIntent::OpenDialog {
            kind,
            subject,
            invoker: None,
        }
    }
}

/// How T3 opens one popup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PopupOpener {
    /// Its global chord (spec §11.10), through raw input.
    Chord(Modifiers, Key),
    /// No global chord reaches it: the app's own `UiIntent::OpenDialog`, as
    /// the control that opens it pushes, with a subject from the startup
    /// data ([`T3Subjects`]).
    Intent,
}

/// The popup T3 opens after `previous` (`None`: the first one). This match
/// is the script's list: it is exhaustive, so a new `DialogKind` does not
/// compile until it has an arm here, and that arm must link it into the
/// chain (an arm that returns `None` early would cut off the rest). The
/// `end_to_end` T3 test checks the chain against `DialogKind::ALL`.
const fn popup_after(previous: Option<DialogKind>) -> Option<DialogKind> {
    match previous {
        None => Some(DialogKind::NewScan),
        Some(DialogKind::NewScan) => Some(DialogKind::MatchingRules),
        Some(DialogKind::MatchingRules) => Some(DialogKind::KeyboardShortcuts),
        Some(DialogKind::KeyboardShortcuts) => Some(DialogKind::Cleanup),
        Some(DialogKind::Cleanup) => Some(DialogKind::About),
        Some(DialogKind::About) => Some(DialogKind::QueueIdentical),
        Some(DialogKind::QueueIdentical) => Some(DialogKind::UniqueTextReview),
        Some(DialogKind::UniqueTextReview) => None,
    }
}

/// Every popup T3 opens on each workspace, in script order (spec §11.9 T3:
/// "Each popup opened and closed over both workspaces").
#[must_use]
pub fn t3_popups() -> Vec<DialogKind> {
    let mut popups = Vec::new();
    let mut previous = None;
    // A mis-edited chain that loops back stops at the repeat instead of
    // spinning the UI thread.
    while let Some(kind) = popup_after(previous)
        && !popups.contains(&kind)
    {
        popups.push(kind);
        previous = Some(kind);
    }
    popups
}

/// How T3 opens `kind`: its chord where spec §11.10 gives it one.
#[must_use]
pub const fn popup_opener(kind: DialogKind) -> PopupOpener {
    match kind {
        DialogKind::NewScan => PopupOpener::Chord(Modifiers::CTRL, Key::N),
        DialogKind::MatchingRules => PopupOpener::Chord(Modifiers::CTRL, Key::Comma),
        DialogKind::KeyboardShortcuts => PopupOpener::Chord(Modifiers::NONE, Key::F1),
        DialogKind::Cleanup => PopupOpener::Chord(Modifiers::CTRL, Key::Enter),
        // About sits in the main menu, Queue identical copies in the
        // Actions menu, Review unique text in the row and Only in menus.
        DialogKind::About | DialogKind::QueueIdentical | DialogKind::UniqueTextReview => {
            PopupOpener::Intent
        }
    }
}

/// One scripted input: when it fell due and the events that make it.
#[derive(Clone, Debug, PartialEq)]
pub struct Injection {
    pub at: Duration,
    pub events: Vec<Event>,
}

/// T4's scan lifecycle as the driver sees it through the reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ScanStep {
    /// Start a scan at this time.
    StartAt(Duration),
    /// `StartScan` was sent; waiting for its Started event.
    Requested,
    Running {
        scan: ScanId,
        started: Duration,
        cancelled: bool,
    },
}

/// The script of one scenario run.
#[derive(Debug)]
pub struct Driver {
    scenario: MeasureScenario,
    duration: Duration,
    /// The first frame's time; the script's clock starts here.
    origin: Option<Duration>,
    next_input: Duration,
    inputs_sent: u64,
    /// T2: Compare was asked for, and its comparison has arrived.
    compare_requested: bool,
    compare_ready: bool,
    scan: ScanStep,
    /// T3: the next step's index, counting every step (an intent step
    /// injects no input, so `inputs_sent` cannot index the list).
    t3_step: usize,
    /// T3: built once at the start; `None` outside T3.
    t3_subjects: Option<T3Subjects>,
    /// Intents queued by `inject` for the same frame's `take_intents`.
    pending: Vec<UiIntent>,
    quit_sent: bool,
}

impl Driver {
    #[must_use]
    pub fn new(scenario: MeasureScenario, duration: Duration) -> Self {
        Self {
            scenario,
            duration,
            origin: None,
            next_input: LEAD_IN,
            inputs_sent: 0,
            compare_requested: false,
            compare_ready: false,
            scan: ScanStep::StartAt(LEAD_IN),
            t3_step: 0,
            t3_subjects: match scenario {
                MeasureScenario::T3 => T3Subjects::from_startup(),
                MeasureScenario::T1 | MeasureScenario::T2 | MeasureScenario::T4 => None,
            },
            pending: Vec::new(),
            quit_sent: false,
        }
    }

    /// The script has asked the app to quit.
    #[must_use]
    pub const fn finished(&self) -> bool {
        self.quit_sent
    }

    fn elapsed(&mut self, now: Duration) -> Duration {
        let origin = *self.origin.get_or_insert(now);
        now.saturating_sub(origin)
    }

    /// The inputs due by `now` for a window whose content is `screen`, each
    /// with its due time on the same clock as `now`.
    pub fn inject(&mut self, now: Duration, screen: egui::Rect) -> Vec<Injection> {
        let elapsed = self.elapsed(now);
        let origin = now.saturating_sub(elapsed);
        let mut out = Vec::new();
        if self.quit_sent || elapsed >= self.duration {
            return out;
        }
        while self.next_input <= elapsed && out.len() < MAX_INPUTS_PER_FRAME {
            let due = self.next_input;
            let (events, every) = self.script_input(due, screen.center());
            self.next_input = due + every;
            if !events.is_empty() {
                self.inputs_sent += 1;
                out.push(Injection {
                    at: origin + due,
                    events,
                });
            }
        }
        if self.next_input <= elapsed {
            // Skipped, not queued: see MAX_INPUTS_PER_FRAME.
            self.next_input = elapsed;
        }
        out
    }

    /// The input due at `due` (script time) and the cadence to the next.
    /// A T3 intent step returns no events and queues its intent instead.
    fn script_input(&mut self, due: Duration, center: Pos2) -> (Vec<Event>, Duration) {
        let phase = (due.as_millis() / PHASE.as_millis()).is_multiple_of(2);
        match self.scenario {
            MeasureScenario::T1 => {
                if self.inputs_sent == 0 {
                    // Point at the table and focus a row, so wheel and keys
                    // land on it.
                    return (click(center), REPEAT_EVERY);
                }
                if phase {
                    (wheel(center, down(due)), WHEEL_EVERY)
                } else {
                    (vec![repeat(arrow(due))], REPEAT_EVERY)
                }
            }
            MeasureScenario::T2 => {
                if !self.compare_ready {
                    return (Vec::new(), REPEAT_EVERY);
                }
                // Each phase boundary toggles wrap (Alt+Z), and every other
                // one selects a whole pane and copies it (IR-006).
                let cycle = due.as_millis() / PHASE.as_millis();
                let at_boundary = due.as_millis() % PHASE.as_millis() < REPEAT_EVERY.as_millis();
                if at_boundary {
                    let mut events = chord(Modifiers::ALT, Key::Z);
                    if cycle % 2 == 1 {
                        events.extend(chord(Modifiers::CTRL, Key::A));
                        events.push(Event::Copy);
                    }
                    return (events, REPEAT_EVERY);
                }
                if phase {
                    (wheel(center, down(due)), WHEEL_EVERY)
                } else {
                    let key = if down(due) {
                        Key::PageDown
                    } else {
                        Key::PageUp
                    };
                    (vec![repeat(key)], REPEAT_EVERY)
                }
            }
            MeasureScenario::T3 => {
                let steps = t3_steps();
                let step = steps[self.t3_step % steps.len()];
                self.t3_step += 1;
                match step {
                    T3Step::Key(modifiers, key) => (chord(modifiers, key), POPUP_STEP_EVERY),
                    T3Step::Open(kind) => {
                        if let Some(subjects) = &self.t3_subjects {
                            self.pending.push(subjects.open(kind));
                        }
                        (Vec::new(), POPUP_STEP_EVERY)
                    }
                }
            }
            MeasureScenario::T4 => {
                let key = if self.inputs_sent.is_multiple_of(2) {
                    Key::ArrowDown
                } else {
                    Key::ArrowUp
                };
                (chord(Modifiers::NONE, key), T4_KEY_EVERY)
            }
        }
    }

    /// Reads the current frame's drained events.
    pub fn observe(&mut self, now: Duration, events: &FrameEvents) {
        let elapsed = self.elapsed(now);
        if events.comparison_ready && self.compare_requested {
            self.compare_ready = true;
        }
        if let (ScanStep::Requested, Some(scan)) = (self.scan, events.scan_started) {
            self.scan = ScanStep::Running {
                scan,
                started: elapsed,
                cancelled: false,
            };
        }
        if events.scan_ended.is_some() && matches!(self.scan, ScanStep::Running { .. }) {
            self.scan = ScanStep::StartAt(elapsed + T4_RESTART_AFTER);
        }
    }

    /// Intents for the app to dispatch, and whether one of them cancels a
    /// scan (the recorder's `cancel_ack` start).
    pub fn take_intents(&mut self, now: Duration) -> (Vec<UiIntent>, bool) {
        let elapsed = self.elapsed(now);
        if self.quit_sent {
            return (Vec::new(), false);
        }
        if elapsed >= self.duration {
            self.quit_sent = true;
            return (vec![UiIntent::Quit], false);
        }
        match self.scenario {
            MeasureScenario::T2 if !self.compare_requested && elapsed >= LEAD_IN => {
                self.compare_requested = true;
                (
                    vec![UiIntent::OpenCompare {
                        target: T2_TARGET,
                        counterpart: T2_COUNTERPART,
                        return_row: None,
                    }],
                    false,
                )
            }
            // Queued by this frame's `inject`, so dispatched after the
            // same frame's pass, as a chord pressed in that frame would be.
            MeasureScenario::T3 => (std::mem::take(&mut self.pending), false),
            MeasureScenario::T4 => self.scan_intents(elapsed),
            _ => (Vec::new(), false),
        }
    }

    fn scan_intents(&mut self, elapsed: Duration) -> (Vec<UiIntent>, bool) {
        match self.scan {
            ScanStep::StartAt(at) if elapsed >= at => {
                self.scan = ScanStep::Requested;
                (
                    vec![
                        // As New scan's Start scan does, minus the
                        // preference writes (see the module docs).
                        UiIntent::Review {
                            command: ReviewCommand::ClearForNewScan,
                            invoker: None,
                        },
                        UiIntent::Submit(EngineRequest::StartScan(t4_start())),
                    ],
                    false,
                )
            }
            ScanStep::Running {
                scan,
                started,
                cancelled: false,
            } if elapsed >= started + T4_CANCEL_AFTER => {
                self.scan = ScanStep::Running {
                    scan,
                    started,
                    cancelled: true,
                };
                (
                    vec![UiIntent::Submit(EngineRequest::CancelScan(scan))],
                    true,
                )
            }
            _ => (Vec::new(), false),
        }
    }
}

/// T4's scan: the `large` scenario, 100,000 rows of simulated work over
/// every worker thread. The folder is never read (plan D-005).
#[must_use]
pub fn t4_start() -> StartScan {
    StartScan {
        setup: ScanSetup {
            folders: vec![ScanFolder {
                path: PathBuf::from("/measurement/t4"),
                protect: false,
            }],
            file_types: vec!["txt".to_owned()],
        },
        rules: MatchingRules {
            mode: MatchMode::ExactAndFuzzy,
            minimum_similarity: Permille::new(800).unwrap_or(Permille::FULL),
            ignore: IgnoreOptions {
                whitespace: false,
                line_endings: true,
                letter_case: false,
                punctuation: false,
            },
            revision: RulesRevision::new(1),
        },
        scenario: ScenarioKind::Large,
        seed: fixtures::DEFAULT_SEED,
    }
}

/// Scrolling direction at script time `due`: down for two phases, then
/// back up, so a long run stays within the content instead of pinning at
/// its end, where scrolling stops costing anything.
fn down(due: Duration) -> bool {
    (due.as_millis() / (2 * PHASE.as_millis())).is_multiple_of(2)
}

fn arrow(due: Duration) -> Key {
    if down(due) {
        Key::ArrowDown
    } else {
        Key::ArrowUp
    }
}

fn key(key: Key, modifiers: Modifiers, pressed: bool, repeat: bool) -> Event {
    Event::Key {
        key,
        physical_key: None,
        pressed,
        repeat,
        modifiers,
    }
}

fn chord(modifiers: Modifiers, k: Key) -> Vec<Event> {
    vec![
        key(k, modifiers, true, false),
        key(k, modifiers, false, false),
    ]
}

/// One auto-repeat press, as a held key delivers it.
fn repeat(k: Key) -> Event {
    key(k, Modifiers::NONE, true, true)
}

/// One wheel notch with the pointer over `at`: egui scrolls the area under
/// the pointer, and a scripted run has no pointer until one is sent.
fn wheel(at: Pos2, down: bool) -> Vec<Event> {
    vec![
        Event::PointerMoved(at),
        Event::MouseWheel {
            unit: MouseWheelUnit::Line,
            delta: Vec2::new(0.0, if down { -3.0 } else { 3.0 }),
            phase: TouchPhase::Move,
            source: MouseWheelSource::Unknown,
            modifiers: Modifiers::NONE,
        },
    ]
}

fn click(at: Pos2) -> Vec<Event> {
    let button = |pressed| Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    };
    vec![Event::PointerMoved(at), button(true), button(false)]
}
