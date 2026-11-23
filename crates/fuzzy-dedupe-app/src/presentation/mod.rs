// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! The presentation core (spec §11.8, plan C-12): state owners, typed
//! intents to the application services, engine-event ingestion,
//! projections and preferences. Nothing here draws; `ui` draws from it.

pub mod commands;
pub mod events;
pub mod preferences;
pub mod projections;
pub mod state;
