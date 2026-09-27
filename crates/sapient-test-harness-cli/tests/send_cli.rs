//! `send` shares its connection setup with `run` (KI-023: hostnames were
//! rejected outright) but has its own copy of the fix in `connect_as_dmm`/
//! `connect_as_asm`, so it needs its own regression coverage -- a fix to
//! one function's copy wouldn't touch the other.

use std::process::Stdio;
use std::time::Duration;

use tokio::{io::AsyncReadExt, net::TcpListener, process::Command, time::timeout};

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
