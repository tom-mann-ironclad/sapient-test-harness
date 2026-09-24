//! Async DMM-role driver: owns the actual socket I/O and drives the pure
//! [`DmmSession`] state machine as bytes arrive. See `framing.rs` for why
//! this uses its own minimal framing rather than `sapient_rs::utils::read`.
//!
//! [`DmmConnection`] mirrors `asm.rs`'s `AsmConnection`: split read/write
//! halves so a caller can interleave `issue_task` (harness-initiated, the
//! one thing the plain reactive [`run`] loop below can't do -- it owns
//! the whole stream for its entire lifetime with no way to inject a
//! `Task` mid-loop) with `poll_once` (reactive, auto-replying to
//! `Registration`/`Alert`/decode-or-validation failures as needed).

use std::io;

use tokio::io::{AsyncRead, AsyncWrite, split};

use crate::framing::{FrameReader, FrameWriter};
use crate::state::{DmmEvent, DmmSession, SessionState};
use sapient_conformance_core::{bsi_flex_335_v2_0::Task, finding::Finding};

/// Drives one DMM-role session over an already-connected stream (a
/// connected `TcpStream`, or one half of a `tokio::io::duplex` in tests),
/// split into independent read (`R`) and write (`W`) halves.
pub struct DmmConnection<R, W> {
    session: DmmSession,
    reader: R,
    writer: W,
    /// Retained across cancelled polls and interleaved proactive sends.
    frame_reader: FrameReader,
    /// Outbound bytes retained until fully written, including automatic replies.
    frame_writer: FrameWriter,
    /// An inbound frame was processed, but its poll has not yet returned.
    pending_poll: bool,
}

impl<R, W> DmmConnection<R, W>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    pub fn new(harness_node_id: impl Into<String>, reader: R, writer: W) -> Self {
        Self::with_frame_reader(harness_node_id, reader, writer, FrameReader::default())
    }

    /// Construct a connection with a configured receive limit and notification hook.
    pub fn with_frame_reader(
        harness_node_id: impl Into<String>,
        reader: R,
        writer: W,
        frame_reader: FrameReader,
    ) -> Self {
        DmmConnection {
            session: DmmSession::new(harness_node_id),
            reader,
            writer,
            frame_reader,
            frame_writer: FrameWriter::default(),
            pending_poll: false,
        }
    }

    /// Consume progress from the most recently processed inbound message.
    /// Call after `poll_once` returns `Ok(true)`, before polling another frame.
    /// Returns an event at most once; `None` is not a conformance verdict.
    /// EOF and I/O errors are conveyed by `poll_once`, not by this event slot.
    pub fn take_event(&mut self) -> Option<DmmEvent> {
        self.session.take_event()
    }

    pub fn state(&self) -> &SessionState {
        self.session.state()
    }

    pub fn findings(&self) -> &[Finding] {
        self.session.findings()
    }

    pub fn take_findings(&mut self) -> Vec<Finding> {
        self.session.take_findings()
    }

    /// Issue a `Task` to the ASM (harness-initiated, not a reply to
    /// inbound traffic) -- see [`DmmSession::issue_task`].
    pub async fn issue_task(&mut self, task: &Task) -> io::Result<()> {
        self.frame_writer.drain(&mut self.writer).await?;
        let bytes = self.session.issue_task(task);
        self.frame_writer.queue(&bytes)?;
        self.frame_writer.drain(&mut self.writer).await
    }

    /// Read and process exactly one inbound frame, auto-replying if the
    /// protocol requires it (`RegistrationAck`, `AlertAck`, or `Error`).
    /// Returns `Ok(false)` on a clean disconnect.
    /// Cancellation retains partial reads and replies. The next poll finishes
    /// any reply and returns this frame's progress before reading another frame.
    /// Proactive sends also finish pending writes first. Discard the connection
    /// after an I/O error; retrying a cancelled proactive call sends a new message.
    pub async fn poll_once(&mut self) -> io::Result<bool> {
        self.frame_writer.drain(&mut self.writer).await?;
        if self.pending_poll {
            self.pending_poll = false;
            return Ok(true);
        }
        match self.frame_reader.read(&mut self.reader).await? {
            Some(raw) => {
                let reply = self.session.on_bytes(&raw);
                self.frame_reader.recycle(raw);
                if let Some(reply) = reply {
                    self.frame_writer.queue(&reply)?;
                    self.pending_poll = true;
                    self.frame_writer.drain(&mut self.writer).await?;
                    self.pending_poll = false;
                }
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// Whether an outbound frame is still partially or wholly unsent.
    /// A run deadline in this state is an interrupted write, not a completed
    /// observation window. The caller must discard the connection when ending a run.
    pub fn has_pending_write(&self) -> bool {
        self.frame_writer.is_pending()
    }

    /// Poll until the peer disconnects, auto-replying to everything that
    /// needs it along the way.
    pub async fn run_until_disconnect(&mut self) -> io::Result<()> {
        while self.poll_once().await? {}
        Ok(())
    }
}

/// Drives one DMM-role session over an already-connected stream until the
/// peer disconnects, auto-replying to everything reactively. Returns
/// every finding accumulated over the whole session. A thin wrapper
/// around [`DmmConnection`] for the common case that doesn't need to
/// issue any `Task`s -- reach for `DmmConnection` directly when it does.
pub async fn run<S>(harness_node_id: impl Into<String>, stream: S) -> io::Result<Vec<Finding>>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let (reader, writer) = split(stream);
    let mut connection = DmmConnection::new(harness_node_id, reader, writer);
    connection.run_until_disconnect().await?;
    Ok(connection.take_findings())
}
