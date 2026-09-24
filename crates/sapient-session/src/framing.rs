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

/// Incremental frame decoder. Keep one instance per stream across timeouts.
/// Cancelling `read` preserves every consumed byte; retry on the same stream.
#[derive(Default)]
pub struct FrameReader {
    /// Header bytes and the number already consumed.
    header: [u8; 4],
    header_read: usize,
    /// Allocated only after the complete header arrives.
    payload: Vec<u8>,
    payload_read: usize,
}

impl FrameReader {
    /// Read one payload, or `None` for EOF exactly between frames.
    /// EOF inside a header or payload is `UnexpectedEof`. After an I/O error,
    /// discard the connection rather than trying to recover framing.
    pub async fn read<S: AsyncRead + Unpin>(
        &mut self,
        stream: &mut S,
    ) -> io::Result<Option<Vec<u8>>> {
        while self.header_read < 4 {
            let n = stream.read(&mut self.header[self.header_read..]).await?;
            if n == 0 {
                return if self.header_read == 0 {
                    Ok(None)
                } else {
                    Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "truncated frame header",
                    ))
                };
            }
            self.header_read += n;
        }
        let length = u32::from_le_bytes(self.header) as usize;
        self.payload.resize(length, 0);
        while self.payload_read < length {
            let n = stream.read(&mut self.payload[self.payload_read..]).await?;
            if n == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "truncated frame payload",
                ));
            }
            self.payload_read += n;
        }
        self.header_read = 0;
        self.payload_read = 0;
        Ok(Some(std::mem::take(&mut self.payload)))
    }
}

/// Reads one frame without retaining cancellation state. Use [`FrameReader`]
/// if the read may be cancelled and the stream subsequently reused.
pub async fn read_frame<S: AsyncRead + Unpin>(stream: &mut S) -> io::Result<Option<Vec<u8>>> {
    FrameReader::default().read(stream).await
}

/// One pending outbound frame, with progress retained across cancellation.
/// Drivers drain this before queuing any later frame to preserve wire order.
#[derive(Default)]
pub(crate) struct FrameWriter {
    /// Complete wire frame (header followed by payload).
    bytes: Vec<u8>,
    /// Prefix already accepted by the underlying writer.
    written: usize,
}

impl FrameWriter {
    /// Queue a frame synchronously, before the first cancellable write.
    pub(crate) fn queue(&mut self, payload: &[u8]) {
        assert!(self.bytes.is_empty(), "pending frame must be drained first");
        self.bytes
            .extend_from_slice(&(payload.len() as u32).to_le_bytes());
        self.bytes.extend_from_slice(payload);
    }

    /// Finish the pending frame without losing the offset if cancelled.
    pub(crate) async fn drain<S: AsyncWrite + Unpin>(&mut self, stream: &mut S) -> io::Result<()> {
        while self.written < self.bytes.len() {
            let n = stream.write(&self.bytes[self.written..]).await?;
            if n == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "could not write frame",
                ));
            }
            self.written += n;
        }
        self.bytes.clear();
        self.written = 0;
        Ok(())
    }
}

pub async fn write_frame<S: AsyncWrite + Unpin>(stream: &mut S, payload: &[u8]) -> io::Result<()> {
    let length = (payload.len() as u32).to_le_bytes();
    stream.write_all(&length).await?;
    stream.write_all(payload).await?;
    Ok(())
}
