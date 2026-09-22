use crate::bsi_flex_335_v2_0::RegistrationAck;
use crate::finding::ValidationOutcome;

/// Function to validation a SAPIENT registration acknowledgement message
pub fn validate_registration_ack(registration_ack: RegistrationAck) -> ValidationOutcome {
    validate_acceptance(registration_ack.acceptance)
}

fn validate_acceptance(acceptance: Option<bool>) -> ValidationOutcome {
    match acceptance {
        Some(_) => ValidationOutcome::pass(),
        None => ValidationOutcome::fail(
            "registration_ack.acceptance.missing",
            "Acceptance must be specified in a registration ack message.",
        ),
    }
}

#[cfg(test)]
mod registration_validation_tests {
    use crate::finding::ValidationOutcome;
    use crate::validation::registration_ack::*;

    /// Unit test to check that registration acknowledgements are correctly validated
    #[test]
    fn test_registration_ack_validation() {
        let accepted_registration_ack = RegistrationAck {
            acceptance: Some(true),
            ack_response_reason: vec![],
        };
        assert_eq!(
            ValidationOutcome::pass(),
            validate_registration_ack(accepted_registration_ack)
        );

        let rejected_registration_ack = RegistrationAck {
            acceptance: Some(false),
            ack_response_reason: vec!["unsupported mode".to_string()],
        };
        assert_eq!(
            ValidationOutcome::pass(),
            validate_registration_ack(rejected_registration_ack)
        );

        let invalid_registration_ack = RegistrationAck {
            acceptance: None,
            ack_response_reason: vec![],
        };
        assert_eq!(
            ValidationOutcome::fail(
                "registration_ack.acceptance.missing",
                "Acceptance must be specified in a registration ack message."
            ),
            validate_registration_ack(invalid_registration_ack)
        );
    }

    #[test]
    fn test_acceptance_validation() {
        assert_eq!(ValidationOutcome::pass(), validate_acceptance(Some(true)));
        assert_eq!(ValidationOutcome::pass(), validate_acceptance(Some(false)));
        assert_eq!(
            ValidationOutcome::fail(
                "registration_ack.acceptance.missing",
                "Acceptance must be specified in a registration ack message."
            ),
            validate_acceptance(None)
        );
    }
}
