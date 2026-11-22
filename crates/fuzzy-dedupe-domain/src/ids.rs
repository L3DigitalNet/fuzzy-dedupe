// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! Opaque newtype IDs. Each names one kind of thing, so a group ID can never
//! be passed where a file ID is expected.
//!
//! Presentation code keys focus, expansion and queue membership by these IDs,
//! never by row index or file name (spec §11.4), so an ID must stay stable
//! for the life of the scan that issued it. Events carry the scan, request or
//! plan ID that originated them so stale ones can be discarded (spec §11.8).

macro_rules! id_type {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(u64);

        impl $name {
            /// Wraps a raw value. Issuers allocate values; consumers only
            /// compare and hash them.
            #[must_use]
            pub const fn new(raw: u64) -> Self {
                Self(raw)
            }

            /// The raw value, for logging and deterministic ordering.
            #[must_use]
            pub const fn get(self) -> u64 {
                self.0
            }
        }
    };
}

id_type!(
    /// One scan's result set. A new scan never reuses an earlier scan's ID,
    /// so results from different scans are never merged.
    ScanId
);
id_type!(
    /// A group within one scan.
    GroupId
);
id_type!(
    /// A file entry within one scan, the identity every decision, queue
    /// entry and plan item refers to.
    FileId
);
id_type!(
    /// One saved version of the matching rules. A scan stores the revision
    /// it used; review decisions bind to it (spec DR-008).
    RulesRevision
);
id_type!(
    /// One asynchronous request, such as a comparison, so a superseded
    /// response can be recognised and ignored.
    RequestId
);
id_type!(
    /// One engine-issued cleanup plan. A revised plan gets a new revision,
    /// and each revision authorises at most one execution.
    PlanRevision
);
id_type!(
    /// One observation of a file's content. Meaningful only together with
    /// its [`FileId`]: a decision bound to (file, snapshot) is invalid once
    /// that file's current snapshot differs.
    SnapshotId
);
