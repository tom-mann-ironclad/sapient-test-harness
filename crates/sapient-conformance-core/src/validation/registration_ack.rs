use crate::bsi_flex_335_v2_0::RegistrationAck;

/// Function to validation a SAPIENT registration acknowledgement message
pub fn validate_registration_ack(registration_ack: RegistrationAck) -> (bool, String) {
    validate_acceptance(registration_ack.acceptance)
}

fn validate_acceptance(acceptance: Option<bool>) -> (bool, String) {
    match acceptance {
        Some(_) => (true, "".to_string()),
        None => (
            false,
            "Acceptance must be specified in a registration ack message.".to_string(),
        ),
    }
}

#[cfg(test)]
mod registration_validation_tests {
    use crate::validation::registration_ack::*;

    /// Unit test to check that registration acknowledgements are correctly validated
    #[test]
    fn test_registration_ack_validation() {
        let accepted_registration_ack = RegistrationAck {
            acceptance: Some(true),
            ack_response_reason: vec![],
        };
        assert_eq!(
            (true, "".to_string()),
            validate_registration_ack(accepted_registration_ack)
        );

        let rejected_registration_ack = RegistrationAck {
            acceptance: Some(false),
            ack_response_reason: vec!["unsupported mode".to_string()],
        };
        assert_eq!(
            (true, "".to_string()),
            validate_registration_ack(rejected_registration_ack)
        );

        let invalid_registration_ack = RegistrationAck {
            acceptance: None,
            ack_response_reason: vec![],
        };
        assert_eq!(
            (
                false,
                "Acceptance must be specified in a registration ack message.".to_string()
            ),
            validate_registration_ack(invalid_registration_ack)
        );
    }

    #[test]
    fn test_acceptance_validation() {
        assert_eq!((true, "".to_string()), validate_acceptance(Some(true)));
        assert_eq!((true, "".to_string()), validate_acceptance(Some(false)));
        assert_eq!(
            (
                false,
                "Acceptance must be specified in a registration ack message.".to_string()
            ),
            validate_acceptance(None)
        );
    }
}
