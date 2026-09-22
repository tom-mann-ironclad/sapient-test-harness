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
use crate::validation::common::{
    validate_location_coordinate_system,
    validate_location_or_range_bearing as validate_common_location_or_range_bearing,
    validate_nonzero, validate_range_bearing_coordinate_system, validate_required_nonzero,
    validate_required_string, validate_uuid_v4,
};

/// Function to validation a SAPIENT registration message
pub fn validate_registration(registration: Registration) -> (bool, String) {
    let mut validations = vec![];

    // Check node type
    validations.push(validate_node_definition(registration.node_definition));

    // Check ICD version
    validations.push(validate_icd_version(registration.icd_version));

    // Check capabilities
    validations.push(validate_capabilities(registration.capabilities));

    // Check status definition
    if registration.status_definition.is_none() {
        return (
            false,
            "Status definition must be specified in registration.".to_string(),
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
        if !validation.0 {
            return validation;
        }
    }

    (true, "".to_string())
}

/// Function to check the node definition as specified in the BSI Flex 335 V2.0
fn validate_node_definition(node_definitions: Vec<NodeDefinition>) -> (bool, String) {
    if node_definitions.is_empty() {
        return (
            false,
            "Node type must be specified in node defintition.".to_string(),
        );
    }
    for node_definition in node_definitions {
        let node_type_validation = validate_required_nonzero(
            node_definition.node_type,
            "Node type must be specified in node defintition.",
        );
        if !node_type_validation.0 {
            return node_type_validation;
        }
    }
    (true, "".to_string())
}

fn validate_capabilities(capabilities: Vec<Capability>) -> (bool, String) {
    if capabilities.is_empty() {
        return (
            false,
            "Capabilities must be specified in registration.".to_string(),
        );
    }

    for capability in capabilities {
        let category_validation = validate_required_string(
            capability.category.as_deref(),
            "Capability category must be specified in registration.",
        );
        if !category_validation.0 {
            return category_validation;
        }

        let type_validation = validate_required_string(
            capability.r#type.as_deref(),
            "Capability type must be specified in registration.",
        );
        if !type_validation.0 {
            return type_validation;
        }
    }

    (true, "".to_string())
}

/// The exact ICD version string a BSI Flex 335 v2.0 registration must declare.
/// Matches the legacy reference validator's rule
/// (`RegistrationValidator.cs`: `RuleFor(x => x.IcdVersion)...Equal("BSI Flex 335 v2.0")`).
const REQUIRED_ICD_VERSION: &str = "BSI Flex 335 v2.0";

/// Function to check the ICD version as specified in the BSI Flex 335 V2.0
fn validate_icd_version(icd_version: Option<String>) -> (bool, String) {
    match icd_version {
        Some(version) if version.is_empty() => (
            false,
            "No ICD version specified in registration message".to_string(),
        ),
        Some(version) if version == REQUIRED_ICD_VERSION => (true, "".to_string()),
        Some(_) => (
            false,
            "ICD version specified in registration is not a valid option.".to_string(),
        ),
        None => (
            false,
            "No ICD version specified in registration message".to_string(),
        ),
    }
}

/// Function to check the status definition as specified in the BSI Flex 335 V2.0
fn validate_status_definition(status_definition: StatusDefinition) -> (bool, String) {
    // Check the status interval
    if status_definition.status_interval.is_none() {
        return (
            false,
            "Status interval must be specified in status definition.".to_string(),
        );
    }
    let valid_status_interval =
        validate_status_interval(status_definition.status_interval.unwrap());
    if !valid_status_interval.0 {
        return valid_status_interval;
    }

    if let Some(location_definition) = status_definition.location_definition {
        let validation = validate_location_type(location_definition);
        if !validation.0 {
            return validation;
        }
    }

    if let Some(coverage_definition) = status_definition.coverage_definition {
        let validation = validate_location_type(coverage_definition);
        if !validation.0 {
            return validation;
        }
    }

    if let Some(obscuration_definition) = status_definition.obscuration_definition {
        let validation = validate_location_type(obscuration_definition);
        if !validation.0 {
            return validation;
        }
    }

    if let Some(field_of_view_definition) = status_definition.field_of_view_definition {
        let validation = validate_location_type(field_of_view_definition);
        if !validation.0 {
            return validation;
        }
    }

    for status_report in status_definition.status_report {
        let validation = validate_status_report_definition(status_report);
        if !validation.0 {
            return validation;
        }
    }

    (true, "".to_string())
}

/// Function to check the status interval as specified in the BSI Flex 335 V2.0
fn validate_status_interval(duration: Duration) -> (bool, String) {
    // Check the units
    let duration_units = validate_duration_units(duration.units);
    if !duration_units.0 {
        return duration_units;
    }

    // Check the value
    let duration_value = validate_duration_value(duration.value);
    if !duration_value.0 {
        return duration_value;
    }

    (true, "".to_string())
}

/// Function to check the duration units as specified in the BSI Flex 335 V2.0
fn validate_duration_units(units: Option<i32>) -> (bool, String) {
    validate_required_nonzero(units, "Time Units must be specified.")
}

/// Function to check the duration units as specified in the BSI Flex 335 V2.0
fn validate_duration_value(value: Option<f32>) -> (bool, String) {
    if value.is_none() {
        return (false, "Duration value must be provided.".to_string());
    }
    if value < Some(0.0) {
        return (false, "Duration value must be 0 or greater.".to_string());
    }
    (true, "".to_string())
}

/// Function to check the mode definitions as specified in the BSI Flex 335 V2.0
fn validate_mode_definitions(mode_definitions: Vec<ModeDefinition>) -> (bool, String) {
    if mode_definitions.is_empty() {
        return (
            false,
            "Mode definition must be specified in registration.".to_string(),
        );
    }

    let mut validations = vec![];

    for mode_definition in mode_definitions {
        validations.push(validate_mode_definition(mode_definition));
    }

    for validation in validations {
        if !validation.0 {
            return validation;
        }
    }

    (true, "".to_string())
}

/// Function to check the mode definition as specified in the BSI Flex 335 V2.0
fn validate_mode_definition(mode_definition: ModeDefinition) -> (bool, String) {
    let mut validations = vec![];

    // Check mode name
    validations.push(validate_mode_name(mode_definition.mode_name));

    // Check settle time
    if mode_definition.settle_time.is_none() {
        return (
            false,
            "Settle time must be specified in mode definition.".to_string(),
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
        if !validation.0 {
            return validation;
        }
    }

    (true, "".to_string())
}

/// Function to check the mode name as specified in the BSI Flex 335 V2.0
fn validate_mode_name(mode_name: Option<String>) -> (bool, String) {
    validate_required_string(
        mode_name.as_deref(),
        "Mode name must be specified in mode definition.",
    )
}

/// Function to check the settle time as specified in the BSI Flex 335 V2.0
fn validate_settle_time(settle_time: Duration) -> (bool, String) {
    // Check the units
    let duration_units = validate_duration_units(settle_time.units);
    if !duration_units.0 {
        return duration_units;
    }

    // Check the value
    let duration_value = validate_duration_value(settle_time.value);
    if !duration_value.0 {
        return duration_value;
    }

    (true, "".to_string())
}

fn validate_mode_parameter(mode_parameter: ModeParameter) -> (bool, String) {
    let type_validation = validate_required_string(
        mode_parameter.r#type.as_deref(),
        "Mode parameter type must be specified.",
    );
    if !type_validation.0 {
        return type_validation;
    }

    let value_validation = validate_required_string(
        mode_parameter.value.as_deref(),
        "Mode parameter value must be specified.",
    );
    if !value_validation.0 {
        return value_validation;
    }

    (true, "".to_string())
}

/// Function to check the detection definition as specified in the BSI Flex 335 V2.0
fn validate_detection_definition(detection_definition: DetectionDefinition) -> (bool, String) {
    // Check location type
    if detection_definition.location_type.is_none() {
        return (
            false,
            "Location type must be specified in detection definition.".to_string(),
        );
    }
    let valid_location_type = validate_location_type(detection_definition.location_type.unwrap());
    if !valid_location_type.0 {
        return valid_location_type;
    }

    if let Some(geometric_error) = detection_definition.geometric_error {
        let validation = validate_geometric_error(geometric_error);
        if !validation.0 {
            return validation;
        }
    }

    if let Some(velocity_type) = detection_definition.velocity_type {
        let validation = validate_velocity_type(velocity_type);
        if !validation.0 {
            return validation;
        }
    }

    for detection_performance in detection_definition.detection_performance {
        let validation = validate_performance_value(detection_performance);
        if !validation.0 {
            return validation;
        }
    }

    for detection_report in detection_definition.detection_report {
        let validation = validate_detection_report_definition(detection_report);
        if !validation.0 {
            return validation;
        }
    }

    for detection_class_definition in detection_definition.detection_class_definition {
        let validation = validate_detection_class_definition(detection_class_definition);
        if !validation.0 {
            return validation;
        }
    }

    for behaviour_definition in detection_definition.behaviour_definition {
        let validation = validate_behaviour_definition(behaviour_definition);
        if !validation.0 {
            return validation;
        }
    }

    (true, "".to_string())
}

/// Function to check location type as specified in the BSI Flex 335 V2.0
fn validate_location_type(location_type: LocationType) -> (bool, String) {
    // Check location type coorindates
    if location_type.coordinates_oneof.is_none() {
        return (
            false,
            "Units must be specified in location type.".to_string(),
        );
    };
    let valid_units = match location_type.coordinates_oneof.unwrap() {
        LocationUnits(units) => validate_location_coordinate_system(
            Some(units),
            "Units must be specified in location type.",
        ),
        RangeBearingUnits(units) => validate_range_bearing_coordinate_system(
            Some(units),
            "Units must be specified in location type.",
        ),
    };
    if !valid_units.0 {
        return valid_units;
    }

    // Check location type datum
    if location_type.datum_oneof.is_none() {
        return (
            false,
            "Datum must be specified in location type.".to_string(),
        );
    };
    let valid_datum = match location_type.datum_oneof.unwrap() {
        LocationDatum(datum) => validate_coordinate_datum(datum),
        RangeBearingDatum(datum) => validate_coordinate_datum(datum),
    };
    if !valid_datum.0 {
        return valid_datum;
    }

    (true, "".to_string())
}

/// Function to check coordinate datum as specified in the BSI Flex 335 V2.0
fn validate_coordinate_datum(datum: i32) -> (bool, String) {
    validate_nonzero(datum, "Datum must be specified in location type.")
}

/// Function to check the mode definition as specified in the BSI Flex 335 V2.0
fn validate_task_definition(task_definition: Option<TaskDefinition>) -> (bool, String) {
    match task_definition {
        None => (false, "Task definition must be populated".to_string()),
        Some(task_def) => {
            match task_def.concurrent_tasks {
                None => {
                    return (
                        false,
                        "Concurrent tasks must be specified in task definition.".to_string(),
                    );
                }
                Some(concurrent_tasks) if concurrent_tasks < 0 => {
                    return (false, "Concurrent tasks must be 0 or greater.".to_string());
                }
                Some(_) => {}
            }

            // Check region definition
            if task_def.region_definition.is_none() {
                return (
                    false,
                    "Region definition must be specified in task definition.".to_string(),
                );
            }
            let valid_region_definition =
                validate_region_definition(task_def.region_definition.unwrap());
            if !valid_region_definition.0 {
                return valid_region_definition;
            }

            for command in task_def.command {
                let validation = validate_command_definition(command);
                if !validation.0 {
                    return validation;
                }
            }

            (true, "".to_string())
        }
    }
}

/// Function to check region definition as specified in the BSI Flex 335 V2.0
fn validate_region_definition(region_definition: RegionDefinition) -> (bool, String) {
    // Check region type
    if region_definition.region_type.is_empty() {
        return (
            false,
            "Region type must be specified in region definition.".to_string(),
        );
    }
    for region_type in region_definition.region_type {
        let valid_region_type = region_type != 0;
        if !valid_region_type {
            return (
                false,
                "Region type must be specified in region definition.".to_string(),
            );
        }
    }

    if let Some(settle_time) = region_definition.settle_time {
        let validation = validate_settle_time(settle_time);
        if !validation.0 {
            return validation;
        }
    }

    if region_definition.region_area.is_empty() {
        return (
            false,
            "Region area must be specified in region definition.".to_string(),
        );
    }

    // Check location type
    for region_area in region_definition.region_area {
        let valid_region_area = validate_location_type(region_area);
        if !valid_region_area.0 {
            return valid_region_area;
        }
    }

    for class_filter_definition in region_definition.class_filter_definition {
        let validation = validate_class_filter_definition(class_filter_definition);
        if !validation.0 {
            return validation;
        }
    }

    for behaviour_filter_definition in region_definition.behaviour_filter_definition {
        let validation = validate_behaviour_filter_definition(behaviour_filter_definition);
        if !validation.0 {
            return validation;
        }
    }

    (true, "".to_string())
}

/// Function to check the mode definition as specified in the BSI Flex 335 V2.0
fn validate_mode_type(mode_type: Option<i32>) -> (bool, String) {
    validate_required_nonzero(mode_type, "Mode type must be specified.")
}

fn validate_status_report_definition(
    status_report: crate::bsi_flex_335_v2_0::registration::StatusReport,
) -> (bool, String) {
    let category_validation = validate_required_nonzero(
        status_report.category,
        "Status report category must be specified in registration.",
    );
    if !category_validation.0 {
        return category_validation;
    }

    validate_required_string(
        status_report.r#type.as_deref(),
        "Status report type must be specified in registration.",
    )
}

fn validate_geometric_error(geometric_error: GeometricError) -> (bool, String) {
    match geometric_error.r#type.as_deref() {
        Some("") | None => {
            return (false, "Geometric error type must be specified.".to_string());
        }
        Some(_) => {}
    }

    match geometric_error.units.as_deref() {
        Some("") | None => {
            return (
                false,
                "Geometric error units must be specified.".to_string(),
            );
        }
        Some(_) => {}
    }

    match geometric_error.variation_type.as_deref() {
        Some("") | None => {
            return (
                false,
                "Geometric error variation type must be specified.".to_string(),
            );
        }
        Some(_) => {}
    }

    for performance_value in geometric_error.performance_value {
        let validation = validate_performance_value(performance_value);
        if !validation.0 {
            return validation;
        }
    }

    (true, "".to_string())
}

fn validate_performance_value(performance_value: PerformanceValue) -> (bool, String) {
    match performance_value.r#type.as_deref() {
        Some("") | None => {
            return (
                false,
                "Performance value type must be specified.".to_string(),
            );
        }
        Some(_) => {}
    }

    match performance_value.units.as_deref() {
        Some("") | None => {
            return (
                false,
                "Performance value units must be specified.".to_string(),
            );
        }
        Some(_) => {}
    }

    match performance_value.unit_value.as_deref() {
        Some("") | None => {
            return (
                false,
                "Performance value unit value must be specified.".to_string(),
            );
        }
        Some(_) => {}
    }

    (true, "".to_string())
}

fn validate_detection_report_definition(
    detection_report: crate::bsi_flex_335_v2_0::registration::DetectionReport,
) -> (bool, String) {
    if detection_report.category.is_none() || detection_report.category == Some(0) {
        return (
            false,
            "Detection report category must be specified.".to_string(),
        );
    }

    match detection_report.r#type.as_deref() {
        Some("") | None => {
            return (
                false,
                "Detection report type must be specified.".to_string(),
            );
        }
        Some(_) => {}
    }

    match detection_report.units.as_deref() {
        Some("") | None => {
            return (
                false,
                "Detection report units must be specified.".to_string(),
            );
        }
        Some(_) => {}
    }

    (true, "".to_string())
}

fn validate_detection_class_definition(
    detection_class_definition: DetectionClassDefinition,
) -> (bool, String) {
    for class_performance in detection_class_definition.class_performance {
        let validation = validate_performance_value(class_performance);
        if !validation.0 {
            return validation;
        }
    }

    for class_definition in detection_class_definition.class_definition {
        let validation = validate_class_definition(class_definition);
        if !validation.0 {
            return validation;
        }
    }

    for taxonomy_dock_definition in detection_class_definition.taxonomy_dock_definition {
        let validation = validate_taxonomy_dock_definition(taxonomy_dock_definition);
        if !validation.0 {
            return validation;
        }
    }

    (true, "".to_string())
}

fn validate_class_definition(class_definition: ClassDefinition) -> (bool, String) {
    match class_definition.r#type.as_deref() {
        Some("") | None => {
            return (
                false,
                "Class definition type must be specified.".to_string(),
            );
        }
        Some(_) => {}
    }

    for sub_class in class_definition.sub_class {
        let validation = validate_sub_class_definition(sub_class);
        if !validation.0 {
            return validation;
        }
    }

    (true, "".to_string())
}

fn validate_sub_class_definition(sub_class: SubClass) -> (bool, String) {
    match sub_class.r#type.as_deref() {
        Some("") | None => {
            return (false, "Sub class type must be specified.".to_string());
        }
        Some(_) => {}
    }

    if sub_class.level.is_none() {
        return (false, "Sub class level must be specified.".to_string());
    }

    for nested_sub_class in sub_class.sub_class {
        let validation = validate_sub_class_definition(nested_sub_class);
        if !validation.0 {
            return validation;
        }
    }

    (true, "".to_string())
}

fn validate_behaviour_definition(behaviour_definition: BehaviourDefinition) -> (bool, String) {
    match behaviour_definition.r#type.as_deref() {
        Some("") | None => (
            false,
            "Behaviour definition type must be specified.".to_string(),
        ),
        Some(_) => (true, "".to_string()),
    }
}

fn validate_velocity_type(velocity_type: VelocityType) -> (bool, String) {
    let velocity_units = match velocity_type.velocity_units_oneof {
        Some(EnuVelocityUnits(enu_velocity_units)) => enu_velocity_units,
        None => {
            return (
                false,
                "Velocity units must be specified in velocity type.".to_string(),
            );
        }
    };

    let units_validation = validate_enu_velocity_units(velocity_units);
    if !units_validation.0 {
        return units_validation;
    }

    if velocity_type.datum_oneof.is_none() {
        return (
            false,
            "Datum must be specified in velocity type.".to_string(),
        );
    }

    (true, "".to_string())
}

/// `SpeedUnits` values 3 and 4 are `reserved` in `velocity.proto` (used up
/// to SAPIENT v7, dropped for non-SI units) -- still legal `int32`s on the
/// wire, so must be explicitly excluded rather than just checked for
/// nonzero.
fn validate_speed_units(value: Option<i32>, error_message: &str) -> (bool, String) {
    match value {
        Some(v) if [1, 2].contains(&v) => (true, String::new()),
        _ => (false, error_message.to_string()),
    }
}

fn validate_enu_velocity_units(enu_velocity_units: RegistrationEnuVelocityUnits) -> (bool, String) {
    let east_north_validation = validate_speed_units(
        enu_velocity_units.east_north_rate_units,
        "East/north rate units must be specified in velocity type.",
    );
    if !east_north_validation.0 {
        return east_north_validation;
    }

    if let Some(up_rate_units) = enu_velocity_units.up_rate_units {
        let up_rate_validation = validate_speed_units(
            Some(up_rate_units),
            "Up rate units is not a valid option in velocity type.",
        );
        if !up_rate_validation.0 {
            return up_rate_validation;
        }
    }

    (true, "".to_string())
}

fn validate_command_definition(command: Command) -> (bool, String) {
    match command.units.as_deref() {
        Some("") | None => return (false, "Command units must be specified.".to_string()),
        Some(_) => {}
    }

    if command.completion_time.is_none() {
        return (
            false,
            "Command completion time must be specified.".to_string(),
        );
    }
    let completion_time_validation = validate_settle_time(command.completion_time.unwrap());
    if !completion_time_validation.0 {
        return completion_time_validation;
    }

    if command.r#type.is_none() || command.r#type == Some(0) {
        return (false, "Command type must be specified.".to_string());
    }

    (true, "".to_string())
}

fn validate_class_filter_definition(
    class_filter_definition: ClassFilterDefinition,
) -> (bool, String) {
    match class_filter_definition.r#type.as_deref() {
        Some("") | None => {
            return (
                false,
                "Class filter definition type must be specified.".to_string(),
            );
        }
        Some(_) => {}
    }

    for filter_parameter in class_filter_definition.filter_parameter {
        let validation = validate_filter_parameter(filter_parameter);
        if !validation.0 {
            return validation;
        }
    }

    for sub_class_definition in class_filter_definition.sub_class_definition {
        let validation = validate_sub_class_filter_definition(sub_class_definition);
        if !validation.0 {
            return validation;
        }
    }

    (true, "".to_string())
}

fn validate_sub_class_filter_definition(
    sub_class_filter_definition: SubClassFilterDefinition,
) -> (bool, String) {
    if sub_class_filter_definition.level.is_none() {
        return (
            false,
            "Sub class filter definition level must be specified.".to_string(),
        );
    }

    match sub_class_filter_definition.r#type.as_deref() {
        Some("") | None => {
            return (
                false,
                "Sub class filter definition type must be specified.".to_string(),
            );
        }
        Some(_) => {}
    }

    for filter_parameter in sub_class_filter_definition.filter_parameter {
        let validation = validate_filter_parameter(filter_parameter);
        if !validation.0 {
            return validation;
        }
    }

    for nested_sub_class_definition in sub_class_filter_definition.sub_class_definition {
        let validation = validate_sub_class_filter_definition(nested_sub_class_definition);
        if !validation.0 {
            return validation;
        }
    }

    (true, "".to_string())
}

fn validate_filter_parameter(filter_parameter: FilterParameter) -> (bool, String) {
    match filter_parameter.parameter.as_deref() {
        Some("") | None => {
            return (
                false,
                "Filter parameter name must be specified.".to_string(),
            );
        }
        Some(_) => {}
    }

    if filter_parameter.operators.is_empty() || filter_parameter.operators.contains(&0) {
        return (
            false,
            "Filter parameter operators must be specified.".to_string(),
        );
    }

    (true, "".to_string())
}

fn validate_behaviour_filter_definition(
    behaviour_filter_definition: BehaviourFilterDefinition,
) -> (bool, String) {
    match behaviour_filter_definition.r#type.as_deref() {
        Some("") | None => {
            return (
                false,
                "Behaviour filter definition type must be specified.".to_string(),
            );
        }
        Some(_) => {}
    }

    for filter_parameter in behaviour_filter_definition.filter_parameter {
        let validation = validate_filter_parameter(filter_parameter);
        if !validation.0 {
            return validation;
        }
    }

    (true, "".to_string())
}

fn validate_taxonomy_dock_definition(
    taxonomy_dock_definition: TaxonomyDockDefinition,
) -> (bool, String) {
    match taxonomy_dock_definition.dock_class_namespace.as_deref() {
        Some("") | None => {
            return (
                false,
                "Taxonomy dock class namespace must be specified.".to_string(),
            );
        }
        Some(_) => {}
    }

    match taxonomy_dock_definition.dock_class.as_deref() {
        Some("") | None => {
            return (false, "Taxonomy dock class must be specified.".to_string());
        }
        Some(_) => {}
    }

    for extension_subclass in taxonomy_dock_definition.extension_subclass {
        let validation = validate_extension_subclass(extension_subclass);
        if !validation.0 {
            return validation;
        }
    }

    (true, "".to_string())
}

fn validate_extension_subclass(extension_subclass: ExtensionSubclass) -> (bool, String) {
    match extension_subclass.subclass_namespace.as_deref() {
        Some("") | None => {
            return (
                false,
                "Extension subclass namespace must be specified.".to_string(),
            );
        }
        Some(_) => {}
    }

    match extension_subclass.subclass_name.as_deref() {
        Some("") | None => (
            false,
            "Extension subclass name must be specified.".to_string(),
        ),
        Some(_) => (true, "".to_string()),
    }
}

fn validate_dependent_nodes(dependent_nodes: Vec<String>) -> (bool, String) {
    for dependent_node in dependent_nodes {
        let validation = validate_uuid_v4(
            Some(dependent_node.as_str()),
            "A valid UUID v4 must be used for a dependent node ID in registration.",
        );
        if !validation.0 {
            return validation;
        }
    }

    (true, "".to_string())
}

fn validate_reporting_region(
    reporting_region: Vec<crate::bsi_flex_335_v2_0::LocationOrRangeBearing>,
) -> (bool, String) {
    for region in reporting_region {
        let validation = validate_common_location_or_range_bearing(
            region,
            "Location or range-bearing must be specified in reporting region.",
        );
        if !validation.0 {
            return validation;
        }
    }

    (true, "".to_string())
}

fn validate_config_data(config_data: Vec<ConfigurationData>) -> (bool, String) {
    if config_data.is_empty() {
        return (
            false,
            "Configuration data must be specified in registration.".to_string(),
        );
    }

    for configuration_data in config_data {
        let validation = validate_configuration_data_entry(configuration_data);
        if !validation.0 {
            return validation;
        }
    }

    (true, "".to_string())
}

fn validate_configuration_data_entry(configuration_data: ConfigurationData) -> (bool, String) {
    if configuration_data.manufacturer.is_empty() {
        return (
            false,
            "Configuration data manufacturer must be specified.".to_string(),
        );
    }

    if configuration_data.model.is_empty() {
        return (
            false,
            "Configuration data model must be specified.".to_string(),
        );
    }

    for sub_component in configuration_data.sub_components {
        let validation = validate_configuration_data_entry(sub_component);
        if !validation.0 {
            return validation;
        }
    }

    (true, "".to_string())
}

#[cfg(test)]
mod registration_validation_tests {
    use crate::bsi_flex_335_v2_0::registration::location_type::{CoordinatesOneof, DatumOneof};
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
            (true, "".to_string()),
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
            (
                false,
                "ICD version specified in registration is not a valid option.".to_string()
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
            (true, "".to_string()),
            validate_node_definition(vec![valid_node_definition])
        );

        // invalid node definition
        let invalid_node_definition = NodeDefinition {
            node_type: None,
            node_sub_type: vec![],
        };
        assert_eq!(
            (
                false,
                "Node type must be specified in node defintition.".to_string()
            ),
            validate_node_definition(vec![invalid_node_definition])
        );

        // missing node definition
        assert_eq!(
            (
                false,
                "Node type must be specified in node defintition.".to_string()
            ),
            validate_node_definition(vec![])
        );
    }

    /// Unit test to check that ICD versions are correctly validated
    #[test]
    fn test_icd_versions_validation() {
        // valid version
        assert_eq!(
            (true, "".to_string()),
            validate_icd_version(Some("BSI Flex 335 v2.0".to_string()))
        );

        // invalid version
        assert_eq!(
            (
                false,
                "ICD version specified in registration is not a valid option.".to_string()
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
            (true, "".to_string()),
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
            (
                false,
                "Status interval must be specified in status definition.".to_string()
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
            (false, "Duration value must be 0 or greater.".to_string()),
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
            (true, "".to_string()),
            validate_status_interval(valid_duration)
        );

        // missing units
        let missing_units_duration = Duration {
            units: None,
            value: Some(1.0),
        };
        assert_eq!(
            (false, "Time Units must be specified.".to_string()),
            validate_status_interval(missing_units_duration)
        );

        // missing value
        let missing_value_duration = Duration {
            units: Some(1),
            value: None,
        };
        assert_eq!(
            (false, "Duration value must be provided.".to_string()),
            validate_status_interval(missing_value_duration)
        );
    }

    /// Unit test to check that duration units are correctly validated
    #[test]
    fn test_duration_units_validation() {
        // valid units
        for i in 1..6 {
            assert_eq!((true, "".to_string()), validate_duration_units(Some(i)));
        }

        // invalid units
        assert_eq!(
            (false, "Time Units must be specified.".to_string()),
            validate_duration_units(Some(0))
        );
    }

    /// Unit test to check that duration values are correctly validated
    #[test]
    fn test_duration_values_validation() {
        // valid values
        for i in 0..500 {
            assert_eq!(
                (true, "".to_string()),
                validate_duration_value(Some(i as f32))
            );
        }

        // invalid values
        assert_eq!(
            (false, "Duration value must be 0 or greater.".to_string()),
            validate_duration_value(Some(-1.0))
        );

        // missing values
        assert_eq!(
            (false, "Duration value must be provided.".to_string()),
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
            (true, "".to_string()),
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
            (
                false,
                "Performance value type must be specified.".to_string()
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
            (true, "".to_string()),
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
            (
                false,
                "Mode name must be specified in mode definition.".to_string()
            ),
            validate_mode_definitions(vec![invalid_mode_definition])
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
            (true, "".to_string()),
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
            (
                false,
                "Mode name must be specified in mode definition.".to_string()
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
            (
                false,
                "Settle time must be specified in mode definition.".to_string()
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
            (true, "".to_string()),
            validate_mode_definition(mode_definition_without_detection_definitions)
        );
    }

    /// Unit test to check that mode names are correctly validated
    #[test]
    fn test_mode_name_validation() {
        // valid mode name
        assert_eq!(
            (true, "".to_string()),
            validate_mode_name(Some("Default".to_string()))
        );

        // invalid mode name
        assert_eq!(
            (
                false,
                "Mode name must be specified in mode definition.".to_string()
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
        assert_eq!((true, "".to_string()), validate_settle_time(valid_duration));

        // missing units
        let missing_units_duration = Duration {
            units: None,
            value: Some(1.0),
        };
        assert_eq!(
            (false, "Time Units must be specified.".to_string()),
            validate_settle_time(missing_units_duration)
        );

        // invalid value
        let missing_value_duration = Duration {
            units: Some(1),
            value: Some(-1.0),
        };
        assert_eq!(
            (false, "Duration value must be 0 or greater.".to_string()),
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
            (true, "".to_string()),
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
            (
                false,
                "Location type must be specified in detection definition.".to_string()
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
            (
                false,
                "Datum must be specified in location type.".to_string()
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
            (true, "".to_string()),
            validate_location_type(valid_location_type)
        );

        // missing units
        let missing_units_location_type = LocationType {
            coordinates_oneof: None,
            datum_oneof: Some(DatumOneof::RangeBearingDatum(1)),
            zone: None,
        };
        assert_eq!(
            (
                false,
                "Units must be specified in location type.".to_string()
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
            (
                false,
                "Datum must be specified in location type.".to_string()
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
            (
                false,
                "Units must be specified in location type.".to_string()
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
                (true, "".to_string()),
                validate_location_coordinate_system(
                    Some(i),
                    "Units must be specified in location type."
                )
            );
        }

        for i in [0, 3, 4] {
            assert_eq!(
                (
                    false,
                    "Units must be specified in location type.".to_string()
                ),
                validate_location_coordinate_system(
                    Some(i),
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
                (true, "".to_string()),
                validate_range_bearing_coordinate_system(
                    Some(i),
                    "Units must be specified in location type."
                )
            );
        }

        for i in [0, 5, 6] {
            assert_eq!(
                (
                    false,
                    "Units must be specified in location type.".to_string()
                ),
                validate_range_bearing_coordinate_system(
                    Some(i),
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
            assert_eq!((true, "".to_string()), validate_coordinate_datum(i));
        }

        // invalid units
        assert_eq!(
            (
                false,
                "Datum must be specified in location type.".to_string()
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
                (true, "".to_string()),
                validate_enu_velocity_units(RegistrationEnuVelocityUnits {
                    east_north_rate_units: Some(i),
                    up_rate_units: Some(i),
                })
            );
        }

        // mandatory east/north units missing or invalid
        for east_north in [None, Some(0), Some(3), Some(4)] {
            assert_eq!(
                (
                    false,
                    "East/north rate units must be specified in velocity type.".to_string()
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
                (
                    false,
                    "Up rate units is not a valid option in velocity type.".to_string()
                ),
                validate_enu_velocity_units(RegistrationEnuVelocityUnits {
                    east_north_rate_units: Some(1),
                    up_rate_units: up_rate,
                })
            );
        }

        // up rate units is optional -- absent is fine
        assert_eq!(
            (true, "".to_string()),
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
            (true, "".to_string()),
            validate_task_definition(Some(task_definition))
        );

        // missing region type
        let missing_region_type_task_definition = TaskDefinition {
            command: vec![],
            concurrent_tasks: Some(1),
            region_definition: None,
        };
        assert_eq!(
            (
                false,
                "Region definition must be specified in task definition.".to_string()
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
            (true, "".to_string()),
            validate_task_definition(Some(task_definition))
        );

        // missing region type
        let missing_region_type_task_definition = TaskDefinition {
            command: vec![],
            concurrent_tasks: Some(1),
            region_definition: None,
        };
        assert_eq!(
            (
                false,
                "Region definition must be specified in task definition.".to_string()
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
            (
                false,
                "Region type must be specified in region definition.".to_string()
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
            (true, "".to_string()),
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
            (
                false,
                "Region type must be specified in region definition.".to_string()
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
            (
                false,
                "Region type must be specified in region definition.".to_string()
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
            (
                false,
                "Datum must be specified in location type.".to_string()
            ),
            validate_region_definition(invalid_area_region_definition)
        );
    }

    /// Unit test to check that mode types are correctly validated
    #[test]
    fn test_mode_type_validation() {
        assert_eq!(
            (false, "Mode type must be specified.".to_string()),
            validate_mode_type(Some(0))
        );

        for mode_type in 1..2 {
            assert_eq!((true, "".to_string()), validate_mode_type(Some(mode_type)))
        }
    }
}
