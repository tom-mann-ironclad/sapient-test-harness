use crate::bsi_flex_335_v2_0::TaskAck;
use crate::finding::ValidationOutcome;
use crate::validation::common::{validate_associated_file, validate_ulid};

/// Function to validation a SAPIENT task acknowledgement message
pub fn validate_task_ack(task_ack: TaskAck) -> ValidationOutcome {
    let mut validations = vec![];

    // Check task ID
    validations.push(validate_task_id(task_ack.task_id));

    // Check status
    validations.push(validate_status(task_ack.task_status));

    if let Some(associated_file) = task_ack.associated_file {
        validations.push(validate_associated_file(
            associated_file,
            "task_ack.associated_file",
            "Associated file type must be specified in a task ack message.",
            "Associated file URL must be specified in a task ack message.",
        ));
    }

    for validation in validations {
        if !validation.passed {
            return validation;
        }
    }

    ValidationOutcome::pass()
}

/// Function to check a task ID as specified in the SAPIENT version 7 ICD
fn validate_task_id(task_id: Option<String>) -> ValidationOutcome {
    validate_ulid(
        task_id.as_deref(),
        "task_ack.task_id.invalid",
        "A valid ULID must be used for a task ID in a task ack message.",
    )
}

/// Function to check a status as specified in the SAPIENT version 7 ICD
fn validate_status(status: Option<i32>) -> ValidationOutcome {
    let valid_status = match status {
        None | Some(0) => false,
        Some(1) => true,
        Some(2) => true,
        Some(3) => true,
        Some(4) => true,
        _ => false,
    };
    if !valid_status {
        return ValidationOutcome::fail(
            "task_ack.task_status.invalid",
            "Task status must be specified in a task ack message.",
        );
    }

    ValidationOutcome::pass()
}

#[cfg(test)]
mod task_ack_validation_tests {
    use crate::{
        bsi_flex_335_v2_0::{AssociatedFile, TaskAck},
        finding::ValidationOutcome,
        validation::task_ack::{validate_status, validate_task_ack, validate_task_id},
    };

    /// Unit test to check that task acknowledgements are correctly validated
    #[test]
    fn test_task_ack_validation() {
        // valid task
        let valid_task_ack = TaskAck {
            task_id: Some("01H1VV3VN40RV97CDFSXJB44K9".to_string()),
            task_status: Some(1),
            reason: vec![],
            associated_file: None,
        };
        assert_eq!(ValidationOutcome::pass(), validate_task_ack(valid_task_ack));

        // invalid task
        let invalid_task_ack = TaskAck {
            task_id: None,
            task_status: Some(1),
            reason: vec![],
            associated_file: None,
        };
        assert_eq!(
            ValidationOutcome::fail(
                "task_ack.task_id.invalid",
                "A valid ULID must be used for a task ID in a task ack message."
            ),
            validate_task_ack(invalid_task_ack)
        );

        let invalid_file_task_ack = TaskAck {
            task_id: Some("01H1VV3VN40RV97CDFSXJB44K9".to_string()),
            task_status: Some(1),
            reason: vec![],
            associated_file: Some(AssociatedFile {
                r#type: None,
                url: Some("https://example.test/file".to_string()),
            }),
        };
        assert_eq!(
            ValidationOutcome::fail(
                "task_ack.associated_file.type.missing",
                "Associated file type must be specified in a task ack message."
            ),
            validate_task_ack(invalid_file_task_ack)
        );
    }

    /// Unit test to check that task IDs are correctly validated
    #[test]
    fn test_task_id_validation() {
        // valid task ID
        let valid_task_id = "01H1VV3VN40RV97CDFSXJB44K9".to_string();
        assert_eq!(
            ValidationOutcome::pass(),
            validate_task_id(Some(valid_task_id))
        );

        // invalid report ID
        let invalid_task_id = "".to_string();
        assert_eq!(
            ValidationOutcome::fail(
                "task_ack.task_id.invalid",
                "A valid ULID must be used for a task ID in a task ack message."
            ),
            validate_task_id(Some(invalid_task_id))
        );
    }

    /// Unit test to check that statuses are correctly validated
    #[test]
    fn test_status_validation() {
        // valid status
        for status in 1..5 {
            assert_eq!(ValidationOutcome::pass(), validate_status(Some(status)));
        }

        // invalid status
        assert_eq!(
            ValidationOutcome::fail(
                "task_ack.task_status.invalid",
                "Task status must be specified in a task ack message."
            ),
            validate_status(Some(0))
        );
        assert_eq!(
            ValidationOutcome::fail(
                "task_ack.task_status.invalid",
                "Task status must be specified in a task ack message."
            ),
            validate_status(Some(5))
        );
    }
}
