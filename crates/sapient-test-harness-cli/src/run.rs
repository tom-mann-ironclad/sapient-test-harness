//! Implements the `run` subcommand: connects to (or accepts a connection
//! from) the target, drives the bundled default scenario for the chosen
//! role, and renders the resulting report.

use std::io;
use std::net::SocketAddr;
use std::process::ExitCode;
use std::time::Duration;

use sapient_conformance_core::finding::Finding;
use sapient_conformance_core::validation::common::validate_uuid_v4;
use sapient_session::framing::FrameReader;
use sapient_session::{asm::AsmConnection, dmm::DmmConnection};
use tokio::io::split;
use tokio::net::{TcpListener, TcpStream};
use tokio::time::{Instant, timeout};

use crate::cli::{OutputFormat, Role, RunArgs};
use crate::completion::ScenarioResult;
use crate::report::RunReport;
use crate::scenario::{run_asm_scenario, run_dmm_scenario};

pub async fn run(args: RunArgs) -> ExitCode {
    let started_at_unix_millis = crate::report::now_unix_millis();
    if args.verbose && args.format == OutputFormat::Json {
        eprintln!(
            "error: --verbose styles the text report and can't be combined with --format json"
        );
        return ExitCode::from(2);
    }
    if args.suite != "v2.0" {
        eprintln!(
            "error: unknown suite {:?} -- only \"v2.0\" is bundled today (a pluggable \
             scenario/scripting format for adding more without recompiling is deferred)",
            args.suite
        );
        return ExitCode::from(2);
    }

    let harness_node_id = args
        .node_id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let connect_timeout = Duration::from_secs(args.connect_timeout_secs);
    let max_runtime = Duration::from_secs(args.max_runtime_secs);

    let (findings, scenario) = if let Err(err) = validate_harness_node_id(&harness_node_id) {
        let mut scenario = ScenarioResult::for_role(args.role);
        scenario.record_error("configure_node_id", err);
        (Vec::new(), scenario)
    } else {
        let outcome = match args.role {
            Role::Dmm => {
                run_as_dmm(
                    &harness_node_id,
                    &args.target,
                    connect_timeout,
                    max_runtime,
                    args.max_frame_bytes,
                    args.allowed_status_report_intervals,
                )
                .await
            }
            Role::Asm => {
                run_as_asm(
                    &harness_node_id,
                    &args.target,
                    connect_timeout,
                    max_runtime,
                    args.max_frame_bytes,
                )
                .await
            }
        };

        match outcome {
            Ok(result) => result,
            Err(err) => {
                let mut scenario = ScenarioResult::for_role(args.role);
                scenario.record_error(
                    match args.role {
                        Role::Asm => "connect",
                        Role::Dmm => "listen_or_accept",
                    },
                    err,
                );
                (Vec::new(), scenario)
            }
        }
    };

    let report = RunReport::new(
        args.role,
        args.suite,
        args.target.to_string(),
        harness_node_id,
        started_at_unix_millis,
        findings,
        scenario,
    );
    match args.format {
        OutputFormat::Text if args.verbose => crate::pretty::print_verbose(&report),
        OutputFormat::Text => report.print_text(),
        OutputFormat::Json => report.print_json(),
    }

    report.exit_code()
}

/// Rejects a `--node-id` that isn't a valid UUID v4 before any socket is
/// opened, using the same rule `sapient_message.node_id.invalid` enforces
/// on the wire -- a value that would fail as soon as it's stamped onto the
/// harness's own outgoing messages should fail locally instead of letting
/// a strict target reject the harness and look like a conformance failure.
fn validate_harness_node_id(node_id: &str) -> io::Result<()> {
    // `validate_uuid_v4` needs a rule_id, but this check never surfaces one
    // as a Finding -- only `.passed` is used, and this value is discarded.
    // Deliberately not shaped like a real rule ID (no dot), so it doesn't
    // read as a new, uncatalogued one to `scripts/generate-rules.sh`'s
    // completeness sweep.
    if validate_uuid_v4(Some(node_id), "harness_node_id_check", "").passed {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("--node-id {node_id:?} is not a valid UUID v4"),
        ))
    }
}

async fn run_as_dmm(
    harness_node_id: &str,
    target: &str,
    connect_timeout: Duration,
    max_runtime: Duration,
    max_frame_bytes: u32,
    allowed_status_report_intervals: u32,
) -> io::Result<(Vec<Finding>, ScenarioResult)> {
    let target: SocketAddr = target.parse().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "--target {target:?} is not a literal address -- --role dmm listens on one \
                 specific ip:port (e.g. 0.0.0.0:5000), not a hostname"
            ),
        )
    })?;
    let listener = TcpListener::bind(target).await?;
    eprintln!("Listening on {target} for an ASM to connect...");
    let (stream, peer_addr) =
        timeout(connect_timeout, listener.accept())
            .await
            .map_err(|_| {
                io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!("no ASM connected to {target} within {connect_timeout:?}"),
                )
            })??;
    eprintln!("ASM connected from {peer_addr}.");

    let (reader, writer) = split(stream);
    let mut connection = DmmConnection::with_frame_reader(
        harness_node_id,
        reader,
        writer,
        FrameReader::new(max_frame_bytes)
            .with_large_message_warning(crate::cli::warn_large_message),
    )
    .with_allowed_status_report_intervals(allowed_status_report_intervals);
    let Some(deadline) = Instant::now().checked_add(max_runtime) else {
        let mut scenario = ScenarioResult::for_role(Role::Dmm);
        scenario.record_error(
            "configure_deadline",
            io::Error::new(io::ErrorKind::InvalidInput, "max runtime is too large"),
        );
        return Ok((connection.take_findings(), scenario));
    };
    let scenario = run_dmm_scenario(&mut connection, deadline).await;
    Ok((connection.take_findings(), scenario))
}

async fn run_as_asm(
    harness_node_id: &str,
    target: &str,
    connect_timeout: Duration,
    max_runtime: Duration,
    max_frame_bytes: u32,
) -> io::Result<(Vec<Finding>, ScenarioResult)> {
    eprintln!("Connecting to {target}...");
    let stream = timeout(connect_timeout, TcpStream::connect(target))
        .await
        .map_err(|_| {
            io::Error::new(
                io::ErrorKind::TimedOut,
                format!("could not connect to {target} within {connect_timeout:?}"),
            )
        })??;
    eprintln!("Connected.");

    let (reader, writer) = split(stream);
    let mut connection = AsmConnection::with_frame_reader(
        harness_node_id,
        reader,
        writer,
        FrameReader::new(max_frame_bytes)
            .with_large_message_warning(crate::cli::warn_large_message),
    );
    let Some(deadline) = Instant::now().checked_add(max_runtime) else {
        let mut scenario = ScenarioResult::for_role(Role::Asm);
        scenario.record_error(
            "configure_deadline",
            io::Error::new(io::ErrorKind::InvalidInput, "max runtime is too large"),
        );
        return Ok((connection.take_findings(), scenario));
    };
    let scenario = run_asm_scenario(&mut connection, deadline).await;
    Ok((connection.take_findings(), scenario))
}
