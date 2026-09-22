//! End-to-end test of the real async wire path (`dmm::run` over an actual
//! duplex stream, with real framing) -- the one thing `tests/dmm_session.rs`'s
//! direct `on_bytes` calls don't exercise. Scenario coverage itself
//! (sequencing, timing, correlation, etc.) lives in that file, since the
//! state machine is pure and doesn't need a socket to test thoroughly.

use sapient_conformance_core::bsi_flex_335_v2_0::{
    AlertAck, RegistrationAck, sapient_message::Content,
};

mod common;
use common::{envelope, send_message, valid_registration};

const HARNESS_NODE_ID: &str = "550e8400-e29b-41d4-a716-446655440000";
const ASM_NODE_ID: &str = "550e8400-e29b-41d4-a716-446655440001";

#[tokio::test]
async fn full_session_over_a_real_duplex_stream() {
    let (mut asm_side, harness_side) = tokio::io::duplex(4096);

    let driver = tokio::spawn(sapient_session::dmm::run(HARNESS_NODE_ID, harness_side));

    // ASM connects and registers.
    send_message(
        &mut asm_side,
        &envelope(
            ASM_NODE_ID,
            HARNESS_NODE_ID,
            0,
            Content::Registration(valid_registration()),
        ),
    )
    .await;

    let ack = common::recv_message(&mut asm_side).await;
    match ack.content {
        Some(Content::RegistrationAck(RegistrationAck { acceptance, .. })) => {
            assert_eq!(acceptance, Some(true));
        }
        other => panic!("expected a RegistrationAck, got {other:?}"),
    }

    // ASM sends an Alert, expects an AlertAck.
    send_message(
        &mut asm_side,
        &envelope(
            ASM_NODE_ID,
            HARNESS_NODE_ID,
            1,
            Content::Alert(sapient_conformance_core::bsi_flex_335_v2_0::Alert {
                alert_id: Some("01H1VV3VN40RV97CDFSXJB44K9".to_string()),
                alert_type: Some(1),
                status: Some(1),
                description: None,
                location_oneof: Some(
                    sapient_conformance_core::bsi_flex_335_v2_0::alert::LocationOneof::Location(
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
                    ),
                ),
                region_id: None,
                priority: None,
                ranking: None,
                confidence: None,
                associated_file: vec![],
                associated_detection: vec![],
                additional_information: None,
            }),
        ),
    )
    .await;

    let alert_ack = common::recv_message(&mut asm_side).await;
    match alert_ack.content {
        Some(Content::AlertAck(AlertAck { alert_id, .. })) => {
            assert_eq!(alert_id.as_deref(), Some("01H1VV3VN40RV97CDFSXJB44K9"));
        }
        other => panic!("expected an AlertAck, got {other:?}"),
    }

    // Plain disconnect (the common teardown -- no GoodBye) ends the
    // session cleanly, and the driver returns with no findings recorded
    // for a fully conformant exchange.
    drop(asm_side);
    let findings = driver.await.unwrap().unwrap();
    assert!(
        findings.is_empty(),
        "conformant session recorded findings: {findings:?}"
    );
}
