//! `sapient-harness`: the CLI for the independent SAPIENT / BSI Flex 335
//! conformance test harness.

use clap::Parser;
use sapient_test_harness_cli::cli::{Cli, Command};
use sapient_test_harness_cli::{run, selftest, send};

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Run(args) => run::run(args).await,
        Command::Selftest(args) => selftest::selftest(args.format),
        Command::Send(args) => send::send(args).await,
    }
}
