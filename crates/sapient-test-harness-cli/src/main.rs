//! Placeholder binary. The real CLI (scenario running, reporting) is
//! Milestone 3 in ROADMAP.md; this crate just proves the workspace wiring
//! and gives `sapient-conformance-core` a binary consumer.

fn main() {
    println!(
        "sapient-harness {} (placeholder)",
        env!("CARGO_PKG_VERSION")
    );
}
