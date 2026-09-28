use crate::bsi_flex_335_v2_0::{Alert, alert::LocationOneof};
use crate::finding::ValidationOutcome;
use crate::validation::common::{
    validate_associated_detection, validate_associated_file, validate_location,
    validate_optional_enum, validate_range_bearing, validate_ulid, validate_unit_interval,
};

/// Function to validation a SAPIENT alert message
pub fn validate_alert(alert: Alert) -> ValidationOutcome {
    let mut validations = vec![validate_alert_id(alert.alert_id)];

    // `AlertType`, `AlertStatus`, and `DiscretePriority` are all optional
    // and have no reserved gaps (0-6, 0-5, and 0-3 respectively).
    validations.push(validate_optional_enum(
        alert.alert_type,
        6,
        "alert.alert_type.invalid",
        "Alert type is not a valid option in an alert message.",
    ));
    validations.push(validate_optional_enum(
        alert.status,
        5,
        "alert.status.invalid",
        "Alert status is not a valid option in an alert message.",
    ));
    validations.push(validate_optional_enum(
        alert.priority,
        3,
        "alert.priority.invalid",
        "Alert priority is not a valid option in an alert message.",
    ));

    if alert.region_id.is_some() {
        validations.push(validate_ulid(
            alert.region_id.as_deref(),
            "alert.region_id.invalid",
            "A valid ULID must be used for a region ID in an alert message.",
        ));
    }

    validations.push(validate_unit_interval(
        alert.ranking,
        "alert.ranking.invalid",
        "Alert ranking must be between 0.0 and 1.0.",
    ));
    validations.push(validate_unit_interval(
        alert.confidence,
        "alert.confidence.invalid",
        "Alert confidence must be between 0.0 and 1.0.",
    ));

    if let Some(location_oneof) = alert.location_oneof {
        validations.push(validate_location_oneof(location_oneof));
    }

    for associated_file in alert.associated_file {
        validations.push(validate_associated_file(
            associated_file,
            "alert.associated_file",
            "Associated file type must be specified in an alert message.",
            "Associated file URL must be specified in an alert message.",
        ));
    }

    for associated_detection in alert.associated_detection {
        validations.push(validate_associated_detection(
            associated_detection,
            "alert.associated_detection",
            "A valid UUID v4 must be used for a node ID in an alert associated detection.",
            "A valid ULID must be used for an object ID in an alert associated detection.",
        ));
    }

    for validation in validations {
        if !validation.passed {
            return validation;
        }
    }

    ValidationOutcome::pass()
}

/// Function to check a alert ID as specified in the SAPIENT version 7 ICD
fn validate_alert_id(alert_id: Option<String>) -> ValidationOutcome {
    validate_ulid(
        alert_id.as_deref(),
        "alert.alert_id.invalid",
        "A valid ULID must be used for an alert ID in an alert message.",
    )
}

fn validate_location_oneof(location_oneof: LocationOneof) -> ValidationOutcome {
    match location_oneof {
        LocationOneof::Location(location) => validate_location(location, "alert.location"),
        LocationOneof::RangeBearing(range_bearing) => {
            validate_range_bearing(range_bearing, "alert.range_bearing")
        }
    }
}

#[cfg(test)]
mod alert_validation_tests {
    use crate::{
        bsi_flex_335_v2_0::{Alert, AssociatedFile, alert::LocationOneof},
        finding::ValidationOutcome,
        validation::alert::{validate_alert, validate_alert_id},
    };

    /// Unit test to check that alerts are correctly validated
    #[test]
    fn test_alert_validation() {
        // valid alert
        let valid_alert = Alert {
            alert_id: Some("01H1VV3VN40RV97CDFSXJB44K9".to_string()),
            alert_type: None,
            status: None,
            description: None,
            region_id: None,
            priority: None,
            ranking: None,
            confidence: None,
            associated_file: vec![],
            associated_detection: vec![],
            additional_information: None,
            location_oneof: None,
        };
        assert_eq!(ValidationOutcome::pass(), validate_alert(valid_alert));

        // invalid alert
        let invalid_alert_ = Alert {
            alert_id: None,
            alert_type: None,
            status: None,
            description: None,
            region_id: None,
            priority: None,
            ranking: None,
            confidence: None,
            associated_file: vec![],
            associated_detection: vec![],
            additional_information: None,
            location_oneof: None,
        };
        assert_eq!(
            ValidationOutcome::fail(
                "alert.alert_id.invalid",
                "A valid ULID must be used for an alert ID in an alert message."
            ),
            validate_alert(invalid_alert_)
        );

        let invalid_file_alert = Alert {
            alert_id: Some("01H1VV3VN40RV97CDFSXJB44K9".to_string()),
            alert_type: None,
            status: None,
            description: None,
            region_id: None,
            priority: None,
            ranking: None,
            confidence: None,
            associated_file: vec![AssociatedFile {
                r#type: None,
                url: Some("https://example.test/file".to_string()),
            }],
            associated_detection: vec![],
            additional_information: None,
            location_oneof: None,
        };
        assert_eq!(
            ValidationOutcome::fail(
                "alert.associated_file.type.missing",
                "Associated file type must be specified in an alert message."
            ),
            validate_alert(invalid_file_alert)
        );

        let valid_location_alert = Alert {
            alert_id: Some("01H1VV3VN40RV97CDFSXJB44K9".to_string()),
            alert_type: None,
            status: None,
            description: None,
            region_id: Some("01H1VV3VN40RV97CDFSXJB44K9".to_string()),
            priority: None,
            ranking: None,
            confidence: None,
            associated_file: vec![],
            associated_detection: vec![],
            additional_information: None,
            location_oneof: Some(LocationOneof::Location(
                crate::bsi_flex_335_v2_0::Location {
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
        };
        assert_eq!(
            ValidationOutcome::pass(),
            validate_alert(valid_location_alert)
        );
    }

    /// Unit test to check that alert IDs are correctly validated
    #[test]
    fn test_alert_id_validation() {
        // valid alert ID
        let valid_alert_id = "01H1VV3VN40RV97CDFSXJB44K9".to_string();
        assert_eq!(
            ValidationOutcome::pass(),
            validate_alert_id(Some(valid_alert_id))
        );

        // invalid alert ID
        let invalid_alert_id = "".to_string();
        assert_eq!(
            ValidationOutcome::fail(
                "alert.alert_id.invalid",
                "A valid ULID must be used for an alert ID in an alert message."
            ),
            validate_alert_id(Some(invalid_alert_id))
        );
    }

    fn minimal_alert() -> Alert {
        Alert {
            alert_id: Some("01H1VV3VN40RV97CDFSXJB44K9".to_string()),
            alert_type: None,
            status: None,
            description: None,
            region_id: None,
            priority: None,
            ranking: None,
            confidence: None,
            associated_file: vec![],
            associated_detection: vec![],
            additional_information: None,
            location_oneof: None,
        }
    }

    #[test]
    fn test_alert_type_status_and_priority_out_of_range_are_findings() {
        assert_eq!(
            ValidationOutcome::fail(
                "alert.alert_type.invalid",
                "Alert type is not a valid option in an alert message."
            ),
            validate_alert(Alert {
                alert_type: Some(999),
                ..minimal_alert()
            })
        );
        assert_eq!(
            ValidationOutcome::fail(
                "alert.status.invalid",
                "Alert status is not a valid option in an alert message."
            ),
            validate_alert(Alert {
                status: Some(999),
                ..minimal_alert()
            })
        );
        assert_eq!(
            ValidationOutcome::fail(
                "alert.priority.invalid",
                "Alert priority is not a valid option in an alert message."
            ),
            validate_alert(Alert {
                priority: Some(999),
                ..minimal_alert()
            })
        );
    }
}
