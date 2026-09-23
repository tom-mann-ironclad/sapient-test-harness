//! `sapient-harness`: the CLI for the independent SAPIENT / BSI Flex 335
//! conformance test harness.

mod cli;
mod report;
mod run;
mod scenario;
mod selftest;

use clap::Parser;
use cli::{Cli, Command};

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Run(args) => run::run(args).await,
        Command::Selftest(args) => selftest::selftest(args.format),
    }
}
