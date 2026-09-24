//! Regression coverage for `send_message`, the raw (no session tracking)
//! core of the `send` subcommand: does sending a non-conformant message
//! still produce a warning-worthy `ValidationOutcome` without blocking the
//! send, in both directions, over a real (if in-process) connection?
//! Mirrors the `tokio::io::duplex`-based testing pattern used elsewhere in
//! this workspace (`dmm_asm_interop.rs`), rather than spawning real
//! subprocesses and scraping stdout.

use sapient_session::framing::FrameReader;
use std::time::Duration;

use prost_types::Timestamp;
use sapient_conformance_core::bsi_flex_335_v2_0::{
    Alert, RegistrationAck, SapientMessage, Task, sapient_message::Content,
};
use sapient_test_harness_cli::send::{ReplyOutcome, send_message};

const DMM_NODE_ID: &str = "550e8400-e29b-41d4-a716-446655440000";
const ASM_NODE_ID: &str = "550e8400-e29b-41d4-a716-446655440001";
const RESPONSE_TIMEOUT: Duration = Duration::from_millis(500);

fn envelope(node_id: &str, destination_id: &str, content: Content) -> SapientMessage {
    SapientMessage {
        timestamp: Some(Timestamp {
            seconds: 0,
            nanos: 0,
        }),
        node_id: Some(node_id.to_string()),
        destination_id: Some(destination_id.to_string()),
        content: Some(content),
        additional_information: None,
    }
}

/// A `Task` missing its mandatory `control` field -- invalid per
/// `task.control.invalid`.
fn invalid_task_from_dmm() -> SapientMessage {
    envelope(
        DMM_NODE_ID,
        ASM_NODE_ID,
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
    )
}

/// An `Alert` missing its mandatory `alert_id` -- invalid per
/// `alert.alert_id.invalid`.
fn invalid_alert_from_asm() -> SapientMessage {
    envelope(
        ASM_NODE_ID,
        DMM_NODE_ID,
        Content::Alert(Alert {
            alert_id: None,
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
        }),
    )
}

fn valid_registration_ack() -> SapientMessage {
    envelope(
        DMM_NODE_ID,
        ASM_NODE_ID,
        Content::RegistrationAck(RegistrationAck {
            acceptance: Some(true),
            ack_response_reason: vec![],
        }),
    )
}

#[tokio::test]
async fn sends_non_conformant_messages_from_both_sides_and_warns_without_blocking_the_send() {
    let (mut dmm_side, mut asm_side) = tokio::io::duplex(16 * 1024);

    let dmm_message = invalid_task_from_dmm();
    let asm_message = invalid_alert_from_asm();

    let mut dmm_reader = FrameReader::default();
    let mut asm_reader = FrameReader::default();
    let (dmm_result, asm_result) = tokio::join!(
        send_message(
            &mut dmm_side,
            &mut dmm_reader,
            &dmm_message,
            RESPONSE_TIMEOUT,
            RESPONSE_TIMEOUT
        ),
        send_message(
            &mut asm_side,
            &mut asm_reader,
            &asm_message,
            RESPONSE_TIMEOUT,
            RESPONSE_TIMEOUT
        ),
    );

    let dmm_outcome = dmm_result.expect("send over an in-memory duplex should not I/O-error");
    let asm_outcome = asm_result.expect("send over an in-memory duplex should not I/O-error");

    // Both sides' own outgoing messages should be flagged invalid --
    // that's the whole point of "warn but still send".
    assert!(!dmm_outcome.validation.passed);
    assert!(
        dmm_outcome
            .validation
            .findings
            .iter()
            .any(|f| f.rule_id == "task.control.invalid"),
        "expected a task.control.invalid finding, got {:?}",
        dmm_outcome.validation.findings
    );

    assert!(!asm_outcome.validation.passed);
    assert!(
        asm_outcome
            .validation
            .findings
            .iter()
            .any(|f| f.rule_id == "alert.alert_id.invalid"),
        "expected an alert.alert_id.invalid finding, got {:?}",
        asm_outcome.validation.findings
    );

    // The send happened regardless: each side received the other's
    // message as its own "reply".
    match dmm_outcome.reply {
        ReplyOutcome::Reply(reply) => {
            assert!(matches!(reply.content, Some(Content::Alert(_))))
        }
        other => panic!("expected the DMM side to receive the ASM's Alert, got {other:?}"),
    }
    match asm_outcome.reply {
        ReplyOutcome::Reply(reply) => {
            assert!(matches!(reply.content, Some(Content::Task(_))))
        }
        other => panic!("expected the ASM side to receive the DMM's Task, got {other:?}"),
    }
}

#[tokio::test]
async fn sending_a_conformant_message_produces_no_warning() {
    // Only the validation outcome matters here, not the reply -- keep the
    // peer half alive (so the write itself can land) but never reply, and
    // use a short timeout so the test doesn't hang waiting on it.
    let (mut a, _b) = tokio::io::duplex(16 * 1024);

    let mut reader = FrameReader::default();
    let outcome = send_message(
        &mut a,
        &mut reader,
        &valid_registration_ack(),
        Duration::from_millis(50),
        RESPONSE_TIMEOUT,
    )
    .await
    .expect("send over an in-memory duplex should not I/O-error");

    assert!(outcome.validation.passed);
    assert!(outcome.validation.findings.is_empty());
}

#[tokio::test]
async fn no_reply_within_the_timeout_is_reported_not_an_error() {
    let (mut a, _b) = tokio::io::duplex(16 * 1024);

    let mut reader = FrameReader::default();
    let outcome = send_message(
        &mut a,
        &mut reader,
        &valid_registration_ack(),
        Duration::from_millis(50),
        RESPONSE_TIMEOUT,
    )
    .await
    .expect("send over an in-memory duplex should not I/O-error");

    assert!(matches!(outcome.reply, ReplyOutcome::TimedOut));
}

#[tokio::test]
async fn peer_disconnecting_before_replying_is_reported_not_an_error() {
    // `DuplexStream`'s write side errors immediately once the peer half is
    // gone, so `b` has to survive long enough for `send_message`'s write
    // to land before disconnecting -- otherwise this tests a write
    // failure, not a peer hanging up after receiving something.
    let (mut a, b) = tokio::io::duplex(16 * 1024);
    let message = valid_registration_ack();

    let mut reader = FrameReader::default();
    let (result, _) = tokio::join!(
        send_message(
            &mut a,
            &mut reader,
            &message,
            RESPONSE_TIMEOUT,
            RESPONSE_TIMEOUT
        ),
        async {
            tokio::time::sleep(Duration::from_millis(10)).await;
            drop(b);
        },
    );
    let outcome = result.expect("send over an in-memory duplex should not I/O-error");

    assert!(matches!(outcome.reply, ReplyOutcome::Disconnected));
}

#[tokio::test]
async fn successive_sends_resume_partial_late_replies() {
    use prost::Message;
    use sapient_session::framing::{read_frame, write_frame};
    use tokio::io::AsyncWriteExt;

    let expected = valid_registration_ack();
    let raw = expected.encode_to_vec();
    let mut wire = (raw.len() as u32).to_le_bytes().to_vec();
    wire.extend_from_slice(&raw);
    for split in [2, 6] {
        let (mut stream, mut peer) = tokio::io::duplex(16384);
        let mut reader = FrameReader::default();
        peer.write_all(&wire[..split]).await.unwrap();
        let first = send_message(
            &mut stream,
            &mut reader,
            &expected,
            Duration::from_millis(10),
            RESPONSE_TIMEOUT,
        )
        .await
        .unwrap();
        assert!(matches!(first.reply, ReplyOutcome::TimedOut));
        assert_eq!(read_frame(&mut peer).await.unwrap(), Some(raw.clone()));
        peer.write_all(&wire[split..]).await.unwrap();
        write_frame(&mut peer, &raw).await.unwrap();
        for _ in 0..2 {
            let next = send_message(
                &mut stream,
                &mut reader,
                &expected,
                RESPONSE_TIMEOUT,
                RESPONSE_TIMEOUT,
            )
            .await
            .unwrap();
            match next.reply {
                ReplyOutcome::Reply(reply) => assert_eq!(*reply, expected),
                other => panic!("expected intact late reply, got {other:?}"),
            }
            assert_eq!(read_frame(&mut peer).await.unwrap(), Some(raw.clone()));
        }
    }
}

#[tokio::test(start_paused = true)]
async fn blocked_raw_send_has_a_separate_write_timeout() {
    let (mut stream, _peer) = tokio::io::duplex(1);
    let mut reader = FrameReader::default();
    let start = tokio::time::Instant::now();
    let error = send_message(
        &mut stream,
        &mut reader,
        &valid_registration_ack(),
        Duration::from_secs(60),
        Duration::from_secs(2),
    )
    .await
    .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
    assert_eq!(tokio::time::Instant::now() - start, Duration::from_secs(2));
}
