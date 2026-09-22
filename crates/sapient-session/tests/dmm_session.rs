//! Synchronous tests driving `DmmSession` directly (no sockets needed --
//! the state machine itself is pure). A separate test (`tests/dmm_driver.rs`)
//! exercises the real async wire path end-to-end for the happy path, since
//! that's the one thing this file's direct `on_bytes` calls don't touch.

use prost::Message;
use prost_types::Timestamp;
use sapient_conformance_core::bsi_flex_335_v2_0::{
    Alert, DetectionReport, Registration, SapientMessage, StatusReport, Task, TaskAck,
    alert::LocationOneof,
    detection_report::{DetectionReportClassification, LocationOneof as DetectionLocationOneof},
    registration::{
        Capability, ClassDefinition, ConfigurationData, DetectionClassDefinition,
        DetectionDefinition, Duration, LocationType, ModeDefinition, ModeType, NodeDefinition,
        RegionDefinition, StatusDefinition, TaskDefinition, TimeUnits,
        location_type::{CoordinatesOneof, DatumOneof},
    },
    sapient_message::Content,
    status_report::System,
    task::{Command, command::Command as TaskCommandKind},
    task_ack::TaskStatus,
};
use sapient_session::{DmmSession, SessionState};

const HARNESS_NODE_ID: &str = "550e8400-e29b-41d4-a716-446655440000";
const ASM_NODE_ID: &str = "550e8400-e29b-41d4-a716-446655440001";
const DEFAULT_MODE: &str = "Default";
const ALTERNATE_MODE: &str = "Alternate";
const DECLARED_CLASSIFICATION_TYPE: &str = "Human";
const STATUS_INTERVAL_SECONDS: f32 = 5.0;

fn valid_location_type() -> LocationType {
    LocationType {
        coordinates_oneof: Some(CoordinatesOneof::RangeBearingUnits(1)),
        datum_oneof: Some(DatumOneof::RangeBearingDatum(1)),
        zone: None,
    }
}

fn duration(units: TimeUnits, value: f32) -> Duration {
    Duration {
        units: Some(units as i32),
        value: Some(value),
    }
}

fn mode(name: &str, mode_type: ModeType) -> ModeDefinition {
    ModeDefinition {
        mode_name: Some(name.to_string()),
        mode_type: Some(mode_type as i32),
        mode_description: None,
        settle_time: Some(duration(TimeUnits::Seconds, 1.0)),
        maximum_latency: None,
        scan_type: None,
        tracking_type: None,
        duration: None,
        mode_parameter: vec![],
        detection_definition: vec![DetectionDefinition {
            behaviour_definition: vec![],
            detection_performance: vec![],
            detection_class_definition: vec![DetectionClassDefinition {
                confidence_definition: None,
                class_performance: vec![],
                class_definition: vec![ClassDefinition {
                    r#type: Some(DECLARED_CLASSIFICATION_TYPE.to_string()),
                    units: None,
                    sub_class: vec![],
                }],
                taxonomy_dock_definition: vec![],
            }],
            detection_report: vec![],
            geometric_error: None,
            velocity_type: None,
            location_type: Some(valid_location_type()),
        }],
        task: Some(TaskDefinition {
            command: vec![],
            concurrent_tasks: Some(1),
            region_definition: Some(RegionDefinition {
                settle_time: None,
                region_type: vec![1],
                region_area: vec![valid_location_type()],
                class_filter_definition: vec![],
                behaviour_filter_definition: vec![],
            }),
        }),
    }
}

/// A registration declaring two modes (one MODE_TYPE_DEFAULT, one
/// MODE_TYPE_PERMANENT reachable via a mode_change task) and a
/// `status_interval` of `STATUS_INTERVAL_SECONDS`.
fn valid_registration() -> Registration {
    Registration {
        icd_version: Some("BSI Flex 335 v2.0".to_string()),
        node_definition: vec![NodeDefinition {
            node_type: Some(1),
            node_sub_type: vec![],
        }],
        name: None,
        short_name: None,
        capabilities: vec![Capability {
            category: Some("Radar".to_string()),
            r#type: Some("Range".to_string()),
            value: None,
            units: None,
        }],
        status_definition: Some(StatusDefinition {
            status_interval: Some(duration(TimeUnits::Seconds, STATUS_INTERVAL_SECONDS)),
            location_definition: None,
            coverage_definition: None,
            obscuration_definition: None,
            status_report: vec![],
            field_of_view_definition: None,
        }),
        mode_definition: vec![
            mode(DEFAULT_MODE, ModeType::Default),
            mode(ALTERNATE_MODE, ModeType::Permanent),
        ],
        reporting_region: vec![],
        dependent_nodes: vec![],
        config_data: vec![ConfigurationData {
            manufacturer: "Acme".to_string(),
            model: "Mk1".to_string(),
            serial_number: None,
            hardware_version: None,
            software_version: None,
            sub_components: vec![],
        }],
    }
}

fn timestamp(seconds: i64) -> Timestamp {
    Timestamp { seconds, nanos: 0 }
}

fn envelope(node_id: &str, timestamp_seconds: i64, content: Content) -> SapientMessage {
    SapientMessage {
        timestamp: Some(timestamp(timestamp_seconds)),
        node_id: Some(node_id.to_string()),
        destination_id: Some(HARNESS_NODE_ID.to_string()),
        content: Some(content),
        additional_information: None,
    }
}

fn encode(message: SapientMessage) -> Vec<u8> {
    message.encode_to_vec()
}

fn decode_reply(reply: Vec<u8>) -> SapientMessage {
    SapientMessage::decode(reply.as_slice()).expect("harness replies should always be valid")
}

fn status_report_at(seconds: i64, mode_name: &str) -> SapientMessage {
    envelope(
        ASM_NODE_ID,
        seconds,
        Content::StatusReport(StatusReport {
            report_id: Some("01H1VV3VN40RV97CDFSXJB44K9".to_string()),
            system: Some(1),
            info: Some(1),
            active_task_id: None,
            mode: Some(mode_name.to_string()),
            power: None,
            node_location: None,
            field_of_view: None,
            obscuration: vec![],
            status: vec![],
            coverage: vec![],
        }),
    )
}

fn register(session: &mut DmmSession) {
    let reply = session
        .on_bytes(&encode(envelope(
            ASM_NODE_ID,
            0,
            Content::Registration(valid_registration()),
        )))
        .expect("Registration should always get a RegistrationAck reply");
    let ack = decode_reply(reply);
    assert!(matches!(ack.content, Some(Content::RegistrationAck(_))));
}

#[test]
fn happy_path_registration_is_accepted() {
    let mut session = DmmSession::new(HARNESS_NODE_ID);
    register(&mut session);

    assert!(matches!(session.state(), SessionState::Registered(_)));
    assert!(session.findings().is_empty());
}

#[test]
fn message_before_registration_is_a_sequencing_violation() {
    let mut session = DmmSession::new(HARNESS_NODE_ID);

    let reply = session.on_bytes(&encode(status_report_at(0, DEFAULT_MODE)));

    assert!(
        reply.is_none(),
        "no reply -- Error is post-Registration only"
    );
    assert!(matches!(
        session.state(),
        SessionState::AwaitingRegistration
    ));
    let findings = session.findings();
    assert_eq!(findings.len(), 1);
    assert_eq!(
        findings[0].rule_id,
        "session.sequencing.registration_required"
    );
}

#[test]
fn invalid_registration_is_rejected_via_registration_ack_not_error() {
    let mut session = DmmSession::new(HARNESS_NODE_ID);
    let mut bad_registration = valid_registration();
    bad_registration.icd_version = Some("wrong version".to_string());

    let reply = session
        .on_bytes(&encode(envelope(
            ASM_NODE_ID,
            0,
            Content::Registration(bad_registration),
        )))
        .expect("an invalid Registration still gets a RegistrationAck reply");
    let ack = decode_reply(reply);

    match ack.content {
        Some(Content::RegistrationAck(ack)) => assert_eq!(ack.acceptance, Some(false)),
        other => panic!("expected a RegistrationAck, got {other:?}"),
    }
    assert!(matches!(
        session.state(),
        SessionState::AwaitingRegistration
    ));
}

#[test]
fn status_report_within_interval_produces_no_finding() {
    let mut session = DmmSession::new(HARNESS_NODE_ID);
    register(&mut session);

    session.on_bytes(&encode(status_report_at(1, DEFAULT_MODE)));
    session.on_bytes(&encode(status_report_at(
        1 + STATUS_INTERVAL_SECONDS as i64,
        DEFAULT_MODE,
    )));

    assert!(session.findings().is_empty());
}

#[test]
fn status_report_exceeding_interval_is_a_finding() {
    let mut session = DmmSession::new(HARNESS_NODE_ID);
    register(&mut session);

    session.on_bytes(&encode(status_report_at(1, DEFAULT_MODE)));
    session.on_bytes(&encode(status_report_at(
        1 + STATUS_INTERVAL_SECONDS as i64 + 10,
        DEFAULT_MODE,
    )));

    let findings = session.findings();
    assert!(
        findings
            .iter()
            .any(|f| f.rule_id == "session.status_report.interval_exceeded"),
        "expected an interval_exceeded finding, got {findings:?}"
    );
}

#[test]
fn status_report_mode_mismatch_is_a_finding() {
    let mut session = DmmSession::new(HARNESS_NODE_ID);
    register(&mut session);

    // ASM claims to be in a mode the harness never observed it entering.
    session.on_bytes(&encode(status_report_at(1, ALTERNATE_MODE)));

    let findings = session.findings();
    assert!(
        findings
            .iter()
            .any(|f| f.rule_id == "session.status_report.mode_mismatch"),
        "expected a mode_mismatch finding, got {findings:?}"
    );
}

#[test]
fn goodbye_status_report_returns_to_awaiting_registration() {
    let mut session = DmmSession::new(HARNESS_NODE_ID);
    register(&mut session);

    let goodbye = envelope(
        ASM_NODE_ID,
        1,
        Content::StatusReport(StatusReport {
            report_id: Some("01H1VV3VN40RV97CDFSXJB44K9".to_string()),
            system: Some(System::Goodbye as i32),
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
    );
    let reply = session.on_bytes(&encode(goodbye));

    assert!(reply.is_none());
    assert!(matches!(
        session.state(),
        SessionState::AwaitingRegistration
    ));

    // The connection can stay open for a fresh Registration afterwards.
    register(&mut session);
    assert!(matches!(session.state(), SessionState::Registered(_)));
}

#[test]
fn re_registration_fully_replaces_the_contract() {
    let mut session = DmmSession::new(HARNESS_NODE_ID);
    register(&mut session);
    session.on_bytes(&encode(status_report_at(1, DEFAULT_MODE)));
    assert!(session.findings().is_empty());

    // Re-register with a Registration that has swapped which mode is
    // MODE_TYPE_DEFAULT -- the old contract's active mode assumption
    // must not leak through.
    let mut second_registration = valid_registration();
    second_registration.mode_definition = vec![
        mode(DEFAULT_MODE, ModeType::Permanent),
        mode(ALTERNATE_MODE, ModeType::Default),
    ];
    let reply = session
        .on_bytes(&encode(envelope(
            ASM_NODE_ID,
            2,
            Content::Registration(second_registration),
        )))
        .unwrap();
    assert!(matches!(
        decode_reply(reply).content,
        Some(Content::RegistrationAck(ref ack)) if ack.acceptance == Some(true)
    ));

    // Under the new contract, ALTERNATE_MODE is now the active
    // (MODE_TYPE_DEFAULT) mode, so a StatusReport claiming DEFAULT_MODE
    // should now be the mismatch.
    session.on_bytes(&encode(status_report_at(3, DEFAULT_MODE)));
    assert!(
        session
            .findings()
            .iter()
            .any(|f| f.rule_id == "session.status_report.mode_mismatch"),
        "expected the re-registration to have swapped the active mode"
    );
}

#[test]
fn mode_change_task_updates_active_mode() {
    let mut session = DmmSession::new(HARNESS_NODE_ID);
    register(&mut session);

    session.issue_task(&Task {
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
    });

    // No StatusReport claiming ALTERNATE_MODE should now be a mismatch.
    session.on_bytes(&encode(status_report_at(1, ALTERNATE_MODE)));
    assert!(
        !session
            .findings()
            .iter()
            .any(|f| f.rule_id == "session.status_report.mode_mismatch"),
        "mode_change should have updated the tracked active mode"
    );
}

#[test]
fn mode_change_to_unknown_mode_is_a_finding() {
    let mut session = DmmSession::new(HARNESS_NODE_ID);
    register(&mut session);

    session.issue_task(&Task {
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
    });

    assert!(
        session
            .findings()
            .iter()
            .any(|f| f.rule_id == "session.task.mode_change_unknown_mode"),
    );
}

#[test]
fn detection_report_with_declared_classification_produces_no_finding() {
    let mut session = DmmSession::new(HARNESS_NODE_ID);
    register(&mut session);

    session.on_bytes(&encode(envelope(
        ASM_NODE_ID,
        1,
        Content::DetectionReport(DetectionReport {
            report_id: Some("01H1VV3VN40RV97CDFSXJB44K9".to_string()),
            object_id: Some("01H1VV3VN40RV97CDFSXJB44KA".to_string()),
            task_id: None,
            state: None,
            location_oneof: Some(DetectionLocationOneof::Location(
                sapient_conformance_core::bsi_flex_335_v2_0::Location {
                    x: Some(1.0),
                    y: Some(2.0),
                    z: None,
                    x_error: None,
                    y_error: None,
                    z_error: None,
                    coordinate_system: Some(1),
                    datum: Some(1),
                    utm_zone: None,
                },
            )),
            detection_confidence: None,
            track_info: vec![],
            prediction_location: None,
            object_info: vec![],
            classification: vec![DetectionReportClassification {
                r#type: Some(DECLARED_CLASSIFICATION_TYPE.to_string()),
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
        }),
    )));

    assert!(session.findings().is_empty());
}

#[test]
fn detection_report_with_undeclared_classification_is_a_finding() {
    let mut session = DmmSession::new(HARNESS_NODE_ID);
    register(&mut session);

    session.on_bytes(&encode(envelope(
        ASM_NODE_ID,
        1,
        Content::DetectionReport(DetectionReport {
            report_id: Some("01H1VV3VN40RV97CDFSXJB44K9".to_string()),
            object_id: Some("01H1VV3VN40RV97CDFSXJB44KA".to_string()),
            task_id: None,
            state: None,
            location_oneof: Some(DetectionLocationOneof::Location(
                sapient_conformance_core::bsi_flex_335_v2_0::Location {
                    x: Some(1.0),
                    y: Some(2.0),
                    z: None,
                    x_error: None,
                    y_error: None,
                    z_error: None,
                    coordinate_system: Some(1),
                    datum: Some(1),
                    utm_zone: None,
                },
            )),
            detection_confidence: None,
            track_info: vec![],
            prediction_location: None,
            object_info: vec![],
            classification: vec![DetectionReportClassification {
                r#type: Some("SomethingNeverDeclared".to_string()),
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
        }),
    )));

    assert!(
        session
            .findings()
            .iter()
            .any(|f| f.rule_id == "session.detection_report.undeclared_classification"),
    );
}

#[test]
fn alert_receives_an_alert_ack_reply() {
    let mut session = DmmSession::new(HARNESS_NODE_ID);
    register(&mut session);

    let reply = session
        .on_bytes(&encode(envelope(
            ASM_NODE_ID,
            1,
            Content::Alert(Alert {
                alert_id: Some("01H1VV3VN40RV97CDFSXJB44K9".to_string()),
                alert_type: Some(1),
                status: Some(1),
                description: None,
                location_oneof: Some(LocationOneof::Location(
                    sapient_conformance_core::bsi_flex_335_v2_0::Location {
                        x: Some(1.0),
                        y: Some(2.0),
                        z: None,
                        x_error: None,
                        y_error: None,
                        z_error: None,
                        coordinate_system: Some(1),
                        datum: Some(1),
                        utm_zone: None,
                    },
                )),
                region_id: None,
                priority: None,
                ranking: None,
                confidence: None,
                associated_file: vec![],
                associated_detection: vec![],
                additional_information: None,
            }),
        )))
        .expect("a valid Alert should get an AlertAck reply");

    match decode_reply(reply).content {
        Some(Content::AlertAck(ack)) => {
            assert_eq!(ack.alert_id.as_deref(), Some("01H1VV3VN40RV97CDFSXJB44K9"));
        }
        other => panic!("expected an AlertAck, got {other:?}"),
    }
}

#[test]
fn task_ack_correlates_against_outstanding_tasks() {
    let mut session = DmmSession::new(HARNESS_NODE_ID);
    register(&mut session);

    session.issue_task(&Task {
        task_id: Some("01H1VV3VN40RV97CDFSXJB44KA".to_string()),
        task_name: None,
        task_description: None,
        task_start_time: None,
        task_end_time: None,
        control: Some(1),
        region: vec![],
        command: None,
    });

    session.on_bytes(&encode(envelope(
        ASM_NODE_ID,
        1,
        Content::TaskAck(TaskAck {
            task_id: Some("01H1VV3VN40RV97CDFSXJB44KA".to_string()),
            task_status: Some(TaskStatus::Accepted as i32),
            associated_file: None,
            reason: vec![],
        }),
    )));

    assert!(session.findings().is_empty());
}

#[test]
fn task_ack_with_unknown_task_id_is_a_finding() {
    let mut session = DmmSession::new(HARNESS_NODE_ID);
    register(&mut session);

    session.on_bytes(&encode(envelope(
        ASM_NODE_ID,
        1,
        Content::TaskAck(TaskAck {
            task_id: Some("01H1VV3VN40RV97CDFSXJB44KA".to_string()),
            task_status: Some(TaskStatus::Accepted as i32),
            associated_file: None,
            reason: vec![],
        }),
    )));

    assert!(
        session
            .findings()
            .iter()
            .any(|f| f.rule_id == "session.task_ack.correlation_mismatch"),
    );
}

#[test]
fn post_registration_invalid_message_triggers_an_error_reply() {
    let mut session = DmmSession::new(HARNESS_NODE_ID);
    register(&mut session);

    // A StatusReport missing its mandatory `system` field.
    let mut bad_status_report = match status_report_at(1, DEFAULT_MODE).content {
        Some(Content::StatusReport(status_report)) => status_report,
        _ => unreachable!(),
    };
    bad_status_report.system = None;

    let reply = session
        .on_bytes(&encode(envelope(
            ASM_NODE_ID,
            1,
            Content::StatusReport(bad_status_report),
        )))
        .expect("a post-Registration validation failure should get an Error reply");

    assert!(matches!(
        decode_reply(reply).content,
        Some(Content::Error(_))
    ));
    // Purely informational -- session stays Registered.
    assert!(matches!(session.state(), SessionState::Registered(_)));
}

#[test]
fn undecodable_bytes_after_registration_trigger_an_error_reply_with_the_packet() {
    let mut session = DmmSession::new(HARNESS_NODE_ID);
    register(&mut session);

    let garbage = vec![0xFF_u8, 0x00, 0xAB, 0xCD];
    let reply = session
        .on_bytes(&garbage)
        .expect("undecodable bytes post-Registration should get an Error reply");

    match decode_reply(reply).content {
        Some(Content::Error(error)) => {
            assert_eq!(error.packet.as_deref(), Some(garbage.as_slice()));
        }
        other => panic!("expected an Error, got {other:?}"),
    }
}

#[test]
fn undecodable_bytes_before_registration_get_no_reply() {
    let mut session = DmmSession::new(HARNESS_NODE_ID);

    let reply = session.on_bytes(&[0xFF_u8, 0x00]);

    assert!(reply.is_none());
    assert!(matches!(
        session.state(),
        SessionState::AwaitingRegistration
    ));
}

#[test]
fn receiving_an_unprompted_error_is_recorded_as_a_finding_about_the_peer() {
    let mut session = DmmSession::new(HARNESS_NODE_ID);
    register(&mut session);

    let reply = session.on_bytes(&encode(envelope(
        ASM_NODE_ID,
        1,
        Content::Error(sapient_conformance_core::bsi_flex_335_v2_0::Error {
            packet: Some(vec![1, 2, 3]),
            error_message: vec!["couldn't parse your last message".to_string()],
        }),
    )));

    assert!(
        reply.is_none(),
        "Error is purely informational, no reply expected"
    );
    assert!(matches!(session.state(), SessionState::Registered(_)));
    assert!(
        session
            .findings()
            .iter()
            .any(|f| f.rule_id == "session.peer_reported_error"),
    );
}
