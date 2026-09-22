//! Protocol-level session state machine and role drivers for the SAPIENT
//! / BSI Flex 335 test harness. Builds on `sapient_conformance_core`'s
//! single-message validation and`sapient_rs`'s async framing, adding
//! session sequencing, timing, and cross-message correlation that a
//! single-message validator can't check.

pub mod asm;
pub mod asm_state;
pub mod dmm;
pub mod framing;
pub mod state;

pub use asm_state::{AsmSession, AsmSessionState, RegisteredAsmContract};
pub use state::{DmmSession, RegisteredContract, SessionState};
