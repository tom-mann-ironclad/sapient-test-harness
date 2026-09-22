# Legacy parity fixtures

These 231 `.json` files are copied verbatim from the legacy
`BSI-Flex-335-v2-Test-Harness`'s
`SapientServicesValidator.UnitTests/{True,False}` fixture set (Crown
Copyright, Apache License 2.0 — see that repository's `LICENSE.txt`), which
pins down the exact behaviour of the reference C# validator this crate aims
for parity with.

- `True/` — fixtures the reference validator accepts.
- `False/` — fixtures the reference validator rejects (including a handful
  that don't even parse as valid SAPIENT protobuf JSON; see the comment in
  `tests/parity.rs` for how that's handled).

Add new fixtures here — following the existing `NNNN.Name.Reason.json`
naming convention — rather than writing new Rust test cases; `tests/parity.rs`
picks up every file in these two directories automatically.
