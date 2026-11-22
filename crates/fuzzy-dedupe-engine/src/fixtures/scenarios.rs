// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! Generators for the six T4 scenarios.
//!
//! A group is described as a small template: node 0 is the reference, every
//! other node's content is derived from an earlier node by one [`Op`], and
//! each derivation is a direct-match edge. Optional extra edges join two
//! existing nodes. The builder then computes every measurement and Unique
//! count from the content and assigns counterparts with the domain's
//! `rule_counterpart`, so nothing about the evidence is typed in by hand.
//!
//! Node 0 must be the §6 default reference under the owner's reading B
//! (2026-11-22): the first by path among protected members if any; else
//! among members in the first-listed scan folder (`setup.folders[0]`); else
//! among all members. The hand-written scenarios order their nodes that way;
//! `large` sorts its random nodes by [`reference_rank`] before assigning
//! roles. The contract test checks both against its own implementation.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use fuzzy_dedupe_domain::{
    ContainmentDirection, CounterpartBasis, CounterpartLink, Coverage, EvidenceUpdate, FileId,
    Group, GroupId, IgnoreOptions, IssueKind, MatchEdge, MatchMode, MatchingRules, Measurement,
    Member, MemberRole, Permille, RulesRevision, ScanFolder, ScanId, ScanIssue, ScanResults,
    ScanSetup, SnapshotId, Verification, rule_counterpart,
};

use super::{
    FileContent, FixtureContents, LineEnding, Scenario, ScenarioKind, SeededDecision, SplitMix64,
    TextScript,
};

/// Exactly the row count REQ-020 names.
const LARGE_ROWS: usize = 100_000;

/// 2026-01-01T00:00:00Z; fixture modification times fall in 2026.
const EPOCH_2026: u64 = 1_767_225_600;

/// Illustrative, not an algorithm default (OQ-005). Every generated Similar
/// edge clears it: one replaced line out of 12 to 17 gives 846 to 888.
const MINIMUM_SIMILARITY: u16 = 800;

/// How a node's content derives from an earlier node's, and so which edge
/// joins them.
#[derive(Clone, Copy, Debug)]
enum Op {
    /// Node 0: fresh lines.
    Base { ending: LineEnding },
    /// Same bytes.
    Copy(usize),
    /// Same text: the parent's lines with the other line ending.
    ReEnd(usize),
    /// Similar text: the parent with one line replaced.
    Replace(usize),
    /// Fully inside the parent: a strict slice of it.
    Slice(usize),
    /// Partly inside the parent: a slice plus one line of its own.
    SliceExtra(usize),
    /// Contains the parent: its lines plus new ones. With `repeat`, one new
    /// line occurs three times, so its Unique count counts each occurrence.
    Extend { parent: usize, repeat: bool },
}

/// One node's file identity before roles are known.
#[derive(Clone, Debug)]
struct NodeAttr {
    /// Index into the scan setup's folders.
    root: usize,
    /// Folder below the root, empty for the root itself.
    sub: String,
    name: String,
}

impl NodeAttr {
    fn new(root: usize, sub: &str, name: &str) -> Self {
        Self {
            root,
            sub: sub.to_owned(),
            name: name.to_owned(),
        }
    }
}

fn folder_display(setup: &ScanSetup, attr: &NodeAttr) -> String {
    let root = setup.folders[attr.root].path.to_string_lossy();
    if attr.sub.is_empty() {
        root.into_owned()
    } else {
        format!("{root}/{}", attr.sub)
    }
}

/// Accumulates one scenario.
struct Builder {
    scan: ScanId,
    rng: SplitMix64,
    setup: ScanSetup,
    groups: Vec<Group>,
    contents: FixtureContents,
    next_file: u64,
    next_token: u32,
}

impl Builder {
    fn new(scan: ScanId, seed: u64, folders: &[(&str, bool)]) -> Self {
        Self {
            scan,
            rng: SplitMix64::new(seed),
            setup: ScanSetup {
                folders: folders
                    .iter()
                    .map(|(path, protect)| ScanFolder {
                        path: (*path).into(),
                        protect: *protect,
                    })
                    .collect(),
                file_types: vec!["txt".to_owned(), "md".to_owned()],
            },
            groups: Vec::new(),
            contents: FixtureContents::default(),
            next_file: 1,
            next_token: 1,
        }
    }

    fn fresh_token(&mut self) -> u32 {
        let token = self.next_token;
        self.next_token += 1;
        token
    }

    fn fresh_tokens(&mut self, count: usize) -> Vec<u32> {
        (0..count).map(|_| self.fresh_token()).collect()
    }

    /// Derives one node's content from its op. Every derived file keeps at
    /// least two lines, so a changed line ending always changes the bytes.
    fn derive(&mut self, op: Op, script: TextScript, built: &[FileContent]) -> FileContent {
        let lines_of = |c: &FileContent| match c {
            FileContent::Lines {
                tokens,
                ending,
                final_newline,
                ..
            } => (tokens.clone(), *ending, *final_newline),
            FileContent::Bytes(_) => (Vec::new(), LineEnding::Lf, true),
        };
        let (tokens, ending, final_newline) = match op {
            Op::Base { ending } => {
                let count = self.rng.between(12, 17);
                // One base file in ten lacks a final newline, which §10.1
                // says must never make its last line unique.
                let final_newline = !self.rng.chance(1, 10);
                (self.fresh_tokens(count), ending, final_newline)
            }
            Op::Copy(p) => lines_of(&built[p]),
            Op::ReEnd(p) => {
                let (tokens, ending, final_newline) = lines_of(&built[p]);
                let flipped = match ending {
                    LineEnding::Lf => LineEnding::Crlf,
                    LineEnding::Crlf | LineEnding::Mixed => LineEnding::Lf,
                };
                (tokens, flipped, final_newline)
            }
            Op::Replace(p) => {
                let (mut tokens, ending, final_newline) = lines_of(&built[p]);
                let at = self.rng.below(tokens.len());
                tokens[at] = self.fresh_token();
                (tokens, ending, final_newline)
            }
            Op::Slice(p) | Op::SliceExtra(p) => {
                let (tokens, ending, _) = lines_of(&built[p]);
                let len = self
                    .rng
                    .between((tokens.len() / 2).max(2), tokens.len() - 1);
                let start = self.rng.below(tokens.len() - len + 1);
                let mut slice = tokens[start..start + len].to_vec();
                if matches!(op, Op::SliceExtra(_)) {
                    slice.push(self.fresh_token());
                }
                (slice, ending, true)
            }
            Op::Extend { parent, repeat } => {
                let (tokens, ending, _) = lines_of(&built[parent]);
                let mut out = vec![self.fresh_token()];
                out.extend(tokens);
                out.extend(self.fresh_tokens(2));
                if repeat {
                    let again = self.fresh_token();
                    out.extend([again, again, again]);
                }
                (out, ending, true)
            }
        };
        FileContent::Lines {
            tokens,
            script,
            ending,
            final_newline,
        }
    }

    /// The direct-match edges of one group: one per derived node, joining it
    /// to its parent, plus the template's extra edges. Every measurement and
    /// Unique count is computed from the content.
    fn edges(
        &mut self,
        files: &[FileId],
        contents: &[FileContent],
        ops: &[Op],
        extra: &[(usize, usize)],
    ) -> Vec<MatchEdge> {
        let mut edges = Vec::with_capacity(ops.len() - 1 + extra.len());
        for (child, op) in ops.iter().enumerate().skip(1) {
            let (parent, measurement) = match *op {
                Op::Base { .. } => continue,
                Op::Copy(p) => {
                    // One exact copy in four is still only a hash candidate.
                    let level = if self.rng.chance(1, 4) {
                        Verification::HashCandidate
                    } else {
                        Verification::ByteVerified
                    };
                    (p, Measurement::Identical(level))
                }
                Op::ReEnd(p) => (p, Measurement::SameText),
                Op::Replace(p) => (p, similar(&contents[child], &contents[p])),
                Op::Slice(p) | Op::SliceExtra(p) => (
                    p,
                    Measurement::Containment {
                        coverage: coverage(&contents[child], &contents[p]),
                        direction: ContainmentDirection::InsideCounterpart,
                    },
                ),
                Op::Extend { parent, .. } => (
                    parent,
                    Measurement::Containment {
                        coverage: coverage(&contents[parent], &contents[child]),
                        direction: ContainmentDirection::ContainsCounterpart,
                    },
                ),
            };
            edges.push(edge(files, contents, child, parent, measurement));
        }
        for &(x, y) in extra {
            let measurement = if contents[x].render() == contents[y].render() {
                Measurement::Identical(Verification::ByteVerified)
            } else if tokens(&contents[x]) == tokens(&contents[y]) {
                Measurement::SameText
            } else {
                similar(&contents[x], &contents[y])
            };
            edges.push(edge(files, contents, x, y, measurement));
        }

        edges
    }

    /// Builds one group from a template and returns its files in node order.
    fn group(
        &mut self,
        attrs: &[NodeAttr],
        ops: &[Op],
        extra: &[(usize, usize)],
        script: TextScript,
    ) -> Vec<FileId> {
        let mut contents: Vec<FileContent> = Vec::with_capacity(ops.len());
        for op in ops {
            let content = self.derive(*op, script, &contents);
            contents.push(content);
        }
        let files: Vec<FileId> = (0..ops.len())
            .map(|i| FileId::new(self.next_file + i as u64))
            .collect();
        self.next_file += ops.len() as u64;

        let edges = self.edges(&files, &contents, ops, extra);

        let reference = files[0];
        let mut members: Vec<Member> = attrs
            .iter()
            .zip(&files)
            .zip(&contents)
            .map(|((attr, file), content)| {
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
                let folder = folder_display(&self.setup, attr);
                let seconds = EPOCH_2026 + self.rng.below(330 * 86_400) as u64;
                Member {
                    file: *file,
                    name: attr.name.clone(),
                    display_path: format!("{folder}/{}", attr.name),
                    folder,
                    size_bytes: content.byte_len() as u64,
                    modified: SystemTime::UNIX_EPOCH + Duration::from_secs(seconds),
                    folder_protected: self.setup.folders[attr.root].protect,
                    snapshot: SnapshotId::new(file.get()),
                    role,
                }
            })
            .collect();
        // Reference first, then the strongest match (spec §11.4).
        members[1..].sort_by(|x, y| {
            let (mx, my) = (x.measurement(), y.measurement());
            match (mx, my) {
                (Some(mx), Some(my)) => my.cmp_strength(mx),
                _ => std::cmp::Ordering::Equal,
            }
            .then_with(|| x.file.cmp(&y.file))
        });

        for (file, content) in files.iter().zip(contents) {
            self.contents.insert(*file, content);
        }
        let number = self.groups.len() + 1;
        self.groups.push(Group {
            id: GroupId::new(number as u64),
            label: format!("G{number:03}"),
            members,
            edges,
        });
        files
    }

    fn finish(
        self,
        kind: ScenarioKind,
        seed: u64,
        issues: Vec<ScanIssue>,
        seeded_decisions: Vec<SeededDecision>,
        evidence_changes: Vec<EvidenceUpdate>,
    ) -> Scenario {
        let coverage = if self.groups.is_empty() {
            Coverage::Empty
        } else {
            Coverage::Complete
        };
        Scenario {
            kind,
            seed,
            results: Arc::new(ScanResults {
                scan: self.scan,
                setup: self.setup,
                rules: rules(),
                groups: self.groups,
                issues,
                coverage,
            }),
            contents: self.contents,
            seeded_decisions,
            evidence_changes,
        }
    }
}

fn rules() -> MatchingRules {
    MatchingRules {
        mode: MatchMode::ExactAndFuzzy,
        minimum_similarity: Permille::new(MINIMUM_SIMILARITY).unwrap_or(Permille::FULL),
        ignore: IgnoreOptions {
            whitespace: false,
            line_endings: true,
            letter_case: false,
            punctuation: false,
        },
        revision: RulesRevision::new(1),
    }
}

fn tokens(content: &FileContent) -> &[u32] {
    match content {
        FileContent::Lines { tokens, .. } => tokens,
        FileContent::Bytes(_) => &[],
    }
}

fn distinct(content: &FileContent) -> Vec<u32> {
    let mut out = tokens(content).to_vec();
    out.sort_unstable();
    out.dedup();
    out
}

fn shared(x: &[u32], y: &[u32]) -> usize {
    x.iter().filter(|t| y.binary_search(t).is_ok()).count()
}

fn permille(numerator: usize, denominator: usize) -> Permille {
    let value = (numerator * 1000).checked_div(denominator).unwrap_or(0);
    Permille::new(u16::try_from(value).unwrap_or(1000)).unwrap_or(Permille::FULL)
}

/// Jaccard resemblance of the distinct lines, rounded down.
fn similar(x: &FileContent, y: &FileContent) -> Measurement {
    let (dx, dy) = (distinct(x), distinct(y));
    let both = shared(&dx, &dy);
    Measurement::Similar(permille(both, dx.len() + dy.len() - both))
}

/// Share of `inner`'s distinct lines found in `outer`, rounded down, so it
/// is 1000 exactly when `inner` has no unique line.
fn coverage(inner: &FileContent, outer: &FileContent) -> Permille {
    let (di, d_outer) = (distinct(inner), distinct(outer));
    permille(shared(&di, &d_outer), di.len())
}

/// §10.1 unique lines of `x` against `y`, every occurrence counted. Token
/// equality is line-text equality because rendering is injective.
fn unique(x: &FileContent, y: &FileContent) -> u64 {
    let dy = distinct(y);
    tokens(x)
        .iter()
        .filter(|t| dy.binary_search(t).is_err())
        .count() as u64
}

fn edge(
    files: &[FileId],
    contents: &[FileContent],
    a: usize,
    b: usize,
    measurement: Measurement,
) -> MatchEdge {
    MatchEdge {
        a: files[a],
        b: files[b],
        measurement,
        unique_lines_a: unique(&contents[a], &contents[b]),
        unique_lines_b: unique(&contents[b], &contents[a]),
    }
}

// ── large ────────────────────────────────────────────────────────────────

/// Group templates of `large`, cycled through once before random choice so
/// every shape exists whatever the seed.
#[derive(Clone, Copy, Debug)]
enum Style {
    Exact,
    SameText,
    Similar,
    Containment,
    /// Each member similar only to the previous one: counterpart-chained.
    Chain,
    /// Several relation types in one group.
    Mixed,
    /// A member whose best direct match is not the reference.
    BestMatch,
}

const STYLES: [Style; 7] = [
    Style::Exact,
    Style::SameText,
    Style::Similar,
    Style::Containment,
    Style::Chain,
    Style::Mixed,
    Style::BestMatch,
];

fn style_ops(style: Style, size: usize, ending: LineEnding) -> (Vec<Op>, Vec<(usize, usize)>) {
    let mut ops = vec![Op::Base { ending }];
    let mut extra = Vec::new();
    for node in 1..size {
        let op = match style {
            Style::Exact => Op::Copy(0),
            Style::SameText => Op::ReEnd(0),
            Style::Similar => Op::Replace(0),
            Style::Containment => match node % 3 {
                1 => Op::Slice(0),
                2 => Op::Extend {
                    parent: 0,
                    repeat: node % 2 == 0,
                },
                _ => Op::SliceExtra(0),
            },
            Style::Chain => Op::Replace(node - 1),
            Style::Mixed => match node {
                1 => Op::Copy(0),
                2 => Op::Replace(0),
                3 => Op::SliceExtra(2),
                4 => Op::ReEnd(0),
                _ => Op::Extend {
                    parent: 1,
                    repeat: true,
                },
            },
            // Node 3 copies node 2 and also matches node 1, so its edges are
            // Same bytes with node 2 and Similar with node 1, and neither is
            // the reference.
            Style::BestMatch => match node {
                2 => Op::Replace(1),
                3 => {
                    extra.push((3, 1));
                    Op::Copy(2)
                }
                _ => Op::Replace(0),
            },
        };
        ops.push(op);
    }
    (ops, extra)
}

/// The §6 reading-B clause a member falls in; lower wins, and the path
/// decides within a clause.
const fn reference_rank(protected: bool, root: usize) -> u8 {
    if protected {
        0
    } else if root == 0 {
        1
    } else {
        2
    }
}

/// The first-listed folder sorts after the others by path on purpose, so
/// groups exist where clause 2 (first-listed folder) overrules path order.
const LARGE_FOLDERS: [(&str, bool); 4] = [
    ("/home/fixture/Workspace", false),
    ("/home/fixture/Archive", true),
    ("/home/fixture/Downloads", false),
    ("/home/fixture/Projects", false),
];
const SUBFOLDERS: [&str; 5] = ["Reports", "Meetings", "Drafts", "Specs", ""];
const STEMS: [&str; 8] = [
    "budget",
    "minutes",
    "requirements",
    "notes",
    "draft",
    "report",
    "summary",
    "checklist",
];

pub(super) fn large(scan: ScanId, seed: u64) -> Scenario {
    let mut b = Builder::new(scan, seed, &LARGE_FOLDERS);
    let mut remaining = LARGE_ROWS;
    let mut index = 0usize;
    while remaining > 0 {
        // A group of s members takes s + 1 rows. Near the end the sizes are
        // chosen so the total lands on exactly LARGE_ROWS: 3 to 7 rows left
        // fit one group, 8 to 14 split into two that each fit.
        let size = if remaining <= 7 {
            remaining - 1
        } else if remaining <= 14 {
            remaining / 2 - 1
        } else if index < STYLES.len() {
            5
        } else {
            b.rng.between(2, 6)
        };
        remaining -= size + 1;

        let mut style = if index < STYLES.len() {
            STYLES[index]
        } else {
            STYLES[b.rng.below(STYLES.len())]
        };
        let needs = match style {
            Style::Mixed => 3,
            Style::BestMatch => 4,
            _ => 2,
        };
        if size < needs {
            style = Style::Similar;
        }

        let mut attrs: Vec<NodeAttr> = (0..size)
            .map(|node| {
                let root = match b.rng.below(10) {
                    0..=4 => 0,
                    5 | 6 => 1,
                    7 | 8 => 2,
                    _ => 3,
                };
                let sub = SUBFOLDERS[b.rng.below(SUBFOLDERS.len())];
                let stem = STEMS[b.rng.below(STEMS.len())];
                let ext = if b.rng.chance(1, 4) { "md" } else { "txt" };
                NodeAttr::new(root, sub, &format!("{stem}-{index:06}-{node}.{ext}"))
            })
            .collect();
        // Put the §6 default reference at node 0.
        attrs.sort_by_cached_key(|a| {
            (
                reference_rank(LARGE_FOLDERS[a.root].1, a.root),
                format!("{}/{}", folder_display(&b.setup, a), a.name),
            )
        });
        let ending = if b.rng.chance(1, 5) {
            LineEnding::Crlf
        } else {
            LineEnding::Lf
        };
        let (ops, extra) = style_ops(style, size, ending);
        b.group(&attrs, &ops, &extra, TextScript::Latin);
        index += 1;
    }
    b.finish(
        ScenarioKind::Large,
        seed,
        Vec::new(),
        Vec::new(),
        Vec::new(),
    )
}

// ── mixed ────────────────────────────────────────────────────────────────

const MIXED_FOLDERS: [(&str, bool); 3] = [
    ("/home/fixture/Protected", true),
    ("/home/fixture/Drafts", false),
    ("/home/fixture/Copies", false),
];

pub(super) fn mixed(scan: ScanId, seed: u64) -> Scenario {
    let mut b = Builder::new(scan, seed, &MIXED_FOLDERS);
    let lf = Op::Base {
        ending: LineEnding::Lf,
    };
    // FR-013: the identical pair (nodes 1 and 2) is only similar to the
    // protected reference, and the containment pair (nodes 3 and 4)
    // excludes the reference, so node 4 is measured against node 3.
    b.group(
        &[
            NodeAttr::new(0, "", "Specification.txt"),
            NodeAttr::new(1, "", "Specification_v2.txt"),
            NodeAttr::new(2, "", "Specification_v2 (copy).txt"),
            NodeAttr::new(1, "", "Specification_full.txt"),
            NodeAttr::new(2, "", "Specification_excerpt.txt"),
        ],
        &[
            lf,
            Op::Replace(0),
            Op::Copy(1),
            Op::Replace(1),
            Op::Slice(3),
        ],
        &[(2, 0)],
        TextScript::Latin,
    );
    // FR-013 chain: verified A–B and B–C edges only, no A–C score.
    b.group(
        &[
            NodeAttr::new(1, "Chain", "Minutes.txt"),
            NodeAttr::new(1, "Chain", "Minutes_edited.txt"),
            NodeAttr::new(1, "Chain", "Minutes_final.txt"),
        ],
        &[lf, Op::Replace(0), Op::Replace(1)],
        &[],
        TextScript::Latin,
    );
    // Reading B, clause 3: no protected member and none in the first-listed
    // folder, so the first by path across folders is the reference, though
    // its folder is listed after the other's.
    b.group(
        &[
            NodeAttr::new(2, "", "Notes (Windows).txt"),
            NodeAttr::new(1, "", "Notes.txt"),
        ],
        &[lf, Op::ReEnd(0)],
        &[],
        TextScript::Latin,
    );
    // Plan_v2 copy.txt matches Plan_v2.txt exactly and Plan_v3.txt by
    // similarity, and has no edge to the reference: its counterpart is its
    // best direct match.
    b.group(
        &[
            NodeAttr::new(1, "Plans", "Plan.txt"),
            NodeAttr::new(1, "Plans", "Plan_v2.txt"),
            NodeAttr::new(1, "Plans", "Plan_v2 copy.txt"),
            NodeAttr::new(1, "Plans", "Plan_v3.txt"),
        ],
        &[lf, Op::Replace(0), Op::Copy(1), Op::Replace(2)],
        &[],
        TextScript::Latin,
    );
    b.group(
        &[
            NodeAttr::new(1, "Reports", "Report.txt"),
            NodeAttr::new(1, "Reports", "Report_summary.txt"),
            NodeAttr::new(1, "Reports", "Report_with_appendix.txt"),
            NodeAttr::new(1, "Reports", "Report_notes.txt"),
        ],
        &[
            lf,
            Op::Slice(0),
            Op::Extend {
                parent: 0,
                repeat: true,
            },
            Op::SliceExtra(0),
        ],
        &[],
        TextScript::Latin,
    );
    b.group(
        &[
            NodeAttr::new(2, "", "Contract.txt"),
            NodeAttr::new(1, "", "Contract.txt"),
        ],
        &[lf, Op::Copy(0)],
        &[],
        TextScript::Latin,
    );
    b.finish(
        ScenarioKind::Mixed,
        seed,
        Vec::new(),
        Vec::new(),
        Vec::new(),
    )
}

// ── issues, empty ────────────────────────────────────────────────────────

pub(super) fn issues(scan: ScanId, seed: u64) -> Scenario {
    let mut b = Builder::new(
        scan,
        seed,
        &[
            ("/home/fixture/Shared", false),
            ("/home/fixture/Private", true),
        ],
    );
    let lf = Op::Base {
        ending: LineEnding::Lf,
    };
    b.group(
        &[
            NodeAttr::new(1, "", "Handbook.txt"),
            NodeAttr::new(0, "", "Handbook.txt"),
        ],
        &[lf, Op::Copy(0)],
        &[],
        TextScript::Latin,
    );
    b.group(
        &[
            NodeAttr::new(0, "Team", "Roadmap.md"),
            NodeAttr::new(0, "Team", "Roadmap_old.md"),
            NodeAttr::new(0, "Team", "Roadmap_proposal.md"),
        ],
        &[lf, Op::Replace(0), Op::Replace(0)],
        &[],
        TextScript::Latin,
    );
    let issues = IssueKind::ALL
        .iter()
        .enumerate()
        .map(|(i, kind)| ScanIssue {
            display_path: if *kind == IssueKind::MissingFolder {
                "/home/fixture/Shared/Removed".to_owned()
            } else {
                format!("/home/fixture/Shared/Problems/file-{i}.txt")
            },
            kind: *kind,
        })
        .collect();
    b.finish(ScenarioKind::Issues, seed, issues, Vec::new(), Vec::new())
}

pub(super) fn empty(scan: ScanId, seed: u64) -> Scenario {
    let b = Builder::new(scan, seed, &[("/home/fixture/Unrelated", false)]);
    b.finish(
        ScenarioKind::Empty,
        seed,
        Vec::new(),
        Vec::new(),
        Vec::new(),
    )
}

// ── unicode ──────────────────────────────────────────────────────────────

pub(super) fn unicode(scan: ScanId, seed: u64) -> Scenario {
    let mut b = Builder::new(
        scan,
        seed,
        &[
            ("/home/fixture/文書", false),
            ("/home/fixture/أرشيف", false),
        ],
    );
    b.group(
        &[
            NodeAttr::new(0, "会議", "議事録_ミーティング.txt"),
            NodeAttr::new(1, "", "تقرير_الاجتماع.txt"),
        ],
        &[
            Op::Base {
                ending: LineEnding::Mixed,
            },
            Op::Replace(0),
        ],
        &[],
        TextScript::Unicode,
    );
    // "Cafe\u{301}" is the decomposed (NFD) spelling, which must survive
    // untouched in names even though matching normalizes text to NFC.
    b.group(
        &[
            NodeAttr::new(0, "", "📝 notes ✅.txt"),
            NodeAttr::new(1, "", "Cafe\u{301} menu.txt"),
        ],
        &[
            Op::Base {
                ending: LineEnding::Mixed,
            },
            Op::ReEnd(0),
        ],
        &[],
        TextScript::Unicode,
    );
    let deep = [
        "プロジェクト",
        "二〇二六年度",
        "要件定義",
        "レビュー済み",
        "最終版",
        "アーカイブ",
        "長いフォルダー名の例",
    ]
    .join("/");
    b.group(
        &[
            NodeAttr::new(
                0,
                &deep,
                "非常に長いファイル名の例_要件定義書_最終版_確定.txt",
            ),
            NodeAttr::new(1, "", "نسخة.txt"),
        ],
        &[
            Op::Base {
                ending: LineEnding::Crlf,
            },
            Op::Copy(0),
        ],
        &[],
        TextScript::Unicode,
    );
    b.finish(
        ScenarioKind::Unicode,
        seed,
        Vec::new(),
        Vec::new(),
        Vec::new(),
    )
}

// ── stale ────────────────────────────────────────────────────────────────

/// Offset for injected snapshot IDs, far above any file's initial one.
const CHANGED_SNAPSHOT: u64 = 1_000_000;

pub(super) fn stale(scan: ScanId, seed: u64) -> Scenario {
    let mut b = Builder::new(
        scan,
        seed,
        &[
            ("/home/fixture/Team", false),
            ("/home/fixture/Personal", false),
        ],
    );
    let lf = Op::Base {
        ending: LineEnding::Lf,
    };
    let similar = b.group(
        &[
            NodeAttr::new(0, "", "Proposal.txt"),
            NodeAttr::new(0, "", "Proposal_review.txt"),
            NodeAttr::new(1, "", "Proposal_mine.txt"),
            NodeAttr::new(1, "", "Proposal_old.txt"),
        ],
        &[lf, Op::Replace(0), Op::Replace(0), Op::Replace(0)],
        &[],
        TextScript::Latin,
    );
    let exact = b.group(
        &[
            NodeAttr::new(0, "", "Invoice.txt"),
            NodeAttr::new(1, "", "Invoice.txt"),
            NodeAttr::new(1, "Backup", "Invoice.txt"),
        ],
        &[lf, Op::Copy(0), Op::Copy(0)],
        &[],
        TextScript::Latin,
    );
    let seeded = vec![
        SeededDecision::Queue {
            file: similar[1],
            counterpart: similar[0],
        },
        SeededDecision::Kept(similar[2]),
        SeededDecision::Protect(similar[3]),
        SeededDecision::Queue {
            file: exact[1],
            counterpart: exact[0],
        },
    ];
    let changed = |file: FileId| EvidenceUpdate::FileSnapshot {
        file,
        snapshot: SnapshotId::new(CHANGED_SNAPSHOT + file.get()),
    };
    let evidence = vec![
        // The queued file's counterpart changes: its approval is invalid.
        changed(similar[0]),
        // The Kept file itself changes.
        changed(similar[2]),
        // A repeat of the current value changes nothing.
        EvidenceUpdate::FileSnapshot {
            file: exact[2],
            snapshot: SnapshotId::new(exact[2].get()),
        },
        // A rules change touches every binding.
        EvidenceUpdate::Rules(RulesRevision::new(2)),
    ];
    b.finish(ScenarioKind::Stale, seed, Vec::new(), seeded, evidence)
}
