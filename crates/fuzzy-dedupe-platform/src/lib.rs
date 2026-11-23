// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! Operating-system adapter traits and their Linux implementations (plan
//! Appendix A C-07, spec §8.5 and NFR-008).
//!
//! The only crate where OS-conditional code and OS-specific dependencies may
//! appear. It deliberately omits the `#![forbid(unsafe_code)]` every other crate
//! root carries: the workspace `deny` still rejects unsafe code here, but an
//! adapter may relax it with a scoped `#[allow(unsafe_code)]` where an OS API
//! requires it.
//!
//! The MS-1 adapters:
//!
//! - [`dirs`]: per-user configuration, state and log directories;
//! - [`picker`]: the folder picker, run off the UI thread;
//! - [`open`]: opening a folder or `https` address with the desktop's
//!   default application;
//! - [`app_files`]: the app-owned writer, confined to the [`dirs`]
//!   directories;
//! - [`log_retention`]: age-based expiry of the application's own logs
//!   (plan D-015);
//! - [`fonts`]: system fallback-font lookup, a stub until plan T21.
//!
//! No-mutation confinement (plan C-09): `app_files`, `log_retention` and
//! `open` are the only modules in the workspace allowed to make the
//! filesystem-write, delete and process-spawn calls the workspace
//! `clippy.toml` otherwise rejects, each under an item-level
//! `#[expect(clippy::disallowed_*, reason = "…")]`. The textual guard,
//! `crates/fuzzy-dedupe-app/tests/no_file_mutation.rs`, pins those three
//! paths; renaming or splitting one of them is a change to that guard too.

pub mod app_files;
pub mod dirs;
pub mod fonts;
pub mod log_retention;
pub mod open;
pub mod picker;
