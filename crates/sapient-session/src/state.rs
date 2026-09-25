//! Pure, synchronous session state machine for the harness playing the
//! **DMM** role (accepting a connection from an ASM under test). This
//! is original design confirmed against the standard's own principal
//! author, not a port of legacy behaviour (the legacy C# harness has no
//! enforced session state machine to match).
//!
//! No I/O here: [`DmmSession::on_bytes`] takes raw bytes in and returns an
//! optional raw-message reply out, so this module is fully unit-testable
//! without a socket. The async driver (`dmm.rs`) owns the actual framing
//! and TCP.

use std::collections::HashSet;

use prost::Message;
use prost_types::Timestamp;
use sapient_conformance_core::{
    bsi_flex_335_v2_0::{
        Alert, AlertAck, DetectionReport, Error as ErrorMessage, Registration, RegistrationAck,
        SapientMessage, StatusReport, Task, TaskAck,
        alert_ack::AlertAckStatus,
        registration::ModeDefinition,
        sapient_message::Content,
        status_report::System,
        task::{Command as TaskCommand, command::Command as TaskCommandKind},
    },
    finding::{Finding, Severity},
    validation::{
        alert::validate_alert, detection_report::validate_detection_report,
        registration::validate_registration, sapient_message::validate_envelope,
        task_ack::validate_task_ack,
    },
};

use crate::active_mode::{ActiveModeError, ActiveModeSource, resolve_active_mode};

/// Progress from the most recently processed message. These events let callers
/// distinguish acknowledged work from state cleared by GoodBye/re-registration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DmmEvent {
    /// A registration payload established or replaced the local contract.
    /// Envelope diagnostics do not suppress this progress event.
    /// The driver still has to transmit the returned RegistrationAck.
    RegistrationAccepted,
    /// A non-GoodBye report passed payload validation. Session-level findings
    /// (such as a mode mismatch or late interval) may still accompany it.
    StatusReportValidated,
    /// A valid TaskAck matched an outstanding task, regardless of its status.
    /// Receipt alone does not establish successful task execution.
    TaskAcknowledged {
        /// Correlation ID of the task that was acknowledged.
        task_id: String,
    },
    /// A GoodBye caused the contract to be cleared; this is not a transport EOF
    /// or a guarantee that the whole message passed validation.
    GoodbyeReceived,
}

/// Where a [`DmmSession`] currently is in the protocol lifecycle.
#[derive(Debug, Clone)]
pub enum SessionState {
    /// Waiting for a `Registration`. Nothing else is meaningfully
    /// processable yet -- this is also where the session returns to
    /// after an explicit `GoodBye` status report.
    AwaitingRegistration,
    /// A `Registration` was accepted; this is the active contract. Boxed
    /// since `RegisteredContract` (which embeds a full `Registration`) is
    /// much larger than the unit `AwaitingRegistration` variant.
    Registered(Box<RegisteredContract>),
}

/// The declared contract for an active session, captured from the most
/// recently accepted `Registration`.
#[derive(Debug, Clone)]
pub struct RegisteredContract {
    pub node_id: String,
    pub registration: Registration,
    /// The `ModeDefinition` currently in effect. Starts as the
    /// `MODE_TYPE_DEFAULT` mode and changes on a `mode_change` `Task`.
    pub active_mode: ModeDefinition,
    /// The peer-declared timestamp of the most recent `StatusReport`,
    /// used for the retroactive interval check -- not wall-clock receipt
    /// time, so this is deterministic and testable without real delays.
    pub last_status_report_timestamp: Option<Timestamp>,
    /// `task_id`s issued by this session that haven't yet been
    /// acknowledged by a matching `TaskAck`.
    pub outstanding_task_ids: HashSet<String>,
    /// `alert_id`s this session has acknowledged are tracked implicitly
    /// (an `AlertAck` is sent synchronously in reply to every valid
    /// `Alert`); this set exists for symmetry and future use, e.g. if
    /// ack sending becomes asynchronous.
    pub outstanding_alert_ids: HashSet<String>,
}

/// Session state machine for the harness acting as DMM: it accepts a
/// connection from an ASM under test, so it *receives*
/// `Registration`/`StatusReport`/`DetectionReport`/`Alert`/`TaskAck`, and
/// *sends* `RegistrationAck`/`Task`/`AlertAck`/`Error`.
pub struct DmmSession {
    /// This harness's own node ID, stamped on every outgoing message.
    harness_node_id: String,
    state: SessionState,
    findings: Vec<Finding>,
    /// The raw bytes of whatever's currently being processed by
    /// `on_message`/the decode-failure path in `on_bytes` -- scratch
    /// space so `error_reply` can embed the actual offending packet in
    /// `Error.packet` without threading `raw` through every handler's
    /// signature.
    current_raw: Vec<u8>,
    /// Single-message progress slot, reset before decoding each inbound frame.
    /// Findings have separate retention; this slot is not an event queue.
    event: Option<DmmEvent>,
}

impl DmmSession {
    pub fn new(harness_node_id: impl Into<String>) -> Self {
        DmmSession {
            harness_node_id: harness_node_id.into(),
            state: SessionState::AwaitingRegistration,
            findings: Vec::new(),
            current_raw: Vec::new(),
            event: None,
        }
    }

    /// Consume progress from the most recent `on_bytes` call, at most once.
    /// Every new frame replaces any unconsumed event, including on decode failure.
    /// Call after each processed frame; absence of an event does not imply success.
    pub fn take_event(&mut self) -> Option<DmmEvent> {
        self.event.take()
    }

    pub fn state(&self) -> &SessionState {
        &self.state
    }

    pub fn findings(&self) -> &[Finding] {
        &self.findings
    }

    /// Drain and return every finding recorded so far.
    pub fn take_findings(&mut self) -> Vec<Finding> {
        std::mem::take(&mut self.findings)
    }

    /// Feed raw bytes received from the peer (already de-framed -- the
    /// driver strips the 4-byte length prefix before calling this).
    /// Returns the raw bytes of a reply to send back, if the protocol
    /// requires one.
    pub fn on_bytes(&mut self, raw: &[u8]) -> Option<Vec<u8>> {
        self.event = None;
        self.current_raw = raw.to_vec();

        let message = match SapientMessage::decode(raw) {
            Ok(message) => message,
            Err(err) => {
                let registered = matches!(self.state, SessionState::Registered(_));
                self.findings.push(Finding {
                    rule_id: "session.framing.undecodable".to_string(),
                    field_path: "session.framing".to_string(),
                    severity: Severity::Error,
                    message: format!("received bytes that don't decode as a SapientMessage: {err}"),
                });
                // Error is scoped to the post-Registration steady state.
                // Pre-Registration, undecodable input just gets recorded,
                // there's no contract yet to reply meaningfully under.
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
        let peer_node_id = message.node_id.clone();
        let peer_timestamp = message.timestamp;
        let content = message.content.clone();

        match content {
            Some(Content::Registration(registration)) => {
                self.handle_registration(peer_node_id, registration)
            }
            Some(Content::StatusReport(status_report)) => {
                self.handle_status_report(peer_timestamp, status_report)
            }
            Some(Content::DetectionReport(detection_report)) => {
                self.handle_detection_report(detection_report)
            }
            Some(Content::Alert(alert)) => self.handle_alert(alert),
            Some(Content::TaskAck(task_ack)) => self.handle_task_ack(task_ack),
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

    fn handle_registration(
        &mut self,
        peer_node_id: Option<String>,
        registration: Registration,
    ) -> Option<SapientMessage> {
        let outcome = validate_registration(registration.clone());

        if !outcome.passed {
            let reason = outcome
                .findings
                .first()
                .map(|f| f.message.clone())
                .unwrap_or_else(|| "registration failed validation".to_string());
            self.findings.extend(outcome.findings);
            // A rejected (or re-attempted-but-invalid) Registration drops
            // any existing contract -- re-registration is a full
            // re-declaration, and a failed one shouldn't leave a
            // stale contract quietly in effect.
            self.state = SessionState::AwaitingRegistration;
            return Some(self.registration_ack_reply(false, vec![reason]));
        }

        let active_mode = match resolve_active_mode(&registration.mode_definition) {
            Ok((mode, ActiveModeSource::Explicit)) => mode,
            Ok((mode, ActiveModeSource::PermanentNamedDefault)) => {
                self.findings.push(Finding {
                    rule_id: "session.registration.default_mode_via_permanent_name".to_string(),
                    field_path: "registration.mode_definition".to_string(),
                    severity: Severity::Warning,
                    message: format!(
                        "Registration declares no mode with mode_type MODE_TYPE_DEFAULT; using \
                         the MODE_TYPE_PERMANENT mode named {:?} as the initial active mode, \
                         matching the legacy DMM convention MODE_TYPE_DEFAULT was introduced to \
                         replace. Declare MODE_TYPE_DEFAULT explicitly to avoid this warning.",
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
                        "Registration declares no mode with mode_type MODE_TYPE_DEFAULT and no \
                         MODE_TYPE_PERMANENT mode named \"default\"; falling back to the first \
                         declared MODE_TYPE_PERMANENT mode ({:?}) as the initial active mode. \
                         Declare MODE_TYPE_DEFAULT explicitly, or name a Permanent mode \
                         \"Default\", to avoid this warning.",
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
                    message: "Registration must declare either a mode with mode_type \
                              MODE_TYPE_DEFAULT, or (for backward compatibility) at least one \
                              mode with mode_type MODE_TYPE_PERMANENT, so the session has a \
                              starting mode."
                        .to_string(),
                });
                self.state = SessionState::AwaitingRegistration;
                return Some(self.registration_ack_reply(
                    false,
                    vec![
                        "No mode declared with mode_type MODE_TYPE_DEFAULT or \
                         MODE_TYPE_PERMANENT."
                            .to_string(),
                    ],
                ));
            }
            Err(ActiveModeError::MultipleDefaultModes(count)) => {
                self.findings.push(Finding {
                    rule_id: "session.registration.multiple_default_modes".to_string(),
                    field_path: "registration.mode_definition".to_string(),
                    severity: Severity::Error,
                    message: format!(
                        "Registration declares {count} modes with mode_type MODE_TYPE_DEFAULT; \
                         exactly one is required."
                    ),
                });
                self.state = SessionState::AwaitingRegistration;
                return Some(self.registration_ack_reply(
                    false,
                    vec![
                        "More than one mode declared with mode_type MODE_TYPE_DEFAULT.".to_string(),
                    ],
                ));
            }
        };

        let node_id = peer_node_id.unwrap_or_default();
        self.state = SessionState::Registered(Box::new(RegisteredContract {
            node_id,
            registration,
            active_mode,
            last_status_report_timestamp: None,
            outstanding_task_ids: HashSet::new(),
            outstanding_alert_ids: HashSet::new(),
        }));

        self.event = Some(DmmEvent::RegistrationAccepted);
        Some(self.registration_ack_reply(true, vec![]))
    }

    fn handle_status_report(
        &mut self,
        peer_timestamp: Option<Timestamp>,
        status_report: StatusReport,
    ) -> Option<SapientMessage> {
        let contract = match &mut self.state {
            SessionState::AwaitingRegistration => {
                return self.sequencing_violation("StatusReport");
            }
            SessionState::Registered(contract) => contract,
        };

        // Teardown remains meaningful even for a malformed payload. Record its
        // validation findings before clearing the contract; do not suppress the
        // GoodBye event or attempt an Error reply after the session has ended.
        if status_report.system == Some(System::Goodbye as i32) {
            self.findings
                .extend(validate_status_report_outcome(status_report).findings);
            self.event = Some(DmmEvent::GoodbyeReceived);
            self.state = SessionState::AwaitingRegistration;
            return None;
        }

        let outcome = validate_status_report_outcome(status_report.clone());
        if !outcome.passed {
            self.findings.extend(outcome.findings);
            return Some(self.error_reply(vec!["StatusReport failed validation.".to_string()]));
        }

        // Declared-vs-actual: the ASM's own claimed current mode should
        // match what the harness tracked via mode_change tasks. A
        // mismatch means either side has drifted from the other's
        // understanding of the contract.
        if let Some(reported_mode) = &status_report.mode
            && contract.active_mode.mode_name.as_deref() != Some(reported_mode.as_str())
        {
            self.findings.push(Finding {
                rule_id: "session.status_report.mode_mismatch".to_string(),
                field_path: "status_report.mode".to_string(),
                severity: Severity::Error,
                message: format!(
                    "StatusReport declares mode {reported_mode:?}, but the session's \
                     tracked active mode (from Registration/mode_change tasks) is {:?}.",
                    contract.active_mode.mode_name
                ),
            });
        }

        // Retroactive interval check: compare this report's declared
        // timestamp against the last one, against the interval fixed at
        // Registration time.
        if let (Some(previous), Some(current)) =
            (contract.last_status_report_timestamp, peer_timestamp)
            && let Some(declared_seconds) = contract
                .registration
                .status_definition
                .as_ref()
                .and_then(|definition| definition.status_interval.as_ref())
                .and_then(duration_to_seconds)
            && let Some(elapsed_seconds) = timestamp_diff_seconds(&previous, &current)
            && elapsed_seconds > declared_seconds
        {
            self.findings.push(Finding {
                rule_id: "session.status_report.interval_exceeded".to_string(),
                field_path: "registration.status_definition.status_interval".to_string(),
                severity: Severity::Error,
                message: format!(
                    "StatusReport arrived {elapsed_seconds:.3}s after the previous one, \
                     exceeding the declared interval of {declared_seconds:.3}s."
                ),
            });
        }

        contract.last_status_report_timestamp = peer_timestamp;
        self.event = Some(DmmEvent::StatusReportValidated);
        None
    }

    fn handle_detection_report(
        &mut self,
        detection_report: DetectionReport,
    ) -> Option<SapientMessage> {
        let contract = match &self.state {
            SessionState::AwaitingRegistration => {
                return self.sequencing_violation("DetectionReport");
            }
            SessionState::Registered(contract) => contract,
        };

        let outcome = validate_detection_report(detection_report.clone());
        if !outcome.passed {
            self.findings.extend(outcome.findings);
            return Some(self.error_reply(vec!["DetectionReport failed validation.".to_string()]));
        }

        // Declared-vs-actual, first pass (enum/category membership only
        // , full structural matching is deferred): a reported
        // classification type should be one the active mode actually
        // declared somewhere in its detection class definitions.
        let declared_types: HashSet<&str> = contract
            .active_mode
            .detection_definition
            .iter()
            .flat_map(|definition| definition.detection_class_definition.iter())
            .flat_map(|class_definition| class_definition.class_definition.iter())
            .filter_map(|class| class.r#type.as_deref())
            .collect();

        if !declared_types.is_empty() {
            for classification in &detection_report.classification {
                if let Some(reported_type) = classification.r#type.as_deref()
                    && !declared_types.contains(reported_type)
                {
                    self.findings.push(Finding {
                        rule_id: "session.detection_report.undeclared_classification".to_string(),
                        field_path: "detection_report.classification.type".to_string(),
                        severity: Severity::Error,
                        message: format!(
                            "DetectionReport classifies the object as {reported_type:?}, \
                             which the active mode ({:?}) never declared in its \
                             detection_class_definition.",
                            contract.active_mode.mode_name
                        ),
                    });
                }
            }
        }

        None
    }

    fn handle_alert(&mut self, alert: Alert) -> Option<SapientMessage> {
        let contract = match &mut self.state {
            SessionState::AwaitingRegistration => {
                return self.sequencing_violation("Alert");
            }
            SessionState::Registered(contract) => contract,
        };

        let outcome = validate_alert(alert.clone());
        if !outcome.passed {
            self.findings.extend(outcome.findings);
            return Some(self.error_reply(vec!["Alert failed validation.".to_string()]));
        }

        let alert_id = alert.alert_id.clone().unwrap_or_default();
        contract.outstanding_alert_ids.insert(alert_id.clone());

        Some(self.alert_ack_reply(alert_id))
    }

    fn handle_task_ack(&mut self, task_ack: TaskAck) -> Option<SapientMessage> {
        let contract = match &mut self.state {
            SessionState::AwaitingRegistration => {
                return self.sequencing_violation("TaskAck");
            }
            SessionState::Registered(contract) => contract,
        };

        let outcome = validate_task_ack(task_ack.clone());
        if !outcome.passed {
            self.findings.extend(outcome.findings);
            return Some(self.error_reply(vec!["TaskAck failed validation.".to_string()]));
        }

        let task_id = task_ack.task_id.clone().unwrap_or_default();
        if !contract.outstanding_task_ids.remove(&task_id) {
            self.findings.push(Finding {
                rule_id: "session.task_ack.correlation_mismatch".to_string(),
                field_path: "task_ack.task_id".to_string(),
                severity: Severity::Error,
                message: format!(
                    "TaskAck references task_id {task_id:?}, which doesn't match any Task \
                     this session issued that's still awaiting acknowledgement."
                ),
            });
        } else {
            self.event = Some(DmmEvent::TaskAcknowledged { task_id });
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
            message: "Received a message type the DMM role never expects as inbound traffic \
                      (e.g. Task, AlertAck, RegistrationAck are DMM-to-ASM messages)."
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
                "Received a {message_type} before a successful Registration/RegistrationAck \
                 handshake; Registration must always come first."
            ),
        });
        // No reply: Error is scoped to the post-Registration steady state
        // and there's no established contract yet to build a
        // RegistrationAck-style rejection around, since this isn't a Registration at all.
        None
    }

    /// Issue a `Task` to the ASM (harness-initiated, not a reply to
    /// inbound traffic). Tracks the `task_id` as outstanding until a
    /// matching `TaskAck` arrives, and if the command is a mode change,
    /// updates the tracked active mode.
    pub fn issue_task(&mut self, task: &Task) -> Vec<u8> {
        if let SessionState::Registered(contract) = &mut self.state {
            if let Some(task_id) = &task.task_id {
                contract.outstanding_task_ids.insert(task_id.clone());
            }

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
                    Some(mode) => contract.active_mode = mode.clone(),
                    None => {
                        self.findings.push(Finding {
                            rule_id: "session.task.mode_change_unknown_mode".to_string(),
                            field_path: "task.command.mode_change".to_string(),
                            severity: Severity::Error,
                            message: format!(
                                "Issued a mode_change task targeting mode {target_mode_name:?}, \
                                 which isn't declared anywhere in the registration's \
                                 mode_definition list."
                            ),
                        });
                    }
                }
            }
        } else {
            self.findings.push(Finding {
                rule_id: "session.task.issued_before_registration".to_string(),
                field_path: "session".to_string(),
                severity: Severity::Error,
                message: "A Task was issued before any ASM had registered on this session."
                    .to_string(),
            });
        }

        self.encode(self.wrap(Content::Task(task.clone()), None))
    }

    fn registration_ack_reply(&self, accepted: bool, reasons: Vec<String>) -> SapientMessage {
        let destination = self.peer_node_id();
        self.wrap(
            Content::RegistrationAck(RegistrationAck {
                acceptance: Some(accepted),
                ack_response_reason: reasons,
            }),
            destination,
        )
    }

    fn alert_ack_reply(&self, alert_id: String) -> SapientMessage {
        let destination = self.peer_node_id();
        self.wrap(
            Content::AlertAck(AlertAck {
                alert_id: Some(alert_id),
                reason: vec![],
                alert_ack_status: Some(AlertAckStatus::Accepted as i32),
            }),
            destination,
        )
    }

    fn error_reply(&self, error_messages: Vec<String>) -> SapientMessage {
        let destination = self.peer_node_id();
        self.wrap(
            Content::Error(ErrorMessage {
                packet: Some(self.current_raw.clone()),
                error_message: error_messages,
            }),
            destination,
        )
    }

    fn peer_node_id(&self) -> Option<String> {
        match &self.state {
            SessionState::Registered(contract) => Some(contract.node_id.clone()),
            SessionState::AwaitingRegistration => None,
        }
    }

    fn wrap(&self, content: Content, destination_id: Option<String>) -> SapientMessage {
        SapientMessage {
            timestamp: Some(now_timestamp()),
            node_id: Some(self.harness_node_id.clone()),
            destination_id,
            content: Some(content),
            additional_information: None,
        }
    }
}

fn validate_status_report_outcome(
    status_report: StatusReport,
) -> sapient_conformance_core::finding::ValidationOutcome {
    sapient_conformance_core::validation::status_report::validate_status_report(status_report)
}

fn now_timestamp() -> Timestamp {
    let duration = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    Timestamp {
        seconds: duration.as_secs() as i64,
        nanos: duration.subsec_nanos() as i32,
    }
}

fn timestamp_diff_seconds(earlier: &Timestamp, later: &Timestamp) -> Option<f64> {
    let earlier_nanos = earlier.seconds as f64 * 1e9 + earlier.nanos as f64;
    let later_nanos = later.seconds as f64 * 1e9 + later.nanos as f64;
    let diff = (later_nanos - earlier_nanos) / 1e9;
    if diff.is_finite() {
        Some(diff.max(0.0))
    } else {
        None
    }
}

fn duration_to_seconds(
    duration: &sapient_conformance_core::bsi_flex_335_v2_0::registration::Duration,
) -> Option<f64> {
    use sapient_conformance_core::bsi_flex_335_v2_0::registration::TimeUnits;

    let value = duration.value? as f64;
    let units = TimeUnits::try_from(duration.units?).ok()?;
    let multiplier = match units {
        TimeUnits::Unspecified => return None,
        TimeUnits::Nanoseconds => 1e-9,
        TimeUnits::Microseconds => 1e-6,
        TimeUnits::Milliseconds => 1e-3,
        TimeUnits::Seconds => 1.0,
        TimeUnits::Minutes => 60.0,
        TimeUnits::Hours => 3600.0,
        TimeUnits::Days => 86400.0,
    };
    Some(value * multiplier)
}
