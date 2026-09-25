//! KI-001: exercise real scenarios with controlled peers, including completion
//! and report/exit-code behavior. Paused time keeps deadline cases deterministic.
use std::{process::ExitCode, time::Duration};

use prost::Message;
use prost_types::Timestamp;
use sapient_conformance_core::bsi_flex_335_v2_0::{
    AlertAck, RegistrationAck, SapientMessage, StatusReport, TaskAck, sapient_message::Content,
};
use sapient_session::{asm::AsmConnection, dmm::DmmConnection, fixtures};
use sapient_test_harness_cli::{
    cli::Role,
    completion::{Check, CheckStatus, ScenarioResult},
    report::{RunOutcome, RunReport},
    scenario::{run_asm_scenario, run_dmm_scenario},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt, DuplexStream, duplex, split},
    time::{Instant, sleep},
};

const NODE: &str = "550e8400-e29b-41d4-a716-446655440000";
const ID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

async fn send(peer: &mut DuplexStream, content: Content) {
    let bytes = SapientMessage {
        timestamp: Some(Timestamp {
            seconds: 1,
            nanos: 0,
        }),
        node_id: Some(NODE.into()),
        content: Some(content),
        ..Default::default()
    }
    .encode_to_vec();
    peer.write_u32_le(bytes.len() as u32).await.unwrap();
    peer.write_all(&bytes).await.unwrap();
}

async fn receive(peer: &mut DuplexStream) -> Content {
    let len = peer.read_u32_le().await.unwrap();
    let mut bytes = vec![0; len as usize];
    peer.read_exact(&mut bytes).await.unwrap();
    SapientMessage::decode(bytes.as_slice())
        .unwrap()
        .content
        .unwrap()
}

fn report(
    role: Role,
    result: ScenarioResult,
    findings: Vec<sapient_conformance_core::finding::Finding>,
) -> RunReport {
    RunReport::new(role, "v2.0".into(), "test".into(), findings, result)
}

fn check(report: &RunReport, expected: Check, status: CheckStatus) {
    assert_eq!(
        report
            .checks
            .iter()
            .find(|c| c.check == expected)
            .unwrap()
            .status,
        status
    );
}

fn verdict(report: &RunReport, outcome: RunOutcome) {
    assert_eq!(report.outcome, outcome);
    assert_eq!(report.passed, outcome == RunOutcome::Passed);
    assert_eq!(
        report.exit_code(),
        if report.passed {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        }
    );
    let json = serde_json::to_value(report).unwrap();
    assert_eq!(json["passed"], report.passed);
    assert_eq!(
        json["outcome"],
        match outcome {
            RunOutcome::Passed => "passed",
            RunOutcome::Failed => "failed",
            RunOutcome::Incomplete => "incomplete",
        }
    );
    assert!(json["checks"].is_array());
}

#[tokio::test(start_paused = true)]
async fn silent_peers_cannot_pass() {
    let (stream, _peer) = duplex(65536);
    let (r, w) = split(stream);
    let mut asm = AsmConnection::new(NODE, r, w);
    let result = run_asm_scenario(&mut asm, Instant::now() + Duration::from_secs(1)).await;
    let result = report(Role::Asm, result, asm.take_findings());
    verdict(&result, RunOutcome::Incomplete);
    check(&result, Check::Registration, CheckStatus::Incomplete);

    let (stream, _peer) = duplex(65536);
    let (r, w) = split(stream);
    let mut dmm = DmmConnection::new(NODE, r, w);
    let result = run_dmm_scenario(&mut dmm, Instant::now() + Duration::from_secs(1)).await;
    verdict(
        &report(Role::Dmm, result, dmm.take_findings()),
        RunOutcome::Incomplete,
    );
}

#[tokio::test(start_paused = true)]
async fn immediate_disconnects_cannot_pass() {
    let (stream, peer) = duplex(65536);
    drop(peer);
    let (r, w) = split(stream);
    let mut dmm = DmmConnection::new(NODE, r, w);
    let result = run_dmm_scenario(&mut dmm, Instant::now() + Duration::from_secs(10)).await;
    verdict(
        &report(Role::Dmm, result, dmm.take_findings()),
        RunOutcome::Incomplete,
    );

    // Let the ASM finish its registration write, then disconnect without an ack.
    let (stream, mut peer) = duplex(65536);
    let (r, w) = split(stream);
    let mut asm = AsmConnection::new(NODE, r, w);
    let (result, ()) = tokio::join!(
        run_asm_scenario(&mut asm, Instant::now() + Duration::from_secs(10)),
        async move {
            receive(&mut peer).await;
        },
    );
    verdict(
        &report(Role::Asm, result, asm.take_findings()),
        RunOutcome::Incomplete,
    );
}

// 0: complete, 1: no AlertAck, 2: disconnect before AlertAck,
// 3: disconnect after registration, 4: invalid correlated ack, 5: reject registration,
// 6: uncorrelated alert ack, 7: invalid registration ack.
async fn asm_case(case: u8) -> RunReport {
    let (stream, mut peer) = duplex(65536);
    let (r, w) = split(stream);
    let mut asm = AsmConnection::new(NODE, r, w);
    let target = async move {
        assert!(matches!(receive(&mut peer).await, Content::Registration(_)));
        send(
            &mut peer,
            Content::RegistrationAck(RegistrationAck {
                acceptance: if case == 7 { None } else { Some(case != 5) },
                ack_response_reason: vec![],
            }),
        )
        .await;
        if case == 5 || case == 7 {
            return;
        }
        assert!(matches!(receive(&mut peer).await, Content::StatusReport(_)));
        assert!(matches!(
            receive(&mut peer).await,
            Content::DetectionReport(_)
        ));
        if case == 3 {
            return;
        }
        let Content::Alert(alert) = receive(&mut peer).await else {
            panic!("expected alert")
        };
        if case == 2 {
            return;
        }
        if case != 1 {
            send(
                &mut peer,
                Content::AlertAck(AlertAck {
                    alert_id: if case == 6 {
                        Some(ID.into())
                    } else {
                        alert.alert_id
                    },
                    alert_ack_status: Some(if case == 4 { 999 } else { 1 }),
                    reason: vec![],
                }),
            )
            .await;
        }
        // Invalid ack causes an Error reply, but still does not complete the check.
        if case == 4 {
            assert!(matches!(receive(&mut peer).await, Content::Error(_)));
        }
        if matches!(case, 1 | 4 | 6) {
            // Keep the peer open through the deadline; teardown cannot write after it.
            sleep(Duration::from_secs(11)).await;
        } else {
            assert!(matches!(receive(&mut peer).await, Content::StatusReport(_)));
        }
    };
    let (result, ()) = tokio::join!(
        run_asm_scenario(&mut asm, Instant::now() + Duration::from_secs(10)),
        target
    );
    report(Role::Asm, result, asm.take_findings())
}

#[tokio::test(start_paused = true)]
async fn missing_alert_ack_cannot_pass_or_send_goodbye_after_deadline() {
    let result = asm_case(1).await;
    verdict(&result, RunOutcome::Incomplete);
    check(&result, Check::AlertAck, CheckStatus::Incomplete);
    check(&result, Check::Goodbye, CheckStatus::Incomplete);
}

#[tokio::test(start_paused = true)]
async fn disconnects_after_partial_asm_progress_cannot_pass() {
    for case in [2, 3] {
        let result = asm_case(case).await;
        verdict(&result, RunOutcome::Incomplete);
        check(&result, Check::Registration, CheckStatus::Completed);
        check(&result, Check::AlertAck, CheckStatus::Incomplete);
    }
}

#[tokio::test(start_paused = true)]
async fn invalid_alert_ack_is_failed_and_does_not_complete_check() {
    let result = asm_case(4).await;
    verdict(&result, RunOutcome::Failed);
    check(&result, Check::AlertAck, CheckStatus::Incomplete);
}

#[tokio::test(start_paused = true)]
async fn registration_rejection_is_failed_not_passed() {
    let result = asm_case(5).await;
    verdict(&result, RunOutcome::Failed);
    check(&result, Check::Registration, CheckStatus::Incomplete);
}

#[tokio::test(start_paused = true)]
async fn complete_asm_run_passes_without_an_unsolicited_task() {
    let result = asm_case(0).await;
    verdict(&result, RunOutcome::Passed);
    assert!(
        result
            .checks
            .iter()
            .all(|c| c.status == CheckStatus::Completed)
    );
}

// 0: complete with GoodBye, 1: missing TaskAck + GoodBye, 2: one mode,
// 3: complete observation deadline, 4: no status, 5: re-registration clears task,
// 6: uncorrelated TaskAck, 7: missing TaskAck + observation deadline.
async fn dmm_case(case: u8) -> RunReport {
    let (stream, mut peer) = duplex(65536);
    let (r, w) = split(stream);
    let mut dmm = DmmConnection::new(NODE, r, w);
    let target = async move {
        let mut registration = fixtures::valid_registration();
        if case == 2 {
            registration.mode_definition.truncate(1);
        }
        send(&mut peer, Content::Registration(registration)).await;
        assert!(matches!(
            receive(&mut peer).await,
            Content::RegistrationAck(_)
        ));
        if case == 4 {
            sleep(Duration::from_secs(11)).await;
            return;
        }
        send(
            &mut peer,
            Content::StatusReport(StatusReport {
                report_id: Some(ID.into()),
                system: Some(1),
                info: Some(1),
                mode: Some("Default".into()),
                ..Default::default()
            }),
        )
        .await;
        if case != 2 {
            let Content::Task(task) = receive(&mut peer).await else {
                panic!("expected task")
            };
            if ![1, 5, 7].contains(&case) {
                send(
                    &mut peer,
                    Content::TaskAck(TaskAck {
                        task_id: if case == 6 {
                            Some(ID.into())
                        } else {
                            task.task_id
                        },
                        task_status: Some(1),
                        ..Default::default()
                    }),
                )
                .await;
            }
        }
        if case == 5 {
            send(
                &mut peer,
                Content::Registration(fixtures::valid_registration()),
            )
            .await;
            receive(&mut peer).await;
        }
        if case == 3 || case == 7 {
            sleep(Duration::from_secs(11)).await;
        } else {
            send(
                &mut peer,
                Content::StatusReport(StatusReport {
                    report_id: Some(ID.into()),
                    system: Some(5),
                    info: Some(1),
                    mode: Some("Default".into()),
                    ..Default::default()
                }),
            )
            .await;
        }
    };
    let (result, ()) = tokio::join!(
        run_dmm_scenario(&mut dmm, Instant::now() + Duration::from_secs(10)),
        target
    );
    report(Role::Dmm, result, dmm.take_findings())
}

#[tokio::test(start_paused = true)]
async fn missing_task_ack_survives_goodbye_reregistration_and_deadline() {
    for case in [1, 5, 7] {
        let result = dmm_case(case).await;
        verdict(&result, RunOutcome::Incomplete);
        check(&result, Check::TaskAck, CheckStatus::Incomplete);
    }
}

#[tokio::test(start_paused = true)]
async fn uncorrelated_task_ack_does_not_complete_probe() {
    let result = dmm_case(6).await;
    verdict(&result, RunOutcome::Failed);
    check(&result, Check::TaskAck, CheckStatus::Incomplete);
}

#[tokio::test(start_paused = true)]
async fn registration_alone_does_not_complete_dmm_scenario() {
    let result = dmm_case(4).await;
    verdict(&result, RunOutcome::Incomplete);
    check(&result, Check::StatusReport, CheckStatus::Incomplete);
    check(&result, Check::TaskAck, CheckStatus::Incomplete);
}

#[tokio::test(start_paused = true)]
async fn complete_dmm_run_passes_at_goodbye_or_observation_deadline() {
    for case in [0, 3] {
        let result = dmm_case(case).await;
        verdict(&result, RunOutcome::Passed);
    }
}

#[tokio::test(start_paused = true)]
async fn single_mode_skips_task_probe_explicitly() {
    let result = dmm_case(2).await;
    verdict(&result, RunOutcome::Passed);
    check(&result, Check::TaskAck, CheckStatus::Skipped);
    assert!(
        result
            .checks
            .iter()
            .find(|c| c.check == Check::TaskAck)
            .unwrap()
            .reason
            .is_some()
    );
}

#[tokio::test(start_paused = true)]
async fn reregistration_before_probe_uses_new_contract_and_requires_ack() {
    let (stream, mut peer) = duplex(65536);
    let (r, w) = split(stream);
    let mut dmm = DmmConnection::new(NODE, r, w);
    let target = async move {
        let mut registration = fixtures::valid_registration();
        registration.mode_definition.truncate(1);
        send(&mut peer, Content::Registration(registration)).await;
        receive(&mut peer).await;
        let mut replacement = fixtures::valid_registration();
        replacement.mode_definition[1].mode_name = Some("Replacement".into());
        send(&mut peer, Content::Registration(replacement)).await;
        receive(&mut peer).await;
        send(
            &mut peer,
            Content::StatusReport(StatusReport {
                report_id: Some(ID.into()),
                system: Some(1),
                info: Some(1),
                mode: Some("Default".into()),
                ..Default::default()
            }),
        )
        .await;
        let Content::Task(task) = receive(&mut peer).await else {
            panic!("expected task")
        };
        assert_eq!(
            task.command.unwrap().command,
            Some(
                sapient_conformance_core::bsi_flex_335_v2_0::task::command::Command::ModeChange(
                    "Replacement".into()
                )
            )
        );
        // Close without acknowledging: the earlier skip must not survive.
    };
    let (result, ()) = tokio::join!(
        run_dmm_scenario(&mut dmm, Instant::now() + Duration::from_secs(10)),
        target
    );
    let result = report(Role::Dmm, result, dmm.take_findings());
    verdict(&result, RunOutcome::Incomplete);
    check(&result, Check::TaskAck, CheckStatus::Incomplete);
}

#[tokio::test(start_paused = true)]
async fn deadline_after_registration_before_alert_is_incomplete() {
    let (stream, mut peer) = duplex(65536);
    let (r, w) = split(stream);
    let mut asm = AsmConnection::new(NODE, r, w);
    let target = async move {
        receive(&mut peer).await;
        send(
            &mut peer,
            Content::RegistrationAck(RegistrationAck {
                acceptance: Some(true),
                ack_response_reason: vec![],
            }),
        )
        .await;
        receive(&mut peer).await;
        receive(&mut peer).await;
        sleep(Duration::from_secs(2)).await;
    };
    let (result, ()) = tokio::join!(
        run_asm_scenario(&mut asm, Instant::now() + Duration::from_secs(1)),
        target
    );
    let result = report(Role::Asm, result, asm.take_findings());
    verdict(&result, RunOutcome::Incomplete);
    check(&result, Check::Registration, CheckStatus::Completed);
    check(&result, Check::AlertAck, CheckStatus::Incomplete);
    check(&result, Check::Goodbye, CheckStatus::Incomplete);
}

#[tokio::test(start_paused = true)]
async fn uncorrelated_alert_ack_does_not_complete_check() {
    let result = asm_case(6).await;
    verdict(&result, RunOutcome::Failed);
    check(&result, Check::AlertAck, CheckStatus::Incomplete);
}

#[tokio::test(start_paused = true)]
async fn invalid_registration_ack_does_not_complete_check() {
    let result = asm_case(7).await;
    verdict(&result, RunOutcome::Failed);
    check(&result, Check::Registration, CheckStatus::Incomplete);
}

#[tokio::test(start_paused = true)]
async fn every_asm_send_is_bounded_and_preserves_prior_checks() {
    for (completed_sends, stage) in [
        (0, "send_registration"),
        (1, "send_status_report"),
        (2, "send_detection_report"),
        (3, "send_alert"),
        (4, "send_goodbye"),
    ] {
        let (r, mut inbound) = duplex(65536);
        let (w, mut outbound) = duplex(1);
        let mut asm = AsmConnection::new(NODE, r, w);
        let start = Instant::now();
        let target = async {
            for index in 0..completed_sends {
                let content = receive(&mut outbound).await;
                if index == 0 {
                    send(
                        &mut inbound,
                        Content::RegistrationAck(RegistrationAck {
                            acceptance: Some(true),
                            ack_response_reason: vec![],
                        }),
                    )
                    .await;
                }
                if let Content::Alert(alert) = content {
                    send(
                        &mut inbound,
                        Content::AlertAck(AlertAck {
                            alert_id: alert.alert_id,
                            alert_ack_status: Some(1),
                            reason: vec![],
                        }),
                    )
                    .await;
                }
            }
            sleep(Duration::from_secs(6)).await;
        };
        let (result, ()) = tokio::join!(
            async {
                let result = run_asm_scenario(&mut asm, start + Duration::from_secs(5)).await;
                assert_eq!(Instant::now(), start + Duration::from_secs(5));
                result
            },
            target
        );
        let report = report(Role::Asm, result, asm.take_findings());
        assert_eq!(report.exit_code(), ExitCode::from(2));
        assert_eq!(report.outcome, RunOutcome::Incomplete);
        let error = report.operational_error.as_ref().unwrap();
        assert_eq!(error.stage, stage);
        assert_eq!(error.kind, "TimedOut");
        check(&report, Check::Goodbye, CheckStatus::Incomplete);
        if completed_sends >= 2 {
            check(&report, Check::StatusReport, CheckStatus::Completed);
        }
        if completed_sends >= 3 {
            check(&report, Check::DetectionReport, CheckStatus::Completed);
        }
        if completed_sends >= 4 {
            check(&report, Check::AlertAck, CheckStatus::Completed);
        }
    }
}

#[tokio::test(start_paused = true)]
async fn blocked_automatic_reply_keeps_findings_and_cannot_pass() {
    let (r, mut inbound) = duplex(65536);
    let (w, _outbound) = duplex(1);
    let mut dmm = DmmConnection::new(NODE, r, w);
    let raw = SapientMessage {
        content: Some(Content::Registration(fixtures::valid_registration())),
        ..Default::default()
    }
    .encode_to_vec();
    inbound.write_u32_le(raw.len() as u32).await.unwrap();
    inbound.write_all(&raw).await.unwrap();
    let start = Instant::now();
    let result = run_dmm_scenario(&mut dmm, start + Duration::from_secs(1)).await;
    assert_eq!(Instant::now(), start + Duration::from_secs(1));
    let report = report(Role::Dmm, result, dmm.take_findings());
    assert_eq!(report.exit_code(), ExitCode::from(2));
    assert_eq!(report.outcome, RunOutcome::Failed);
    assert_eq!(report.findings.len(), 2);
    assert_eq!(report.operational_error.unwrap().stage, "receive_or_reply");
}

#[tokio::test(start_paused = true)]
async fn blocked_dmm_probe_keeps_registration_and_status_progress() {
    let (r, mut inbound) = duplex(65536);
    let (w, mut outbound) = duplex(1);
    let mut dmm = DmmConnection::new(NODE, r, w);
    let start = Instant::now();
    let target = async {
        send(
            &mut inbound,
            Content::Registration(fixtures::valid_registration()),
        )
        .await;
        receive(&mut outbound).await;
        send(
            &mut inbound,
            Content::StatusReport(StatusReport {
                report_id: Some(ID.into()),
                system: Some(1),
                info: Some(1),
                mode: Some("Default".into()),
                ..Default::default()
            }),
        )
        .await;
        sleep(Duration::from_secs(2)).await;
    };
    let (result, ()) = tokio::join!(
        run_dmm_scenario(&mut dmm, start + Duration::from_secs(1)),
        target
    );
    let report = report(Role::Dmm, result, dmm.take_findings());
    check(&report, Check::Registration, CheckStatus::Completed);
    check(&report, Check::StatusReport, CheckStatus::Completed);
    check(&report, Check::TaskAck, CheckStatus::Incomplete);
    assert_eq!(
        report.operational_error.as_ref().unwrap().stage,
        "send_task"
    );
    assert_eq!(report.exit_code(), ExitCode::from(2));
}

#[tokio::test]
async fn connection_reset_keeps_preceding_findings_and_progress() {
    // Deterministic reset after a complete frame, independent of OS TCP buffering.
    struct ResetAfter {
        bytes: Vec<u8>,
        offset: usize,
    }
    impl tokio::io::AsyncRead for ResetAfter {
        fn poll_read(
            mut self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
            buf: &mut tokio::io::ReadBuf<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            if self.offset == self.bytes.len() {
                return std::task::Poll::Ready(Err(std::io::ErrorKind::ConnectionReset.into()));
            }
            let count = buf.remaining().min(self.bytes.len() - self.offset);
            buf.put_slice(&self.bytes[self.offset..self.offset + count]);
            self.offset += count;
            std::task::Poll::Ready(Ok(()))
        }
    }
    let raw = SapientMessage {
        content: Some(Content::Registration(fixtures::valid_registration())),
        ..Default::default()
    }
    .encode_to_vec();
    let mut bytes = (raw.len() as u32).to_le_bytes().to_vec();
    bytes.extend(raw);
    let mut dmm = DmmConnection::new(NODE, ResetAfter { bytes, offset: 0 }, tokio::io::sink());
    let result = run_dmm_scenario(&mut dmm, Instant::now() + Duration::from_secs(1)).await;
    let report = report(Role::Dmm, result, dmm.take_findings());
    assert_eq!(report.findings.len(), 2);
    check(&report, Check::Registration, CheckStatus::Completed);
    assert_eq!(
        report.operational_error.as_ref().unwrap().kind,
        "ConnectionReset"
    );
    assert_eq!(report.outcome, RunOutcome::Failed);
    assert_eq!(report.exit_code(), ExitCode::from(2));
}

#[test]
fn operational_failure_prevents_pass_even_after_all_checks_complete() {
    let mut scenario = ScenarioResult::new(&[Check::Registration]);
    scenario.complete(Check::Registration);
    scenario.record_error(
        "receive_or_reply",
        std::io::ErrorKind::ConnectionReset.into(),
    );
    let report = report(Role::Dmm, scenario, vec![]);
    assert_eq!(report.outcome, RunOutcome::Incomplete);
    assert!(!report.passed);
    assert_eq!(report.exit_code(), ExitCode::from(2));
    assert_eq!(
        serde_json::to_value(report).unwrap()["operational_error"]["kind"],
        "ConnectionReset"
    );
}

#[tokio::test(start_paused = true)]
async fn delayed_fragmented_ack_keeps_status_cadence_and_current_mode() {
    use sapient_conformance_core::{
        bsi_flex_335_v2_0::{
            Task,
            detection_report::LocationOneof,
            registration::location_type::{CoordinatesOneof, DatumOneof},
            task::{Command, command::Command as TaskCommand},
        },
        validation::sapient_message::validate_sapient_message,
    };
    async fn validated(peer: &mut DuplexStream) -> Content {
        let len = peer.read_u32_le().await.unwrap();
        let mut bytes = vec![0; len as usize];
        peer.read_exact(&mut bytes).await.unwrap();
        let message = SapientMessage::decode(bytes.as_slice()).unwrap();
        let outcome = validate_sapient_message(message.clone());
        assert!(outcome.passed, "{:?}", outcome.findings);
        message.content.unwrap()
    }
    let (stream, mut peer) = duplex(65536);
    let (r, w) = split(stream);
    let mut asm = AsmConnection::new(NODE, r, w);
    let start = Instant::now();
    let target = async {
        let Content::Registration(registration) = validated(&mut peer).await else {
            panic!("registration");
        };
        send(
            &mut peer,
            Content::RegistrationAck(RegistrationAck {
                acceptance: Some(true),
                ack_response_reason: vec![],
            }),
        )
        .await;
        let Content::StatusReport(initial) = validated(&mut peer).await else {
            panic!("status");
        };
        assert_eq!(initial.mode.as_deref(), Some("Default"));
        let Content::DetectionReport(detection) = validated(&mut peer).await else {
            panic!("detection");
        };
        let Some(LocationOneof::RangeBearing(position)) = detection.location_oneof else {
            panic!("range/bearing detection");
        };
        for mode in &registration.mode_definition {
            let declared = mode.detection_definition[0].location_type.as_ref().unwrap();
            assert_eq!(
                declared.coordinates_oneof,
                position
                    .coordinate_system
                    .map(CoordinatesOneof::RangeBearingUnits)
            );
            assert_eq!(
                declared.datum_oneof,
                position.datum.map(DatumOneof::RangeBearingDatum)
            );
        }
        let Content::Alert(alert) = validated(&mut peer).await else {
            panic!("alert");
        };
        send(
            &mut peer,
            Content::Task(Task {
                task_id: Some(ID.into()),
                control: Some(1),
                command: Some(Command {
                    command: Some(TaskCommand::ModeChange("Alternate".into())),
                    command_parameter: None,
                }),
                ..Default::default()
            }),
        )
        .await;
        assert!(matches!(validated(&mut peer).await, Content::TaskAck(_)));
        // Leave the AlertAck header incomplete across two status deadlines.
        let ack = SapientMessage {
            timestamp: Some(Timestamp {
                seconds: 10,
                nanos: 0,
            }),
            node_id: Some(NODE.into()),
            content: Some(Content::AlertAck(AlertAck {
                alert_id: alert.alert_id,
                alert_ack_status: Some(1),
                reason: vec![],
            })),
            ..Default::default()
        }
        .encode_to_vec();
        let header = (ack.len() as u32).to_le_bytes();
        peer.write_all(&header[..2]).await.unwrap();
        let mut last_id = initial.report_id;
        for seconds in [5, 10] {
            let Content::StatusReport(status) = validated(&mut peer).await else {
                panic!("periodic status");
            };
            assert_eq!(Instant::now() - start, Duration::from_secs(seconds));
            assert_eq!(status.system, Some(1));
            assert_eq!(status.mode.as_deref(), Some("Alternate"));
            assert_ne!(status.report_id, last_id);
            last_id = status.report_id;
        }
        peer.write_all(&header[2..]).await.unwrap();
        peer.write_all(&ack).await.unwrap();
        let Content::StatusReport(goodbye) = validated(&mut peer).await else {
            panic!("goodbye");
        };
        assert_eq!(goodbye.system, Some(5));
        assert_eq!(goodbye.mode.as_deref(), Some("Alternate"));
        assert_ne!(goodbye.report_id, last_id);
    };
    let (result, ()) = tokio::join!(
        run_asm_scenario(&mut asm, start + Duration::from_secs(20)),
        target
    );
    verdict(
        &report(Role::Asm, result, asm.take_findings()),
        RunOutcome::Passed,
    );
}

#[tokio::test(start_paused = true)]
async fn periodic_status_backpressure_obeys_the_run_deadline() {
    let (r, mut inbound) = duplex(65536);
    let (w, mut outbound) = duplex(1);
    let mut asm = AsmConnection::new(NODE, r, w);
    let start = Instant::now();
    let target = async {
        receive(&mut outbound).await;
        send(
            &mut inbound,
            Content::RegistrationAck(RegistrationAck {
                acceptance: Some(true),
                ack_response_reason: vec![],
            }),
        )
        .await;
        for _ in 0..3 {
            receive(&mut outbound).await;
        } // status, detection, alert
        // Neither read periodic reports nor acknowledge the alert.
        sleep(Duration::from_secs(10)).await;
    };
    let (result, ()) = tokio::join!(
        async {
            let result = run_asm_scenario(&mut asm, start + Duration::from_secs(8)).await;
            assert_eq!(Instant::now() - start, Duration::from_secs(8));
            result
        },
        target
    );
    let report = report(Role::Asm, result, asm.take_findings());
    check(&report, Check::StatusReport, CheckStatus::Completed);
    check(&report, Check::Goodbye, CheckStatus::Incomplete);
    assert_eq!(report.exit_code(), ExitCode::from(2));
    assert_eq!(
        report.operational_error.unwrap().stage,
        "send_periodic_status"
    );
}
