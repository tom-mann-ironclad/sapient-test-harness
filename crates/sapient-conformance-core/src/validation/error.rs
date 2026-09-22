use crate::bsi_flex_335_v2_0::Error;

/// Function to validation a SAPIENT error message
pub fn validate_error(error: Error) -> (bool, String) {
    let validations = vec![
        validate_packet(error.packet),
        validate_error_message(error.error_message),
    ];

    for validation in validations {
        if !validation.0 {
            return validation;
        }
    }

    (true, "".to_string())
}

/// Function to check a packet as specified in the SAPIENT version 7 ICD
fn validate_packet(packet: Option<Vec<u8>>) -> (bool, String) {
    match packet {
        Some(data) => {
            if data.is_empty() {
                return (false, "Error must contain a populated packet.".to_string());
            }
        }
        None => return (false, "Error must contain a populated packet.".to_string()),
    };

    (true, "".to_string())
}

/// Function to check an error message as specified in the SAPIENT version 7 ICD
fn validate_error_message(error_message: Vec<String>) -> (bool, String) {
    if error_message.is_empty() || error_message.contains(&"".to_string()) {
        return (
            false,
            "Error must contain a populated error message.".to_string(),
        );
    }

    (true, "".to_string())
}

#[cfg(test)]
mod error_validation_tests {
    use crate::{
        bsi_flex_335_v2_0::Error,
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
        assert_eq!((true, "".to_string()), validate_error(valid_error));

        // invalid error
        let invalid_error = Error {
            packet: None,
            error_message: vec!["Invalid SAPIENT message".to_string()],
        };
        assert_eq!(
            (false, "Error must contain a populated packet.".to_string()),
            validate_error(invalid_error)
        );
    }

    /// Unit test to check that packets are correctly validated
    #[test]
    fn test_packet_validation() {
        // valid packet
        let valid_packet = vec![1, 2, 3];
        assert_eq!((true, "".to_string()), validate_packet(Some(valid_packet)));

        // invalid packet
        let invalid_packet = vec![];
        assert_eq!(
            (false, "Error must contain a populated packet.".to_string()),
            validate_packet(Some(invalid_packet))
        );
    }

    /// Unit test to check that error messages are correctly validated
    #[test]
    fn test_error_message_validation() {
        // valid message
        assert_eq!(
            (true, "".to_string()),
            validate_error_message(vec!["Invalid SAPIENT message".to_string()])
        );

        // invalid message
        assert_eq!(
            (
                false,
                "Error must contain a populated error message.".to_string()
            ),
            validate_error_message(vec![])
        );
    }
}
