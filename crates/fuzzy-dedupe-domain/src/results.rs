// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! The results model: groups, members, direct-match edges and the evidence
//! each row shows (spec §6, §10.1 "Grouping policy", §11.4).
//!
//! Pairwise evidence lives on [`MatchEdge`]s, one per direct match the scan
//! found. A member's [`CounterpartLink`] repeats the evidence of the one edge
//! its row is measured against, so a row can be drawn without a search, and
//! the edges stay available for recomputing counterparts after Make this the
//! reference. Files joined only through an intermediate member have no edge
//! and therefore never receive an invented direct score.

use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::time::SystemTime;

use crate::ids::{FileId, GroupId, ScanId, SnapshotId};
use crate::rules::MatchingRules;
use crate::setup::ScanSetup;

/// A value in thousandths, 0 to 1000 inclusive. Per-mille rather than a
/// float so results compare exactly and display one decimal of a percentage
/// ("98.7%") without rounding drift.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Permille(u16);

impl Permille {
    pub const FULL: Self = Self(1000);

    /// `None` above 1000.
    #[must_use]
    pub const fn new(value: u16) -> Option<Self> {
        if value <= 1000 {
            Some(Self(value))
        } else {
            None
        }
    }

    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

/// The relation type a row or a group header shows (spec §11.4 Match).
/// Declaration order is the default match-type sort: identical, same text,
/// similar, then containment.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RelationKind {
    SameBytes,
    SameText,
    SimilarText,
    Containment,
}

/// How far exact-byte evidence went (spec §10.1). The displayed label and
/// this state must agree (§11.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Verification {
    /// Full-file hashes are equal; bytes not yet compared.
    HashCandidate,
    /// Bytes compared equal.
    ByteVerified,
}

/// Direction of a containment, from the measured file's point of view.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ContainmentDirection {
    /// "100% inside reference": this file's text is inside its counterpart.
    InsideCounterpart,
    /// "Contains the reference": the counterpart's text is inside this file.
    ContainsCounterpart,
}

/// One file's direct match with another, as seen from the first file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Measurement {
    /// Same bytes. The verification level travels with the match, because it
    /// describes this pair's evidence, not either file alone.
    Identical(Verification),
    /// Equal under the scan's normalization profile; original bytes differ.
    SameText,
    /// Resemblance in thousandths, never 1000 (that would be Same text).
    Similar(Permille),
    /// `coverage` is the share of the inner file found in the outer one,
    /// whichever side this file is on; `direction` says which side that is.
    Containment {
        coverage: Permille,
        direction: ContainmentDirection,
    },
}

impl Measurement {
    #[must_use]
    pub const fn relation(&self) -> RelationKind {
        match self {
            Self::Identical(_) => RelationKind::SameBytes,
            Self::SameText => RelationKind::SameText,
            Self::Similar(_) => RelationKind::SimilarText,
            Self::Containment { .. } => RelationKind::Containment,
        }
    }

    /// The same match seen from the other file: containment flips direction,
    /// every other measurement is symmetric.
    #[must_use]
    pub const fn mirrored(self) -> Self {
        match self {
            Self::Containment {
                coverage,
                direction: ContainmentDirection::InsideCounterpart,
            } => Self::Containment {
                coverage,
                direction: ContainmentDirection::ContainsCounterpart,
            },
            Self::Containment {
                coverage,
                direction: ContainmentDirection::ContainsCounterpart,
            } => Self::Containment {
                coverage,
                direction: ContainmentDirection::InsideCounterpart,
            },
            other => other,
        }
    }

    /// Orders matches by strength for "best direct match" (spec §6, §10.1)
    /// and the in-group member order (§11.4). The spec leaves the order open;
    /// this is the owner's decision of 2026-11-22: match type Same bytes >
    /// Same text > Similar > Containment, then the higher per-mille within a
    /// type. Direction does not affect strength. `Greater` means `self` is
    /// stronger; [`rule_counterpart`] breaks remaining ties by lower file ID.
    #[must_use]
    pub fn cmp_strength(&self, other: &Self) -> Ordering {
        self.strength_key().cmp(&other.strength_key())
    }

    const fn strength_key(self) -> (u8, u16) {
        match self {
            Self::Identical(_) => (3, 1000),
            Self::SameText => (2, 1000),
            Self::Similar(p) => (1, p.get()),
            Self::Containment { coverage, .. } => (0, coverage.get()),
        }
    }
}

/// One direct match the scan found between two members of a group. Stored
/// once per unordered pair.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MatchEdge {
    pub a: FileId,
    pub b: FileId,
    /// `a` measured against `b`; [`MatchEdge::measurement_from`] gives `b`'s
    /// view.
    pub measurement: Measurement,
    /// §10.1 unique lines of `a` against `b`.
    pub unique_lines_a: u64,
    /// §10.1 unique lines of `b` against `a`.
    pub unique_lines_b: u64,
}

impl MatchEdge {
    #[must_use]
    pub fn touches(&self, file: FileId) -> bool {
        self.a == file || self.b == file
    }

    /// The other endpoint, or `None` if `file` is not an endpoint.
    #[must_use]
    pub fn other(&self, file: FileId) -> Option<FileId> {
        if self.a == file {
            Some(self.b)
        } else if self.b == file {
            Some(self.a)
        } else {
            None
        }
    }

    /// The measurement as seen from `file`, or `None` if it is not an
    /// endpoint.
    #[must_use]
    pub fn measurement_from(&self, file: FileId) -> Option<Measurement> {
        if self.a == file {
            Some(self.measurement)
        } else if self.b == file {
            Some(self.measurement.mirrored())
        } else {
            None
        }
    }

    /// `file`'s unique lines against the other endpoint.
    #[must_use]
    pub fn unique_lines_of(&self, file: FileId) -> Option<u64> {
        if self.a == file {
            Some(self.unique_lines_a)
        } else if self.b == file {
            Some(self.unique_lines_b)
        } else {
            None
        }
    }
}

/// The counterpart rule of spec §6 and §10.1: the reference whenever
/// `member` has a direct edge to it, otherwise the member with `member`'s
/// best direct match by [`Measurement::cmp_strength`] (the owner's ordering
/// decision of 2026-11-22). Equal strength goes to the lower file ID, also
/// per that decision, so the rule is total and deterministic.
///
/// Returns `None` only when `member` has no edge in `edges`, which a
/// connected group never allows. A counterpart chosen by reviewing in
/// Compare overrides this rule and is the review service's to record.
#[must_use]
pub fn rule_counterpart(edges: &[MatchEdge], reference: FileId, member: FileId) -> Option<FileId> {
    let mut best: Option<(Measurement, FileId)> = None;
    for edge in edges {
        let (Some(other), Some(measurement)) = (edge.other(member), edge.measurement_from(member))
        else {
            continue;
        };
        if other == reference {
            return Some(reference);
        }
        let better = best.is_none_or(|(held, held_file)| {
            measurement
                .cmp_strength(&held)
                .then_with(|| held_file.cmp(&other))
                .is_gt()
        });
        if better {
            best = Some((measurement, other));
        }
    }
    best.map(|(_, file)| file)
}

/// Where a member's counterpart came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CounterpartBasis {
    /// [`rule_counterpart`]. Every scan result starts here.
    Rule,
    /// The user reviewed the row against this member in Compare (§10.1).
    /// Make this the reference discards it.
    ReviewedInCompare,
}

/// What a member row is measured against (spec §6 Counterpart).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CounterpartLink {
    pub counterpart: FileId,
    pub basis: CounterpartBasis,
    /// The direct match with the counterpart, seen from this member. `None`
    /// only for a Compare-chosen counterpart with no direct edge, which must
    /// show no invented score (§10.1); a `Rule` link always has one.
    pub measurement: Option<Measurement>,
    /// §10.1 unique lines of this member against the counterpart: the
    /// Findings Unique column.
    pub unique_lines: u64,
}

/// A member's role in its group.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MemberRole {
    /// The evidence anchor. It has no counterpart, no Match value and no
    /// queue control.
    Reference,
    Member(CounterpartLink),
}

/// One file in a group: one Findings member row.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Member {
    pub file: FileId,
    /// File name, the File name column.
    pub name: String,
    /// Display form of the containing folder, the Folder column.
    pub folder: String,
    /// Display form of the whole path, for inspection and Copy path. Never
    /// the authority for an action (§11.8).
    pub display_path: String,
    pub size_bytes: u64,
    pub modified: SystemTime,
    /// Under a protected scan folder. A per-file Protect is a review
    /// decision and lives in the review service, not here.
    pub folder_protected: bool,
    /// The observation the scan measured.
    pub snapshot: SnapshotId,
    pub role: MemberRole,
}

impl Member {
    #[must_use]
    pub const fn is_reference(&self) -> bool {
        matches!(self.role, MemberRole::Reference)
    }

    #[must_use]
    pub const fn link(&self) -> Option<&CounterpartLink> {
        match &self.role {
            MemberRole::Reference => None,
            MemberRole::Member(link) => Some(link),
        }
    }

    #[must_use]
    pub fn counterpart(&self) -> Option<FileId> {
        self.link().map(|l| l.counterpart)
    }

    #[must_use]
    pub fn measurement(&self) -> Option<&Measurement> {
        self.link().and_then(|l| l.measurement.as_ref())
    }

    #[must_use]
    pub fn unique_lines(&self) -> Option<u64> {
        self.link().map(|l| l.unique_lines)
    }

    /// The row's exact-byte verification level: present only when its match
    /// with its counterpart is Same bytes.
    #[must_use]
    pub fn verification(&self) -> Option<Verification> {
        match self.measurement() {
            Some(Measurement::Identical(v)) => Some(*v),
            _ => None,
        }
    }
}

/// A group header's Match value (spec §11.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GroupHeaderKind {
    Single(RelationKind),
    /// More than one relation type among the member rows. The group counts
    /// under each type it contains (§10.1 Category counts).
    Mixed,
}

/// A connected cluster: every file linked to another by a chain of direct
/// matches (spec §6). It has no group-wide score by design (§10.1); the
/// engine's contract test destructures this struct exhaustively so that
/// adding one fails to compile there.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Group {
    pub id: GroupId,
    /// Stable label such as "G001".
    pub label: String,
    /// Reference first, then members by strength of their match.
    pub members: Vec<Member>,
    pub edges: Vec<MatchEdge>,
}

impl Group {
    /// The first member, which is the reference in every valid group.
    #[must_use]
    pub fn reference(&self) -> Option<&Member> {
        self.members.first().filter(|m| m.is_reference())
    }

    #[must_use]
    pub fn member(&self, file: FileId) -> Option<&Member> {
        self.members.iter().find(|m| m.file == file)
    }

    #[must_use]
    pub fn edge_between(&self, x: FileId, y: FileId) -> Option<&MatchEdge> {
        self.edges
            .iter()
            .find(|e| (e.a == x && e.b == y) || (e.a == y && e.b == x))
    }

    /// The relation types of the member rows, which decide the header and
    /// the match-type filter counts.
    #[must_use]
    pub fn row_kinds(&self) -> BTreeSet<RelationKind> {
        self.members
            .iter()
            .filter_map(|m| m.measurement().map(Measurement::relation))
            .collect()
    }

    /// `None` for a group without a measured member row.
    #[must_use]
    pub fn header(&self) -> Option<GroupHeaderKind> {
        let kinds = self.row_kinds();
        let mut iter = kinds.iter();
        match (iter.next(), iter.next()) {
            (None, _) => None,
            (Some(kind), None) => Some(GroupHeaderKind::Single(*kind)),
            (Some(_), Some(_)) => Some(GroupHeaderKind::Mixed),
        }
    }
}

/// Discovery problems recorded against a file (FR-006).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum IssueKind {
    PermissionDenied,
    ReadFailed,
    /// The text could not be decoded; excluded from text matching.
    DecodingFailed,
    /// The file changed while it was read and was excluded as unstable.
    ChangedDuringRead,
    /// The file disappeared between discovery and reading.
    Vanished,
    /// A scan folder was missing.
    MissingFolder,
}

impl IssueKind {
    pub const ALL: [Self; 6] = [
        Self::PermissionDenied,
        Self::ReadFailed,
        Self::DecodingFailed,
        Self::ChangedDuringRead,
        Self::Vanished,
        Self::MissingFolder,
    ];
}

/// One discovery problem, kept accessible after the scan (spec §11.6).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ScanIssue {
    pub display_path: String,
    pub kind: IssueKind,
}

/// How much of the scan setup the results cover.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Coverage {
    /// Every eligible file was examined and at least one group was found.
    Complete,
    /// Stopped or limited before every eligible file was examined; the
    /// groups found so far are kept.
    Partial,
    /// Completed and found no relationship: the "scan found no
    /// relationships" state, distinct from a search that matches no row.
    Empty,
}

/// One scan's result set. Engine code shares it behind an `Arc` and never
/// mutates it: `setup` and `rules` are the immutable copies the scan used
/// (spec DR-001), and review state such as a changed reference lives in the
/// review service, not here.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ScanResults {
    pub scan: ScanId,
    pub setup: ScanSetup,
    pub rules: MatchingRules,
    pub groups: Vec<Group>,
    pub issues: Vec<ScanIssue>,
    pub coverage: Coverage,
}

impl ScanResults {
    /// Rows with every group expanded: one header per group plus one row per
    /// member (spec §11.9 flattened projection).
    #[must_use]
    pub fn flattened_row_count(&self) -> usize {
        self.groups.iter().map(|g| 1 + g.members.len()).sum()
    }

    /// The group containing `file`.
    #[must_use]
    pub fn group_of(&self, file: FileId) -> Option<&Group> {
        self.groups.iter().find(|g| g.member(file).is_some())
    }
}
