# Quickstart

Get a real conformance result against your own SAPIENT / BSI Flex 335
implementation in about 10 minutes. See [`README.md`](README.md) for how
the harness is built; this doc is just "install it, run it, read the
result."

## 1. Install

There's no published crate or pre-built release binary yet. For now, build
from source. You need a recent stable Rust toolchain 
([rustup.rs](https://rustup.rs) if you don't have one).

```bash
git clone https://github.com/tom-mann-ironclad/sapient-test-harness.git
cd sapient-test-harness
cargo build --release -p sapient-test-harness-cli
```

The binary is now at `target/release/sapient-harness`. Optionally install
it onto your `PATH`:

```bash
cargo install --path crates/sapient-test-harness-cli
```

Confirm it works:

```bash
sapient-harness --help
```

## 2. A note on terminology

The wire protocol and this harness's own code and `--role` flag still use
the original naming -- **ASM** (Autonomous Sensor Module) and **DMM**
(Decision Making Module) -- since that's what the ICD and the existing
test suite are built around. Current SAPIENT usage prefers **Edge Node**
and **C2 Node**. They mean the same things:

| Current term | `--role` value | Sends | Receives |
|---|---|---|---|
| Edge Node | `asm` | `Registration`, `StatusReport`, `DetectionReport`, `Alert` | `RegistrationAck`, `Task`, `AlertAck` |
| C2 Node | `dmm` | `RegistrationAck`, `Task`, `AlertAck` | `Registration`, `StatusReport`, `DetectionReport`, `Alert` |

## 3. Running against your implementation

The harness always plays the *other* role from whatever you're testing --
point it at your implementation, and it drives a real session against it.

**Developing an Edge Node?** Run the harness as the C2 Node it registers
with. `--target` is the address the harness listens on -- bind it
somewhere your Edge Node can actually reach (e.g. `<C2 IP address>:<port>` if it's on
another machine or in a container):

```bash
sapient-harness run --role dmm --target <C2 IP Address>:5000
```

Then point your Edge Node's C2 connection at that address (the C2 test harness
machine's IP and port `5000`, or `127.0.0.1:5000` if it's running
locally too) and let it register.

**Developing a C2 Node?** Run the harness as an Edge Node that connects
out to it:

```bash
sapient-harness run --role asm --target <C2 node address>:5000
```

`--target` means different things for the two roles: for `--role dmm` it's the
literal `ip:port` to listen on (a hostname isn't accepted -- binding needs one
specific local address, not a resolved list); for `--role asm` it's the address
to connect to, and accepts a hostname (`your-service.example:5000`,
`localhost:5000`, a container/service-discovery name, etc.).

Either way, the harness drives one full conformance session (registration,
status reporting, a detection, a mode-change task, an alert) and prints a
pass/fail report. Useful flags:

- `--connect-timeout-secs <N>` (default 30) -- how long to wait for the
  other side to connect.
- `--max-runtime-secs <N>` (default 120) -- cap on all reads and writes once
  connected, including automatic replies and GoodBye. The connection closes at
  the end of the run. A deadline during a blocked write is an operational error;
  an ordinary observation deadline retains the existing completion verdict.
- `--format json` -- machine-readable output for CI, on `stdout` only
  (progress messages go to `stderr`, so `stdout` stays clean JSON).
- `-v` / `--verbose` -- render the final report as a styled checklist
  (✓/✗ per scenario check, coloured when stdout is a terminal; set
  `NO_COLOR` to disable). Text output only, not with `--format json`.
- `--node-id <uuid>` -- override the random node ID the harness stamps on
  its own outgoing messages.

Incoming payloads are limited to **64 MiB** by default in both `run` and `send`.
Use `--max-frame-bytes <bytes>` to change this local resource limit; for example,
`--max-frame-bytes 2147483648` permits payloads up to 2 GiB. The four-byte header
is excluded. Exceeding the limit closes the connection and exits 2 with a resource
error, not a SAPIENT conformance finding.

The receiver reuses a 64 KiB buffer, grows it as bytes arrive, and releases larger
allocations after processing each frame. Frames of **1 MiB or more** produce an
immediate warning on stderr, once per frame, without affecting the verdict or
JSON stdout. Decoding and protocol error replies can require additional copies;
the receive limit is not a cap on total process memory. Very large payloads still
need sufficient RAM. Releasing allocations does not guarantee an immediate drop
in the operating system's reported memory usage.

## 4. Interpreting the result

While the session runs, progress streams live to your terminal (registered,
messages exchanged, any timeouts) -- that's on `stderr`. The final report,
on `stdout`, looks like this on success:

```
sapient-harness run -- role=asm suite=v2.0 target=127.0.0.1:5000

PASS -- required scenario checks completed; no conformance errors.

Scenario checks:
  Registration: Completed
  StatusReport: Completed
  DetectionReport: Completed
  AlertAck: Completed
  Goodbye: Completed

Notes:
  - Registration accepted.
  - Processed an inbound message from the DMM (e.g. a Task) before continuing.
  - Alert acknowledged.
  - Sent a GoodBye StatusReport to end the session gracefully.
```

A failing run instead shows `FAIL -- N finding(s).` followed by the
findings grouped by severity:

```
FAIL -- 1 finding(s).

Error:
  [registration.icd_version.invalid] registration.icd_version: ICD version specified in registration is not a valid option.
```

Each finding has a stable `rule_id` (see
[`RULES.md`](RULES.md) for the full catalog of what the harness checks),
the field it's about, and a human-readable explanation. `Warning`-severity
findings (e.g. falling back to a legacy mode-declaration convention) don't
fail the run; only `Error`-severity ones do.

A `run` can also report `INCOMPLETE`: the connection ended or the deadline
expired before required checks finished. This is not a claim that the peer broke
a protocol rule. It means the scenario did not obtain enough evidence to pass.
The report lists every required check as `completed`, `incomplete`, or `skipped`
(with a reason for skips). Error findings take precedence and produce `failed`,
but incomplete checks remain visible.

The bundled scenario requires:

- **ASM role:** accepted registration, transmitted StatusReport and DetectionReport,
  a valid correlated AlertAck, and a transmitted GoodBye. An unsolicited Task from
  the DMM is optional and its absence does not fail the run.
- **DMM role:** accepted registration, a validated ordinary StatusReport, and a
  valid correlated TaskAck for the probe task. The task check is skipped when the
  registration has no non-default mode for that probe. Spontaneous detections and
  alerts are validated if received but are not required to complete this scenario.
  GoodBye/disconnect is not required: the observation deadline can end a passing
  run if all required checks have already completed. GoodBye or re-registration
  does not substitute for an outstanding TaskAck.

Completed sends and acknowledgements establish only the checks listed here, not
that the peer implements every feature of the standard.

**Exit codes** (`run` and `selftest`): `0` pass, `1` conformance findings or an incomplete `run`,
`2` a harness-level failure (couldn't connect, bad arguments, etc.) --
distinct from `1` so CI can distinguish a failed/incomplete scenario from an
operational failure.

`run` also emits a report when connection setup or session I/O fails. Earlier
findings, notes, and completed checks are retained. An `operational_error` object
records `stage`, `kind`, and `message`. Such a run exits **2** even if conformance
findings also exist; its outcome is `failed` when there are error findings and
`incomplete` otherwise. A transport failure alone is not a protocol violation.
Clean EOF and ordinary reply/observation timeouts retain the existing completion
rules. CLI argument parsing and unsupported suite errors remain command errors.

### JSON output

`--format json` prints the same report as JSON instead of text, on
`stdout` only -- progress messages always go to `stderr`, so piping or
redirecting `stdout` gets you clean JSON with nothing else mixed in:

```bash
sapient-harness run --role asm --target your-c2-node-host:5000 \
  --format json > result.json
```

```json
{
  "role": "asm",
  "suite": "v2.0",
  "target": "127.0.0.1:5000",
  "passed": true,
  "outcome": "passed",
  "checks": [
    { "check": "registration", "status": "completed" },
    { "check": "status_report", "status": "completed" },
    { "check": "detection_report", "status": "completed" },
    { "check": "alert_ack", "status": "completed" },
    { "check": "goodbye", "status": "completed" }
  ],
  "findings": [],
  "notes": [
    "Registration accepted.",
    "Processed an inbound message from the DMM (e.g. a Task) before continuing.",
    "Alert acknowledged.",
    "Sent a GoodBye StatusReport to end the session gracefully."
  ]
}
```

`outcome` is `passed`, `failed`, or `incomplete`; `passed` is true only for
`passed`. `checks` records scenario completion separately from `findings`.

`findings` has the same `rule_id`/`field_path`/`severity`/`message` shape
as the text report's findings, just structured -- handy for `jq`, e.g.
`jq '.findings[] | select(.severity == "error")' result.json`.
`selftest` also takes `--format json` (a `total`/`passed`/`mismatches`
shape). `send` doesn't -- it doesn't produce one structured report to
serialise (see below).

Before trusting a result, you can confirm the harness itself is healthy --
no network needed:

```bash
sapient-harness selftest
```

```
sapient-harness selftest -- 231 bundled fixtures

PASS -- every fixture classified as expected.
```

## 5. Using in CI

`--format json` plus the distinct `0`/`1`/`2` exit codes are meant for
this: run the harness as a CI step, capture its JSON report as a build
artifact, and fail the job on a non-zero exit. A GitHub Actions example,
since that's what this repo's own CI uses (`.github/workflows/ci.yml`):

```yaml
- name: Run SAPIENT conformance test
  id: conformance
  run: |
    status=0
    sapient-harness run --role asm --target your-c2-node-host:5000 \
      --format json > result.json || status=$?
    echo "exit_code=$status" >> "$GITHUB_OUTPUT"

- name: Upload conformance result
  if: always() # capture the artifact whether it passed or not
  uses: actions/upload-artifact@v4
  with:
    name: sapient-conformance-result
    path: result.json

- name: Fail the job on a non-passing result
  if: steps.conformance.outputs.exit_code != '0'
  run: exit 1
```

GitHub Actions runs each step's `run:` script with the shell's fail-fast
option on by default, so if the harness call is left unguarded (e.g.
`... > result.json` followed directly by `echo "exit_code=$?" ...`), a
non-zero exit aborts the script *before* that `echo` line ever runs --
the output is silently never set, rather than being set to the exit code.
The final gate step still happens to fail the job either way (an unset
output isn't `'0'` either), so this is easy to miss, but the whole point
of capturing `1` vs `2` separately is lost. `sapient-harness ... ||
status=$?` avoids this: a command on the left of `||` is exempt from
fail-fast, so the script keeps running and `status` reliably ends up
holding the real exit code either way. Because the failure is now
contained in the `status` variable rather than propagating to the step's
own exit code, `continue-on-error` is no longer needed on the run step
either -- the script's last command (`echo`) always succeeds, so the step
itself always succeeds regardless of what the harness returned.

If you need to tell "conformance findings" (`1`) apart from "the harness
itself couldn't run" (`2`) in CI, branch on
`steps.conformance.outputs.exit_code` directly instead of the `!= '0'`
check above.

## 6. Sending custom messages

`run` drives one fixed scenario. To explore anything else -- a specific
detection, an edge case, a deliberately malformed message to see how your
implementation reacts -- use `send` to fire off hand-crafted messages
directly, one at a time or several in sequence over one connection:

```bash
sapient-harness send --role asm --target your-c2-node-host:5000 \
  --file examples/messages/from-edge-node/01-registration.json \
  --file examples/messages/from-edge-node/02-status-report.json
```

See [`examples/messages/`](examples/messages/) for a full set of ready-to-edit
example messages (one per message type, organised by who sends them) and
[its README](examples/messages/README.md) for what each one is.

Each `--file` is a `SapientMessage` in canonical protobuf JSON -- the same
format used throughout this repo's own fixtures
(`crates/sapient-conformance-core/tests/fixtures/`), so any of those are
fair game as starting points too. `send` is deliberately raw: it doesn't
track session state, so it won't stop you from sending something
non-conformant -- it validates each message against the harness's own
rules first and prints a warning if it fails, but sends it regardless.
That's the point: it's for testing how your implementation handles things
`run`'s fixed scenario doesn't cover, including things that shouldn't be
valid.

`send --write-timeout-secs <N>` (default 30) bounds transmission of each file.
A write timeout closes the connection and exits 2; subsequent files are not sent.
`--response-timeout-secs` separately bounds the wait for a reply after sending.

`send` has no pass/fail verdict of its own (exit `0` once every `--file`
has been sent, `2` on a harness-level failure) -- read the printed replies
and warnings yourself.

## 7. Reporting bugs

For now, open an issue on GitHub:
<https://github.com/tom-mann-ironclad/sapient-test-harness/issues>

Please include:
- The command you ran (redact any addresses you don't want public).
- The full output, including any `stderr` progress lines.
- `sapient-harness --version`, and whether `sapient-harness selftest`
  passes (rules out the harness itself being unhealthy).
- If it's a `send` session, the message file(s) you used.
