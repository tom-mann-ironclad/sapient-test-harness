//! Verify the shipped command's stdout and process exit code, not just its report builder.
use std::process::Stdio;
use std::time::Duration;

use prost::Message;
use prost_types::Timestamp;
use sapient_conformance_core::bsi_flex_335_v2_0::{
    AlertAck, RegistrationAck, SapientMessage, sapient_message::Content,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    process::Command,
    time::timeout,
};

async fn receive(stream: &mut TcpStream) -> Content {
    let length = stream.read_u32_le().await.unwrap();
    let mut raw = vec![0; length as usize];
    stream.read_exact(&mut raw).await.unwrap();
    SapientMessage::decode(raw.as_slice())
        .unwrap()
        .content
        .unwrap()
}

async fn send(stream: &mut TcpStream, content: Content, invalid_envelope: bool) {
    let mut message = SapientMessage {
        timestamp: Some(Timestamp {
            seconds: 1,
            nanos: 0,
        }),
        node_id: Some("550e8400-e29b-41d4-a716-446655440000".into()),
        content: Some(content),
        ..Default::default()
    };
    if invalid_envelope {
        message.timestamp = None;
        message.node_id = Some("bad-node".into());
        message.destination_id = Some("bad-destination".into());
    }
    let raw = message.encode_to_vec();
    stream.write_u32_le(raw.len() as u32).await.unwrap();
    stream.write_all(&raw).await.unwrap();
}

async fn run_case(complete: bool, format: &str, invalid_envelope: bool) -> std::process::Output {
    timeout(Duration::from_secs(10), async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let child = Command::new(env!("CARGO_BIN_EXE_sapient-harness"))
            .args([
                "run",
                "--role",
                "asm",
                "--target",
                &listener.local_addr().unwrap().to_string(),
                "--max-runtime-secs",
                if complete { "5" } else { "1" },
                "--format",
                format,
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let (mut peer, _) = listener.accept().await.unwrap();
        assert!(matches!(receive(&mut peer).await, Content::Registration(_)));
        if complete {
            send(
                &mut peer,
                Content::RegistrationAck(RegistrationAck {
                    acceptance: Some(true),
                    ack_response_reason: vec![],
                }),
                invalid_envelope,
            )
            .await;
            assert!(matches!(receive(&mut peer).await, Content::StatusReport(_)));
            assert!(matches!(
                receive(&mut peer).await,
                Content::DetectionReport(_)
            ));
            let Content::Alert(alert) = receive(&mut peer).await else {
                panic!("expected alert")
            };
            send(
                &mut peer,
                Content::AlertAck(AlertAck {
                    alert_id: alert.alert_id,
                    alert_ack_status: Some(1),
                    reason: vec![],
                }),
                invalid_envelope,
            )
            .await;
            assert!(matches!(receive(&mut peer).await, Content::StatusReport(_)));
        }
        // Keep the connection open: the incomplete case must terminate on its deadline.
        child.wait_with_output().await.unwrap()
    })
    .await
    .expect("CLI must terminate within the test deadline")
}

#[tokio::test]
async fn silent_peer_emits_incomplete_json_and_exits_one() {
    let output = run_case(false, "json", false).await;
    assert_eq!(output.status.code(), Some(1));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["passed"], false);
    assert_eq!(json["outcome"], "incomplete");
    assert_eq!(json["checks"][0]["check"], "registration");
    assert_eq!(json["checks"][0]["status"], "incomplete");
    assert_eq!(json["findings"], serde_json::json!([]));
}

#[tokio::test]
async fn silent_peer_text_reports_incomplete_not_pass_or_zero_findings_failure() {
    let output = run_case(false, "text", false).await;
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("INCOMPLETE --"));
    assert!(!stdout.contains("PASS --"));
    assert!(!stdout.contains("FAIL -- 0"));
}

#[tokio::test]
async fn complete_exchange_emits_passed_json_and_exits_zero() {
    let output = run_case(true, "json", false).await;
    assert_eq!(output.status.code(), Some(0));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["passed"], true);
    assert_eq!(json["outcome"], "passed");
    assert!(
        json["checks"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["status"] == "completed")
    );

    // KI-025: report-level metadata for matching a report back to the
    // exact harness build and run that produced it.
    assert_eq!(json["report_schema_version"], 1);
    assert_eq!(json["harness_version"], env!("CARGO_PKG_VERSION"));
    let node_id = json["harness_node_id"].as_str().unwrap();
    assert!(
        uuid::Uuid::parse_str(node_id).is_ok(),
        "expected a real UUID (the freshly generated default), got {node_id:?}"
    );
    let started = json["started_at_unix_millis"].as_u64().unwrap();
    let ended = json["ended_at_unix_millis"].as_u64().unwrap();
    let duration = json["duration_millis"].as_u64().unwrap();
    assert!(started > 0 && ended >= started);
    assert_eq!(duration, ended - started);
}

#[tokio::test]
async fn invalid_envelopes_are_reported_while_the_whole_exchange_continues() {
    let output = run_case(true, "json", true).await;
    assert_eq!(output.status.code(), Some(1));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["outcome"], "failed");
    assert_eq!(json["passed"], false);
    assert!(
        json["checks"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["status"] == "completed")
    );
    let findings = json["findings"].as_array().unwrap();
    // All three defects in both RegistrationAck and the later AlertAck survive.
    assert_eq!(findings.len(), 6);
    for rule in [
        "sapient_message.timestamp.missing",
        "sapient_message.node_id.invalid",
        "sapient_message.destination_id.invalid",
    ] {
        let matching: Vec<&serde_json::Value> =
            findings.iter().filter(|f| f["rule_id"] == rule).collect();
        assert_eq!(matching.len(), 2);
        // KI-025: the same rule_id fires twice (once from the RegistrationAck,
        // once from the later AlertAck) -- each occurrence must carry its own
        // message context so the two are individually identifiable, not
        // indistinguishable duplicates.
        let sequences: Vec<_> = matching
            .iter()
            .map(|f| f["context"]["sequence"].as_u64().unwrap())
            .collect();
        assert_ne!(
            sequences[0], sequences[1],
            "two distinct messages' findings must not share a sequence number, got {matching:?}"
        );
        let message_types: Vec<_> = matching
            .iter()
            .map(|f| f["context"]["message_type"].as_str().unwrap())
            .collect();
        assert!(
            message_types.contains(&"RegistrationAck") && message_types.contains(&"AlertAck"),
            "expected one finding attributed to each message, got {matching:?}"
        );
        for f in &matching {
            assert_eq!(f["context"]["direction"], "inbound");
            assert!(f["context"]["occurred_at_unix_millis"].as_u64().unwrap() > 0);
        }
    }
}

#[tokio::test]
async fn large_frame_warning_is_immediate_and_kept_out_of_json() {
    use tokio::io::{AsyncBufReadExt, BufReader};
    timeout(Duration::from_secs(10), async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_sapient-harness"))
            .args([
                "run",
                "--role",
                "asm",
                "--target",
                &listener.local_addr().unwrap().to_string(),
                "--max-runtime-secs",
                "2",
                "--max-frame-bytes",
                "1048576",
                "--format",
                "json",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let (mut peer, _) = listener.accept().await.unwrap();
        receive(&mut peer).await;
        peer.write_u32_le(1048576).await.unwrap();
        // No payload arrives: the warning must be visible while still receiving.
        let mut stderr = BufReader::new(child.stderr.take().unwrap()).lines();
        let warning = timeout(Duration::from_secs(1), async {
            loop {
                let line = stderr
                    .next_line()
                    .await
                    .unwrap()
                    .expect("warning before exit");
                if line.contains("WARNING:") {
                    break line;
                }
            }
        })
        .await
        .unwrap();
        assert!(warning.contains("1048576 payload bytes"));
        let output = child.wait_with_output().await.unwrap();
        assert_eq!(output.status.code(), Some(1));
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["outcome"], "incomplete");
        assert_eq!(report["findings"].as_array().unwrap().len(), 0);
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn configured_receive_limit_stops_at_the_header_with_a_resource_diagnostic() {
    timeout(Duration::from_secs(10), async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let child = Command::new(env!("CARGO_BIN_EXE_sapient-harness"))
            .args([
                "run",
                "--role",
                "asm",
                "--target",
                &listener.local_addr().unwrap().to_string(),
                "--max-frame-bytes",
                "32",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let (mut peer, _) = listener.accept().await.unwrap();
        receive(&mut peer).await;
        peer.write_u32_le(33).await.unwrap();
        let output = child.wait_with_output().await.unwrap();
        assert_eq!(output.status.code(), Some(2));
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.contains("configured receive limit of 32 bytes"));
        assert!(text.contains("not a conformance finding"));
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn truncated_payload_retains_findings_progress_and_json() {
    timeout(Duration::from_secs(10), async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let child = Command::new(env!("CARGO_BIN_EXE_sapient-harness"))
            .args([
                "run",
                "--role",
                "asm",
                "--target",
                &listener.local_addr().unwrap().to_string(),
                "--format",
                "json",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let (mut peer, _) = listener.accept().await.unwrap();
        receive(&mut peer).await;
        send(
            &mut peer,
            Content::RegistrationAck(RegistrationAck {
                acceptance: Some(true),
                ack_response_reason: vec![],
            }),
            true,
        )
        .await;
        receive(&mut peer).await;
        receive(&mut peer).await;
        peer.write_u32_le(5).await.unwrap();
        peer.write_all(&[1, 2]).await.unwrap();
        peer.shutdown().await.unwrap();
        let output = child.wait_with_output().await.unwrap();
        assert_eq!(output.status.code(), Some(2));
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["outcome"], "failed");
        assert_eq!(report["findings"].as_array().unwrap().len(), 3);
        assert_eq!(report["operational_error"]["kind"], "UnexpectedEof");
        assert_eq!(report["operational_error"]["stage"], "receive_or_reply");
        assert_eq!(report["checks"][0]["status"], "completed");
        assert_eq!(report["checks"][2]["status"], "completed");
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn connection_failures_and_accept_timeouts_produce_json() {
    for role in ["asm", "dmm"] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap().to_string();
        drop(listener);
        let output = timeout(
            Duration::from_secs(5),
            Command::new(env!("CARGO_BIN_EXE_sapient-harness"))
                .args([
                    "run",
                    "--role",
                    role,
                    "--target",
                    &address,
                    "--connect-timeout-secs",
                    "0",
                    "--format",
                    "json",
                ])
                .kill_on_drop(true)
                .output(),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(output.status.code(), Some(2));
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["outcome"], "incomplete");
        assert_eq!(report["passed"], false);
        assert!(report["operational_error"].is_object());
        assert!(
            report["checks"]
                .as_array()
                .unwrap()
                .iter()
                .all(|check| check["status"] == "incomplete")
        );
    }
}

#[tokio::test]
async fn asm_role_resolves_hostname_targets() {
    timeout(Duration::from_secs(10), async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let child = Command::new(env!("CARGO_BIN_EXE_sapient-harness"))
            .args([
                "run",
                "--role",
                "asm",
                "--target",
                &format!("localhost:{port}"),
                "--max-runtime-secs",
                "5",
            ])
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        // Reaching a real Registration proves "localhost" was resolved and
        // connected to, not just accepted as a syntactically valid string.
        let (mut peer, _) = listener.accept().await.unwrap();
        assert!(matches!(receive(&mut peer).await, Content::Registration(_)));
        drop(child);
    })
    .await
    .expect("hostname target must resolve and connect within the test deadline");
}

#[tokio::test]
async fn dmm_role_rejects_hostname_targets_without_attempting_to_bind() {
    // No peer/listener is set up: a correct fix must fail before ever
    // touching the network, so this test would hang (not just fail) if the
    // rejection didn't happen up front.
    let output = timeout(
        Duration::from_secs(5),
        Command::new(env!("CARGO_BIN_EXE_sapient-harness"))
            .args([
                "run",
                "--role",
                "dmm",
                "--target",
                "localhost:0",
                "--format",
                "json",
            ])
            .kill_on_drop(true)
            .output(),
    )
    .await
    .expect("must reject a hostname target immediately, not hang waiting to bind")
    .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["outcome"], "incomplete");
    let error = report["operational_error"]["message"].as_str().unwrap();
    assert!(error.contains("not a literal address"), "{error}");
}

/// KI-024: a malformed `--node-id` must be rejected before the DMM role ever
/// binds/listens, not stamped onto outgoing messages and left for a strict
/// target to reject. A real, bindable target with nothing ever connecting
/// proves this -- the old (buggy) behavior would instead hang until
/// `--connect-timeout-secs` (default 30s) waiting for a peer, which the
/// outer 5s timeout would catch as a failure.
#[tokio::test]
async fn dmm_role_rejects_an_invalid_node_id_without_attempting_to_bind() {
    let output = timeout(
        Duration::from_secs(5),
        Command::new(env!("CARGO_BIN_EXE_sapient-harness"))
            .args([
                "run",
                "--role",
                "dmm",
                "--target",
                "127.0.0.1:0",
                "--node-id",
                "not-a-uuid",
                "--format",
                "json",
            ])
            .kill_on_drop(true)
            .output(),
    )
    .await
    .expect("must reject an invalid --node-id immediately, not hang waiting for a peer")
    .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["outcome"], "incomplete");
    assert_eq!(report["operational_error"]["stage"], "configure_node_id");
    let error = report["operational_error"]["message"].as_str().unwrap();
    assert!(
        error.contains("not-a-uuid") && error.contains("UUID v4"),
        "{error}"
    );
}

/// Same rejection, ASM role: a real but unreachable target proves the
/// connection attempt is never even made (the old behavior would instead
/// try to connect and report a `connect`-stage error, or succeed and stamp
/// the bad node ID onto a real Registration).
#[tokio::test]
async fn asm_role_rejects_an_invalid_node_id_without_attempting_to_connect() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap().to_string();
    drop(listener);
    let output = timeout(
        Duration::from_secs(5),
        Command::new(env!("CARGO_BIN_EXE_sapient-harness"))
            .args([
                "run",
                "--role",
                "asm",
                "--target",
                &address,
                "--node-id",
                "not-a-uuid",
                "--format",
                "json",
            ])
            .kill_on_drop(true)
            .output(),
    )
    .await
    .expect("must reject an invalid --node-id immediately")
    .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["outcome"], "incomplete");
    assert_eq!(report["operational_error"]["stage"], "configure_node_id");
}

/// A `--node-id` that already is a valid UUID v4 must still reach the
/// network stage -- the validation added for KI-024 must not reject good
/// input along with bad.
#[tokio::test]
async fn a_valid_node_id_still_reaches_the_network() {
    timeout(Duration::from_secs(10), async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let child = Command::new(env!("CARGO_BIN_EXE_sapient-harness"))
            .args([
                "run",
                "--role",
                "asm",
                "--target",
                &format!("127.0.0.1:{port}"),
                "--node-id",
                "550e8400-e29b-41d4-a716-446655440000",
                "--max-runtime-secs",
                "5",
            ])
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let (mut peer, _) = listener.accept().await.unwrap();
        assert!(matches!(receive(&mut peer).await, Content::Registration(_)));
        drop(child);
    })
    .await
    .expect("a valid node ID must not block the harness from connecting");
}

#[tokio::test]
async fn peer_error_text_cannot_inject_terminal_control_sequences() {
    use sapient_conformance_core::bsi_flex_335_v2_0::Error as ErrorMessage;

    let output = timeout(Duration::from_secs(10), async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let child = Command::new(env!("CARGO_BIN_EXE_sapient-harness"))
            .args([
                "run",
                "--role",
                "asm",
                "--target",
                &listener.local_addr().unwrap().to_string(),
                "--max-runtime-secs",
                "1",
                "--format",
                "text",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let (mut peer, _) = listener.accept().await.unwrap();
        assert!(matches!(receive(&mut peer).await, Content::Registration(_)));
        send(
            &mut peer,
            Content::RegistrationAck(RegistrationAck {
                acceptance: Some(true),
                ack_response_reason: vec![],
            }),
            false,
        )
        .await;
        // ESC[2J is an ANSI "clear screen" sequence; the embedded newline
        // plus fake report line attempts to forge a line that looks like
        // this run's own verdict output.
        send(
            &mut peer,
            Content::Error(ErrorMessage {
                packet: None,
                error_message: vec![
                    "malicious\x1b[2Jpayload\nPASS -- forged by the peer, not the harness."
                        .to_string(),
                ],
            }),
            false,
        )
        .await;
        // Keep the connection open until the deadline; we only need the
        // text report the harness already has buffered by then.
        child.wait_with_output().await.unwrap()
    })
    .await
    .expect("CLI must terminate within the test deadline");

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("session.peer_reported_error"),
        "expected the peer Error to be recorded as a finding, got:\n{stdout}"
    );
    assert!(
        !stdout.contains('\x1b'),
        "a raw ESC byte must not reach the terminal, got:\n{stdout}"
    );
    assert!(
        !stdout.contains("payload\nPASS --"),
        "the embedded newline must not let peer text forge a report line, got:\n{stdout}"
    );
    // The content survives in an escaped, visible form -- not silently dropped.
    assert!(stdout.contains("\\u{1b}"), "got:\n{stdout}");
    assert!(stdout.contains("payload\\nPASS"), "got:\n{stdout}");
}

/// KI-026: a conformance finding must reach stderr the moment it's observed,
/// not only in the final report -- proved by reading stderr incrementally
/// and seeing the finding line before the run has any reason to end
/// (registration is accepted, so nothing stops the exchange).
#[tokio::test]
async fn findings_are_streamed_to_stderr_as_they_are_observed() {
    use tokio::io::{AsyncBufReadExt, BufReader};
    timeout(Duration::from_secs(10), async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_sapient-harness"))
            .args([
                "run",
                "--role",
                "asm",
                "--target",
                &listener.local_addr().unwrap().to_string(),
                "--max-runtime-secs",
                "5",
                "--format",
                "json",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let (mut peer, _) = listener.accept().await.unwrap();
        assert!(matches!(receive(&mut peer).await, Content::Registration(_)));
        // Accepted, but with a deliberately invalid envelope: the exchange
        // continues (this doesn't end the run), but a real finding now
        // exists that the old (buggy) behavior would only show at the end.
        send(
            &mut peer,
            Content::RegistrationAck(RegistrationAck {
                acceptance: Some(true),
                ack_response_reason: vec![],
            }),
            true,
        )
        .await;

        let mut stderr = BufReader::new(child.stderr.take().unwrap()).lines();
        let seen = timeout(Duration::from_secs(3), async {
            loop {
                let line = stderr
                    .next_line()
                    .await
                    .unwrap()
                    .expect("a finding line before the process has any reason to exit");
                if line.contains("sapient_message.node_id.invalid") {
                    break line;
                }
            }
        })
        .await;
        assert!(
            seen.is_ok(),
            "expected the finding on stderr well before the run ends"
        );
        drop(child);
    })
    .await
    .expect("CLI must stream the finding within the test deadline");
}

/// KI-026: Ctrl-C (SIGINT) must not silently kill the run. Partial progress
/// and any finding already observed before the interrupt must still be
/// reported, and the process must not exit 0 (a cancelled run is never a
/// pass) or 2 (this isn't an operational/harness failure).
#[tokio::test]
async fn ctrl_c_finalizes_a_report_with_partial_progress_and_findings_retained() {
    timeout(Duration::from_secs(15), async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let child = Command::new(env!("CARGO_BIN_EXE_sapient-harness"))
            .args([
                "run",
                "--role",
                "asm",
                "--target",
                &listener.local_addr().unwrap().to_string(),
                "--max-runtime-secs",
                "30",
                "--format",
                "json",
            ])
            .stdout(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let (mut peer, _) = listener.accept().await.unwrap();
        assert!(matches!(receive(&mut peer).await, Content::Registration(_)));
        // Invalid envelope: a real finding exists before the interrupt,
        // proving it survives -- not just that *some* report gets printed.
        send(
            &mut peer,
            Content::RegistrationAck(RegistrationAck {
                acceptance: Some(true),
                ack_response_reason: vec![],
            }),
            true,
        )
        .await;
        assert!(matches!(receive(&mut peer).await, Content::StatusReport(_)));

        let pid = child.id().expect("child must still be running");
        let status = Command::new("kill")
            .args(["-INT", &pid.to_string()])
            .status()
            .await
            .unwrap();
        assert!(status.success(), "failed to send SIGINT to the harness");

        let output = child.wait_with_output().await.unwrap();
        assert_eq!(
            output.status.code(),
            Some(1),
            "an interrupted run is neither a pass (0) nor an operational failure (2)"
        );
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["passed"], false);
        assert_ne!(report["outcome"], "passed");
        assert!(
            report["checks"]
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c["status"] == "completed"),
            "partial progress (at least Registration) must be retained, got: {report}"
        );
        assert!(
            !report["findings"].as_array().unwrap().is_empty(),
            "the finding already observed before the interrupt must not be lost, got: {report}"
        );
        assert!(
            report["notes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|n| n.as_str().unwrap().contains("Interrupted")),
            "notes should record that the run was interrupted, got: {report}"
        );
    })
    .await
    .expect("an interrupted run must still finalize and exit within the test deadline");
}

/// `--role asm` started before its target is listening: the first attempt
/// is refused, and a retry must connect once the target comes up, rather
/// than the run failing on the first refusal.
#[tokio::test]
async fn asm_role_retries_until_the_target_starts_listening() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);

    timeout(Duration::from_secs(15), async {
        let child = Command::new(env!("CARGO_BIN_EXE_sapient-harness"))
            .args([
                "run",
                "--role",
                "asm",
                "--target",
                &address.to_string(),
                "--connect-timeout-secs",
                "10",
                "--max-runtime-secs",
                "5",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        // Long enough for at least one refused attempt before listening.
        tokio::time::sleep(Duration::from_millis(1500)).await;
        let listener = TcpListener::bind(address).await.unwrap();
        let (mut peer, _) = listener.accept().await.unwrap();
        assert!(matches!(receive(&mut peer).await, Content::Registration(_)));
        drop(child);
    })
    .await
    .expect("the harness must connect once the target starts listening");
}

/// A target that never listens: retries stop at `--connect-timeout-secs`,
/// reporting a `connect`-stage error that keeps the refusal's kind.
#[tokio::test]
async fn asm_role_gives_up_retrying_at_the_connect_timeout() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap().to_string();
    drop(listener);

    let started = std::time::Instant::now();
    let output = timeout(
        Duration::from_secs(10),
        Command::new(env!("CARGO_BIN_EXE_sapient-harness"))
            .args([
                "run",
                "--role",
                "asm",
                "--target",
                &address,
                "--connect-timeout-secs",
                "2",
                "--format",
                "json",
            ])
            .kill_on_drop(true)
            .output(),
    )
    .await
    .expect("retries must stop at the connect timeout")
    .unwrap();
    assert!(
        started.elapsed() >= Duration::from_secs(2),
        "gave up before the connect timeout: {:?}",
        started.elapsed()
    );
    assert_eq!(output.status.code(), Some(2));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["outcome"], "incomplete");
    assert_eq!(report["operational_error"]["stage"], "connect");
    assert_eq!(report["operational_error"]["kind"], "ConnectionRefused");
    let message = report["operational_error"]["message"].as_str().unwrap();
    assert!(message.contains("attempt(s)"), "{message}");
}
