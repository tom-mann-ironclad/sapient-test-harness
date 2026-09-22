//! Runs our own `DmmConnection` against our own `AsmConnection` over a
//! real duplex stream, both driven by actual harness code -- not
//! hand-crafted test messages standing in for "what the other side would
//! send". `dmm_session.rs`/`asm_session.rs` each independently assert
//! their own role's behaviour against fixture messages *someone else*
//! (a test author) constructed by hand; if both files independently made
//! the same wrong assumption about the wire format or protocol
//! semantics, those tests would still pass while the two real
//! implementations quietly couldn't talk to each other. This test is the
//! check that doesn't have that blind spot: whatever `DmmConnection`
//! sends is exactly what `AsmConnection` has to parse, and vice versa.
//!
//! Exercises the full message set in one session: registration handshake
//! (using [`common::valid_registration`], the same fixture every other
//! test in this crate uses, so this isn't a specially-crafted
//! best-case), `StatusReport`, `DetectionReport`, a `mode_change` `Task`
//! paired with its `TaskAck`, an `Alert` paired with its `AlertAck`, and
//! a clean disconnect -- asserting zero findings on *both* sides for a
//! fully conformant exchange.

use sapient_conformance_core::bsi_flex_335_v2_0::{
    Alert, DetectionReport, StatusReport, Task,
    detection_report::{DetectionReportClassification, LocationOneof as DetectionLocationOneof},
    task::{Command, command::Command as TaskCommandKind},
};
use sapient_session::{AsmSessionState, SessionState, asm::AsmConnection, dmm::DmmConnection};
use tokio::io::split;

mod common;
use common::{ALTERNATE_MODE, DECLARED_CLASSIFICATION_TYPE, DEFAULT_MODE, valid_registration};

const DMM_HARNESS_NODE_ID: &str = "550e8400-e29b-41d4-a716-446655440000";
const ASM_HARNESS_NODE_ID: &str = "550e8400-e29b-41d4-a716-446655440001";

#[tokio::test]
async fn our_own_dmm_and_asm_interoperate() {
    let (dmm_side, asm_side) = tokio::io::duplex(16 * 1024);
    let (dmm_read, dmm_write) = split(dmm_side);
    let (asm_read, asm_write) = split(asm_side);

    let mut dmm = DmmConnection::new(DMM_HARNESS_NODE_ID, dmm_read, dmm_write);
    let mut asm = AsmConnection::new(ASM_HARNESS_NODE_ID, asm_read, asm_write);

    // Registration handshake.
    asm.register(valid_registration()).await.unwrap();
    assert!(dmm.poll_once().await.unwrap());
    assert!(asm.poll_once().await.unwrap());
    assert!(matches!(dmm.state(), SessionState::Registered(_)));
    assert!(matches!(asm.state(), AsmSessionState::Registered(_)));

    // StatusReport.
    asm.issue_status_report(StatusReport {
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
    })
    .await
    .unwrap();
    assert!(dmm.poll_once().await.unwrap());

    // DetectionReport, classifying the object as the type our shared
    // registration fixture actually declared for the active mode -- this
    // exercises the DMM's declared-vs-actual cross-check as a real
    // pass, not just an isolated unit test of the check itself.
    asm.issue_detection_report(DetectionReport {
        report_id: Some("01H1VV3VN40RV97CDFSXJB44KB".to_string()),
        object_id: Some("01H1VV3VN40RV97CDFSXJB44KC".to_string()),
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
    })
    .await
    .unwrap();
    assert!(dmm.poll_once().await.unwrap());

    // DMM issues a mode_change Task; ASM should accept it, update its
    // tracked active mode, and reply with a TaskAck the DMM correlates
    // against the Task it issued.
    dmm.issue_task(&Task {
        task_id: Some("01H1VV3VN40RV97CDFSXJB44KD".to_string()),
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
    })
    .await
    .unwrap();
    assert!(asm.poll_once().await.unwrap());
    assert!(dmm.poll_once().await.unwrap());
    if let AsmSessionState::Registered(contract) = asm.state() {
        assert_eq!(
            contract.active_mode.mode_name.as_deref(),
            Some(ALTERNATE_MODE)
        );
    } else {
        panic!("expected ASM to still be Registered");
    }

    // A follow-up StatusReport declaring the new mode should agree with
    // what the DMM now tracks as active, having observed the mode_change
    // Task -- no mode_mismatch finding.
    asm.issue_status_report(StatusReport {
        report_id: Some("01H1VV3VN40RV97CDFSXJB44KE".to_string()),
        system: Some(1),
        info: Some(1),
        active_task_id: None,
        mode: Some(ALTERNATE_MODE.to_string()),
        power: None,
        node_location: None,
        field_of_view: None,
        obscuration: vec![],
        status: vec![],
        coverage: vec![],
    })
    .await
    .unwrap();
    assert!(dmm.poll_once().await.unwrap());

    // ASM sends an Alert; DMM sends AlertAck, ASM correlates it.
    asm.issue_alert(Alert {
        alert_id: Some("01H1VV3VN40RV97CDFSXJB44KF".to_string()),
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
    })
    .await
    .unwrap();
    assert!(dmm.poll_once().await.unwrap());
    assert!(asm.poll_once().await.unwrap());

    assert!(
        dmm.findings().is_empty(),
        "DMM side recorded findings against our own ASM: {:?}",
        dmm.findings()
    );
    assert!(
        asm.findings().is_empty(),
        "ASM side recorded findings against our own DMM: {:?}",
        asm.findings()
    );

    // Plain disconnect, both sides agree the session just ends.
    drop(asm);
    assert!(!dmm.poll_once().await.unwrap());
}
