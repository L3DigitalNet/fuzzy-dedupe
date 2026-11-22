// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! Operating-system adapter traits and their Linux implementations.
//!
//! The only crate where OS-conditional code and OS-specific dependencies may
//! appear. It deliberately omits the `#![forbid(unsafe_code)]` every other crate
//! root carries: the workspace `deny` still rejects unsafe code here, but an
//! adapter may relax it with a scoped `#[allow(unsafe_code)]` where an OS API
//! requires it.
