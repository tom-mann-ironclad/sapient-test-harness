use crate::bsi_flex_335_v2_0::{
    LocationOrRangeBearing, StatusReport,
    status_report::{Power, Status},
};
use crate::finding::ValidationOutcome;
use crate::validation::common::{
    validate_implicit_enum, validate_location,
    validate_location_or_range_bearing as validate_common_location_or_range_bearing,
    validate_required_enum, validate_required_string, validate_ulid,
};

/// Function to validation a SAPIENT status report message
pub fn validate_status_report(status_report: StatusReport) -> ValidationOutcome {
    let mut validations = vec![];

    // Check report ID
    validations.push(validate_report_id(status_report.report_id));

    // Check system
    validations.push(validate_system(status_report.system));

    // Check info
    validations.push(validate_info(status_report.info));

    // Check active task ID if provided
    if status_report.active_task_id.is_some() {
        validations.push(validate_ulid(
            status_report.active_task_id.as_deref(),
            "status_report.active_task_id.invalid",
            "A valid ULID must be used for an active task ID in a status report.",
        ));
    }

    // Check mode
    validations.push(validate_mode(status_report.mode));

    // Check node location if provided
    if let Some(node_location) = status_report.node_location {
        validations.push(validate_location(
            node_location,
            "status_report.node_location",
        ));
    }

    if let Some(power) = status_report.power {
        validations.push(validate_power(power));
    }

    // Check field of view if provided
    if let Some(field_of_view) = status_report.field_of_view {
        validations.push(validate_location_or_range_bearing(
            field_of_view,
            "status_report.field_of_view",
        ));
    }

    for coverage in status_report.coverage {
        validations.push(validate_location_or_range_bearing(
            coverage,
            "status_report.coverage",
        ));
    }

    for obscuration in status_report.obscuration {
        validations.push(validate_location_or_range_bearing(
            obscuration,
            "status_report.obscuration",
        ));
    }

    for status in status_report.status {
        validations.push(validate_status(status));
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
        "status_report.report_id.invalid",
        "A valid ULID must be used for a report ID in a status report.",
    )
}

/// Function to check a system as specified in the SAPIENT version 7 ICD
/// `System` value 4 is `reserved` in `status_report.proto` (withdrawn after
/// SAPIENT v7 as not well-defined) -- still a legal `int32` on the wire, so
/// it must be explicitly excluded rather than just checked for nonzero.
fn validate_system(system: Option<i32>) -> ValidationOutcome {
    match system {
        Some(v) if [1, 2, 3, 5].contains(&v) => ValidationOutcome::pass(),
        _ => ValidationOutcome::fail(
            "status_report.system.invalid",
            "System must be specified in status report.",
        ),
    }
}

/// Function to check a info as specified in the SAPIENT version 7 ICD
fn validate_info(info: Option<i32>) -> ValidationOutcome {
    let valid_info = match info {
        Some(0) => false,
        Some(1) => true,
        Some(2) => true,
        _ => false,
    };
    if !valid_info {
        return ValidationOutcome::fail(
            "status_report.info.invalid",
            "Info must be specified in status report.",
        );
    }

    ValidationOutcome::pass()
}

fn validate_mode(mode: Option<String>) -> ValidationOutcome {
    validate_required_string(
        mode.as_deref(),
        "status_report.mode.missing",
        "Mode must be specified in status report.",
    )
}

fn validate_location_or_range_bearing(
    location_or_range_bearing: LocationOrRangeBearing,
    rule_id_prefix: &str,
) -> ValidationOutcome {
    validate_common_location_or_range_bearing(
        location_or_range_bearing,
        rule_id_prefix,
        "Location or range-bearing must be specified in status report.",
    )
}

fn validate_status(status: Status) -> ValidationOutcome {
    // `StatusType` is mandatory and has no reserved gaps (0-13).
    let status_type_validation = validate_required_enum(
        status.status_type,
        13,
        "status_report.status.status_type.missing",
        "Status type must be specified in status report.",
    );
    if !status_type_validation.passed {
        return status_type_validation;
    }

    validate_status_level(status.status_level)
}

/// `StatusLevel` value 1 is `reserved` in `status_report.proto`
/// (`STATUS_LEVEL_SENSOR_STATUS`, withdrawn) -- still a legal `int32` on
/// the wire. `status_level` itself is optional, so `None` is fine, but a
/// present value must be one of the currently-defined ones.
fn validate_status_level(status_level: Option<i32>) -> ValidationOutcome {
    match status_level {
        None => ValidationOutcome::pass(),
        Some(v) if [2, 3, 4].contains(&v) => ValidationOutcome::pass(),
        Some(_) => ValidationOutcome::fail(
            "status_report.status.status_level.invalid",
            "Status level is not a valid option in status report.",
        ),
    }
}

/// The local schema describes battery level as 0-100; `level` is optional,
/// so absence passes. `source`/`status` are proto3 fields declared without
/// `optional` (see `status_report.proto`'s `Power` message), so the wire
/// format cannot tell "absent" from "explicitly 0/UNSPECIFIED" for either
/// -- 0 must be accepted there, only a genuinely undefined discriminant is
/// invalid.
fn validate_power(power: Power) -> ValidationOutcome {
    if let Some(level) = power.level
        && !(0..=100).contains(&level)
    {
        return ValidationOutcome::fail(
            "status_report.power.level.invalid",
            "Power level must be between 0 and 100 in status report.",
        );
    }

    // `PowerSource` has no reserved gaps (0-8).
    let source_validation = validate_implicit_enum(
        power.source,
        8,
        "status_report.power.source.invalid",
        "Power source is not a valid option in status report.",
    );
    if !source_validation.passed {
        return source_validation;
    }

    // `PowerStatus` has no reserved gaps (0-2).
    validate_implicit_enum(
        power.status,
        2,
        "status_report.power.status.invalid",
        "Power status is not a valid option in status report.",
    )
}

#[cfg(test)]
mod status_report_validation_tests {
    use crate::{
        bsi_flex_335_v2_0::{
            Location, LocationList, LocationOrRangeBearing, StatusReport,
            location_or_range_bearing::FovOneof,
            status_report::{Power, Status},
        },
        finding::ValidationOutcome,
        validation::status_report::{
            validate_info, validate_mode, validate_power, validate_report_id, validate_status,
            validate_status_report, validate_system,
        },
    };

    /// Unit test to check that status reports are correctly validated
    #[test]
    fn test_status_report_validation() {
        // valid status report
        let valid_status_report = StatusReport {
            report_id: Some("01H1VV3VN40RV97CDFSXJB44K9".to_string()),
            system: Some(1),
            info: Some(1),
            active_task_id: Some("01H1VV3VN40RV97CDFSXJB44K9".to_string()),
            mode: Some("Default".to_string()),
            power: None,
            node_location: Some(Location {
                x: Some(1.0),
                y: Some(1.0),
                z: None,
                x_error: None,
                y_error: None,
                z_error: None,
                coordinate_system: Some(1),
                datum: Some(1),
                utm_zone: None,
            }),
            field_of_view: Some(LocationOrRangeBearing {
                fov_oneof: Some(FovOneof::LocationList(LocationList {
                    locations: vec![Location {
                        x: Some(1.0),
                        y: Some(1.0),
                        z: None,
                        x_error: None,
                        y_error: None,
                        z_error: None,
                        coordinate_system: Some(1),
                        datum: Some(1),
                        utm_zone: None,
                    }],
                })),
            }),
            coverage: vec![],
            obscuration: vec![],
            status: vec![Status {
                status_level: Some(2),
                status_value: Some("raining".to_string()),
                status_type: Some(4),
            }],
        };
        assert_eq!(
            ValidationOutcome::pass(),
            validate_status_report(valid_status_report)
        );

        // missing report ID
        let valid_status_report = StatusReport {
            report_id: None,
            system: Some(1),
            info: Some(1),
            active_task_id: None,
            mode: Some("Default".to_string()),
            power: None,
            node_location: None,
            field_of_view: None,
            coverage: vec![],
            obscuration: vec![],
            status: vec![],
        };
        assert_eq!(
            ValidationOutcome::fail(
                "status_report.report_id.invalid",
                "A valid ULID must be used for a report ID in a status report."
            ),
            validate_status_report(valid_status_report)
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
                "status_report.report_id.invalid",
                "A valid ULID must be used for a report ID in a status report."
            ),
            validate_report_id(Some(invalid_report_id))
        );
    }

    /// Unit test to check that systems are correctly validated, including
    /// that the `reserved` value 4 (withdrawn `SYSTEM_TAMPER`) is rejected,
    /// not just 0.
    #[test]
    fn test_system_validation() {
        for i in [1, 2, 3, 5] {
            assert_eq!(ValidationOutcome::pass(), validate_system(Some(i)));
        }

        for i in [0, 4] {
            assert_eq!(
                ValidationOutcome::fail(
                    "status_report.system.invalid",
                    "System must be specified in status report."
                ),
                validate_system(Some(i))
            );
        }
    }

    #[test]
    fn test_mode_validation() {
        assert_eq!(
            ValidationOutcome::pass(),
            validate_mode(Some("Default".to_string()))
        );
        assert_eq!(
            ValidationOutcome::fail(
                "status_report.mode.missing",
                "Mode must be specified in status report."
            ),
            validate_mode(None)
        );
    }

    /// Unit test to check that infos are correctly validated
    #[test]
    fn test_info_validation() {
        // valid info
        assert_eq!(ValidationOutcome::pass(), validate_info(Some(1)));
        assert_eq!(ValidationOutcome::pass(), validate_info(Some(2)));

        // invalid info
        assert_eq!(
            ValidationOutcome::fail(
                "status_report.info.invalid",
                "Info must be specified in status report."
            ),
            validate_info(None)
        );
        assert_eq!(
            ValidationOutcome::fail(
                "status_report.info.invalid",
                "Info must be specified in status report."
            ),
            validate_info(Some(0))
        );
        assert_eq!(
            ValidationOutcome::fail(
                "status_report.info.invalid",
                "Info must be specified in status report."
            ),
            validate_info(Some(3))
        );
    }

    #[test]
    fn test_status_entry_validation() {
        assert_eq!(
            ValidationOutcome::pass(),
            validate_status(Status {
                status_level: Some(2),
                status_value: Some("clear".to_string()),
                status_type: Some(4),
            })
        );
        assert_eq!(
            ValidationOutcome::fail(
                "status_report.status.status_type.missing",
                "Status type must be specified in status report."
            ),
            validate_status(Status {
                status_level: Some(2),
                status_value: Some("clear".to_string()),
                status_type: None,
            })
        );

        // status_level is optional -- absent is fine
        assert_eq!(
            ValidationOutcome::pass(),
            validate_status(Status {
                status_level: None,
                status_value: Some("clear".to_string()),
                status_type: Some(4),
            })
        );

        // reserved value 1 (withdrawn STATUS_LEVEL_SENSOR_STATUS) must be
        // rejected when present, not just 0
        for level in [0, 1] {
            assert_eq!(
                ValidationOutcome::fail(
                    "status_report.status.status_level.invalid",
                    "Status level is not a valid option in status report."
                ),
                validate_status(Status {
                    status_level: Some(level),
                    status_value: Some("clear".to_string()),
                    status_type: Some(4),
                })
            );
        }
    }

    #[test]
    fn test_optional_power_subfields_are_accepted() {
        let mut status_report = StatusReport {
            report_id: Some("01H1VV3VN40RV97CDFSXJB44K9".to_string()),
            system: Some(1),
            info: Some(1),
            active_task_id: None,
            mode: Some("Default".to_string()),
            power: Some(Power {
                level: Some(95),
                source: 0,
                status: 0,
            }),
            node_location: None,
            field_of_view: None,
            coverage: vec![],
            obscuration: vec![],
            status: vec![],
        };
        assert_eq!(
            ValidationOutcome::pass(),
            validate_status_report(status_report.clone())
        );

        status_report.power = Some(Power {
            level: Some(95),
            source: 1,
            status: 0,
        });
        assert_eq!(
            ValidationOutcome::pass(),
            validate_status_report(status_report)
        );
    }

    #[test]
    fn test_power_level_out_of_range_is_a_finding() {
        assert_eq!(
            ValidationOutcome::fail(
                "status_report.power.level.invalid",
                "Power level must be between 0 and 100 in status report."
            ),
            validate_power(Power {
                level: Some(101),
                source: 0,
                status: 0,
            })
        );
        assert_eq!(
            ValidationOutcome::fail(
                "status_report.power.level.invalid",
                "Power level must be between 0 and 100 in status report."
            ),
            validate_power(Power {
                level: Some(-1),
                source: 0,
                status: 0,
            })
        );
        assert_eq!(
            ValidationOutcome::pass(),
            validate_power(Power {
                level: None,
                source: 0,
                status: 0,
            })
        );
    }

    #[test]
    fn test_power_undefined_source_or_status_is_a_finding() {
        assert_eq!(
            ValidationOutcome::fail(
                "status_report.power.source.invalid",
                "Power source is not a valid option in status report."
            ),
            validate_power(Power {
                level: None,
                source: 99,
                status: 0,
            })
        );
        assert_eq!(
            ValidationOutcome::fail(
                "status_report.power.status.invalid",
                "Power status is not a valid option in status report."
            ),
            validate_power(Power {
                level: None,
                source: 0,
                status: 99,
            })
        );
    }
}
