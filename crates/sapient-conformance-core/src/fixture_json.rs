//! Decodes canonical protobuf JSON (the format SAPIENT conformance
//! fixtures are written in -- see `tests/fixtures/README.md`) into a
//! [`SapientMessage`]. Real code, not test scaffolding: `tests/parity.rs`
//! uses this to run the fixture set at build/test time, and the CLI's
//! `selftest` command (Milestone 3) uses the same function to run the same
//! fixtures, bundled into the shipped binary, at run time -- one
//! definition of "how a fixture file becomes a `SapientMessage`" rather
//! than two.

use std::fmt;

use prost::Message;
use prost_for_reflect::Message as _;
use prost_reflect::{DescriptorPool, DynamicMessage, MessageDescriptor};

use crate::bsi_flex_335_v2_0::SapientMessage;

const SAPIENT_MESSAGE_TYPE: &str = "sapient_msg.bsi_flex_335_v2_0.SapientMessage";

/// `False/` fixtures whose violation can only be expressed in JSON as a
/// protobuf JSON parse failure, so they never reach a validator rule.
/// Keyed by `<dir>::<file stem>`, as `tests/parity.rs` and `selftest` name
/// fixtures. Any other `False/` fixture must decode and be rejected by a
/// rule; `tests/parity.rs` fails if one doesn't, or if an entry here
/// decodes.
pub const PARSE_ONLY_FIXTURES: &[(&str, &str)] = &[(
    "False::0001.Timestamp.Error",
    "a non-RFC 3339 timestamp string cannot decode into google.protobuf.Timestamp; \
     sapient_message.timestamp.malformed is covered by unit tests instead",
)];

/// Whether `fixture` (named `<dir>::<file stem>`) is listed in
/// [`PARSE_ONLY_FIXTURES`].
pub fn is_parse_only_fixture(fixture: &str) -> bool {
    PARSE_ONLY_FIXTURES.iter().any(|(name, _)| *name == fixture)
}

#[derive(Debug)]
pub enum DecodeError {
    Json(serde_json::Error),
    Protobuf(prost::DecodeError),
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecodeError::Json(err) => write!(f, "failed to parse as protobuf JSON: {err}"),
            DecodeError::Protobuf(err) => {
                write!(
                    f,
                    "decoded JSON re-encoded to invalid protobuf bytes: {err}"
                )
            }
        }
    }
}

impl std::error::Error for DecodeError {}

/// The `sapient_msg.bsi_flex_335_v2_0.SapientMessage` descriptor, decoded
/// from `sapient-rs`'s embedded file descriptor set. Building this is the
/// only fallible, one-time setup [`decode_sapient_message_json`] needs;
/// callers processing many fixtures should build it once and reuse it.
pub fn sapient_message_descriptor() -> MessageDescriptor {
    let pool = DescriptorPool::decode(sapient_rs::FILE_DESCRIPTOR_SET_BYTES)
        .expect("sapient-rs's embedded file descriptor set should be valid");
    pool.get_message_by_name(SAPIENT_MESSAGE_TYPE)
        .unwrap_or_else(|| panic!("descriptor pool is missing {SAPIENT_MESSAGE_TYPE}"))
}

/// Decodes one canonical-protobuf-JSON fixture into a [`SapientMessage`].
pub fn decode_sapient_message_json(
    json: &str,
    message_descriptor: &MessageDescriptor,
) -> Result<SapientMessage, DecodeError> {
    let mut deserializer = serde_json::Deserializer::from_str(json);
    let dynamic_message =
        DynamicMessage::deserialize(message_descriptor.clone(), &mut deserializer)
            .map_err(DecodeError::Json)?;
    // A JSON document with trailing garbage after the message is also a
    // parse failure -- `Deserializer::deserialize` alone wouldn't notice.
    deserializer.end().map_err(DecodeError::Json)?;

    let bytes = dynamic_message.encode_to_vec();
    SapientMessage::decode(bytes.as_slice()).map_err(DecodeError::Protobuf)
}
