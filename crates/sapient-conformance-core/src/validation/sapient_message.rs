use crate::bsi_flex_335_v2_0::{SapientMessage, sapient_message::Content};
use crate::finding::ValidationOutcome;
use crate::validation::{
    alert::validate_alert,
    alert_ack::validate_alert_ack,
    common::{validate_timestamp, validate_uuid_v4},
    detection_report::validate_detection_report,
    error::validate_error,
    registration::validate_registration,
    registration_ack::validate_registration_ack,
    status_report::validate_status_report,
    task::validate_task,
    task_ack::validate_task_ack,
};

/// Validate the common envelope without traversing the payload.
///
/// All independent envelope issues are collected in field order. Sessions use
/// this borrowed check to record diagnostics, then continue their normal payload
/// and sequencing logic rather than treating a malformed header as a stop signal.
/// Content presence is checked separately by the message/session dispatcher.
pub fn validate_envelope(message: &SapientMessage) -> ValidationOutcome {
    let mut findings = Vec::new();
    findings.extend(
        validate_timestamp(
            message.timestamp,
            "sapient_message.timestamp.missing",
            "Timestamp must be specified in sapient message.",
            "sapient_message.timestamp.malformed",
            "Timestamp is malformed in sapient message.",
        )
        .findings,
    );
    findings.extend(
        validate_uuid_v4(
            message.node_id.as_deref(),
            "sapient_message.node_id.invalid",
            "A valid UUID v4 must be used for a node ID in sapient message.",
        )
        .findings,
    );
    if message.destination_id.is_some() {
        findings.extend(
            validate_uuid_v4(
                message.destination_id.as_deref(),
                "sapient_message.destination_id.invalid",
                "A valid UUID v4 must be used for a destination ID in sapient message.",
            )
            .findings,
        );
    }
    ValidationOutcome {
        passed: findings.is_empty(),
        findings,
    }
}

/// Validate the envelope and payload independently so envelope errors do not
/// hide payload diagnostics. Nested validators retain their own collection policy.
pub fn validate_sapient_message(message: SapientMessage) -> ValidationOutcome {
    let mut outcome = validate_envelope(&message);
    let payload = match message.content {
        Some(Content::Registration(registration)) => validate_registration(registration),
        Some(Content::RegistrationAck(ack)) => validate_registration_ack(ack),
        Some(Content::StatusReport(status_report)) => validate_status_report(status_report),
        Some(Content::DetectionReport(detection_report)) => {
            validate_detection_report(detection_report)
        }
        Some(Content::Task(task)) => validate_task(task),
        Some(Content::TaskAck(task_ack)) => validate_task_ack(task_ack),
        Some(Content::Alert(alert)) => validate_alert(alert),
        Some(Content::AlertAck(alert_ack)) => validate_alert_ack(alert_ack),
        Some(Content::Error(error)) => validate_error(error),
        None => ValidationOutcome::fail(
            "sapient_message.content.missing",
            "Content must be specified in sapient message.",
        ),
    };
    outcome.passed &= payload.passed;
    outcome.findings.extend(payload.findings);
    outcome
}

#[cfg(test)]
mod sapient_message_validation_tests {
    use prost_types::Timestamp;

    use crate::bsi_flex_335_v2_0::{
        Registration, SapientMessage,
        registration::{
            Capability, ConfigurationData, DetectionDefinition, Duration, LocationType,
            ModeDefinition, NodeDefinition, RegionDefinition, StatusDefinition, TaskDefinition,
            location_type::{CoordinatesOneof, DatumOneof},
        },
        sapient_message::Content,
    };
    use crate::finding::ValidationOutcome;

    use super::validate_sapient_message;

    fn valid_registration() -> Registration {
        let valid_node_definition = NodeDefinition {
            node_type: Some(1),
            node_sub_type: vec![],
        };
        let valid_duration = Duration {
            units: Some(1),
            value: Some(1.0),
        };
        let valid_status_definition = StatusDefinition {
            coverage_definition: None,
            field_of_view_definition: None,
            location_definition: None,
            obscuration_definition: None,
            status_report: vec![],
            status_interval: Some(valid_duration),
        };
        let valid_location_type = LocationType {
            coordinates_oneof: Some(CoordinatesOneof::RangeBearingUnits(1)),
            datum_oneof: Some(DatumOneof::RangeBearingDatum(1)),
            zone: None,
        };
        let detection_definition = DetectionDefinition {
            behaviour_definition: vec![],
            detection_performance: vec![],
            detection_class_definition: vec![],
            detection_report: vec![],
            geometric_error: None,
            velocity_type: None,
            location_type: Some(valid_location_type),
        };
        let valid_mode_definition = ModeDefinition {
            duration: None,
            maximum_latency: None,
            detection_definition: vec![detection_definition],
            mode_name: Some("Default".to_string()),
            mode_parameter: vec![],
            mode_description: None,
            mode_type: Some(1),
            scan_type: None,
            settle_time: Some(Duration {
                units: Some(1),
                value: Some(1.0),
            }),
            task: Some(TaskDefinition {
                command: vec![],
                concurrent_tasks: Some(1),
                region_definition: Some(RegionDefinition {
                    settle_time: None,
                    region_type: vec![1],
                    region_area: vec![LocationType {
                        coordinates_oneof: Some(CoordinatesOneof::LocationUnits(1)),
                        datum_oneof: Some(DatumOneof::LocationDatum(1)),
                        zone: None,
                    }],
                    class_filter_definition: vec![],
                    behaviour_filter_definition: vec![],
                }),
            }),
            tracking_type: None,
        };

        Registration {
            node_definition: vec![valid_node_definition],
            icd_version: Some("BSI Flex 335 v2.0".to_string()),
            name: None,
            short_name: None,
            capabilities: vec![Capability {
                category: Some("Radar".to_string()),
                r#type: Some("Range".to_string()),
                value: None,
                units: None,
            }],
            status_definition: Some(valid_status_definition),
            mode_definition: vec![valid_mode_definition],
            dependent_nodes: vec![],
            reporting_region: vec![],
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

    #[test]
    fn test_sapient_message_validation() {
        let valid_message = SapientMessage {
            timestamp: Some(Timestamp {
                seconds: 1,
                nanos: 0,
            }),
            node_id: Some("550e8400-e29b-41d4-a716-446655440000".to_string()),
            destination_id: Some("550e8400-e29b-41d4-a716-446655440001".to_string()),
            additional_information: None,
            content: Some(Content::Registration(valid_registration())),
        };
        assert_eq!(
            ValidationOutcome::pass(),
            validate_sapient_message(valid_message)
        );

        let missing_timestamp = SapientMessage {
            timestamp: None,
            node_id: Some("550e8400-e29b-41d4-a716-446655440000".to_string()),
            destination_id: None,
            additional_information: None,
            content: Some(Content::Registration(valid_registration())),
        };
        assert_eq!(
            ValidationOutcome::fail(
                "sapient_message.timestamp.missing",
                "Timestamp must be specified in sapient message."
            ),
            validate_sapient_message(missing_timestamp)
        );

        // The legacy `False/0001.Timestamp.Error` fixture's `"AAA"` can't
        // decode, so malformed timestamps are only reachable from binary
        // protobuf (out-of-range nanos or seconds), covered here instead.
        for timestamp in [
            Timestamp {
                seconds: 1,
                nanos: -1,
            },
            Timestamp {
                seconds: 1,
                nanos: 1_000_000_000,
            },
            Timestamp {
                seconds: i64::MAX,
                nanos: 0,
            },
        ] {
            let malformed_timestamp = SapientMessage {
                timestamp: Some(timestamp),
                node_id: Some("550e8400-e29b-41d4-a716-446655440000".to_string()),
                destination_id: None,
                additional_information: None,
                content: Some(Content::Registration(valid_registration())),
            };
            assert_eq!(
                ValidationOutcome::fail(
                    "sapient_message.timestamp.malformed",
                    "Timestamp is malformed in sapient message."
                ),
                validate_sapient_message(malformed_timestamp)
            );
        }

        let invalid_node_id = SapientMessage {
            timestamp: Some(Timestamp {
                seconds: 1,
                nanos: 0,
            }),
            node_id: Some("bad-node-id".to_string()),
            destination_id: None,
            additional_information: None,
            content: Some(Content::Registration(valid_registration())),
        };
        assert_eq!(
            ValidationOutcome::fail(
                "sapient_message.node_id.invalid",
                "A valid UUID v4 must be used for a node ID in sapient message."
            ),
            validate_sapient_message(invalid_node_id)
        );

        let missing_content = SapientMessage {
            timestamp: Some(Timestamp {
                seconds: 1,
                nanos: 0,
            }),
            node_id: Some("550e8400-e29b-41d4-a716-446655440000".to_string()),
            destination_id: None,
            additional_information: None,
            content: None,
        };
        assert_eq!(
            ValidationOutcome::fail(
                "sapient_message.content.missing",
                "Content must be specified in sapient message."
            ),
            validate_sapient_message(missing_content)
        );
    }
}
