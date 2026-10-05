# Changelog

## Unreleased

### Changed

- `run --role asm` now retries a failed connection every second until
  `--connect-timeout-secs` runs out, instead of failing on the first
  refusal. An invalid `--target` address still fails immediately.

## 1.0.0-beta.3

### Fixed

- The shell installer now accepts systems with glibc 2.34, such as
  RHEL / Rocky / Alma 9. It previously required glibc 2.35, the build
  runner's version, although the Linux binaries only need glibc 2.34. No
  changes to the harness itself.

## 1.0.0-beta.2

### Fixed

- Fixture `False/0112.DetectionReport.DetectionLocation.RangeBearing` now
  decodes as v2.0. It used `RangeBearingCone` extent fields and set both
  `location` and `rangeBearing`. It is now rejected by
  `detection_report.location.datum.missing` rather than by a JSON parse
  error.
- `selftest` no longer reports decode-only failures. `False/0001.Timestamp.Error`
  is documented as parse-only, because its `"AAA"` timestamp can't decode as
  protobuf JSON. `sapient_message.timestamp.malformed` now has its own unit
  test.

### Changed

- The fixture parity test now fails if a `False/` fixture is rejected only
  because it can't be parsed, unless that fixture is listed in
  `PARSE_ONLY_FIXTURES`.

## 1.0.0-beta.1

First public pre-release of `sapient-harness`, an independent conformance
test harness for SAPIENT / BSI Flex 335 v2.0. See
[`QUICKSTART.md`](https://github.com/tom-mann-ironclad/sapient-test-harness/blob/v1.0.0-beta.1/QUICKSTART.md) to install and run it.

### Commands

- **`run`**: drives the bundled v2.0 scenario against a real Edge Node
  (`--role dmm`) or C2 Node (`--role asm`) over TCP and reports `PASS`,
  `FAIL`, or `INCOMPLETE`.
  - `--format json` produces a versioned, machine-readable report.
  - `-v` / `--verbose` prints a styled checklist.
  - Exit codes are `0` for a pass, `1` for findings or an incomplete run,
    and `2` for a harness or operational failure.
- **`send`**: sends hand-crafted `SapientMessage` JSON files to a target,
  including deliberately non-conformant ones, and prints its replies.
- **`selftest`**: checks the harness's own validators against its 231
  bundled fixtures. No network is needed.

### Conformance checks

- Per-message validation for every v2.0 message type, ported from the
  legacy C# harness. Parity is checked against that harness's own
  fixture set.
- Session-level checks for both roles:
  - registration handshake and rejection handling
  - status-report cadence and monotonic timestamps
  - detection and registration consistency
  - task and alert correlation
  - mode changes
  - GoodBye handling
- Every finding has a stable `rule_id`, listed in [`RULES.md`](https://github.com/tom-mann-ironclad/sapient-test-harness/blob/v1.0.0-beta.1/RULES.md).
  Findings in reports carry message identity and repeat spans.

### Operational

- Connect, write, and overall run deadlines.
- Configurable incoming frame-size limit, with a warning for large frames.
- Ctrl-C produces a partial report.
- Text output escapes peer-supplied content so it can't inject terminal
  control sequences.
- `--role asm` accepts hostnames as well as IP addresses.
