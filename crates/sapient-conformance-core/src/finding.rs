//! The structured result type every `validation::*` function returns.
//!
//! Replaces the crate's original `(bool, String)` convention. Each
//! [`Finding`] carries a [`rule_id`](Finding::rule_id) stable across
//! releases -- the ID a report or a future fixture can reference -- rather
//! than only a human-readable message. `field_path` is derived from
//! `rule_id` (its text up to the last `.segment`), so it does not yet
//! track live indices into repeated fields (e.g. which `mode_definition`
//! entry, by position, failed) -- a rule ID like
//! `"registration.mode_definition.settle_time.missing"` identifies *what*
//! failed precisely, but not *which* `mode_definition` in a
//! `repeated` field. Threading live index context through for exact
//! per-instance addressing is a reasonable follow-up once something
//! (the CLI, a report renderer) actually needs it.
//!
//! Envelope validation collects independent field errors, and whole-message
//! validation retains both envelope and payload findings. Most nested payload
//! validators still return their first failure; the collection policy belongs
//! to each validator rather than this result type.

/// How serious a [`Finding`] is. Every check in this crate today is a hard
/// ICD conformance rule, so only [`Severity::Error`] is currently
/// produced; `Warning` exists so a future advisory-level check (e.g. "this
/// is valid but deprecated") doesn't need a breaking type change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
}

/// A single conformance rule violation.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Finding {
    /// Stable identifier for the violated rule, e.g.
    /// `"registration.icd_version.invalid"`. Dot-separated
    /// `<message_type>.<field>.<violation_kind>`, where `violation_kind`
    /// is one of `missing` (required field absent), `invalid` (present
    /// but fails a format/range/enum-membership check), `malformed`
    /// (present but structurally broken), or `empty` (present but an
    /// empty collection/string where at least one entry is required).
    ///
    /// `String`, not `&'static str`: composite validators reused across
    /// several parent contexts (e.g. `validate_location`, called for
    /// `alert.location`, `status_report.node_location`, ...) build their
    /// rule IDs from a caller-supplied prefix, so they aren't knowable at
    /// compile time. The naming *convention* is still stable across runs;
    /// only the Rust representation is owned rather than `'static`.
    pub rule_id: String,
    /// Dotted path to the field that failed, derived from `rule_id` (see
    /// the module docs for what this does and doesn't capture).
    pub field_path: String,
    pub severity: Severity,
    /// Human-readable explanation, suitable for a developer or a report.
    pub message: String,
}

/// The result of validating a SAPIENT message, or some part of one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationOutcome {
    pub passed: bool,
    pub findings: Vec<Finding>,
}

impl ValidationOutcome {
    pub fn pass() -> Self {
        ValidationOutcome {
            passed: true,
            findings: Vec::new(),
        }
    }

    pub fn fail(rule_id: impl Into<String>, message: impl Into<String>) -> Self {
        let rule_id = rule_id.into();
        let field_path = field_path_from_rule_id(&rule_id);
        ValidationOutcome {
            passed: false,
            findings: vec![Finding {
                rule_id,
                field_path,
                severity: Severity::Error,
                message: message.into(),
            }],
        }
    }
}

fn field_path_from_rule_id(rule_id: &str) -> String {
    match rule_id.rsplit_once('.') {
        Some((path, _violation_kind)) => path.to_string(),
        None => rule_id.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pass_has_no_findings() {
        let outcome = ValidationOutcome::pass();
        assert!(outcome.passed);
        assert!(outcome.findings.is_empty());
    }

    #[test]
    fn fail_derives_field_path_from_rule_id() {
        let outcome = ValidationOutcome::fail("registration.icd_version.invalid", "bad version");
        assert!(!outcome.passed);
        assert_eq!(outcome.findings.len(), 1);
        let finding = &outcome.findings[0];
        assert_eq!(finding.rule_id, "registration.icd_version.invalid");
        assert_eq!(finding.field_path, "registration.icd_version");
        assert_eq!(finding.severity, Severity::Error);
        assert_eq!(finding.message, "bad version");
    }

    #[test]
    fn fail_falls_back_to_whole_rule_id_when_no_dot() {
        let outcome = ValidationOutcome::fail("no_dots_here", "message");
        assert_eq!(outcome.findings[0].field_path, "no_dots_here");
    }
}
