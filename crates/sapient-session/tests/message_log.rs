//! `last_inbound`/`last_reply` must describe every inbound message, including
//! ones that produce no progress event, so a live session can log all traffic.
mod common;

use common::{ALTERNATE_MODE, DEFAULT_MODE, encode, envelope, valid_registration};
use sapient_conformance_core::bsi_flex_335_v2_0::{
    RegistrationAck, StatusReport, Task, sapient_message::Content, status_report::System,
    task::Command, task::Control, task::command::Command as TaskCommandKind,
};
use sapient_conformance_core::finding::Direction;
use sapient_session::{AsmSession, DmmSession};

const ASM: &str = "550e8400-e29b-41d4-a716-446655440000";
const DMM: &str = "550e8400-e29b-41d4-a716-446655440001";

fn mode_change_task() -> Content {
    Content::Task(Task {
        task_id: Some("01ARZ3NDEKTSV4RRFFQ69G5FAV".into()),
        control: Some(Control::Start as i32),
        command: Some(Command {
            command: Some(TaskCommandKind::ModeChange(ALTERNATE_MODE.into())),
            command_parameter: None,
        }),
        ..Default::default()
    })
}

#[test]
fn asm_session_records_each_inbound_message_and_its_reply() {
    let mut session = AsmSession::new(ASM);
    assert!(session.last_inbound().is_none());
    assert_eq!(session.last_reply(), None);

    session.register(valid_registration());
    session.on_bytes(&encode(envelope(
        DMM,
        ASM,
        1,
        Content::RegistrationAck(RegistrationAck {
            acceptance: Some(true),
            ack_response_reason: vec![],
        }),
    )));
    let inbound = session.last_inbound().unwrap();
    assert_eq!(
        (
            inbound.sequence,
            inbound.direction,
            inbound.message_type.as_str()
        ),
        (1, Direction::Inbound, "RegistrationAck")
    );
    assert_eq!(session.last_reply(), None);

    let reply = session.on_bytes(&encode(envelope(DMM, ASM, 2, mode_change_task())));
    assert!(reply.is_some());
    assert_eq!(session.last_inbound().unwrap().message_type, "Task");
    assert_eq!(session.last_reply(), Some("TaskAck"));

    // Registered, so undecodable bytes get an Error reply.
    session.on_bytes(&[0xff, 0xff, 0xff]);
    let inbound = session.last_inbound().unwrap();
    assert_eq!(
        (inbound.sequence, inbound.message_type.as_str()),
        (3, "undecodable")
    );
    assert_eq!(session.last_reply(), Some("Error"));
}

#[test]
fn dmm_session_records_each_inbound_message_and_its_reply() {
    let mut session = DmmSession::new(DMM);
    session.on_bytes(&encode(envelope(
        ASM,
        DMM,
        1,
        Content::Registration(valid_registration()),
    )));
    assert_eq!(session.last_inbound().unwrap().message_type, "Registration");
    assert_eq!(session.last_reply(), Some("RegistrationAck"));

    // A StatusReport needs no reply; the previous reply must not linger.
    session.on_bytes(&encode(envelope(
        ASM,
        DMM,
        2,
        Content::StatusReport(StatusReport {
            report_id: Some("01ARZ3NDEKTSV4RRFFQ69G5FAW".into()),
            system: Some(System::Ok as i32),
            info: Some(System::Ok as i32),
            mode: Some(DEFAULT_MODE.into()),
            ..Default::default()
        }),
    )));
    let inbound = session.last_inbound().unwrap();
    assert_eq!(
        (inbound.sequence, inbound.message_type.as_str()),
        (2, "StatusReport")
    );
    assert_eq!(session.last_reply(), None);
}
