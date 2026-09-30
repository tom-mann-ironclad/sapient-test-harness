//! `send` shares its connection setup with `run` (KI-023: hostnames were
//! rejected outright) but has its own copy of the fix in `connect_as_dmm`/
//! `connect_as_asm`, so it needs its own regression coverage -- a fix to
//! one function's copy wouldn't touch the other.

use std::process::Stdio;
use std::time::Duration;

use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, BufReader},
    net::TcpListener,
    process::Command,
    time::timeout,
};

/// A minimal, valid canonical-protobuf-JSON `SapientMessage` -- content
/// doesn't matter for these tests, only that it's real enough to send.
fn registration_ack_json(path: &std::path::Path) {
    std::fs::write(
        path,
        r#"{
            "timestamp": "2024-01-01T00:00:00Z",
            "nodeId": "550e8400-e29b-41d4-a716-446655440000",
            "destinationId": "550e8400-e29b-41d4-a716-446655440001",
            "registrationAck": { "acceptance": true }
        }"#,
    )
    .unwrap();
}

/// Decodable but non-conformant: a `Task` missing its mandatory `control`
/// field (`task.control.invalid`), so sending it triggers a real warning.
fn invalid_task_json(path: &std::path::Path) {
    std::fs::write(
        path,
        r#"{
            "timestamp": "2024-01-01T00:00:00Z",
            "nodeId": "550e8400-e29b-41d4-a716-446655440000",
            "destinationId": "550e8400-e29b-41d4-a716-446655440001",
            "task": { "taskId": "01H1VV3VN40RV97CDFSXJB44KA" }
        }"#,
    )
    .unwrap();
}

#[tokio::test]
async fn asm_role_resolves_hostname_targets() {
    let file = std::env::temp_dir().join("send_cli_hostname_test_registration_ack.json");
    registration_ack_json(&file);

    timeout(Duration::from_secs(10), async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let child = Command::new(env!("CARGO_BIN_EXE_sapient-harness"))
            .args([
                "send",
                "--role",
                "asm",
                "--target",
                &format!("localhost:{port}"),
                "--file",
                file.to_str().unwrap(),
            ])
            .stdout(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        // Accepting a real connection proves "localhost" was resolved and
        // connected to, not just accepted as a syntactically valid string.
        let (mut peer, _) = listener.accept().await.unwrap();
        let mut buf = [0u8; 4];
        peer.read_exact(&mut buf).await.unwrap();
        drop(child);
    })
    .await
    .expect("hostname target must resolve and connect within the test deadline");

    let _ = std::fs::remove_file(&file);
}

#[tokio::test]
async fn dmm_role_rejects_hostname_targets_without_attempting_to_bind() {
    let file = std::env::temp_dir().join("send_cli_hostname_test_dmm_unused.json");
    registration_ack_json(&file);

    // No peer/listener is set up: a correct fix must fail before ever
    // touching the network, so this test would hang if it didn't.
    let output = timeout(
        Duration::from_secs(5),
        Command::new(env!("CARGO_BIN_EXE_sapient-harness"))
            .args([
                "send",
                "--role",
                "dmm",
                "--target",
                "localhost:0",
                "--file",
                file.to_str().unwrap(),
            ])
            .kill_on_drop(true)
            .output(),
    )
    .await
    .expect("must reject a hostname target immediately, not hang waiting to bind")
    .unwrap();

    let _ = std::fs::remove_file(&file);

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("not a literal address"), "{stderr}");
}

/// A malformed *second* file must be caught before the first file is ever
/// sent and before any connection is attempted -- not discovered partway
/// through, after earlier files already went out.
#[tokio::test]
async fn a_later_malformed_file_fails_before_any_file_is_sent_or_any_connection_made() {
    let good_file = std::env::temp_dir().join("send_cli_ki031_good.json");
    let bad_file = std::env::temp_dir().join("send_cli_ki031_bad.json");
    registration_ack_json(&good_file);
    std::fs::write(&bad_file, "not valid json").unwrap();

    // A real, bindable target with no peer ever connecting: a correct fix
    // never reaches the connect/bind stage at all, so this returns almost
    // immediately; the old (buggy) order would instead hang here until
    // `--connect-timeout-secs` (30s default), which the outer 5s timeout
    // would catch as a failure.
    let output = timeout(
        Duration::from_secs(5),
        Command::new(env!("CARGO_BIN_EXE_sapient-harness"))
            .args([
                "send",
                "--role",
                "dmm",
                "--target",
                "127.0.0.1:0",
                "--file",
                good_file.to_str().unwrap(),
                "--file",
                bad_file.to_str().unwrap(),
            ])
            .kill_on_drop(true)
            .output(),
    )
    .await
    .expect("a malformed later file must fail fast, not hang waiting to connect")
    .unwrap();

    let _ = std::fs::remove_file(&good_file);
    let _ = std::fs::remove_file(&bad_file);

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("bad") && stderr.contains("failed to decode"),
        "expected an error naming the bad file, got: {stderr}"
    );
    // Nothing from the (never-reached) send loop should have printed.
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(!stdout.contains("Sent."), "got: {stdout}");
}

/// The validation warning and "Sent." must print as soon as the send
/// completes, not buffered until a reply arrives (or the response timeout
/// elapses) -- proved by holding the reply back and reading stdout
/// incrementally, not by inspecting the final combined output.
#[tokio::test]
async fn validation_warning_and_sent_print_before_the_reply_is_received() {
    let file = std::env::temp_dir().join("send_cli_ki031_warning_timing.json");
    invalid_task_json(&file);

    timeout(Duration::from_secs(10), async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_sapient-harness"))
            .args([
                "send",
                "--role",
                "asm",
                "--target",
                &listener.local_addr().unwrap().to_string(),
                "--file",
                file.to_str().unwrap(),
                "--response-timeout-secs",
                "20",
            ])
            .stdout(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let (mut peer, _) = listener.accept().await.unwrap();
        // Deliberately don't reply yet: under the old (buggy) ordering,
        // "Sent." wouldn't print until the reply arrived or the 20s
        // response timeout elapsed, so this next read would block well
        // past the short inner timeout below if the fix regressed.
        let mut stdout = BufReader::new(child.stdout.take().unwrap()).lines();
        let mut saw_warning = false;
        let saw_sent = timeout(Duration::from_secs(2), async {
            loop {
                let line = stdout
                    .next_line()
                    .await
                    .unwrap()
                    .expect("Sent. before the peer ever replies");
                if line.contains("WARNING:") {
                    saw_warning = true;
                }
                if line == "Sent." {
                    break;
                }
            }
        })
        .await;
        assert!(
            saw_sent.is_ok(),
            "\"Sent.\" must print before the reply arrives, not after"
        );
        assert!(saw_warning, "expected a WARNING line before \"Sent.\"");

        // Now let the run finish normally: echo back whatever frame we
        // received as the "reply" (its content doesn't matter here), and
        // confirm the reply line names the file it's a reply to.
        let mut len = [0u8; 4];
        peer.read_exact(&mut len).await.unwrap();
        let mut payload = vec![0u8; u32::from_le_bytes(len) as usize];
        peer.read_exact(&mut payload).await.unwrap();
        use tokio::io::AsyncWriteExt;
        peer.write_all(&len).await.unwrap();
        peer.write_all(&payload).await.unwrap();

        // `child.stdout` was already taken for incremental reading above,
        // so keep draining the same `BufReader` rather than
        // `wait_with_output` (which would find stdout already gone).
        let mut remaining_stdout = String::new();
        while let Some(line) = stdout.next_line().await.unwrap() {
            remaining_stdout.push_str(&line);
            remaining_stdout.push('\n');
        }
        let status = child.wait().await.unwrap();
        assert_eq!(status.code(), Some(0));
        assert!(
            remaining_stdout.contains(&format!("Reply to {}", file.display())),
            "expected the reply to be identified by its source file, got:\n{remaining_stdout}"
        );
    })
    .await
    .unwrap();

    let _ = std::fs::remove_file(&file);
}
