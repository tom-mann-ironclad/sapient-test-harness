use crate::bsi_flex_335_v2_0::{
    LocationOrRangeBearing, Task,
    task::{
        BehaviourFilter, ClassFilter, Command, Parameter, Region, SubClassFilter,
        command::Command as TaskCommand,
    },
};
use crate::validation::common::{
    validate_follow_object, validate_location_list,
    validate_location_or_range_bearing as validate_common_location_or_range_bearing,
    validate_required_nonzero, validate_required_string, validate_ulid,
};

/// Function to validation a SAPIENT task message
pub fn validate_task(task: Task) -> (bool, String) {
    let mut validations = vec![];

    // Check task ID
    validations.push(validate_task_id(task.task_id));

    // Check control
    validations.push(validate_control(task.control));

    if let Some(command) = task.command {
        validations.push(validate_command(command));
    }

    for region in task.region {
        validations.push(validate_region(region));
    }

    for validation in validations {
        if !validation.0 {
            return validation;
        }
    }

    (true, "".to_string())
}

fn validate_command(command: Command) -> (bool, String) {
    let command = match command.command {
        Some(command) => command,
        None => return (false, "Task command must be specified.".to_string()),
    };

    match command {
        TaskCommand::Request(request) | TaskCommand::ModeChange(request) => {
            if request.is_empty() {
                return (false, "Task command must be populated.".to_string());
            }
        }
        TaskCommand::DetectionThreshold(level)
        | TaskCommand::DetectionReportRate(level)
        | TaskCommand::ClassificationThreshold(level) => {
            if !(1..=3).contains(&level) {
                return (false, "Task threshold must be specified.".to_string());
            }
        }
        TaskCommand::LookAt(location_or_range_bearing) => {
            let validation = validate_location_or_range_bearing(location_or_range_bearing);
            if !validation.0 {
                return validation;
            }
        }
        TaskCommand::MoveTo(location_list) | TaskCommand::Patrol(location_list) => {
            let validation = validate_location_list(location_list);
            if !validation.0 {
                return validation;
            }
        }
        TaskCommand::Follow(follow_object) => {
            let validation = validate_follow_object(follow_object);
            if !validation.0 {
                return validation;
            }
        }
    }

    (true, "".to_string())
}

fn validate_region(region: Region) -> (bool, String) {
    let region_type_validation = validate_required_nonzero(
        region.r#type,
        "Region type must be specified in task message.",
    );
    if !region_type_validation.0 {
        return region_type_validation;
    }

    let region_id_validation = validate_ulid(
        region.region_id.as_deref(),
        "A valid ULID must be used for a region ID in a task message.",
    );
    if !region_id_validation.0 {
        return region_id_validation;
    }

    let region_name_validation = validate_required_string(
        region.region_name.as_deref(),
        "Region name must be specified in task message.",
    );
    if !region_name_validation.0 {
        return region_name_validation;
    }

    let region_area = match region.region_area {
        Some(region_area) => region_area,
        None => {
            return (
                false,
                "Region area must be specified in task message.".to_string(),
            );
        }
    };
    let region_area_validation = validate_location_or_range_bearing(region_area);
    if !region_area_validation.0 {
        return region_area_validation;
    }

    for class_filter in region.class_filter {
        let validation = validate_class_filter(class_filter);
        if !validation.0 {
            return validation;
        }
    }

    for behaviour_filter in region.behaviour_filter {
        let validation = validate_behaviour_filter(behaviour_filter);
        if !validation.0 {
            return validation;
        }
    }

    (true, "".to_string())
}

fn validate_location_or_range_bearing(
    location_or_range_bearing: LocationOrRangeBearing,
) -> (bool, String) {
    validate_common_location_or_range_bearing(
        location_or_range_bearing,
        "Location or range-bearing must be specified in task message.",
    )
}

fn validate_class_filter(class_filter: ClassFilter) -> (bool, String) {
    let parameter = match class_filter.parameter {
        Some(parameter) => parameter,
        None => {
            return (
                false,
                "Parameter must be specified in class filter.".to_string(),
            );
        }
    };

    let parameter_validation = validate_parameter(parameter);
    if !parameter_validation.0 {
        return parameter_validation;
    }

    let type_validation = validate_required_string(
        class_filter.r#type.as_deref(),
        "Type must be specified in class filter.",
    );
    if !type_validation.0 {
        return type_validation;
    }

    for sub_class_filter in class_filter.sub_class_filter {
        let validation = validate_sub_class_filter(sub_class_filter);
        if !validation.0 {
            return validation;
        }
    }

    (true, "".to_string())
}

fn validate_sub_class_filter(sub_class_filter: SubClassFilter) -> (bool, String) {
    let parameter = match sub_class_filter.parameter {
        Some(parameter) => parameter,
        None => {
            return (
                false,
                "Parameter must be specified in sub class filter.".to_string(),
            );
        }
    };

    let parameter_validation = validate_parameter(parameter);
    if !parameter_validation.0 {
        return parameter_validation;
    }

    let type_validation = validate_required_string(
        sub_class_filter.r#type.as_deref(),
        "Type must be specified in sub class filter.",
    );
    if !type_validation.0 {
        return type_validation;
    }

    for nested_sub_class_filter in sub_class_filter.sub_class_filter {
        let validation = validate_sub_class_filter(nested_sub_class_filter);
        if !validation.0 {
            return validation;
        }
    }

    (true, "".to_string())
}

fn validate_behaviour_filter(behaviour_filter: BehaviourFilter) -> (bool, String) {
    let parameter = match behaviour_filter.parameter {
        Some(parameter) => parameter,
        None => {
            return (
                false,
                "Parameter must be specified in behaviour filter.".to_string(),
            );
        }
    };

    let parameter_validation = validate_parameter(parameter);
    if !parameter_validation.0 {
        return parameter_validation;
    }

    (true, "".to_string())
}

fn validate_parameter(parameter: Parameter) -> (bool, String) {
    let name_validation = validate_required_string(
        parameter.name.as_deref(),
        "Parameter name must be specified.",
    );
    if !name_validation.0 {
        return name_validation;
    }

    let operator_validation =
        validate_required_nonzero(parameter.operator, "Parameter operator must be specified.");
    if !operator_validation.0 {
        return operator_validation;
    }

    if parameter.value.is_none() {
        return (false, "Parameter value must be specified.".to_string());
    }

    (true, "".to_string())
}

/// Function to check a task ID as specified in the SAPIENT version 7 ICD
fn validate_task_id(task_id: Option<String>) -> (bool, String) {
    validate_ulid(
        task_id.as_deref(),
        "A valid ULID must be used for a task ID in a task message.",
    )
}

/// Function to check a control as specified in the SAPIENT version 7 ICD
/// `Control` value 4 (`CONTROL_DEFAULT`) is `reserved` in `task.proto`
/// ("Default task has been removed as it is not possible to define") --
/// still a legal `int32` on the wire, so it must be explicitly excluded.
/// The legacy reference validator (`TaskValidator.cs`) rejects it via
/// FluentValidation's `.IsInEnum()`, which only accepts values present as
/// actual enum members in the generated C# type.
fn validate_control(control: Option<i32>) -> (bool, String) {
    let valid_control = match control {
        None | Some(0) => false,
        Some(1) => true,
        Some(2) => true,
        Some(3) => true,
        _ => false,
    };
    if !valid_control {
        return (
            false,
            "Control must be specified in a task message.".to_string(),
        );
    }

    (true, "".to_string())
}

#[cfg(test)]
mod task_validation_tests {
    use crate::{
        bsi_flex_335_v2_0::{
            FollowObject, Location, LocationList, LocationOrRangeBearing, Task,
            location_or_range_bearing::FovOneof,
            task::{
                BehaviourFilter, ClassFilter, Command, Parameter, Region, SubClassFilter,
                command::Command::{Follow, Request},
            },
        },
        validation::task::{validate_control, validate_task, validate_task_id},
    };

    /// Unit test to check that task are correctly validated
    #[test]
    fn test_task_validation() {
        // valid task
        let request = Request("SendRegistration".to_string());
        let valid_command = Command {
            command_parameter: None,
            command: Some(request.clone()),
        };
        let valid_task = Task {
            task_id: Some("01H1VV3VN40RV97CDFSXJB44K9".to_string()),
            task_name: None,
            task_description: None,
            task_start_time: None,
            task_end_time: None,
            control: Some(1),
            region: vec![],
            command: Some(valid_command.clone()),
        };
        assert_eq!((true, "".to_string()), validate_task(valid_task));

        // invalid task
        let invalid_task = Task {
            task_id: None,
            task_name: None,
            task_description: None,
            task_start_time: None,
            task_end_time: None,
            control: Some(1),
            region: vec![],
            command: Some(valid_command.clone()),
        };
        assert_eq!(
            (
                false,
                "A valid ULID must be used for a task ID in a task message.".to_string()
            ),
            validate_task(invalid_task)
        );

        // empty task is valid in the v2 proto because region and command are both optional
        let empty_task = Task {
            task_id: Some("01H1VV3VN40RV97CDFSXJB44K9".to_string()),
            task_name: None,
            task_description: None,
            task_start_time: None,
            task_end_time: None,
            control: Some(1),
            region: vec![],
            command: None,
        };
        assert_eq!((true, "".to_string()), validate_task(empty_task));

        let follow_command = Command {
            command_parameter: None,
            command: Some(Follow(FollowObject {
                follow_object_id: "bad-id".to_string(),
            })),
        };
        let invalid_follow_task = Task {
            task_id: Some("01H1VV3VN40RV97CDFSXJB44K9".to_string()),
            task_name: None,
            task_description: None,
            task_start_time: None,
            task_end_time: None,
            control: Some(1),
            region: vec![],
            command: Some(follow_command),
        };
        assert_eq!(
            (
                false,
                "A valid ULID must be used for a follow object ID in a follow object message."
                    .to_string()
            ),
            validate_task(invalid_follow_task)
        );

        let region_task = Task {
            task_id: Some("01H1VV3VN40RV97CDFSXJB44K9".to_string()),
            task_name: None,
            task_description: None,
            task_start_time: None,
            task_end_time: None,
            control: Some(1),
            region: vec![Region {
                r#type: Some(1),
                region_id: Some("01H1VV3VN40RV97CDFSXJB44K9".to_string()),
                region_name: Some("AOI".to_string()),
                region_area: Some(LocationOrRangeBearing {
                    fov_oneof: Some(FovOneof::LocationList(LocationList {
                        locations: vec![Location {
                            x: Some(1.0),
                            y: Some(2.0),
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
                class_filter: vec![ClassFilter {
                    parameter: Some(Parameter {
                        name: Some("confidence".to_string()),
                        operator: Some(1),
                        value: Some(0.5),
                    }),
                    r#type: Some("aircraft".to_string()),
                    sub_class_filter: vec![SubClassFilter {
                        parameter: Some(Parameter {
                            name: Some("size".to_string()),
                            operator: Some(1),
                            value: Some(1.0),
                        }),
                        r#type: Some("small".to_string()),
                        sub_class_filter: vec![],
                        priority: None,
                    }],
                    priority: None,
                }],
                behaviour_filter: vec![BehaviourFilter {
                    parameter: Some(Parameter {
                        name: Some("speed".to_string()),
                        operator: Some(1),
                        value: Some(10.0),
                    }),
                    r#type: Some("moving".to_string()),
                    priority: None,
                }],
            }],
            command: None,
        };
        assert_eq!((true, "".to_string()), validate_task(region_task));
    }

    /// Unit test to check that task IDs are correctly validated
    #[test]
    fn test_task_id_validation() {
        // valid task ID
        let valid_task_id = "01H1VV3VN40RV97CDFSXJB44K9".to_string();
        assert_eq!(
            (true, "".to_string()),
            validate_task_id(Some(valid_task_id))
        );

        // invalid report ID
        let invalid_task_id = "".to_string();
        assert_eq!(
            (
                false,
                "A valid ULID must be used for a task ID in a task message.".to_string()
            ),
            validate_task_id(Some(invalid_task_id))
        );
    }

    /// Unit test to check that controls are correctly validated, including
    /// that the `reserved` value 4 (`CONTROL_DEFAULT`, removed from
    /// `task.proto`) is rejected, not accepted as a valid control.
    #[test]
    fn test_control_validation() {
        // valid control
        for control in 1..4 {
            assert_eq!((true, "".to_string()), validate_control(Some(control)));
        }

        // invalid control
        for control in [0, 4, 5] {
            assert_eq!(
                (
                    false,
                    "Control must be specified in a task message.".to_string()
                ),
                validate_control(Some(control))
            );
        }
    }
}
