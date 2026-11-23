// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! The `design` scenario (plan T9, REQ-038): the review frames' sample rows,
//! group labels, decision states, scan progress and cleanup rows, as a
//! fixture. It is the default scenario for snapshots and the design review
//! (T22, T24), which compare the running UI with `docs/design/reference/`.
//! Sample values are fixtures only and set no production default (spec §11.1
//! Prototype limitation).
//!
//! Two things come out of this file. [`build`] returns the C-01 scenario:
//! results, in-memory contents and seeded decisions. [`sample`] returns the
//! frame state C-01 has no field for, such as group titles, the scanning
//! frame's counts and the cleanup popups' item states, keyed by the IDs
//! `build` assigns. Both ignore the caller's seed: the frames fix every value,
//! so any seed reproduces them.
//!
//! The results keep the §6 default reference (owner reading B). Where a
//! frame shows another reference (G002, G005), a `MakeReference` seed
//! precedes the group's other seeds, per the owner decision of 2026-11-22;
//! replaying the seeds in order yields the frame. `Requirements_v2.txt` is
//! protected by a per-file Protect seed, not by its folder.
//!
//! Invented values, all deterministic:
//! - The stored scan setup lists the New scan popup's three folders first
//!   (`~/Documents/Projects` protected, so `Project_Plan.txt` shows
//!   Protected), then folders covering every other file shown. The popup's
//!   own list is the next scan's setup and lives in [`DesignSample`].
//! - Folder segments the frames elide with "…" are chosen so path order picks
//!   each frame's reference.
//! - The 74 undrawn groups and the members of the four collapsed ones bring
//!   the totals to the frames' 84 groups, 236 matched files and per-type
//!   counts. No name or folder contains "guide v9" (frame 01e).
//! - File texts are generated prose, sized so every size displays as the
//!   frame's value. Compare's visible lines (frame `13:1304`) are verbatim;
//!   identical text follows on both sides, and A uses CRLF where B uses LF,
//!   because identical appended text alone cannot reach both sizes and §10.1
//!   compares lines without their terminators.
//!
//! Every measurement and Unique count is computed from the bytes, except the
//! Similar per-mille values, which are the frames' fixture scores. The
//! engine performs no filesystem access (plan D-005); the `~/` paths are
//! display forms only.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use fuzzy_dedupe_domain::{
    ContainmentDirection, CounterpartBasis, CounterpartLink, Coverage, FileId, Group, GroupId,
    IgnoreOptions, MatchEdge, MatchMode, MatchingRules, Measurement, Member, MemberRole, Permille,
    PlanItem, PlanItemStatus, RulesRevision, ScanFolder, ScanId, ScanProgress, ScanResults,
    ScanSetup, SnapshotId, Verification, rule_counterpart,
};

use super::{FileContent, FixtureContents, Scenario, ScenarioKind, SeededDecision};
use crate::service::NotImplemented;

/// Frame state the scenario's results and seeds have no field for, keyed by
/// the IDs [`build`] assigns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DesignSample {
    /// The short title each group header shows in the File name column.
    pub group_titles: BTreeMap<GroupId, String>,
    /// Groups frame 01 (`13:648`) draws expanded; the rest are collapsed.
    pub expanded_groups: Vec<GroupId>,
    /// The highlighted row of frame 01.
    pub focused_file: Option<FileId>,
    /// The pair frame 02 (`13:1304`) compares, which frame 01's G004 header
    /// marks "Last compared".
    pub last_compared: Option<ComparedPair>,
    /// The status bar's "12,648 text files scanned".
    pub files_scanned: u64,
    /// The status bar's "finished 14:32", local time.
    pub finished_at: ClockTime,
    /// Frame 01b (`28:2171`).
    pub scan_progress: ScanProgress,
    /// Frame 01b's "of about 12,600 files".
    pub scan_estimated_files: u64,
    /// Popup New scan (`16:806`): the next scan's setup.
    pub new_scan: NewScanSample,
    /// Popup Review cleanup (`16:886`), in plan order, all `Planned`.
    pub cleanup_items: Vec<PlanItem>,
    /// Item states of Review cleanup — running (`27:1390`).
    pub cleanup_running: Vec<PlanItemStatus>,
    /// Item states of Review cleanup — finished (`27:1533`).
    pub cleanup_finished: Vec<PlanItemStatus>,
    /// Frame 01e's search, which matches no file name or folder.
    pub search_without_matches: String,
}

/// A Compare pair: `a` is the left pane (the reference), `b` the right.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ComparedPair {
    pub group: GroupId,
    pub a: FileId,
    pub b: FileId,
}

/// A wall-clock time of day.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ClockTime {
    pub hour: u8,
    pub minute: u8,
}

/// What the New scan popup lists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewScanSample {
    pub folders: Vec<ScanFolder>,
    /// Extensions shown, without the dot.
    pub file_types_shown: Vec<String>,
    /// The "+12 more" count.
    pub more_file_types: usize,
}

/// Builds the design scenario for scan `scan`. `seed` is recorded but does
/// not change any value (see the module docs).
///
/// # Errors
///
/// Never; the `Result` is the shape [`super::build`] shares with every
/// scenario module.
pub fn build(scan: ScanId, seed: u64) -> Result<Scenario, NotImplemented> {
    Ok(assemble(scan, seed).0)
}

/// The frame state beside the scenario. Its IDs match every [`build`].
#[must_use]
pub fn sample() -> DesignSample {
    assemble(ScanId::new(0), 0).1
}

// ── Text ─────────────────────────────────────────────────────────────────

/// One file's lines. Every file ends in a line terminator.
#[derive(Clone, Debug)]
struct Text {
    lines: Vec<String>,
    crlf: bool,
}

impl Text {
    const fn lf(lines: Vec<String>) -> Self {
        Self { lines, crlf: false }
    }

    const fn ending_len(&self) -> usize {
        if self.crlf { 2 } else { 1 }
    }

    fn size(&self) -> usize {
        self.lines.iter().map(|l| l.len() + self.ending_len()).sum()
    }

    fn bytes(&self) -> Vec<u8> {
        let ending: &[u8] = if self.crlf { b"\r\n" } else { b"\n" };
        let mut out = Vec::with_capacity(self.size());
        for line in &self.lines {
            out.extend_from_slice(line.as_bytes());
            out.extend_from_slice(ending);
        }
        out
    }

    fn nonblank(&self) -> Vec<usize> {
        (0..self.lines.len())
            .filter(|i| !self.lines[*i].is_empty())
            .collect()
    }

    /// Lengthens the lines at `grow` until the file is `target` bytes. Only
    /// lines whose text is already shared, or already unique, are grown, so
    /// no Unique count changes. A file already larger stays as it is, and
    /// the size assertions of `tests/design_scenario.rs` report it.
    fn grow_to(&mut self, grow: &[usize], target: usize) {
        if grow.is_empty() {
            return;
        }
        let delta = target.saturating_sub(self.size());
        let count = grow.len();
        for (k, index) in grow.iter().enumerate() {
            let extra = delta / count + usize::from(k < delta % count);
            let line = &mut self.lines[*index];
            match extra {
                0 => {}
                1 => line.push('.'),
                _ => {
                    line.push(' ');
                    line.push_str(&filler(k * 13 + index, extra - 1));
                }
            }
        }
    }

    /// Drops trailing lines until the file fits in `target` bytes.
    fn truncate_to(&mut self, target: usize) {
        while self.size() > target && !self.lines.is_empty() {
            self.lines.pop();
        }
    }

    /// Appends empty lines up to exactly `target` bytes (LF files). Every
    /// prose text holds empty lines, so they are never unique.
    fn pad_blank_to(&mut self, target: usize) {
        while self.size() + self.ending_len() <= target {
            self.lines.push(String::new());
        }
    }
}

const FILLER: &str = "the team reviewed the open items and agreed on the next steps for each \
                      section while the owners confirmed dates and recorded the remaining \
                      questions for the following meeting ";

/// `len` ASCII characters of running prose that never end in a space.
fn filler(offset: usize, len: usize) -> String {
    let source = FILLER.as_bytes();
    let mut out: String = (0..len)
        .map(|i| char::from(source[(offset + i) % source.len()]))
        .collect();
    if out.ends_with(' ') {
        out.pop();
        out.push('.');
    }
    out
}

/// `count` numbered lines of about `width` characters, with an empty line
/// after every seven. The number makes each non-empty line distinct.
fn prose(tag: &str, count: usize, width: usize) -> Vec<String> {
    (0..count)
        .map(|i| {
            if i % 8 == 7 {
                String::new()
            } else {
                let head = format!("{tag} {}. ", i + 1);
                let rest = width.saturating_sub(head.len()).max(4);
                format!("{head}{}", filler(i * 7, rest))
            }
        })
        .collect()
}

/// The smallest size in bytes that displays as `tenths` tenths of a KiB.
/// It sits at the bottom of the tenth's window, so round-to-nearest and
/// truncation both show the frame's value, and the three cleanup targets
/// (12.8 + 12.8 + 22.4) still total "48.0 KiB": sizes placed mid-window
/// would sum to 48.1.
const fn kib(tenths: usize) -> usize {
    (tenths * 1024).div_ceil(10)
}

// ── §10.1 measures ───────────────────────────────────────────────────────

fn lines_of(bytes: &[u8]) -> Vec<&[u8]> {
    let mut out: Vec<&[u8]> = bytes
        .split(|b| *b == b'\n')
        .map(|l| l.strip_suffix(b"\r").unwrap_or(l))
        .collect();
    if bytes.ends_with(b"\n") || bytes.is_empty() {
        out.pop();
    }
    out
}

fn unique(target: &[u8], counterpart: &[u8]) -> u64 {
    let theirs: std::collections::HashSet<&[u8]> = lines_of(counterpart).into_iter().collect();
    lines_of(target)
        .iter()
        .filter(|l| !theirs.contains(*l))
        .count() as u64
}

/// Share of `inner`'s distinct lines found in `outer`, rounded down, so it is
/// 1000 exactly when `inner` has no unique line.
fn coverage(inner: &[u8], outer: &[u8]) -> Permille {
    let inner: std::collections::BTreeSet<&[u8]> = lines_of(inner).into_iter().collect();
    let outer: std::collections::HashSet<&[u8]> = lines_of(outer).into_iter().collect();
    let shared = inner.iter().filter(|l| outer.contains(*l)).count();
    let value = (shared * 1000).checked_div(inner.len()).unwrap_or(0);
    Permille::new(u16::try_from(value).unwrap_or(1000)).unwrap_or(Permille::FULL)
}

// ── Builder ──────────────────────────────────────────────────────────────

/// How a file's text relates to another's, and so which edge joins them.
#[derive(Clone, Copy, Debug)]
enum Link {
    Identical,
    SameText,
    /// The frame's fixture score in per-mille.
    Similar(u16),
    /// The first file is inside the second.
    Inside,
    /// The first file contains the second.
    Contains,
}

struct FileSpec {
    folder: String,
    name: String,
    modified: SystemTime,
    text: Text,
}

impl FileSpec {
    fn new(folder: &str, name: &str, modified: SystemTime, text: Text) -> Self {
        Self {
            folder: folder.to_owned(),
            name: name.to_owned(),
            modified,
            text,
        }
    }

    fn path(&self) -> String {
        format!("{}/{}", self.folder, self.name)
    }
}

struct Builder {
    setup: ScanSetup,
    groups: Vec<Group>,
    titles: BTreeMap<GroupId, String>,
    contents: FixtureContents,
    next_file: u64,
}

impl Builder {
    /// The scan folder `path` lies under: the first listed whose path is a
    /// prefix, as the contract test resolves it.
    fn root(&self, path: &str) -> Option<usize> {
        self.setup
            .folders
            .iter()
            .position(|f| path.starts_with(&*f.path.to_string_lossy()))
    }

    fn protected(&self, path: &str) -> bool {
        self.root(path)
            .is_some_and(|r| self.setup.folders[r].protect)
    }

    /// The §6 default reference under the owner's reading B: the first by
    /// path among protected files, else among files in the first-listed
    /// folder, else among all.
    fn reference_index(&self, paths: &[String]) -> usize {
        let rank = |p: &String| {
            if self.protected(p) {
                0
            } else if self.root(p) == Some(0) {
                1
            } else {
                2
            }
        };
        (0..paths.len())
            .min_by(|x, y| (rank(&paths[*x]), &paths[*x]).cmp(&(rank(&paths[*y]), &paths[*y])))
            .unwrap_or(0)
    }

    /// Adds one group and returns its files in `files` order.
    fn group(
        &mut self,
        title: &str,
        files: &[FileSpec],
        links: &[(usize, usize, Link)],
    ) -> Vec<FileId> {
        let ids: Vec<FileId> = (0..files.len())
            .map(|i| FileId::new(self.next_file + i as u64))
            .collect();
        self.next_file += files.len() as u64;
        let bytes: Vec<Vec<u8>> = files.iter().map(|f| f.text.bytes()).collect();

        let edges: Vec<MatchEdge> = links
            .iter()
            .map(|&(a, b, link)| {
                let measurement = match link {
                    Link::Identical => Measurement::Identical(Verification::ByteVerified),
                    Link::SameText => Measurement::SameText,
                    Link::Similar(p) => {
                        Measurement::Similar(Permille::new(p).unwrap_or(Permille::FULL))
                    }
                    Link::Inside => Measurement::Containment {
                        coverage: coverage(&bytes[a], &bytes[b]),
                        direction: ContainmentDirection::InsideCounterpart,
                    },
                    Link::Contains => Measurement::Containment {
                        coverage: coverage(&bytes[b], &bytes[a]),
                        direction: ContainmentDirection::ContainsCounterpart,
                    },
                };
                MatchEdge {
                    a: ids[a],
                    b: ids[b],
                    measurement,
                    unique_lines_a: unique(&bytes[a], &bytes[b]),
                    unique_lines_b: unique(&bytes[b], &bytes[a]),
                }
            })
            .collect();

        let paths: Vec<String> = files.iter().map(FileSpec::path).collect();
        let reference = ids[self.reference_index(&paths)];
        let mut members: Vec<Member> = files
            .iter()
            .zip(&ids)
            .zip(&paths)
            .map(|((spec, file), path)| {
                let role = if *file == reference {
                    MemberRole::Reference
                } else {
                    let counterpart =
                        rule_counterpart(&edges, reference, *file).unwrap_or(reference);
                    let found = edges.iter().find(|e| e.other(*file) == Some(counterpart));
                    MemberRole::Member(CounterpartLink {
                        counterpart,
                        basis: CounterpartBasis::Rule,
                        measurement: found.and_then(|e| e.measurement_from(*file)),
                        unique_lines: found.and_then(|e| e.unique_lines_of(*file)).unwrap_or(0),
                    })
                };
                Member {
                    file: *file,
                    name: spec.name.clone(),
                    folder: spec.folder.clone(),
                    display_path: path.clone(),
                    size_bytes: spec.text.size() as u64,
                    modified: spec.modified,
                    folder_protected: self.protected(path),
                    snapshot: SnapshotId::new(file.get()),
                    role,
                }
            })
            .collect();
        // Reference first, then the strongest match (spec §11.4).
        members.sort_by(|x, y| {
            y.is_reference()
                .cmp(&x.is_reference())
                .then_with(|| match (x.measurement(), y.measurement()) {
                    (Some(mx), Some(my)) => my.cmp_strength(mx),
                    _ => std::cmp::Ordering::Equal,
                })
                .then_with(|| x.file.cmp(&y.file))
        });

        for (file, content) in ids.iter().zip(bytes) {
            self.contents
                .insert(*file, FileContent::Bytes(Arc::from(content)));
        }
        let number = self.groups.len() + 1;
        let id = GroupId::new(number as u64);
        self.titles.insert(id, title.to_owned());
        self.groups.push(Group {
            id,
            label: format!("G{number:03}"),
            members,
            edges,
        });
        ids
    }

    /// An undrawn or collapsed group of `size` files whose text and names
    /// derive from `title`. The §6 default reference holds the base text,
    /// and every other file has one edge to it.
    fn invented(&mut self, title: &str, kind: Invented, size: usize, index: usize) -> Vec<FileId> {
        const FOLDERS: [&str; 7] = [
            "~/Documents/Archive",
            "~/Downloads",
            "~/Backups/Documents",
            "~/Documents/Team/Shared",
            "~/Documents/Archive/2025",
            "~/Downloads/Mail attachments",
            "~/Backups/Laptop/Documents",
        ];
        const SUFFIXES: [&str; 4] = ["", "_copy", " (2)", "_old"];
        let stem: String = title
            .split(' ')
            .map(|w| {
                let mut chars = w.chars();
                chars.next().map_or_else(String::new, |c| {
                    c.to_ascii_uppercase().to_string() + chars.as_str()
                })
            })
            .collect::<Vec<_>>()
            .join("_");
        let placed: Vec<(String, String)> = (0..size)
            .map(|m| {
                (
                    FOLDERS[(index * 3 + m) % FOLDERS.len()].to_owned(),
                    format!("{stem}{}.txt", SUFFIXES[m]),
                )
            })
            .collect();
        let paths: Vec<String> = placed.iter().map(|(f, n)| format!("{f}/{n}")).collect();
        let hub = self.reference_index(&paths);

        let base = prose(title, 14 + index % 9, 56);
        let mut derived = Vec::new();
        let texts: Vec<Text> = (0..size)
            .map(|m| {
                if m == hub {
                    return Text::lf(base.clone());
                }
                let (text, link) = match kind {
                    Invented::SameBytes => (Text::lf(base.clone()), Link::Identical),
                    Invented::SameText => (
                        Text {
                            lines: base.clone(),
                            crlf: true,
                        },
                        Link::SameText,
                    ),
                    Invented::Similar => {
                        let mut lines = base.clone();
                        // Lines 1, 3 and 5 are never the empty eighth line.
                        lines[1 + 2 * m] = format!("{title} revision {m}: {}", filler(m * 11, 30));
                        let score = 950 + u16::try_from((index * 7 + m * 13) % 49).unwrap_or(0);
                        (Text::lf(lines), Link::Similar(score))
                    }
                    Invented::Containment => {
                        if m % 2 == 1 {
                            (Text::lf(base[2..base.len() - 2].to_vec()), Link::Inside)
                        } else {
                            let mut lines = base.clone();
                            lines.extend(
                                (1..=3).map(|k| format!("{title} addendum {k}: {}", filler(k, 28))),
                            );
                            (Text::lf(lines), Link::Contains)
                        }
                    }
                };
                derived.push((m, hub, link));
                text
            })
            .collect();
        let files: Vec<FileSpec> = placed
            .into_iter()
            .zip(texts)
            .enumerate()
            .map(|(m, ((folder, name), text))| {
                let month = 1 + u32::try_from((index * 5 + m) % 11).unwrap_or(0);
                let day = 1 + u32::try_from((index * 7 + m * 3) % 27).unwrap_or(0);
                FileSpec::new(&folder, &name, date(2026, month, day), text)
            })
            .collect();
        self.group(title, &files, &derived)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Invented {
    SameBytes,
    SameText,
    Similar,
    Containment,
}

/// Noon UTC on a calendar date, so the displayed date is the frame's in
/// every time zone within twelve hours of UTC. Hinnant's `days_from_civil`.
fn date(year: i64, month: u32, day: u32) -> SystemTime {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let m = i64::from(month);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let seconds = u64::try_from(days).unwrap_or(0) * 86_400 + 12 * 3600;
    SystemTime::UNIX_EPOCH + Duration::from_secs(seconds)
}

// ── The frames ───────────────────────────────────────────────────────────

/// The stored setup of the completed scan. The first three are the New scan
/// popup's folders; folder 0 being the protected Projects folder means
/// reading B's clause 2 never decides a group no protected file decides.
const STORED_FOLDERS: [(&str, bool); 10] = [
    ("~/Documents/Projects", true),
    ("~/Documents/Archive", false),
    ("~/Downloads", false),
    ("~/Documents/Release", false),
    ("~/Documents/Meetings", false),
    ("~/Documents/Team", false),
    ("~/Documents/Specs", false),
    ("~/Documents/Design", false),
    ("~/Documents/Guides", false),
    ("~/Backups", false),
];

const FILE_TYPES: [&str; 6] = ["txt", "md", "rst", "log", "csv", "json"];

/// Frame 02 (`13:1304`) Document A, lines 1 to 34, verbatim.
const COMPARE_A: [&str; 34] = [
    "Design Specification",
    "",
    "1. Overview",
    "This document describes the design for the document",
    "deduplication solution, including core functionality,",
    "data processing, and user experience.",
    "",
    "The system identifies duplicate and near-duplicate",
    "files across one or more folders.",
    "",
    "2. Functional Requirements",
    "2.1 Scan and identify duplicates",
    "The system scans selected locations, compares file content,",
    "and groups files that are exact or similar matches.",
    "",
    "2.2 Review and take action",
    "Users can review matched files, compare differences, and",
    "choose which files to retain or queue for cleanup.",
    "",
    "3. Non-Functional Requirements",
    "The solution is fast, reliable, and easy to use.",
    "",
    "3.1 Local processing",
    "Documents remain on the local filesystem.",
    "Scans never modify the source files.",
    "",
    "3.2 File safety",
    "Protected references cannot be queued for removal.",
    "Changed files must be rescanned before cleanup.",
    "",
    "4. Supported formats",
    "Plain text, Markdown, source and configuration files.",
    "",
    "End of specification.",
];

/// Document B's lines that differ from A: B line 8 replaces A lines 8 and 9,
/// B line 17 replaces A line 18. These are Compare's two changes.
const COMPARE_B_LINE_8: &str = "The system finds near-duplicates within the selected scope.";
const COMPARE_B_LINE_17: &str = "select the version to retain.";

/// Undrawn groups by relation type, before interleaving (G011 to G084).
/// With the ten drawn groups they give frame 01's filters: Same bytes 42,
/// Same text 16, Similar text 20, Containment 6.
const INVENTED_COUNTS: [(Invented, usize); 4] = [
    (Invented::SameBytes, 38),
    (Invented::SameText, 15),
    (Invented::Similar, 16),
    (Invented::Containment, 5),
];

const TOPICS: [&str; 37] = [
    "Vendor contract",
    "Budget summary",
    "Onboarding checklist",
    "Travel policy",
    "Product roadmap",
    "Incident report",
    "Team charter",
    "Interview notes",
    "Press release",
    "Training plan",
    "Security review",
    "Expense report",
    "Style sheet",
    "Data retention",
    "Office move",
    "Grant proposal",
    "Partner agreement",
    "Quarterly review",
    "Hiring plan",
    "Release checklist",
    "Customer feedback",
    "Risk register",
    "Status update",
    "Workshop agenda",
    "Research summary",
    "Service levels",
    "Supplier list",
    "Board minutes",
    "Inventory count",
    "Event plan",
    "Support macros",
    "Pricing notes",
    "Accessibility audit",
    "Migration plan",
    "Backup schedule",
    "Volunteer roster",
    "Annual report",
];

#[expect(
    clippy::too_many_lines,
    reason = "one straight-line transcription of the frames, group by group; splitting it would \
              scatter the frame-to-value mapping the comments document"
)]
fn assemble(scan: ScanId, seed: u64) -> (Scenario, DesignSample) {
    let mut b = Builder {
        setup: ScanSetup {
            folders: STORED_FOLDERS
                .iter()
                .map(|(path, protect)| ScanFolder {
                    path: (*path).into(),
                    protect: *protect,
                })
                .collect(),
            file_types: FILE_TYPES.iter().map(|t| (*t).to_owned()).collect(),
        },
        groups: Vec::new(),
        titles: BTreeMap::new(),
        contents: FixtureContents::default(),
        next_file: 1,
    };
    let mut seeds = Vec::new();

    // G001, frame 01 rows `24:990` to `24:1052`: three identical copies,
    // the reference protected by its folder.
    let mut plan = Text::lf(prose("Project plan", 170, 64));
    let grow = plan.nonblank();
    plan.grow_to(&grow, kib(128));
    let g001 = b.group(
        "Project plan",
        &[
            FileSpec::new(
                "~/Documents/Projects",
                "Project_Plan.txt",
                date(2026, 3, 14),
                plan.clone(),
            ),
            FileSpec::new(
                "~/Documents/Archive",
                "Project_Plan_copy.txt",
                date(2026, 3, 14),
                plan.clone(),
            ),
            FileSpec::new(
                "~/Backups/Documents",
                "Project_Plan (2).txt",
                date(2026, 3, 14),
                plan,
            ),
        ],
        &[(1, 0, Link::Identical), (2, 0, Link::Identical)],
    );
    seeds.push(SeededDecision::Queue {
        file: g001[1],
        counterpart: g001[0],
    });
    seeds.push(SeededDecision::Queue {
        file: g001[2],
        counterpart: g001[0],
    });

    // G002, rows `24:1100` to `24:1189`. Reading B picks
    // Requirements_draft.txt (`Specs/Old/` sorts before
    // `Specs/Requirements.txt`), so a MakeReference seed restores the
    // frame's reference. Requirements_final.txt has no edge to the
    // reference; its counterpart is Requirements_v2.txt under either
    // reference, and so is v2's per-file Protect binding (C-05 I-7), which
    // is why the Protect seed survives the reference change.
    let mut req = Text::lf(prose("Requirement", 260, 60));
    let grow = req.nonblank();
    req.grow_to(&grow, kib(184));
    let mut draft = req.clone();
    let changed = [9, 41, 97, 150];
    for (k, at) in changed.iter().enumerate() {
        draft.lines[*at] = format!("Draft change {}: scope narrowed", k + 1);
    }
    draft.truncate_to(kib(181));
    draft.grow_to(&changed, kib(181));
    let mut v2 = Text::lf(Vec::new());
    let mut added = Vec::new();
    for (i, line) in req.lines.iter().enumerate() {
        v2.lines.push(line.clone());
        if [20, 50, 80, 110, 140, 170, 200, 230, 250].contains(&i) {
            added.push(v2.lines.len());
            v2.lines
                .push(format!("Version 2 addition {}: new clause", added.len()));
        }
    }
    v2.grow_to(&added, kib(189));
    let mut last = v2.clone();
    last.truncate_to(kib(172));
    last.pad_blank_to(kib(172));
    let g002 = b.group(
        "Requirements",
        &[
            FileSpec::new(
                "~/Documents/Specs",
                "Requirements.txt",
                date(2026, 6, 2),
                req,
            ),
            FileSpec::new(
                "~/Documents/Specs/Old",
                "Requirements_draft.txt",
                date(2026, 5, 20),
                draft,
            ),
            FileSpec::new(
                "~/Documents/Team/Specs",
                "Requirements_v2.txt",
                date(2026, 5, 28),
                v2,
            ),
            FileSpec::new(
                "~/Documents/Team/Specs/2024",
                "Requirements_final.txt",
                date(2024, 11, 8),
                last,
            ),
        ],
        &[
            (1, 0, Link::Similar(987)),
            (2, 0, Link::Similar(971)),
            (3, 2, Link::Similar(958)),
        ],
    );
    seeds.push(SeededDecision::Protect(g002[2]));
    seeds.push(SeededDecision::MakeReference(g002[0]));

    // G003, rows `24:1234`, `24:1265`: the copy differs only in line
    // endings, so it is one byte per line larger (6.2 and 6.3 KiB).
    let mut notes = Text::lf(prose("Meeting note", 100, 52));
    let grow = notes.nonblank();
    // 26 bytes above the 6.2 KiB floor, so the 100 extra CR bytes of the
    // copy reach the 6.3 KiB window and stay in its lower half.
    notes.grow_to(&grow, kib(62) + 26);
    let notes_crlf = Text {
        lines: notes.lines.clone(),
        crlf: true,
    };
    let g003 = b.group(
        "Meeting notes",
        &[
            FileSpec::new(
                "~/Documents/Meetings",
                "Meeting_Notes.md",
                date(2026, 5, 6),
                notes,
            ),
            FileSpec::new(
                "~/Documents/Team/Meetings/2024-05",
                "Meeting_Notes_copy.md",
                date(2026, 5, 6),
                notes_crlf,
            ),
        ],
        &[(1, 0, Link::SameText)],
    );
    seeds.push(SeededDecision::Kept(g003[1]));

    // G004, rows `24:1310` to `24:1369`, and frame 02. A and B share the
    // appendix after the visible lines, so Compare keeps exactly its two
    // changes and its 3 and 2 unique lines. A (CRLF) is sized to 12.8 KiB;
    // B (LF) then lands on 12.6 KiB, which the test checks.
    let appendix = prose("Appendix note", 150, 80);
    let mut spec_a = Text {
        lines: COMPARE_A
            .iter()
            .map(|l| (*l).to_owned())
            .chain(appendix)
            .collect(),
        crlf: true,
    };
    let grow: Vec<usize> = spec_a
        .nonblank()
        .into_iter()
        .filter(|i| *i >= COMPARE_A.len())
        .collect();
    spec_a.grow_to(&grow, 13_150);
    let mut b_lines: Vec<String> = Vec::new();
    for (i, line) in spec_a.lines.iter().enumerate() {
        match i {
            7 => b_lines.push(COMPARE_B_LINE_8.to_owned()),
            8 => {}
            17 => b_lines.push(COMPARE_B_LINE_17.to_owned()),
            _ => b_lines.push(line.clone()),
        }
    }
    let spec_b = Text::lf(b_lines);
    let mut rev = Text::lf(Vec::new());
    let mut notes_at = Vec::new();
    for (i, line) in spec_a.lines.iter().enumerate() {
        rev.lines.push(line.clone());
        if [44, 74, 104, 134, 164].contains(&i) {
            notes_at.push(rev.lines.len());
            rev.lines
                .push(format!("Revision note {}: wording", notes_at.len()));
        }
    }
    rev.grow_to(&notes_at, kib(130));
    let g004 = b.group(
        "Design specification",
        &[
            FileSpec::new(
                "~/Documents/Design",
                "Design_Spec.txt",
                date(2026, 5, 2),
                spec_a,
            ),
            FileSpec::new(
                "~/Documents/Design/Drafts",
                "Design_Spec_draft.txt",
                date(2026, 4, 18),
                spec_b,
            ),
            FileSpec::new(
                "~/Documents/Design/Reviews/Archive",
                "Design_Spec_rev.txt",
                date(2026, 4, 27),
                rev,
            ),
        ],
        &[(1, 0, Link::Similar(979)), (2, 0, Link::Similar(964))],
    );

    // G005, rows `24:1414`, `24:1445`. Reading B picks the copy (`Old/`
    // sorts before `Release_Notes.txt`); the MakeReference seed restores the
    // frame's reference before the copy is queued against it.
    let mut release = Text::lf(prose("Release note", 300, 64));
    let grow = release.nonblank();
    release.grow_to(&grow, kib(224));
    let g005 = b.group(
        "Release notes",
        &[
            FileSpec::new(
                "~/Documents/Release",
                "Release_Notes.txt",
                date(2026, 7, 11),
                release.clone(),
            ),
            FileSpec::new(
                "~/Documents/Release/Old",
                "Release_Notes_copy.txt",
                date(2026, 7, 11),
                release,
            ),
        ],
        &[(1, 0, Link::Identical)],
    );
    seeds.push(SeededDecision::MakeReference(g005[0]));
    seeds.push(SeededDecision::Queue {
        file: g005[1],
        counterpart: g005[0],
    });

    // G006 to G008 and G010 are collapsed in frame 01 (`24:1476`,
    // `24:1493`, `24:1510`, `24:1631`): only label, size, type and title
    // are drawn.
    b.invented("User guide", Invented::Similar, 3, 6);
    b.invented("Installation notes", Invented::SameBytes, 2, 7);
    b.invented("Support procedures", Invented::Similar, 3, 8);

    // G009, rows `24:1544` to `24:1603`: a full excerpt and an extension
    // with 41 lines of its own; the extension also repeats an opening
    // passage of the guide, which §10.1 does not count as unique.
    let mut guide = Text::lf(prose("Guide section", 640, 70));
    let grow = guide.nonblank();
    guide.grow_to(&grow, kib(480));
    let mut excerpt = Text::lf(guide.lines[96..240].to_vec());
    excerpt.truncate_to(kib(96));
    excerpt.pad_blank_to(kib(96));
    let mut extended = guide.clone();
    extended.lines.extend(guide.lines[..60].iter().cloned());
    let own: Vec<usize> = (0..41).map(|k| extended.lines.len() + k).collect();
    extended
        .lines
        .extend((1..=41).map(|k| format!("Extended topic {k}: local notes")));
    extended.grow_to(&own, kib(551));
    b.group(
        "Guide excerpt",
        &[
            FileSpec::new("~/Documents/Guides", "Guide.txt", date(2026, 2, 10), guide),
            FileSpec::new(
                "~/Documents/Guides/Training/Extracts",
                "Guide_excerpt.txt",
                date(2026, 2, 12),
                excerpt,
            ),
            FileSpec::new(
                "~/Downloads",
                "Guide_extended.txt",
                date(2026, 8, 30),
                extended,
            ),
        ],
        &[(1, 0, Link::Inside), (2, 0, Link::Contains)],
    );

    b.invented("Configuration reference", Invented::SameBytes, 2, 10);

    // G011 to G084, interleaved by type. Same bytes groups have three files
    // and the first two four, the others two (Same text) or three, which
    // brings the matched files to 236.
    let mut order: Vec<(u64, Invented, usize)> = INVENTED_COUNTS
        .iter()
        .flat_map(|&(kind, n)| {
            (0..n).map(move |i| ((2 * i as u64 + 1) * 1_000_000 / (2 * n as u64), kind, i))
        })
        .collect();
    order.sort_by_key(|(key, _, _)| *key);
    for (k, (_, kind, i)) in order.into_iter().enumerate() {
        let index = 11 + k;
        let topic = TOPICS[k % TOPICS.len()];
        let title = if k < TOPICS.len() {
            topic.to_owned()
        } else {
            format!("{topic} archive")
        };
        let size = match kind {
            Invented::SameBytes if i < 2 => 4,
            Invented::SameText => 2,
            _ => 3,
        };
        b.invented(&title, kind, size, index);
    }

    let by_label = |label: &str| {
        b.groups
            .iter()
            .find(|g| g.label == label)
            .map_or(GroupId::new(0), |g| g.id)
    };
    let size_of = |file: FileId| {
        b.groups
            .iter()
            .flat_map(|g| &g.members)
            .find(|m| m.file == file)
            .map_or(0, |m| m.size_bytes)
    };
    let cleanup_items = [(g001[1], g001[0]), (g001[2], g001[0]), (g005[1], g005[0])]
        .into_iter()
        .map(|(target, counterpart)| PlanItem {
            target,
            counterpart,
            size_bytes: size_of(target),
            status: PlanItemStatus::Planned,
            reason: None,
        })
        .collect();
    let sample = DesignSample {
        group_titles: b.titles.clone(),
        expanded_groups: ["G001", "G005", "G003", "G002", "G004", "G009"]
            .into_iter()
            .map(by_label)
            .collect(),
        focused_file: Some(g004[1]),
        last_compared: Some(ComparedPair {
            group: by_label("G004"),
            a: g004[0],
            b: g004[1],
        }),
        files_scanned: 12_648,
        finished_at: ClockTime {
            hour: 14,
            minute: 32,
        },
        scan_progress: ScanProgress {
            current_folder: "~/Documents/Archive".to_owned(),
            files_read: 8_214,
            groups_found: 61,
            elapsed: Duration::from_secs(42),
        },
        scan_estimated_files: 12_600,
        new_scan: NewScanSample {
            folders: STORED_FOLDERS[..3]
                .iter()
                .map(|(path, protect)| ScanFolder {
                    path: (*path).into(),
                    protect: *protect,
                })
                .collect(),
            file_types_shown: FILE_TYPES.iter().map(|t| (*t).to_owned()).collect(),
            more_file_types: 12,
        },
        cleanup_items,
        cleanup_running: vec![
            PlanItemStatus::Succeeded,
            PlanItemStatus::Executing,
            PlanItemStatus::Approved,
        ],
        cleanup_finished: vec![PlanItemStatus::Succeeded; 3],
        search_without_matches: "guide v9".to_owned(),
    };

    let scenario = Scenario {
        kind: ScenarioKind::Design,
        seed,
        results: Arc::new(ScanResults {
            scan,
            setup: b.setup,
            // Popup Matching rules `16:735`: Exact + fuzzy text, 95.0 %,
            // whitespace and line-ending differences ignored.
            rules: MatchingRules {
                mode: MatchMode::ExactAndFuzzy,
                minimum_similarity: Permille::new(950).unwrap_or(Permille::FULL),
                ignore: IgnoreOptions {
                    whitespace: true,
                    line_endings: true,
                    letter_case: false,
                    punctuation: false,
                },
                revision: RulesRevision::new(1),
            },
            groups: b.groups,
            issues: Vec::new(),
            coverage: Coverage::Complete,
        }),
        contents: b.contents,
        seeded_decisions: seeds,
        evidence_changes: Vec::new(),
    };
    (scenario, sample)
}
