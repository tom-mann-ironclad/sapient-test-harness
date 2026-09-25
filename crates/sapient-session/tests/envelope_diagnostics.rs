//! Decodable invalid envelopes must be reported without suppressing normal
//! payload processing, protocol replies, or observations later in the same run.
mod common;

use common::{decode, encode, envelope, valid_registration};
use sapient_conformance_core::{
    bsi_flex_335_v2_0::{
        Alert, AlertAck, RegistrationAck, SapientMessage, Task, TaskAck, sapient_message::Content,
    },
    finding::Finding,
};
use sapient_session::{
    AsmEvent, AsmSession, AsmSessionState, DmmEvent, DmmSession, SessionState,
    asm::AsmConnection,
    dmm::DmmConnection,
    framing::{read_frame, write_frame},
};
use tokio::io::{duplex, split};

const ASM: &str = "550e8400-e29b-41d4-a716-446655440000";
const DMM: &str = "550e8400-e29b-41d4-a716-446655440001";
const ID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

#[derive(Clone, Copy, Debug)]
enum Defect {
    MissingTimestamp,
    MalformedTimestamp,
    MissingNode,
    InvalidNode,
    InvalidDestination,
    All,
}
const DEFECTS: [Defect; 6] = [
    Defect::MissingTimestamp,
    Defect::MalformedTimestamp,
    Defect::MissingNode,
    Defect::InvalidNode,
    Defect::InvalidDestination,
    Defect::All,
];

fn damage(message: &mut SapientMessage, defect: Defect) -> Vec<&'static str> {
    match defect {
        Defect::MissingTimestamp => {
            message.timestamp = None;
            vec!["sapient_message.timestamp.missing"]
        }
        Defect::MalformedTimestamp => {
            message.timestamp.as_mut().unwrap().nanos = -1;
            vec!["sapient_message.timestamp.malformed"]
        }
        Defect::MissingNode => {
            message.node_id = None;
            vec!["sapient_message.node_id.invalid"]
        }
        Defect::InvalidNode => {
            message.node_id = Some("bad-node".into());
            vec!["sapient_message.node_id.invalid"]
        }
        Defect::InvalidDestination => {
            message.destination_id = Some("bad-destination".into());
            vec!["sapient_message.destination_id.invalid"]
        }
        Defect::All => {
            let mut rules = damage(message, Defect::MissingTimestamp);
            rules.extend(damage(message, Defect::InvalidNode));
            rules.extend(damage(message, Defect::InvalidDestination));
            rules
        }
    }
}

fn rules(findings: &[Finding]) -> Vec<&str> {
    findings.iter().map(|f| f.rule_id.as_str()).collect()
}

fn accepted() -> Content {
    Content::RegistrationAck(RegistrationAck {
        acceptance: Some(true),
        ack_response_reason: vec![],
    })
}

fn registered_asm() -> AsmSession {
    let mut asm = AsmSession::new(ASM);
    asm.register(valid_registration());
    asm.on_bytes(&encode(envelope(DMM, ASM, 1, accepted())));
    asm.take_event();
    asm
}

fn registered_dmm() -> DmmSession {
    let mut dmm = DmmSession::new(DMM);
    dmm.on_bytes(&encode(envelope(
        ASM,
        DMM,
        1,
        Content::Registration(valid_registration()),
    )));
    dmm.take_event();
    dmm
}

#[tokio::test]
async fn both_drivers_report_every_envelope_defect_and_continue_the_handshake() {
    for defect in DEFECTS {
        let (stream, mut peer) = duplex(65536);
        let (r, w) = split(stream);
        let mut dmm = DmmConnection::new(DMM, r, w);
        let mut registration = envelope(ASM, DMM, 1, Content::Registration(valid_registration()));
        let expected = damage(&mut registration, defect);
        write_frame(&mut peer, &encode(registration)).await.unwrap();
        assert!(dmm.poll_once().await.unwrap());
        assert_eq!(rules(dmm.findings()), expected, "{defect:?}");
        assert!(matches!(dmm.state(), SessionState::Registered(_)));
        assert_eq!(dmm.take_event(), Some(DmmEvent::RegistrationAccepted));
        let reply = read_frame(&mut peer).await.unwrap().unwrap();
        assert!(matches!(
            decode(&reply).content,
            Some(Content::RegistrationAck(RegistrationAck {
                acceptance: Some(true),
                ..
            }))
        ));

        let (stream, mut peer) = duplex(65536);
        let (r, w) = split(stream);
        let mut asm = AsmConnection::new(ASM, r, w);
        asm.register(valid_registration()).await.unwrap();
        read_frame(&mut peer).await.unwrap();
        let mut ack = envelope(DMM, ASM, 1, accepted());
        damage(&mut ack, defect);
        write_frame(&mut peer, &encode(ack)).await.unwrap();
        assert!(asm.poll_once().await.unwrap());
        assert_eq!(rules(asm.findings()), expected, "{defect:?}");
        assert!(matches!(asm.state(), AsmSessionState::Registered(_)));
        assert_eq!(asm.take_event(), Some(AsmEvent::RegistrationAccepted));
    }
}

#[test]
fn dmm_reports_envelope_and_payload_errors_then_handles_later_messages() {
    let mut dmm = registered_dmm();
    let mut message = envelope(ASM, DMM, 2, Content::Alert(Alert::default()));
    let mut expected = damage(&mut message, Defect::All);
    expected.push("alert.alert_id.invalid");
    let raw = encode(message);
    let reply = decode(&dmm.on_bytes(&raw).unwrap());
    assert!(
        matches!(reply.content, Some(Content::Error(ref error)) if error.packet.as_deref() == Some(raw.as_slice()))
    );
    assert_eq!(rules(dmm.findings()), expected);
    assert!(matches!(dmm.state(), SessionState::Registered(_)));
    let reply = decode(
        &dmm.on_bytes(&encode(envelope(
            ASM,
            DMM,
            3,
            Content::Alert(Alert {
                alert_id: Some(ID.into()),
                ..Default::default()
            }),
        )))
        .unwrap(),
    );
    assert!(matches!(reply.content, Some(Content::AlertAck(_))));
    assert_eq!(rules(dmm.findings()), expected);
}

#[test]
fn asm_reports_envelope_and_payload_errors_then_handles_later_messages() {
    let mut asm = registered_asm();
    let mut message = envelope(DMM, ASM, 2, Content::Task(Task::default()));
    let mut expected = damage(&mut message, Defect::All);
    expected.push("task.task_id.invalid");
    let raw = encode(message);
    let reply = decode(&asm.on_bytes(&raw).unwrap());
    assert!(
        matches!(reply.content, Some(Content::Error(ref error)) if error.packet.as_deref() == Some(raw.as_slice()))
    );
    assert_eq!(rules(asm.findings()), expected);
    let reply = decode(
        &asm.on_bytes(&encode(envelope(
            DMM,
            ASM,
            3,
            Content::Task(Task {
                task_id: Some(ID.into()),
                control: Some(1),
                ..Default::default()
            }),
        )))
        .unwrap(),
    );
    assert!(matches!(reply.content, Some(Content::TaskAck(_))));
    assert_eq!(rules(asm.findings()), expected);
}

#[test]
fn correlated_acknowledgements_still_progress_but_keep_envelope_findings() {
    let mut dmm = registered_dmm();
    dmm.issue_task(&Task {
        task_id: Some(ID.into()),
        control: Some(1),
        ..Default::default()
    });
    let mut ack = envelope(
        ASM,
        DMM,
        2,
        Content::TaskAck(TaskAck {
            task_id: Some(ID.into()),
            task_status: Some(1),
            ..Default::default()
        }),
    );
    let expected = damage(&mut ack, Defect::All);
    assert!(dmm.on_bytes(&encode(ack)).is_none());
    assert_eq!(rules(dmm.findings()), expected);
    assert_eq!(
        dmm.take_event(),
        Some(DmmEvent::TaskAcknowledged { task_id: ID.into() })
    );

    let mut asm = registered_asm();
    asm.issue_alert(Alert {
        alert_id: Some(ID.into()),
        ..Default::default()
    });
    let mut ack = envelope(
        DMM,
        ASM,
        2,
        Content::AlertAck(AlertAck {
            alert_id: Some(ID.into()),
            alert_ack_status: Some(1),
            reason: vec![],
        }),
    );
    damage(&mut ack, Defect::All);
    assert!(asm.on_bytes(&encode(ack)).is_none());
    assert_eq!(rules(asm.findings()), expected);
    assert_eq!(
        asm.take_event(),
        Some(AsmEvent::AlertAcknowledged {
            alert_id: ID.into()
        })
    );
}

#[test]
fn pre_registration_findings_accumulate_without_error_replies() {
    let mut dmm = DmmSession::new(DMM);
    let mut message = envelope(
        ASM,
        DMM,
        1,
        Content::Alert(Alert {
            alert_id: Some(ID.into()),
            ..Default::default()
        }),
    );
    let mut expected = damage(&mut message, Defect::All);
    expected.push("session.sequencing.registration_required");
    assert!(dmm.on_bytes(&encode(message)).is_none());
    assert_eq!(rules(dmm.findings()), expected);

    let mut asm = AsmSession::new(ASM);
    let mut message = envelope(
        DMM,
        ASM,
        1,
        Content::Task(Task {
            task_id: Some(ID.into()),
            control: Some(1),
            ..Default::default()
        }),
    );
    damage(&mut message, Defect::All);
    assert!(asm.on_bytes(&encode(message)).is_none());
    assert_eq!(rules(asm.findings()), expected);
}

#[test]
fn invalid_registration_payload_still_uses_negative_registration_ack() {
    let mut dmm = DmmSession::new(DMM);
    let mut registration = valid_registration();
    registration.icd_version = None;
    let mut message = envelope(ASM, DMM, 1, Content::Registration(registration));
    let mut expected = damage(&mut message, Defect::All);
    expected.push("registration.icd_version.missing");
    let reply = decode(&dmm.on_bytes(&encode(message)).unwrap());
    assert!(matches!(
        reply.content,
        Some(Content::RegistrationAck(RegistrationAck {
            acceptance: Some(false),
            ..
        }))
    ));
    assert_eq!(rules(dmm.findings()), expected);
    // Rejection is a protocol response, not termination of the connection/session object.
    dmm.on_bytes(&encode(envelope(
        ASM,
        DMM,
        2,
        Content::Registration(valid_registration()),
    )));
    assert!(matches!(dmm.state(), SessionState::Registered(_)));
    assert_eq!(rules(dmm.findings()), expected);
}

#[test]
fn goodbye_envelope_is_checked_even_though_goodbye_clears_the_contract() {
    use sapient_conformance_core::bsi_flex_335_v2_0::StatusReport;
    let mut dmm = registered_dmm();
    let mut message = envelope(
        ASM,
        DMM,
        2,
        Content::StatusReport(StatusReport {
            report_id: Some(ID.into()),
            system: Some(5),
            info: Some(1),
            mode: Some("Default".into()),
            ..Default::default()
        }),
    );
    let expected = damage(&mut message, Defect::All);
    assert!(dmm.on_bytes(&encode(message)).is_none());
    assert_eq!(rules(dmm.findings()), expected);
    assert_eq!(dmm.take_event(), Some(DmmEvent::GoodbyeReceived));
    assert!(matches!(dmm.state(), SessionState::AwaitingRegistration));
}

#[test]
fn malformed_goodbye_payload_records_findings_and_still_ends_session() {
    use sapient_conformance_core::bsi_flex_335_v2_0::StatusReport;
    for (field, rule) in [
        ("report_id", "status_report.report_id.invalid"),
        ("info", "status_report.info.invalid"),
        ("mode", "status_report.mode.missing"),
    ] {
        let mut dmm = registered_dmm();
        let mut goodbye = StatusReport {
            report_id: Some(ID.into()),
            system: Some(5),
            info: Some(1),
            mode: Some("Default".into()),
            ..Default::default()
        };
        match field {
            "report_id" => goodbye.report_id = None,
            "info" => goodbye.info = None,
            _ => goodbye.mode = None,
        }
        let message = envelope(ASM, DMM, 2, Content::StatusReport(goodbye));
        assert!(dmm.on_bytes(&encode(message)).is_none());
        assert_eq!(rules(dmm.findings()), vec![rule]);
        assert_eq!(dmm.take_event(), Some(DmmEvent::GoodbyeReceived));
        assert!(matches!(dmm.state(), SessionState::AwaitingRegistration));
    }
}
