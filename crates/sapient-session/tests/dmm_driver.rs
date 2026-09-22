//! End-to-end test of the real async wire path (`dmm::run` over an actual
//! duplex stream, with real framing) -- the one thing `tests/dmm_session.rs`'s
//! direct `on_bytes` calls don't exercise. Scenario coverage itself
//! (sequencing, timing, correlation, etc.) lives in that file, since the
//! state machine is pure and doesn't need a socket to test thoroughly.

use prost::Message;
use prost_types::Timestamp;
use sapient_conformance_core::bsi_flex_335_v2_0::{
    AlertAck, Registration, RegistrationAck, SapientMessage,
    registration::{
        Capability, ConfigurationData, Duration, LocationType, ModeDefinition, ModeType,
        NodeDefinition, RegionDefinition, StatusDefinition, TaskDefinition, TimeUnits,
        location_type::{CoordinatesOneof, DatumOneof},
    },
    sapient_message::Content,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const HARNESS_NODE_ID: &str = "550e8400-e29b-41d4-a716-446655440000";
const ASM_NODE_ID: &str = "550e8400-e29b-41d4-a716-446655440001";

fn valid_registration() -> Registration {
    let location_type = LocationType {
        coordinates_oneof: Some(CoordinatesOneof::RangeBearingUnits(1)),
        datum_oneof: Some(DatumOneof::RangeBearingDatum(1)),
        zone: None,
    };
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
            status_interval: Some(Duration {
                units: Some(TimeUnits::Seconds as i32),
                value: Some(5.0),
            }),
            location_definition: None,
            coverage_definition: None,
            obscuration_definition: None,
            status_report: vec![],
            field_of_view_definition: None,
        }),
        mode_definition: vec![ModeDefinition {
            mode_name: Some("Default".to_string()),
            mode_type: Some(ModeType::Default as i32),
            mode_description: None,
            settle_time: Some(Duration {
                units: Some(TimeUnits::Seconds as i32),
                value: Some(1.0),
            }),
            maximum_latency: None,
            scan_type: None,
            tracking_type: None,
            duration: None,
            mode_parameter: vec![],
            detection_definition: vec![],
            task: Some(TaskDefinition {
                command: vec![],
                concurrent_tasks: Some(1),
                region_definition: Some(RegionDefinition {
                    settle_time: None,
                    region_type: vec![1],
                    region_area: vec![location_type],
                    class_filter_definition: vec![],
                    behaviour_filter_definition: vec![],
                }),
            }),
        }],
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

async fn write_frame<S: tokio::io::AsyncWrite + Unpin>(stream: &mut S, message: &SapientMessage) {
    let payload = message.encode_to_vec();
    stream
        .write_all(&(payload.len() as u32).to_le_bytes())
        .await
        .unwrap();
    stream.write_all(&payload).await.unwrap();
}

async fn read_frame<S: tokio::io::AsyncRead + Unpin>(stream: &mut S) -> SapientMessage {
    let mut length_buf = [0_u8; 4];
    stream.read_exact(&mut length_buf).await.unwrap();
    let length = u32::from_le_bytes(length_buf) as usize;
    let mut payload = vec![0_u8; length];
    stream.read_exact(&mut payload).await.unwrap();
    SapientMessage::decode(payload.as_slice()).unwrap()
}

#[tokio::test]
async fn full_session_over_a_real_duplex_stream() {
    let (mut asm_side, harness_side) = tokio::io::duplex(4096);

    let driver = tokio::spawn(sapient_session::dmm::run(HARNESS_NODE_ID, harness_side));

    // ASM connects and registers.
    write_frame(
        &mut asm_side,
        &SapientMessage {
            timestamp: Some(Timestamp {
                seconds: 0,
                nanos: 0,
            }),
            node_id: Some(ASM_NODE_ID.to_string()),
            destination_id: Some(HARNESS_NODE_ID.to_string()),
            content: Some(Content::Registration(valid_registration())),
            additional_information: None,
        },
    )
    .await;

    let ack = read_frame(&mut asm_side).await;
    match ack.content {
        Some(Content::RegistrationAck(RegistrationAck { acceptance, .. })) => {
            assert_eq!(acceptance, Some(true));
        }
        other => panic!("expected a RegistrationAck, got {other:?}"),
    }

    // ASM sends an Alert, expects an AlertAck.
    write_frame(
        &mut asm_side,
        &SapientMessage {
            timestamp: Some(Timestamp {
                seconds: 1,
                nanos: 0,
            }),
            node_id: Some(ASM_NODE_ID.to_string()),
            destination_id: Some(HARNESS_NODE_ID.to_string()),
            content: Some(Content::Alert(
                sapient_conformance_core::bsi_flex_335_v2_0::Alert {
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
                },
            )),
            additional_information: None,
        },
    )
    .await;

    let alert_ack = read_frame(&mut asm_side).await;
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
