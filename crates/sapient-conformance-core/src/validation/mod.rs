//! One submodule per BSI Flex 335 v2.0 SAPIENT message type, each holding
//! that message's conformance rules (required fields, value ranges,
//! cross-field constraints) as plain Rust functions over the generated
//! `sapient-rs` types -- see [`sapient_message::validate_sapient_message`]
//! for the top-level entry point that dispatches to the rest.
//!
//! These rules are meant to be correct against the ICD itself, not
//! reverse-engineered from the legacy C# validator; `common` holds the
//! primitive checks (ULID/UUID format, timestamp bounds, required-string,
//! ...) shared across message types. Parity with the legacy reference
//! implementation is verified externally, in this crate's
//! `tests/parity.rs`, rather than assumed here -- see the crate-level docs
//! in `lib.rs` for why that split matters.

pub mod alert;
pub mod alert_ack;
pub mod common;
pub mod detection_report;
pub mod error;
pub mod registration;
pub mod registration_ack;
pub mod sapient_message;
pub mod status_report;
pub mod task;
pub mod task_ack;
