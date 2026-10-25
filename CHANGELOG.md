# Changelog

## 1.0.0-beta.7

### Added

- `examples/messages/from-edge-node/06-detection-report-malformed.json`: a
  deliberately non-conformant DetectionReport (invalid `reportId`), for
  checking how a Fusion Node under test handles bad input.

### Changed

- `RULES.md` lists every rule built by a shared validator (locations,
  range-bearings, durations, associated files and detections, task
  parameters) by its full rule ID and message, under the message type that
  uses it. It replaces the per-file prefix lists and the hand-kept
  "Composite validator suffixes" section, which had fallen out of date:
  it was missing the finite-number checks (e.g. `alert.location.x.invalid`),
  `*.associated_detection.association_type.invalid` and
  `task.*.parameter.value.invalid`. `scripts/generate-rules.sh` now finds
  these rules by following the validators in source, and fails if it can't.
- Clearer messages for four Registration rules that said a field "must be
  specified" when it was present but invalid:
  `registration.region_definition.region_type.invalid`,
  `registration.location_type.units.invalid`,
  `registration.location_type.datum.invalid` and
  `registration.filter_parameter.operators.invalid`. Rule IDs are
  unchanged.
- Fixed the spelling of "definition" in the `registration.node_definition.*`
  messages.
- `send --role edge` now retries a failed connection every second until
  `--connect-timeout-secs` runs out, like `run` and `session`, instead of
  failing on the first refusal.

## 1.0.0-beta.6

### Changed

- The harness now uses the BSI Flex 335 v2.0 node names throughout:
  `--role edge` (was `asm`) and `--role fusion` (was `dmm`), and "Edge
  Node"/"Fusion Node" in output, finding messages and docs. `--role asm`
  and `--role dmm` still work as hidden aliases. Rule IDs are unchanged.
- **JSON report:** `role` is now `"edge"` or `"fusion"`, and
  `report_schema_version` is 2. Update anything that reads `role`.
- `examples/messages/from-c2-node/` is renamed `from-fusion-node/`.

## 1.0.0-beta.5

### Added

- Releases include a CycloneDX SBOM (`sapient-test-harness-cli.cdx.xml`,
  with a checksum) covering every platform's dependencies, and signed
  GitHub build-provenance attestations for every release file
  (`gh attestation verify`). Binaries are built with `cargo auditable`, so
  `cargo audit bin` and similar scanners can read their dependency list.
- CI generates the SBOM on every push.

## 1.0.0-beta.4

### Added

- `session` command: keeps a connection open as an Edge Node (`--role asm`)
  or C2 Node (`--role dmm`), logs and validates all traffic as it happens,
  sends StatusReports automatically (and DetectionReports with
  `--detection-interval-secs`), and sends detections, alerts, or tasks from
  typed commands. In a terminal, commands are typed at a prompt kept below
  the log, with history and file-path completion.

### Changed

- `run --role asm` now retries a failed connection every second until
  `--connect-timeout-secs` runs out, instead of failing on the first
  refusal. An invalid `--target` address still fails immediately.
- `session.status_report.interval_exceeded` now allows a gap of up to 10%
  more than the declared status interval, so an Edge Node reporting at
  exactly its declared rate isn't flagged for ordinary timing jitter.
- The harness's Edge Node now sends periodic StatusReports at 90% of its
  registered interval, so normal timing jitter can't make it exceed that
  interval.
- Release archives now include `QUICKSTART.md` and `examples/messages/`,
  so the quick start, including its `send` examples, works from a download
  without the repository.

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
