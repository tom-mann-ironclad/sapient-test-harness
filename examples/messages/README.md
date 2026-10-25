# Example messages

Canonical-protobuf-JSON `SapientMessage` files for use with
`sapient-harness send --file <path>` (see [`QUICKSTART.md`](../../QUICKSTART.md)).
Copy one, edit it, send it -- no Rust required.

Organised by which side sends them, matching `send`'s `--role`:

- **`from-edge-node/`** -- what an Edge Node sends. Use with
  `--role edge` to send these to a Fusion Node under test.
- **`from-fusion-node/`** -- what a Fusion Node sends. Use with
  `--role fusion` to send these to an Edge Node under test.

Each directory is numbered in a realistic session order (registration
first, etc.), but `send` doesn't require that order -- send whichever
file(s) you want, in any order, to see how a target reacts.

| File | Message type | Notes |
|---|---|---|
| `from-edge-node/01-registration.json` | `Registration` | Declares two modes, `Default` (`MODE_TYPE_DEFAULT`) and `Alternate` (`MODE_TYPE_PERMANENT`), and a `Human` detection classification capability. |
| `from-edge-node/02-status-report.json` | `StatusReport` | Declares the `Default` mode as active. |
| `from-edge-node/03-detection-report.json` | `DetectionReport` | Classifies an object as `Human`, matching the registration's declared capability. |
| `from-edge-node/04-task-ack.json` | `TaskAck` | Accepts the mode-change task below. |
| `from-edge-node/05-alert.json` | `Alert` | A plain informational alert. |
| `from-edge-node/06-detection-report-malformed.json` | `DetectionReport` | **Deliberately non-conformant**: identical to `03-detection-report.json` except its `reportId` is `"not-a-ulid"`, so it breaks `detection_report.report_id.invalid`. It still decodes, so `send` warns and sends it anyway. A conformant Fusion Node should reply with an `Error`. Use the standalone `sapient-harness send` command for it: `session`'s `send` gives every report a fresh ID, which would replace the bad one. |
| `from-fusion-node/01-registration-ack.json` | `RegistrationAck` | Accepts the registration above. |
| `from-fusion-node/02-task-mode-change.json` | `Task` | Switches the Edge Node to its `Alternate` mode. |
| `from-fusion-node/03-alert-ack.json` | `AlertAck` | Acknowledges the alert above. |

All the others validate cleanly against this crate's own conformance rules
(no warnings from `send`). To try other faults, copy one and break it --
deleting `"alertId"` from `from-edge-node/05-alert.json`, for example, is
enough to trigger `alert.alert_id.invalid` (see [`RULES.md`](../../RULES.md)
for every rule). Validators, including this harness's, usually report only
the first fault they find in a message, so break one thing per file to know
which fault a reply is about.
