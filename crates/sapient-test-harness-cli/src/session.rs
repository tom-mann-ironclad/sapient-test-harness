//! Implements the `session` subcommand: a long-running, manually driven
//! session rather than `run`'s scripted scenario. The harness registers (as
//! an ASM) or accepts a registration (as a DMM), then keeps the connection
//! open: every inbound message is logged and validated, required replies are
//! sent automatically, an ASM sends StatusReports at its registered interval,
//! and typed commands on stdin inject detections, alerts, or tasks.
//!
//! Everything the harness sends goes through the same `sapient-session`
//! drivers `run` uses, so envelopes carry the current time and harness node
//! ID, and injected alerts and tasks are tracked and correlated with their
//! acknowledgements. The session log goes to stdout; connection progress
//! from the shared connect helpers goes to stderr, as it does for `run`.

use std::future::Future;
use std::io::{self, IsTerminal};
use std::path::Path;
use std::process::ExitCode;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use prost_reflect::MessageDescriptor;
use prost_types::Timestamp;
use rustyline::completion::FilenameCompleter;
use rustyline::error::ReadlineError;
use rustyline::history::DefaultHistory;
use rustyline::{
    Completer, Config, Editor, ExternalPrinter, Helper, Highlighter, Hinter, Validator,
};
use sapient_conformance_core::bsi_flex_335_v2_0::{
    SapientMessage, Task,
    sapient_message::Content,
    status_report::System,
    task::{Command as TaskCommand, Control, command::Command as TaskCommandKind},
};
use sapient_conformance_core::finding::{Finding, MessageContext, Severity};
use sapient_conformance_core::fixture_json::{
    decode_sapient_message_json, sapient_message_descriptor,
};
use sapient_conformance_core::validation::sapient_message::validate_sapient_message;
use sapient_session::{
    AsmEvent, AsmSessionState, DmmEvent, SessionState, asm::AsmConnection, dmm::DmmConnection,
    fixtures, framing::FrameReader,
};
use tokio::io::{AsyncRead, AsyncWrite, split};
use tokio::sync::mpsc;
use tokio::time::{Instant, sleep_until, timeout};
use ulid::Ulid;

use crate::cli::{Role, SessionArgs};
use crate::pretty::{Painter, Style, stdout_color};
use crate::report::{format_context_suffix, sanitize_for_terminal};
use crate::run::{connect_with_retries, listen_and_accept, validate_harness_node_id};
use crate::scenario::{asm_status, asm_status_period, scripted_alert, scripted_detection_report};
use crate::terminal::TerminalGuard;

const ASM_HELP: &str = "\
Commands:
  detection      send the scripted DetectionReport
  alert          send the scripted Alert
  status         send a StatusReport now (they're also sent automatically)
  send <file>    send a DetectionReport, Alert, or StatusReport from a
                 SapientMessage JSON file (e.g. examples/messages/from-edge-node/)
  help           show this list
  quit           send a GoodBye StatusReport and end the session";

const DMM_HELP: &str = "\
Commands:
  task [mode]    send a mode_change Task to `mode`, or by default to a
                 registered mode other than the active one
  send <file>    send a Task from a SapientMessage JSON file
                 (e.g. examples/messages/from-c2-node/02-task-mode-change.json)
  help           show this list
  quit           end the session";

pub async fn session(args: SessionArgs) -> ExitCode {
    if args.role == Role::Dmm && args.detection_interval_secs.is_some() {
        eprintln!("error: --detection-interval-secs only applies to --role asm");
        return ExitCode::from(2);
    }
    let harness_node_id = args
        .node_id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    if let Err(err) = validate_harness_node_id(&harness_node_id) {
        eprintln!("error: {err}");
        return ExitCode::from(2);
    }

    let connect_timeout = Duration::from_secs(args.connect_timeout_secs);
    let stream = match args.role {
        Role::Asm => {
            eprintln!("Connecting to {}...", args.target);
            connect_with_retries(&args.target, connect_timeout).await
        }
        Role::Dmm => listen_and_accept(&args.target, connect_timeout).await,
    };
    let stream = match stream {
        Ok(stream) => stream,
        Err(err) => {
            eprintln!("error: {err}");
            return ExitCode::from(2);
        }
    };

    let (reader, writer) = split(stream);
    let frame_reader = FrameReader::new(args.max_frame_bytes)
        .with_large_message_warning(crate::cli::warn_large_message);
    let console = Console::start();
    let mut log = Log::new(console.printer);
    let mut commands = console.commands;
    let options = Options {
        harness_node_id: harness_node_id.clone(),
        write_timeout: Duration::from_secs(args.write_timeout_secs),
        detection_interval: args.detection_interval_secs.map(Duration::from_secs),
        descriptor: sapient_message_descriptor(),
    };
    log.line(&format!(
        "Session started as {} (node ID {harness_node_id}). Type `help` for commands.",
        args.role.to_string().to_uppercase()
    ));

    let outcome = match args.role {
        Role::Asm => {
            let mut connection =
                AsmConnection::with_frame_reader(&harness_node_id, reader, writer, frame_reader);
            let outcome = asm_session(&mut connection, &options, &mut log, &mut commands).await;
            log.findings(connection.findings());
            outcome.map(|()| connection.take_findings())
        }
        Role::Dmm => {
            let mut connection =
                DmmConnection::with_frame_reader(&harness_node_id, reader, writer, frame_reader)
                    .with_allowed_status_report_intervals(args.allowed_status_report_intervals);
            let outcome = dmm_session(&mut connection, &options, &mut log, &mut commands).await;
            log.findings(connection.findings());
            outcome.map(|()| connection.take_findings())
        }
    };

    let exit_code = match outcome {
        Ok(findings) => {
            let (errors, error_occurrences) = count(&findings, Severity::Error);
            let (warnings, warning_occurrences) = count(&findings, Severity::Warning);
            log.line(&format!(
                "Session ended: {errors} error finding(s) ({error_occurrences} occurrence(s)), \
                 {warnings} warning(s) ({warning_occurrences} occurrence(s))."
            ));
            if errors > 0 {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            }
        }
        Err(err) => {
            log.error(&format!("Session ended by an I/O error: {err}"));
            ExitCode::from(2)
        }
    };
    // The editor thread may still be blocked in raw mode; put the terminal
    // back and leave the shell prompt on a fresh line below the `> ` prompt.
    if let Some(guard) = console.terminal {
        drop(log);
        drop(guard);
        println!();
    }
    exit_code
}

struct Options {
    harness_node_id: String,
    write_timeout: Duration,
    detection_interval: Option<Duration>,
    descriptor: MessageDescriptor,
}

/// What a command asks the session loop to do next.
enum Flow {
    Continue,
    Quit,
}

async fn asm_session<R, W>(
    connection: &mut AsmConnection<R, W>,
    options: &Options,
    log: &mut Log,
    commands: &mut mpsc::UnboundedReceiver<Input>,
) -> io::Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let status_interval = asm_status_period();
    write(connection.register(fixtures::valid_registration()), options).await?;
    log.sent("Registration", None);
    log.findings(connection.findings());

    // Both disabled until registration is accepted.
    let mut next_status: Option<Instant> = None;
    let mut next_detection: Option<Instant> = None;
    let mut commands_open = true;
    let mut ctrl_c = std::pin::pin!(tokio::signal::ctrl_c());

    loop {
        let next_timer = next_status.into_iter().chain(next_detection).min();
        tokio::select! {
            _ = &mut ctrl_c => {
                log.line("Interrupted (Ctrl-C).");
                return asm_goodbye(connection, options, log).await;
            }
            input = commands.recv(), if commands_open => match input {
                Some(Input::Line(line)) => {
                    if let Flow::Quit = asm_command(connection, options, log, &line).await? {
                        return asm_goodbye(connection, options, log).await;
                    }
                }
                Some(Input::End) => return asm_goodbye(connection, options, log).await,
                None => {
                    commands_open = false;
                    log.line("stdin closed: no more commands. Ctrl-C ends the session.");
                }
            },
            polled = connection.poll_once() => {
                if !polled? {
                    log.line("DMM disconnected.");
                    return Ok(());
                }
                log.received(connection.last_inbound(), connection.last_reply());
                log.findings(connection.findings());
                match connection.take_event() {
                    Some(AsmEvent::RegistrationAccepted) => {
                        log.line(&format!(
                            "Registration accepted. Sending a StatusReport every {status_interval:?}{}.",
                            options
                                .detection_interval
                                .map(|interval| format!(" and a DetectionReport every {interval:?}"))
                                .unwrap_or_default()
                        ));
                        send_status(connection, options, log, System::Ok).await?;
                        next_status = Some(Instant::now() + status_interval);
                        next_detection = options.detection_interval.map(|interval| Instant::now() + interval);
                    }
                    Some(AsmEvent::RegistrationRejected) => {
                        log.error("The DMM rejected our Registration; ending the session.");
                        return Ok(());
                    }
                    Some(AsmEvent::RegistrationFailed) => {
                        log.error("Registration failed (see findings); ending the session.");
                        return Ok(());
                    }
                    Some(AsmEvent::AlertAcknowledged { alert_id }) => {
                        log.line(&format!("Alert {alert_id} acknowledged."));
                    }
                    None => {}
                }
            }
            () = sleep_until(next_timer.unwrap_or_else(Instant::now)), if next_timer.is_some() => {
                if !matches!(connection.state(), AsmSessionState::Registered(_)) {
                    next_status = None;
                    next_detection = None;
                    continue;
                }
                let now = Instant::now();
                if next_status.is_some_and(|due| now >= due) {
                    send_status(connection, options, log, System::Ok).await?;
                    next_status = Some(now + status_interval);
                }
                if let Some(interval) = options.detection_interval
                    && next_detection.is_some_and(|due| now >= due)
                {
                    send_detection(connection, options, log, scripted_detection_report()).await?;
                    next_detection = Some(now + interval);
                }
            }
        }
    }
}

async fn asm_command<R, W>(
    connection: &mut AsmConnection<R, W>,
    options: &Options,
    log: &mut Log,
    line: &str,
) -> io::Result<Flow>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let (command, argument) = split_command(line);
    let registered = matches!(connection.state(), AsmSessionState::Registered(_));
    match command {
        "" => {}
        "help" => log.raw(ASM_HELP),
        "quit" | "exit" => return Ok(Flow::Quit),
        "status" | "detection" | "alert" | "send" if !registered => {
            log.error("Not registered yet; wait for the RegistrationAck.");
        }
        "status" => send_status(connection, options, log, System::Ok).await?,
        "detection" => {
            send_detection(connection, options, log, scripted_detection_report()).await?;
        }
        "alert" => {
            let alert = scripted_alert(Ulid::new().to_string());
            validate_before_send(log, options, Content::Alert(alert.clone()));
            write(connection.issue_alert(alert.clone()), options).await?;
            log.sent("Alert", alert.alert_id.as_deref());
        }
        "send" => {
            let Some(message) = load_for_send(log, options, argument) else {
                return Ok(Flow::Continue);
            };
            match message.content {
                Some(Content::DetectionReport(detection)) => {
                    send_detection(connection, options, log, detection).await?;
                }
                Some(Content::Alert(mut alert)) => {
                    alert.alert_id = Some(Ulid::new().to_string());
                    validate_before_send(log, options, Content::Alert(alert.clone()));
                    write(connection.issue_alert(alert.clone()), options).await?;
                    log.sent("Alert", alert.alert_id.as_deref());
                }
                Some(Content::StatusReport(mut status)) => {
                    status.report_id = Some(Ulid::new().to_string());
                    validate_before_send(log, options, Content::StatusReport(status.clone()));
                    write(connection.issue_status_report(status.clone()), options).await?;
                    log.sent("StatusReport", status.report_id.as_deref());
                }
                other => log.error(&format!(
                    "The ASM role can send a DetectionReport, Alert, or StatusReport here, not {}. \
                     Use `sapient-harness send` for arbitrary raw messages.",
                    content_name(other.as_ref())
                )),
            }
        }
        unknown => log.error(&format!("Unknown command {unknown:?}. Type `help`.")),
    }
    Ok(Flow::Continue)
}

/// Send a GoodBye StatusReport if still registered, ending the session cleanly.
async fn asm_goodbye<R, W>(
    connection: &mut AsmConnection<R, W>,
    options: &Options,
    log: &mut Log,
) -> io::Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    if matches!(connection.state(), AsmSessionState::Registered(_)) {
        send_status(connection, options, log, System::Goodbye).await?;
    }
    Ok(())
}

async fn send_status<R, W>(
    connection: &mut AsmConnection<R, W>,
    options: &Options,
    log: &mut Log,
    system: System,
) -> io::Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let status = asm_status(connection.state(), system);
    write(connection.issue_status_report(status.clone()), options).await?;
    let label = if system == System::Goodbye {
        "StatusReport (GoodBye)"
    } else {
        "StatusReport"
    };
    log.sent(label, status.report_id.as_deref());
    Ok(())
}

/// Send a DetectionReport with a fresh report ID. The object ID is kept, so
/// resending a file reports the same tracked object again.
async fn send_detection<R, W>(
    connection: &mut AsmConnection<R, W>,
    options: &Options,
    log: &mut Log,
    mut detection: sapient_conformance_core::bsi_flex_335_v2_0::DetectionReport,
) -> io::Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    detection.report_id = Some(Ulid::new().to_string());
    validate_before_send(log, options, Content::DetectionReport(detection.clone()));
    write(
        connection.issue_detection_report(detection.clone()),
        options,
    )
    .await?;
    log.sent("DetectionReport", detection.report_id.as_deref());
    Ok(())
}

async fn dmm_session<R, W>(
    connection: &mut DmmConnection<R, W>,
    options: &Options,
    log: &mut Log,
    commands: &mut mpsc::UnboundedReceiver<Input>,
) -> io::Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    log.line("Waiting for the ASM's Registration.");
    let mut commands_open = true;
    let mut ctrl_c = std::pin::pin!(tokio::signal::ctrl_c());

    loop {
        tokio::select! {
            _ = &mut ctrl_c => {
                log.line("Interrupted (Ctrl-C).");
                return Ok(());
            }
            input = commands.recv(), if commands_open => match input {
                Some(Input::Line(line)) => {
                    if let Flow::Quit = dmm_command(connection, options, log, &line).await? {
                        return Ok(());
                    }
                }
                Some(Input::End) => return Ok(()),
                None => {
                    commands_open = false;
                    log.line("stdin closed: no more commands. Ctrl-C ends the session.");
                }
            },
            polled = connection.poll_once() => {
                if !polled? {
                    log.line("ASM disconnected.");
                    return Ok(());
                }
                log.received(connection.last_inbound(), connection.last_reply());
                log.findings(connection.findings());
                match connection.take_event() {
                    Some(DmmEvent::RegistrationAccepted) => {
                        if let SessionState::Registered(contract) = connection.state() {
                            log.line(&format!(
                                "Registration accepted; active mode {:?}.",
                                contract.active_mode.mode_name.as_deref().unwrap_or("")
                            ));
                        }
                    }
                    Some(DmmEvent::RegistrationRejected) => {
                        log.error("Registration rejected (see findings); waiting for a valid one.");
                    }
                    Some(DmmEvent::TaskAcknowledged { task_id }) => {
                        log.line(&format!("Task {task_id} acknowledged."));
                    }
                    Some(DmmEvent::GoodbyeReceived) => {
                        log.line("The ASM sent GoodBye; waiting for it to re-register or disconnect.");
                    }
                    Some(DmmEvent::StatusReportValidated) | None => {}
                }
            }
        }
    }
}

async fn dmm_command<R, W>(
    connection: &mut DmmConnection<R, W>,
    options: &Options,
    log: &mut Log,
    line: &str,
) -> io::Result<Flow>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let (command, argument) = split_command(line);
    let contract = match connection.state() {
        SessionState::Registered(contract) => Some(contract),
        _ => None,
    };
    match command {
        "" => {}
        "help" => log.raw(DMM_HELP),
        "quit" | "exit" => return Ok(Flow::Quit),
        "task" | "send" if contract.is_none() => {
            log.error("The ASM isn't registered yet.");
        }
        "task" => {
            let target_mode = if argument.is_empty() {
                contract.and_then(|contract| {
                    contract
                        .registration
                        .mode_definition
                        .iter()
                        .find(|mode| mode.mode_name != contract.active_mode.mode_name)
                        .and_then(|mode| mode.mode_name.clone())
                })
            } else {
                Some(argument.to_string())
            };
            let Some(target_mode) = target_mode else {
                log.error("The registration declares no other mode; name one: `task <mode>`.");
                return Ok(Flow::Continue);
            };
            let task = Task {
                task_id: Some(Ulid::new().to_string()),
                control: Some(Control::Start as i32),
                command: Some(TaskCommand {
                    command: Some(TaskCommandKind::ModeChange(target_mode.clone())),
                    command_parameter: None,
                }),
                ..Default::default()
            };
            send_task(connection, options, log, task, Some(&target_mode)).await?;
        }
        "send" => {
            let Some(message) = load_for_send(log, options, argument) else {
                return Ok(Flow::Continue);
            };
            match message.content {
                Some(Content::Task(mut task)) => {
                    task.task_id = Some(Ulid::new().to_string());
                    send_task(connection, options, log, task, None).await?;
                }
                other => log.error(&format!(
                    "The DMM role can send a Task here, not {}. Use `sapient-harness send` \
                     for arbitrary raw messages.",
                    content_name(other.as_ref())
                )),
            }
        }
        unknown => log.error(&format!("Unknown command {unknown:?}. Type `help`.")),
    }
    Ok(Flow::Continue)
}

async fn send_task<R, W>(
    connection: &mut DmmConnection<R, W>,
    options: &Options,
    log: &mut Log,
    task: Task,
    target_mode: Option<&str>,
) -> io::Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    validate_before_send(log, options, Content::Task(task.clone()));
    write(connection.issue_task(&task), options).await?;
    let label = match target_mode {
        Some(mode) => format!("Task (mode_change to {mode:?})"),
        None => "Task".to_string(),
    };
    log.sent(&label, task.task_id.as_deref());
    log.findings(connection.findings());
    Ok(())
}

/// Bound a send by `--write-timeout-secs`. A timed-out write leaves a partial
/// frame on the wire, so the session must end rather than continue.
async fn write(send: impl Future<Output = io::Result<()>>, options: &Options) -> io::Result<()> {
    timeout(options.write_timeout, send).await.map_err(|_| {
        io::Error::new(
            io::ErrorKind::TimedOut,
            format!(
                "a write didn't complete within {:?}; the connection can't be reused",
                options.write_timeout
            ),
        )
    })?
}

/// Load `path` as a SapientMessage for `send`, logging any problem instead
/// of ending the session.
fn load_for_send(log: &mut Log, options: &Options, path: &str) -> Option<SapientMessage> {
    if path.is_empty() {
        log.error("Usage: send <file>");
        return None;
    }
    let path = Path::new(path);
    let json = match std::fs::read_to_string(path) {
        Ok(json) => json,
        Err(err) => {
            log.error(&format!("Couldn't read {}: {err}", path.display()));
            return None;
        }
    };
    match decode_sapient_message_json(&json, &options.descriptor) {
        Ok(message) => Some(message),
        Err(err) => {
            log.error(&format!(
                "{} isn't a valid SapientMessage: {err}",
                path.display()
            ));
            None
        }
    }
}

/// Validate `content` as it will go out (current timestamp, harness node ID)
/// and warn about any finding. Like `send`, it's sent regardless.
fn validate_before_send(log: &mut Log, options: &Options, content: Content) {
    let message = SapientMessage {
        timestamp: Some(now_timestamp()),
        node_id: Some(options.harness_node_id.clone()),
        destination_id: None,
        content: Some(content),
        additional_information: None,
    };
    let outcome = validate_sapient_message(message);
    if !outcome.passed {
        log.warn("This message doesn't conform to the harness's own rules; sending it anyway:");
        for finding in &outcome.findings {
            log.finding(finding);
        }
    }
}

/// Split a command line into its first word and the (trimmed) rest.
fn split_command(line: &str) -> (&str, &str) {
    let line = line.trim();
    match line.split_once(char::is_whitespace) {
        Some((command, argument)) => (command, argument.trim()),
        None => (line, ""),
    }
}

fn content_name(content: Option<&Content>) -> &'static str {
    match content {
        None => "a message with no content",
        Some(Content::Registration(_)) => "a Registration",
        Some(Content::RegistrationAck(_)) => "a RegistrationAck",
        Some(Content::StatusReport(_)) => "a StatusReport",
        Some(Content::DetectionReport(_)) => "a DetectionReport",
        Some(Content::Task(_)) => "a Task",
        Some(Content::TaskAck(_)) => "a TaskAck",
        Some(Content::Alert(_)) => "an Alert",
        Some(Content::AlertAck(_)) => "an AlertAck",
        Some(Content::Error(_)) => "an Error",
    }
}

/// Distinct findings of `severity`, and their total occurrences counting
/// repeats the session folded together.
fn count(findings: &[Finding], severity: Severity) -> (usize, u64) {
    findings
        .iter()
        .filter(|f| f.severity == severity)
        .fold((0, 0), |(distinct, total), f| {
            (distinct + 1, total + u64::from(f.occurrences.max(1)))
        })
}

/// Something typed by the user.
enum Input {
    /// A command line.
    Line(String),
    /// Ctrl-C or Ctrl-D in the line editor: end the session as `quit` does.
    /// (The editor reads keys in raw mode, so Ctrl-C arrives as a key, not
    /// as the signal the session loop also listens for.)
    End,
}

/// Where commands come from and how log lines reach the terminal.
struct Console {
    commands: mpsc::UnboundedReceiver<Input>,
    /// The line editor's printer, which prints above the `> ` prompt and
    /// redraws any partly typed command below it. `None` for plain stdin.
    printer: Option<Box<dyn ExternalPrinter + Send>>,
    /// Restores the terminal at the end of an interactive session.
    terminal: Option<TerminalGuard>,
}

/// Completes file paths, for `send <file>`.
#[derive(Helper, Completer, Hinter, Highlighter, Validator)]
struct CommandHelper {
    #[rustyline(Completer)]
    completer: FilenameCompleter,
}

impl Console {
    /// A line editor (prompt, history, path completion) when stdin and
    /// stdout are both terminals; otherwise plain lines, so piped or
    /// scripted commands work unchanged.
    fn start() -> Self {
        if std::io::stdin().is_terminal() && std::io::stdout().is_terminal() {
            let terminal = TerminalGuard::save();
            match Self::start_editor() {
                Ok((commands, printer)) => {
                    return Console {
                        commands,
                        printer: Some(printer),
                        terminal,
                    };
                }
                Err(err) => {
                    eprintln!("warning: line editor unavailable ({err}); using plain input")
                }
            }
        }
        Console {
            commands: spawn_stdin_reader(),
            printer: None,
            terminal: None,
        }
    }

    #[allow(clippy::type_complexity)]
    fn start_editor() -> rustyline::Result<(
        mpsc::UnboundedReceiver<Input>,
        Box<dyn ExternalPrinter + Send>,
    )> {
        let config = Config::builder().auto_add_history(true).build();
        let mut editor: Editor<CommandHelper, DefaultHistory> = Editor::with_config(config)?;
        editor.set_helper(Some(CommandHelper {
            completer: FilenameCompleter::new(),
        }));
        let printer = editor.create_external_printer()?;
        let (sender, receiver) = mpsc::unbounded_channel();
        std::thread::spawn(move || {
            loop {
                let input = match editor.readline("> ") {
                    Ok(line) => Input::Line(line),
                    Err(ReadlineError::Interrupted | ReadlineError::Eof) => Input::End,
                    Err(_) => break,
                };
                let end = matches!(input, Input::End);
                if sender.send(input).is_err() || end {
                    break;
                }
            }
        });
        Ok((receiver, Box::new(printer)))
    }
}

/// Read stdin lines on a plain thread. A tokio stdin read can't be
/// cancelled, so it would keep the runtime from shutting down at the end of
/// the session until Enter was pressed; a detached thread doesn't.
fn spawn_stdin_reader() -> mpsc::UnboundedReceiver<Input> {
    let (sender, receiver) = mpsc::unbounded_channel();
    std::thread::spawn(move || {
        for line in std::io::stdin().lines() {
            let Ok(line) = line else { break };
            if sender.send(Input::Line(line)).is_err() {
                break;
            }
        }
    });
    receiver
}

fn now_timestamp() -> Timestamp {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    Timestamp {
        seconds: now.as_secs() as i64,
        nanos: now.subsec_nanos() as i32,
    }
}

/// Timestamped session log on stdout. Findings are printed once each, as the
/// session records them; peer-supplied text is sanitized for the terminal.
struct Log {
    painter: Painter,
    /// The line editor's printer in an interactive session; see [`Console`].
    printer: Option<Box<dyn ExternalPrinter + Send>>,
    shown_findings: usize,
    /// `occurrences` of the last shown finding when it was shown. The
    /// session folds an exact repeat into that entry instead of appending,
    /// so a growing count is a new occurrence to show.
    last_shown_occurrences: u32,
}

impl Log {
    fn new(printer: Option<Box<dyn ExternalPrinter + Send>>) -> Self {
        Log {
            painter: Painter {
                color: stdout_color(),
            },
            printer,
            shown_findings: 0,
            last_shown_occurrences: 0,
        }
    }

    fn line(&mut self, text: &str) {
        let clock = self.painter.paint(Style::Dim, &utc_clock());
        self.raw(&format!("{clock} {text}"));
    }

    /// Print `text` as-is, above the prompt in an interactive session.
    fn raw(&mut self, text: &str) {
        // The printer adds no newline of its own when the editor isn't
        // mid-`readline`, so always supply one.
        if let Some(printer) = &mut self.printer
            && printer.print(format!("{text}\n")).is_ok()
        {
            return;
        }
        println!("{text}");
    }

    fn error(&mut self, text: &str) {
        self.line(&format!("{} {text}", self.painter.fail()));
    }

    fn warn(&mut self, text: &str) {
        self.line(&format!("{} {text}", self.painter.warn()));
    }

    fn sent(&mut self, message_type: &str, id: Option<&str>) {
        let id = id.map(|id| format!(" {id}")).unwrap_or_default();
        self.line(&format!(
            "{} {message_type}{}",
            self.painter.paint(Style::Bold, "→"),
            self.painter.paint(Style::Dim, &id)
        ));
    }

    fn received(&mut self, inbound: Option<&MessageContext>, reply: Option<&str>) {
        let Some(inbound) = inbound else { return };
        let reply = reply
            .map(|reply| {
                format!(
                    "  {} {reply} (automatic)",
                    self.painter.paint(Style::Bold, "→")
                )
            })
            .unwrap_or_default();
        self.line(&format!(
            "{} {}{}{reply}",
            self.painter.paint(Style::Bold, "←"),
            inbound.message_type,
            self.painter
                .paint(Style::Dim, &format!(" #{}", inbound.sequence))
        ));
    }

    /// Print findings recorded since the last call, including new repeats
    /// folded into the last one shown.
    fn findings(&mut self, findings: &[Finding]) {
        let start = self.shown_findings.min(findings.len());
        if let Some(last) = start.checked_sub(1).map(|index| &findings[index])
            && last.occurrences > self.last_shown_occurrences
        {
            self.finding(last);
        }
        for finding in &findings[start..] {
            self.finding(finding);
        }
        self.shown_findings = findings.len();
        self.last_shown_occurrences = findings.last().map_or(0, |last| last.occurrences);
    }

    fn finding(&mut self, finding: &Finding) {
        let marker = if finding.severity == Severity::Error {
            self.painter.fail()
        } else {
            self.painter.warn()
        };
        self.line(&format!(
            "  {marker} [{}] {}: {}{}",
            finding.rule_id,
            finding.field_path,
            sanitize_for_terminal(&finding.message),
            self.painter
                .paint(Style::Dim, &format_context_suffix(finding))
        ));
    }
}

/// Current UTC wall-clock time as `HH:MM:SS.mmmZ`, matching the UTC
/// timestamps SAPIENT messages carry.
fn utc_clock() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let seconds_today = now.as_secs() % 86_400;
    format!(
        "{:02}:{:02}:{:02}.{:03}Z",
        seconds_today / 3600,
        seconds_today / 60 % 60,
        seconds_today % 60,
        now.subsec_millis()
    )
}
