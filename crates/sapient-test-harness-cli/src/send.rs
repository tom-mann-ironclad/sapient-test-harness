//! Implements the `send` subcommand: connects to (or accepts a connection
//! from) the target, then sends each `--file` in order over that one raw
//! connection, printing a conformance warning first if the message doesn't
//! validate, and whatever reply (if any) comes back within
//! `--response-timeout-secs`.
//!
//! Deliberately doesn't use `DmmSession`/`AsmSession`: no active-mode
//! tracking, no correlation, no session-level findings -- this command
//! exists specifically to let a developer send exactly what's in their
//! file, including a deliberately non-conformant message, without the
//! harness's own session rules getting in the way. See `cli.rs`'s
//! `SendArgs` doc comment.
//!
//! [`send_message`] is the testable core (validate, send regardless,
//! observe the reply): it takes an already-decoded [`SapientMessage`] and
//! any `AsyncRead + AsyncWrite` stream, so `tests/send.rs` can drive it
//! directly over a `tokio::io::duplex` without a real socket or a JSON
//! file on disk. Everything file/socket/printing-specific stays out of
//! that function, in this module's other, non-public items.

use std::fs;
use std::io;
use std::net::SocketAddr;
use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;

use prost::Message;
use prost_reflect::MessageDescriptor;
use sapient_conformance_core::{
    bsi_flex_335_v2_0::SapientMessage,
    finding::ValidationOutcome,
    fixture_json::{decode_sapient_message_json, sapient_message_descriptor},
    validation::sapient_message::validate_sapient_message,
};
use sapient_session::framing::{FrameReader, write_frame};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;

use crate::cli::{Role, SendArgs};

/// The result of [`send_message`]: whether the message we sent conformed
/// to this crate's own rules, and whatever happened waiting for a reply.
#[derive(Debug)]
pub struct SendOutcome {
    pub validation: ValidationOutcome,
    pub reply: ReplyOutcome,
}

/// What came back (if anything) after sending a message.
#[derive(Debug)]
pub enum ReplyOutcome {
    /// A frame arrived and decoded as a `SapientMessage`.
    Reply(Box<SapientMessage>),
    /// A frame arrived but didn't decode as a `SapientMessage`.
    UndecodableReply { raw: Vec<u8>, error: String },
    /// The peer closed the connection before replying.
    Disconnected,
    /// No frame arrived within the response timeout.
    TimedOut,
}

/// Validates `message` against this crate's own conformance rules, then
/// sends it over `stream` regardless of the validation outcome. Transmission
/// is bounded by `write_timeout`; discard the stream on a write timeout or
/// other I/O error. Returns once the write has completed, so a caller can
/// print a "sending anyway" warning and confirm the send *before* waiting on
/// [`wait_for_reply`] -- which is exactly what `send`'s CLI driver does, so
/// that output reflects each stage as it actually happens rather than only
/// appearing after the whole round trip (including the reply wait) is over.
pub async fn validate_and_send<S>(
    stream: &mut S,
    message: &SapientMessage,
    write_timeout: Duration,
) -> io::Result<ValidationOutcome>
where
    S: AsyncWrite + Unpin,
{
    let validation = validate_sapient_message(message.clone());

    timeout(write_timeout, write_frame(stream, &message.encode_to_vec()))
        .await
        .map_err(|_| {
            io::Error::new(
                io::ErrorKind::TimedOut,
                "message write timed out; connection must be closed",
            )
        })??;

    Ok(validation)
}

/// Waits up to `response_timeout` for one reply frame after a send. Retain
/// `reader` for the lifetime of this stream so a timeout can resume a
/// partial reply on the next call. Replies are observed in wire order; a
/// late reply is not necessarily a response to the latest sent file.
pub async fn wait_for_reply<S>(
    stream: &mut S,
    reader: &mut FrameReader,
    response_timeout: Duration,
) -> io::Result<ReplyOutcome>
where
    S: AsyncRead + Unpin,
{
    let reply = match timeout(response_timeout, reader.read(stream)).await {
        Ok(Ok(Some(raw))) => {
            let decoded = SapientMessage::decode(raw.as_slice());
            match decoded {
                Ok(reply) => {
                    reader.recycle(raw);
                    ReplyOutcome::Reply(Box::new(reply))
                }
                Err(err) => ReplyOutcome::UndecodableReply {
                    raw,
                    error: err.to_string(),
                },
            }
        }
        Ok(Ok(None)) => ReplyOutcome::Disconnected,
        Ok(Err(err)) => return Err(err),
        Err(_elapsed) => ReplyOutcome::TimedOut,
    };

    Ok(reply)
}

/// Combines [`validate_and_send`] and [`wait_for_reply`] into one call, for
/// callers that don't need to act between the send completing and the reply
/// arriving -- the testable core this module's own doc comment describes.
/// `send`'s CLI driver uses the two halves directly instead, specifically so
/// it can print progress between them; this wrapper exists for tests and any
/// other caller that just wants the combined outcome.
pub async fn send_message<S>(
    stream: &mut S,
    reader: &mut FrameReader,
    message: &SapientMessage,
    response_timeout: Duration,
    write_timeout: Duration,
) -> io::Result<SendOutcome>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let validation = validate_and_send(stream, message, write_timeout).await?;
    let reply = wait_for_reply(stream, reader, response_timeout).await?;
    Ok(SendOutcome { validation, reply })
}

pub async fn send(args: SendArgs) -> ExitCode {
    let connect_timeout = Duration::from_secs(args.connect_timeout_secs);
    let response_timeout = Duration::from_secs(args.response_timeout_secs);
    let write_timeout = Duration::from_secs(args.write_timeout_secs);
    let message_descriptor = sapient_message_descriptor();

    // Parse every requested file up front, before any connection is
    // attempted: a missing or malformed file -- first or last in the list
    // -- must fail locally and immediately, not after a network wait, and
    // not after earlier files have already been transmitted.
    let mut messages = Vec::with_capacity(args.files.len());
    for path in &args.files {
        match load_message_from_file(path, &message_descriptor) {
            Ok(message) => messages.push((path.as_path(), message)),
            Err(err) => {
                eprintln!("error: {err}");
                return ExitCode::from(2);
            }
        }
    }

    let stream = match args.role {
        Role::Dmm => connect_as_dmm(&args.target, connect_timeout).await,
        Role::Asm => connect_as_asm(&args.target, connect_timeout).await,
    };
    let mut stream = match stream {
        Ok(stream) => stream,
        Err(err) => {
            eprintln!("error: {err}");
            return ExitCode::from(2);
        }
    };

    let mut reader = FrameReader::new(args.max_frame_bytes)
        .with_large_message_warning(crate::cli::warn_large_message);

    for (path, message) in &messages {
        println!("\n=== {} ===", path.display());

        // Validated and sent as one step, so the warning (if any) prints
        // immediately after validation and before the reply wait -- not
        // buffered until the whole round trip, including that wait, is over.
        let validation = match validate_and_send(&mut stream, message, write_timeout).await {
            Ok(validation) => validation,
            Err(err) => {
                eprintln!("error: {err}");
                return ExitCode::from(2);
            }
        };
        print_validation_warning(&validation);
        println!("Sent.");

        let reply = match wait_for_reply(&mut stream, &mut reader, response_timeout).await {
            Ok(reply) => reply,
            Err(err) => {
                eprintln!("error: {err}");
                return ExitCode::from(2);
            }
        };
        print_reply(path, &reply, response_timeout);
        if let ReplyOutcome::UndecodableReply { raw, .. } = reply {
            reader.recycle(raw);
        }
    }

    println!("\nDone.");
    ExitCode::SUCCESS
}

fn load_message_from_file(
    path: &Path,
    message_descriptor: &MessageDescriptor,
) -> io::Result<SapientMessage> {
    let json = fs::read_to_string(path)
        .map_err(|err| io::Error::other(format!("failed to read {}: {err}", path.display())))?;
    decode_sapient_message_json(&json, message_descriptor).map_err(|err| {
        io::Error::other(format!(
            "failed to decode {} as a SapientMessage: {err}",
            path.display()
        ))
    })
}

fn print_validation_warning(validation: &ValidationOutcome) {
    if !validation.passed {
        println!("WARNING: this message does not conform to sapient-conformance-core's own rules:");
        for finding in &validation.findings {
            println!(
                "  [{}] {}: {}",
                finding.rule_id, finding.field_path, finding.message
            );
        }
        println!("Sending it anyway.");
    }
}

fn print_reply(path: &Path, reply: &ReplyOutcome, response_timeout: Duration) {
    match reply {
        ReplyOutcome::Reply(reply) => println!("Reply to {}:\n{reply:#?}", path.display()),
        ReplyOutcome::UndecodableReply { raw, error } => println!(
            "Reply to {} didn't decode as a SapientMessage: {error} (raw {} bytes)",
            path.display(),
            raw.len()
        ),
        ReplyOutcome::Disconnected => {
            println!("Peer disconnected after sending {}.", path.display())
        }
        ReplyOutcome::TimedOut => println!(
            "No reply to {} within {response_timeout:?}.",
            path.display()
        ),
    }
}

async fn connect_as_dmm(target: &str, connect_timeout: Duration) -> io::Result<TcpStream> {
    let target: SocketAddr = target.parse().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "--target {target:?} is not a literal address -- --role dmm listens on one \
                 specific ip:port (e.g. 0.0.0.0:5000), not a hostname"
            ),
        )
    })?;
    let listener = TcpListener::bind(target).await?;
    println!("Listening on {target} for a peer to connect...");
    let (stream, peer_addr) =
        timeout(connect_timeout, listener.accept())
            .await
            .map_err(|_| {
                io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!("no peer connected to {target} within {connect_timeout:?}"),
                )
            })??;
    println!("Peer connected from {peer_addr}.");
    Ok(stream)
}

async fn connect_as_asm(target: &str, connect_timeout: Duration) -> io::Result<TcpStream> {
    println!("Connecting to {target}...");
    let stream = timeout(connect_timeout, TcpStream::connect(target))
        .await
        .map_err(|_| {
            io::Error::new(
                io::ErrorKind::TimedOut,
                format!("could not connect to {target} within {connect_timeout:?}"),
            )
        })??;
    println!("Connected.");
    Ok(stream)
}
