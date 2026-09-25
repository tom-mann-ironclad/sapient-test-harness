//! Task acceptance is progress, not completion; mode requests are not active modes.
use sapient_conformance_core::bsi_flex_335_v2_0::{
    Task, TaskAck, sapient_message::Content, task_ack::TaskStatus,
};
use sapient_session::{DmmEvent, DmmSession, SessionState};
mod common;
use common::{encode, envelope, valid_registration};
const LOCAL: &str = "550e8400-e29b-41d4-a716-446655440000";
const PEER: &str = "550e8400-e29b-41d4-a716-446655440001";
const ID: &str = "01H1VV3VN40RV97CDFSXJB44KA";
fn registered() -> DmmSession {
    let mut session = DmmSession::new(LOCAL);
    session.on_bytes(&encode(envelope(
        PEER,
        LOCAL,
        0,
        Content::Registration(valid_registration()),
    )));
    session.take_event();
    session
}
fn issue(session: &mut DmmSession) {
    session.issue_task(&Task {
        task_id: Some(ID.into()),
        control: Some(1),
        ..Default::default()
    });
}
fn ack(session: &mut DmmSession, status: TaskStatus) {
    session.on_bytes(&encode(envelope(
        PEER,
        LOCAL,
        1,
        Content::TaskAck(TaskAck {
            task_id: Some(ID.into()),
            task_status: Some(status as i32),
            ..Default::default()
        }),
    )));
}
#[test]
fn accepted_tasks_remain_correlated_until_completed_or_failed() {
    for terminal in [TaskStatus::Completed, TaskStatus::Failed] {
        let mut session = registered();
        issue(&mut session);
        ack(&mut session, TaskStatus::Accepted);
        let SessionState::Registered(contract) = session.state() else {
            panic!()
        };
        assert!(contract.outstanding_task_ids.contains(ID));
        assert_eq!(contract.tasks[ID].status, Some(TaskStatus::Accepted));
        ack(&mut session, terminal);
        let SessionState::Registered(contract) = session.state() else {
            panic!()
        };
        assert!(!contract.outstanding_task_ids.contains(ID));
        assert!(!contract.tasks.contains_key(ID));
        assert_eq!(
            session.take_event(),
            Some(DmmEvent::TaskAcknowledged { task_id: ID.into() })
        );
        assert!(session.findings().is_empty());
    }
}
#[test]
fn duplicate_and_invalid_transitions_preserve_the_last_valid_state() {
    let mut session = registered();
    issue(&mut session);
    ack(&mut session, TaskStatus::Failed);
    assert_eq!(
        session.findings().last().unwrap().rule_id,
        "session.task_ack.invalid_transition"
    );
    ack(&mut session, TaskStatus::Accepted);
    ack(&mut session, TaskStatus::Accepted);
    assert_eq!(
        session.findings().last().unwrap().rule_id,
        "session.task_ack.duplicate"
    );
    assert!(session.take_event().is_none());
    ack(&mut session, TaskStatus::Rejected);
    assert_eq!(
        session.findings().last().unwrap().rule_id,
        "session.task_ack.invalid_transition"
    );
    let SessionState::Registered(contract) = session.state() else {
        panic!()
    };
    assert_eq!(contract.tasks[ID].status, Some(TaskStatus::Accepted));
    ack(&mut session, TaskStatus::Completed);
    ack(&mut session, TaskStatus::Accepted);
    assert_eq!(
        session.findings().last().unwrap().rule_id,
        "session.task_ack.correlation_mismatch"
    );
}
#[test]
fn direct_rejection_and_completion_are_terminal_but_unknown_ids_are_not_progress() {
    for terminal in [TaskStatus::Rejected, TaskStatus::Completed] {
        let mut session = registered();
        issue(&mut session);
        ack(&mut session, terminal);
        assert!(session.findings().is_empty());
        let SessionState::Registered(contract) = session.state() else {
            panic!()
        };
        assert!(!contract.outstanding_task_ids.contains(ID));
        assert!(!contract.tasks.contains_key(ID));
    }
    let mut session = registered();
    ack(&mut session, TaskStatus::Accepted);
    assert_eq!(
        session.findings()[0].rule_id,
        "session.task_ack.correlation_mismatch"
    );
    assert!(session.take_event().is_none());
}

fn mode_task(session: &mut DmmSession) {
    use sapient_conformance_core::bsi_flex_335_v2_0::task::{Command, command::Command as Kind};
    session.issue_task(&Task {
        task_id: Some(ID.into()),
        control: Some(1),
        command: Some(Command {
            command: Some(Kind::ModeChange("Alternate".into())),
            command_parameter: None,
        }),
        ..Default::default()
    });
}
fn status(session: &mut DmmSession, time: i64, mode: &str) {
    use sapient_conformance_core::bsi_flex_335_v2_0::StatusReport;
    session.on_bytes(&encode(envelope(
        PEER,
        LOCAL,
        time,
        Content::StatusReport(StatusReport {
            report_id: Some(ID.into()),
            system: Some(1),
            info: Some(1),
            mode: Some(mode.into()),
            ..Default::default()
        }),
    )));
}
fn mode(session: &DmmSession) -> &str {
    let SessionState::Registered(contract) = session.state() else {
        panic!()
    };
    contract.active_mode.mode_name.as_deref().unwrap()
}
#[test]
fn rejected_mode_changes_never_activate_and_failed_changes_restore_previous_mode() {
    let mut session = registered();
    mode_task(&mut session);
    assert_eq!(mode(&session), "Default");
    status(&mut session, 0, "Default");
    ack(&mut session, TaskStatus::Rejected);
    status(&mut session, 2, "Default");
    assert_eq!(mode(&session), "Default");
    assert!(session.findings().is_empty());
    let mut session = registered();
    mode_task(&mut session);
    ack(&mut session, TaskStatus::Accepted);
    assert_eq!(mode(&session), "Alternate");
    ack(&mut session, TaskStatus::Failed);
    status(&mut session, 2, "Default");
    assert_eq!(mode(&session), "Default");
    assert!(session.findings().is_empty());
}
#[test]
fn old_mode_is_valid_before_acceptance_and_through_declared_settling_time() {
    let mut session = registered();
    mode_task(&mut session);
    status(&mut session, 0, "Default");
    ack(&mut session, TaskStatus::Accepted); // t=1, fixture settle_time=1s
    status(&mut session, 1, "Alternate");
    status(&mut session, 2, "Default");
    assert!(session.findings().is_empty());
    status(&mut session, 3, "Default");
    assert_eq!(
        session.findings().last().unwrap().rule_id,
        "session.status_report.mode_mismatch"
    );
}
#[test]
fn completion_ends_settling_and_duplicate_issue_does_not_reset_task() {
    let mut session = registered();
    mode_task(&mut session);
    ack(&mut session, TaskStatus::Accepted);
    issue(&mut session); // same ID must not replace the mode request/lifecycle
    assert_eq!(
        session.findings().last().unwrap().rule_id,
        "session.task.duplicate_id"
    );
    session.take_findings();
    ack(&mut session, TaskStatus::Completed); // completion t=1
    status(&mut session, 2, "Default");
    assert_eq!(
        session.findings().last().unwrap().rule_id,
        "session.status_report.mode_mismatch"
    );
}
#[test]
fn sequential_tasks_are_removed_without_disturbing_an_accepted_task() {
    let mut session = registered();
    issue(&mut session);
    ack(&mut session, TaskStatus::Accepted);
    for index in 0..256 {
        let id = format!("{index:026}");
        session.issue_task(&Task {
            task_id: Some(id.clone()),
            control: Some(1),
            ..Default::default()
        });
        session.on_bytes(&encode(envelope(
            PEER,
            LOCAL,
            1,
            Content::TaskAck(TaskAck {
                task_id: Some(id),
                task_status: Some(TaskStatus::Rejected as i32),
                ..Default::default()
            }),
        )));
    }
    assert!(session.findings().is_empty(), "{:?}", session.findings());
    let SessionState::Registered(contract) = session.state() else {
        panic!()
    };
    assert_eq!(contract.tasks.len(), 1);
    assert!(contract.outstanding_task_ids.contains(ID));
    ack(&mut session, TaskStatus::Completed);
    assert!(session.findings().is_empty());
}

#[test]
fn detection_contract_allows_old_mode_only_during_settling() {
    use sapient_conformance_core::bsi_flex_335_v2_0::{
        DetectionReport,
        detection_report::{DetectionReportClassification, LocationOneof},
    };
    let mut registration = valid_registration();
    registration.mode_definition[1].detection_definition[0].detection_class_definition[0]
        .class_definition[0]
        .r#type = Some("Vehicle".into());
    let mut session = DmmSession::new(LOCAL);
    session.on_bytes(&encode(envelope(
        PEER,
        LOCAL,
        0,
        Content::Registration(registration),
    )));
    mode_task(&mut session);
    ack(&mut session, TaskStatus::Accepted);
    for time in [2, 3] {
        session.on_bytes(&encode(envelope(
            PEER,
            LOCAL,
            time,
            Content::DetectionReport(DetectionReport {
                report_id: Some(ID.into()),
                object_id: Some(ID.into()),
                location_oneof: Some(LocationOneof::RangeBearing(
                    sapient_session::fixtures::detection_position(),
                )),
                classification: vec![DetectionReportClassification {
                    r#type: Some("Human".into()),
                    ..Default::default()
                }],
                ..Default::default()
            }),
        )));
        if time == 2 {
            assert!(session.findings().is_empty());
        }
    }
    assert_eq!(session.findings().len(), 1);
    assert_eq!(
        session.findings()[0].rule_id,
        "session.detection_report.undeclared_classification"
    );
}

#[test]
fn replacement_registration_clears_outstanding_tasks_and_mode_transition() {
    let mut session = registered();
    mode_task(&mut session);
    ack(&mut session, TaskStatus::Accepted);
    session.on_bytes(&encode(envelope(
        PEER,
        LOCAL,
        2,
        Content::Registration(valid_registration()),
    )));
    assert_eq!(mode(&session), "Default");
    let SessionState::Registered(contract) = session.state() else {
        panic!()
    };
    assert!(contract.tasks.is_empty());
    assert!(contract.mode_transition.is_none());
    ack(&mut session, TaskStatus::Completed);
    assert_eq!(
        session.findings().last().unwrap().rule_id,
        "session.task_ack.correlation_mismatch"
    );
}
