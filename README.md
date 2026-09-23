# sapient-test-harness

An independent, greenfield test harness for the SAPIENT / BSI Flex 335
protocol, replacing the legacy Windows/.NET/PostgreSQL
`BSI-Flex-335-v2-Test-Harness`. Built in Rust on top of
[`sapient-rs`](https://crates.io/crates/sapient-rs), the published SAPIENT
protobuf bindings.

## Layout

This is a Cargo workspace:

- `crates/sapient-conformance-core` — the validation engine. Per-message
  validators for the BSI Flex 335 v2.0 ICD (`src/validation/`), with no
  CLI or transport dependencies, so the same logic can back a CLI and a
  future hosted service without forking it.
- `crates/sapient-session` — protocol-level session state machine and
  role drivers, for both DMM and ASM. `state.rs`/`asm_state.rs` are pure,
  synchronous state machines with no I/O; `dmm.rs`/`asm.rs` are thin
  `tokio` drivers (`DmmConnection`/`AsmConnection`) that own the actual
  socket, sharing frame read/write logic from `framing.rs`. Both split
  the stream into independent read/write halves so a caller can issue a
  message (e.g. the DMM issuing a `Task`, or the ASM sending
  `Registration`/`StatusReport`/`DetectionReport`/`Alert`) and poll for
  inbound messages without either blocking the other; `dmm::run` remains
  as a thin reactive read-and-reply wrapper over `DmmConnection` for
  callers that don't need to issue tasks mid-session. Builds on
  `sapient-conformance-core` for message validation and adds session-level
  sequencing, timing, and cross-message correlation a single-message
  validator can't check.
  `tests/dmm_asm_interop.rs` drives the harness's own `DmmConnection`
  against its own `AsmConnection` over a real `tokio::io::duplex`, as a
  genuine interoperability check independent of the hand-built fixtures
  `tests/dmm_session.rs`/`tests/asm_session.rs` each test their own role
  against.
- `crates/sapient-test-harness-cli` — the `sapient-harness` binary.
  `sapient-harness run --role dmm|asm --target <addr>` drives the bundled
  default v2.0 scenario against a real target over TCP, reusing
  `sapient-session`'s `DmmConnection`/`AsmConnection` directly. `--role
  dmm` listens on `--target` for an ASM to connect in; `--role asm`
  connects out to a DMM/middleware under test. `sapient-harness selftest`
  runs the harness's own validators against its bundled fixture set (the
  same 231 fixtures `sapient-conformance-core/tests/parity.rs` checks at
  build time, embedded into the binary via `include_dir!` so no source
  tree is needed at run time) -- no network, just a health check that the
  harness itself is trustworthy before pointing `run` at something. Both
  commands take `--format json` for CI; exit code 0 (pass), 1
  (conformance findings/fixture mismatches), or 2 (harness-level failure,
  e.g. couldn't connect).

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

## Conformance rule catalog

[`RULES.md`](../RULES.md) lists every conformance rule this crate can
produce a finding for — rule ID, source file, and message — generated
directly from `crates/sapient-conformance-core/src/validation/*.rs` by
`scripts/generate-rules.sh`. Don't hand-edit it; rerun the script after
changing validation logic (CI checks it's up to date).

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
