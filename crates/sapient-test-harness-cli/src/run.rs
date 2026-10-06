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
use tokio::time::{Instant, sleep_until, timeout, timeout_at};

use crate::cli::{OutputFormat, Role, RunArgs};
use crate::completion::ScenarioResult;
use crate::report::RunReport;
use crate::scenario::{run_asm_scenario, run_dmm_scenario};

/// Delay between `--role asm` connection attempts, so a harness started
/// before the target is listening connects once it comes up rather than
/// failing on the first refusal.
const CONNECT_RETRY_INTERVAL: Duration = Duration::from_secs(1);

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
pub(crate) fn validate_harness_node_id(node_id: &str) -> io::Result<()> {
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
    let stream = listen_and_accept(target, connect_timeout).await?;

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
    let stream = connect_with_retries(target, connect_timeout).await?;

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

/// Connects to `target`, retrying every [`CONNECT_RETRY_INTERVAL`] until
/// `connect_timeout` has elapsed in total. That deadline also bounds each
/// attempt, so a hung attempt can't overrun it. An unparseable target
/// (`InvalidInput`) fails at once, since retrying can't fix it. On giving
/// up, the error keeps the last attempt's kind (e.g. `ConnectionRefused`),
/// or `TimedOut` if the final attempt was still pending at the deadline.
pub(crate) async fn connect_with_retries(
    target: &str,
    connect_timeout: Duration,
) -> io::Result<TcpStream> {
    // `None` only for a timeout too large to represent: no deadline at all.
    let deadline = Instant::now().checked_add(connect_timeout);
    let mut attempts = 0u32;
    loop {
        attempts += 1;
        let attempt = match deadline {
            Some(deadline) => timeout_at(deadline, TcpStream::connect(target)).await,
            None => Ok(TcpStream::connect(target).await),
        };
        let error = match attempt {
            Ok(Ok(stream)) => {
                if attempts > 1 {
                    eprintln!("Connected after {attempts} attempts.");
                } else {
                    eprintln!("Connected.");
                }
                return Ok(stream);
            }
            Ok(Err(error)) if error.kind() == io::ErrorKind::InvalidInput => return Err(error),
            Ok(Err(error)) => error,
            Err(_elapsed) => {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!(
                        "could not connect to {target} within {connect_timeout:?} \
                         ({attempts} attempt(s); the last was still pending)"
                    ),
                ));
            }
        };

        if attempts == 1 {
            eprintln!(
                "Could not connect to {target} ({error}); retrying every {}s for up to {connect_timeout:?}...",
                CONNECT_RETRY_INTERVAL.as_secs()
            );
        }
        let next_attempt = Instant::now() + CONNECT_RETRY_INTERVAL;
        let wake = deadline.map_or(next_attempt, |deadline| next_attempt.min(deadline));
        sleep_until(wake).await;
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            return Err(io::Error::new(
                error.kind(),
                format!(
                    "could not connect to {target} within {connect_timeout:?} \
                     ({attempts} attempt(s); last error: {error})"
                ),
            ));
        }
    }
}

/// Listens on the literal `ip:port` `target` and accepts one ASM connection
/// within `connect_timeout`. A hostname is rejected: binding needs one
/// specific local address, not a resolved list.
pub(crate) async fn listen_and_accept(
    target: &str,
    connect_timeout: Duration,
) -> io::Result<TcpStream> {
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
    Ok(stream)
}
