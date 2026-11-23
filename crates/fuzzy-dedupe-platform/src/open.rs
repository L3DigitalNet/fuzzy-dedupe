// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! Open a folder or web address with the desktop's default application
//! (C-07, plan D-012; serves Open containing folder, Open log folder and
//! About's Source code, FR-028).
//!
//! The Linux implementation hands the target to `xdg-open`, the workspace's
//! only process spawn (C-09 allowlists `std::process::Command` in this file
//! alone). Rejected: an opener crate, which has no approved spec §8.3 row.
//!
//! Only two kinds of target are ever launched: an existing directory given
//! as an absolute path, and an `https` address whose host passes a
//! restricted grammar. Anything else is refused before a process starts,
//! because `xdg-open` dispatches on the target's type: a regular file would
//! be *run or opened* by whatever application claims it, and a `file:`,
//! `http:` or custom-scheme URL could reach a handler the user never chose.
//!
//! Nothing here runs on the caller's thread. [`SystemOpener::start`] creates
//! two persistent threads before any open is accepted: a launch worker that
//! validates each target (the directory check touches the filesystem, which
//! can stall on a network mount), launches `xdg-open` and answers through a
//! non-blocking [`OpenHandle`]; and a reaper that owns and waits on every
//! child. Creating both first means a thread-creation failure surfaces
//! before any child exists, so no child is ever left unowned. Admission is
//! bounded ([`JOB_QUEUE_CAPACITY`], [`MAX_LIVE_CHILDREN`]): excess opens are
//! answered [`OpenError::Busy`] at once.
//!
//! Untested residue: reaping and the live-process cap act only on real
//! children, and the C-09 guard bars spawning a process in tests, so no
//! test exercises them.
//!
//! Threat model: a folder target is checked, then handed to `xdg-open`,
//! which resolves the path again. Another process running as the same user
//! could retarget a link in that window; that is out of scope, because such
//! a process can already launch anything as that user directly, and
//! closing the window would need handle-based launching that `std` and the
//! approved crates do not offer.
//!
//! Requirements: `xdg-open` (from xdg-utils) on `PATH`; without it
//! [`OpenError::Launch`] is returned.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TryRecvError, TrySendError};
use std::time::Duration;

/// A target for [`Opener::open`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Location {
    /// An existing directory, as an absolute path.
    Folder(PathBuf),
    /// An `https` address.
    WebAddress(String),
}

/// Why a location was not opened.
#[derive(Debug)]
pub enum OpenError {
    /// The folder target is relative, missing or not a directory.
    NotADirectory(PathBuf),
    /// The address is not an `https` URL with an acceptable host.
    NotHttps(String),
    /// The desktop opener could not be started.
    Launch(std::io::Error),
    /// The opener worker could not be created, so no open is accepted.
    WorkerUnavailable(std::io::Error),
    /// The opener worker stopped or panicked before answering.
    WorkerFailed,
    /// Too many opens are pending or running ([`JOB_QUEUE_CAPACITY`],
    /// [`MAX_LIVE_CHILDREN`]); nothing was launched. Retrying later works.
    Busy,
}

impl std::fmt::Display for OpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotADirectory(p) => write!(f, "{} is not a folder", p.display()),
            Self::NotHttps(a) => write!(f, "{a:?} is not an https address"),
            Self::Launch(e) => write!(f, "the desktop opener could not be started: {e}"),
            Self::WorkerUnavailable(e) => write!(f, "opening locations is unavailable: {e}"),
            Self::WorkerFailed => f.write_str("the desktop opener stopped unexpectedly"),
            Self::Busy => f.write_str("the desktop opener is busy; try again shortly"),
        }
    }
}

impl std::error::Error for OpenError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Launch(e) | Self::WorkerUnavailable(e) => Some(e),
            _ => None,
        }
    }
}

/// A pending open. [`OpenHandle::poll`] never blocks.
#[derive(Debug)]
pub struct OpenHandle {
    // `None` once the result has been delivered.
    rx: Option<Receiver<Result<(), OpenError>>>,
}

impl OpenHandle {
    /// A handle that already holds `result`, for test doubles of [`Opener`].
    #[must_use]
    pub fn ready(result: Result<(), OpenError>) -> Self {
        let (tx, rx) = mpsc::sync_channel(1);
        // The receiver is alive and the buffer is empty, so this cannot fail.
        let _ = tx.send(result);
        Self { rx: Some(rx) }
    }

    /// The result, exactly once: `None` while the open is in progress and
    /// after the result has been taken. `Ok` means the opener started;
    /// whether the desktop then shows the target is not observable here.
    pub fn poll(&mut self) -> Option<Result<(), OpenError>> {
        let result = match self.rx.as_ref()?.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return None,
            // The worker answers every job it receives, panic or not, so a
            // disconnect means the job never reached a live worker.
            Err(TryRecvError::Disconnected) => Err(OpenError::WorkerFailed),
        };
        self.rx = None;
        Some(result)
    }
}

/// Opens locations with the desktop's default application. The app holds a
/// `dyn Opener` so its tests can substitute a recorder.
pub trait Opener {
    /// Validate `location` with [`check`] and hand it to the desktop, off
    /// the calling thread. `notify` runs once the result is available to
    /// [`OpenHandle::poll`]; the app passes egui's repaint request.
    ///
    /// The result is [`OpenError::NotADirectory`] or
    /// [`OpenError::NotHttps`] when [`check`] refuses, with nothing
    /// launched; [`OpenError::Launch`] when `xdg-open` cannot start;
    /// [`OpenError::Busy`] when the queue or the live-process cap is full;
    /// and [`OpenError::WorkerFailed`] when the worker is gone. A panic in
    /// `notify` is contained.
    fn open(&self, location: Location, notify: Box<dyn FnOnce() + Send>) -> OpenHandle;
}

struct Job {
    location: Location,
    reply: SyncSender<Result<(), OpenError>>,
    notify: Box<dyn FnOnce() + Send>,
}

/// Opens waiting for the launch worker, beyond the one it is handling. A
/// person clicks Open a handful of times at most; a full queue means the
/// worker is stuck (a folder check stalled on a dead network mount), and
/// further opens are refused with [`OpenError::Busy`] at once instead of
/// piling up behind it.
pub const JOB_QUEUE_CAPACITY: usize = 8;

/// Opener processes alive at once. `xdg-open` usually exits within a
/// second, so reaching this means children are hanging; a launch beyond it
/// is refused with [`OpenError::Busy`] rather than spawning without bound.
pub const MAX_LIVE_CHILDREN: usize = 16;

/// How often the reaper polls its children while any is still running.
const REAP_INTERVAL: Duration = Duration::from_millis(500);

/// The validation the launch worker runs before spawning; [`check`] in
/// production. A parameter only so the unit tests can gate the worker on a
/// blocking validation without launching anything.
type Validate = Arc<dyn Fn(&Location) -> Result<(), OpenError> + Send + Sync>;

/// The `xdg-open` implementation of [`Opener`], backed by two persistent
/// threads: a launch worker (validate, spawn, answer, notify) and a reaper
/// that owns every child. Keeping them apart means a validation stalled on
/// the filesystem delays later opens but never the reaping of earlier ones.
#[derive(Debug)]
pub struct SystemOpener {
    jobs: SyncSender<Job>,
}

impl SystemOpener {
    /// Create the launch worker and the reaper.
    ///
    /// # Errors
    ///
    /// [`OpenError::WorkerUnavailable`] when either thread cannot be
    /// created; no process has been spawned at that point.
    pub fn start() -> Result<Self, OpenError> {
        Self::start_with(Arc::new(check))
    }

    fn start_with(validate: Validate) -> Result<Self, OpenError> {
        let live = Arc::new(AtomicUsize::new(0));
        let (to_reaper, children) = mpsc::channel();
        let reaper_live = Arc::clone(&live);
        // The reaper starts first: should the launch worker then fail to
        // start, `to_reaper` is dropped with its closure and the reaper,
        // which owns no child yet, sees the disconnect and ends.
        std::thread::Builder::new()
            .name("opener-reaper".to_owned())
            .spawn(move || reaper(&children, &reaper_live))
            .map_err(OpenError::WorkerUnavailable)?;
        let (jobs, rx) = mpsc::sync_channel(JOB_QUEUE_CAPACITY);
        std::thread::Builder::new()
            .name("opener".to_owned())
            .spawn(move || launch_worker(&rx, &validate, &to_reaper, &live))
            .map_err(OpenError::WorkerUnavailable)?;
        Ok(Self { jobs })
    }
}

impl Opener for SystemOpener {
    fn open(&self, location: Location, notify: Box<dyn FnOnce() + Send>) -> OpenHandle {
        let (reply, rx) = mpsc::sync_channel(1);
        let job = Job {
            location,
            reply,
            notify,
        };
        // `try_send` never blocks the caller, which is the UI thread.
        match self.jobs.try_send(job) {
            Ok(()) => {}
            Err(TrySendError::Full(job)) => answer(&job.reply, Err(OpenError::Busy), job.notify),
            // The worker is gone. Dropping the reply sender makes the first
            // poll report WorkerFailed; notify still runs so the UI looks.
            Err(TrySendError::Disconnected(job)) => {
                drop(job.reply);
                run_notify(job.notify);
            }
        }
        OpenHandle { rx: Some(rx) }
    }
}

/// Publish `result`, then notify. The order is the contract a consumer
/// driven only by notification relies on: when `notify` runs, the next
/// [`OpenHandle::poll`] already returns the result.
fn answer(
    reply: &SyncSender<Result<(), OpenError>>,
    result: Result<(), OpenError>,
    notify: Box<dyn FnOnce() + Send>,
) {
    // The caller may have dropped its handle; not an error.
    let _ = reply.send(result);
    run_notify(notify);
}

/// Run a caller's `notify`, containing a panic in it: the callback is the
/// caller's code, and its failure must not take down the worker that serves
/// every other open, nor unwind into the caller of [`Opener::open`]. The
/// result was already published, so nothing is lost.
fn run_notify(notify: Box<dyn FnOnce() + Send>) {
    let _ = catch_unwind(AssertUnwindSafe(notify));
}

/// The launch worker: validate, enforce [`MAX_LIVE_CHILDREN`], spawn, hand
/// the child to the reaper, answer and notify, one job at a time.
fn launch_worker(
    rx: &Receiver<Job>,
    validate: &Validate,
    to_reaper: &mpsc::Sender<std::process::Child>,
    live: &AtomicUsize,
) {
    // Children the reaper could not take because it died; owned and reaped
    // here instead, so none is ever left unwaited.
    let mut stranded: Vec<std::process::Child> = Vec::new();
    for job in rx {
        let Job {
            location,
            reply,
            notify,
        } = job;
        // A panic in one job must not end the worker.
        let result = catch_unwind(AssertUnwindSafe(|| {
            validate(&location)?;
            if live.load(Ordering::SeqCst) >= MAX_LIVE_CHILDREN {
                return Err(OpenError::Busy);
            }
            launch(&location)
        }))
        .unwrap_or(Err(OpenError::WorkerFailed));
        let result = result.map(|child| {
            live.fetch_add(1, Ordering::SeqCst);
            if let Err(mpsc::SendError(child)) = to_reaper.send(child) {
                stranded.push(child);
            }
        });
        stranded.retain_mut(|child| matches!(child.try_wait(), Ok(None)));
        answer(&reply, result, notify);
    }
}

/// The reaper: owns every spawned child and waits on it.
///
/// It blocks on the hand-off channel while it owns no child, and otherwise
/// wakes every [`REAP_INTERVAL`] to collect exited children, so none stays
/// a zombie for long. Once the launch worker is gone it keeps reaping until
/// its last child has exited, then ends.
fn reaper(children_rx: &Receiver<std::process::Child>, live: &AtomicUsize) {
    let mut children: Vec<std::process::Child> = Vec::new();
    let mut open = true;
    while open || !children.is_empty() {
        if !open {
            std::thread::sleep(REAP_INTERVAL);
        } else if children.is_empty() {
            match children_rx.recv() {
                Ok(child) => children.push(child),
                Err(_) => open = false,
            }
        } else {
            match children_rx.recv_timeout(REAP_INTERVAL) {
                Ok(child) => children.push(child),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => open = false,
            }
        }
        // Keep a child while it is still running; an exited child is now
        // reaped, and a `try_wait` error means it can no longer be waited
        // on at all. Either way it no longer counts as live.
        let before = children.len();
        children.retain_mut(|child| matches!(child.try_wait(), Ok(None)));
        live.fetch_sub(before - children.len(), Ordering::SeqCst);
    }
}

/// Accept only an absolute path to an existing directory or an `https`
/// address with an acceptable host, the two targets [`SystemOpener`]
/// launches. This touches the filesystem for a folder; [`SystemOpener`]
/// calls it on its worker, never on the caller's thread.
///
/// # Errors
///
/// [`OpenError::NotADirectory`] or [`OpenError::NotHttps`].
pub fn check(location: &Location) -> Result<(), OpenError> {
    match location {
        // `is_dir` follows links, so a link to a directory is accepted: what
        // is launched is the directory, as for any other path to it.
        Location::Folder(path) if path.is_absolute() && path.is_dir() => Ok(()),
        Location::Folder(path) => Err(OpenError::NotADirectory(path.clone())),
        Location::WebAddress(address) if is_https(address) => Ok(()),
        Location::WebAddress(address) => Err(OpenError::NotHttps(address.clone())),
    }
}

/// `https://` (scheme matched case-insensitively, RFC 3986 §3.1), then a
/// host, an optional `:port`, then `/`, `?`, `#` or the end, with no
/// whitespace or control character anywhere.
///
/// The host is either ASCII letters, digits, `-` and `.`, or a bracketed
/// literal that `std::net::Ipv6Addr` parses; the port is digits whose value
/// is 1 to 65535 (leading zeros allowed, as RFC 3986 permits). The grammar
/// is deliberately narrower than RFC 3986: it has no userinfo (`user@`), so
/// an address cannot show one host and name another, and no
/// percent-encoding or IPv6 zone, which desktop handlers parse
/// inconsistently. The path, query and fragment are not checked beyond the
/// whitespace and control rule: `xdg-open` receives the address as one
/// argument with no shell in between, so they cannot inject anything. The
/// fixed prefix also guarantees the argument never starts with `-`, so
/// `xdg-open` cannot read it as an option.
fn is_https(address: &str) -> bool {
    const SCHEME: &str = "https://";
    if address.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return false;
    }
    let Some(rest) = address
        .get(..SCHEME.len())
        .filter(|s| s.eq_ignore_ascii_case(SCHEME))
        .map(|_| &address[SCHEME.len()..])
    else {
        return false;
    };
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..end];
    let port = if let Some(literal) = authority.strip_prefix('[') {
        let Some((ip, after)) = literal.split_once(']') else {
            return false;
        };
        if ip.parse::<std::net::Ipv6Addr>().is_err() {
            return false;
        }
        after
    } else {
        let (host, port) = authority
            .find(':')
            .map_or((authority, ""), |i| authority.split_at(i));
        let named = !host.is_empty()
            && host
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.');
        if !named {
            return false;
        }
        port
    };
    port.is_empty()
        || port.strip_prefix(':').is_some_and(|digits| {
            // `u16::from_str` alone would accept a leading `+`; the digit
            // check rules that out, and overflow rejects values above 65535.
            digits.bytes().all(|b| b.is_ascii_digit())
                && digits.parse::<u16>().is_ok_and(|port| port != 0)
        })
}

/// Start `xdg-open` on an already checked target and return the child for
/// the worker to own and reap.
#[expect(
    clippy::disallowed_types,
    reason = "open location: the workspace's only process spawn, xdg-open on a checked folder or https target (C-09 allowlist)"
)]
fn launch(location: &Location) -> Result<std::process::Child, OpenError> {
    use std::process::{Command, Stdio};

    let arg = match location {
        Location::Folder(path) => path.as_os_str().to_owned(),
        Location::WebAddress(address) => address.into(),
    };
    Command::new("xdg-open")
        .arg(arg)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(OpenError::Launch)
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::sync::mpsc::Sender;
    use std::time::Instant;

    use super::*;

    const DEADLINE: Duration = Duration::from_secs(10);

    fn wait(handle: &mut OpenHandle) -> Result<(), OpenError> {
        let deadline = Instant::now() + DEADLINE;
        loop {
            if let Some(result) = handle.poll() {
                return result;
            }
            assert!(Instant::now() < deadline, "open result never arrived");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// A full queue answers `Busy` at once. The launch worker is held inside
    /// a validation that blocks until released and then refuses, so nothing
    /// is ever launched (the C-09 guard bars spawning in tests).
    #[test]
    fn a_full_queue_answers_busy_without_blocking() {
        let (entered, worker_entered) = mpsc::channel::<()>();
        let (release, gate) = mpsc::channel::<()>();
        let entered = Mutex::new(entered);
        let gate = Mutex::new(gate);
        let validate: Validate = Arc::new(move |_| {
            let _ = entered.lock().expect("entered").send(());
            gate.lock().expect("gate").recv().expect("released");
            Err(OpenError::NotHttps(String::new()))
        });
        let opener = SystemOpener::start_with(validate).expect("opener");
        let target = || Location::WebAddress("https://example.com/".to_owned());
        let quiet = || -> Box<dyn FnOnce() + Send> { Box::new(|| {}) };

        let mut handles = vec![opener.open(target(), quiet())];
        worker_entered
            .recv_timeout(DEADLINE)
            .expect("worker holds job 1");
        for _ in 0..JOB_QUEUE_CAPACITY {
            handles.push(opener.open(target(), quiet()));
        }

        let (signal, woken): (Sender<()>, _) = mpsc::channel();
        let started = Instant::now();
        let mut busy = opener.open(target(), Box::new(move || signal.send(()).expect("signal")));
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "open never blocks"
        );
        woken.recv_timeout(DEADLINE).expect("Busy is notified");
        assert!(matches!(busy.poll(), Some(Err(OpenError::Busy))));

        for _ in 0..handles.len() {
            release.send(()).expect("release");
        }
        for handle in &mut handles {
            assert!(matches!(wait(handle), Err(OpenError::NotHttps(_))));
        }
    }
}
