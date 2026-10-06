//! Library surface for `sapient-harness`, split out from the `main.rs`
//! binary purely so its internal modules (e.g. `send`) are reachable from
//! integration tests in `tests/` -- Cargo integration tests can only see a
//! crate's public library API, not a bin-only crate's private modules.

pub mod cli;
pub mod pretty;
pub mod report;
pub mod run;
pub mod scenario;
pub mod selftest;
pub mod send;
pub mod session;
mod terminal;

pub mod completion;
