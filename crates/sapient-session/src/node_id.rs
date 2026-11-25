//! Choosing which peer node ID the harness addresses its messages to.

use sapient_conformance_core::validation::common::validate_uuid_v4;

/// `id` if it's a valid node ID (a UUID v4), otherwise `None`. The harness
/// only uses a valid peer ID as `destination_id`, so a peer that sends a
/// malformed `node_id` has it reported once, as an envelope finding on its
/// own message, rather than echoed back on everything the harness sends.
pub(crate) fn valid_node_id(id: Option<&str>) -> Option<String> {
    let id = id?;
    // `validate_uuid_v4` needs a rule ID, but this check never reports one.
    validate_uuid_v4(Some(id), "", "")
        .passed
        .then(|| id.to_string())
}
