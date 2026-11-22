// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! Use cases, orchestration, analyzer registry and scheduling above storage,
//! scanning, analysis and file operations.
//!
//! In MS-1 the engine runs on fixtures only (plan D-002, D-005): it reads no
//! user file and performs no filesystem access at all. Fixture "files" are
//! deterministic in-memory buffers, and the simulated cleanup acts on IDs.
//!
//! [`service`] is the facade the application talks to (plan C-02). The four
//! job and review modules behind it are contract-first stubs (plan D-004):
//! this crate root and the facade declare every module and call shape, and
//! each later task fills one module without editing the shared files.
//!
//! | Module          | Contract | Filled by |
//! | --------------- | -------- | --------- |
//! | [`scan_sim`]    | C-03     | T5        |
//! | [`compare`]     | C-04     | T6        |
//! | [`review`]      | C-05     | T7        |
//! | [`cleanup_sim`] | C-06     | T8        |
//! | `fixtures::design` | design scenario | T9 |

#![forbid(unsafe_code)]

/// The domain model (plan C-01), re-exported so the application can name
/// facade payloads through one crate.
pub use fuzzy_dedupe_domain as domain;

pub mod cleanup_sim;
pub mod compare;
pub mod fixtures;
pub mod review;
pub mod scan_sim;
pub mod service;
