//! End-to-end test of the real async wire path (`AsmConnection` over an
//! actual duplex stream, with real framing). Also demonstrates the thing
//! `AsmConnection` is specifically shaped for that the DMM driver isn't:
//! interleaving our own sends (`register`, `issue_alert`) with reads
//! (`poll_once`, auto-replying to an inbound `Task` with a `TaskAck`) on
//! the same connection, since the ASM role -- unlike the purely-reactive
//! DMM role -- actively initiates traffic on its own schedule.

use sapient_conformance_core::bsi_flex_335_v2_0::{
    Alert, AlertAck, RegistrationAck, Task, TaskAck, sapient_message::Content, task_ack::TaskStatus,
};
use sapient_session::asm::AsmConnection;
use tokio::io::split;

mod common;
use common::{envelope, recv_message, send_message, valid_registration};

const HARNESS_NODE_ID: &str = "550e8400-e29b-41d4-a716-446655440000";
const DMM_NODE_ID: &str = "550e8400-e29b-41d4-a716-446655440001";

#[tokio::test]
async fn registration_then_interleaved_alert_and_task_over_a_real_duplex_stream() {
    let (dmm_side, harness_side) = tokio::io::duplex(4096);
    let (mut dmm_read, mut dmm_write) = split(dmm_side);
    let (harness_read, harness_write) = split(harness_side);

    let mut connection = AsmConnection::new(HARNESS_NODE_ID, harness_read, harness_write);

    // Send our Registration, and act out the DMM side accepting it.
    connection.register(valid_registration()).await.unwrap();
    let registration_message = recv_message(&mut dmm_read).await;
    assert!(matches!(
        registration_message.content,
        Some(Content::Registration(_))
    ));
    send_message(
        &mut dmm_write,
        &envelope(
            DMM_NODE_ID,
            HARNESS_NODE_ID,
            0,
            Content::RegistrationAck(RegistrationAck {
                acceptance: Some(true),
                ack_response_reason: vec![],
            }),
        ),
    )
    .await;
    assert!(connection.poll_once().await.unwrap());
    assert!(matches!(
        connection.state(),
        sapient_session::AsmSessionState::Registered(_)
    ));

    // The DMM side issues a Task concurrently with us sending an Alert --
    // this is exactly the interleaving AsmConnection's split read/write
    // halves exist for.
    let dmm_task_send = tokio::spawn(async move {
        send_message(
            &mut dmm_write,
            &envelope(
                DMM_NODE_ID,
                HARNESS_NODE_ID,
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
            ),
        )
        .await;
        dmm_write
    });

    connection
        .issue_alert(Alert {
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
        })
        .await
        .unwrap();

    // Read the Alert on the DMM side.
    let alert_message = recv_message(&mut dmm_read).await;
    assert!(matches!(alert_message.content, Some(Content::Alert(_))));

    // Poll our connection: consumes the inbound Task, auto-replies with a
    // TaskAck.
    assert!(connection.poll_once().await.unwrap());

    let mut dmm_write = dmm_task_send.await.unwrap();
    let task_ack_message = recv_message(&mut dmm_read).await;
    match task_ack_message.content {
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

    // DMM acks our Alert.
    send_message(
        &mut dmm_write,
        &envelope(
            DMM_NODE_ID,
            HARNESS_NODE_ID,
            2,
            Content::AlertAck(AlertAck {
                alert_id: Some("01H1VV3VN40RV97CDFSXJB44K9".to_string()),
                reason: vec![],
                alert_ack_status: Some(1),
            }),
        ),
    )
    .await;
    assert!(connection.poll_once().await.unwrap());

    assert!(
        connection.findings().is_empty(),
        "conformant session recorded findings: {:?}",
        connection.findings()
    );

    // Plain disconnect.
    drop(dmm_write);
    drop(dmm_read);
    assert!(!connection.poll_once().await.unwrap());
}
