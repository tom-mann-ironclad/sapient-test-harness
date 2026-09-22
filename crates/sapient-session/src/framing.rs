//! The wire framing shared by every driver and test in this crate: a
//! 4-byte little-endian length prefix followed by that many bytes of
//! payload (matching `sapient-rs::utils`'s own framing). Deliberately
//! not `sapient_rs::utils::send`/`read`, which decode into a typed
//! `M: Message` internally and don't hand back the raw bytes on a decode
//! failure -- every caller here needs the raw payload regardless of
//! whether it turns out to decode, so `read_frame` returns bytes and
//! leaves decoding to the caller (see `dmm.rs`/`asm.rs` for why: a
//! decode failure is itself a session-observable event, not just an I/O
//! error).

use std::io;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Reads one length-prefixed frame's payload. Returns `Ok(None)` on a
/// clean disconnect (EOF exactly at a frame boundary); any other I/O
/// error (including a partial frame, i.e. EOF mid-read) propagates.
pub async fn read_frame<S: AsyncRead + Unpin>(stream: &mut S) -> io::Result<Option<Vec<u8>>> {
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

pub async fn write_frame<S: AsyncWrite + Unpin>(stream: &mut S, payload: &[u8]) -> io::Result<()> {
    let length = (payload.len() as u32).to_le_bytes();
    stream.write_all(&length).await?;
    stream.write_all(payload).await?;
    Ok(())
}
