//! Async ASM-role driver: owns the actual socket I/O and framing around
//! the pure [`AsmSession`] state machine.
//!
//! Unlike the DMM driver (`dmm.rs`, a simple read-and-reply loop -- the
//! DMM role is purely reactive once connected), the ASM role needs to
//! both *actively send* (`Registration` first, then whatever
//! `StatusReport`/`DetectionReport`/`Alert` traffic a test scenario
//! wants, on its own schedule) and *reactively receive* (`Task`,
//! `AlertAck`, auto-replying with `TaskAck` where needed) against the
//! same connection. [`AsmConnection`] splits the stream into independent
//! read/write halves so a caller (a test, or eventually a scenario
//! script) can freely interleave `register`/`issue_*` calls with
//! `poll_once` calls without them fighting over one shared stream.

use std::io;

use tokio::io::{AsyncRead, AsyncWrite};

use crate::asm_state::{AsmEvent, AsmSession, AsmSessionState};
use crate::framing::{FrameReader, FrameWriter};
use sapient_conformance_core::{
    bsi_flex_335_v2_0::{Alert, DetectionReport, Registration, StatusReport},
    finding::Finding,
};

/// Drives one ASM-role session over an already-connected stream (a
/// connected `TcpStream`, or one half of a `tokio::io::duplex` in tests),
/// split into independent read (`R`) and write (`W`) halves.
pub struct AsmConnection<R, W> {
    session: AsmSession,
    reader: R,
    writer: W,
    /// Retained across cancelled polls and interleaved proactive sends.
    frame_reader: FrameReader,
    /// Outbound bytes retained until fully written, including automatic replies.
    frame_writer: FrameWriter,
    /// An inbound frame was processed, but its poll has not yet returned.
    pending_poll: bool,
}

impl<R, W> AsmConnection<R, W>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    pub fn new(harness_node_id: impl Into<String>, reader: R, writer: W) -> Self {
        AsmConnection {
            session: AsmSession::new(harness_node_id),
            reader,
            writer,
            frame_reader: FrameReader::default(),
            frame_writer: FrameWriter::default(),
            pending_poll: false,
        }
    }

    /// Consume progress from the most recently processed inbound message.
    /// Call after `poll_once` returns `Ok(true)`, before polling another frame.
    /// Returns an event at most once; `None` is not a conformance verdict.
    /// EOF and I/O errors are conveyed by `poll_once`, not by this event slot.
    pub fn take_event(&mut self) -> Option<AsmEvent> {
        self.session.take_event()
    }

    pub fn state(&self) -> &AsmSessionState {
        self.session.state()
    }

    pub fn findings(&self) -> &[Finding] {
        self.session.findings()
    }

    pub fn take_findings(&mut self) -> Vec<Finding> {
        self.session.take_findings()
    }

    /// Send our `Registration` to the DMM. Must happen before anything
    /// else -- see [`AsmSession::register`].
    pub async fn register(&mut self, registration: Registration) -> io::Result<()> {
        self.frame_writer.drain(&mut self.writer).await?;
        let bytes = self.session.register(registration);
        self.frame_writer.queue(&bytes);
        self.frame_writer.drain(&mut self.writer).await
    }

    pub async fn issue_status_report(&mut self, status_report: StatusReport) -> io::Result<()> {
        self.frame_writer.drain(&mut self.writer).await?;
        let bytes = self.session.issue_status_report(status_report);
        self.frame_writer.queue(&bytes);
        self.frame_writer.drain(&mut self.writer).await
    }

    pub async fn issue_detection_report(
        &mut self,
        detection_report: DetectionReport,
    ) -> io::Result<()> {
        self.frame_writer.drain(&mut self.writer).await?;
        let bytes = self.session.issue_detection_report(detection_report);
        self.frame_writer.queue(&bytes);
        self.frame_writer.drain(&mut self.writer).await
    }

    pub async fn issue_alert(&mut self, alert: Alert) -> io::Result<()> {
        self.frame_writer.drain(&mut self.writer).await?;
        let bytes = self.session.issue_alert(alert);
        self.frame_writer.queue(&bytes);
        self.frame_writer.drain(&mut self.writer).await
    }

    /// Read and process exactly one inbound frame, auto-replying if the
    /// protocol requires it (e.g. a `TaskAck` in response to a `Task`).
    /// Returns `Ok(false)` on a clean disconnect, so a caller can loop
    /// `while connection.poll_once().await? {}` to drain everything the
    /// peer sends until it closes the connection.
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
                if let Some(reply) = self.session.on_bytes(&raw) {
                    self.frame_writer.queue(&reply);
                    self.pending_poll = true;
                    self.frame_writer.drain(&mut self.writer).await?;
                    self.pending_poll = false;
                }
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// Poll until the peer disconnects, auto-replying to everything that
    /// needs it along the way. Convenience for scenarios that don't need
    /// to interleave their own sends with reads (e.g. "register, then
    /// just wait for the DMM to do its thing and hang up").
    pub async fn run_until_disconnect(&mut self) -> io::Result<()> {
        while self.poll_once().await? {}
        Ok(())
    }
}
