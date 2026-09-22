//! Shared fixture builders and wire helpers for `sapient-session`'s
//! integration tests. Previously each of `dmm_session.rs`/`asm_session.rs`/
//! `dmm_driver.rs`/`asm_driver.rs` hand-rolled its own near-identical copy
//! of `Registration`/`ModeDefinition`/envelope/encode-decode builders;
//! consolidated here so there's one definition of what a "valid test
//! registration" looks like, used identically by every test regardless of
//! which role it's exercising -- including `dmm_asm_interop.rs`, where
//! both sides genuinely must agree on it since it's the same message sent
//! once over a real connection, not two independently hand-built copies
//! that happen to agree by construction.
//!
//! `#[allow(dead_code)]` throughout: each test binary only uses a subset
//! of these, and Rust warns per-binary about the rest as unused.

#![allow(dead_code)]

use prost::Message;
use prost_types::Timestamp;
use sapient_conformance_core::bsi_flex_335_v2_0::{
    Registration, SapientMessage,
    registration::{
        Capability, ClassDefinition, ConfigurationData, DetectionClassDefinition,
        DetectionDefinition, Duration, LocationType, ModeDefinition, ModeType, NodeDefinition,
        RegionDefinition, StatusDefinition, TaskDefinition, TimeUnits,
        location_type::{CoordinatesOneof, DatumOneof},
    },
    sapient_message::Content,
};
use sapient_session::framing::{read_frame, write_frame};

pub const DEFAULT_MODE: &str = "Default";
pub const ALTERNATE_MODE: &str = "Alternate";
pub const DECLARED_CLASSIFICATION_TYPE: &str = "Human";
pub const STATUS_INTERVAL_SECONDS: f32 = 5.0;

pub fn valid_location_type() -> LocationType {
    LocationType {
        coordinates_oneof: Some(CoordinatesOneof::RangeBearingUnits(1)),
        datum_oneof: Some(DatumOneof::RangeBearingDatum(1)),
        zone: None,
    }
}

pub fn duration(units: TimeUnits, value: f32) -> Duration {
    Duration {
        units: Some(units as i32),
        value: Some(value),
    }
}

pub fn mode(name: &str, mode_type: ModeType) -> ModeDefinition {
    ModeDefinition {
        mode_name: Some(name.to_string()),
        mode_type: Some(mode_type as i32),
        mode_description: None,
        settle_time: Some(duration(TimeUnits::Seconds, 1.0)),
        maximum_latency: None,
        scan_type: None,
        tracking_type: None,
        duration: None,
        mode_parameter: vec![],
        detection_definition: vec![DetectionDefinition {
            behaviour_definition: vec![],
            detection_performance: vec![],
            detection_class_definition: vec![DetectionClassDefinition {
                confidence_definition: None,
                class_performance: vec![],
                class_definition: vec![ClassDefinition {
                    r#type: Some(DECLARED_CLASSIFICATION_TYPE.to_string()),
                    units: None,
                    sub_class: vec![],
                }],
                taxonomy_dock_definition: vec![],
            }],
            detection_report: vec![],
            geometric_error: None,
            velocity_type: None,
            location_type: Some(valid_location_type()),
        }],
        task: Some(TaskDefinition {
            command: vec![],
            concurrent_tasks: Some(1),
            region_definition: Some(RegionDefinition {
                settle_time: None,
                region_type: vec![1],
                region_area: vec![valid_location_type()],
                class_filter_definition: vec![],
                behaviour_filter_definition: vec![],
            }),
        }),
    }
}

/// A registration declaring two modes (one `MODE_TYPE_DEFAULT`, one
/// `MODE_TYPE_PERMANENT` reachable via a `mode_change` task -- named
/// [`DEFAULT_MODE`]/[`ALTERNATE_MODE`]) and a `status_interval` of
/// [`STATUS_INTERVAL_SECONDS`].
pub fn valid_registration() -> Registration {
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
            status_interval: Some(duration(TimeUnits::Seconds, STATUS_INTERVAL_SECONDS)),
            location_definition: None,
            coverage_definition: None,
            obscuration_definition: None,
            status_report: vec![],
            field_of_view_definition: None,
        }),
        mode_definition: vec![
            mode(DEFAULT_MODE, ModeType::Default),
            mode(ALTERNATE_MODE, ModeType::Permanent),
        ],
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

pub fn timestamp(seconds: i64) -> Timestamp {
    Timestamp { seconds, nanos: 0 }
}

/// Wraps `content` in a `SapientMessage` envelope, timestamped at
/// `timestamp_seconds`, from `node_id` addressed to `destination_id`.
pub fn envelope(
    node_id: &str,
    destination_id: &str,
    timestamp_seconds: i64,
    content: Content,
) -> SapientMessage {
    SapientMessage {
        timestamp: Some(timestamp(timestamp_seconds)),
        node_id: Some(node_id.to_string()),
        destination_id: Some(destination_id.to_string()),
        content: Some(content),
        additional_information: None,
    }
}

pub fn encode(message: SapientMessage) -> Vec<u8> {
    message.encode_to_vec()
}

pub fn decode(bytes: &[u8]) -> SapientMessage {
    SapientMessage::decode(bytes).expect("test-constructed messages should always decode")
}

/// Encodes and writes `message` as one length-prefixed frame.
pub async fn send_message<S: tokio::io::AsyncWrite + Unpin>(
    stream: &mut S,
    message: &SapientMessage,
) {
    write_frame(stream, &message.encode_to_vec())
        .await
        .expect("test stream write should not fail");
}

/// Reads and decodes one length-prefixed frame. Panics if the stream ends
/// first -- every test using this expects a specific message to arrive.
pub async fn recv_message<S: tokio::io::AsyncRead + Unpin>(stream: &mut S) -> SapientMessage {
    let payload = read_frame(stream)
        .await
        .expect("test stream read should not fail")
        .expect("stream ended before the expected message arrived");
    decode(&payload)
}
