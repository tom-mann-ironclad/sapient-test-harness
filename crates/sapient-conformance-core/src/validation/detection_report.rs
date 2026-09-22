use crate::bsi_flex_335_v2_0::{
    DetectionReport, EnuVelocity, Location, RangeBearing,
    detection_report::{
        Behaviour, DerivedDetection, DetectionReportClassification, LocationOneof,
        PredictedLocation, Signal, SubClass, TrackObjectInfo, VelocityOneof,
        predicted_location::PredictedLocationOneof,
    },
};
use crate::validation::common::{
    validate_associated_detection, validate_associated_file,
    validate_location as validate_common_location,
    validate_range_bearing as validate_common_range_bearing, validate_timestamp, validate_ulid,
    validate_unit_interval,
};

/// Function to validation a SAPIENT detection report message
pub fn validate_detection_report(detection_report: DetectionReport) -> (bool, String) {
    let mut validations = vec![];

    // Check report ID
    validations.push(validate_report_id(detection_report.report_id));

    // Check object ID
    validations.push(validate_object_id(detection_report.object_id));

    if detection_report.task_id.is_some() {
        validations.push(validate_ulid(
            detection_report.task_id.as_deref(),
            "A valid ULID must be used for a task ID in a detection report.",
        ));
    }

    validations.push(validate_unit_interval(
        detection_report.detection_confidence,
        "Detection confidence must be between 0.0 and 1.0 in detection report.",
    ));

    // Check location
    if detection_report.location_oneof.is_none() {
        return (
            false,
            "Location or range-bearing must be specified in detection report.".to_string(),
        );
    }
    validations.push(validate_location_oneof(
        detection_report.location_oneof.unwrap(),
    ));

    if let Some(prediction_location) = detection_report.prediction_location {
        validations.push(validate_predicted_location(prediction_location));
    }

    for track_info in detection_report.track_info {
        validations.push(validate_track_object_info(track_info));
    }

    for object_info in detection_report.object_info {
        validations.push(validate_track_object_info(object_info));
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
            "Associated file type must be specified in a detection report.",
            "Associated file URL must be specified in a detection report.",
        ));
    }

    for associated_detection in detection_report.associated_detection {
        validations.push(validate_associated_detection(
            associated_detection,
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
        if !validation.0 {
            return validation;
        }
    }

    (true, "".to_string())
}

/// Function to check a report ID as specified in the SAPIENT version 7 ICD
fn validate_report_id(report_id: Option<String>) -> (bool, String) {
    validate_ulid(
        report_id.as_deref(),
        "A valid ULID must be used for a report ID in a detection report.",
    )
}

/// Function to check a object ID as specified in the SAPIENT version 7 ICD
fn validate_object_id(object_id: Option<String>) -> (bool, String) {
    validate_ulid(
        object_id.as_deref(),
        "A valid ULID must be used for an object ID in a detection report.",
    )
}

/// Function to check a location/rangebearing as specified in the SAPIENT version 7 ICD
fn validate_location_oneof(location_oneof: LocationOneof) -> (bool, String) {
    match location_oneof {
        LocationOneof::Location(location) => validate_location(location),
        LocationOneof::RangeBearing(range_bearing) => validate_range_bearing(range_bearing),
    }
}

/// Function to check a location as specified in the SAPIENT version 7 ICD
fn validate_location(location: Location) -> (bool, String) {
    validate_common_location(location)
}

/// Function to check a range bearing as specified in the SAPIENT version 7 ICD
fn validate_range_bearing(range_bearing: RangeBearing) -> (bool, String) {
    validate_common_range_bearing(range_bearing)
}

fn validate_predicted_location(predicted_location: PredictedLocation) -> (bool, String) {
    match predicted_location.predicted_location_oneof {
        Some(PredictedLocationOneof::Location(location)) => validate_location(location),
        Some(PredictedLocationOneof::RangeBearing(range_bearing)) => {
            validate_range_bearing(range_bearing)
        }
        None => (true, "".to_string()),
    }
}

fn validate_track_object_info(track_object_info: TrackObjectInfo) -> (bool, String) {
    match track_object_info.r#type.as_deref() {
        Some("") | None => {
            return (
                false,
                "Track object info type must be specified in detection report.".to_string(),
            );
        }
        Some(_) => {}
    }

    match track_object_info.value.as_deref() {
        Some("") | None => {
            return (
                false,
                "Track object info value must be specified in detection report.".to_string(),
            );
        }
        Some(_) => {}
    }

    (true, "".to_string())
}

fn validate_classification(classification: DetectionReportClassification) -> (bool, String) {
    match classification.r#type.as_deref() {
        Some("") | None => {
            return (
                false,
                "Classification type must be specified in detection report.".to_string(),
            );
        }
        Some(_) => {}
    }

    for sub_class in classification.sub_class {
        let validation = validate_sub_class(sub_class);
        if !validation.0 {
            return validation;
        }
    }

    let confidence_validation = validate_unit_interval(
        classification.confidence,
        "Classification confidence must be between 0.0 and 1.0 in detection report.",
    );
    if !confidence_validation.0 {
        return confidence_validation;
    }

    (true, "".to_string())
}

fn validate_sub_class(sub_class: SubClass) -> (bool, String) {
    match sub_class.r#type.as_deref() {
        Some("") | None => {
            return (
                false,
                "Classification sub class type must be specified in detection report.".to_string(),
            );
        }
        Some(_) => {}
    }

    if sub_class.level.is_none() {
        return (
            false,
            "Classification sub class level must be specified in detection report.".to_string(),
        );
    }

    for nested_sub_class in sub_class.sub_class {
        let validation = validate_sub_class(nested_sub_class);
        if !validation.0 {
            return validation;
        }
    }

    (true, "".to_string())
}

fn validate_behaviour(behaviour: Behaviour) -> (bool, String) {
    match behaviour.r#type.as_deref() {
        Some("") | None => (
            false,
            "Behaviour type must be specified in detection report.".to_string(),
        ),
        Some(_) => {
            let confidence_validation = validate_unit_interval(
                behaviour.confidence,
                "Behaviour confidence must be between 0.0 and 1.0 in detection report.",
            );
            if !confidence_validation.0 {
                return confidence_validation;
            }
            (true, "".to_string())
        }
    }
}

fn validate_signal(signal: Signal) -> (bool, String) {
    if signal.amplitude.is_none() {
        return (
            false,
            "Signal amplitude must be specified in detection report.".to_string(),
        );
    }

    if signal.centre_frequency.is_none() {
        return (
            false,
            "Signal centre frequency must be specified in detection report.".to_string(),
        );
    }

    (true, "".to_string())
}

fn validate_derived_detection(derived_detection: DerivedDetection) -> (bool, String) {
    if derived_detection.timestamp.is_some() {
        let timestamp_validation = validate_timestamp(
            derived_detection.timestamp,
            "Derived detection timestamp must be specified in detection report.",
            "Derived detection timestamp is malformed in detection report.",
        );
        if !timestamp_validation.0 {
            return timestamp_validation;
        }
    }

    let node_id_validation = crate::validation::common::validate_uuid_v4(
        derived_detection.node_id.as_deref(),
        "A valid UUID v4 must be used for a node ID in a derived detection.",
    );
    if !node_id_validation.0 {
        return node_id_validation;
    }

    validate_ulid(
        derived_detection.object_id.as_deref(),
        "A valid ULID must be used for an object ID in a derived detection.",
    )
}

fn validate_velocity_oneof(velocity_oneof: VelocityOneof) -> (bool, String) {
    match velocity_oneof {
        VelocityOneof::EnuVelocity(enu_velocity) => validate_enu_velocity(enu_velocity),
    }
}

fn validate_enu_velocity(enu_velocity: EnuVelocity) -> (bool, String) {
    if enu_velocity.east_rate.is_none() {
        return (
            false,
            "ENU velocity east rate must be specified in detection report.".to_string(),
        );
    }

    if enu_velocity.north_rate.is_none() {
        return (
            false,
            "ENU velocity north rate must be specified in detection report.".to_string(),
        );
    }

    (true, "".to_string())
}

#[cfg(test)]
mod detection_report_validation_tests {
    use prost_types::Timestamp;

    use crate::{
        bsi_flex_335_v2_0::{
            DetectionReport, EnuVelocity, Location, RangeBearing,
            detection_report::{LocationOneof, Signal, VelocityOneof},
        },
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
            (true, "".to_string()),
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
            (
                false,
                "Location or range-bearing must be specified in detection report.".to_string()
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
            (false, "Datum must be specified in location.".to_string()),
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
            (
                false,
                "Signal amplitude must be specified in detection report.".to_string()
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
            (
                false,
                "ENU velocity east rate must be specified in detection report.".to_string()
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
            (true, "".to_string()),
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
            (true, "".to_string()),
            validate_detection_report(valid_predicted_detection_report)
        );
    }

    /// Unit test to check that report IDs are correctly validated
    #[test]
    fn test_report_id_validation() {
        // valid report ID
        let valid_report_id = "01H1VV3VN40RV97CDFSXJB44K9".to_string();
        assert_eq!(
            (true, "".to_string()),
            validate_report_id(Some(valid_report_id))
        );

        // invalid report ID
        let invalid_report_id = "".to_string();
        assert_eq!(
            (
                false,
                "A valid ULID must be used for a report ID in a detection report.".to_string()
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
            (true, "".to_string()),
            validate_object_id(Some(valid_object_id))
        );

        // invalid object ID
        let invalid_object_id = "".to_string();
        assert_eq!(
            (
                false,
                "A valid ULID must be used for an object ID in a detection report.".to_string()
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
            (true, "".to_string()),
            validate_location_oneof(location_location_oneof)
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
            (true, "".to_string()),
            validate_location_oneof(range_bearing_location_oneof)
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
        assert_eq!((true, "".to_string()), validate_location(valid_location));

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
            (
                false,
                "Coordinate system must be specified in location.".to_string()
            ),
            validate_location(missing_coordinate_location)
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
            (false, "Datum must be specified in location.".to_string()),
            validate_location(missing_datum_location)
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
            (true, "".to_string()),
            validate_range_bearing(valid_range_bearing)
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
            (
                false,
                "Coordinate system must be specified in range bearing.".to_string()
            ),
            validate_range_bearing(missing_coordinate_range_bearing)
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
            (
                false,
                "Datum must be specified in range bearing.".to_string()
            ),
            validate_range_bearing(missing_datum_range_bearing)
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
            (true, "".to_string()),
            validate_range_bearing(no_coordinate_range_bearing)
        );
    }
}
