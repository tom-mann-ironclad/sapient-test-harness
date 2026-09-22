//! Async DMM-role driver: owns the actual socket I/O and framing, and
//! drives the pure [`DmmSession`] state machine as bytes arrive. Uses its
//! own minimal framing (matching `sapient-rs::utils`'s 4-byte
//! little-endian length prefix) rather than `sapient_rs::utils::read`,
//! because that function decodes internally and discards the raw bytes on
//! failure -- this driver needs to keep them, so a decode failure can
//! still be reported (and, once registered, replied to with an `Error`
//! carrying the offending packet) at the session layer rather than
//! surfacing only as an opaque I/O error.

use std::io;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::state::DmmSession;
use sapient_conformance_core::finding::Finding;

/// Drives one DMM-role session over an already-connected stream (an
/// accepted `TcpStream`, or one half of a `tokio::io::duplex` in tests)
/// until the peer disconnects. Returns every finding accumulated over the
/// whole session.
pub async fn run<S>(harness_node_id: impl Into<String>, mut stream: S) -> io::Result<Vec<Finding>>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut session = DmmSession::new(harness_node_id);

    while let Some(raw) = read_frame(&mut stream).await? {
        if let Some(reply) = session.on_bytes(&raw) {
            write_frame(&mut stream, &reply).await?;
        }
    }

    Ok(session.take_findings())
}

/// Reads one length-prefixed frame's payload. Returns `Ok(None)` on a
/// clean disconnect (EOF exactly at a frame boundary); any other I/O
/// error (including a partial frame, i.e. EOF mid-read) propagates.
async fn read_frame<S: AsyncRead + Unpin>(stream: &mut S) -> io::Result<Option<Vec<u8>>> {
    let mut length_buf = [0_u8; 4];
    match stream.read_exact(&mut length_buf).await {
        Ok(_) => {}
        Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(err) => return Err(err),
    }

    let length = u32::from_le_bytes(length_buf) as usize;
    let mut payload = vec![0_u8; length];
    stream.read_exact(&mut payload).await?;
    Ok(Some(payload))
}

async fn write_frame<S: AsyncWrite + Unpin>(stream: &mut S, payload: &[u8]) -> io::Result<()> {
    let length = (payload.len() as u32).to_le_bytes();
    stream.write_all(&length).await?;
    stream.write_all(payload).await?;
    Ok(())
}
