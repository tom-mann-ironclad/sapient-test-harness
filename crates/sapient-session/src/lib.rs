//! Protocol-level session state machine and role drivers for the SAPIENT
//! / BSI Flex 335 test harness. See `ROADMAP.md`'s Milestone 2 section
//! (monorepo root) for the design. Builds on
//! `sapient_conformance_core`'s single-message validation and
//! `sapient_rs`'s async framing, adding session sequencing, timing, and
//! cross-message correlation that a single-message validator can't check.

pub mod dmm;
pub mod state;

pub use state::{DmmSession, RegisteredContract, SessionState};
