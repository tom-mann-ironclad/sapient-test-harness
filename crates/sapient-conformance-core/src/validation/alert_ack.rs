use crate::bsi_flex_335_v2_0::AlertAck;
use crate::finding::ValidationOutcome;
use crate::validation::common::validate_ulid;

/// Function to validation a SAPIENT alert acknowledgement message
pub fn validate_alert_ack(alert_ack: AlertAck) -> ValidationOutcome {
    let validations = vec![
        validate_alert_id(alert_ack.alert_id),
        validate_status(alert_ack.alert_ack_status),
    ];

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
        "alert_ack.alert_id.invalid",
        "A valid ULID must be used for an alert ID in an alert ack message.",
    )
}

/// Function to check a status as specified in the SAPIENT version 7 ICD
fn validate_status(status: Option<i32>) -> ValidationOutcome {
    let valid_status = match status {
        None | Some(0) => false,
        Some(1) => true,
        Some(2) => true,
        Some(3) => true,
        _ => false,
    };
    if !valid_status {
        return ValidationOutcome::fail(
            "alert_ack.alert_ack_status.invalid",
            "Alert status must be specified in an alert ack message.",
        );
    }

    ValidationOutcome::pass()
}

#[cfg(test)]
mod alert_ack_validation_tests {
    use crate::{
        bsi_flex_335_v2_0::AlertAck,
        finding::ValidationOutcome,
        validation::alert_ack::{validate_alert_ack, validate_alert_id, validate_status},
    };

    /// Unit test to check that alert acknowledgements are correctly validated
    #[test]
    fn test_alert_ack_validation() {
        // valid alert
        let valid_alert_ack = AlertAck {
            alert_id: Some("01H1VV3VN40RV97CDFSXJB44K9".to_string()),
            alert_ack_status: Some(1),
            reason: vec![],
        };
        assert_eq!(
            ValidationOutcome::pass(),
            validate_alert_ack(valid_alert_ack)
        );

        // invalid alert
        let invalid_alert_ack = AlertAck {
            alert_id: None,
            alert_ack_status: Some(1),
            reason: vec![],
        };
        assert_eq!(
            ValidationOutcome::fail(
                "alert_ack.alert_id.invalid",
                "A valid ULID must be used for an alert ID in an alert ack message."
            ),
            validate_alert_ack(invalid_alert_ack)
        );

        // missing status alert
        let missing_status_alert_ack = AlertAck {
            alert_id: Some("01H1VV3VN40RV97CDFSXJB44K9".to_string()),
            alert_ack_status: None,
            reason: vec![],
        };
        assert_eq!(
            ValidationOutcome::fail(
                "alert_ack.alert_ack_status.invalid",
                "Alert status must be specified in an alert ack message."
            ),
            validate_alert_ack(missing_status_alert_ack)
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

        // invalid report ID
        let invalid_alert_id = "".to_string();
        assert_eq!(
            ValidationOutcome::fail(
                "alert_ack.alert_id.invalid",
                "A valid ULID must be used for an alert ID in an alert ack message."
            ),
            validate_alert_id(Some(invalid_alert_id))
        );
    }

    /// Unit test to check that statuses are correctly validated
    #[test]
    fn test_status_validation() {
        // valid status
        for status in 1..4 {
            assert_eq!(ValidationOutcome::pass(), validate_status(Some(status)));
        }

        // invalid status
        assert_eq!(
            ValidationOutcome::fail(
                "alert_ack.alert_ack_status.invalid",
                "Alert status must be specified in an alert ack message."
            ),
            validate_status(Some(0))
        );
        assert_eq!(
            ValidationOutcome::fail(
                "alert_ack.alert_ack_status.invalid",
                "Alert status must be specified in an alert ack message."
            ),
            validate_status(Some(4))
        );
    }
}
