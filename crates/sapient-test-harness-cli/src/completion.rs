//! Required steps of the bundled scenarios, independent of conformance findings.

use serde::Serialize;

/// A named step in the bundled scenario. These identify coverage, not protocol
/// rules: completing a step does not imply that its traffic had no findings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Check {
    /// A registration handshake established a usable session contract.
    Registration,
    /// ASM: a normal report was sent. DMM: a normal report passed payload validation.
    StatusReport,
    /// The ASM scenario transmitted its scripted detection; no reply is required.
    DetectionReport,
    /// The DMM received a valid acknowledgement correlated to its probe task.
    TaskAck,
    /// The ASM received a valid acknowledgement correlated to its scripted alert.
    AlertAck,
    /// The ASM transmitted its closing GoodBye; this does not require a peer reply.
    Goodbye,
}

/// Completion of one scenario step, independent of conformance severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    /// Initial state, retained if the run ends before the required evidence arrives.
    Incomplete,
    /// The scenario observed the required send, handshake, or correlated reply.
    Completed,
    /// The check is inapplicable under the scenario policy; a reason explains why.
    Skipped,
}

/// One reportable step. Serialized names use snake_case in JSON reports.
#[derive(Debug, Serialize)]
pub struct ScenarioCheck {
    /// The step being tracked; identifiers should be unique within a scenario.
    pub check: Check,
    /// Whether the step still prevents the scenario from being complete.
    pub status: CheckStatus,
    /// Explanation for a skipped step; omitted from JSON when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Progress accumulated during one run and handed to the report builder.
/// Required steps start incomplete so silence or early termination cannot pass.
/// Findings are collected separately by the session and combined at finalization.
#[derive(Debug)]
pub struct ScenarioResult {
    /// Declared steps in display order, including explicit skips.
    pub checks: Vec<ScenarioCheck>,
    /// Human-readable observations; notes alone never determine success.
    pub notes: Vec<String>,
}

impl ScenarioResult {
    /// Declare the scenario's required steps, all initially incomplete.
    /// Callers should supply each check once; duplicates are not deduplicated.
    ///
    /// # Panics
    /// Panics for an empty list: a scenario must declare what it intends to check.
    pub fn new(checks: &[Check]) -> Self {
        assert!(
            !checks.is_empty(),
            "a scenario must declare required checks"
        );
        Self {
            checks: checks
                .iter()
                .map(|&check| ScenarioCheck {
                    check,
                    status: CheckStatus::Incomplete,
                    reason: None,
                })
                .collect(),
            notes: Vec::new(),
        }
    }

    /// Reset a declared step to incomplete and clear any previous skip reason.
    /// Used when replacement registration changes a not-yet-issued probe's scope.
    ///
    /// # Panics
    /// Panics if the check was not declared at construction.
    pub fn require(&mut self, check: Check) {
        self.set(check, CheckStatus::Incomplete, None);
    }

    /// Record the evidence required for a step and clear any previous skip reason.
    /// This does not suppress findings associated with the same exchange.
    ///
    /// # Panics
    /// Panics if the check was not declared at construction.
    pub fn complete(&mut self, check: Check) {
        self.set(check, CheckStatus::Completed, None);
    }

    /// Mark a step inapplicable, explaining the scenario policy behind the skip.
    /// Missing replies and expired deadlines must remain incomplete, not skipped.
    ///
    /// # Panics
    /// Panics if the check was not declared at construction.
    pub fn skip(&mut self, check: Check, reason: &str) {
        self.set(check, CheckStatus::Skipped, Some(reason.to_string()));
    }

    /// Whether every declared step completed or was explicitly skipped.
    /// An empty list returns false defensively. This is not a conformance verdict;
    /// the report must also consider the session's error findings.
    pub fn is_complete(&self) -> bool {
        !self.checks.is_empty()
            && self
                .checks
                .iter()
                .all(|c| c.status != CheckStatus::Incomplete)
    }

    // Keep status and its explanation in sync; undeclared checks are caller bugs.
    fn set(&mut self, check: Check, status: CheckStatus, reason: Option<String>) {
        let entry = self
            .checks
            .iter_mut()
            .find(|entry| entry.check == check)
            .expect("check must be declared by the scenario");
        entry.status = status;
        entry.reason = reason;
    }
}
