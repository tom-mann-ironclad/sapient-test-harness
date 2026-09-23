//! `include_dir!` (in `src/selftest.rs`) embeds
//! `sapient-conformance-core/tests/fixtures` at compile time, but a
//! proc-macro invocation doesn't register the directory it reads as a
//! build input on its own -- without this, `cargo build` won't notice a
//! fixture file added, removed, or edited there and will silently keep
//! serving a stale embedded copy until something else (a `.rs` change)
//! happens to force recompilation.

fn main() {
    println!("cargo:rerun-if-changed=../sapient-conformance-core/tests/fixtures");
}
