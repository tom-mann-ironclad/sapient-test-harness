//! Pure, synchronous session state machine for the harness playing the
//! **ASM** role (connecting out to a DMM/middleware implementation under
//! test). Mirrors `state.rs` (the DMM role) structurally -- same
//! `Finding`/rule-ID conventions, same raw-bytes-in/raw-bytes-out shape --
//! but flipped: the harness here is the one *sending* `Registration`,
//! `StatusReport`, `DetectionReport`, `Alert`, and *receiving*
//! `RegistrationAck`, `Task`, `AlertAck`.

use std::collections::HashSet;

use prost::Message;
use sapient_conformance_core::{
    bsi_flex_335_v2_0::{
        Alert, AlertAck, DetectionReport, Error as ErrorMessage, Registration, RegistrationAck,
        SapientMessage, StatusReport, Task, TaskAck,
        registration::{ModeDefinition, ModeType},
        sapient_message::Content,
        task::{Command as TaskCommand, command::Command as TaskCommandKind},
        task_ack::TaskStatus,
    },
    finding::{Finding, Severity},
    validation::{
        alert_ack::validate_alert_ack, registration_ack::validate_registration_ack,
        task::validate_task,
    },
};

/// Where an [`AsmSession`] currently is in the protocol lifecycle.
#[derive(Debug, Clone)]
pub enum AsmSessionState {
    /// Nothing sent yet.
    NotRegistered,
    /// We've sent our `Registration` and are waiting for the DMM's
    /// `RegistrationAck`.
    AwaitingRegistrationAck { registration: Box<Registration> },
    /// The DMM accepted our `Registration`; this is our active contract.
    Registered(Box<RegisteredAsmContract>),
}

/// The contract this session declared about itself, captured from the
/// `Registration` we sent once it's been accepted.
#[derive(Debug, Clone)]
pub struct RegisteredAsmContract {
    pub registration: Registration,
    /// The `ModeDefinition` we're currently operating under. Starts as
    /// our own `MODE_TYPE_DEFAULT` mode and changes when the DMM issues a
    /// `mode_change` `Task`.
    pub active_mode: ModeDefinition,
    /// `alert_id`s we've sent that haven't yet been acknowledged by a
    /// matching `AlertAck` from the DMM.
    pub outstanding_alert_ids: HashSet<String>,
}

/// Session state machine for the harness acting as ASM: it connects out
/// to a DMM/middleware implementation under test, so it *sends*
/// `Registration`/`StatusReport`/`DetectionReport`/`Alert`/`TaskAck`, and
/// *receives* `RegistrationAck`/`Task`/`AlertAck`/`Error`.
pub struct AsmSession {
    /// This harness's own node ID, stamped on every outgoing message.
    harness_node_id: String,
    /// The DMM's node ID, once known (from the first message it sends
    /// us) -- used as `destination_id` on our own outgoing messages.
    peer_node_id: Option<String>,
    state: AsmSessionState,
    findings: Vec<Finding>,
    current_raw: Vec<u8>,
}

impl AsmSession {
    pub fn new(harness_node_id: impl Into<String>) -> Self {
        AsmSession {
            harness_node_id: harness_node_id.into(),
            peer_node_id: None,
            state: AsmSessionState::NotRegistered,
            findings: Vec::new(),
            current_raw: Vec::new(),
        }
    }

    pub fn state(&self) -> &AsmSessionState {
        &self.state
    }

    pub fn findings(&self) -> &[Finding] {
        &self.findings
    }

    /// Drain and return every finding recorded so far.
    pub fn take_findings(&mut self) -> Vec<Finding> {
        std::mem::take(&mut self.findings)
    }

    /// Harness-initiated: send our own `Registration` to the DMM. Must be
    /// called before any other `issue_*` method -- returns the encoded
    /// bytes to send; the caller (driver, or a test) is responsible for
    /// actually writing them to the connection.
    pub fn register(&mut self, registration: Registration) -> Vec<u8> {
        if !matches!(self.state, AsmSessionState::NotRegistered) {
            self.findings.push(Finding {
                rule_id: "session.registration.sent_twice".to_string(),
                field_path: "session".to_string(),
                severity: Severity::Error,
                message: "register() was called again while a Registration was already \
                          outstanding or accepted; this session's own Registration should \
                          only be sent once (re-registration is a legitimate protocol event, \
                          but the harness itself choosing to send a second one mid-test is \
                          almost always a scenario bug, not something to encode here)."
                    .to_string(),
            });
        }

        self.state = AsmSessionState::AwaitingRegistrationAck {
            registration: Box::new(registration.clone()),
        };
        self.encode(self.wrap(Content::Registration(registration)))
    }

    /// Feed raw bytes received from the peer (already de-framed).
    /// Returns the raw bytes of a reply to send back, if the protocol
    /// requires one (`TaskAck`, or `Error` for a post-Registration
    /// decode/validation failure).
    pub fn on_bytes(&mut self, raw: &[u8]) -> Option<Vec<u8>> {
        self.current_raw = raw.to_vec();

        let message = match SapientMessage::decode(raw) {
            Ok(message) => message,
            Err(err) => {
                let registered = matches!(self.state, AsmSessionState::Registered(_));
                self.findings.push(Finding {
                    rule_id: "session.framing.undecodable".to_string(),
                    field_path: "session.framing".to_string(),
                    severity: Severity::Error,
                    message: format!("received bytes that don't decode as a SapientMessage: {err}"),
                });
                return if registered {
                    let reply = self.error_reply(vec![format!(
                        "failed to decode received packet as a SapientMessage: {err}"
                    )]);
                    Some(self.encode(reply))
                } else {
                    None
                };
            }
        };

        self.on_message(message).map(|reply| self.encode(reply))
    }

    fn encode(&self, message: SapientMessage) -> Vec<u8> {
        message.encode_to_vec()
    }

    fn on_message(&mut self, message: SapientMessage) -> Option<SapientMessage> {
        if self.peer_node_id.is_none() {
            self.peer_node_id = message.node_id.clone();
        }
        let content = message.content.clone();

        match content {
            Some(Content::RegistrationAck(ack)) => self.handle_registration_ack(ack),
            Some(Content::Task(task)) => self.handle_task(task),
            Some(Content::AlertAck(alert_ack)) => self.handle_alert_ack(alert_ack),
            Some(Content::Error(error)) => self.handle_incoming_error(error),
            Some(other) => self.handle_wrong_role_message(other),
            None => {
                self.findings.push(Finding {
                    rule_id: "sapient_message.content.missing".to_string(),
                    field_path: "sapient_message.content".to_string(),
                    severity: Severity::Error,
                    message: "Content must be specified in sapient message.".to_string(),
                });
                None
            }
        }
    }

    fn handle_registration_ack(&mut self, ack: RegistrationAck) -> Option<SapientMessage> {
        let pending_registration = match &self.state {
            AsmSessionState::AwaitingRegistrationAck { registration } => {
                registration.as_ref().clone()
            }
            AsmSessionState::NotRegistered => {
                self.findings.push(Finding {
                    rule_id: "session.sequencing.unexpected_registration_ack".to_string(),
                    field_path: "sapient_message.content".to_string(),
                    severity: Severity::Error,
                    message: "Received a RegistrationAck before this session ever sent a \
                              Registration."
                        .to_string(),
                });
                return None;
            }
            AsmSessionState::Registered(_) => {
                self.findings.push(Finding {
                    rule_id: "session.unexpected_message_for_role".to_string(),
                    field_path: "sapient_message.content".to_string(),
                    severity: Severity::Error,
                    message: "Received an unprompted RegistrationAck while already registered; \
                              expected at most one per Registration sent."
                        .to_string(),
                });
                return None;
            }
        };

        let outcome = validate_registration_ack(ack.clone());
        if !outcome.passed {
            self.findings.extend(outcome.findings);
        }

        if ack.acceptance != Some(true) {
            self.findings.push(Finding {
                rule_id: "session.registration.rejected".to_string(),
                field_path: "registration_ack.acceptance".to_string(),
                severity: Severity::Error,
                message: format!(
                    "DMM rejected our Registration: {}",
                    ack.ack_response_reason.join("; ")
                ),
            });
            self.state = AsmSessionState::NotRegistered;
            return None;
        }

        let default_modes: Vec<&ModeDefinition> = pending_registration
            .mode_definition
            .iter()
            .filter(|mode| mode.mode_type == Some(ModeType::Default as i32))
            .collect();

        let active_mode = match default_modes.as_slice() {
            [single] => (*single).clone(),
            _ => {
                // Our own Registration should always declare exactly one
                // MODE_TYPE_DEFAULT mode -- if it doesn't, that's a bug in
                // the harness/test scenario that built it, not a protocol
                // violation by the peer. Record it and stay unregistered
                // rather than silently picking an arbitrary mode.
                self.findings.push(Finding {
                    rule_id: "session.registration.no_default_mode".to_string(),
                    field_path: "registration.mode_definition".to_string(),
                    severity: Severity::Error,
                    message: format!(
                        "Our own Registration declared {} modes with mode_type \
                         MODE_TYPE_DEFAULT; exactly one is required. This is a harness/test \
                         scenario bug (the DMM already accepted it), not something the peer \
                         did wrong.",
                        default_modes.len()
                    ),
                });
                self.state = AsmSessionState::NotRegistered;
                return None;
            }
        };

        self.state = AsmSessionState::Registered(Box::new(RegisteredAsmContract {
            registration: pending_registration,
            active_mode,
            outstanding_alert_ids: HashSet::new(),
        }));

        None
    }

    fn handle_task(&mut self, task: Task) -> Option<SapientMessage> {
        let contract = match &mut self.state {
            AsmSessionState::Registered(contract) => contract,
            _ => return self.sequencing_violation("Task"),
        };

        let outcome = validate_task(task.clone());
        if !outcome.passed {
            self.findings.extend(outcome.findings);
            return Some(self.error_reply(vec!["Task failed validation.".to_string()]));
        }

        let task_id = task.task_id.clone().unwrap_or_default();

        if let Some(TaskCommand {
            command: Some(TaskCommandKind::ModeChange(target_mode_name)),
            ..
        }) = &task.command
        {
            match contract
                .registration
                .mode_definition
                .iter()
                .find(|mode| mode.mode_name.as_deref() == Some(target_mode_name.as_str()))
            {
                Some(mode) => {
                    contract.active_mode = mode.clone();
                    return Some(self.task_ack_reply(task_id, TaskStatus::Accepted, vec![]));
                }
                None => {
                    self.findings.push(Finding {
                        rule_id: "session.task.mode_change_unknown_mode".to_string(),
                        field_path: "task.command.mode_change".to_string(),
                        severity: Severity::Error,
                        message: format!(
                            "DMM issued a mode_change task targeting mode \
                             {target_mode_name:?}, which we never declared in our own \
                             Registration's mode_definition list."
                        ),
                    });
                    return Some(self.task_ack_reply(
                        task_id,
                        TaskStatus::Rejected,
                        vec![format!("Unknown mode: {target_mode_name:?}")],
                    ));
                }
            }
        }

        Some(self.task_ack_reply(task_id, TaskStatus::Accepted, vec![]))
    }

    fn handle_alert_ack(&mut self, alert_ack: AlertAck) -> Option<SapientMessage> {
        let contract = match &mut self.state {
            AsmSessionState::Registered(contract) => contract,
            _ => return self.sequencing_violation("AlertAck"),
        };

        let outcome = validate_alert_ack(alert_ack.clone());
        if !outcome.passed {
            self.findings.extend(outcome.findings);
            return Some(self.error_reply(vec!["AlertAck failed validation.".to_string()]));
        }

        let alert_id = alert_ack.alert_id.clone().unwrap_or_default();
        if !contract.outstanding_alert_ids.remove(&alert_id) {
            self.findings.push(Finding {
                rule_id: "session.alert_ack.correlation_mismatch".to_string(),
                field_path: "alert_ack.alert_id".to_string(),
                severity: Severity::Error,
                message: format!(
                    "AlertAck references alert_id {alert_id:?}, which doesn't match any \
                     Alert this session sent that's still awaiting acknowledgement."
                ),
            });
        }

        None
    }

    fn handle_incoming_error(&mut self, error: ErrorMessage) -> Option<SapientMessage> {
        self.findings.push(Finding {
            rule_id: "session.peer_reported_error".to_string(),
            field_path: "error.error_message".to_string(),
            severity: Severity::Error,
            message: format!(
                "Peer sent an Error message about a packet it received: {}",
                error.error_message.join("; ")
            ),
        });
        None
    }

    fn handle_wrong_role_message(&mut self, _content: Content) -> Option<SapientMessage> {
        self.findings.push(Finding {
            rule_id: "session.unexpected_message_for_role".to_string(),
            field_path: "sapient_message.content".to_string(),
            severity: Severity::Error,
            message: "Received a message type the ASM role never expects as inbound traffic \
                      (e.g. Registration, StatusReport, DetectionReport, Alert, TaskAck are \
                      ASM-to-DMM messages)."
                .to_string(),
        });
        None
    }

    fn sequencing_violation(&mut self, message_type: &str) -> Option<SapientMessage> {
        self.findings.push(Finding {
            rule_id: "session.sequencing.registration_required".to_string(),
            field_path: "sapient_message.content".to_string(),
            severity: Severity::Error,
            message: format!(
                "Received a {message_type} before our Registration was accepted; the DMM \
                 shouldn't send this yet."
            ),
        });
        None
    }

    /// Send a `StatusReport` (harness-initiated). Not validated against
    /// our own declared contract before sending -- a test scenario may
    /// deliberately want to send a report that's inconsistent with the
    /// Registration, to check how the DMM under test reacts to it.
    pub fn issue_status_report(&self, status_report: StatusReport) -> Vec<u8> {
        self.encode(self.wrap(Content::StatusReport(status_report)))
    }

    /// Send a `DetectionReport` (harness-initiated).
    pub fn issue_detection_report(&self, detection_report: DetectionReport) -> Vec<u8> {
        self.encode(self.wrap(Content::DetectionReport(detection_report)))
    }

    /// Send an `Alert` (harness-initiated). Tracks the `alert_id` as
    /// outstanding until a matching `AlertAck` arrives.
    pub fn issue_alert(&mut self, alert: Alert) -> Vec<u8> {
        if let AsmSessionState::Registered(contract) = &mut self.state
            && let Some(alert_id) = &alert.alert_id
        {
            contract.outstanding_alert_ids.insert(alert_id.clone());
        }
        self.encode(self.wrap(Content::Alert(alert)))
    }

    fn task_ack_reply(
        &self,
        task_id: String,
        status: TaskStatus,
        reason: Vec<String>,
    ) -> SapientMessage {
        self.wrap(Content::TaskAck(TaskAck {
            task_id: Some(task_id),
            task_status: Some(status as i32),
            associated_file: None,
            reason,
        }))
    }

    fn error_reply(&self, error_messages: Vec<String>) -> SapientMessage {
        self.wrap(Content::Error(ErrorMessage {
            packet: Some(self.current_raw.clone()),
            error_message: error_messages,
        }))
    }

    fn wrap(&self, content: Content) -> SapientMessage {
        SapientMessage {
            timestamp: Some(now_timestamp()),
            node_id: Some(self.harness_node_id.clone()),
            destination_id: self.peer_node_id.clone(),
            content: Some(content),
            additional_information: None,
        }
    }
}

fn now_timestamp() -> prost_types::Timestamp {
    let duration = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    prost_types::Timestamp {
        seconds: duration.as_secs() as i64,
        nanos: duration.subsec_nanos() as i32,
    }
}
