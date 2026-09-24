//! Envelope findings are independent diagnostics, not a gate on payload checks.
use prost_types::Timestamp;
use sapient_conformance_core::{
    bsi_flex_335_v2_0::{RegistrationAck, SapientMessage, sapient_message::Content},
    validation::sapient_message::{validate_envelope, validate_sapient_message},
};

#[test]
fn retains_every_envelope_issue_and_the_payload_failure() {
    let message = SapientMessage {
        destination_id: Some("invalid".into()),
        content: Some(Content::RegistrationAck(RegistrationAck::default())),
        ..Default::default()
    };
    let outcome = validate_sapient_message(message);
    assert!(!outcome.passed);
    assert_eq!(
        outcome
            .findings
            .iter()
            .map(|f| f.rule_id.as_str())
            .collect::<Vec<_>>(),
        [
            "sapient_message.timestamp.missing",
            "sapient_message.node_id.invalid",
            "sapient_message.destination_id.invalid",
            "registration_ack.acceptance.missing",
        ]
    );
}

#[test]
fn missing_content_is_reported_alongside_missing_envelope_fields() {
    let outcome = validate_sapient_message(SapientMessage::default());
    assert_eq!(
        outcome
            .findings
            .iter()
            .map(|f| f.rule_id.as_str())
            .collect::<Vec<_>>(),
        [
            "sapient_message.timestamp.missing",
            "sapient_message.node_id.invalid",
            "sapient_message.content.missing",
        ]
    );
}

#[test]
fn envelope_check_does_not_validate_payload_and_destination_is_optional() {
    let message = SapientMessage {
        timestamp: Some(Timestamp {
            seconds: 1,
            nanos: 0,
        }),
        node_id: Some("550e8400-e29b-41d4-a716-446655440000".into()),
        ..Default::default()
    };
    assert!(validate_envelope(&message).passed);
    assert!(!validate_sapient_message(message).passed);
}
