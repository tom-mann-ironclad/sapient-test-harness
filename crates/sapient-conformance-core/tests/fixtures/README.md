# Legacy parity fixtures

These 231 `.json` files are copied verbatim from the legacy
`BSI-Flex-335-v2-Test-Harness`'s
`SapientServicesValidator.UnitTests/{True,False}` fixture set (Crown
Copyright, Apache License 2.0 — see that repository's `LICENSE.txt`), which
pins down the exact behaviour of the reference C# validator this crate aims
for parity with.

- `True/` — fixtures the reference validator accepts.
- `False/` — fixtures the reference validator rejects. Each must decode and
  be rejected by a validator rule. The only exceptions are listed in
  `PARSE_ONLY_FIXTURES` (`src/fixture_json.rs`), whose violation can only
  be written in JSON as a parse failure. `tests/parity.rs` fails for any
  other fixture that doesn't decode.

A few fixtures have been corrected where the legacy copy couldn't decode as
v2.0 for reasons unrelated to the rule it's named for (e.g. `0112` used
`RangeBearingCone` extent fields and set both `location` and
`rangeBearing`). See the git history for each change.

Add new fixtures here — following the existing `NNNN.Name.Reason.json`
naming convention — rather than writing new Rust test cases; `tests/parity.rs`
picks up every file in these two directories automatically.
