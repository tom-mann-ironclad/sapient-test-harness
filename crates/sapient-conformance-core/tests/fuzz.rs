//! Property-based tests: decode arbitrary byte strings as each SAPIENT
//! message type and assert the corresponding validator never panics,
//! regardless of whether the decoded message is well-formed, garbage, or
//! anywhere in between.
//!
//! `tests/parity.rs` checks *correctness* against known-good/known-bad
//! fixtures a human picked. This checks *robustness*: the parts of the
//! state space no fixture author would think to write by hand -- an
//! `Option` that's `None` somewhere a validator assumed it had already
//! been checked, an out-of-range enum value, an empty string where a
//! `.chars().next()` was assumed, NaN in a float field -- which is
//! exactly the class of bug fixed fixtures can't find. This is also a
//! reasonable proxy for what a real, buggy or malicious ASM/DMM
//! implementation might send once this harness is actually on the wire.
//!
//! `prost::Message::decode` itself is trusted not to panic on malformed
//! input (well-tested upstream, and bounds its own recursion depth on
//! self-referential messages like `SubClass`) -- these tests exercise
//! this crate's *own* validation logic on whatever it successfully
//! decodes, not the decoder.

use proptest::prelude::*;
use prost::Message;
use sapient_conformance_core::{
    bsi_flex_335_v2_0::{
        Alert, AlertAck, DetectionReport, Error, Registration, RegistrationAck, SapientMessage,
        StatusReport, Task, TaskAck,
    },
    validation::{
        alert::validate_alert, alert_ack::validate_alert_ack,
        detection_report::validate_detection_report, error::validate_error,
        registration::validate_registration, registration_ack::validate_registration_ack,
        sapient_message::validate_sapient_message, status_report::validate_status_report,
        task::validate_task, task_ack::validate_task_ack,
    },
};

/// Generates one proptest for `$message_type`: decode arbitrary bytes as
/// it, and if that succeeds, feed the result to `$validate` and just
/// require it doesn't panic -- the pass/fail *outcome* is irrelevant
/// here, only whether evaluating it blows up.
macro_rules! never_panics_on_arbitrary_bytes {
    ($test_name:ident, $message_type:ty, $validate:expr) => {
        proptest! {
            #[test]
            fn $test_name(bytes in prop::collection::vec(any::<u8>(), 0..2048)) {
                if let Ok(message) = <$message_type>::decode(bytes.as_slice()) {
                    let _ = $validate(message);
                }
            }
        }
    };
}

never_panics_on_arbitrary_bytes!(
    sapient_message_validation_never_panics,
    SapientMessage,
    validate_sapient_message
);
never_panics_on_arbitrary_bytes!(
    registration_validation_never_panics,
    Registration,
    validate_registration
);
never_panics_on_arbitrary_bytes!(
    registration_ack_validation_never_panics,
    RegistrationAck,
    validate_registration_ack
);
never_panics_on_arbitrary_bytes!(
    status_report_validation_never_panics,
    StatusReport,
    validate_status_report
);
never_panics_on_arbitrary_bytes!(
    detection_report_validation_never_panics,
    DetectionReport,
    validate_detection_report
);
never_panics_on_arbitrary_bytes!(task_validation_never_panics, Task, validate_task);
never_panics_on_arbitrary_bytes!(task_ack_validation_never_panics, TaskAck, validate_task_ack);
never_panics_on_arbitrary_bytes!(alert_validation_never_panics, Alert, validate_alert);
never_panics_on_arbitrary_bytes!(
    alert_ack_validation_never_panics,
    AlertAck,
    validate_alert_ack
);
never_panics_on_arbitrary_bytes!(error_validation_never_panics, Error, validate_error);
