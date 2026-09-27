//! Wire helpers for `sapient-session`'s integration tests, plus a
//! re-export of the crate's default fixture (`sapient_session::fixtures`)
//! so every test file gets it from one place. The fixture builders
//! themselves live in `src/fixtures.rs`, not here -- they're production
//! code now, shared with the CLI's bundled default scenario suite
//! (Milestone 3), not just test scaffolding.
//!
//! `#[allow(dead_code, unused_imports)]` throughout: each test binary only
//! uses a subset of these, and Rust warns per-binary about the rest as
//! unused (dead_code for locally defined items, unused_imports for the
//! re-exported ones).

#![allow(dead_code, unused_imports)]

use prost::Message;
use prost_types::Timestamp;
use sapient_conformance_core::bsi_flex_335_v2_0::{SapientMessage, sapient_message::Content};
use sapient_session::framing::{read_frame, write_frame};

pub use sapient_session::fixtures::{
    ALTERNATE_MODE, DECLARED_CLASSIFICATION_TYPE, DEFAULT_MODE, STATUS_INTERVAL_SECONDS,
    detection_position, duration, mode, valid_location_type, valid_registration,
};

pub fn timestamp(seconds: i64) -> Timestamp {
    Timestamp { seconds, nanos: 0 }
}

/// Wraps `content` in a `SapientMessage` envelope, timestamped at
/// `timestamp_seconds`, from `node_id` addressed to `destination_id`.
pub fn envelope(
    node_id: &str,
    destination_id: &str,
    timestamp_seconds: i64,
    content: Content,
) -> SapientMessage {
    SapientMessage {
        timestamp: Some(timestamp(timestamp_seconds)),
        node_id: Some(node_id.to_string()),
        destination_id: Some(destination_id.to_string()),
        content: Some(content),
        additional_information: None,
    }
}

pub fn encode(message: SapientMessage) -> Vec<u8> {
    message.encode_to_vec()
}

pub fn decode(bytes: &[u8]) -> SapientMessage {
    SapientMessage::decode(bytes).expect("test-constructed messages should always decode")
}

/// Encodes and writes `message` as one length-prefixed frame.
pub async fn send_message<S: tokio::io::AsyncWrite + Unpin>(
    stream: &mut S,
    message: &SapientMessage,
) {
    write_frame(stream, &message.encode_to_vec())
        .await
        .expect("test stream write should not fail");
}

/// Reads and decodes one length-prefixed frame. Panics if the stream ends
/// first -- every test using this expects a specific message to arrive.
pub async fn recv_message<S: tokio::io::AsyncRead + Unpin>(stream: &mut S) -> SapientMessage {
    let payload = read_frame(stream)
        .await
        .expect("test stream read should not fail")
        .expect("stream ended before the expected message arrived");
    decode(&payload)
}
