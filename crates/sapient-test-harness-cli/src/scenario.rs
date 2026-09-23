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
//! Progress is streamed to stderr as it happens (`note`, and a heartbeat
//! during any wait long enough to otherwise look hung) rather than only
//! shown at the end -- stdout stays reserved for the final report (text or
//! `--format json`), matching `run.rs`'s own connect/listen messages.

use std::io;
use std::time::Duration;

use sapient_conformance_core::bsi_flex_335_v2_0::{
    Alert, DetectionReport, Location, LocationCoordinateSystem, LocationDatum, StatusReport, Task,
    alert::{AlertStatus, AlertType},
    detection_report::{DetectionReportClassification, LocationOneof as DetectionLocationOneof},
    registration::ModeType,
    status_report::System,
    task::{Command, command::Command as TaskCommandKind},
};
use sapient_session::{
    AsmSessionState, SessionState, asm::AsmConnection, dmm::DmmConnection, fixtures,
};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::time::{Instant, timeout};
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

async fn asm_poll_with_heartbeat<R, W>(
    connection: &mut AsmConnection<R, W>,
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

/// Drives a DMM-role run against an ASM under test until it ends the
/// session itself (disconnect or `GoodBye`) or `deadline` passes. Once
/// registered, proactively issues one `mode_change` `Task` (targeting
/// whatever non-default mode the ASM's own `Registration` declared, if
/// any) to exercise `Task`/`TaskAck` and mode-change handling; everything
/// else is reactive, auto-replying via `DmmConnection`.
pub async fn run_dmm_scenario<R, W>(
    connection: &mut DmmConnection<R, W>,
    deadline: Instant,
) -> io::Result<Vec<String>>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut notes = Vec::new();
    let mut mode_change_issued = false;
    let mut ever_registered = false;
    // Becomes true once the loop has already seen `Registered` on a
    // *previous* iteration -- so the mode_change probe below only fires
    // after the ASM has had at least one full poll_once round-trip to send
    // something (its first StatusReport, in the bundled default ASM-role
    // scenario) following its RegistrationAck. Without this delay, issuing
    // the probe on the very same iteration the harness first observes
    // `Registered` races the ASM's own first StatusReport: the harness's
    // locally tracked active_mode flips to the target mode the instant it
    // sends the Task, while that in-flight StatusReport still (correctly)
    // declares the prior mode, producing a false mode_mismatch finding
    // against nobody's actual protocol violation.
    let mut registered_on_prior_iteration = false;

    loop {
        if Instant::now() >= deadline {
            note(
                &mut notes,
                "Reached the run's max runtime before the ASM under test disconnected or sent \
                 a GoodBye StatusReport. A DMM-role run never disconnects first, so this run \
                 gave up waiting rather than hanging up on the peer.",
            );
            break;
        }

        if let SessionState::Registered(contract) = connection.state() {
            ever_registered = true;
            if !mode_change_issued && registered_on_prior_iteration {
                mode_change_issued = true;
                let target_mode = contract
                    .registration
                    .mode_definition
                    .iter()
                    .find(|mode| mode.mode_type != Some(ModeType::Default as i32))
                    .and_then(|mode| mode.mode_name.clone());

                match target_mode {
                    Some(target_mode) => {
                        note(
                            &mut notes,
                            format!(
                                "Issuing a mode_change Task targeting {target_mode:?} to \
                                 exercise Task/TaskAck and mode-change handling."
                            ),
                        );
                        let task = Task {
                            task_id: Some(Ulid::new().to_string()),
                            task_name: None,
                            task_description: None,
                            task_start_time: None,
                            task_end_time: None,
                            control: Some(
                                sapient_conformance_core::bsi_flex_335_v2_0::task::Control::Start
                                    as i32,
                            ),
                            region: vec![],
                            command: Some(Command {
                                command_parameter: None,
                                command: Some(TaskCommandKind::ModeChange(target_mode)),
                            }),
                        };
                        connection.issue_task(&task).await?;
                    }
                    None => {
                        note(
                            &mut notes,
                            "The ASM's Registration declared no mode besides \
                             MODE_TYPE_DEFAULT; skipped the mode_change Task/TaskAck check.",
                        );
                    }
                }
            }
            registered_on_prior_iteration = true;
        } else if ever_registered {
            note(
                &mut notes,
                "ASM sent a GoodBye StatusReport; ending the run.",
            );
            break;
        } else {
            registered_on_prior_iteration = false;
        }

        match dmm_poll_with_heartbeat(connection, deadline, "for the ASM").await? {
            PollWait::Processed => continue,
            PollWait::Disconnected => {
                note(&mut notes, "ASM disconnected.");
                break;
            }
            PollWait::DeadlineReached => {
                note(
                    &mut notes,
                    "Reached the run's max runtime while waiting for further messages from the \
                     ASM.",
                );
                break;
            }
        }
    }

    Ok(notes)
}

/// Drives an ASM-role run against a DMM/middleware under test: registers,
/// sends a `StatusReport` and a `DetectionReport`, gives the peer a short
/// window to react (e.g. issue a `Task`), sends an `Alert` and waits for
/// its `AlertAck`, then ends the session gracefully with a `GoodBye`
/// `StatusReport`. Unlike the DMM role, the ASM role legitimately manages
/// its own session lifecycle, so concluding the run this way is correct
/// protocol behaviour, not the harness cutting the peer off.
pub async fn run_asm_scenario<R, W>(
    connection: &mut AsmConnection<R, W>,
    deadline: Instant,
) -> io::Result<Vec<String>>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut notes = Vec::new();

    connection.register(fixtures::valid_registration()).await?;

    loop {
        match asm_poll_with_heartbeat(connection, deadline, "for a RegistrationAck").await? {
            PollWait::Processed => {
                if matches!(connection.state(), AsmSessionState::Registered(_)) {
                    break;
                }
            }
            PollWait::Disconnected => {
                note(
                    &mut notes,
                    "DMM under test disconnected before acknowledging our Registration.",
                );
                return Ok(notes);
            }
            PollWait::DeadlineReached => {
                note(
                    &mut notes,
                    "Timed out waiting for a RegistrationAck from the DMM under test.",
                );
                return Ok(notes);
            }
        }
    }
    note(&mut notes, "Registration accepted.");

    let active_mode_name = match connection.state() {
        AsmSessionState::Registered(contract) => contract.active_mode.mode_name.clone(),
        _ => None,
    };

    connection
        .issue_status_report(StatusReport {
            report_id: Some(Ulid::new().to_string()),
            system: Some(System::Ok as i32),
            info: Some(System::Ok as i32),
            active_task_id: None,
            mode: active_mode_name,
            power: None,
            node_location: None,
            field_of_view: None,
            obscuration: vec![],
            status: vec![],
            coverage: vec![],
        })
        .await?;

    connection
        .issue_detection_report(DetectionReport {
            report_id: Some(Ulid::new().to_string()),
            object_id: Some(Ulid::new().to_string()),
            task_id: None,
            state: None,
            location_oneof: Some(DetectionLocationOneof::Location(Location {
                x: Some(1.0),
                y: Some(2.0),
                z: None,
                x_error: None,
                y_error: None,
                z_error: None,
                coordinate_system: Some(LocationCoordinateSystem::LatLngDegM as i32),
                datum: Some(LocationDatum::Wgs84E as i32),
                utm_zone: None,
            })),
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
        })
        .await?;

    // Give the DMM a short window to react (e.g. issue a Task) before
    // moving on -- not every DMM proactively does this, so this is a
    // best-effort check, not something a timeout here should be a finding,
    // and short enough that it doesn't need a heartbeat of its own.
    let short_wait = Duration::from_secs(2).min(deadline.saturating_duration_since(Instant::now()));
    if !short_wait.is_zero() {
        match timeout(short_wait, connection.poll_once()).await {
            Ok(Ok(true)) => note(
                &mut notes,
                "Processed an inbound message from the DMM (e.g. a Task) before continuing.",
            ),
            Ok(Ok(false)) => {
                note(&mut notes, "DMM under test disconnected.");
                return Ok(notes);
            }
            Ok(Err(err)) => return Err(err),
            Err(_elapsed) => {}
        }
    }

    let alert_id = Ulid::new().to_string();
    connection
        .issue_alert(Alert {
            alert_id: Some(alert_id.clone()),
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
        })
        .await?;

    loop {
        let still_outstanding = matches!(
            connection.state(),
            AsmSessionState::Registered(contract) if contract.outstanding_alert_ids.contains(&alert_id)
        );
        if !still_outstanding {
            note(&mut notes, "Alert acknowledged.");
            break;
        }

        match asm_poll_with_heartbeat(connection, deadline, "for an AlertAck").await? {
            PollWait::Processed => continue,
            PollWait::Disconnected => {
                note(
                    &mut notes,
                    "DMM under test disconnected before acknowledging our Alert.",
                );
                return Ok(notes);
            }
            PollWait::DeadlineReached => {
                note(
                    &mut notes,
                    "Timed out waiting for an AlertAck from the DMM under test.",
                );
                break;
            }
        }
    }

    connection
        .issue_status_report(StatusReport {
            report_id: Some(Ulid::new().to_string()),
            system: Some(System::Goodbye as i32),
            info: None,
            active_task_id: None,
            mode: None,
            power: None,
            node_location: None,
            field_of_view: None,
            obscuration: vec![],
            status: vec![],
            coverage: vec![],
        })
        .await?;
    note(
        &mut notes,
        "Sent a GoodBye StatusReport to end the session gracefully.",
    );

    Ok(notes)
}
