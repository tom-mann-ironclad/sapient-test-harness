# sapient-test-harness

An independent, greenfield test harness for the SAPIENT / BSI Flex 335
protocol, replacing the legacy Windows/.NET/PostgreSQL
`BSI-Flex-335-v2-Test-Harness`. Built in Rust on top of
[`sapient-rs`](https://crates.io/crates/sapient-rs), the published SAPIENT
protobuf bindings.

See [`ROADMAP.md`](../ROADMAP.md) (one level up, at the monorepo root) for
the full milestone plan. This crate is currently at **Milestone 0**:
turning scaffolded validation code into a real, tested, CI-gated
foundation.

## Layout

This is a Cargo workspace:

- `crates/sapient-conformance-core` — the validation engine. Per-message
  validators for the BSI Flex 335 v2.0 ICD (`src/validation/`), with no
  CLI or transport dependencies, so the same logic can back a CLI and a
  future hosted service without forking it.
- `crates/sapient-test-harness-cli` — the `sapient-harness` binary. Just a
  placeholder today; the real CLI (scenario running, reporting) is
  Milestone 3.

## Parity with the legacy harness

`crates/sapient-conformance-core/tests/parity.rs` is a data-driven test
that loads all 231 fixtures from
`crates/sapient-conformance-core/tests/fixtures/{True,False}/` — copied
from the legacy harness's own `SapientServicesValidator.UnitTests` — and
asserts this crate's validators agree with the reference C# validator's
recorded pass/fail outcome for each one. Add a new fixture file rather than
writing new Rust to extend coverage; see `tests/fixtures/README.md`.

Fixture JSON is canonical protobuf JSON, decoded via `prost-reflect` against
`sapient_rs::FILE_DESCRIPTOR_SET_BYTES` — a compiled `FileDescriptorSet`
that `sapient-rs` itself generates and ships (as of 0.2.0), so this crate
has no vendored `.proto` sources or descriptor-compilation step of its own.

## Build & test

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Licensing

Dual-licensed under MIT or Apache-2.0, at your option, matching
`sapient-rs`. The legacy fixture set under
`crates/sapient-conformance-core/tests/fixtures/` is separately
Crown-Copyright / Apache-2.0 (see the `README.md` in that directory).
