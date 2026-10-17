//! StagePilot core: events, state, settings and models (serde). No I/O. Filled in by phase 2.
//!
//! Phase 1 only proves the type-generation path: [`api`] mirrors one real model group from
//! `backend/src/stagepilot/models/api.py` and exports TypeScript for it.

pub mod api;
