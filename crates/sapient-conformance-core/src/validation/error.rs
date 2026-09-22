use crate::bsi_flex_335_v2_0::Error;
use crate::finding::ValidationOutcome;

/// Function to validation a SAPIENT error message
pub fn validate_error(error: Error) -> ValidationOutcome {
    let validations = vec![
        validate_packet(error.packet),
        validate_error_message(error.error_message),
    ];

    for validation in validations {
        if !validation.passed {
            return validation;
        }
    }

    ValidationOutcome::pass()
}

/// Function to check a packet as specified in the SAPIENT version 7 ICD
fn validate_packet(packet: Option<Vec<u8>>) -> ValidationOutcome {
    match packet {
        Some(data) => {
            if data.is_empty() {
                return ValidationOutcome::fail(
                    "error.packet.empty",
                    "Error must contain a populated packet.",
                );
            }
        }
        None => {
            return ValidationOutcome::fail(
                "error.packet.missing",
                "Error must contain a populated packet.",
            );
        }
    };

    ValidationOutcome::pass()
}

/// Function to check an error message as specified in the SAPIENT version 7 ICD
fn validate_error_message(error_message: Vec<String>) -> ValidationOutcome {
    if error_message.is_empty() || error_message.contains(&"".to_string()) {
        return ValidationOutcome::fail(
            "error.error_message.empty",
            "Error must contain a populated error message.",
        );
    }

    ValidationOutcome::pass()
}

#[cfg(test)]
mod error_validation_tests {
    use crate::{
        bsi_flex_335_v2_0::Error,
        finding::ValidationOutcome,
        validation::error::{validate_error, validate_error_message, validate_packet},
    };

    /// Unit test to check that errors are correctly validated
    #[test]
    fn test_error_validation() {
        // valid error
        let valid_error = Error {
            packet: Some(vec![1, 2, 3]),
            error_message: vec!["Invalid SAPIENT message".to_string()],
        };
        assert_eq!(ValidationOutcome::pass(), validate_error(valid_error));

        // invalid error
        let invalid_error = Error {
            packet: None,
            error_message: vec!["Invalid SAPIENT message".to_string()],
        };
        assert_eq!(
            ValidationOutcome::fail(
                "error.packet.missing",
                "Error must contain a populated packet."
            ),
            validate_error(invalid_error)
        );
    }

    /// Unit test to check that packets are correctly validated
    #[test]
    fn test_packet_validation() {
        // valid packet
        let valid_packet = vec![1, 2, 3];
        assert_eq!(
            ValidationOutcome::pass(),
            validate_packet(Some(valid_packet))
        );

        // invalid packet (present but empty)
        let invalid_packet = vec![];
        assert_eq!(
            ValidationOutcome::fail(
                "error.packet.empty",
                "Error must contain a populated packet."
            ),
            validate_packet(Some(invalid_packet))
        );

        // missing packet
        assert_eq!(
            ValidationOutcome::fail(
                "error.packet.missing",
                "Error must contain a populated packet."
            ),
            validate_packet(None)
        );
    }

    /// Unit test to check that error messages are correctly validated
    #[test]
    fn test_error_message_validation() {
        // valid message
        assert_eq!(
            ValidationOutcome::pass(),
            validate_error_message(vec!["Invalid SAPIENT message".to_string()])
        );

        // invalid message
        assert_eq!(
            ValidationOutcome::fail(
                "error.error_message.empty",
                "Error must contain a populated error message."
            ),
            validate_error_message(vec![])
        );
    }
}
