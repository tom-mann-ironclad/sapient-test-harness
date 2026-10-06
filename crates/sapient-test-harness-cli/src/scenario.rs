//! The bundled default ("v2.0") scenario: drives a real target
//! implementation through the full message set over one continuous
//! session, mirroring `sapient-session`'s own `dmm_asm_interop.rs` test --
//! except here the peer is an arbitrary developer's implementation, not
//! our own fixture-driven counterpart, so these scripts can only react to
//! whatever it actually does, not assert a specific shape from it.
//!
//! DMM role and ASM role are deliberately asymmetric in how they end a
//! run, matching the session model in ROADMAP.md: the ASM is the party
//! that manages a session's lifecycle (it's the one a `GoodBye`
//! `StatusReport` comes from), so a DMM-role run never disconnects first --
//! it only stops engaging once the peer disconnects, sends `GoodBye`, or
//! the run's `max_runtime` elapses (a bounded give-up, not a scripted
//! hang-up). An ASM-role run, conversely, is free to conclude itself, and
//! does so the correct way: an explicit `GoodBye` `StatusReport` before
//! disconnecting.
//!
//! Progress is streamed to stderr as it happens (`note`, a heartbeat during
//! any wait long enough to otherwise look hung, and every newly observed
//! conformance finding -- see `stream_new_findings`) rather than only shown
//! at the end -- stdout stays reserved for the final report (text or
//! `--format json`), matching `run.rs`'s own connect/listen messages. A
//! Ctrl-C during either scenario (see `run_dmm_scenario`/`run_asm_scenario`)
//! stops the run and still produces a report of whatever was observed, the
//! same way reaching the deadline already did -- it is never a silent kill.

use std::io;
use std::time::Duration;

use crate::completion::{Check, ScenarioResult};

use crate::cli::Role;
use sapient_conformance_core::bsi_flex_335_v2_0::{
    Alert, DetectionReport, StatusReport, Task,
    alert::{AlertStatus, AlertType},
    detection_report::{DetectionReportClassification, LocationOneof as DetectionLocationOneof},
    status_report::System,
    task::{Command, command::Command as TaskCommandKind},
};
use sapient_conformance_core::finding::Finding;
use sapient_session::{
    AsmEvent, AsmSessionState, DmmEvent, SessionState, asm::AsmConnection, dmm::DmmConnection,
    fixtures,
};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::time::{Instant, timeout, timeout_at};
use ulid::Ulid;

/// How often a long wait (see `dmm_poll_with_heartbeat`/
/// `asm_poll_with_heartbeat`) prints a liveness line to stderr while it's
/// still waiting for the next message.
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);

/// Records `message` in `notes` (the structured record returned in the
/// final report) and echoes it to stderr immediately, so a developer
/// watching the run sees each protocol event as it happens rather than
/// only in the summary at the end.
fn note(notes: &mut Vec<String>, message: impl Into<String>) {
    let message = message.into();
    eprintln!("{message}");
    notes.push(message);
}

/// Records why a run stopped on Ctrl-C: `tokio::signal::ctrl_c()` itself
/// failing (the OS refused to let the harness install a signal handler) is
/// vanishingly unlikely but still worth distinguishing from an actual
/// interrupt in the note, rather than treating both identically.
fn note_interrupted(notes: &mut Vec<String>, ctrl_c: io::Result<()>) {
    match ctrl_c {
        Ok(()) => note(
            notes,
            "Interrupted (Ctrl-C); finalizing the report with whatever was observed so far.",
        ),
        Err(err) => note(
            notes,
            format!(
                "Stopping early: could not listen for Ctrl-C ({err}); finalizing the report \
                 with whatever was observed so far."
            ),
        ),
    }
}

/// Echoes every finding added to `findings` since the last call (tracked by
/// `shown`, a count rather than a cursor type since a session's findings
/// only ever grow during a run) to stderr immediately -- e.g. a
/// registration-rejection reason, or any other session-level finding --
/// rather than leaving it buried in the session until the final report.
/// stdout (reserved for `--format json`) is untouched; sanitized the same
/// way the final text report is (KI-021), since this text can embed
/// arbitrary peer-supplied content.
fn stream_new_findings(findings: &[Finding], shown: &mut usize) {
    for finding in &findings[*shown..] {
        eprintln!(
            "[{:?}] [{}] {}: {}{}",
            finding.severity,
            finding.rule_id,
            finding.field_path,
            crate::report::sanitize_for_terminal(&finding.message),
            crate::report::format_context_suffix(finding)
        );
    }
    *shown = findings.len();
}

/// Outcome of a heartbeat-monitored wait for the next inbound message.
enum PollWait {
    Processed,
    Disconnected,
    DeadlineReached,
}

async fn dmm_poll_with_heartbeat<R, W>(
    connection: &mut DmmConnection<R, W>,
    deadline: Instant,
    waiting_for: &str,
) -> io::Result<PollWait>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Ok(PollWait::DeadlineReached);
        }
        let wait = remaining.min(HEARTBEAT_INTERVAL);
        match timeout(wait, connection.poll_once()).await {
            Ok(Ok(true)) => return Ok(PollWait::Processed),
            Ok(Ok(false)) => return Ok(PollWait::Disconnected),
            Ok(Err(err)) => return Err(err),
            Err(_elapsed) if wait < remaining => {
                let remaining = deadline.saturating_duration_since(Instant::now()).as_secs();
                eprintln!(
                    "... still waiting {waiting_for} ({remaining}s left before this run gives up)"
                );
            }
            Err(_elapsed) => return Ok(PollWait::DeadlineReached),
        }
    }
}

/// How often the harness's ASM sends ordinary StatusReports: 90% of the
/// registered interval. A DMM measures the gap between consecutive reports'
/// own timestamps against the declared interval with no tolerance, so
/// scheduling exactly one interval after the previous send would overrun it
/// by timer and write latency.
pub(crate) fn asm_status_period() -> Duration {
    Duration::from_secs_f32(fixtures::STATUS_INTERVAL_SECONDS * 0.9)
}

/// Build ordinary and closing reports from the session's current mode, rather
/// than the mode captured at registration (tasks can change it while waiting).
pub(crate) fn asm_status(state: &AsmSessionState, system: System) -> StatusReport {
    let mode = match state {
        AsmSessionState::Registered(contract) => contract.active_mode.mode_name.clone(),
        _ => None,
    };
    StatusReport {
        report_id: Some(Ulid::new().to_string()),
        system: Some(system as i32),
        info: Some(System::Ok as i32),
        mode,
        ..Default::default()
    }
}

/// The scripted DetectionReport: a fresh report and object ID, located and
/// classified consistently with [`fixtures::valid_registration`].
pub(crate) fn scripted_detection_report() -> DetectionReport {
    DetectionReport {
        report_id: Some(Ulid::new().to_string()),
        object_id: Some(Ulid::new().to_string()),
        task_id: None,
        state: None,
        location_oneof: Some(DetectionLocationOneof::RangeBearing(
            fixtures::detection_position(),
        )),
        detection_confidence: None,
        track_info: vec![],
        prediction_location: None,
        object_info: vec![],
        classification: vec![DetectionReportClassification {
            r#type: Some(fixtures::DECLARED_CLASSIFICATION_TYPE.to_string()),
            confidence: None,
            sub_class: vec![],
        }],
        behaviour: vec![],
        associated_file: vec![],
        signal: vec![],
        associated_detection: vec![],
        derived_detection: vec![],
        velocity_oneof: None,
        colour: None,
        id: None,
    }
}

/// The scripted information Alert, with the caller's correlation ID.
pub(crate) fn scripted_alert(alert_id: String) -> Alert {
    Alert {
        alert_id: Some(alert_id),
        alert_type: Some(AlertType::Information as i32),
        status: Some(AlertStatus::Active as i32),
        description: None,
        location_oneof: None,
        region_id: None,
        priority: None,
        ranking: None,
        confidence: None,
        associated_file: vec![],
        associated_detection: vec![],
        additional_information: None,
    }
}

/// Receive while servicing the registered ASM's status cadence. Cancellation
/// retains framing state in the driver; automatic replies finish before status
/// writes. Backpressure remains bounded by the outer run deadline.
async fn asm_poll_with_heartbeat<R, W>(
    connection: &mut AsmConnection<R, W>,
    deadline: Instant,
    waiting_for: &str,
    next_status: &mut Option<Instant>,
    stage: &mut &'static str,
) -> io::Result<PollWait>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut next_heartbeat = Instant::now() + HEARTBEAT_INTERVAL;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Ok(PollWait::DeadlineReached);
        }
        if next_status.is_some_and(|due| Instant::now() >= due) {
            if matches!(connection.state(), AsmSessionState::Registered(_)) {
                *stage = "send_periodic_status";
                let status = asm_status(connection.state(), System::Ok);
                connection.issue_status_report(status).await?;
                *next_status = Some(Instant::now() + asm_status_period());
            } else {
                *next_status = None;
            }
            // Recompute the remaining wait after a potentially blocked send.
            continue;
        }
        let status_wait = next_status
            .map(|due| due.saturating_duration_since(Instant::now()))
            .unwrap_or(remaining);
        let heartbeat_wait = next_heartbeat.saturating_duration_since(Instant::now());
        let wait = remaining.min(heartbeat_wait).min(status_wait);
        *stage = "receive_or_reply";
        match timeout(wait, connection.poll_once()).await {
            Ok(Ok(true)) => return Ok(PollWait::Processed),
            Ok(Ok(false)) => return Ok(PollWait::Disconnected),
            Ok(Err(err)) => return Err(err),
            Err(_elapsed) if wait < remaining => {
                if Instant::now() < next_heartbeat {
                    continue;
                }
                next_heartbeat = Instant::now() + HEARTBEAT_INTERVAL;
                let remaining = deadline.saturating_duration_since(Instant::now()).as_secs();
                eprintln!(
                    "... still waiting {waiting_for} ({remaining}s left before this run gives up)"
                );
            }
            Err(_elapsed) => return Ok(PollWait::DeadlineReached),
        }
    }
}

/// Drives a DMM-role run against an ASM under test until it ends the
/// session itself (disconnect or `GoodBye`) or `deadline` passes. Once
/// registered, proactively issues one `mode_change` `Task` (targeting
/// whatever non-default mode the ASM's own `Registration` declared, if
/// any) to exercise `Task`/`TaskAck` and mode-change handling; everything
/// else is reactive, auto-replying via `DmmConnection`.
/// Returns partial progress and operational errors even if I/O fails. All waits
/// and writes share `deadline`. Drop the connection when this run returns;
/// pending frames must not be reused after a run-ending deadline or I/O error.
pub async fn run_dmm_scenario<R, W>(
    connection: &mut DmmConnection<R, W>,
    deadline: Instant,
) -> ScenarioResult
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut result = ScenarioResult::for_role(Role::Dmm);
    let mut stage = "starting_scenario";
    // Progress lives outside the cancellable future so every exit retains it
    // -- including a Ctrl-C, not just reaching the deadline: dropping the
    // `dmm_steps` future on either branch of this `select!` cannot discard
    // `result`, since it's owned by this function's own stack frame, not by
    // the future being raced and dropped.
    let outcome = tokio::select! {
        ctrl_c = tokio::signal::ctrl_c() => {
            note_interrupted(&mut result.notes, ctrl_c);
            None
        }
        outcome = timeout_at(
            deadline,
            dmm_steps(connection, deadline, &mut result, &mut stage),
        ) => Some(outcome),
    };
    match outcome {
        None => {}
        Some(Ok(Err(error))) => result.record_error(stage, error),
        Some(Err(_)) if connection.has_pending_write() => result.record_error(
            stage,
            io::Error::new(
                io::ErrorKind::TimedOut,
                "run deadline expired with an unfinished write; connection must be closed",
            ),
        ),
        Some(Err(_)) => note(&mut result.notes, "Reached the run's max runtime."),
        Some(Ok(Ok(()))) => {}
    }
    // A heartbeat/optional wait may observe the same deadline before the outer timer.
    if result.operational_error.is_none() && connection.has_pending_write() {
        result.record_error(
            stage,
            io::Error::new(
                io::ErrorKind::TimedOut,
                "run ended with an unfinished reply; connection must be closed",
            ),
        );
    }
    result
}

async fn dmm_steps<R, W>(
    connection: &mut DmmConnection<R, W>,
    deadline: Instant,
    result: &mut ScenarioResult,
    stage: &mut &'static str,
) -> io::Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut status_received = false;
    let mut target_mode = None;
    let mut issued_task_id = None;
    let mut shown_findings = 0;

    loop {
        if Instant::now() >= deadline {
            note(
                &mut result.notes,
                "Reached the run's max runtime while observing the ASM.",
            );
            break;
        }

        // Wait for a validated ordinary status report before issuing the probe.
        // Completion of an issued task is observed explicitly: clearing the
        // session contract must never stand in for a matching TaskAck.
        if status_received
            && issued_task_id.is_none()
            && let Some(target_mode) = target_mode.take()
        {
            note(
                &mut result.notes,
                format!("Issuing a mode_change Task targeting {target_mode:?}."),
            );
            let task_id = Ulid::new().to_string();
            let task = Task {
                task_id: Some(task_id.clone()),
                control: Some(
                    sapient_conformance_core::bsi_flex_335_v2_0::task::Control::Start as i32,
                ),
                command: Some(Command {
                    command: Some(TaskCommandKind::ModeChange(target_mode)),
                    command_parameter: None,
                }),
                ..Default::default()
            };
            *stage = "send_task";
            connection.issue_task(&task).await?;
            stream_new_findings(connection.findings(), &mut shown_findings);
            issued_task_id = Some(task_id);
        }

        *stage = "receive_or_reply";
        let poll_result = dmm_poll_with_heartbeat(connection, deadline, "for the ASM").await?;
        stream_new_findings(connection.findings(), &mut shown_findings);
        match poll_result {
            PollWait::Processed => match connection.take_event() {
                Some(DmmEvent::RegistrationAccepted) => {
                    result.complete(Check::Registration);
                    // Every accepted registration -- first attempt or a
                    // replacement -- starts a fresh epoch: the session
                    // itself wipes outstanding tasks on replacement (a
                    // stale issued_task_id can never be acknowledged
                    // again), and a replacement contract may declare
                    // entirely different modes, so evidence from a
                    // discarded contract shouldn't excuse the new one from
                    // demonstrating the same behaviour again.
                    status_received = false;
                    issued_task_id = None;
                    target_mode = None;
                    result.require(Check::StatusReport);
                    result.require(Check::TaskAck);
                    if let SessionState::Registered(contract) = connection.state() {
                        // A mode with a non-Default *type* isn't
                        // necessarily distinct from the one actually
                        // resolved active: an all-Permanent registration
                        // (legacy-style, no MODE_TYPE_DEFAULT at all) can
                        // resolve its Permanent-named-"default" mode as
                        // active, and that same mode would still be the
                        // first "non-Default-type" entry in the list --
                        // targeting it wouldn't exercise a transition at
                        // all, just a same-mode round trip. Compare by
                        // name against the mode that's actually active
                        // instead.
                        target_mode = contract
                            .registration
                            .mode_definition
                            .iter()
                            .find(|mode| mode.mode_name != contract.active_mode.mode_name)
                            .and_then(|mode| mode.mode_name.clone());
                    }
                    if target_mode.is_none() {
                        result.skip(
                            Check::TaskAck,
                            "Registration declares no mode distinct from the currently active \
                             one to probe a transition with.",
                        );
                    }
                }
                Some(DmmEvent::RegistrationRejected) => {
                    // Distinct from GoodbyeReceived on purpose: the session
                    // is AwaitingRegistration for a completely different
                    // reason (the ASM's own findings explain why), and --
                    // unlike GoodBye -- this doesn't end the run. A
                    // corrected registration on the same connection is
                    // still perfectly usable. Already-completed checks are
                    // left alone: they're valid evidence from whatever
                    // contract was in effect when they completed, not
                    // undone by a later, unrelated rejection. Local
                    // per-epoch tracking is cleared since there's no live
                    // contract for it to refer to any more.
                    note(
                        &mut result.notes,
                        "ASM's registration was rejected (see findings); waiting for a valid \
                         one on the same connection.",
                    );
                    status_received = false;
                    issued_task_id = None;
                    target_mode = None;
                }
                Some(DmmEvent::StatusReportValidated) => {
                    status_received = true;
                    result.complete(Check::StatusReport);
                }
                Some(DmmEvent::TaskAcknowledged { task_id })
                    if issued_task_id.as_deref() == Some(task_id.as_str()) =>
                {
                    result.complete(Check::TaskAck);
                }
                Some(DmmEvent::GoodbyeReceived) => {
                    note(
                        &mut result.notes,
                        "ASM sent a GoodBye StatusReport; ending the run.",
                    );
                    break;
                }
                _ => {}
            },
            PollWait::Disconnected => {
                note(&mut result.notes, "ASM disconnected.");
                break;
            }
            PollWait::DeadlineReached => {
                note(
                    &mut result.notes,
                    "Reached the run's max runtime while waiting for further messages from the ASM.",
                );
                break;
            }
        }
    }

    Ok(())
}

/// Drives an ASM-role run against a DMM/middleware under test: registers,
/// sends a `StatusReport` and a `DetectionReport`, gives the peer a short
/// window to react (e.g. issue a `Task`), sends an `Alert` and waits for
/// its `AlertAck`, then ends the session gracefully with a `GoodBye`
/// `StatusReport`. Unlike the DMM role, the ASM role legitimately manages
/// its own session lifecycle, so concluding the run this way is correct
/// protocol behaviour, not the harness cutting the peer off.
/// Returns partial progress and operational errors even if I/O fails. All waits
/// and writes share `deadline`. Drop the connection when this run returns;
/// graceful teardown is attempted only within the remaining runtime.
pub async fn run_asm_scenario<R, W>(
    connection: &mut AsmConnection<R, W>,
    deadline: Instant,
) -> ScenarioResult
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut result = ScenarioResult::for_role(Role::Asm);
    let mut stage = "starting_scenario";
    // Progress lives outside the cancellable future so every exit retains it
    // -- including a Ctrl-C, not just reaching the deadline: dropping the
    // `asm_steps` future on either branch of this `select!` cannot discard
    // `result`, since it's owned by this function's own stack frame, not by
    // the future being raced and dropped.
    let outcome = tokio::select! {
        ctrl_c = tokio::signal::ctrl_c() => {
            note_interrupted(&mut result.notes, ctrl_c);
            None
        }
        outcome = timeout_at(
            deadline,
            asm_steps(connection, deadline, &mut result, &mut stage),
        ) => Some(outcome),
    };
    match outcome {
        None => {}
        Some(Ok(Err(error))) => result.record_error(stage, error),
        Some(Err(_)) if connection.has_pending_write() => result.record_error(
            stage,
            io::Error::new(
                io::ErrorKind::TimedOut,
                "run deadline expired with an unfinished write; connection must be closed",
            ),
        ),
        Some(Err(_)) => note(&mut result.notes, "Reached the run's max runtime."),
        Some(Ok(Ok(()))) => {}
    }
    // A heartbeat/optional wait may observe the same deadline before the outer timer.
    if result.operational_error.is_none() && connection.has_pending_write() {
        result.record_error(
            stage,
            io::Error::new(
                io::ErrorKind::TimedOut,
                "run ended with an unfinished reply; connection must be closed",
            ),
        );
    }
    result
}

async fn asm_steps<R, W>(
    connection: &mut AsmConnection<R, W>,
    deadline: Instant,
    result: &mut ScenarioResult,
    stage: &mut &'static str,
) -> io::Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    // Disabled until the initial ordinary status is sent after registration.
    let mut next_status = None;
    let mut shown_findings = 0;
    *stage = "send_registration";
    connection.register(fixtures::valid_registration()).await?;
    stream_new_findings(connection.findings(), &mut shown_findings);

    loop {
        *stage = "receive_or_reply";
        let poll_result = asm_poll_with_heartbeat(
            connection,
            deadline,
            "for a RegistrationAck",
            &mut next_status,
            stage,
        )
        .await?;
        stream_new_findings(connection.findings(), &mut shown_findings);
        match poll_result {
            PollWait::Processed => match connection.take_event() {
                Some(AsmEvent::RegistrationAccepted) => {
                    result.complete(Check::Registration);
                    break;
                }
                Some(AsmEvent::RegistrationRejected | AsmEvent::RegistrationFailed) => {
                    return Ok(());
                }
                _ => {}
            },
            PollWait::Disconnected => {
                note(
                    &mut result.notes,
                    "DMM under test disconnected before acknowledging our Registration.",
                );
                return Ok(());
            }
            PollWait::DeadlineReached => {
                note(
                    &mut result.notes,
                    "Timed out waiting for a RegistrationAck from the DMM under test.",
                );
                return Ok(());
            }
        }
    }
    note(&mut result.notes, "Registration accepted.");

    *stage = "send_status_report";
    let status = asm_status(connection.state(), System::Ok);
    connection.issue_status_report(status).await?;
    stream_new_findings(connection.findings(), &mut shown_findings);
    next_status = Some(Instant::now() + asm_status_period());

    result.complete(Check::StatusReport);

    *stage = "send_detection_report";
    connection
        .issue_detection_report(scripted_detection_report())
        .await?;
    stream_new_findings(connection.findings(), &mut shown_findings);

    result.complete(Check::DetectionReport);

    // Give the DMM a short window to react (e.g. issue a Task) before
    // moving on -- not every DMM proactively does this, so this is a
    // best-effort check, not something a timeout here should be a finding,
    // and short enough that it doesn't need a heartbeat of its own.
    let short_deadline = (Instant::now() + Duration::from_secs(2)).min(deadline);
    let poll_result = asm_poll_with_heartbeat(
        connection,
        short_deadline,
        "for an optional Task",
        &mut next_status,
        stage,
    )
    .await?;
    stream_new_findings(connection.findings(), &mut shown_findings);
    match poll_result {
        PollWait::Processed => {
            connection.take_event();
            note(
                &mut result.notes,
                "Processed an inbound message from the DMM before continuing.",
            );
        }
        PollWait::Disconnected => {
            note(&mut result.notes, "DMM under test disconnected.");
            return Ok(());
        }
        PollWait::DeadlineReached => {}
    }

    if Instant::now() >= deadline {
        note(
            &mut result.notes,
            "Reached the run's max runtime before sending the Alert.",
        );
        return Ok(());
    }

    let alert_id = Ulid::new().to_string();
    *stage = "send_alert";
    connection
        .issue_alert(scripted_alert(alert_id.clone()))
        .await?;
    stream_new_findings(connection.findings(), &mut shown_findings);

    loop {
        *stage = "receive_or_reply";
        let poll_result = asm_poll_with_heartbeat(
            connection,
            deadline,
            "for an AlertAck",
            &mut next_status,
            stage,
        )
        .await?;
        stream_new_findings(connection.findings(), &mut shown_findings);
        match poll_result {
            PollWait::Processed => {
                if let Some(AsmEvent::AlertAcknowledged {
                    alert_id: acknowledged_id,
                }) = connection.take_event()
                    && acknowledged_id == alert_id
                {
                    result.complete(Check::AlertAck);
                    note(&mut result.notes, "Alert acknowledged.");
                    break;
                }
            }
            PollWait::Disconnected => {
                note(
                    &mut result.notes,
                    "DMM under test disconnected before acknowledging our Alert.",
                );
                return Ok(());
            }
            PollWait::DeadlineReached => {
                note(
                    &mut result.notes,
                    "Timed out waiting for an AlertAck from the DMM under test.",
                );
                break;
            }
        }
    }

    // Never start graceful teardown after the run deadline has already elapsed.
    if Instant::now() >= deadline {
        return Ok(());
    }
    *stage = "send_goodbye";
    let goodbye = asm_status(connection.state(), System::Goodbye);
    connection.issue_status_report(goodbye).await?;
    stream_new_findings(connection.findings(), &mut shown_findings);
    result.complete(Check::Goodbye);
    note(
        &mut result.notes,
        "Sent a GoodBye StatusReport to end the session gracefully.",
    );

    Ok(())
}
