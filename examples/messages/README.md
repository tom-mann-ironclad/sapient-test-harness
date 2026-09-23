# Example messages

Canonical-protobuf-JSON `SapientMessage` files for use with
`sapient-harness send --file <path>` (see [`QUICKSTART.md`](../../QUICKSTART.md)).
Copy one, edit it, send it -- no Rust required.

Organised by which side sends them, matching `send`'s `--role`:

- **`from-edge-node/`** -- what an Edge Node (ASM) sends. Use with
  `--role asm` to send these to a C2 Node under test.
- **`from-c2-node/`** -- what a C2 Node (DMM) sends. Use with `--role dmm`
  to send these to an Edge Node under test.

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
| `from-c2-node/01-registration-ack.json` | `RegistrationAck` | Accepts the registration above. |
| `from-c2-node/02-task-mode-change.json` | `Task` | Switches the Edge Node to its `Alternate` mode. |
| `from-c2-node/03-alert-ack.json` | `AlertAck` | Acknowledges the alert above. |

All eight validate cleanly against this crate's own conformance rules (no
warnings from `send`). To see the warning path instead, edit one --
deleting `"alertId"` from `from-edge-node/05-alert.json`, for example, is
enough to trigger `alert.alert_id.invalid`.
