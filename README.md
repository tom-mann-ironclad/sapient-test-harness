# sapient-test-harness

An independent, greenfield test harness for the SAPIENT / BSI Flex 335
protocol, replacing the legacy Windows/.NET/PostgreSQL
`BSI-Flex-335-v2-Test-Harness`. Built in Rust on
[`sapient-rs`](https://crates.io/crates/sapient-rs), the published SAPIENT
protobuf bindings.

Implementing SAPIENT yourself rather than testing against this harness? The
bindings are maintained as a multi-language
[monorepo](https://github.com/tom-mann-ironclad/sapient-bindings), published
as [`sapient-rs`](https://crates.io/crates/sapient-rs) (Rust),
[`sapient-py`](https://pypi.org/project/sapient-py/) (Python), and
[`Sapient.Bindings`](https://www.nuget.org/packages/Sapient.Bindings/) (C#),
plus a Go module.

**New here?** Start with [`QUICKSTART.md`](QUICKSTART.md) — install, point
it at your Edge Node or C2 Node implementation, and read the result. This
README covers the workspace layout instead.

## Layout

A Cargo workspace of three crates:

- **`crates/sapient-conformance-core`** — the validation engine. Per-message
  BSI Flex 335 v2.0 checks, with no CLI or transport dependencies, so the
  same logic can back a CLI and a future hosted service without forking it.
- **`crates/sapient-session`** — protocol-level session state machines and
  role drivers (DMM and ASM), built on `sapient-conformance-core` for
  message validation and adding session-level sequencing, timing, and
  cross-message correlation a single-message validator can't check.
- **`crates/sapient-test-harness-cli`** — the `sapient-harness` binary:
  `run` drives a bundled scenario against a real target over TCP, `send`
  fires hand-crafted messages at one without session tracking, `selftest`
  checks the harness's own validators against its bundled fixture set.

Each crate's own module docs (`cargo doc --open`) cover its internals in
depth — session event semantics, framing/cancellation behavior, report
schema, and so on.

## Parity with the legacy harness

`crates/sapient-conformance-core/tests/parity.rs` is a data-driven test
that loads 231 fixtures copied from the legacy harness's own
`SapientServicesValidator.UnitTests`
(`crates/sapient-conformance-core/tests/fixtures/{True,False}/`) and
asserts this crate's validators agree with the reference C# validator's
recorded pass/fail outcome for each one. Add a fixture file to extend
coverage, not new Rust — see
[`tests/fixtures/README.md`](crates/sapient-conformance-core/tests/fixtures/README.md).

## Conformance rule catalog

[`RULES.md`](RULES.md) lists every conformance rule this crate can
produce a finding for, generated directly from the validation source by
`scripts/generate-rules.sh`. Don't hand-edit it; CI checks it's current.

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
