// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! Domain results into the text, rows and metrics views draw (spec §11.8).
//!
//! Projections are pure functions of presentation state: no engine call and
//! no I/O, so a view can run them while drawing. T12 provides the shell's
//! projections (status line, labels); T13 adds the Findings row projections
//! and T15 the Compare ones, each in its own section of this file.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt::Write as _;
use std::time::SystemTime;

use fuzzy_dedupe_domain::{
    ContainmentDirection, FileId, Group, GroupId, Measurement, Member, MemberDecision, MemberRole,
    Refusal, RelationKind, ScanId, ScanResults,
};
use fuzzy_dedupe_engine::compare::ComparisonRequest;

use crate::presentation::state::{
    ComparePair, FindingsState, PresentationState, QueueState, RowId, ScanStage, SortKey,
};

/// The status bar's two ends.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusLine {
    /// Scan counts and completion, or the scan's progress.
    pub left: String,
    /// The queue count.
    pub right: String,
}

/// `12648` as `12,648`.
#[must_use]
pub fn thousands(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// The compact status line (frames `13:648`, `28:1399`).
#[must_use]
pub fn status_line(state: &PresentationState) -> StatusLine {
    const SEP: &str = "   ·   ";
    let left = match (&state.scan.results, state.scan.stage) {
        (_, ScanStage::Running | ScanStage::Stopping) => {
            let progress = state.scan.progress.as_ref();
            let files = progress.map_or(0, |p| p.files_read);
            let groups = progress.map_or(0, |p| p.groups_found);
            format!(
                "Scanning{SEP}{} files read{SEP}{} groups found",
                thousands(files),
                thousands(groups)
            )
        }
        (None, _) => "No scan loaded".to_owned(),
        (Some(results), done) => {
            let groups = results.groups.len() as u64;
            let matched: u64 = results.groups.iter().map(|g| g.members.len() as u64).sum();
            let mut summary = format!(
                "{} text files scanned{SEP}{} groups{SEP}{} matched files",
                thousands(state.scan.files_scanned),
                thousands(groups),
                thousands(matched)
            );
            if done == ScanStage::Partial {
                summary.push_str(SEP);
                summary.push_str("partial: stopped before every file was read");
            } else if let Some(time) = state.scan.finished_at {
                let _ = write!(summary, "{SEP}finished {:02}:{:02}", time.hour, time.minute);
            }
            summary
        }
    };
    let mut right = format!("{} queued", thousands(state.queue.queued_count() as u64));
    if let Some(note) = findings_queue_note(state) {
        right.push_str(", ");
        right.push_str(&note);
    }
    StatusLine { left, right }
}

// ── Findings projections (T13) ───────────────────────────────────────────

/// The status bar's note on queued files the current search or filter
/// hides, such as "3 outside the search" (spec §11.4): queued files whose
/// rows fail the search or the match-type filter. A collapsed group hides
/// nothing here; collapsing is not filtering.
///
/// Reads the cached projection, so while a search edit is being projected
/// the note can trail it by one frame; the queue itself is read live.
#[must_use]
pub fn findings_queue_note(state: &PresentationState) -> Option<String> {
    let projection = state.findings.projection.as_ref()?;
    let matched = projection.matched.as_ref()?;
    let hidden = state
        .queue
        .entries
        .keys()
        .filter(|file| !matched.contains(file))
        .count();
    if hidden == 0 {
        return None;
    }
    let scope = if projection.key.search.trim().is_empty() {
        "the filter"
    } else {
        "the search"
    };
    Some(format!("{} outside {scope}", thousands(hidden as u64)))
}

/// What the projection was built from. A projection whose key equals the
/// current one is reused; any difference rebuilds it (spec §11.9: on data,
/// filter, sort or expansion change, never per frame). `revision` covers
/// results and review changes: `events.rs` bumps it on both.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectionKey {
    pub scan: Option<ScanId>,
    pub revision: u64,
    pub search: String,
    pub match_filter: Option<RelationKind>,
    pub sort: SortKey,
    pub expanded: BTreeSet<GroupId>,
}

impl ProjectionKey {
    #[must_use]
    pub fn of(findings: &FindingsState, scan: Option<ScanId>) -> Self {
        Self {
            scan,
            revision: findings.revision,
            search: findings.search.clone(),
            match_filter: findings.match_filter,
            sort: findings.sort,
            expanded: findings.expanded.clone(),
        }
    }
}

/// One visible table row: indices into the current results, plus its
/// stable identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VisibleRow {
    pub id: RowId,
    /// Index into `ScanResults::groups`.
    pub group: usize,
    /// Index into the group's `members`; `None` for the group header.
    pub member: Option<usize>,
    /// A reference shown only as context for matching members (spec §11.4:
    /// "mark context-only rows rather than misrepresenting them").
    pub context: bool,
}

/// The relationship filter counts (spec §10.1 Category counts): a group
/// counts under every relation type its member rows contain, so a Mixed
/// group adds to several. Counts follow the search, not the type filter
/// (frame `30:2370` shows "All 0 · Same bytes 0 …" for a search without
/// matches).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FilterCounts {
    pub all: usize,
    /// By [`RelationKind`] declaration order.
    pub by_kind: [usize; 4],
}

impl FilterCounts {
    #[must_use]
    pub const fn of(&self, kind: RelationKind) -> usize {
        self.by_kind[kind_index(kind)]
    }
}

const fn kind_index(kind: RelationKind) -> usize {
    match kind {
        RelationKind::SameBytes => 0,
        RelationKind::SameText => 1,
        RelationKind::SimilarText => 2,
        RelationKind::Containment => 3,
    }
}

/// Every relation type in filter-row order.
pub const RELATION_KINDS: [RelationKind; 4] = [
    RelationKind::SameBytes,
    RelationKind::SameText,
    RelationKind::SimilarText,
    RelationKind::Containment,
];

/// The flattened visible-row projection of the Findings table.
#[derive(Clone, Debug, PartialEq)]
pub struct FindingsProjection {
    pub key: ProjectionKey,
    pub rows: Vec<VisibleRow>,
    positions: HashMap<RowId, usize>,
    pub counts: FilterCounts,
    /// Groups the search and filter show, in sort order; Queue identical
    /// copies' "groups shown by the current filter" scope.
    pub shown_groups: Vec<GroupId>,
    /// Every group in sort order, for Expand all groups.
    pub all_groups: Vec<GroupId>,
    /// Files whose rows pass the search and filter, or `None` when neither
    /// is active (every file passes).
    pub matched: Option<HashSet<FileId>>,
}

impl FindingsProjection {
    /// The row's position, if it is visible.
    #[must_use]
    pub fn position(&self, row: RowId) -> Option<usize> {
        self.positions.get(&row).copied()
    }

    /// Whether a search or a match-type filter hides anything.
    #[must_use]
    pub const fn is_filtered(&self) -> bool {
        self.matched.is_some()
    }
}

/// A member row's evidence against its current counterpart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemberView {
    /// The group's current reference.
    pub reference: FileId,
    /// `None` on the reference row.
    pub counterpart: Option<FileId>,
    /// `None` on the reference row, and for a counterpart without a direct
    /// match, which shows no invented score (§10.1).
    pub measurement: Option<Measurement>,
    /// Lines of this file missing from its counterpart.
    pub unique: Option<u64>,
}

/// The group's current reference, by the recorded roles
/// ([`QueueState::reference`]).
#[must_use]
pub fn current_reference(group: &Group, queue: &QueueState) -> Option<FileId> {
    queue.reference(group)
}

/// `member`'s counterpart and evidence under its current role, which the
/// review service reported ([`QueueState::role`]).
///
/// The role is the only source: queue entries and decisions are never used
/// to guess a counterpart, because a Compare review can move the
/// counterpart of a row that is not queued and has no telling decision.
/// Rejected: recomputing it by the §10.1 rule after a reference change,
/// which duplicates the engine's policy and misses Compare choices.
///
/// Match and Unique are the link's own: the review service builds every
/// link from the direct edge when one exists, and from the reviewed
/// comparison when none does, which shows no invented score (§10.1).
#[must_use]
pub fn member_view(group: &Group, queue: &QueueState, member: &Member) -> MemberView {
    let reference = current_reference(group, queue).unwrap_or(member.file);
    match queue.role(member) {
        MemberRole::Member(link) if member.file != reference => MemberView {
            reference,
            counterpart: Some(link.counterpart),
            measurement: link.measurement,
            unique: Some(link.unique_lines),
        },
        _ => MemberView {
            reference,
            counterpart: None,
            measurement: None,
            unique: None,
        },
    }
}

/// One group's review-time facts, computed once per projection build.
struct Prepared {
    index: usize,
    reference: Option<FileId>,
    kinds: BTreeSet<RelationKind>,
    /// Each member's relation to its current counterpart.
    relation: Vec<Option<RelationKind>>,
}

fn prepare(results: &ScanResults, queue: &QueueState) -> Vec<Prepared> {
    results
        .groups
        .iter()
        .enumerate()
        .map(|(index, group)| {
            let relation: Vec<Option<RelationKind>> = group
                .members
                .iter()
                .map(|m| {
                    member_view(group, queue, m)
                        .measurement
                        .map(|x| x.relation())
                })
                .collect();
            Prepared {
                index,
                reference: current_reference(group, queue),
                kinds: relation.iter().flatten().copied().collect(),
                relation,
            }
        })
        .collect()
}

/// The header and, when expanded, the member rows of one shown group:
/// every passing member, and the reference as context when it fails.
fn push_group_rows(
    rows: &mut Vec<VisibleRow>,
    group: &Group,
    p: &Prepared,
    expanded: bool,
    passes: &dyn Fn(usize) -> bool,
) {
    rows.push(VisibleRow {
        id: RowId::Group(group.id),
        group: p.index,
        member: None,
        context: false,
    });
    if !expanded {
        return;
    }
    let reference_index = group
        .members
        .iter()
        .position(|m| Some(m.file) == p.reference);
    let member_order = reference_index
        .into_iter()
        .chain((0..group.members.len()).filter(|i| Some(*i) != reference_index));
    for i in member_order {
        let hit = passes(i);
        if hit || Some(i) == reference_index {
            rows.push(VisibleRow {
                id: RowId::Member {
                    group: group.id,
                    file: group.members[i].file,
                },
                group: p.index,
                member: Some(i),
                context: !hit,
            });
        }
    }
}

/// Builds the visible-row projection: groups that pass the search and the
/// match-type filter, in sort order, each header followed by its member
/// rows while expanded. Groups stay intact: sorting moves whole groups and
/// the in-group order is fixed (current reference first, then the scan's
/// strength order).
#[must_use]
pub fn findings_projection(
    results: &ScanResults,
    queue: &QueueState,
    key: ProjectionKey,
) -> FindingsProjection {
    let needle = key.search.trim().to_lowercase();
    let filtered = !needle.is_empty() || key.match_filter.is_some();
    let prepared = prepare(results, queue);
    let mut order: Vec<&Prepared> = prepared.iter().collect();
    order.sort_by(|a, b| {
        let (ga, gb) = (&results.groups[a.index], &results.groups[b.index]);
        sort_cmp(
            key.sort,
            (ga, &a.kinds, a.reference),
            (gb, &b.kinds, b.reference),
        )
        .then_with(|| ga.label.cmp(&gb.label))
        .then_with(|| ga.id.cmp(&gb.id))
    });

    let mut rows = Vec::new();
    let mut counts = FilterCounts::default();
    let mut shown_groups = Vec::new();
    let mut matched = filtered.then(HashSet::new);
    for p in &order {
        let group = &results.groups[p.index];
        let search_hit: Vec<bool> = group
            .members
            .iter()
            .map(|m| {
                needle.is_empty()
                    || m.name.to_lowercase().contains(&needle)
                    || m.folder.to_lowercase().contains(&needle)
            })
            .collect();
        if !search_hit.iter().any(|hit| *hit) {
            continue;
        }
        counts.all += 1;
        for kind in &p.kinds {
            counts.by_kind[kind_index(*kind)] += 1;
        }
        let passes = |i: usize| {
            search_hit[i]
                && key
                    .match_filter
                    .is_none_or(|kind| p.relation[i] == Some(kind))
        };
        if !(0..group.members.len()).any(passes) {
            continue;
        }
        shown_groups.push(group.id);
        if let Some(matched) = matched.as_mut() {
            matched.extend(
                group
                    .members
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| passes(*i))
                    .map(|(_, m)| m.file),
            );
        }
        push_group_rows(
            &mut rows,
            group,
            p,
            key.expanded.contains(&group.id),
            &passes,
        );
    }
    let positions = rows.iter().enumerate().map(|(i, r)| (r.id, i)).collect();
    let all_groups = order.iter().map(|p| results.groups[p.index].id).collect();
    FindingsProjection {
        key,
        rows,
        positions,
        counts,
        shown_groups,
        all_groups,
        matched,
    }
}

/// The §11.4 group sort keys. Match type orders identical, same text,
/// similar, then containment, by the strongest type a group contains, with
/// a single-type group before a Mixed one of the same strongest type. The
/// size, total and newest keys put the largest or newest first; Folder
/// orders by the current reference's folder.
fn sort_cmp(
    sort: SortKey,
    (a, a_kinds, a_reference): (&Group, &BTreeSet<RelationKind>, Option<FileId>),
    (b, b_kinds, b_reference): (&Group, &BTreeSet<RelationKind>, Option<FileId>),
) -> std::cmp::Ordering {
    match sort {
        SortKey::MatchType => {
            let rank = |kinds: &BTreeSet<RelationKind>| (kinds.first().copied(), kinds.len() > 1);
            rank(a_kinds).cmp(&rank(b_kinds))
        }
        SortKey::GroupSize => b.members.len().cmp(&a.members.len()),
        SortKey::TotalSize => {
            let total = |g: &Group| g.members.iter().map(|m| m.size_bytes).sum::<u64>();
            total(b).cmp(&total(a))
        }
        SortKey::NewestChange => {
            let newest = |g: &Group| g.members.iter().map(|m| m.modified).max();
            newest(b).cmp(&newest(a))
        }
        SortKey::Folder => {
            let folder = |g: &Group, r: Option<FileId>| {
                r.and_then(|r| g.member(r)).map(|m| m.folder.clone())
            };
            folder(a, a_reference).cmp(&folder(b, b_reference))
        }
    }
}

/// The Sort ▾ items, in menu order, with their labels (spec §11.2).
pub const SORT_ITEMS: [(SortKey, &str); 5] = [
    (SortKey::MatchType, "Match type"),
    (SortKey::GroupSize, "Group size"),
    (SortKey::TotalSize, "Total size"),
    (SortKey::NewestChange, "Newest change"),
    (SortKey::Folder, "Folder"),
];

#[must_use]
pub fn sort_label(sort: SortKey) -> &'static str {
    SORT_ITEMS
        .iter()
        .find(|(key, _)| *key == sort)
        .map_or("Match type", |(_, label)| label)
}

/// A relation type's name in the filter row and on a group header.
#[must_use]
pub const fn kind_label(kind: RelationKind) -> &'static str {
    match kind {
        RelationKind::SameBytes => "Same bytes",
        RelationKind::SameText => "Same text",
        RelationKind::SimilarText => "Similar text",
        RelationKind::Containment => "Containment",
    }
}

/// A group header's Match value: the one relation type of its member rows,
/// or "Mixed". Never a score (spec §11.4).
#[must_use]
pub fn header_match_label(group: &Group, queue: &QueueState) -> &'static str {
    let kinds: BTreeSet<RelationKind> = group
        .members
        .iter()
        .filter_map(|m| {
            member_view(group, queue, m)
                .measurement
                .map(|x| x.relation())
        })
        .collect();
    match (kinds.len(), kinds.first()) {
        (1, Some(kind)) => kind_label(*kind),
        (0, _) => "",
        _ => "Mixed",
    }
}

/// "G001 (3)".
#[must_use]
pub fn group_label(group: &Group) -> String {
    format!("{} ({})", group.label, group.members.len())
}

/// A header's File name cell: the group's title, or its reference's name.
#[must_use]
pub fn group_title(group: &Group, findings: &FindingsState, queue: &QueueState) -> String {
    findings
        .group_titles
        .get(&group.id)
        .cloned()
        .unwrap_or_else(|| {
            current_reference(group, queue)
                .and_then(|r| group.member(r))
                .map(|m| m.name.clone())
                .unwrap_or_default()
        })
}

/// A header's Decision cell: the group's queued count, or "Last compared"
/// for the group of Compare's current pair.
#[must_use]
pub fn header_decision(
    group: &Group,
    queue: &QueueState,
    last_compared: Option<GroupId>,
) -> String {
    let queued = group
        .members
        .iter()
        .filter(|m| queue.is_queued(m.file))
        .count();
    if queued > 0 {
        format!("{queued} queued")
    } else if last_compared == Some(group.id) {
        "Last compared".to_owned()
    } else {
        String::new()
    }
}

/// The name a member label uses for `counterpart`: "reference" for the
/// current reference, otherwise its file name.
fn counterpart_name(group: &Group, view: &MemberView, counterpart: FileId) -> String {
    if counterpart == view.reference {
        "reference".to_owned()
    } else {
        group
            .member(counterpart)
            .map_or_else(String::new, |m| m.name.clone())
    }
}

/// A member row's Match label (spec §11.4 member-row labels), always over
/// the counterpart it is measured against. Empty on the reference row and
/// for a counterpart with no direct score.
#[must_use]
pub fn match_label(group: &Group, view: &MemberView) -> String {
    let (Some(counterpart), Some(measurement)) = (view.counterpart, view.measurement) else {
        return String::new();
    };
    let name = counterpart_name(group, view, counterpart);
    match measurement {
        Measurement::Identical(_) => format!("Identical to {name}"),
        Measurement::SameText => format!("Same text as {name}"),
        Measurement::Similar(p) => format!("{} similar to {name}", percent(p.get())),
        Measurement::Containment {
            coverage,
            direction: ContainmentDirection::InsideCounterpart,
        } => format!("{} inside {name}", percent(coverage.get())),
        Measurement::Containment {
            direction: ContainmentDirection::ContainsCounterpart,
            ..
        } if counterpart == view.reference => "Contains the reference".to_owned(),
        Measurement::Containment {
            direction: ContainmentDirection::ContainsCounterpart,
            ..
        } => format!("Contains {name}"),
    }
}

/// One decimal of a percentage from per-mille, "100%" when whole.
#[must_use]
pub fn percent(permille: u16) -> String {
    if permille.is_multiple_of(10) {
        format!("{}%", permille / 10)
    } else {
        format!("{}.{}%", permille / 10, permille % 10)
    }
}

/// The Unique cell: "4 lines", "1 line", or "—" when there are none.
#[must_use]
pub fn unique_label(view: &MemberView) -> String {
    match view.unique {
        None | Some(0) => "—".to_owned(),
        Some(1) => "1 line".to_owned(),
        Some(n) => format!("{} lines", thousands(n)),
    }
}

/// The row menu's first item (spec §11.4): "Compare with reference", or
/// "Compare with" and the counterpart's name for a row measured against
/// another member.
#[must_use]
pub fn compare_item_label(group: &Group, view: &MemberView) -> String {
    match view.counterpart {
        Some(counterpart) if counterpart != view.reference => {
            format!(
                "Compare with {}",
                counterpart_name(group, view, counterpart)
            )
        }
        _ => "Compare with reference".to_owned(),
    }
}

/// Whether queueing the row first needs Review unique text, by the owner
/// rule (2026-11-23): not byte-identical to its counterpart and a Unique
/// count above 0.
fn needs_review(view: &MemberView) -> bool {
    view.measurement
        .is_some_and(|m| m.relation() != RelationKind::SameBytes)
        && view.unique.is_some_and(|n| n > 0)
}

/// A member row's Decision cell, with the precedence Reference, Queued,
/// the row's own FR-025 refusal, Protected, Kept, Counterpart queued,
/// Review first, Unreviewed. It matches the T9 design-scenario test
/// (`fuzzy-dedupe-engine/tests/design_scenario.rs`, `decision_label`).
///
/// The view never decides a state: queue membership and protection come
/// from the engine's published view, Kept and acknowledgments from the
/// review outcomes in `QueueState::decisions`. A row no outcome has
/// reported yet shows its scan-time state, Review first or Unreviewed.
#[must_use]
pub fn decision_label(member: &Member, view: &MemberView, queue: &QueueState) -> &'static str {
    let file = member.file;
    let decision = queue.decisions.get(&file).copied();
    if view.counterpart.is_none() {
        return "Reference";
    }
    if queue.is_queued(file) {
        return "Queued";
    }
    // Only the two FR-025 refusals have a row label; every other refusal
    // already reads as the row's own state (Reference, Protected) or is a
    // transient status-bar message.
    if let Some((refused, refusal)) = queue.last_refusal
        && refused == file
        && matches!(
            refusal,
            Refusal::CounterpartQueued | Refusal::CounterpartOfQueuedFile
        )
    {
        return refusal_text(refusal);
    }
    if member.folder_protected || queue.is_protected(file) {
        return "Protected";
    }
    if decision == Some(MemberDecision::Kept) {
        return "Kept";
    }
    if view.counterpart.is_some_and(|c| queue.is_queued(c)) {
        return "Counterpart queued";
    }
    match decision {
        Some(MemberDecision::ReviewFirst) => "Review first",
        Some(MemberDecision::Unreviewed) => "Unreviewed",
        _ if needs_review(view) => "Review first",
        _ => "Unreviewed",
    }
}

/// The queue checkbox a member row shows (spec §11.4 Queue column).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum QueueBox {
    /// Reference rows have no checkbox.
    None,
    Checked,
    Unchecked,
    /// Protected and counterpart-queued rows show it disabled.
    Disabled,
}

#[must_use]
pub fn queue_box(member: &Member, view: &MemberView, queue: &QueueState) -> QueueBox {
    if view.counterpart.is_none() {
        QueueBox::None
    } else if queue.is_queued(member.file) {
        QueueBox::Checked
    } else if member.folder_protected
        || queue.is_protected(member.file)
        || view.counterpart.is_some_and(|c| queue.is_queued(c))
    {
        QueueBox::Disabled
    } else {
        QueueBox::Unchecked
    }
}

/// The Size cell in binary units (spec §11.4): bytes below 1 KiB, then one
/// decimal of KiB, MiB or GiB, rounded to nearest.
#[must_use]
pub fn size_label(bytes: u64) -> String {
    const UNITS: [&str; 3] = ["KiB", "MiB", "GiB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut unit = 0;
    let mut divisor: u64 = 1024;
    while unit + 1 < UNITS.len() && bytes >= divisor * 1024 {
        divisor *= 1024;
        unit += 1;
    }
    let tenths = (bytes * 10 + divisor / 2) / divisor;
    format!("{}.{} {}", tenths / 10, tenths % 10, UNITS[unit])
}

/// The Modified cell: the UTC calendar date, by Hinnant's `civil_from_days`
/// (the same form the T9 design test checks).
#[must_use]
pub fn date_label(time: SystemTime) -> String {
    let secs = time
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let z = i64::try_from(secs / 86_400).unwrap_or(0) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

/// The Folder cell's display form, ending in "/" as the frames show it.
#[must_use]
pub fn folder_label(folder: &str) -> String {
    if folder.ends_with('/') {
        folder.to_owned()
    } else {
        format!("{folder}/")
    }
}
// ── Compare projections (T15) ────────────────────────────────────────────

/// Separator between the parts of a pinned file identity's detail line
/// (frame `13:1415`).
pub const IDENTITY_SEPARATOR: &str = "   ·   ";

/// One pane's pinned file identity (frame `13:1410`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaneIdentity {
    pub file: FileId,
    pub name: String,
    pub path: String,
    /// "12.8 KiB   ·   modified 2026-05-02".
    pub size_and_date: String,
    /// "A · Reference", "B · Review first": the side letter and the file's
    /// Decision cell, so the pane says what Findings says about the row.
    pub role: String,
}

/// What Compare shows about its pair. Side A is the counterpart (the
/// evidence anchor, left pane) and side B the target, the file Queue B
/// names (C-04 as T35 oriented it).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompareView {
    /// "G004".
    pub group_label: String,
    /// The pair, A first: "`Design_Spec.txt`  ↔  `Design_Spec_draft.txt`".
    pub pair_label: String,
    pub a: PaneIdentity,
    pub b: PaneIdentity,
    /// The pair's direct relation; `None` when the two have no direct edge,
    /// which shows no invented score (§10.1).
    pub relation: Option<RelationKind>,
    /// "97.9% similar"; `None` for a relation that has no score.
    pub score: Option<String>,
    /// The candidate stepper's files: every other member of the group, the
    /// current reference first, then group order.
    pub candidates: Vec<FileId>,
    /// The counterpart's place in [`Self::candidates`].
    pub candidate_index: usize,
}

fn identity(group: &Group, queue: &QueueState, member: &Member, letter: &str) -> PaneIdentity {
    let view = member_view(group, queue, member);
    PaneIdentity {
        file: member.file,
        name: member.name.clone(),
        path: member.display_path.clone(),
        size_and_date: format!(
            "{}{IDENTITY_SEPARATOR}modified {}",
            size_label(member.size_bytes),
            date_label(member.modified)
        ),
        role: format!("{letter} · {}", decision_label(member, &view, queue)),
    }
}

/// The pinned identity row's accessible name: "A · Reference:
/// `Design_Spec.txt`, `~/Documents/Design/Design_Spec.txt`, 12.8 KiB,
/// modified 2026-05-02".
#[must_use]
pub fn identity_name(identity: &PaneIdentity) -> String {
    format!(
        "{}: {}, {}, {}",
        identity.role,
        identity.name,
        identity.path,
        identity.size_and_date.replace(IDENTITY_SEPARATOR, ", ")
    )
}

/// Compare's view of `pair` in `results`, or `None` when the pair is not
/// one of these results' groups (a pair left over from an earlier scan, or
/// a file compared with itself): Compare then shows its no-pair state.
#[must_use]
pub fn compare_view(
    results: &ScanResults,
    queue: &QueueState,
    pair: ComparePair,
) -> Option<CompareView> {
    if pair.target == pair.counterpart {
        return None;
    }
    let group = results.groups.iter().find(|g| g.id == pair.group)?;
    let target = group.member(pair.target)?;
    let counterpart = group.member(pair.counterpart)?;
    let measurement = group
        .edge_between(pair.target, pair.counterpart)
        .and_then(|edge| edge.measurement_from(pair.target));
    let score = measurement.and_then(|m| match m {
        Measurement::Similar(p) => Some(format!("{} similar", percent(p.get()))),
        Measurement::Containment {
            coverage,
            direction: ContainmentDirection::InsideCounterpart,
        } => Some(format!("{} of B is inside A", percent(coverage.get()))),
        Measurement::Containment {
            coverage,
            direction: ContainmentDirection::ContainsCounterpart,
        } => Some(format!("{} of A is inside B", percent(coverage.get()))),
        Measurement::Identical(_) | Measurement::SameText => None,
    });
    let reference = current_reference(group, queue);
    let mut candidates: Vec<FileId> = reference
        .into_iter()
        .chain(group.members.iter().map(|m| m.file))
        .filter(|file| *file != pair.target)
        .collect();
    let mut seen = HashSet::new();
    candidates.retain(|file| seen.insert(*file));
    let candidate_index = candidates
        .iter()
        .position(|file| *file == pair.counterpart)
        .unwrap_or(0);
    Some(CompareView {
        group_label: group.label.clone(),
        pair_label: format!("{}  ↔  {}", counterpart.name, target.name),
        a: identity(group, queue, counterpart, "A"),
        b: identity(group, queue, target, "B"),
        relation: measurement.map(|m| m.relation()),
        score,
        candidates,
        candidate_index,
    })
}

/// "Candidate 1 of 2".
#[must_use]
pub fn candidate_label(index: usize, count: usize) -> String {
    format!("Candidate {} of {count}", index + 1)
}

/// "Change 1 of 2", or "No changes" for a pair whose panes hold none.
#[must_use]
pub fn change_label(active: usize, changes: usize) -> String {
    if changes == 0 {
        "No changes".to_owned()
    } else {
        format!("Change {} of {changes}", active.min(changes - 1) + 1)
    }
}

/// What changed since the comparison Compare shows was computed, or `None`
/// while its evidence is current: "A changed since this comparison", and
/// so on for B, both files and the matching rules.
///
/// The comparison's own key is the evidence; the engine's published view
/// is the current state. A file the view does not know is not reported as
/// changed: the stale marker states a change, it never guesses one.
#[must_use]
pub fn stale_note(shown: &ComparisonRequest, queue: &QueueState) -> Option<String> {
    let changed = |file, snapshot| queue.snapshot(file).is_some_and(|now| now != snapshot);
    let a = changed(shown.counterpart, shown.counterpart_snapshot);
    let b = changed(shown.target, shown.target_snapshot);
    let rules = queue
        .view
        .as_ref()
        .is_some_and(|view| view.rules != shown.rules);
    let files = match (a, b) {
        (true, true) => Some("A and B"),
        (true, false) => Some("A"),
        (false, true) => Some("B"),
        (false, false) => None,
    };
    match (files, rules) {
        (None, false) => None,
        (Some(files), false) => Some(format!("{files} changed since this comparison")),
        (None, true) => Some("The matching rules changed since this comparison".to_owned()),
        (Some(files), true) => Some(format!(
            "{files} and the matching rules changed since this comparison"
        )),
    }
}

/// The toolbar's "Review cleanup (N)" label.
#[must_use]
pub fn review_cleanup_label(state: &PresentationState) -> String {
    format!("Review cleanup ({})", state.queue.queued_count())
}

/// The status-bar text for a refused review command (C-05 refusal labels).
#[must_use]
pub const fn refusal_text(refusal: Refusal) -> &'static str {
    match refusal {
        Refusal::Reference => "The reference is never queued.",
        Refusal::Protected => "Protected files are never queued.",
        Refusal::CounterpartQueued => "Counterpart queued",
        Refusal::CounterpartOfQueuedFile => "Counterpart of a queued file",
        Refusal::NotByteIdentical => "Only byte-identical copies are queued here.",
        Refusal::UnknownFile => "That file is not in the current results.",
        Refusal::EvidenceUnavailable | Refusal::EvidenceMismatch => {
            "The comparison changed; compare again before queueing."
        }
    }
}
