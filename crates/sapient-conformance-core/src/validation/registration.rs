use std::collections::HashSet;

use crate::bsi_flex_335_v2_0::registration::location_type::CoordinatesOneof::{
    LocationUnits, RangeBearingUnits,
};
use crate::bsi_flex_335_v2_0::registration::location_type::DatumOneof::{
    LocationDatum, RangeBearingDatum,
};
use crate::bsi_flex_335_v2_0::registration::velocity_type::VelocityUnitsOneof::EnuVelocityUnits;
use crate::bsi_flex_335_v2_0::{
    EnuVelocityUnits as RegistrationEnuVelocityUnits, Registration,
    registration::{
        BehaviourDefinition, BehaviourFilterDefinition, Capability, ClassDefinition,
        ClassFilterDefinition, Command, ConfigurationData, DetectionClassDefinition,
        DetectionDefinition, Duration, ExtensionSubclass, FilterParameter, GeometricError,
        LocationType, ModeDefinition, ModeParameter, NodeDefinition, PerformanceValue,
        RegionDefinition, StatusDefinition, SubClass, SubClassFilterDefinition, TaskDefinition,
        TaxonomyDockDefinition, VelocityType,
    },
};
use crate::finding::ValidationOutcome;
use crate::validation::common::{
    validate_location_coordinate_system,
    validate_location_or_range_bearing as validate_common_location_or_range_bearing,
    validate_nonzero, validate_optional_enum, validate_range_bearing_coordinate_system,
    validate_required_enum, validate_required_string, validate_uuid_v4,
};

/// Function to validation a SAPIENT registration message
pub fn validate_registration(registration: Registration) -> ValidationOutcome {
    let mut validations = vec![];

    // Check node type
    validations.push(validate_node_definition(registration.node_definition));

    // Check ICD version
    validations.push(validate_icd_version(registration.icd_version));

    // Check capabilities
    validations.push(validate_capabilities(registration.capabilities));

    // Check status definition
    if registration.status_definition.is_none() {
        return ValidationOutcome::fail(
            "registration.status_definition.missing",
            "Status definition must be specified in registration.",
        );
    }
    validations.push(validate_status_definition(
        registration.status_definition.unwrap(),
    ));

    // Check mode definition
    validations.push(validate_mode_definitions(registration.mode_definition));

    // Check dependent nodes
    validations.push(validate_dependent_nodes(registration.dependent_nodes));

    // Check reporting region
    validations.push(validate_reporting_region(registration.reporting_region));

    // Check config data
    validations.push(validate_config_data(registration.config_data));

    for validation in validations {
        if !validation.passed {
            return validation;
        }
    }

    ValidationOutcome::pass()
}

/// Function to check the node definition as specified in the BSI Flex 335 V2.0
fn validate_node_definition(node_definitions: Vec<NodeDefinition>) -> ValidationOutcome {
    if node_definitions.is_empty() {
        return ValidationOutcome::fail(
            "registration.node_definition.empty",
            "Node type must be specified in node defintition.",
        );
    }
    for node_definition in node_definitions {
        // `NodeType` is mandatory and has no reserved gaps (0-20).
        let node_type_validation = validate_required_enum(
            node_definition.node_type,
            20,
            "registration.node_definition.node_type.missing",
            "Node type must be specified in node defintition.",
        );
        if !node_type_validation.passed {
            return node_type_validation;
        }
    }
    ValidationOutcome::pass()
}

fn validate_capabilities(capabilities: Vec<Capability>) -> ValidationOutcome {
    if capabilities.is_empty() {
        return ValidationOutcome::fail(
            "registration.capabilities.empty",
            "Capabilities must be specified in registration.",
        );
    }

    for capability in capabilities {
        let category_validation = validate_required_string(
            capability.category.as_deref(),
            "registration.capabilities.category.missing",
            "Capability category must be specified in registration.",
        );
        if !category_validation.passed {
            return category_validation;
        }

        let type_validation = validate_required_string(
            capability.r#type.as_deref(),
            "registration.capabilities.type.missing",
            "Capability type must be specified in registration.",
        );
        if !type_validation.passed {
            return type_validation;
        }
    }

    ValidationOutcome::pass()
}

/// The exact ICD version string a BSI Flex 335 v2.0 registration must declare.
/// Matches the legacy reference validator's rule
/// (`RegistrationValidator.cs`: `RuleFor(x => x.IcdVersion)...Equal("BSI Flex 335 v2.0")`).
const REQUIRED_ICD_VERSION: &str = "BSI Flex 335 v2.0";

/// Function to check the ICD version as specified in the BSI Flex 335 V2.0
fn validate_icd_version(icd_version: Option<String>) -> ValidationOutcome {
    match icd_version {
        Some(version) if version.is_empty() => ValidationOutcome::fail(
            "registration.icd_version.missing",
            "No ICD version specified in registration message",
        ),
        Some(version) if version == REQUIRED_ICD_VERSION => ValidationOutcome::pass(),
        Some(_) => ValidationOutcome::fail(
            "registration.icd_version.invalid",
            "ICD version specified in registration is not a valid option.",
        ),
        None => ValidationOutcome::fail(
            "registration.icd_version.missing",
            "No ICD version specified in registration message",
        ),
    }
}

/// Function to check the status definition as specified in the BSI Flex 335 V2.0
fn validate_status_definition(status_definition: StatusDefinition) -> ValidationOutcome {
    // Check the status interval
    if status_definition.status_interval.is_none() {
        return ValidationOutcome::fail(
            "registration.status_definition.status_interval.missing",
            "Status interval must be specified in status definition.",
        );
    }
    let valid_status_interval =
        validate_status_interval(status_definition.status_interval.unwrap());
    if !valid_status_interval.passed {
        return valid_status_interval;
    }

    if let Some(location_definition) = status_definition.location_definition {
        let validation = validate_location_type(location_definition);
        if !validation.passed {
            return validation;
        }
    }

    if let Some(coverage_definition) = status_definition.coverage_definition {
        let validation = validate_location_type(coverage_definition);
        if !validation.passed {
            return validation;
        }
    }

    if let Some(obscuration_definition) = status_definition.obscuration_definition {
        let validation = validate_location_type(obscuration_definition);
        if !validation.passed {
            return validation;
        }
    }

    if let Some(field_of_view_definition) = status_definition.field_of_view_definition {
        let validation = validate_location_type(field_of_view_definition);
        if !validation.passed {
            return validation;
        }
    }

    for status_report in status_definition.status_report {
        let validation = validate_status_report_definition(status_report);
        if !validation.passed {
            return validation;
        }
    }

    ValidationOutcome::pass()
}

/// Function to check the status interval as specified in the BSI Flex 335 V2.0
fn validate_status_interval(duration: Duration) -> ValidationOutcome {
    // Check the units
    let duration_units = validate_duration_units(duration.units);
    if !duration_units.passed {
        return duration_units;
    }

    // Check the value
    validate_duration_value(duration.value)
}

/// Function to check the duration units as specified in the BSI Flex 335 V2.0
///
/// Shared by every `Duration`-typed field (status interval, mode settle
/// time, region settle time, command completion time) -- the rule id
/// identifies the check ("a duration needs units"), not which specific
/// field embeds the `Duration`. See `src/finding.rs` for why this pass
/// doesn't thread full per-field context through shared primitives.
fn validate_duration_units(units: Option<i32>) -> ValidationOutcome {
    // `TimeUnits` is mandatory and has no reserved gaps (0-7).
    validate_required_enum(
        units,
        7,
        "registration.duration.units.missing",
        "Time Units must be specified.",
    )
}

/// Function to check the duration units as specified in the BSI Flex 335 V2.0
fn validate_duration_value(value: Option<f32>) -> ValidationOutcome {
    match value {
        None => ValidationOutcome::fail(
            "registration.duration.value.missing",
            "Duration value must be provided.",
        ),
        Some(v) if !v.is_finite() || v < 0.0 => ValidationOutcome::fail(
            "registration.duration.value.invalid",
            "Duration value must be a finite number 0 or greater.",
        ),
        Some(_) => ValidationOutcome::pass(),
    }
}

/// Function to check the mode definitions as specified in the BSI Flex 335 V2.0
fn validate_mode_definitions(mode_definitions: Vec<ModeDefinition>) -> ValidationOutcome {
    if mode_definitions.is_empty() {
        return ValidationOutcome::fail(
            "registration.mode_definition.empty",
            "Mode definition must be specified in registration.",
        );
    }

    // Duplicate mode names make the contract genuinely ambiguous: a
    // mode_change Task addresses a mode by name via case-sensitive exact
    // match (the same policy the session uses at runtime to look one up),
    // so two modes sharing a name means the second one can never be
    // addressed -- lookup would silently resolve to whichever was declared
    // first. Case-sensitive to match that lookup policy exactly; this is
    // separate from (and narrower in scope than) the case-insensitive
    // fallback that resolves an *unnamed* default mode by looking for one
    // named "default".
    let mut seen_names: HashSet<&str> = HashSet::new();
    for mode_definition in &mode_definitions {
        if let Some(name) = mode_definition.mode_name.as_deref()
            && !seen_names.insert(name)
        {
            // Message is a plain, single-line string literal (not
            // `format!`, and not backslash-continued onto a second line,
            // though the specific duplicated name is available above) so
            // `scripts/generate-rules.sh`'s single-line message regex can
            // extract it into RULES.md instead of falling back to "(see
            // source)".
            return ValidationOutcome::fail(
                "registration.mode_definition.mode_name.invalid",
                "Mode names must be unique so a mode_change Task can address each unambiguously.",
            );
        }
    }

    let mut validations = vec![];

    for mode_definition in mode_definitions {
        validations.push(validate_mode_definition(mode_definition));
    }

    for validation in validations {
        if !validation.passed {
            return validation;
        }
    }

    ValidationOutcome::pass()
}

/// Function to check the mode definition as specified in the BSI Flex 335 V2.0
fn validate_mode_definition(mode_definition: ModeDefinition) -> ValidationOutcome {
    let mut validations = vec![];

    // Check mode name
    validations.push(validate_mode_name(mode_definition.mode_name));

    // Check settle time
    if mode_definition.settle_time.is_none() {
        return ValidationOutcome::fail(
            "registration.mode_definition.settle_time.missing",
            "Settle time must be specified in mode definition.",
        );
    }
    validations.push(validate_settle_time(mode_definition.settle_time.unwrap()));

    for mode_parameter in mode_definition.mode_parameter {
        validations.push(validate_mode_parameter(mode_parameter));
    }
    for detection_definition in mode_definition.detection_definition {
        validations.push(validate_detection_definition(detection_definition));
    }

    // Check task definitions
    validations.push(validate_task_definition(mode_definition.task));

    // Check mode type
    validations.push(validate_mode_type(mode_definition.mode_type));

    for validation in validations {
        if !validation.passed {
            return validation;
        }
    }

    ValidationOutcome::pass()
}

/// Function to check the mode name as specified in the BSI Flex 335 V2.0
fn validate_mode_name(mode_name: Option<String>) -> ValidationOutcome {
    validate_required_string(
        mode_name.as_deref(),
        "registration.mode_definition.mode_name.missing",
        "Mode name must be specified in mode definition.",
    )
}

/// Function to check the settle time as specified in the BSI Flex 335 V2.0
fn validate_settle_time(settle_time: Duration) -> ValidationOutcome {
    // Check the units
    let duration_units = validate_duration_units(settle_time.units);
    if !duration_units.passed {
        return duration_units;
    }

    // Check the value
    validate_duration_value(settle_time.value)
}

fn validate_mode_parameter(mode_parameter: ModeParameter) -> ValidationOutcome {
    let type_validation = validate_required_string(
        mode_parameter.r#type.as_deref(),
        "registration.mode_parameter.type.missing",
        "Mode parameter type must be specified.",
    );
    if !type_validation.passed {
        return type_validation;
    }

    validate_required_string(
        mode_parameter.value.as_deref(),
        "registration.mode_parameter.value.missing",
        "Mode parameter value must be specified.",
    )
}

/// Function to check the detection definition as specified in the BSI Flex 335 V2.0
fn validate_detection_definition(detection_definition: DetectionDefinition) -> ValidationOutcome {
    // Check location type
    if detection_definition.location_type.is_none() {
        return ValidationOutcome::fail(
            "registration.detection_definition.location_type.missing",
            "Location type must be specified in detection definition.",
        );
    }
    let valid_location_type = validate_location_type(detection_definition.location_type.unwrap());
    if !valid_location_type.passed {
        return valid_location_type;
    }

    if let Some(geometric_error) = detection_definition.geometric_error {
        let validation = validate_geometric_error(geometric_error);
        if !validation.passed {
            return validation;
        }
    }

    if let Some(velocity_type) = detection_definition.velocity_type {
        let validation = validate_velocity_type(velocity_type);
        if !validation.passed {
            return validation;
        }
    }

    for detection_performance in detection_definition.detection_performance {
        let validation = validate_performance_value(detection_performance);
        if !validation.passed {
            return validation;
        }
    }

    for detection_report in detection_definition.detection_report {
        let validation = validate_detection_report_definition(detection_report);
        if !validation.passed {
            return validation;
        }
    }

    for detection_class_definition in detection_definition.detection_class_definition {
        let validation = validate_detection_class_definition(detection_class_definition);
        if !validation.passed {
            return validation;
        }
    }

    for behaviour_definition in detection_definition.behaviour_definition {
        let validation = validate_behaviour_definition(behaviour_definition);
        if !validation.passed {
            return validation;
        }
    }

    ValidationOutcome::pass()
}

/// Function to check location type as specified in the BSI Flex 335 V2.0
fn validate_location_type(location_type: LocationType) -> ValidationOutcome {
    // Check location type coorindates
    if location_type.coordinates_oneof.is_none() {
        return ValidationOutcome::fail(
            "registration.location_type.units.missing",
            "Units must be specified in location type.",
        );
    };
    let valid_units = match location_type.coordinates_oneof.unwrap() {
        LocationUnits(units) => validate_location_coordinate_system(
            Some(units),
            "registration.location_type.units.invalid",
            "Units must be specified in location type.",
        ),
        RangeBearingUnits(units) => validate_range_bearing_coordinate_system(
            Some(units),
            "registration.location_type.units.invalid",
            "Units must be specified in location type.",
        ),
    };
    if !valid_units.passed {
        return valid_units;
    }

    // Check location type datum
    if location_type.datum_oneof.is_none() {
        return ValidationOutcome::fail(
            "registration.location_type.datum.missing",
            "Datum must be specified in location type.",
        );
    };
    match location_type.datum_oneof.unwrap() {
        LocationDatum(datum) => validate_coordinate_datum(datum),
        RangeBearingDatum(datum) => validate_coordinate_datum(datum),
    }
}

/// Function to check coordinate datum as specified in the BSI Flex 335 V2.0
fn validate_coordinate_datum(datum: i32) -> ValidationOutcome {
    validate_nonzero(
        datum,
        "registration.location_type.datum.invalid",
        "Datum must be specified in location type.",
    )
}

/// Function to check the mode definition as specified in the BSI Flex 335 V2.0
fn validate_task_definition(task_definition: Option<TaskDefinition>) -> ValidationOutcome {
    match task_definition {
        None => ValidationOutcome::fail(
            "registration.task_definition.missing",
            "Task definition must be populated",
        ),
        Some(task_def) => {
            match task_def.concurrent_tasks {
                None => {
                    return ValidationOutcome::fail(
                        "registration.task_definition.concurrent_tasks.missing",
                        "Concurrent tasks must be specified in task definition.",
                    );
                }
                Some(concurrent_tasks) if concurrent_tasks < 0 => {
                    return ValidationOutcome::fail(
                        "registration.task_definition.concurrent_tasks.invalid",
                        "Concurrent tasks must be 0 or greater.",
                    );
                }
                Some(_) => {}
            }

            // Check region definition
            if task_def.region_definition.is_none() {
                return ValidationOutcome::fail(
                    "registration.task_definition.region_definition.missing",
                    "Region definition must be specified in task definition.",
                );
            }
            let valid_region_definition =
                validate_region_definition(task_def.region_definition.unwrap());
            if !valid_region_definition.passed {
                return valid_region_definition;
            }

            for command in task_def.command {
                let validation = validate_command_definition(command);
                if !validation.passed {
                    return validation;
                }
            }

            ValidationOutcome::pass()
        }
    }
}

/// Function to check region definition as specified in the BSI Flex 335 V2.0
fn validate_region_definition(region_definition: RegionDefinition) -> ValidationOutcome {
    // Check region type
    if region_definition.region_type.is_empty() {
        return ValidationOutcome::fail(
            "registration.region_definition.region_type.empty",
            "Region type must be specified in region definition.",
        );
    }
    // `RegionType` has no reserved gaps (0-5).
    for region_type in region_definition.region_type {
        if !(1..=5).contains(&region_type) {
            return ValidationOutcome::fail(
                "registration.region_definition.region_type.invalid",
                "Region type must be specified in region definition.",
            );
        }
    }

    if let Some(settle_time) = region_definition.settle_time {
        let validation = validate_settle_time(settle_time);
        if !validation.passed {
            return validation;
        }
    }

    if region_definition.region_area.is_empty() {
        return ValidationOutcome::fail(
            "registration.region_definition.region_area.empty",
            "Region area must be specified in region definition.",
        );
    }

    // Check location type
    for region_area in region_definition.region_area {
        let valid_region_area = validate_location_type(region_area);
        if !valid_region_area.passed {
            return valid_region_area;
        }
    }

    for class_filter_definition in region_definition.class_filter_definition {
        let validation = validate_class_filter_definition(class_filter_definition);
        if !validation.passed {
            return validation;
        }
    }

    for behaviour_filter_definition in region_definition.behaviour_filter_definition {
        let validation = validate_behaviour_filter_definition(behaviour_filter_definition);
        if !validation.passed {
            return validation;
        }
    }

    ValidationOutcome::pass()
}

/// Function to check the mode definition as specified in the BSI Flex 335 V2.0
fn validate_mode_type(mode_type: Option<i32>) -> ValidationOutcome {
    // `ModeType` is mandatory and has no reserved gaps (0-3).
    validate_required_enum(
        mode_type,
        3,
        "registration.mode_definition.mode_type.missing",
        "Mode type must be specified.",
    )
}

fn validate_status_report_definition(
    status_report: crate::bsi_flex_335_v2_0::registration::StatusReport,
) -> ValidationOutcome {
    // `StatusReportCategory` is mandatory and has no reserved gaps (0-4).
    let category_validation = validate_required_enum(
        status_report.category,
        4,
        "registration.status_definition.status_report.category.missing",
        "Status report category must be specified in registration.",
    );
    if !category_validation.passed {
        return category_validation;
    }

    validate_required_string(
        status_report.r#type.as_deref(),
        "registration.status_definition.status_report.type.missing",
        "Status report type must be specified in registration.",
    )
}

fn validate_geometric_error(geometric_error: GeometricError) -> ValidationOutcome {
    match geometric_error.r#type.as_deref() {
        Some("") | None => {
            return ValidationOutcome::fail(
                "registration.geometric_error.type.missing",
                "Geometric error type must be specified.",
            );
        }
        Some(_) => {}
    }

    match geometric_error.units.as_deref() {
        Some("") | None => {
            return ValidationOutcome::fail(
                "registration.geometric_error.units.missing",
                "Geometric error units must be specified.",
            );
        }
        Some(_) => {}
    }

    match geometric_error.variation_type.as_deref() {
        Some("") | None => {
            return ValidationOutcome::fail(
                "registration.geometric_error.variation_type.missing",
                "Geometric error variation type must be specified.",
            );
        }
        Some(_) => {}
    }

    for performance_value in geometric_error.performance_value {
        let validation = validate_performance_value(performance_value);
        if !validation.passed {
            return validation;
        }
    }

    ValidationOutcome::pass()
}

fn validate_performance_value(performance_value: PerformanceValue) -> ValidationOutcome {
    match performance_value.r#type.as_deref() {
        Some("") | None => {
            return ValidationOutcome::fail(
                "registration.performance_value.type.missing",
                "Performance value type must be specified.",
            );
        }
        Some(_) => {}
    }

    match performance_value.units.as_deref() {
        Some("") | None => {
            return ValidationOutcome::fail(
                "registration.performance_value.units.missing",
                "Performance value units must be specified.",
            );
        }
        Some(_) => {}
    }

    match performance_value.unit_value.as_deref() {
        Some("") | None => {
            return ValidationOutcome::fail(
                "registration.performance_value.unit_value.missing",
                "Performance value unit value must be specified.",
            );
        }
        Some(_) => {}
    }

    ValidationOutcome::pass()
}

fn validate_detection_report_definition(
    detection_report: crate::bsi_flex_335_v2_0::registration::DetectionReport,
) -> ValidationOutcome {
    // `DetectionReportCategory` is mandatory and has no reserved gaps (0-4).
    let category_validation = validate_required_enum(
        detection_report.category,
        4,
        "registration.detection_report.category.missing",
        "Detection report category must be specified.",
    );
    if !category_validation.passed {
        return category_validation;
    }

    match detection_report.r#type.as_deref() {
        Some("") | None => {
            return ValidationOutcome::fail(
                "registration.detection_report.type.missing",
                "Detection report type must be specified.",
            );
        }
        Some(_) => {}
    }

    match detection_report.units.as_deref() {
        Some("") | None => {
            return ValidationOutcome::fail(
                "registration.detection_report.units.missing",
                "Detection report units must be specified.",
            );
        }
        Some(_) => {}
    }

    ValidationOutcome::pass()
}

fn validate_detection_class_definition(
    detection_class_definition: DetectionClassDefinition,
) -> ValidationOutcome {
    // `ConfidenceDefinition` is optional and has no reserved gaps (0-2).
    let confidence_definition_validation = validate_optional_enum(
        detection_class_definition.confidence_definition,
        2,
        "registration.detection_class_definition.confidence_definition.invalid",
        "Confidence definition is not a valid option in detection class definition.",
    );
    if !confidence_definition_validation.passed {
        return confidence_definition_validation;
    }

    for class_performance in detection_class_definition.class_performance {
        let validation = validate_performance_value(class_performance);
        if !validation.passed {
            return validation;
        }
    }

    for class_definition in detection_class_definition.class_definition {
        let validation = validate_class_definition(class_definition);
        if !validation.passed {
            return validation;
        }
    }

    for taxonomy_dock_definition in detection_class_definition.taxonomy_dock_definition {
        let validation = validate_taxonomy_dock_definition(taxonomy_dock_definition);
        if !validation.passed {
            return validation;
        }
    }

    ValidationOutcome::pass()
}

fn validate_class_definition(class_definition: ClassDefinition) -> ValidationOutcome {
    match class_definition.r#type.as_deref() {
        Some("") | None => {
            return ValidationOutcome::fail(
                "registration.class_definition.type.missing",
                "Class definition type must be specified.",
            );
        }
        Some(_) => {}
    }

    for sub_class in class_definition.sub_class {
        let validation = validate_sub_class_definition(sub_class);
        if !validation.passed {
            return validation;
        }
    }

    ValidationOutcome::pass()
}

fn validate_sub_class_definition(sub_class: SubClass) -> ValidationOutcome {
    match sub_class.r#type.as_deref() {
        Some("") | None => {
            return ValidationOutcome::fail(
                "registration.sub_class.type.missing",
                "Sub class type must be specified.",
            );
        }
        Some(_) => {}
    }

    if sub_class.level.is_none() {
        return ValidationOutcome::fail(
            "registration.sub_class.level.missing",
            "Sub class level must be specified.",
        );
    }

    for nested_sub_class in sub_class.sub_class {
        let validation = validate_sub_class_definition(nested_sub_class);
        if !validation.passed {
            return validation;
        }
    }

    ValidationOutcome::pass()
}

fn validate_behaviour_definition(behaviour_definition: BehaviourDefinition) -> ValidationOutcome {
    match behaviour_definition.r#type.as_deref() {
        Some("") | None => ValidationOutcome::fail(
            "registration.behaviour_definition.type.missing",
            "Behaviour definition type must be specified.",
        ),
        Some(_) => ValidationOutcome::pass(),
    }
}

fn validate_velocity_type(velocity_type: VelocityType) -> ValidationOutcome {
    let velocity_units = match velocity_type.velocity_units_oneof {
        Some(EnuVelocityUnits(enu_velocity_units)) => enu_velocity_units,
        None => {
            return ValidationOutcome::fail(
                "registration.velocity_type.velocity_units.missing",
                "Velocity units must be specified in velocity type.",
            );
        }
    };

    let units_validation = validate_enu_velocity_units(velocity_units);
    if !units_validation.passed {
        return units_validation;
    }

    if velocity_type.datum_oneof.is_none() {
        return ValidationOutcome::fail(
            "registration.velocity_type.datum.missing",
            "Datum must be specified in velocity type.",
        );
    }

    ValidationOutcome::pass()
}

/// `SpeedUnits` values 3 and 4 are `reserved` in `velocity.proto` (used up
/// to SAPIENT v7, dropped for non-SI units) -- still legal `int32`s on the
/// wire, so must be explicitly excluded rather than just checked for
/// nonzero.
fn validate_speed_units(
    value: Option<i32>,
    rule_id: impl Into<String>,
    error_message: &str,
) -> ValidationOutcome {
    match value {
        Some(v) if [1, 2].contains(&v) => ValidationOutcome::pass(),
        _ => ValidationOutcome::fail(rule_id, error_message),
    }
}

fn validate_enu_velocity_units(
    enu_velocity_units: RegistrationEnuVelocityUnits,
) -> ValidationOutcome {
    let east_north_validation = validate_speed_units(
        enu_velocity_units.east_north_rate_units,
        "registration.enu_velocity_units.east_north_rate_units.missing",
        "East/north rate units must be specified in velocity type.",
    );
    if !east_north_validation.passed {
        return east_north_validation;
    }

    if let Some(up_rate_units) = enu_velocity_units.up_rate_units {
        let up_rate_validation = validate_speed_units(
            Some(up_rate_units),
            "registration.enu_velocity_units.up_rate_units.invalid",
            "Up rate units is not a valid option in velocity type.",
        );
        if !up_rate_validation.passed {
            return up_rate_validation;
        }
    }

    ValidationOutcome::pass()
}

fn validate_command_definition(command: Command) -> ValidationOutcome {
    match command.units.as_deref() {
        Some("") | None => {
            return ValidationOutcome::fail(
                "registration.command.units.missing",
                "Command units must be specified.",
            );
        }
        Some(_) => {}
    }

    if command.completion_time.is_none() {
        return ValidationOutcome::fail(
            "registration.command.completion_time.missing",
            "Command completion time must be specified.",
        );
    }
    let completion_time_validation = validate_settle_time(command.completion_time.unwrap());
    if !completion_time_validation.passed {
        return completion_time_validation;
    }

    // `CommandType` is mandatory and has no reserved gaps (0-9).
    validate_required_enum(
        command.r#type,
        9,
        "registration.command.type.missing",
        "Command type must be specified.",
    )
}

fn validate_class_filter_definition(
    class_filter_definition: ClassFilterDefinition,
) -> ValidationOutcome {
    match class_filter_definition.r#type.as_deref() {
        Some("") | None => {
            return ValidationOutcome::fail(
                "registration.class_filter_definition.type.missing",
                "Class filter definition type must be specified.",
            );
        }
        Some(_) => {}
    }

    for filter_parameter in class_filter_definition.filter_parameter {
        let validation = validate_filter_parameter(filter_parameter);
        if !validation.passed {
            return validation;
        }
    }

    for sub_class_definition in class_filter_definition.sub_class_definition {
        let validation = validate_sub_class_filter_definition(sub_class_definition);
        if !validation.passed {
            return validation;
        }
    }

    ValidationOutcome::pass()
}

fn validate_sub_class_filter_definition(
    sub_class_filter_definition: SubClassFilterDefinition,
) -> ValidationOutcome {
    if sub_class_filter_definition.level.is_none() {
        return ValidationOutcome::fail(
            "registration.sub_class_filter_definition.level.missing",
            "Sub class filter definition level must be specified.",
        );
    }

    match sub_class_filter_definition.r#type.as_deref() {
        Some("") | None => {
            return ValidationOutcome::fail(
                "registration.sub_class_filter_definition.type.missing",
                "Sub class filter definition type must be specified.",
            );
        }
        Some(_) => {}
    }

    for filter_parameter in sub_class_filter_definition.filter_parameter {
        let validation = validate_filter_parameter(filter_parameter);
        if !validation.passed {
            return validation;
        }
    }

    for nested_sub_class_definition in sub_class_filter_definition.sub_class_definition {
        let validation = validate_sub_class_filter_definition(nested_sub_class_definition);
        if !validation.passed {
            return validation;
        }
    }

    ValidationOutcome::pass()
}

fn validate_filter_parameter(filter_parameter: FilterParameter) -> ValidationOutcome {
    match filter_parameter.parameter.as_deref() {
        Some("") | None => {
            return ValidationOutcome::fail(
                "registration.filter_parameter.parameter.missing",
                "Filter parameter name must be specified.",
            );
        }
        Some(_) => {}
    }

    // `Operator` has no reserved gaps (0-4).
    if filter_parameter.operators.is_empty()
        || filter_parameter
            .operators
            .iter()
            .any(|operator| !(1..=4).contains(operator))
    {
        return ValidationOutcome::fail(
            "registration.filter_parameter.operators.invalid",
            "Filter parameter operators must be specified.",
        );
    }

    ValidationOutcome::pass()
}

fn validate_behaviour_filter_definition(
    behaviour_filter_definition: BehaviourFilterDefinition,
) -> ValidationOutcome {
    match behaviour_filter_definition.r#type.as_deref() {
        Some("") | None => {
            return ValidationOutcome::fail(
                "registration.behaviour_filter_definition.type.missing",
                "Behaviour filter definition type must be specified.",
            );
        }
        Some(_) => {}
    }

    for filter_parameter in behaviour_filter_definition.filter_parameter {
        let validation = validate_filter_parameter(filter_parameter);
        if !validation.passed {
            return validation;
        }
    }

    ValidationOutcome::pass()
}

fn validate_taxonomy_dock_definition(
    taxonomy_dock_definition: TaxonomyDockDefinition,
) -> ValidationOutcome {
    match taxonomy_dock_definition.dock_class_namespace.as_deref() {
        Some("") | None => {
            return ValidationOutcome::fail(
                "registration.taxonomy_dock_definition.dock_class_namespace.missing",
                "Taxonomy dock class namespace must be specified.",
            );
        }
        Some(_) => {}
    }

    match taxonomy_dock_definition.dock_class.as_deref() {
        Some("") | None => {
            return ValidationOutcome::fail(
                "registration.taxonomy_dock_definition.dock_class.missing",
                "Taxonomy dock class must be specified.",
            );
        }
        Some(_) => {}
    }

    for extension_subclass in taxonomy_dock_definition.extension_subclass {
        let validation = validate_extension_subclass(extension_subclass);
        if !validation.passed {
            return validation;
        }
    }

    ValidationOutcome::pass()
}

fn validate_extension_subclass(extension_subclass: ExtensionSubclass) -> ValidationOutcome {
    match extension_subclass.subclass_namespace.as_deref() {
        Some("") | None => {
            return ValidationOutcome::fail(
                "registration.extension_subclass.subclass_namespace.missing",
                "Extension subclass namespace must be specified.",
            );
        }
        Some(_) => {}
    }

    match extension_subclass.subclass_name.as_deref() {
        Some("") | None => ValidationOutcome::fail(
            "registration.extension_subclass.subclass_name.missing",
            "Extension subclass name must be specified.",
        ),
        Some(_) => ValidationOutcome::pass(),
    }
}

fn validate_dependent_nodes(dependent_nodes: Vec<String>) -> ValidationOutcome {
    for dependent_node in dependent_nodes {
        let validation = validate_uuid_v4(
            Some(dependent_node.as_str()),
            "registration.dependent_nodes.invalid",
            "A valid UUID v4 must be used for a dependent node ID in registration.",
        );
        if !validation.passed {
            return validation;
        }
    }

    ValidationOutcome::pass()
}

fn validate_reporting_region(
    reporting_region: Vec<crate::bsi_flex_335_v2_0::LocationOrRangeBearing>,
) -> ValidationOutcome {
    for region in reporting_region {
        let validation = validate_common_location_or_range_bearing(
            region,
            "registration.reporting_region",
            "Location or range-bearing must be specified in reporting region.",
        );
        if !validation.passed {
            return validation;
        }
    }

    ValidationOutcome::pass()
}

fn validate_config_data(config_data: Vec<ConfigurationData>) -> ValidationOutcome {
    if config_data.is_empty() {
        return ValidationOutcome::fail(
            "registration.config_data.empty",
            "Configuration data must be specified in registration.",
        );
    }

    for configuration_data in config_data {
        let validation = validate_configuration_data_entry(configuration_data);
        if !validation.passed {
            return validation;
        }
    }

    ValidationOutcome::pass()
}

fn validate_configuration_data_entry(configuration_data: ConfigurationData) -> ValidationOutcome {
    if configuration_data.manufacturer.is_empty() {
        return ValidationOutcome::fail(
            "registration.config_data.manufacturer.missing",
            "Configuration data manufacturer must be specified.",
        );
    }

    if configuration_data.model.is_empty() {
        return ValidationOutcome::fail(
            "registration.config_data.model.missing",
            "Configuration data model must be specified.",
        );
    }

    for sub_component in configuration_data.sub_components {
        let validation = validate_configuration_data_entry(sub_component);
        if !validation.passed {
            return validation;
        }
    }

    ValidationOutcome::pass()
}

#[cfg(test)]
mod registration_validation_tests {
    use crate::bsi_flex_335_v2_0::registration::location_type::{CoordinatesOneof, DatumOneof};
    use crate::finding::ValidationOutcome;
    use crate::validation::registration::*;

    /// Unit test to check that registration messages are correctly validated
    #[test]
    fn test_registrations_validation() {
        // valid registration
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
        let settle_time = Duration {
            units: Some(1),
            value: Some(1.0),
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
            detection_definition: vec![detection_definition.clone()],
            mode_name: Some("Default".to_string()),
            mode_parameter: vec![],
            mode_description: None,
            mode_type: Some(1),
            scan_type: None,
            settle_time: Some(settle_time),
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
        let valid_registration = Registration {
            icd_version: Some("BSI Flex 335 v2.0".to_string()),
            node_definition: vec![valid_node_definition.clone()],
            capabilities: vec![Capability {
                category: Some("Radar".to_string()),
                r#type: Some("Range".to_string()),
                value: None,
                units: None,
            }],
            short_name: None,
            name: None,
            status_definition: Some(valid_status_definition.clone()),
            mode_definition: vec![valid_mode_definition.clone()],
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
        };
        assert_eq!(
            ValidationOutcome::pass(),
            validate_registration(valid_registration)
        );

        // invalid registration
        let invalid_registration = Registration {
            icd_version: Some("0".to_string()),
            node_definition: vec![valid_node_definition.clone()],
            capabilities: vec![Capability {
                category: Some("Radar".to_string()),
                r#type: Some("Range".to_string()),
                value: None,
                units: None,
            }],
            short_name: None,
            name: None,
            status_definition: Some(valid_status_definition.clone()),
            mode_definition: vec![valid_mode_definition.clone()],
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
        };
        assert_eq!(
            ValidationOutcome::fail(
                "registration.icd_version.invalid",
                "ICD version specified in registration is not a valid option."
            ),
            validate_registration(invalid_registration)
        );
    }

    /// Unit test to check that node definitions are correctly validated
    #[test]
    fn test_node_definitions_validation() {
        // valid node definition
        let valid_node_definition = NodeDefinition {
            node_type: Some(1),
            node_sub_type: vec![],
        };
        assert_eq!(
            ValidationOutcome::pass(),
            validate_node_definition(vec![valid_node_definition])
        );

        // invalid node definition
        let invalid_node_definition = NodeDefinition {
            node_type: None,
            node_sub_type: vec![],
        };
        assert_eq!(
            ValidationOutcome::fail(
                "registration.node_definition.node_type.missing",
                "Node type must be specified in node defintition."
            ),
            validate_node_definition(vec![invalid_node_definition])
        );

        // missing node definition
        assert_eq!(
            ValidationOutcome::fail(
                "registration.node_definition.empty",
                "Node type must be specified in node defintition."
            ),
            validate_node_definition(vec![])
        );

        // undefined discriminant -- not just nonzero, must be one of the
        // v2.0-defined NodeType values.
        let undefined_node_type = NodeDefinition {
            node_type: Some(999),
            node_sub_type: vec![],
        };
        assert_eq!(
            ValidationOutcome::fail(
                "registration.node_definition.node_type.missing",
                "Node type must be specified in node defintition."
            ),
            validate_node_definition(vec![undefined_node_type])
        );
    }

    /// Unit test to check that ICD versions are correctly validated
    #[test]
    fn test_icd_versions_validation() {
        // valid version
        assert_eq!(
            ValidationOutcome::pass(),
            validate_icd_version(Some("BSI Flex 335 v2.0".to_string()))
        );

        // invalid version
        assert_eq!(
            ValidationOutcome::fail(
                "registration.icd_version.invalid",
                "ICD version specified in registration is not a valid option."
            ),
            validate_icd_version(Some("BSI Flex 335 v1.0".to_string()))
        );
    }

    /// Unit test to check that status definitions are correctly validated
    #[test]
    fn test_status_definitions_validation() {
        // valid status definition
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
        assert_eq!(
            ValidationOutcome::pass(),
            validate_status_definition(valid_status_definition)
        );

        // missing status interval
        let invalid_status_definition = StatusDefinition {
            coverage_definition: None,
            field_of_view_definition: None,
            location_definition: None,
            obscuration_definition: None,
            status_report: vec![],
            status_interval: None,
        };
        assert_eq!(
            ValidationOutcome::fail(
                "registration.status_definition.status_interval.missing",
                "Status interval must be specified in status definition."
            ),
            validate_status_definition(invalid_status_definition)
        );

        // invalid duration value
        let missing_value_duration = Duration {
            units: Some(1),
            value: Some(-1.0),
        };
        let missing_value_status_definition = StatusDefinition {
            coverage_definition: None,
            field_of_view_definition: None,
            location_definition: None,
            obscuration_definition: None,
            status_report: vec![],
            status_interval: Some(missing_value_duration),
        };
        assert_eq!(
            ValidationOutcome::fail(
                "registration.duration.value.invalid",
                "Duration value must be a finite number 0 or greater."
            ),
            validate_status_definition(missing_value_status_definition)
        );
    }

    /// Unit test to check that status intervals are correctly validated
    #[test]
    fn test_status_intervals_validation() {
        // valid status interval
        let valid_duration = Duration {
            units: Some(1),
            value: Some(1.0),
        };
        assert_eq!(
            ValidationOutcome::pass(),
            validate_status_interval(valid_duration)
        );

        // missing units
        let missing_units_duration = Duration {
            units: None,
            value: Some(1.0),
        };
        assert_eq!(
            ValidationOutcome::fail(
                "registration.duration.units.missing",
                "Time Units must be specified."
            ),
            validate_status_interval(missing_units_duration)
        );

        // missing value
        let missing_value_duration = Duration {
            units: Some(1),
            value: None,
        };
        assert_eq!(
            ValidationOutcome::fail(
                "registration.duration.value.missing",
                "Duration value must be provided."
            ),
            validate_status_interval(missing_value_duration)
        );
    }

    /// Unit test to check that duration units are correctly validated
    #[test]
    fn test_duration_units_validation() {
        // valid units
        for i in 1..6 {
            assert_eq!(ValidationOutcome::pass(), validate_duration_units(Some(i)));
        }

        // invalid units
        assert_eq!(
            ValidationOutcome::fail(
                "registration.duration.units.missing",
                "Time Units must be specified."
            ),
            validate_duration_units(Some(0))
        );
    }

    /// Unit test to check that duration values are correctly validated
    #[test]
    fn test_duration_values_validation() {
        // valid values
        for i in 0..500 {
            assert_eq!(
                ValidationOutcome::pass(),
                validate_duration_value(Some(i as f32))
            );
        }

        // invalid values
        assert_eq!(
            ValidationOutcome::fail(
                "registration.duration.value.invalid",
                "Duration value must be a finite number 0 or greater."
            ),
            validate_duration_value(Some(-1.0))
        );

        // missing values
        assert_eq!(
            ValidationOutcome::fail(
                "registration.duration.value.missing",
                "Duration value must be provided."
            ),
            validate_duration_value(None)
        );
    }

    #[test]
    fn test_geometric_error_validation() {
        let valid_geometric_error = GeometricError {
            r#type: Some("standard deviation".to_string()),
            units: Some("m".to_string()),
            variation_type: Some("linear".to_string()),
            performance_value: vec![PerformanceValue {
                r#type: Some("range".to_string()),
                units: Some("m".to_string()),
                unit_value: Some("10".to_string()),
                variation_type: None,
            }],
        };
        assert_eq!(
            ValidationOutcome::pass(),
            validate_geometric_error(valid_geometric_error)
        );

        let nested_invalid_performance_value = GeometricError {
            r#type: Some("standard deviation".to_string()),
            units: Some("m".to_string()),
            variation_type: Some("linear".to_string()),
            performance_value: vec![PerformanceValue {
                r#type: None,
                units: Some("m".to_string()),
                unit_value: Some("10".to_string()),
                variation_type: None,
            }],
        };
        assert_eq!(
            ValidationOutcome::fail(
                "registration.performance_value.type.missing",
                "Performance value type must be specified."
            ),
            validate_geometric_error(nested_invalid_performance_value)
        );
    }

    /// Unit test to check that multiple mode definitions are correctly validated
    #[test]
    fn test_mode_definitions_validation() {
        let settle_time = Duration {
            units: Some(1),
            value: Some(1.0),
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
        // valid mode definition
        let valid_mode_definition = ModeDefinition {
            duration: None,
            maximum_latency: None,
            detection_definition: vec![detection_definition.clone()],
            mode_name: Some("Default".to_string()),
            mode_parameter: vec![],
            mode_description: None,
            mode_type: Some(1),
            scan_type: None,
            settle_time: Some(settle_time),
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
        assert_eq!(
            ValidationOutcome::pass(),
            validate_mode_definitions(vec![valid_mode_definition])
        );

        // invalid mode definition
        let invalid_mode_definition = ModeDefinition {
            duration: None,
            maximum_latency: None,
            detection_definition: vec![detection_definition.clone()],
            mode_name: Some("".to_string()),
            mode_parameter: vec![],
            mode_description: None,
            mode_type: Some(1),
            scan_type: None,
            settle_time: Some(settle_time),
            task: None,
            tracking_type: None,
        };
        assert_eq!(
            ValidationOutcome::fail(
                "registration.mode_definition.mode_name.missing",
                "Mode name must be specified in mode definition."
            ),
            validate_mode_definitions(vec![invalid_mode_definition])
        );
    }

    /// Two modes with the same name make the contract genuinely ambiguous
    /// (a mode_change Task can't address the second one), and duplicate
    /// mode names are the real-world case Tom (the standard's principal
    /// author) has actually seen from suppliers -- confirmed rejected
    /// rather than a case-insensitive or lenient policy.
    #[test]
    fn test_duplicate_mode_names_are_rejected() {
        let settle_time = Duration {
            units: Some(1),
            value: Some(1.0),
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
        let mode = |name: &str| ModeDefinition {
            duration: None,
            maximum_latency: None,
            detection_definition: vec![detection_definition.clone()],
            mode_name: Some(name.to_string()),
            mode_parameter: vec![],
            mode_description: None,
            mode_type: Some(1),
            scan_type: None,
            settle_time: Some(settle_time),
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

        // Identical names, otherwise-conflicting definitions (mode_type
        // differs) -- still rejected on the name collision alone.
        let mut second_mode_conflicting = mode("Default");
        second_mode_conflicting.mode_type = Some(3);
        assert_eq!(
            ValidationOutcome::fail(
                "registration.mode_definition.mode_name.invalid",
                "Mode names must be unique so a mode_change Task can address each unambiguously."
            ),
            validate_mode_definitions(vec![mode("Default"), second_mode_conflicting])
        );

        // Names differing only by case are distinct under the
        // case-sensitive exact-match lookup policy, so not a collision.
        assert_eq!(
            ValidationOutcome::pass(),
            validate_mode_definitions(vec![mode("Default"), mode("default")])
        );

        // Distinct names remain valid.
        assert_eq!(
            ValidationOutcome::pass(),
            validate_mode_definitions(vec![mode("Default"), mode("Alternate")])
        );
    }

    /// Unit test to check that mode definitions are correctly validated
    #[test]
    fn test_mode_definition_validation() {
        let settle_time = Duration {
            units: Some(1),
            value: Some(1.0),
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
        // valid mode definition
        let valid_mode_definition = ModeDefinition {
            duration: None,
            maximum_latency: None,
            detection_definition: vec![detection_definition.clone()],
            mode_name: Some("Default".to_string()),
            mode_parameter: vec![],
            mode_description: None,
            mode_type: Some(1),
            scan_type: None,
            settle_time: Some(settle_time),
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
        assert_eq!(
            ValidationOutcome::pass(),
            validate_mode_definition(valid_mode_definition)
        );

        // invalid mode definition
        let invalid_mode_definition = ModeDefinition {
            duration: None,
            maximum_latency: None,
            detection_definition: vec![detection_definition.clone()],
            mode_name: Some("".to_string()),
            mode_parameter: vec![],
            mode_description: None,
            mode_type: Some(1),
            scan_type: None,
            settle_time: Some(settle_time),
            task: None,
            tracking_type: None,
        };
        assert_eq!(
            ValidationOutcome::fail(
                "registration.mode_definition.mode_name.missing",
                "Mode name must be specified in mode definition."
            ),
            validate_mode_definition(invalid_mode_definition)
        );

        // missing settle time
        let missing_settle_time_mode_definition = ModeDefinition {
            duration: None,
            maximum_latency: None,
            detection_definition: vec![detection_definition.clone()],
            mode_name: Some("Default".to_string()),
            mode_parameter: vec![],
            mode_description: None,
            mode_type: Some(1),
            scan_type: None,
            settle_time: None,
            task: None,
            tracking_type: None,
        };
        assert_eq!(
            ValidationOutcome::fail(
                "registration.mode_definition.settle_time.missing",
                "Settle time must be specified in mode definition."
            ),
            validate_mode_definition(missing_settle_time_mode_definition)
        );

        // empty detection definitions are valid in the v2 proto because the field is not mandatory
        let mode_definition_without_detection_definitions = ModeDefinition {
            duration: None,
            maximum_latency: None,
            detection_definition: vec![],
            mode_name: Some("Default".to_string()),
            mode_parameter: vec![],
            mode_description: None,
            mode_type: Some(1),
            scan_type: None,
            settle_time: Some(settle_time),
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
        assert_eq!(
            ValidationOutcome::pass(),
            validate_mode_definition(mode_definition_without_detection_definitions)
        );
    }

    /// Unit test to check that mode names are correctly validated
    #[test]
    fn test_mode_name_validation() {
        // valid mode name
        assert_eq!(
            ValidationOutcome::pass(),
            validate_mode_name(Some("Default".to_string()))
        );

        // invalid mode name
        assert_eq!(
            ValidationOutcome::fail(
                "registration.mode_definition.mode_name.missing",
                "Mode name must be specified in mode definition."
            ),
            validate_mode_name(Some("".to_string()))
        );
    }

    /// Unit test to check that settle times are correctly validated
    #[test]
    fn test_settle_time_validation() {
        // valid settle time
        let valid_duration = Duration {
            units: Some(1),
            value: Some(1.0),
        };
        assert_eq!(
            ValidationOutcome::pass(),
            validate_settle_time(valid_duration)
        );

        // missing units
        let missing_units_duration = Duration {
            units: None,
            value: Some(1.0),
        };
        assert_eq!(
            ValidationOutcome::fail(
                "registration.duration.units.missing",
                "Time Units must be specified."
            ),
            validate_settle_time(missing_units_duration)
        );

        // invalid value
        let missing_value_duration = Duration {
            units: Some(1),
            value: Some(-1.0),
        };
        assert_eq!(
            ValidationOutcome::fail(
                "registration.duration.value.invalid",
                "Duration value must be a finite number 0 or greater."
            ),
            validate_settle_time(missing_value_duration)
        );
    }

    /// Unit test to check that detection definitions are correctly validated
    #[test]
    fn test_detection_definition_validation() {
        // valid detection definition
        let valid_location_type = LocationType {
            coordinates_oneof: Some(CoordinatesOneof::RangeBearingUnits(1)),
            datum_oneof: Some(DatumOneof::RangeBearingDatum(1)),
            zone: None,
        };
        let valid_detection_definition = DetectionDefinition {
            behaviour_definition: vec![],
            detection_performance: vec![],
            detection_class_definition: vec![],
            detection_report: vec![],
            geometric_error: None,
            velocity_type: None,
            location_type: Some(valid_location_type),
        };
        assert_eq!(
            ValidationOutcome::pass(),
            validate_detection_definition(valid_detection_definition)
        );

        // missing location type
        let missing_location_type_detection_definition = DetectionDefinition {
            behaviour_definition: vec![],
            detection_performance: vec![],
            detection_class_definition: vec![],
            detection_report: vec![],
            geometric_error: None,
            velocity_type: None,
            location_type: None,
        };
        assert_eq!(
            ValidationOutcome::fail(
                "registration.detection_definition.location_type.missing",
                "Location type must be specified in detection definition."
            ),
            validate_detection_definition(missing_location_type_detection_definition)
        );

        // invliad detection definition
        let missing_datum_location_type = LocationType {
            coordinates_oneof: Some(CoordinatesOneof::RangeBearingUnits(1)),
            datum_oneof: None,
            zone: None,
        };
        let invalid_detection_definition = DetectionDefinition {
            behaviour_definition: vec![],
            detection_performance: vec![],
            detection_class_definition: vec![],
            detection_report: vec![],
            geometric_error: None,
            velocity_type: None,
            location_type: Some(missing_datum_location_type),
        };
        assert_eq!(
            ValidationOutcome::fail(
                "registration.location_type.datum.missing",
                "Datum must be specified in location type."
            ),
            validate_detection_definition(invalid_detection_definition)
        );
    }

    /// Unit test to check that location types are correctly validated
    #[test]
    fn test_location_type_validation() {
        // valid location type
        let valid_location_type = LocationType {
            coordinates_oneof: Some(CoordinatesOneof::RangeBearingUnits(1)),
            datum_oneof: Some(DatumOneof::RangeBearingDatum(1)),
            zone: None,
        };
        assert_eq!(
            ValidationOutcome::pass(),
            validate_location_type(valid_location_type)
        );

        // missing units
        let missing_units_location_type = LocationType {
            coordinates_oneof: None,
            datum_oneof: Some(DatumOneof::RangeBearingDatum(1)),
            zone: None,
        };
        assert_eq!(
            ValidationOutcome::fail(
                "registration.location_type.units.missing",
                "Units must be specified in location type."
            ),
            validate_location_type(missing_units_location_type)
        );

        // missing datum
        let missing_datum_location_type = LocationType {
            coordinates_oneof: Some(CoordinatesOneof::RangeBearingUnits(1)),
            datum_oneof: None,
            zone: None,
        };
        assert_eq!(
            ValidationOutcome::fail(
                "registration.location_type.datum.missing",
                "Datum must be specified in location type."
            ),
            validate_location_type(missing_datum_location_type)
        );

        // invalid location type
        let invalid_location_type = LocationType {
            coordinates_oneof: Some(CoordinatesOneof::LocationUnits(0)),
            datum_oneof: Some(DatumOneof::LocationDatum(1)),
            zone: None,
        };
        assert_eq!(
            ValidationOutcome::fail(
                "registration.location_type.units.invalid",
                "Units must be specified in location type."
            ),
            validate_location_type(invalid_location_type)
        );

        // mis-matched range bearing and location
        // TODO:
    }

    /// Unit test to check that location coordinate units are correctly
    /// validated, including that values 3 and 4 (`reserved` in
    /// `location.proto` since SAPIENT v7) are rejected, not just 0.
    #[test]
    fn test_location_coorindate_units_validation() {
        for i in [1, 2, 5] {
            assert_eq!(
                ValidationOutcome::pass(),
                validate_location_coordinate_system(
                    Some(i),
                    "test.location_type.units",
                    "Units must be specified in location type."
                )
            );
        }

        for i in [0, 3, 4] {
            assert_eq!(
                ValidationOutcome::fail(
                    "test.location_type.units",
                    "Units must be specified in location type."
                ),
                validate_location_coordinate_system(
                    Some(i),
                    "test.location_type.units",
                    "Units must be specified in location type."
                )
            );
        }
    }

    /// Unit test to check that range bearing coordinate units are correctly
    /// validated, including that values 5 and 6 (`reserved` in
    /// `range_bearing.proto` since SAPIENT v7) are rejected, not just 0.
    #[test]
    fn test_range_bearing_coorindate_units_validation() {
        for i in [1, 2, 3, 4] {
            assert_eq!(
                ValidationOutcome::pass(),
                validate_range_bearing_coordinate_system(
                    Some(i),
                    "test.location_type.units",
                    "Units must be specified in location type."
                )
            );
        }

        for i in [0, 5, 6] {
            assert_eq!(
                ValidationOutcome::fail(
                    "test.location_type.units",
                    "Units must be specified in location type."
                ),
                validate_range_bearing_coordinate_system(
                    Some(i),
                    "test.location_type.units",
                    "Units must be specified in location type."
                )
            );
        }
    }

    /// Unit test to check that coordinate datums are correctly validated
    #[test]
    fn test_coorindate_datums_validation() {
        // valid units
        for i in 1..6 {
            assert_eq!(ValidationOutcome::pass(), validate_coordinate_datum(i));
        }

        // invalid units
        assert_eq!(
            ValidationOutcome::fail(
                "registration.location_type.datum.invalid",
                "Datum must be specified in location type."
            ),
            validate_coordinate_datum(0)
        );
    }

    /// Unit test to check that ENU velocity units are correctly validated,
    /// including that the `reserved` values 3 and 4 (withdrawn
    /// non-SI `SpeedUnits`) are rejected, not just 0.
    #[test]
    fn test_enu_velocity_units_validation() {
        for i in [1, 2] {
            assert_eq!(
                ValidationOutcome::pass(),
                validate_enu_velocity_units(RegistrationEnuVelocityUnits {
                    east_north_rate_units: Some(i),
                    up_rate_units: Some(i),
                })
            );
        }

        // mandatory east/north units missing or invalid
        for east_north in [None, Some(0), Some(3), Some(4)] {
            assert_eq!(
                ValidationOutcome::fail(
                    "registration.enu_velocity_units.east_north_rate_units.missing",
                    "East/north rate units must be specified in velocity type."
                ),
                validate_enu_velocity_units(RegistrationEnuVelocityUnits {
                    east_north_rate_units: east_north,
                    up_rate_units: None,
                })
            );
        }

        // optional up rate units, when present, must still be valid
        for up_rate in [Some(0), Some(3), Some(4)] {
            assert_eq!(
                ValidationOutcome::fail(
                    "registration.enu_velocity_units.up_rate_units.invalid",
                    "Up rate units is not a valid option in velocity type."
                ),
                validate_enu_velocity_units(RegistrationEnuVelocityUnits {
                    east_north_rate_units: Some(1),
                    up_rate_units: up_rate,
                })
            );
        }

        // up rate units is optional -- absent is fine
        assert_eq!(
            ValidationOutcome::pass(),
            validate_enu_velocity_units(RegistrationEnuVelocityUnits {
                east_north_rate_units: Some(1),
                up_rate_units: None,
            })
        );
    }

    /// Unit test to check that multiple task definitions are correctly validated
    #[test]
    fn test_task_definitions_validation() {
        let area = LocationType {
            coordinates_oneof: Some(CoordinatesOneof::LocationUnits(1)),
            datum_oneof: Some(DatumOneof::LocationDatum(1)),
            zone: None,
        };

        // valid definition
        let region_definition = RegionDefinition {
            region_type: vec![1],
            region_area: vec![area.clone()],
            settle_time: None,
            behaviour_filter_definition: vec![],
            class_filter_definition: vec![],
        };
        let task_definition = TaskDefinition {
            command: vec![],
            concurrent_tasks: Some(1),
            region_definition: Some(region_definition.clone()),
        };
        assert_eq!(
            ValidationOutcome::pass(),
            validate_task_definition(Some(task_definition))
        );

        // missing region type
        let missing_region_type_task_definition = TaskDefinition {
            command: vec![],
            concurrent_tasks: Some(1),
            region_definition: None,
        };
        assert_eq!(
            ValidationOutcome::fail(
                "registration.task_definition.region_definition.missing",
                "Region definition must be specified in task definition."
            ),
            validate_task_definition(Some(missing_region_type_task_definition))
        );
    }

    /// Unit test to check that task definition are correctly validated
    #[test]
    fn test_task_definition_validation() {
        let area = LocationType {
            coordinates_oneof: Some(CoordinatesOneof::LocationUnits(1)),
            datum_oneof: Some(DatumOneof::LocationDatum(1)),
            zone: None,
        };

        // valid definition
        let region_definition = RegionDefinition {
            region_type: vec![1],
            region_area: vec![area.clone()],
            settle_time: None,
            behaviour_filter_definition: vec![],
            class_filter_definition: vec![],
        };
        let task_definition = TaskDefinition {
            command: vec![],
            concurrent_tasks: Some(1),
            region_definition: Some(region_definition.clone()),
        };
        assert_eq!(
            ValidationOutcome::pass(),
            validate_task_definition(Some(task_definition))
        );

        // missing region type
        let missing_region_type_task_definition = TaskDefinition {
            command: vec![],
            concurrent_tasks: Some(1),
            region_definition: None,
        };
        assert_eq!(
            ValidationOutcome::fail(
                "registration.task_definition.region_definition.missing",
                "Region definition must be specified in task definition."
            ),
            validate_task_definition(Some(missing_region_type_task_definition))
        );

        // invalid region type
        let invalid_type_region_definition = RegionDefinition {
            region_type: vec![0],
            region_area: vec![area.clone()],
            settle_time: None,
            behaviour_filter_definition: vec![],
            class_filter_definition: vec![],
        };
        let invalid_region_type_task_definition = TaskDefinition {
            command: vec![],
            concurrent_tasks: Some(1),
            region_definition: Some(invalid_type_region_definition),
        };
        assert_eq!(
            ValidationOutcome::fail(
                "registration.region_definition.region_type.invalid",
                "Region type must be specified in region definition."
            ),
            validate_task_definition(Some(invalid_region_type_task_definition))
        );
    }

    /// Unit test to check that region definition region types are correctly validated
    #[test]
    fn test_region_definition_region_type_validation() {
        let area = LocationType {
            coordinates_oneof: Some(CoordinatesOneof::LocationUnits(1)),
            datum_oneof: Some(DatumOneof::LocationDatum(1)),
            zone: None,
        };

        // valid definition
        let region_definition = RegionDefinition {
            region_type: vec![1],
            region_area: vec![area.clone()],
            settle_time: None,
            behaviour_filter_definition: vec![],
            class_filter_definition: vec![],
        };
        assert_eq!(
            ValidationOutcome::pass(),
            validate_region_definition(region_definition)
        );

        // invalid region type
        let invalid_type_region_definition = RegionDefinition {
            region_type: vec![0],
            region_area: vec![area.clone()],
            settle_time: None,
            behaviour_filter_definition: vec![],
            class_filter_definition: vec![],
        };
        assert_eq!(
            ValidationOutcome::fail(
                "registration.region_definition.region_type.invalid",
                "Region type must be specified in region definition."
            ),
            validate_region_definition(invalid_type_region_definition)
        );

        // missing region type
        let invalid_type_region_definition = RegionDefinition {
            region_type: vec![],
            region_area: vec![area.clone()],
            settle_time: None,
            behaviour_filter_definition: vec![],
            class_filter_definition: vec![],
        };
        assert_eq!(
            ValidationOutcome::fail(
                "registration.region_definition.region_type.empty",
                "Region type must be specified in region definition."
            ),
            validate_region_definition(invalid_type_region_definition)
        );
    }

    /// Unit test to check that region definition  region areas are correctly validated
    #[test]
    fn test_region_definition_region_area_validation() {
        // invalid region area
        let invalid_area = LocationType {
            coordinates_oneof: Some(CoordinatesOneof::LocationUnits(1)),
            datum_oneof: Some(DatumOneof::LocationDatum(0)),
            zone: None,
        };
        let invalid_area_region_definition = RegionDefinition {
            region_type: vec![1],
            region_area: vec![invalid_area],
            settle_time: None,
            behaviour_filter_definition: vec![],
            class_filter_definition: vec![],
        };
        assert_eq!(
            ValidationOutcome::fail(
                "registration.location_type.datum.invalid",
                "Datum must be specified in location type."
            ),
            validate_region_definition(invalid_area_region_definition)
        );
    }

    /// Unit test to check that mode types are correctly validated
    #[test]
    fn test_mode_type_validation() {
        assert_eq!(
            ValidationOutcome::fail(
                "registration.mode_definition.mode_type.missing",
                "Mode type must be specified."
            ),
            validate_mode_type(Some(0))
        );

        for mode_type in 1..2 {
            assert_eq!(
                ValidationOutcome::pass(),
                validate_mode_type(Some(mode_type))
            )
        }
    }
}
