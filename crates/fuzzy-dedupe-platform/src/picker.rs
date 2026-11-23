// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! The folder picker (C-07; spec §8.3 row `rfd`, C-016), used by New scan.
//!
//! The dialog is modal to its caller and can stay open for minutes, so it
//! never runs on the UI thread: [`FolderPicker::pick_folders`] starts it on
//! a worker thread and returns a [`PickHandle`] at once. The UI polls the
//! handle each frame and is told to repaint through the `notify` callback
//! when the answer arrives, so egui's reactive mode does not leave the
//! result unseen until the next input event.
//!
//! Backend: `rfd`'s XDG desktop portal backend (`default-features = false`,
//! `xdg-portal`), which works under GNOME and KDE Plasma on Wayland without
//! linking GTK. When no portal answers, `rfd` itself falls back to running
//! `zenity`; that spawn is inside the dependency, outside the workspace
//! sources C-09 governs. Calling the blocking dialog from a worker thread is
//! validated for Linux only; on macOS `rfd` re-dispatches the dialog to the
//! main thread itself, which a macOS adapter must verify against eframe's
//! event loop.
//!
//! Errors are limited by `rfd`'s blocking API, which returns no value both
//! when the user cancels and when neither the portal nor `zenity` could show
//! a dialog (it logs the cause through the `log` crate). Both therefore
//! arrive as [`PickOutcome::Dismissed`]; the only typed failure is
//! [`PickError::WorkerFailed`], for a worker that panicked or could not
//! start. No path through here panics on the UI thread.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex};

/// What the user chose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickOutcome {
    /// One or more folders, in the order the dialog returned them.
    Chosen(Vec<PathBuf>),
    /// No folder: the user cancelled, or no dialog could be shown.
    Dismissed,
}

/// The picker failed without an answer from the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickError {
    /// The dialog worker panicked or could not be started.
    WorkerFailed,
}

impl std::fmt::Display for PickError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WorkerFailed => f.write_str("the folder picker failed to open"),
        }
    }
}

impl std::error::Error for PickError {}

/// What to show in the dialog.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PickRequest {
    pub title: String,
    /// The folder the dialog opens in, when the portal honours it.
    pub start_dir: Option<PathBuf>,
}

type NotifySlot = Arc<Mutex<Option<Box<dyn FnOnce() + Send>>>>;

/// Run the notify callback if no one has yet. A poisoned lock still holds
/// the callback, so it is recovered rather than lost.
fn take_and_notify(slot: &NotifySlot) {
    let notify = slot
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take();
    if let Some(notify) = notify {
        notify();
    }
}

/// A pending pick. [`PickHandle::poll`] never blocks.
#[derive(Debug)]
pub struct PickHandle {
    // `None` once the result has been delivered.
    rx: Option<Receiver<Result<PickOutcome, PickError>>>,
}

impl PickHandle {
    /// Run `job` on a new worker thread and return a handle to its result.
    ///
    /// `notify` runs on the worker after the result is available to
    /// [`PickHandle::poll`]; the app passes egui's repaint request. A
    /// panicking `job` yields [`PickError::WorkerFailed`]. This is the
    /// mechanism behind [`PortalPicker`] and is public so test doubles of
    /// [`FolderPicker`] can produce real handles.
    pub fn spawn(
        job: impl FnOnce() -> PickOutcome + Send + 'static,
        notify: impl FnOnce() + Send + 'static,
    ) -> Self {
        let (tx, rx) = mpsc::sync_channel(1);
        // `notify` waits in a slot both sides can reach, so a worker that
        // never starts still has it called below: otherwise a consumer
        // driven only by notification would never look at the handle and
        // never see the WorkerFailed.
        let slot: NotifySlot = Arc::new(Mutex::new(Some(Box::new(notify))));
        let worker_slot = Arc::clone(&slot);
        let started = std::thread::Builder::new()
            .name("folder-picker".to_owned())
            .spawn(move || {
                // AssertUnwindSafe: nothing `job` captured is observed after
                // a panic; the only state crossing back is the message.
                let result =
                    catch_unwind(AssertUnwindSafe(job)).map_err(|_| PickError::WorkerFailed);
                // Send before notify: a consumer woken by notify must find
                // the result already there. A dropped handle means nobody is
                // waiting; not an error.
                let _ = tx.send(result);
                take_and_notify(&worker_slot);
            });
        if started.is_err() {
            // The closure, and the sender in it, were dropped unrun, so the
            // channel is already disconnected and the first poll reports
            // WorkerFailed; notify now that it is observable.
            take_and_notify(&slot);
        }
        Self { rx: Some(rx) }
    }

    /// The result, exactly once: `None` while the dialog is open and after
    /// the result has been taken.
    pub fn poll(&mut self) -> Option<Result<PickOutcome, PickError>> {
        let result = match self.rx.as_ref()?.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return None,
            // Only reachable when the worker never started: a started
            // worker sends before it drops the sender, panic or not.
            Err(TryRecvError::Disconnected) => Err(PickError::WorkerFailed),
        };
        self.rx = None;
        Some(result)
    }
}

/// Shows a folder dialog. The app holds a `dyn FolderPicker` so its tests
/// can answer without a desktop.
pub trait FolderPicker {
    /// Start a multi-select folder dialog off the calling thread; see
    /// [`PickHandle::spawn`] for `notify`.
    fn pick_folders(&self, request: PickRequest, notify: Box<dyn FnOnce() + Send>) -> PickHandle;
}

/// The `rfd` XDG desktop portal implementation of [`FolderPicker`].
#[derive(Debug, Clone, Copy, Default)]
pub struct PortalPicker;

impl FolderPicker for PortalPicker {
    fn pick_folders(&self, request: PickRequest, notify: Box<dyn FnOnce() + Send>) -> PickHandle {
        PickHandle::spawn(
            move || {
                // No parent window is set: the raw window handle cannot be
                // sent to the worker thread, and the portal then shows the
                // dialog as a normal top-level window.
                let mut dialog = rfd::FileDialog::new().set_title(request.title);
                if let Some(dir) = request.start_dir {
                    dialog = dialog.set_directory(dir);
                }
                match dialog.pick_folders() {
                    Some(folders) if !folders.is_empty() => PickOutcome::Chosen(folders),
                    _ => PickOutcome::Dismissed,
                }
            },
            notify,
        )
    }
}
