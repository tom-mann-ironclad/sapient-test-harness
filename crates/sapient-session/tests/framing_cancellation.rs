//! Cancellation must preserve byte offsets and process each inbound frame once.
use std::{io::ErrorKind, time::Duration};

use sapient_conformance_core::bsi_flex_335_v2_0::{
    RegistrationAck, Task, sapient_message::Content,
};
use sapient_session::{
    AsmEvent, DmmEvent,
    asm::AsmConnection,
    dmm::DmmConnection,
    framing::{FrameReader, read_frame, write_frame},
};
use tokio::{io::AsyncWriteExt, time::timeout};

mod common;
use common::{encode, envelope, recv_message, valid_registration};

const LOCAL: &str = "550e8400-e29b-41d4-a716-446655440000";
const PEER: &str = "550e8400-e29b-41d4-a716-446655440001";
const WAIT: Duration = Duration::from_millis(10);

#[tokio::test]
async fn every_header_and_payload_boundary_survives_repeated_cancellation() {
    let wire = [3, 0, 0, 0, 10, 20, 30];
    for split in 1..wire.len() {
        let (mut stream, mut peer) = tokio::io::duplex(64);
        let mut reader = FrameReader::default();
        peer.write_all(&wire[..split]).await.unwrap();
        for _ in 0..2 {
            assert!(timeout(WAIT, reader.read(&mut stream)).await.is_err());
        }
        peer.write_all(&wire[split..]).await.unwrap();
        write_frame(&mut peer, &[]).await.unwrap();
        write_frame(&mut peer, &[42]).await.unwrap();
        peer.shutdown().await.unwrap();
        assert_eq!(
            reader.read(&mut stream).await.unwrap(),
            Some(vec![10, 20, 30])
        );
        assert_eq!(reader.read(&mut stream).await.unwrap(), Some(vec![]));
        assert_eq!(reader.read(&mut stream).await.unwrap(), Some(vec![42]));
        assert_eq!(reader.read(&mut stream).await.unwrap(), None);
    }
}

#[tokio::test]
async fn partial_headers_and_payloads_are_not_clean_disconnects() {
    let wire = [3, 0, 0, 0, 10, 20, 30];
    for end in 1..wire.len() {
        let (mut stream, mut peer) = tokio::io::duplex(64);
        peer.write_all(&wire[..end]).await.unwrap();
        peer.shutdown().await.unwrap();
        assert_eq!(
            read_frame(&mut stream).await.unwrap_err().kind(),
            ErrorKind::UnexpectedEof
        );
    }
}

#[tokio::test]
async fn dmm_resumes_registration_and_blocked_ack_before_a_proactive_task() {
    // Independent directions allow incoming traffic while a reply is blocked.
    let (reader, mut inbound) = tokio::io::duplex(16384);
    let (writer, mut outbound) = tokio::io::duplex(2);
    let mut connection = DmmConnection::new(LOCAL, reader, writer);
    let raw = encode(envelope(
        PEER,
        LOCAL,
        0,
        Content::Registration(valid_registration()),
    ));
    let header = (raw.len() as u32).to_le_bytes();
    inbound.write_all(&header[..2]).await.unwrap();
    assert!(timeout(WAIT, connection.poll_once()).await.is_err());
    inbound.write_all(&header[2..]).await.unwrap();
    inbound.write_all(&raw[..10]).await.unwrap();
    assert!(timeout(WAIT, connection.poll_once()).await.is_err());
    inbound.write_all(&raw[10..]).await.unwrap();
    // Session processing completes; only the automatic reply is now blocked.
    assert!(timeout(WAIT, connection.poll_once()).await.is_err());
    let task = Task {
        task_id: Some("01H1VV3VN40RV97CDFSXJB44KA".into()),
        control: Some(1),
        ..Default::default()
    };
    let (sent, ()) = tokio::join!(connection.issue_task(&task), async {
        assert!(matches!(
            recv_message(&mut outbound).await.content,
            Some(Content::RegistrationAck(_))
        ));
        assert!(matches!(
            recv_message(&mut outbound).await.content,
            Some(Content::Task(_))
        ));
    });
    sent.unwrap();
    // The completed poll must be delivered before another inbound read.
    assert!(
        timeout(WAIT, connection.poll_once())
            .await
            .unwrap()
            .unwrap()
    );
    assert!(matches!(
        connection.take_event(),
        Some(DmmEvent::RegistrationAccepted)
    ));
    assert!(connection.findings().is_empty());
    assert!(timeout(WAIT, connection.poll_once()).await.is_err());
}

#[tokio::test]
async fn asm_resumes_partial_ack_and_blocked_error_reply_without_reprocessing() {
    let (reader, mut inbound) = tokio::io::duplex(16384);
    let (writer, mut outbound) = tokio::io::duplex(2);
    let mut connection = AsmConnection::new(LOCAL, reader, writer);
    let (sent, _) = tokio::join!(
        connection.register(valid_registration()),
        recv_message(&mut outbound)
    );
    sent.unwrap();
    let raw = encode(envelope(
        PEER,
        LOCAL,
        0,
        Content::RegistrationAck(RegistrationAck {
            acceptance: Some(true),
            ack_response_reason: vec![],
        }),
    ));
    inbound
        .write_all(&(raw.len() as u32).to_le_bytes())
        .await
        .unwrap();
    inbound.write_all(&raw[..10]).await.unwrap();
    assert!(timeout(WAIT, connection.poll_once()).await.is_err());
    inbound.write_all(&raw[10..]).await.unwrap();
    assert!(connection.poll_once().await.unwrap());
    assert!(matches!(
        connection.take_event(),
        Some(AsmEvent::RegistrationAccepted)
    ));
    // A malformed packet now produces an Error containing the exact raw bytes.
    write_frame(&mut inbound, &[255]).await.unwrap();
    for _ in 0..2 {
        assert!(timeout(WAIT, connection.poll_once()).await.is_err());
    }
    let (polled, reply) = tokio::join!(connection.poll_once(), recv_message(&mut outbound));
    assert!(polled.unwrap());
    match reply.content {
        Some(Content::Error(error)) => assert_eq!(error.packet, Some(vec![255])),
        other => panic!("expected Error, got {other:?}"),
    }
    assert_eq!(connection.findings().len(), 1);
    assert!(timeout(WAIT, connection.poll_once()).await.is_err());
}
