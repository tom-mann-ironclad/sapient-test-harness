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
pub struct FrameReader {
    /// Header bytes and the number already consumed.
    header: [u8; 4],
    header_read: usize,
    /// Small reusable allocation; grows only as payload bytes arrive.
    payload: Vec<u8>,
    payload_read: usize,
    /// Local resource policy, not a protocol conformance rule.
    max_frame_bytes: u32,
    /// Called once per large header, before receiving its payload.
    large_message_warning: Option<fn(u32)>,
    /// Prevent duplicate warnings when a payload read is cancelled.
    header_checked: bool,
}

/// Default local receive limit (64 MiB); callers may raise it for large-message tests.
pub const DEFAULT_MAX_FRAME_BYTES: u32 = 64 * 1024 * 1024;
/// Normal reusable allocation and incremental receive chunk size (64 KiB).
pub const RECEIVE_BUFFER_BYTES: usize = 64 * 1024;
/// Announce frames of at least 1 MiB before receiving their payload.
pub const LARGE_MESSAGE_BYTES: u32 = 1024 * 1024;

impl Default for FrameReader {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_FRAME_BYTES)
    }
}

impl FrameReader {
    /// Configure a local payload limit in bytes, excluding the four-byte header.
    pub fn new(max_frame_bytes: u32) -> Self {
        Self {
            header: [0; 4],
            header_read: 0,
            payload: Vec::with_capacity(RECEIVE_BUFFER_BYTES),
            payload_read: 0,
            max_frame_bytes,
            large_message_warning: None,
            header_checked: false,
        }
    }

    /// Install an immediate notification hook. Libraries are silent by default;
    /// CLI callers use stderr so JSON output remains machine-readable.
    pub fn with_large_message_warning(mut self, warning: fn(u32)) -> Self {
        self.large_message_warning = Some(warning);
        self
    }

    /// Return a completed payload after processing to reuse its allocation.
    /// Large allocations are replaced with the normal small buffer immediately.
    /// Call before starting another read; outstanding partial reads are untouched.
    pub fn recycle(&mut self, mut payload: Vec<u8>) {
        if self.header_read != 0 {
            return;
        }
        payload.clear();
        self.payload = if payload.capacity() > RECEIVE_BUFFER_BYTES {
            drop(payload);
            Vec::with_capacity(RECEIVE_BUFFER_BYTES)
        } else {
            payload
        };
    }

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
        if !self.header_checked {
            if length > self.max_frame_bytes as usize {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "frame advertises {length} payload bytes, exceeding the configured receive limit of {} bytes; this is a harness resource limit, not a conformance finding",
                        self.max_frame_bytes
                    ),
                ));
            }
            self.header_checked = true;
            if length >= LARGE_MESSAGE_BYTES as usize
                && let Some(warning) = self.large_message_warning
            {
                warning(length as u32);
            }
        }
        while self.payload_read < length {
            // Read into a bounded scratch buffer: an advertised length alone
            // cannot grow the reusable payload allocation.
            let mut chunk = [0; RECEIVE_BUFFER_BYTES];
            let wanted = (length - self.payload_read).min(chunk.len());
            let n = stream.read(&mut chunk[..wanted]).await?;
            if n == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "truncated frame payload",
                ));
            }
            let needed = self.payload.len() + n;
            if needed > self.payload.capacity() {
                // Geometric growth avoids repeatedly copying a large frame.
                // Reserve only after bytes arrive, never from the header alone.
                let capacity = needed
                    .max(self.payload.capacity().saturating_mul(2))
                    .min(length);
                self.payload
                    .try_reserve_exact(capacity - self.payload.len())
                    .map_err(|err| {
                        io::Error::other(format!("could not allocate receive buffer: {err}"))
                    })?;
            }
            self.payload.extend_from_slice(&chunk[..n]);
            self.payload_read += n;
        }
        self.header_checked = false;
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
    pub(crate) fn queue(&mut self, payload: &[u8]) -> io::Result<()> {
        let length = encoded_length(payload.len())?;
        assert!(self.bytes.is_empty(), "pending frame must be drained first");
        self.bytes.extend_from_slice(&length);
        self.bytes.extend_from_slice(payload);
        Ok(())
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
        if self.bytes.capacity() > RECEIVE_BUFFER_BYTES {
            self.bytes = Vec::new();
        }
        self.written = 0;
        Ok(())
    }
}

pub async fn write_frame<S: AsyncWrite + Unpin>(stream: &mut S, payload: &[u8]) -> io::Result<()> {
    let length = encoded_length(payload.len())?;
    stream.write_all(&length).await?;
    stream.write_all(payload).await?;
    Ok(())
}

/// Validate the wire-length conversion without allocating an oversized payload.
fn encoded_length(length: usize) -> io::Result<[u8; 4]> {
    u32::try_from(length).map(u32::to_le_bytes).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "outgoing payload exceeds the four-byte frame length",
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::atomic::{AtomicUsize, Ordering},
        time::Duration,
    };
    use tokio::time::timeout;

    #[tokio::test]
    async fn limit_is_checked_from_header_without_payload_allocation() {
        for length in [9_u32, u32::MAX] {
            let mut reader = FrameReader::new(8);
            let wire = length.to_le_bytes();
            assert_eq!(
                reader.read(&mut wire.as_slice()).await.unwrap_err().kind(),
                io::ErrorKind::InvalidData
            );
            assert_eq!(reader.payload.capacity(), RECEIVE_BUFFER_BYTES);
            assert!(reader.payload.is_empty());
        }
        for length in [0, 8] {
            let mut wire = (length as u32).to_le_bytes().to_vec();
            wire.extend(vec![42; length]);
            assert_eq!(
                FrameReader::new(8)
                    .read(&mut wire.as_slice())
                    .await
                    .unwrap(),
                Some(vec![42; length])
            );
        }
    }

    #[tokio::test]
    async fn advertised_two_gib_does_not_allocate_two_gib_or_repeat_warning() {
        static WARNINGS: AtomicUsize = AtomicUsize::new(0);
        fn warn(length: u32) {
            assert_eq!(length, 2 * 1024 * 1024 * 1024);
            WARNINGS.fetch_add(1, Ordering::SeqCst);
        }
        let mut reader = FrameReader::new(u32::MAX).with_large_message_warning(warn);
        let (mut stream, mut peer) = tokio::io::duplex(64);
        peer.write_all(&(2_u32 * 1024 * 1024 * 1024).to_le_bytes())
            .await
            .unwrap();
        for _ in 0..2 {
            assert!(
                timeout(Duration::from_millis(5), reader.read(&mut stream))
                    .await
                    .is_err()
            );
            assert_eq!(reader.payload.capacity(), RECEIVE_BUFFER_BYTES);
        }
        assert_eq!(WARNINGS.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn small_buffers_are_reused_and_large_buffers_are_released() {
        let mut reader = FrameReader::default();
        let original = reader.payload.as_ptr();
        for length in [5, 0, RECEIVE_BUFFER_BYTES, RECEIVE_BUFFER_BYTES * 3, 5] {
            let mut wire = (length as u32).to_le_bytes().to_vec();
            wire.extend(vec![42; length]);
            let payload = reader.read(&mut wire.as_slice()).await.unwrap().unwrap();
            assert_eq!(payload, vec![42; length]);
            if length == 0 || length == RECEIVE_BUFFER_BYTES {
                assert_eq!(payload.as_ptr(), original);
            }
            reader.recycle(payload);
            assert_eq!(reader.payload.capacity(), RECEIVE_BUFFER_BYTES);
            assert!(reader.payload.is_empty());
        }
    }

    #[test]
    fn outgoing_length_conversion_never_truncates() {
        assert_eq!(encoded_length(0).unwrap(), [0; 4]);
        assert_eq!(encoded_length(u32::MAX as usize).unwrap(), [255; 4]);
        if let Some(too_large) = (u32::MAX as usize).checked_add(1) {
            assert_eq!(
                encoded_length(too_large).unwrap_err().kind(),
                io::ErrorKind::InvalidInput
            );
        }
    }
}
