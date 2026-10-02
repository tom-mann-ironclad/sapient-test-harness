use crate::bsi_flex_335_v2_0::{
    DetectionReport, EnuVelocity, Location, RangeBearing,
    detection_report::{
        Behaviour, DerivedDetection, DetectionReportClassification, LocationOneof,
        PredictedLocation, Signal, SubClass, TrackObjectInfo, VelocityOneof,
        predicted_location::PredictedLocationOneof,
    },
};
use crate::finding::ValidationOutcome;
use crate::validation::common::{
    validate_associated_detection, validate_associated_file, validate_finite,
    validate_finite_non_negative, validate_location as validate_common_location,
    validate_range_bearing as validate_common_range_bearing, validate_timestamp, validate_ulid,
    validate_unit_interval,
};

/// Function to validation a SAPIENT detection report message
pub fn validate_detection_report(detection_report: DetectionReport) -> ValidationOutcome {
    let mut validations = vec![];

    // Check report ID
    validations.push(validate_report_id(detection_report.report_id));

    // Check object ID
    validations.push(validate_object_id(detection_report.object_id));

    if detection_report.task_id.is_some() {
        validations.push(validate_ulid(
            detection_report.task_id.as_deref(),
            "detection_report.task_id.invalid",
            "A valid ULID must be used for a task ID in a detection report.",
        ));
    }

    validations.push(validate_unit_interval(
        detection_report.detection_confidence,
        "detection_report.detection_confidence.invalid",
        "Detection confidence must be between 0.0 and 1.0 in detection report.",
    ));

    // Check location
    if detection_report.location_oneof.is_none() {
        return ValidationOutcome::fail(
            "detection_report.location.missing",
            "Location or range-bearing must be specified in detection report.",
        );
    }
    validations.push(validate_location_oneof(
        detection_report.location_oneof.unwrap(),
        "detection_report.location",
    ));

    if let Some(prediction_location) = detection_report.prediction_location {
        validations.push(validate_predicted_location(
            prediction_location,
            "detection_report.prediction_location",
        ));
    }

    for track_info in detection_report.track_info {
        validations.push(validate_track_object_info(
            track_info,
            "detection_report.track_info",
        ));
    }

    for object_info in detection_report.object_info {
        validations.push(validate_track_object_info(
            object_info,
            "detection_report.object_info",
        ));
    }

    for classification in detection_report.classification {
        validations.push(validate_classification(classification));
    }

    for behaviour in detection_report.behaviour {
        validations.push(validate_behaviour(behaviour));
    }

    for signal in detection_report.signal {
        validations.push(validate_signal(signal));
    }

    for associated_file in detection_report.associated_file {
        validations.push(validate_associated_file(
            associated_file,
            "detection_report.associated_file",
            "Associated file type must be specified in a detection report.",
            "Associated file URL must be specified in a detection report.",
        ));
    }

    for associated_detection in detection_report.associated_detection {
        validations.push(validate_associated_detection(
            associated_detection,
            "detection_report.associated_detection",
            "A valid UUID v4 must be used for a node ID in a detection report associated detection.",
            "A valid ULID must be used for an object ID in a detection report associated detection.",
        ));
    }

    for derived_detection in detection_report.derived_detection {
        validations.push(validate_derived_detection(derived_detection));
    }

    if let Some(velocity_oneof) = detection_report.velocity_oneof {
        validations.push(validate_velocity_oneof(velocity_oneof));
    }

    for validation in validations {
        if !validation.passed {
            return validation;
        }
    }

    ValidationOutcome::pass()
}

/// Function to check a report ID as specified in the SAPIENT version 7 ICD
fn validate_report_id(report_id: Option<String>) -> ValidationOutcome {
    validate_ulid(
        report_id.as_deref(),
        "detection_report.report_id.invalid",
        "A valid ULID must be used for a report ID in a detection report.",
    )
}

/// Function to check a object ID as specified in the SAPIENT version 7 ICD
fn validate_object_id(object_id: Option<String>) -> ValidationOutcome {
    validate_ulid(
        object_id.as_deref(),
        "detection_report.object_id.invalid",
        "A valid ULID must be used for an object ID in a detection report.",
    )
}

/// Function to check a location/rangebearing as specified in the SAPIENT version 7 ICD
fn validate_location_oneof(
    location_oneof: LocationOneof,
    rule_id_prefix: &str,
) -> ValidationOutcome {
    match location_oneof {
        LocationOneof::Location(location) => validate_location(location, rule_id_prefix),
        LocationOneof::RangeBearing(range_bearing) => {
            validate_range_bearing(range_bearing, rule_id_prefix)
        }
    }
}

/// Function to check a location as specified in the SAPIENT version 7 ICD
fn validate_location(location: Location, rule_id_prefix: &str) -> ValidationOutcome {
    validate_common_location(location, rule_id_prefix)
}

/// Function to check a range bearing as specified in the SAPIENT version 7 ICD
fn validate_range_bearing(range_bearing: RangeBearing, rule_id_prefix: &str) -> ValidationOutcome {
    validate_common_range_bearing(range_bearing, rule_id_prefix)
}

fn validate_predicted_location(
    predicted_location: PredictedLocation,
    rule_id_prefix: &str,
) -> ValidationOutcome {
    match predicted_location.predicted_location_oneof {
        Some(PredictedLocationOneof::Location(location)) => {
            validate_location(location, rule_id_prefix)
        }
        Some(PredictedLocationOneof::RangeBearing(range_bearing)) => {
            validate_range_bearing(range_bearing, rule_id_prefix)
        }
        // Mirrors `validate_detection_report`'s own top-level
        // `location_oneof` check: a `PredictedLocation` present at all but
        // without an actual location or range-bearing in it says nothing
        // usable -- every real fixture that includes a `predictionLocation`
        // always populates this oneof.
        None => ValidationOutcome::fail(
            format!("{rule_id_prefix}.missing"),
            "Predicted location or range-bearing must be specified in detection report.",
        ),
    }
}

fn validate_track_object_info(
    track_object_info: TrackObjectInfo,
    rule_id_prefix: &str,
) -> ValidationOutcome {
    match track_object_info.r#type.as_deref() {
        Some("") | None => {
            return ValidationOutcome::fail(
                format!("{rule_id_prefix}.type.missing"),
                "Track object info type must be specified in detection report.",
            );
        }
        Some(_) => {}
    }

    match track_object_info.value.as_deref() {
        Some("") | None => {
            return ValidationOutcome::fail(
                format!("{rule_id_prefix}.value.missing"),
                "Track object info value must be specified in detection report.",
            );
        }
        Some(_) => {}
    }

    ValidationOutcome::pass()
}

fn validate_classification(classification: DetectionReportClassification) -> ValidationOutcome {
    match classification.r#type.as_deref() {
        Some("") | None => {
            return ValidationOutcome::fail(
                "detection_report.classification.type.missing",
                "Classification type must be specified in detection report.",
            );
        }
        Some(_) => {}
    }

    for sub_class in classification.sub_class {
        let validation = validate_sub_class(sub_class, "detection_report.classification.sub_class");
        if !validation.passed {
            return validation;
        }
    }

    validate_unit_interval(
        classification.confidence,
        "detection_report.classification.confidence.invalid",
        "Classification confidence must be between 0.0 and 1.0 in detection report.",
    )
}

fn validate_sub_class(sub_class: SubClass, rule_id_prefix: &str) -> ValidationOutcome {
    match sub_class.r#type.as_deref() {
        Some("") | None => {
            return ValidationOutcome::fail(
                format!("{rule_id_prefix}.type.missing"),
                "Classification sub class type must be specified in detection report.",
            );
        }
        Some(_) => {}
    }

    if sub_class.level.is_none() {
        return ValidationOutcome::fail(
            format!("{rule_id_prefix}.level.missing"),
            "Classification sub class level must be specified in detection report.",
        );
    }

    for nested_sub_class in sub_class.sub_class {
        let validation = validate_sub_class(nested_sub_class, rule_id_prefix);
        if !validation.passed {
            return validation;
        }
    }

    ValidationOutcome::pass()
}

fn validate_behaviour(behaviour: Behaviour) -> ValidationOutcome {
    match behaviour.r#type.as_deref() {
        Some("") | None => ValidationOutcome::fail(
            "detection_report.behaviour.type.missing",
            "Behaviour type must be specified in detection report.",
        ),
        Some(_) => validate_unit_interval(
            behaviour.confidence,
            "detection_report.behaviour.confidence.invalid",
            "Behaviour confidence must be between 0.0 and 1.0 in detection report.",
        ),
    }
}

fn validate_signal(signal: Signal) -> ValidationOutcome {
    if signal.amplitude.is_none() {
        return ValidationOutcome::fail(
            "detection_report.signal.amplitude.missing",
            "Signal amplitude must be specified in detection report.",
        );
    }

    if signal.centre_frequency.is_none() {
        return ValidationOutcome::fail(
            "detection_report.signal.centre_frequency.missing",
            "Signal centre frequency must be specified in detection report.",
        );
    }

    // Amplitude and frequency are never negative under any convention.
    for (value, field) in [
        (signal.amplitude, "amplitude"),
        (signal.start_frequency, "start_frequency"),
        (signal.centre_frequency, "centre_frequency"),
        (signal.stop_frequency, "stop_frequency"),
        (signal.pulse_duration, "pulse_duration"),
    ] {
        let validation = validate_finite_non_negative(
            value,
            format!("detection_report.signal.{field}.invalid"),
            "Signal value must be a finite number 0 or greater in detection report.",
        );
        if !validation.passed {
            return validation;
        }
    }

    ValidationOutcome::pass()
}

fn validate_derived_detection(derived_detection: DerivedDetection) -> ValidationOutcome {
    let timestamp_validation = validate_timestamp(
        derived_detection.timestamp,
        "detection_report.derived_detection.timestamp.missing",
        "Derived detection timestamp must be specified in detection report.",
        "detection_report.derived_detection.timestamp.malformed",
        "Derived detection timestamp is malformed in detection report.",
    );
    if !timestamp_validation.passed {
        return timestamp_validation;
    }

    let node_id_validation = crate::validation::common::validate_uuid_v4(
        derived_detection.node_id.as_deref(),
        "detection_report.derived_detection.node_id.invalid",
        "A valid UUID v4 must be used for a node ID in a derived detection.",
    );
    if !node_id_validation.passed {
        return node_id_validation;
    }

    validate_ulid(
        derived_detection.object_id.as_deref(),
        "detection_report.derived_detection.object_id.invalid",
        "A valid ULID must be used for an object ID in a derived detection.",
    )
}

fn validate_velocity_oneof(velocity_oneof: VelocityOneof) -> ValidationOutcome {
    match velocity_oneof {
        VelocityOneof::EnuVelocity(enu_velocity) => validate_enu_velocity(enu_velocity),
    }
}

fn validate_enu_velocity(enu_velocity: EnuVelocity) -> ValidationOutcome {
    if enu_velocity.east_rate.is_none() {
        return ValidationOutcome::fail(
            "detection_report.enu_velocity.east_rate.missing",
            "ENU velocity east rate must be specified in detection report.",
        );
    }

    if enu_velocity.north_rate.is_none() {
        return ValidationOutcome::fail(
            "detection_report.enu_velocity.north_rate.missing",
            "ENU velocity north rate must be specified in detection report.",
        );
    }

    // A rate is a signed vector component (direction matters), so only
    // finiteness is enforced here -- no non-negativity.
    for (value, field) in [
        (enu_velocity.east_rate, "east_rate"),
        (enu_velocity.north_rate, "north_rate"),
        (enu_velocity.up_rate, "up_rate"),
        (enu_velocity.east_rate_error, "east_rate_error"),
        (enu_velocity.north_rate_error, "north_rate_error"),
        (enu_velocity.up_rate_error, "up_rate_error"),
    ] {
        let validation = validate_finite(
            value,
            format!("detection_report.enu_velocity.{field}.invalid"),
            "ENU velocity rate must be a finite number in detection report.",
        );
        if !validation.passed {
            return validation;
        }
    }

    ValidationOutcome::pass()
}

#[cfg(test)]
mod detection_report_validation_tests {
    use prost_types::Timestamp;

    use crate::{
        bsi_flex_335_v2_0::{
            DetectionReport, EnuVelocity, Location, RangeBearing,
            detection_report::{LocationOneof, Signal, VelocityOneof},
        },
        finding::ValidationOutcome,
        validation::detection_report::{
            validate_detection_report, validate_location, validate_location_oneof,
            validate_object_id, validate_range_bearing, validate_report_id,
        },
    };

    /// Unit test to check that detection reports are correctly validated
    #[test]
    fn test_detection_report_validation() {
        // valid detection report
        let valid_location = Location {
            x: Some(1.0),
            y: Some(1.0),
            z: None,
            x_error: None,
            y_error: None,
            z_error: None,
            coordinate_system: Some(1),
            datum: Some(1),
            utm_zone: Some("ZX".to_string()),
        };
        let valid_detection_report = DetectionReport {
            report_id: Some("01H1VV3VN40RV97CDFSXJB44K9".to_string()),
            object_id: Some("01H1VV3VN40RV97CDFSXJB44K9".to_string()),
            task_id: None,
            state: None,
            detection_confidence: None,
            track_info: vec![],
            prediction_location: None,
            object_info: vec![],
            classification: vec![],
            behaviour: vec![],
            associated_file: vec![],
            signal: vec![],
            associated_detection: vec![],
            derived_detection: vec![],
            colour: None,
            id: None,
            location_oneof: Some(LocationOneof::Location(valid_location.clone())),
            velocity_oneof: None,
        };
        assert_eq!(
            ValidationOutcome::pass(),
            validate_detection_report(valid_detection_report)
        );

        // missing location detection report
        let missing_location_detection_report = DetectionReport {
            report_id: Some("01H1VV3VN40RV97CDFSXJB44K9".to_string()),
            object_id: Some("01H1VV3VN40RV97CDFSXJB44K9".to_string()),
            task_id: None,
            state: None,
            detection_confidence: None,
            track_info: vec![],
            prediction_location: None,
            object_info: vec![],
            classification: vec![],
            behaviour: vec![],
            associated_file: vec![],
            signal: vec![],
            associated_detection: vec![],
            derived_detection: vec![],
            colour: None,
            id: None,
            location_oneof: None,
            velocity_oneof: None,
        };
        assert_eq!(
            ValidationOutcome::fail(
                "detection_report.location.missing",
                "Location or range-bearing must be specified in detection report."
            ),
            validate_detection_report(missing_location_detection_report)
        );

        // invalid detection report
        let invalid_location = Location {
            x: Some(1.0),
            y: Some(1.0),
            z: None,
            x_error: None,
            y_error: None,
            z_error: None,
            coordinate_system: Some(1),
            datum: None,
            utm_zone: Some("ZX".to_string()),
        };
        let invalid_detection_report = DetectionReport {
            report_id: Some("01H1VV3VN40RV97CDFSXJB44K9".to_string()),
            object_id: Some("01H1VV3VN40RV97CDFSXJB44K9".to_string()),
            task_id: None,
            state: None,
            detection_confidence: None,
            track_info: vec![],
            prediction_location: None,
            object_info: vec![],
            classification: vec![],
            behaviour: vec![],
            associated_file: vec![],
            signal: vec![],
            associated_detection: vec![],
            derived_detection: vec![],
            colour: None,
            id: None,
            location_oneof: Some(LocationOneof::Location(invalid_location.clone())),
            velocity_oneof: None,
        };
        assert_eq!(
            ValidationOutcome::fail(
                "detection_report.location.datum.missing",
                "Datum must be specified in location."
            ),
            validate_detection_report(invalid_detection_report)
        );

        let invalid_signal_detection_report = DetectionReport {
            report_id: Some("01H1VV3VN40RV97CDFSXJB44K9".to_string()),
            object_id: Some("01H1VV3VN40RV97CDFSXJB44K9".to_string()),
            task_id: None,
            state: None,
            detection_confidence: None,
            track_info: vec![],
            prediction_location: None,
            object_info: vec![],
            classification: vec![],
            behaviour: vec![],
            associated_file: vec![],
            signal: vec![Signal {
                amplitude: None,
                start_frequency: None,
                centre_frequency: Some(100.0),
                stop_frequency: None,
                pulse_duration: None,
            }],
            associated_detection: vec![],
            derived_detection: vec![],
            colour: None,
            id: None,
            location_oneof: Some(LocationOneof::Location(valid_location.clone())),
            velocity_oneof: None,
        };
        assert_eq!(
            ValidationOutcome::fail(
                "detection_report.signal.amplitude.missing",
                "Signal amplitude must be specified in detection report."
            ),
            validate_detection_report(invalid_signal_detection_report)
        );

        let invalid_velocity_detection_report = DetectionReport {
            report_id: Some("01H1VV3VN40RV97CDFSXJB44K9".to_string()),
            object_id: Some("01H1VV3VN40RV97CDFSXJB44K9".to_string()),
            task_id: None,
            state: None,
            detection_confidence: None,
            track_info: vec![],
            prediction_location: None,
            object_info: vec![],
            classification: vec![],
            behaviour: vec![],
            associated_file: vec![],
            signal: vec![],
            associated_detection: vec![],
            derived_detection: vec![],
            colour: None,
            id: None,
            location_oneof: Some(LocationOneof::Location(valid_location.clone())),
            velocity_oneof: Some(VelocityOneof::EnuVelocity(EnuVelocity {
                east_rate: None,
                north_rate: Some(1.0),
                up_rate: None,
                east_rate_error: None,
                north_rate_error: None,
                up_rate_error: None,
            })),
        };
        assert_eq!(
            ValidationOutcome::fail(
                "detection_report.enu_velocity.east_rate.missing",
                "ENU velocity east rate must be specified in detection report."
            ),
            validate_detection_report(invalid_velocity_detection_report)
        );

        let valid_predicted_location_detection_report_without_timestamp = DetectionReport {
            report_id: Some("01H1VV3VN40RV97CDFSXJB44K9".to_string()),
            object_id: Some("01H1VV3VN40RV97CDFSXJB44K9".to_string()),
            task_id: None,
            state: None,
            detection_confidence: None,
            track_info: vec![],
            prediction_location: Some(crate::bsi_flex_335_v2_0::detection_report::PredictedLocation {
                predicted_location_oneof: Some(
                    crate::bsi_flex_335_v2_0::detection_report::predicted_location::PredictedLocationOneof::Location(valid_location.clone())
                ),
                predicted_timestamp: None,
            }),
            object_info: vec![],
            classification: vec![],
            behaviour: vec![],
            associated_file: vec![],
            signal: vec![],
            associated_detection: vec![],
            derived_detection: vec![],
            colour: None,
            id: None,
            location_oneof: Some(LocationOneof::Location(valid_location.clone())),
            velocity_oneof: None,
        };
        assert_eq!(
            ValidationOutcome::pass(),
            validate_detection_report(valid_predicted_location_detection_report_without_timestamp)
        );

        let valid_predicted_detection_report = DetectionReport {
            report_id: Some("01H1VV3VN40RV97CDFSXJB44K9".to_string()),
            object_id: Some("01H1VV3VN40RV97CDFSXJB44K9".to_string()),
            task_id: None,
            state: None,
            detection_confidence: None,
            track_info: vec![],
            prediction_location: Some(crate::bsi_flex_335_v2_0::detection_report::PredictedLocation {
                predicted_location_oneof: Some(
                    crate::bsi_flex_335_v2_0::detection_report::predicted_location::PredictedLocationOneof::Location(valid_location.clone())
                ),
                predicted_timestamp: Some(Timestamp { seconds: 1, nanos: 0 }),
            }),
            object_info: vec![],
            classification: vec![],
            behaviour: vec![],
            associated_file: vec![],
            signal: vec![Signal {
                amplitude: Some(1.0),
                start_frequency: None,
                centre_frequency: Some(100.0),
                stop_frequency: None,
                pulse_duration: None,
            }],
            associated_detection: vec![],
            derived_detection: vec![],
            colour: None,
            id: None,
            location_oneof: Some(LocationOneof::Location(valid_location)),
            velocity_oneof: Some(VelocityOneof::EnuVelocity(EnuVelocity {
                east_rate: Some(1.0),
                north_rate: Some(1.0),
                up_rate: None,
                east_rate_error: None,
                north_rate_error: None,
                up_rate_error: None,
            })),
        };
        assert_eq!(
            ValidationOutcome::pass(),
            validate_detection_report(valid_predicted_detection_report)
        );
    }

    /// A `PredictedLocation` present with neither a `location` nor a
    /// `rangeBearing` used to pass silently -- found via the
    /// fixture-decode-gap investigation
    /// (`0118.PredictedLocation.Location.Missing.json`), which exercised
    /// exactly this and had always passed for the wrong reason (the
    /// fixture itself failed to decode, for an unrelated reason, before
    /// that was fixed too). Every real `True` fixture that includes a
    /// `predictionLocation` always populates this oneof, matching
    /// `DetectionReport`'s own top-level `location_oneof`, which was
    /// already correctly mandatory.
    #[test]
    fn test_predicted_location_without_a_location_or_range_bearing_is_a_finding() {
        assert_eq!(
            ValidationOutcome::fail(
                "detection_report.prediction_location.missing",
                "Predicted location or range-bearing must be specified in detection report."
            ),
            validate_detection_report(DetectionReport {
                prediction_location: Some(
                    crate::bsi_flex_335_v2_0::detection_report::PredictedLocation {
                        predicted_location_oneof: None,
                        predicted_timestamp: None,
                    }
                ),
                ..detection_report_with(vec![], None)
            })
        );
    }

    /// Unit test to check that report IDs are correctly validated
    #[test]
    fn test_report_id_validation() {
        // valid report ID
        let valid_report_id = "01H1VV3VN40RV97CDFSXJB44K9".to_string();
        assert_eq!(
            ValidationOutcome::pass(),
            validate_report_id(Some(valid_report_id))
        );

        // invalid report ID
        let invalid_report_id = "".to_string();
        assert_eq!(
            ValidationOutcome::fail(
                "detection_report.report_id.invalid",
                "A valid ULID must be used for a report ID in a detection report."
            ),
            validate_report_id(Some(invalid_report_id))
        );
    }

    /// Unit test to check that object IDs are correctly validated
    #[test]
    fn test_object_id_validation() {
        // valid object ID
        let valid_object_id = "01H1VV3VN40RV97CDFSXJB44K9".to_string();
        assert_eq!(
            ValidationOutcome::pass(),
            validate_object_id(Some(valid_object_id))
        );

        // invalid object ID
        let invalid_object_id = "".to_string();
        assert_eq!(
            ValidationOutcome::fail(
                "detection_report.object_id.invalid",
                "A valid ULID must be used for an object ID in a detection report."
            ),
            validate_object_id(Some(invalid_object_id))
        );
    }

    /// Unit test to check that location one ofs are correctly validated
    #[test]
    fn test_location_oneof_validation() {
        // location location oneof
        let valid_location = Location {
            x: Some(1.0),
            y: Some(1.0),
            z: None,
            x_error: None,
            y_error: None,
            z_error: None,
            coordinate_system: Some(1),
            datum: Some(1),
            utm_zone: Some("ZX".to_string()),
        };
        let location_location_oneof = LocationOneof::Location(valid_location);
        assert_eq!(
            ValidationOutcome::pass(),
            validate_location_oneof(location_location_oneof, "test.location")
        );

        // range-bearing location oneof
        let valid_range_bearing = RangeBearing {
            elevation: Some(1.0),
            azimuth: None,
            range: None,
            elevation_error: None,
            azimuth_error: None,
            range_error: None,
            coordinate_system: Some(1),
            datum: Some(1),
        };
        let range_bearing_location_oneof = LocationOneof::RangeBearing(valid_range_bearing);
        assert_eq!(
            ValidationOutcome::pass(),
            validate_location_oneof(range_bearing_location_oneof, "test.location")
        );
    }

    // Unit test to check that locations are correctly validated
    #[test]
    fn test_location_validation() {
        // valid location
        let valid_location = Location {
            x: Some(1.0),
            y: Some(1.0),
            z: None,
            x_error: None,
            y_error: None,
            z_error: None,
            coordinate_system: Some(1),
            datum: Some(1),
            utm_zone: Some("ZX".to_string()),
        };
        assert_eq!(
            ValidationOutcome::pass(),
            validate_location(valid_location, "test.location")
        );

        // missing coordinate
        let missing_coordinate_location = Location {
            x: Some(1.0),
            y: Some(1.0),
            z: None,
            x_error: None,
            y_error: None,
            z_error: None,
            coordinate_system: None,
            datum: Some(1),
            utm_zone: Some("ZX".to_string()),
        };
        assert_eq!(
            ValidationOutcome::fail(
                "test.location.coordinate_system.invalid",
                "Coordinate system must be specified in location."
            ),
            validate_location(missing_coordinate_location, "test.location")
        );

        // missing datum
        let missing_datum_location = Location {
            x: Some(1.0),
            y: Some(1.0),
            z: None,
            x_error: None,
            y_error: None,
            z_error: None,
            coordinate_system: Some(1),
            datum: None,
            utm_zone: Some("ZX".to_string()),
        };
        assert_eq!(
            ValidationOutcome::fail(
                "test.location.datum.missing",
                "Datum must be specified in location."
            ),
            validate_location(missing_datum_location, "test.location")
        );
    }

    // Unit test to check that range bearings are correctly validated
    #[test]
    fn test_range_bearing_validation() {
        // valid range bearing
        let valid_range_bearing = RangeBearing {
            elevation: Some(1.0),
            azimuth: None,
            range: None,
            elevation_error: None,
            azimuth_error: None,
            range_error: None,
            coordinate_system: Some(1),
            datum: Some(1),
        };
        assert_eq!(
            ValidationOutcome::pass(),
            validate_range_bearing(valid_range_bearing, "test.range_bearing")
        );

        // missing coordinate
        let missing_coordinate_range_bearing = RangeBearing {
            elevation: Some(1.0),
            azimuth: None,
            range: None,
            elevation_error: None,
            azimuth_error: None,
            range_error: None,
            coordinate_system: None,
            datum: Some(1),
        };
        assert_eq!(
            ValidationOutcome::fail(
                "test.range_bearing.coordinate_system.invalid",
                "Coordinate system must be specified in range bearing."
            ),
            validate_range_bearing(missing_coordinate_range_bearing, "test.range_bearing")
        );

        // missing datum
        let missing_datum_range_bearing = RangeBearing {
            elevation: Some(1.0),
            azimuth: None,
            range: None,
            elevation_error: None,
            azimuth_error: None,
            range_error: None,
            coordinate_system: Some(1),
            datum: None,
        };
        assert_eq!(
            ValidationOutcome::fail(
                "test.range_bearing.datum.missing",
                "Datum must be specified in range bearing."
            ),
            validate_range_bearing(missing_datum_range_bearing, "test.range_bearing")
        );

        // no coordinates is valid in the v2 proto because elevation, azimuth, and range are all optional
        let no_coordinate_range_bearing = RangeBearing {
            elevation: None,
            azimuth: None,
            range: None,
            elevation_error: None,
            azimuth_error: None,
            range_error: None,
            coordinate_system: Some(1),
            datum: Some(1),
        };
        assert_eq!(
            ValidationOutcome::pass(),
            validate_range_bearing(no_coordinate_range_bearing, "test.range_bearing")
        );
    }

    fn detection_report_with(
        signal: Vec<Signal>,
        velocity_oneof: Option<VelocityOneof>,
    ) -> DetectionReport {
        DetectionReport {
            report_id: Some("01H1VV3VN40RV97CDFSXJB44K9".to_string()),
            object_id: Some("01H1VV3VN40RV97CDFSXJB44K9".to_string()),
            task_id: None,
            state: None,
            detection_confidence: None,
            track_info: vec![],
            prediction_location: None,
            object_info: vec![],
            classification: vec![],
            behaviour: vec![],
            associated_file: vec![],
            signal,
            associated_detection: vec![],
            derived_detection: vec![],
            colour: None,
            id: None,
            location_oneof: Some(LocationOneof::Location(Location {
                x: Some(1.0),
                y: Some(1.0),
                z: None,
                x_error: None,
                y_error: None,
                z_error: None,
                coordinate_system: Some(1),
                datum: Some(1),
                utm_zone: None,
            })),
            velocity_oneof,
        }
    }

    #[test]
    fn test_signal_negative_and_non_finite_values_are_findings() {
        assert_eq!(
            ValidationOutcome::fail(
                "detection_report.signal.amplitude.invalid",
                "Signal value must be a finite number 0 or greater in detection report."
            ),
            validate_detection_report(detection_report_with(
                vec![Signal {
                    amplitude: Some(f32::NAN),
                    start_frequency: None,
                    centre_frequency: Some(10.0),
                    stop_frequency: None,
                    pulse_duration: None,
                }],
                None
            ))
        );
        assert_eq!(
            ValidationOutcome::fail(
                "detection_report.signal.centre_frequency.invalid",
                "Signal value must be a finite number 0 or greater in detection report."
            ),
            validate_detection_report(detection_report_with(
                vec![Signal {
                    amplitude: Some(1.0),
                    start_frequency: None,
                    centre_frequency: Some(-1.0),
                    stop_frequency: None,
                    pulse_duration: None,
                }],
                None
            ))
        );
    }

    #[test]
    fn test_enu_velocity_non_finite_rate_is_a_finding() {
        assert_eq!(
            ValidationOutcome::fail(
                "detection_report.enu_velocity.east_rate.invalid",
                "ENU velocity rate must be a finite number in detection report."
            ),
            validate_detection_report(detection_report_with(
                vec![],
                Some(VelocityOneof::EnuVelocity(EnuVelocity {
                    east_rate: Some(f64::INFINITY),
                    north_rate: Some(1.0),
                    up_rate: None,
                    east_rate_error: None,
                    north_rate_error: None,
                    up_rate_error: None,
                }))
            ))
        );
    }

    /// `validate_derived_detection` used to only call `validate_timestamp` at
    /// all when a timestamp was already present, so a missing one silently
    /// passed instead of producing the (already-defined, but previously
    /// unreachable) `detection_report.derived_detection.timestamp.missing`
    /// finding -- found via the fixture-decode-gap investigation
    /// (`0129.DerivedDetection.Timestamp.Missing.json`), which exercised
    /// exactly this and had always passed for the wrong reason (the fixture
    /// itself failed to decode, for an unrelated reason, before that was
    /// fixed too).
    #[test]
    fn test_derived_detection_timestamp_validation() {
        use crate::bsi_flex_335_v2_0::detection_report::DerivedDetection;

        let derived_detection_with_timestamp = |timestamp| DerivedDetection {
            timestamp,
            node_id: Some("a8654cdf-4328-47de-81fa-c495589e30c9".to_string()),
            object_id: Some("01H1VV3VN40RV97CDFSXJB44K9".to_string()),
        };

        assert_eq!(
            ValidationOutcome::fail(
                "detection_report.derived_detection.timestamp.missing",
                "Derived detection timestamp must be specified in detection report."
            ),
            validate_detection_report(DetectionReport {
                derived_detection: vec![derived_detection_with_timestamp(None)],
                ..detection_report_with(vec![], None)
            })
        );

        assert_eq!(
            ValidationOutcome::fail(
                "detection_report.derived_detection.timestamp.malformed",
                "Derived detection timestamp is malformed in detection report."
            ),
            validate_detection_report(DetectionReport {
                derived_detection: vec![derived_detection_with_timestamp(Some(Timestamp {
                    seconds: 0,
                    nanos: -1,
                }))],
                ..detection_report_with(vec![], None)
            })
        );

        assert_eq!(
            ValidationOutcome::pass(),
            validate_detection_report(DetectionReport {
                derived_detection: vec![derived_detection_with_timestamp(Some(Timestamp {
                    seconds: 1,
                    nanos: 0,
                }))],
                ..detection_report_with(vec![], None)
            })
        );
    }
}
