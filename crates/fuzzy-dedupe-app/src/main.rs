// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! The `fuzzy-dedupe` egui application binary: a thin entry over the
//! library's `app::run`, so tests drive the same code the binary runs.

#![forbid(unsafe_code)]

fn main() -> eframe::Result {
    fuzzy_dedupe_app::app::run()
}
