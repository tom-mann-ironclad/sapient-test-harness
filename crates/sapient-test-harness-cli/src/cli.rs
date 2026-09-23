//! Command-line argument definitions.

use std::fmt;
use std::net::SocketAddr;

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
}

#[derive(Args)]
pub struct SelftestArgs {
    /// Output format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,
}

#[derive(Args)]
pub struct RunArgs {
    /// Which role the harness plays: `dmm` listens on `--target` for an ASM
    /// under test to connect in; `asm` connects out to `--target`, a
    /// DMM/middleware implementation under test.
    #[arg(long, value_enum)]
    pub role: Role,

    /// For `--role dmm`, the address to listen on. For `--role asm`, the
    /// address to connect to.
    #[arg(long)]
    pub target: SocketAddr,

    /// Conformance suite to run. Only "v2.0" exists today -- the
    /// scenario/scripting DSL for adding more without recompiling is
    /// deferred.
    #[arg(long, default_value = "v2.0")]
    pub suite: String,

    /// Output format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,

    /// How long to wait for the peer to connect (`--role dmm`) or for the
    /// outbound connection to establish (`--role asm`), in seconds.
    #[arg(long, default_value_t = 30)]
    pub connect_timeout_secs: u64,

    /// Overall cap on the run once connected, in seconds. For `--role dmm`
    /// this also bounds how long the harness will wait for the ASM under
    /// test to end the session on its own terms (a `GoodBye` `StatusReport`
    /// or a disconnect) -- the harness never disconnects a DMM-role
    /// session itself.
    #[arg(long, default_value_t = 120)]
    pub max_runtime_secs: u64,

    /// Node ID the harness stamps on its own outgoing messages. Defaults
    /// to a freshly generated random UUID.
    #[arg(long)]
    pub node_id: Option<String>,
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
