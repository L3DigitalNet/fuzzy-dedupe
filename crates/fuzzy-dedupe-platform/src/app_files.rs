// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! The app-owned writer (C-07, C-09): the only place in the workspace that
//! creates directories, creates files or renames, and only inside the
//! application's own [`AppDirs`] configuration and state directories.
//!
//! Callers are the preferences store (`preferences.toml`, DR-009) and the
//! logging setup, which needs the log directory to exist. Reading needs no
//! adapter: `std::fs::read` and `File::open` are not C-09 routes.
//!
//! Confinement policy (owner decision 2026-11-23, "trust the root, no links
//! below"):
//!
//! - **Trusted:** the XDG base directories and the two app roots
//!   (`<config>/fuzzy-dedupe`, `<state>/fuzzy-dedupe`) are the user's own
//!   configuration and may be symbolic links, so dotfile managers such as
//!   GNU Stow keep working. A root is created with `create_dir_all`, which
//!   follows such links.
//! - **Below a root, no link is followed.** A target must first pass a
//!   lexical check: an absolute path with no `.` or `..` component, strictly
//!   below a root, so no spelling of the path can climb out. Then every
//!   directory component under the root, and the target file itself, is
//!   inspected with `symlink_metadata` (which describes the entry, never what
//!   it points to) immediately before each create, write and rename. A link
//!   or wrong file type there is refused with [`AppFileError::UnsafePath`]
//!   and nothing further is created or written. The temporary file and its
//!   rename stay in that one checked parent directory.
//! - **The temporary file is new by construction.** It is opened with
//!   `create_new` (`O_CREAT | O_EXCL`), which fails on any existing entry,
//!   a dangling symbolic link included, so the open itself is the no-follow
//!   guarantee for it. The bytes are then written through that handle, never
//!   by path, and the path is re-checked no-follow before the rename.
//!
//! Threat model: these checks stop a link that is already in place, such as
//! one left by another tool or a restored backup. They do not stop another
//! process running as the same user from swapping a checked directory for a
//! link between the check and the operation. That is out of scope: such a
//! process can already modify these files, and the user's own files,
//! directly. Closing the race would need operations anchored on directory
//! handles (`openat` and `renameat`), which `std` lacks and no approved spec
//! §8.3 crate provides.
//!
//! C-09 allowlists this file for the write, create, open and rename entries
//! only; each function that reaches one carries its own item-level
//! `expect`, so an entry used anywhere else in the file fails Clippy.

use std::ffi::OsString;
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::dirs::AppDirs;

/// Why the app-owned writer refused or failed a request.
#[derive(Debug)]
pub enum AppFileError {
    /// The path is relative, contains a `.` or `..` component, or does not
    /// lie below the configuration or state directory. Nothing was touched.
    OutsideAppDirs(PathBuf),
    /// An existing entry below the app root, either a directory on the way
    /// or the target itself, is a symbolic link or the wrong file type, so
    /// using it could leave the app directory. The entry was not followed
    /// and no further entry was created or written.
    UnsafePath(PathBuf),
    /// The filesystem rejected an operation on this path.
    Io { path: PathBuf, source: io::Error },
}

impl std::fmt::Display for AppFileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OutsideAppDirs(p) => write!(
                f,
                "{} is outside the application's own directories",
                p.display()
            ),
            Self::UnsafePath(p) => write!(
                f,
                "{} is a link or not a folder, so it was not followed",
                p.display()
            ),
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
        }
    }
}

impl std::error::Error for AppFileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

fn io_err(path: &Path) -> impl FnOnce(io::Error) -> AppFileError + '_ {
    move |source| AppFileError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// Writer confined to one [`AppDirs`]' configuration and state directories.
#[derive(Debug, Clone)]
pub struct AppFiles {
    roots: [PathBuf; 2],
}

impl AppFiles {
    #[must_use]
    pub fn new(dirs: &AppDirs) -> Self {
        Self {
            roots: [
                dirs.config_dir().to_path_buf(),
                dirs.state_dir().to_path_buf(),
            ],
        }
    }

    /// Replace `path` with `contents` atomically, creating missing parent
    /// directories inside the app directory.
    ///
    /// The bytes go to a new temporary file beside the target, are flushed
    /// to disk, and the temporary file is then renamed over the target, so a
    /// reader sees either the whole old file or the whole new one, never a
    /// partial write. The target must be absent or a regular file; a link
    /// or directory there is refused, not replaced.
    ///
    /// # Errors
    ///
    /// [`AppFileError::OutsideAppDirs`] or [`AppFileError::UnsafePath`]
    /// before anything is written; [`AppFileError::Io`] when the filesystem
    /// fails. A failure after the temporary file was created leaves that
    /// file (`.<name>.<pid>.<n>.tmp`) behind: this module may not delete
    /// (C-09 confines deletion to `log_retention.rs`), and the unique name
    /// keeps it from colliding with a later write.
    #[expect(
        clippy::disallowed_methods,
        reason = "app-owned writer: the atomic replace renames its own temporary file over a file inside the app directories (C-09 allowlist)"
    )]
    pub fn write_atomic(&self, path: &Path, contents: &[u8]) -> Result<(), AppFileError> {
        let (root, rel) = self.locate(path)?;
        let (Some(name), Some(parent)) = (rel.file_name(), path.parent()) else {
            // `locate` returned a non-empty relative part of Normal
            // components, so both exist; refusing keeps this total anyway.
            return Err(AppFileError::OutsideAppDirs(path.to_path_buf()));
        };
        let rel_parent = rel.parent().unwrap_or(Path::new(""));
        create_within(root, rel_parent)?;
        // Checked before the temporary file exists, so a refused target
        // leaves nothing behind.
        check_target(path)?;

        let temp = parent.join(temp_name(name));
        check_below(root, rel_parent)?;
        write_new(&temp, contents)?;
        // Re-checked right before the rename, the step that publishes the
        // file: the parent chain, the target and the temporary file itself.
        check_below(root, rel_parent)?;
        check_target(path)?;
        check_entry(&temp, EntryKind::File)?;
        std::fs::rename(&temp, path).map_err(io_err(path))?;
        sync_dir(parent);
        Ok(())
    }

    /// Create `dir`, and any missing directory between it and the app
    /// directory it lies in. `dir` may be the configuration or state
    /// directory itself.
    ///
    /// # Errors
    ///
    /// As [`AppFiles::write_atomic`], except that nothing is left behind.
    pub fn ensure_dir(&self, dir: &Path) -> Result<(), AppFileError> {
        let (root, rel) = self.locate_dir(dir)?;
        create_within(root, rel)
    }

    /// The root `path` lies strictly below, and `path` relative to it.
    fn locate<'p>(&self, path: &'p Path) -> Result<(&Path, &'p Path), AppFileError> {
        match self.locate_dir(path)? {
            (_, rel) if rel.as_os_str().is_empty() => {
                Err(AppFileError::OutsideAppDirs(path.to_path_buf()))
            }
            found => Ok(found),
        }
    }

    /// The root `path` lies at or below, and `path` relative to it.
    fn locate_dir<'p>(&self, path: &'p Path) -> Result<(&Path, &'p Path), AppFileError> {
        // `Path::starts_with` compares whole components, so `config-other`
        // is not inside `config`. Rejecting `.` and `..` first is what makes
        // that comparison sufficient: `starts_with` is lexical and would
        // accept `config/../elsewhere`.
        let clean = path.is_absolute()
            && path
                .components()
                .all(|c| !matches!(c, Component::CurDir | Component::ParentDir));
        clean
            .then(|| {
                self.roots
                    .iter()
                    .find_map(|root| Some((root.as_path(), path.strip_prefix(root).ok()?)))
            })
            .flatten()
            .ok_or_else(|| AppFileError::OutsideAppDirs(path.to_path_buf()))
    }
}

/// Create the trusted `root` and then each component of `rel` below it,
/// checking each component no-follow immediately before it is used or
/// created, and refusing any existing entry that is not a real directory.
#[expect(
    clippy::disallowed_methods,
    reason = "app-owned writer: creates the app's own configuration and state directories (C-09 allowlist)"
)]
fn create_within(root: &Path, rel: &Path) -> Result<(), AppFileError> {
    std::fs::create_dir_all(root).map_err(io_err(root))?;
    let mut current = root.to_path_buf();
    for component in rel.components() {
        current.push(component);
        // `symlink_metadata` describes the entry itself, so a link reports
        // as a link here instead of as the directory it points to.
        match std::fs::symlink_metadata(&current) {
            Ok(meta) if meta.is_dir() => {}
            Ok(_) => return Err(AppFileError::UnsafePath(current)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                match std::fs::create_dir(&current) {
                    Ok(()) => {}
                    // Another writer created it between the two calls; the
                    // next iteration's check would not see it, so check now.
                    Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                        let meta = std::fs::symlink_metadata(&current).map_err(io_err(&current))?;
                        if !meta.is_dir() {
                            return Err(AppFileError::UnsafePath(current));
                        }
                    }
                    Err(e) => return Err(io_err(&current)(e)),
                }
            }
            Err(e) => return Err(io_err(&current)(e)),
        }
    }
    Ok(())
}

/// Which file type [`check_entry`] requires.
#[derive(Clone, Copy, PartialEq, Eq)]
enum EntryKind {
    Dir,
    File,
}

/// Require `path` to exist as `kind`, judged by its own file type: a link
/// is never followed, so a link to a directory is not a directory here.
fn check_entry(path: &Path, kind: EntryKind) -> Result<(), AppFileError> {
    let file_type = std::fs::symlink_metadata(path)
        .map_err(|e| {
            if e.kind() == io::ErrorKind::NotFound {
                // It existed a moment ago (it was created or checked);
                // vanishing now means the tree changed under us.
                AppFileError::UnsafePath(path.to_path_buf())
            } else {
                io_err(path)(e)
            }
        })?
        .file_type();
    let ok = match kind {
        EntryKind::Dir => file_type.is_dir(),
        EntryKind::File => file_type.is_file(),
    };
    if ok {
        Ok(())
    } else {
        Err(AppFileError::UnsafePath(path.to_path_buf()))
    }
}

/// Re-check, no-follow, that every component of `rel` below the trusted
/// `root` is still a real directory.
fn check_below(root: &Path, rel: &Path) -> Result<(), AppFileError> {
    let mut current = root.to_path_buf();
    for component in rel.components() {
        current.push(component);
        check_entry(&current, EntryKind::Dir)?;
    }
    Ok(())
}

/// The rename target must be absent or a regular file. A link there would
/// otherwise be replaced silently, and a directory would fail the rename
/// only after a temporary file had been left beside it.
fn check_target(path: &Path) -> Result<(), AppFileError> {
    match std::fs::symlink_metadata(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(io_err(path)(e)),
        Ok(meta) if meta.file_type().is_file() => Ok(()),
        Ok(_) => Err(AppFileError::UnsafePath(path.to_path_buf())),
    }
}

/// Write `contents` to a file that must not exist yet, and flush it to disk
/// before returning, so the rename that follows never publishes a file
/// whose data is still only in the page cache.
#[expect(
    clippy::disallowed_methods,
    reason = "app-owned writer: creates its own temporary file inside the app directories (C-09 allowlist)"
)]
fn write_new(path: &Path, contents: &[u8]) -> Result<(), AppFileError> {
    // `create_new` fails instead of reusing an existing entry, so a link
    // planted at the temporary name is never written through.
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(io_err(path))?;
    file.write_all(contents).map_err(io_err(path))?;
    file.sync_all().map_err(io_err(path))
}

/// A temporary name beside `name`, unique within this process and across
/// concurrent instances by process ID.
fn temp_name(name: &std::ffi::OsStr) -> OsString {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let mut temp = OsString::from(".");
    temp.push(name);
    temp.push(format!(
        ".{}.{}.tmp",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    temp
}

/// Flush the directory entry the rename changed, best effort.
///
/// Without it a crash shortly after the rename can bring back the old file
/// on some filesystems; the replace is still atomic either way, so a
/// failure here is not reported. Windows cannot open a directory as a file,
/// hence Unix only.
fn sync_dir(dir: &Path) {
    #[cfg(unix)]
    if let Ok(handle) = std::fs::File::open(dir) {
        let _ = handle.sync_all();
    }
    #[cfg(not(unix))]
    let _ = dir;
}
