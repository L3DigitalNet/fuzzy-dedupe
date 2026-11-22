// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! Domain model: IDs, snapshots, matching rules, metrics, events and plans.
//!
//! Depends on no other workspace crate and on no GUI or OS-specific code.
//!
//! This is plan contract C-01, the MS-1 results model every engine module and
//! presentation owner shares. It is MS-1 provisional: later milestones extend
//! these types through their own plans. Its invariants, which the engine's
//! `fixtures_contract` suite checks over every fixture scenario, are:
//!
//! - IDs are stable for the life of a scan, and a file belongs to exactly one
//!   group (a connected cluster, spec §6).
//! - Every group lists its reference first, and only there.
//! - A member's counterpart follows [`rule_counterpart`] (spec §6, §10.1),
//!   except one the user chose by reviewing the row in Compare.
//! - A member's declared Unique value is the §10.1 unique-line count against
//!   that counterpart.
//! - No group carries a group-wide percentage; scores live on member links
//!   and direct-match edges only.

#![forbid(unsafe_code)]

mod ids;
mod plan;
mod progress;
mod results;
mod review;
mod rules;
mod setup;

pub use ids::{FileId, GroupId, PlanRevision, RequestId, RulesRevision, ScanId, SnapshotId};
pub use plan::{CleanupPlan, PlanItem, PlanItemReason, PlanItemStatus};
pub use progress::ScanProgress;
pub use results::{
    ContainmentDirection, CounterpartBasis, CounterpartLink, Coverage, Group, GroupHeaderKind,
    IssueKind, MatchEdge, Measurement, Member, MemberRole, Permille, RelationKind, ScanIssue,
    ScanResults, Verification, rule_counterpart,
};
pub use review::{
    DecisionBinding, DecisionKind, EvidenceUpdate, InvalidatedDecision, MemberDecision,
    QueueOrigin, QueueScope, QueuedTarget, Refusal, ReviewedPair, ValidationView,
};
pub use rules::{CompareMode, IgnoreOptions, MatchMode, MatchingRules};
pub use setup::{ScanFolder, ScanSetup};
