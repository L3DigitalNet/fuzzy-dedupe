// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! Reusable widgets (spec §11.8).
//!
//! | Widget            | File               | Filled by |
//! | ----------------- | ------------------ | --------- |
//! | Dialog host       | `dialog_host.rs`   | T12       |
//! | Status bar        | `status.rs`        | T12       |
//! | Grouped table     | `grouped_table.rs` | T13       |
//! | Paired diff view  | `diff_view.rs`     | T14       |

pub mod dialog_host;
pub mod diff_view;
pub mod grouped_table;
pub mod status;
