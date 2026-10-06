//! Command-line argument definitions.

use std::fmt;
use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(
    name = "sapient-harness",
    version,
    about = "Independent SAPIENT / BSI Flex 335 conformance test harness"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Run a conformance suite against a target implementation.
    Run(RunArgs),
    /// Run the harness's own validators against its bundled fixture set,
    /// to confirm the harness itself is healthy before trusting a `run`
    /// result against it. No network needed.
    Selftest(SelftestArgs),
    /// Manually send one or more hand-crafted messages to a target and
    /// observe how it replies, without running the bundled scenario or
    /// tracking session state. For exploring behaviour the bundled `run`
    /// scenario doesn't cover, without writing Rust.
    Send(SendArgs),
    /// Keep a live session open with a target: register (or accept a
    /// registration), keep it alive with automatic status reports, log and
    /// validate everything received, and send detections, alerts, or tasks
    /// on demand from typed commands. No scripted pass/fail scenario.
    Session(SessionArgs),
}

#[derive(Args)]
pub struct SelftestArgs {
    /// Output format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,
}

#[derive(Args)]
pub struct SendArgs {
    /// Which role the harness plays: `dmm` listens on `--target` for a
    /// peer to connect in; `asm` connects out to `--target`.
    #[arg(long, value_enum)]
    pub role: Role,

    /// For `--role dmm`, the literal `ip:port` to listen on (a hostname
    /// isn't accepted here -- binding needs one specific local address, not
    /// a resolved list). For `--role asm`, the address to connect to; this
    /// one does accept `host:port`, resolved when connecting.
    #[arg(long)]
    pub target: String,

    /// Maximum incoming payload bytes (local resource limit, not a conformance rule).
    #[arg(long, default_value_t = sapient_session::framing::DEFAULT_MAX_FRAME_BYTES)]
    pub max_frame_bytes: u32,

    /// A message to send, as a path to a canonical-protobuf-JSON
    /// `SapientMessage` file (the same format used by
    /// `sapient-conformance-core/tests/fixtures/`) -- node_id, timestamp,
    /// destination_id, and content are all taken verbatim from the file,
    /// letting you hand-craft exact message content, including
    /// deliberately non-conformant messages. Repeat `--file` to send
    /// several messages in order over the same connection (e.g. a
    /// Registration, then a StatusReport). Each message is validated
    /// against this crate's own conformance rules before sending; a
    /// failure is printed as a warning, not a reason to skip sending it.
    #[arg(long = "file", required = true)]
    pub files: Vec<PathBuf>,

    /// How long to wait for the peer to connect (`--role dmm`) or for the
    /// outbound connection to establish (`--role asm`), in seconds.
    #[arg(long, default_value_t = 30)]
    pub connect_timeout_secs: u64,

    /// How long to wait for a reply after each message, in seconds. Most
    /// message types don't get a reply at all (only Registration, Task,
    /// and Alert do) -- this just bounds how long to wait before moving on
    /// to the next `--file`.
    #[arg(long, default_value_t = 10)]
    pub response_timeout_secs: u64,

    /// Maximum seconds to transmit each file. A timeout closes the connection.
    #[arg(long, default_value_t = 30)]
    pub write_timeout_secs: u64,
}

#[derive(Args)]
pub struct SessionArgs {
    /// Which role the harness plays: `dmm` listens on `--target` for an ASM
    /// under test to connect in; `asm` connects out to `--target`, a
    /// DMM/middleware implementation under test.
    #[arg(long, value_enum)]
    pub role: Role,

    /// For `--role dmm`, the literal `ip:port` to listen on. For
    /// `--role asm`, the `host:port` to connect to.
    #[arg(long)]
    pub target: String,

    /// Maximum incoming payload bytes (local resource limit, not a conformance rule).
    #[arg(long, default_value_t = sapient_session::framing::DEFAULT_MAX_FRAME_BYTES)]
    pub max_frame_bytes: u32,

    /// How long to wait for the peer to connect (`--role dmm`) or for the
    /// outbound connection to establish (`--role asm`, retried every second
    /// within this time), in seconds.
    #[arg(long, default_value_t = 30)]
    pub connect_timeout_secs: u64,

    /// Maximum seconds to transmit any one message. A timeout ends the
    /// session, since the connection can't be reused after a partial write.
    #[arg(long, default_value_t = 30)]
    pub write_timeout_secs: u64,

    /// Node ID the harness stamps on its own outgoing messages. Defaults
    /// to a freshly generated random UUID.
    #[arg(long)]
    pub node_id: Option<String>,

    /// `--role asm` only: also send the scripted DetectionReport every this
    /// many seconds once registered. Off by default; use the `detection`
    /// command to send one on demand.
    #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
    pub detection_interval_secs: Option<u64>,

    /// `--role dmm` only: see `run --allowed-status-report-intervals`.
    #[arg(
        long,
        default_value_t = sapient_session::state::DEFAULT_ALLOWED_STATUS_REPORT_INTERVALS
    )]
    pub allowed_status_report_intervals: u32,
}

#[derive(Args)]
pub struct RunArgs {
    /// Which role the harness plays: `dmm` listens on `--target` for an ASM
    /// under test to connect in; `asm` connects out to `--target`, a
    /// DMM/middleware implementation under test.
    #[arg(long, value_enum)]
    pub role: Role,

    /// For `--role dmm`, the literal `ip:port` to listen on (a hostname
    /// isn't accepted here -- binding needs one specific local address, not
    /// a resolved list). For `--role asm`, the address to connect to; this
    /// one does accept `host:port`, resolved when connecting.
    #[arg(long)]
    pub target: String,

    /// Maximum incoming payload bytes (local resource limit, not a conformance rule).
    #[arg(long, default_value_t = sapient_session::framing::DEFAULT_MAX_FRAME_BYTES)]
    pub max_frame_bytes: u32,

    /// Conformance suite to run. Only "v2.0" exists today -- the
    /// scenario/scripting DSL for adding more without recompiling is
    /// deferred.
    #[arg(long, default_value = "v2.0")]
    pub suite: String,

    /// Output format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,

    /// Render the text report as a styled checklist (coloured when stdout
    /// is a terminal and `NO_COLOR` is unset). Not valid with `--format json`.
    #[arg(short, long)]
    pub verbose: bool,

    /// How long to wait for the peer to connect (`--role dmm`) or for the
    /// outbound connection to establish (`--role asm`), in seconds. For
    /// `--role asm`, a failed attempt (e.g. refused because the target
    /// isn't listening yet) is retried every second within this time.
    #[arg(long, default_value_t = 30)]
    pub connect_timeout_secs: u64,

    /// Overall cap on reads and writes once connected, in seconds. For `--role dmm`
    /// this also bounds how long the harness will wait for the ASM under
    /// test to end the session on its own terms (a `GoodBye` `StatusReport`
    /// or a disconnect). The connection closes when the run ends, including
    /// on deadline expiry or an I/O failure.
    #[arg(long, default_value_t = 120)]
    pub max_runtime_secs: u64,

    /// Node ID the harness stamps on its own outgoing messages. Defaults
    /// to a freshly generated random UUID.
    #[arg(long)]
    pub node_id: Option<String>,

    /// For `--role dmm`: how many multiples of the declared
    /// `status_interval` a StatusReport gap may span before it's treated
    /// as a problem, applied to the first StatusReport after Registration
    /// (which has no previous report to measure a single-interval gap
    /// against, and Registration itself can land at any phase of the
    /// ASM's reporting rhythm, so a single interval is not a valid
    /// deadline there). Ignored for `--role asm`.
    #[arg(
        long,
        default_value_t = sapient_session::state::DEFAULT_ALLOWED_STATUS_REPORT_INTERVALS
    )]
    pub allowed_status_report_intervals: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Dmm,
    Asm,
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Role::Dmm => write!(f, "dmm"),
            Role::Asm => write!(f, "asm"),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub enum OutputFormat {
    #[default]
    Text,
    Json,
}

/// Announce unusually large incoming frames immediately, before payload reception.
pub(crate) fn warn_large_message(bytes: u32) {
    eprintln!(
        "WARNING: receiving a large message ({bytes} payload bytes); decoding and replies may require additional memory."
    );
}
