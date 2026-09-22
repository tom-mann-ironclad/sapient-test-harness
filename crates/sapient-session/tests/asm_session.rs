//! Synchronous tests driving `AsmSession` directly (no sockets needed --
//! the state machine itself is pure), mirroring `tests/dmm_session.rs`'s
//! coverage but flipped for the ASM role. A separate test
//! (`tests/asm_driver.rs`) exercises the real async wire path
//! end-to-end.

use sapient_conformance_core::bsi_flex_335_v2_0::{
    AlertAck, RegistrationAck, SapientMessage, StatusReport, Task, TaskAck,
    sapient_message::Content,
    task::{Command, command::Command as TaskCommandKind},
    task_ack::TaskStatus,
};
use sapient_session::{AsmSession, AsmSessionState};

mod common;
use common::{ALTERNATE_MODE, DEFAULT_MODE, decode, encode, valid_registration};

const HARNESS_NODE_ID: &str = "550e8400-e29b-41d4-a716-446655440000";
const DMM_NODE_ID: &str = "550e8400-e29b-41d4-a716-446655440001";

fn envelope(timestamp_seconds: i64, content: Content) -> SapientMessage {
    common::envelope(DMM_NODE_ID, HARNESS_NODE_ID, timestamp_seconds, content)
}

/// Drives a session through `register()` + an accepted `RegistrationAck`,
/// landing in `Registered`.
fn register_and_accept(session: &mut AsmSession) {
    session.register(valid_registration());
    let reply = session.on_bytes(&encode(envelope(
        0,
        Content::RegistrationAck(RegistrationAck {
            acceptance: Some(true),
            ack_response_reason: vec![],
        }),
    )));
    assert!(reply.is_none(), "RegistrationAck itself gets no reply");
}

#[test]
fn happy_path_registration_is_accepted() {
    let mut session = AsmSession::new(HARNESS_NODE_ID);
    register_and_accept(&mut session);

    assert!(matches!(session.state(), AsmSessionState::Registered(_)));
    assert!(session.findings().is_empty());
}

#[test]
fn registration_rejected_returns_to_not_registered() {
    let mut session = AsmSession::new(HARNESS_NODE_ID);
    session.register(valid_registration());

    let reply = session.on_bytes(&encode(envelope(
        0,
        Content::RegistrationAck(RegistrationAck {
            acceptance: Some(false),
            ack_response_reason: vec!["not today".to_string()],
        }),
    )));

    assert!(reply.is_none());
    assert!(matches!(session.state(), AsmSessionState::NotRegistered));
    assert!(
        session
            .findings()
            .iter()
            .any(|f| f.rule_id == "session.registration.rejected"),
    );
}

#[test]
fn registration_ack_before_registering_is_a_sequencing_violation() {
    let mut session = AsmSession::new(HARNESS_NODE_ID);

    session.on_bytes(&encode(envelope(
        0,
        Content::RegistrationAck(RegistrationAck {
            acceptance: Some(true),
            ack_response_reason: vec![],
        }),
    )));

    assert!(matches!(session.state(), AsmSessionState::NotRegistered));
    assert!(
        session
            .findings()
            .iter()
            .any(|f| f.rule_id == "session.sequencing.unexpected_registration_ack"),
    );
}

#[test]
fn unprompted_registration_ack_while_registered_is_a_finding() {
    let mut session = AsmSession::new(HARNESS_NODE_ID);
    register_and_accept(&mut session);

    session.on_bytes(&encode(envelope(
        1,
        Content::RegistrationAck(RegistrationAck {
            acceptance: Some(true),
            ack_response_reason: vec![],
        }),
    )));

    assert!(matches!(session.state(), AsmSessionState::Registered(_)));
    assert!(
        session
            .findings()
            .iter()
            .any(|f| f.rule_id == "session.unexpected_message_for_role"),
    );
}

#[test]
fn message_before_registration_accepted_is_a_sequencing_violation() {
    let mut session = AsmSession::new(HARNESS_NODE_ID);

    let reply = session.on_bytes(&encode(envelope(
        0,
        Content::Task(Task {
            task_id: Some("01H1VV3VN40RV97CDFSXJB44KA".to_string()),
            task_name: None,
            task_description: None,
            task_start_time: None,
            task_end_time: None,
            control: Some(1),
            region: vec![],
            command: None,
        }),
    )));

    assert!(
        reply.is_none(),
        "no reply -- Error is post-Registration only"
    );
    assert!(
        session
            .findings()
            .iter()
            .any(|f| f.rule_id == "session.sequencing.registration_required"),
    );
}

#[test]
fn task_without_mode_change_gets_accepted_task_ack() {
    let mut session = AsmSession::new(HARNESS_NODE_ID);
    register_and_accept(&mut session);

    let reply = session
        .on_bytes(&encode(envelope(
            1,
            Content::Task(Task {
                task_id: Some("01H1VV3VN40RV97CDFSXJB44KA".to_string()),
                task_name: None,
                task_description: None,
                task_start_time: None,
                task_end_time: None,
                control: Some(1),
                region: vec![],
                command: None,
            }),
        )))
        .expect("a valid Task should get a TaskAck reply");

    match decode(&reply).content {
        Some(Content::TaskAck(TaskAck {
            task_id,
            task_status,
            ..
        })) => {
            assert_eq!(task_id.as_deref(), Some("01H1VV3VN40RV97CDFSXJB44KA"));
            assert_eq!(task_status, Some(TaskStatus::Accepted as i32));
        }
        other => panic!("expected a TaskAck, got {other:?}"),
    }
    assert!(session.findings().is_empty());
}

#[test]
fn mode_change_task_to_known_mode_updates_active_mode_and_accepts() {
    let mut session = AsmSession::new(HARNESS_NODE_ID);
    register_and_accept(&mut session);

    let reply = session
        .on_bytes(&encode(envelope(
            1,
            Content::Task(Task {
                task_id: Some("01H1VV3VN40RV97CDFSXJB44KA".to_string()),
                task_name: None,
                task_description: None,
                task_start_time: None,
                task_end_time: None,
                control: Some(1),
                region: vec![],
                command: Some(Command {
                    command_parameter: None,
                    command: Some(TaskCommandKind::ModeChange(ALTERNATE_MODE.to_string())),
                }),
            }),
        )))
        .unwrap();

    match decode(&reply).content {
        Some(Content::TaskAck(ack)) => {
            assert_eq!(ack.task_status, Some(TaskStatus::Accepted as i32))
        }
        other => panic!("expected a TaskAck, got {other:?}"),
    }
    assert!(session.findings().is_empty());

    if let AsmSessionState::Registered(contract) = session.state() {
        assert_eq!(
            contract.active_mode.mode_name.as_deref(),
            Some(ALTERNATE_MODE)
        );
    } else {
        panic!("expected Registered state");
    }
}

#[test]
fn mode_change_task_to_unknown_mode_is_rejected_and_a_finding() {
    let mut session = AsmSession::new(HARNESS_NODE_ID);
    register_and_accept(&mut session);

    let reply = session
        .on_bytes(&encode(envelope(
            1,
            Content::Task(Task {
                task_id: Some("01H1VV3VN40RV97CDFSXJB44KA".to_string()),
                task_name: None,
                task_description: None,
                task_start_time: None,
                task_end_time: None,
                control: Some(1),
                region: vec![],
                command: Some(Command {
                    command_parameter: None,
                    command: Some(TaskCommandKind::ModeChange("Nonexistent".to_string())),
                }),
            }),
        )))
        .unwrap();

    match decode(&reply).content {
        Some(Content::TaskAck(ack)) => {
            assert_eq!(ack.task_status, Some(TaskStatus::Rejected as i32))
        }
        other => panic!("expected a TaskAck, got {other:?}"),
    }
    assert!(
        session
            .findings()
            .iter()
            .any(|f| f.rule_id == "session.task.mode_change_unknown_mode"),
    );
    if let AsmSessionState::Registered(contract) = session.state() {
        assert_eq!(
            contract.active_mode.mode_name.as_deref(),
            Some(DEFAULT_MODE)
        );
    } else {
        panic!("expected Registered state");
    }
}

#[test]
fn alert_ack_correlates_against_outstanding_alerts() {
    let mut session = AsmSession::new(HARNESS_NODE_ID);
    register_and_accept(&mut session);

    session.issue_alert(sapient_conformance_core::bsi_flex_335_v2_0::Alert {
        alert_id: Some("01H1VV3VN40RV97CDFSXJB44K9".to_string()),
        alert_type: Some(1),
        status: Some(1),
        description: None,
        location_oneof: None,
        region_id: None,
        priority: None,
        ranking: None,
        confidence: None,
        associated_file: vec![],
        associated_detection: vec![],
        additional_information: None,
    });

    session.on_bytes(&encode(envelope(
        1,
        Content::AlertAck(AlertAck {
            alert_id: Some("01H1VV3VN40RV97CDFSXJB44K9".to_string()),
            reason: vec![],
            alert_ack_status: Some(1),
        }),
    )));

    assert!(session.findings().is_empty());
}

#[test]
fn alert_ack_with_unknown_alert_id_is_a_finding() {
    let mut session = AsmSession::new(HARNESS_NODE_ID);
    register_and_accept(&mut session);

    session.on_bytes(&encode(envelope(
        1,
        Content::AlertAck(AlertAck {
            alert_id: Some("01H1VV3VN40RV97CDFSXJB44K9".to_string()),
            reason: vec![],
            alert_ack_status: Some(1),
        }),
    )));

    assert!(
        session
            .findings()
            .iter()
            .any(|f| f.rule_id == "session.alert_ack.correlation_mismatch"),
    );
}

#[test]
fn post_registration_invalid_task_triggers_an_error_reply() {
    let mut session = AsmSession::new(HARNESS_NODE_ID);
    register_and_accept(&mut session);

    // A Task missing its mandatory `control` field.
    let reply = session
        .on_bytes(&encode(envelope(
            1,
            Content::Task(Task {
                task_id: Some("01H1VV3VN40RV97CDFSXJB44KA".to_string()),
                task_name: None,
                task_description: None,
                task_start_time: None,
                task_end_time: None,
                control: None,
                region: vec![],
                command: None,
            }),
        )))
        .expect("a post-Registration validation failure should get an Error reply");

    assert!(matches!(decode(&reply).content, Some(Content::Error(_))));
    assert!(matches!(session.state(), AsmSessionState::Registered(_)));
}

#[test]
fn undecodable_bytes_after_registration_trigger_an_error_reply_with_the_packet() {
    let mut session = AsmSession::new(HARNESS_NODE_ID);
    register_and_accept(&mut session);

    let garbage = vec![0xFF_u8, 0x00, 0xAB, 0xCD];
    let reply = session
        .on_bytes(&garbage)
        .expect("undecodable bytes post-Registration should get an Error reply");

    match decode(&reply).content {
        Some(Content::Error(error)) => {
            assert_eq!(error.packet.as_deref(), Some(garbage.as_slice()));
        }
        other => panic!("expected an Error, got {other:?}"),
    }
}

#[test]
fn undecodable_bytes_before_registration_get_no_reply() {
    let mut session = AsmSession::new(HARNESS_NODE_ID);

    let reply = session.on_bytes(&[0xFF_u8, 0x00]);

    assert!(reply.is_none());
    assert!(matches!(session.state(), AsmSessionState::NotRegistered));
}

#[test]
fn receiving_an_unprompted_error_is_recorded_as_a_finding_about_the_peer() {
    let mut session = AsmSession::new(HARNESS_NODE_ID);
    register_and_accept(&mut session);

    let reply = session.on_bytes(&encode(envelope(
        1,
        Content::Error(sapient_conformance_core::bsi_flex_335_v2_0::Error {
            packet: Some(vec![1, 2, 3]),
            error_message: vec!["couldn't parse your last message".to_string()],
        }),
    )));

    assert!(reply.is_none());
    assert!(matches!(session.state(), AsmSessionState::Registered(_)));
    assert!(
        session
            .findings()
            .iter()
            .any(|f| f.rule_id == "session.peer_reported_error"),
    );
}

#[test]
fn wrong_role_message_inbound_is_a_finding() {
    let mut session = AsmSession::new(HARNESS_NODE_ID);
    register_and_accept(&mut session);

    // StatusReport is an ASM-to-DMM message; receiving one is wrong-role.
    session.on_bytes(&encode(envelope(
        1,
        Content::StatusReport(StatusReport {
            report_id: Some("01H1VV3VN40RV97CDFSXJB44K9".to_string()),
            system: Some(1),
            info: Some(1),
            active_task_id: None,
            mode: Some(DEFAULT_MODE.to_string()),
            power: None,
            node_location: None,
            field_of_view: None,
            obscuration: vec![],
            status: vec![],
            coverage: vec![],
        }),
    )));

    assert!(
        session
            .findings()
            .iter()
            .any(|f| f.rule_id == "session.unexpected_message_for_role"),
    );
}
