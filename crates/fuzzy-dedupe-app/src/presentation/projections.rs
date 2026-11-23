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
    ContainmentDirection, FileId, Group, GroupId, Measurement, Member, MemberDecision, Refusal,
    RelationKind, ScanId, ScanResults, rule_counterpart,
};

use crate::presentation::state::{
    FindingsState, PresentationState, QueueState, RowId, ScanStage, SortKey,
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

/// The group's current reference.
///
/// The results keep the scan's default reference; Make this the reference
/// changes it only in the review service, whose outcomes reach presentation
/// as [`MemberDecision::Reference`] in `QueueState::decisions` (every row
/// whose role changed is reported, so the old reference's entry changes in
/// the same outcome). Rejected: trusting `Group::reference()` alone, which
/// would keep labelling rows against the scan-time reference after the user
/// changed it.
#[must_use]
pub fn current_reference(group: &Group, queue: &QueueState) -> Option<FileId> {
    group
        .members
        .iter()
        .find(|m| queue.decisions.get(&m.file) == Some(&MemberDecision::Reference))
        .or_else(|| group.reference())
        .or_else(|| group.members.first())
        .map(|m| m.file)
}

/// `member`'s counterpart and evidence under the current review state.
///
/// Cross-crate contract with the engine's `ReviewState::link`
/// (`fuzzy-dedupe-engine/src/review/policy.rs`): a queued target is measured
/// against the counterpart its approval names; otherwise, while the scan's
/// reference stands, the scan's own link applies, and after a reference
/// change the §10.1 rule ([`rule_counterpart`]) recomputes it, because Make
/// this the reference discards Compare choices in the group.
///
/// Limitation: a Compare-chosen counterpart of a row that is not queued
/// (Keep both, an acknowledgment) is not visible here. The review outcome
/// carries it (`FileReviewState::role`), but `events::apply_review` records
/// only the decision, so presentation has no copy of the new role.
#[must_use]
pub fn member_view(group: &Group, queue: &QueueState, member: &Member) -> MemberView {
    let reference = current_reference(group, queue).unwrap_or(member.file);
    if member.file == reference {
        return MemberView {
            reference,
            counterpart: None,
            measurement: None,
            unique: None,
        };
    }
    let scan_reference = group.reference().map(|m| m.file);
    let counterpart = queue
        .entries
        .get(&member.file)
        .map(|entry| entry.counterpart)
        .or_else(|| {
            if scan_reference == Some(reference) {
                member.counterpart()
            } else {
                rule_counterpart(&group.edges, reference, member.file)
            }
        });
    let Some(counterpart) = counterpart else {
        return MemberView {
            reference,
            counterpart: None,
            measurement: None,
            unique: None,
        };
    };
    let link = member.link().filter(|l| l.counterpart == counterpart);
    let edge = group.edge_between(member.file, counterpart);
    MemberView {
        reference,
        counterpart: Some(counterpart),
        measurement: edge
            .and_then(|e| e.measurement_from(member.file))
            .or_else(|| link.and_then(|l| l.measurement)),
        unique: edge
            .and_then(|e| e.unique_lines_of(member.file))
            .or_else(|| link.map(|l| l.unique_lines)),
    }
}

/// Builds the visible-row projection: groups that pass the search and the
/// match-type filter, in sort order, each header followed by its member
/// rows while expanded. Groups stay intact: sorting moves whole groups and
/// the in-group order is fixed (current reference first, then the scan's
/// strength order).
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
