//! Drive the shipped `session` command through its stdin, against a raw peer.
use std::process::Stdio;
use std::time::Duration;

use prost::Message;
use prost_types::Timestamp;
use sapient_conformance_core::bsi_flex_335_v2_0::{
    RegistrationAck, SapientMessage, sapient_message::Content, status_report::System,
    task::command::Command as TaskCommandKind,
};
use sapient_session::fixtures::{ALTERNATE_MODE, valid_registration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    process::{Child, ChildStdin, Command},
    time::timeout,
};

const PEER_NODE_ID: &str = "550e8400-e29b-41d4-a716-446655440000";

async fn receive(stream: &mut TcpStream) -> Content {
    let length = stream.read_u32_le().await.unwrap();
    let mut raw = vec![0; length as usize];
    stream.read_exact(&mut raw).await.unwrap();
    SapientMessage::decode(raw.as_slice())
        .unwrap()
        .content
        .unwrap()
}

async fn send(stream: &mut TcpStream, content: Content) {
    let message = SapientMessage {
        timestamp: Some(Timestamp {
            seconds: 1,
            nanos: 0,
        }),
        node_id: Some(PEER_NODE_ID.into()),
        content: Some(content),
        ..Default::default()
    };
    let raw = message.encode_to_vec();
    stream.write_u32_le(raw.len() as u32).await.unwrap();
    stream.write_all(&raw).await.unwrap();
}

fn spawn_session(args: &[&str]) -> (Child, ChildStdin) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_sapient-harness"))
        .arg("session")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let stdin = child.stdin.take().unwrap();
    (child, stdin)
}

async fn type_command(stdin: &mut ChildStdin, command: &str) {
    stdin
        .write_all(format!("{command}\n").as_bytes())
        .await
        .unwrap();
    stdin.flush().await.unwrap();
}

/// ASM role: registers, sends a StatusReport as soon as it's accepted, sends
/// a DetectionReport on demand, and `quit` ends with a GoodBye and exit 0.
#[tokio::test]
async fn asm_session_registers_reports_and_injects_on_command() {
    let output = timeout(Duration::from_secs(15), async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (child, mut stdin) = spawn_session(&[
            "--role",
            "asm",
            "--target",
            &listener.local_addr().unwrap().to_string(),
        ]);
        let (mut peer, _) = listener.accept().await.unwrap();
        assert!(matches!(receive(&mut peer).await, Content::Registration(_)));
        send(
            &mut peer,
            Content::RegistrationAck(RegistrationAck {
                acceptance: Some(true),
                ack_response_reason: vec![],
            }),
        )
        .await;
        assert!(matches!(receive(&mut peer).await, Content::StatusReport(_)));

        type_command(&mut stdin, "detection").await;
        assert!(matches!(
            receive(&mut peer).await,
            Content::DetectionReport(_)
        ));

        type_command(&mut stdin, "quit").await;
        match receive(&mut peer).await {
            Content::StatusReport(report) => {
                assert_eq!(report.system, Some(System::Goodbye as i32));
            }
            other => panic!("expected a GoodBye StatusReport, got {other:?}"),
        }
        child.wait_with_output().await.unwrap()
    })
    .await
    .expect("the ASM session should finish on `quit`");

    assert_eq!(output.status.code(), Some(0));
    let log = String::from_utf8(output.stdout).unwrap();
    assert!(log.contains("← RegistrationAck"), "{log}");
    assert!(log.contains("→ DetectionReport"), "{log}");
    assert!(log.contains("→ StatusReport (GoodBye)"), "{log}");
}

/// DMM role: accepts a registration with an automatic RegistrationAck, and
/// `task` sends a mode_change to the registration's other declared mode.
#[tokio::test]
async fn dmm_session_acknowledges_registration_and_sends_tasks_on_command() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);

    let output = timeout(Duration::from_secs(15), async {
        let (child, mut stdin) =
            spawn_session(&["--role", "dmm", "--target", &address.to_string()]);
        let mut peer = loop {
            match TcpStream::connect(address).await {
                Ok(stream) => break stream,
                Err(_) => tokio::time::sleep(Duration::from_millis(50)).await,
            }
        };
        send(&mut peer, Content::Registration(valid_registration())).await;
        match receive(&mut peer).await {
            Content::RegistrationAck(ack) => assert_eq!(ack.acceptance, Some(true)),
            other => panic!("expected a RegistrationAck, got {other:?}"),
        }

        type_command(&mut stdin, "task").await;
        match receive(&mut peer).await {
            Content::Task(task) => assert_eq!(
                task.command.and_then(|command| command.command),
                Some(TaskCommandKind::ModeChange(ALTERNATE_MODE.into()))
            ),
            other => panic!("expected a Task, got {other:?}"),
        }

        type_command(&mut stdin, "quit").await;
        child.wait_with_output().await.unwrap()
    })
    .await
    .expect("the DMM session should finish on `quit`");

    assert_eq!(output.status.code(), Some(0));
    let log = String::from_utf8(output.stdout).unwrap();
    assert!(
        log.contains("← Registration #1  → RegistrationAck (automatic)"),
        "{log}"
    );
    assert!(
        log.contains("→ Task (mode_change to \"Alternate\")"),
        "{log}"
    );
}

/// Options that can't apply are rejected before any connection is made.
#[tokio::test]
async fn detection_interval_is_rejected_for_the_dmm_role() {
    let output = timeout(
        Duration::from_secs(5),
        Command::new(env!("CARGO_BIN_EXE_sapient-harness"))
            .args([
                "session",
                "--role",
                "dmm",
                "--target",
                "127.0.0.1:0",
                "--detection-interval-secs",
                "5",
            ])
            .kill_on_drop(true)
            .output(),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(output.status.code(), Some(2));
}
