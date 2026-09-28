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
        registration::ModeDefinition,
        sapient_message::Content,
        task::{Command as TaskCommand, Control, command::Command as TaskCommandKind},
        task_ack::TaskStatus,
    },
    finding::{Finding, Severity},
    validation::{
        alert_ack::validate_alert_ack, registration_ack::validate_registration_ack,
        sapient_message::validate_envelope, task::validate_task,
    },
};

use crate::active_mode::{ActiveModeError, ActiveModeSource, resolve_active_mode};

/// Progress from the most recently processed inbound message, consumed with
/// [`AsmSession::take_event`]. Invalid or uncorrelated AlertAck payloads do not
/// emit progress. Envelope diagnostics are recorded separately and do not suppress
/// payload processing or progress events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AsmEvent {
    /// The peer accepted registration and the local contract was established.
    RegistrationAccepted,
    /// The peer explicitly rejected the pending registration.
    RegistrationRejected,
    /// An invalid acknowledgement or unusable local contract prevented registration.
    /// Details remain available in the session's findings.
    RegistrationFailed,
    /// A valid acknowledgement matched an outstanding alert. This records receipt,
    /// not a claim that the acknowledgement's status was Accepted.
    AlertAcknowledged {
        /// Correlation ID of the outstanding alert acknowledged by the peer.
        alert_id: String,
    },
}

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
    /// The mode we were in immediately before the current `active_mode`,
    /// if a mode-change has happened since Registration -- a single-level
    /// undo slot, not a full history. Set whenever a mode-change task takes
    /// effect, and consumed (reverted into, then cleared) by a `CONTROL_STOP`
    /// naming the currently active mode.
    pub previous_mode: Option<ModeDefinition>,
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
    /// Single-message progress slot, reset before decoding each inbound frame.
    /// Findings have separate retention; this slot is not an event queue.
    event: Option<AsmEvent>,
}

impl AsmSession {
    pub fn new(harness_node_id: impl Into<String>) -> Self {
        AsmSession {
            harness_node_id: harness_node_id.into(),
            peer_node_id: None,
            state: AsmSessionState::NotRegistered,
            findings: Vec::new(),
            current_raw: Vec::new(),
            event: None,
        }
    }

    /// Consume progress from the most recent `on_bytes` call. An event is returned
    /// at most once, and each new inbound frame replaces any unconsumed event.
    pub fn take_event(&mut self) -> Option<AsmEvent> {
        self.event.take()
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
        self.event = None;
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
        // Diagnostic by default: keep processing decoded content so this run can
        // expose payload and sequencing issues too. Envelope findings affect the
        // final verdict, not the existing reply/state-transition policy.
        self.findings.extend(validate_envelope(&message).findings);
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
            self.event = Some(if ack.acceptance == Some(false) {
                AsmEvent::RegistrationRejected
            } else {
                AsmEvent::RegistrationFailed
            });
            self.state = AsmSessionState::NotRegistered;
            return None;
        }

        // Our own Registration should always resolve to an active mode --
        // if it doesn't, that's a bug in the harness/test scenario that
        // built it, not a protocol violation by the peer (the DMM already
        // accepted it). A resolution via the MODE_TYPE_PERMANENT fallback
        // is accepted (with a warning), not treated as that kind of bug --
        // see `active_mode` module docs for why.
        let active_mode = match resolve_active_mode(&pending_registration.mode_definition) {
            Ok((mode, ActiveModeSource::Explicit)) => mode,
            Ok((mode, ActiveModeSource::PermanentNamedDefault)) => {
                self.findings.push(Finding {
                    rule_id: "session.registration.default_mode_via_permanent_name".to_string(),
                    field_path: "registration.mode_definition".to_string(),
                    severity: Severity::Warning,
                    message: format!(
                        "Our own Registration declares no mode with mode_type MODE_TYPE_DEFAULT; \
                         using the MODE_TYPE_PERMANENT mode named {:?} as the initial active \
                         mode, matching the legacy DMM convention MODE_TYPE_DEFAULT was \
                         introduced to replace.",
                        mode.mode_name
                    ),
                });
                mode
            }
            Ok((mode, ActiveModeSource::FirstPermanentMode)) => {
                self.findings.push(Finding {
                    rule_id: "session.registration.default_mode_via_first_permanent".to_string(),
                    field_path: "registration.mode_definition".to_string(),
                    severity: Severity::Warning,
                    message: format!(
                        "Our own Registration declares no mode with mode_type MODE_TYPE_DEFAULT \
                         and no MODE_TYPE_PERMANENT mode named \"default\"; falling back to the \
                         first declared MODE_TYPE_PERMANENT mode ({:?}) as the initial active \
                         mode.",
                        mode.mode_name
                    ),
                });
                mode
            }
            Err(ActiveModeError::NoCandidate) => {
                self.findings.push(Finding {
                    rule_id: "session.registration.no_default_mode".to_string(),
                    field_path: "registration.mode_definition".to_string(),
                    severity: Severity::Error,
                    message: "Our own Registration declared no mode with mode_type \
                              MODE_TYPE_DEFAULT and no mode with mode_type MODE_TYPE_PERMANENT \
                              to fall back to. This is a harness/test scenario bug (the DMM \
                              already accepted it), not something the peer did wrong."
                        .to_string(),
                });
                self.event = Some(AsmEvent::RegistrationFailed);
                self.state = AsmSessionState::NotRegistered;
                return None;
            }
            Err(ActiveModeError::MultipleDefaultModes(count)) => {
                self.findings.push(Finding {
                    rule_id: "session.registration.multiple_default_modes".to_string(),
                    field_path: "registration.mode_definition".to_string(),
                    severity: Severity::Error,
                    message: format!(
                        "Our own Registration declared {count} modes with mode_type \
                         MODE_TYPE_DEFAULT; exactly one is required. This is a harness/test \
                         scenario bug (the DMM already accepted it), not something the peer \
                         did wrong."
                    ),
                });
                self.event = Some(AsmEvent::RegistrationFailed);
                self.state = AsmSessionState::NotRegistered;
                return None;
            }
        };

        self.state = AsmSessionState::Registered(Box::new(RegisteredAsmContract {
            registration: pending_registration,
            active_mode,
            previous_mode: None,
            outstanding_alert_ids: HashSet::new(),
        }));
        self.event = Some(AsmEvent::RegistrationAccepted);

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
        // Already validated by `validate_task`, which only accepts Start/Stop/Pause.
        let control = Control::try_from(task.control.unwrap_or_default())
            .expect("validated Task has a defined control value");

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
                    let already_active = contract.active_mode.mode_name.as_deref()
                        == Some(target_mode_name.as_str());
                    // CONTROL_STOP "stop[s] the task, remove[s] the definition
                    // and revert[s] to the previous task" -- it does not mean
                    // "start the named mode". Naming a mode that isn't
                    // already active doesn't correspond to any task this
                    // simulated ASM could be stopping (single-level history
                    // only, see `previous_mode`), so that's rejected. Naming
                    // the mode we're already in ends that task for real: it
                    // reverts to whatever was active immediately before,
                    // consuming that single-level undo slot -- if there's
                    // nothing recorded there (no mode-change has happened
                    // since Registration), there's genuinely no previous
                    // task to revert to, so that's rejected too.
                    // CONTROL_PAUSE has the same "revert to previous" schema
                    // wording as Stop, but is deliberately left with the
                    // existing (Start-like) behavior for now -- its lifecycle
                    // semantics are still open, see KI-033 in known_issues.md.
                    if control == Control::Stop {
                        if already_active {
                            return match contract.previous_mode.take() {
                                Some(previous) => {
                                    contract.active_mode = previous;
                                    Some(self.task_ack_reply(task_id, TaskStatus::Accepted, vec![]))
                                }
                                None => Some(self.task_ack_reply(
                                    task_id,
                                    TaskStatus::Rejected,
                                    vec![format!(
                                        "Stop requested for the active mode \
                                         {target_mode_name:?}, but no mode-change has \
                                         happened since Registration; there is no previous \
                                         task to revert to."
                                    )],
                                )),
                            };
                        }
                        let current_mode_name = contract.active_mode.mode_name.clone();
                        return Some(self.task_ack_reply(
                            task_id,
                            TaskStatus::Rejected,
                            vec![format!(
                                "Stop requested for mode {target_mode_name:?}, which is not \
                                 the currently active mode ({current_mode_name:?}); there is \
                                 no such task to stop."
                            )],
                        ));
                    }
                    contract.previous_mode = Some(contract.active_mode.clone());
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
        } else {
            self.event = Some(AsmEvent::AlertAcknowledged { alert_id });
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
