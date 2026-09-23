//! The independent SAPIENT / BSI Flex 335 v2.0 conformance validation
//! engine: no CLI, transport, or hosted-service concerns here, so this
//! logic can back a CLI, a session-driving protocol harness, and (later) a
//! hosted service without forking it.
//!
//! [`validation`] is the actual rule set, implemented directly against the
//! BSI Flex 335 v2.0 ICD -- it does not exist to reproduce the legacy
//! `BSI-Flex-335-v2-Test-Harness`'s C# validator, and shouldn't be read as
//! a port of it. Rather than trust that by construction, `tests/parity.rs`
//! (in this crate's `tests/` directory) is a separate, adversarial check:
//! it feeds every fixture from the legacy validator's own recorded
//! pass/fail set through [`validation`] and asserts agreement. A mismatch
//! there means one of the two is wrong and needs investigating -- it isn't
//! assumed to always be this crate (see the ICD-version and
//! `concurrent_tasks` fixes in `validation::registration`, both found this
//! way).

pub use sapient_rs::bsi_flex_335_v2_0;

pub mod finding;
pub mod fixture_json;
pub mod validation;

pub use finding::{Finding, Severity, ValidationOutcome};
