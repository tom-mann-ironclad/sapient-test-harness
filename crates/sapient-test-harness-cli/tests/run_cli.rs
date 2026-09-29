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
        assert_eq!(findings.iter().filter(|f| f["rule_id"] == rule).count(), 2);
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
