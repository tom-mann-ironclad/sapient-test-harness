//! Adjacent-duplicate-collapsing, message-context-tagging accumulator for
//! [`Finding`]s, shared by [`crate::state::DmmSession`] and
//! [`crate::asm_state::AsmSession`].
//!
//! Two independent jobs:
//!
//! 1. **Context (KI-025).** Every `Finding` passing through `push`/`extend`
//!    is stamped with whatever [`MessageContext`] `set_context` last set --
//!    which message (independently numbered per direction) was in flight,
//!    and roughly when. This lives here, not in each call site, because a
//!    session constructs many `Finding`s directly (`Finding { .. }`
//!    literals for session-level rules) as well as ingesting ones from
//!    `sapient_conformance_core` validators that know nothing about a
//!    session at all; one place that stamps context onto everything
//!    passing through means neither kind of call site has to care.
//!
//! 2. **Collapsing (KI-020).** A long-running session (a library caller
//!    with no deadline, unlike the CLI's own bounded runs) can receive many
//!    frames that each trigger the exact same finding -- a peer resending
//!    the same malformed message, or stuck in a bad state that keeps
//!    tripping the same session-level check. Retaining a fresh `Finding`
//!    per occurrence grows without bound. This type instead folds a repeat
//!    of the immediately preceding finding into that entry's `occurrences`
//!    count and `last_seen` context, rather than pushing a duplicate --
//!    bounding the realistic "peer keeps making the same mistake" pattern
//!    without discarding the fact that it happened, how often, or (via
//!    `context`/`last_seen`) over what span.
//!
//!    Deliberately adjacent-only, not a global dedup keyed by content: two
//!    occurrences of the same finding with a different one in between are
//!    not collapsed. A full content-keyed dedup would need to reorder
//!    findings (or track insertion position separately) to still read
//!    chronologically, and would obscure genuine alternation between two
//!    recurring problems. Adjacent collapsing handles the common
//!    pathological case (the same mistake, repeatedly) with a much
//!    simpler, easier-to-reason-about rule -- and, critically, never
//!    collapses two findings that are genuinely about different messages
//!    into one indistinguishable entry, which would defeat the point of
//!    attaching context in the first place.

use sapient_conformance_core::{
    bsi_flex_335_v2_0::sapient_message::Content,
    finding::{Finding, MessageContext},
};

/// Wall-clock time, in milliseconds since the Unix epoch, for
/// [`MessageContext::occurred_at_unix_millis`]. Pre-1970 system clocks (or
/// any other `SystemTime::now` error) fall back to the epoch rather than
/// panicking over a reporting detail.
pub(crate) fn now_unix_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

/// A short, human-readable name for a decoded message's content, for
/// [`MessageContext::message_type`] -- `"undecodable"` for a frame that
/// never decoded far enough to have one, `"no_content"` for a decoded
/// envelope with no `content` set at all (itself a Finding on its own).
pub(crate) fn content_type_name(content: &Option<Content>) -> &'static str {
    match content {
        Some(Content::Registration(_)) => "Registration",
        Some(Content::RegistrationAck(_)) => "RegistrationAck",
        Some(Content::StatusReport(_)) => "StatusReport",
        Some(Content::DetectionReport(_)) => "DetectionReport",
        Some(Content::Task(_)) => "Task",
        Some(Content::TaskAck(_)) => "TaskAck",
        Some(Content::Alert(_)) => "Alert",
        Some(Content::AlertAck(_)) => "AlertAck",
        Some(Content::Error(_)) => "Error",
        None => "no_content",
    }
}

#[derive(Debug, Default)]
pub(crate) struct FindingLog {
    findings: Vec<Finding>,
    /// Context to stamp onto the next finding(s) pushed, set by
    /// `set_context` before a session handles each message. `None` before
    /// the first call, or defensively if a caller somehow pushes without
    /// one first -- an unstamped finding is still recorded, just without
    /// context, rather than panicking a session mid-run over a reporting
    /// detail.
    current_context: Option<MessageContext>,
}

impl FindingLog {
    pub(crate) fn as_slice(&self) -> &[Finding] {
        &self.findings
    }

    /// Set the message context every subsequent `push`/`extend` stamps onto
    /// what it records, until the next call. A session calls this once
    /// before processing each inbound message, and once before each
    /// harness-initiated outbound one -- so every finding produced while
    /// handling that one message, however deep the call chain, is
    /// attributed to it.
    pub(crate) fn set_context(&mut self, context: MessageContext) {
        self.current_context = Some(context);
    }

    /// Drain and return everything recorded so far. Does not reset
    /// `current_context`: a caller mid-way through handling one message
    /// (e.g. a scenario driver calling this between messages) should not
    /// need to re-set context it already set for the message in flight.
    pub(crate) fn take(&mut self) -> Vec<Finding> {
        std::mem::take(&mut self.findings)
    }

    /// Record one finding, stamping it with the current context and
    /// collapsing it into the previous entry if it's an exact content
    /// repeat of the immediately preceding one.
    pub(crate) fn push(&mut self, mut finding: Finding) {
        finding.context = self.current_context.clone();
        finding.occurrences = 1;
        finding.last_seen = None;

        if let Some(stored) = self.findings.last_mut()
            && stored.rule_id == finding.rule_id
            && stored.field_path == finding.field_path
            && stored.severity == finding.severity
            && stored.message == finding.message
        {
            stored.occurrences += 1;
            stored.last_seen = finding.context;
            return;
        }

        self.findings.push(finding);
    }

    /// As [`Self::push`], for a batch (e.g. every finding from one
    /// [`sapient_conformance_core::finding::ValidationOutcome`]).
    pub(crate) fn extend(&mut self, findings: impl IntoIterator<Item = Finding>) {
        for finding in findings {
            self.push(finding);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sapient_conformance_core::finding::{Direction, Severity};

    fn finding(message: &str) -> Finding {
        Finding {
            rule_id: "test.rule".to_string(),
            field_path: "test.field".to_string(),
            severity: Severity::Error,
            message: message.to_string(),
            ..Default::default()
        }
    }

    fn context(sequence: u64) -> MessageContext {
        MessageContext {
            sequence,
            direction: Direction::Inbound,
            message_type: "StatusReport".to_string(),
            occurred_at_unix_millis: 1_000 + sequence,
        }
    }

    #[test]
    fn distinct_findings_are_all_retained() {
        let mut log = FindingLog::default();
        log.push(finding("first"));
        log.push(finding("second"));
        assert_eq!(log.as_slice().len(), 2);
        assert_eq!(log.as_slice()[0].message, "first");
        assert_eq!(log.as_slice()[1].message, "second");
    }

    #[test]
    fn adjacent_repeats_collapse_with_a_count() {
        let mut log = FindingLog::default();
        log.push(finding("oops"));
        log.push(finding("oops"));
        log.push(finding("oops"));
        assert_eq!(log.as_slice().len(), 1);
        assert_eq!(log.as_slice()[0].message, "oops");
        assert_eq!(log.as_slice()[0].occurrences, 3);
    }

    #[test]
    fn repeats_separated_by_a_different_finding_are_not_collapsed() {
        let mut log = FindingLog::default();
        log.push(finding("oops"));
        log.push(finding("different"));
        log.push(finding("oops"));
        assert_eq!(log.as_slice().len(), 3);
        assert_eq!(log.as_slice()[0].message, "oops");
        assert_eq!(log.as_slice()[2].message, "oops");
        assert_eq!(log.as_slice()[0].occurrences, 1);
        assert_eq!(log.as_slice()[2].occurrences, 1);
    }

    #[test]
    fn extend_collapses_within_the_batch_too() {
        let mut log = FindingLog::default();
        log.extend(vec![finding("oops"), finding("oops")]);
        assert_eq!(log.as_slice().len(), 1);
        assert_eq!(log.as_slice()[0].occurrences, 2);
    }

    #[test]
    fn take_drains_but_a_repeat_right_after_is_still_not_collapsed() {
        let mut log = FindingLog::default();
        log.push(finding("oops"));
        let drained = log.take();
        assert_eq!(drained.len(), 1);
        assert!(log.as_slice().is_empty());

        // After a take, a repeat of the same finding must not be treated as
        // adjacent to what was already drained -- it's the first entry of
        // a fresh accumulation.
        log.push(finding("oops"));
        assert_eq!(log.as_slice().len(), 1);
        assert_eq!(log.as_slice()[0].occurrences, 1);
    }

    #[test]
    fn pushed_findings_are_stamped_with_the_current_context() {
        let mut log = FindingLog::default();
        log.set_context(context(1));
        log.push(finding("oops"));
        assert_eq!(log.as_slice()[0].context, Some(context(1)));
        assert_eq!(log.as_slice()[0].last_seen, None);
    }

    #[test]
    fn a_finding_pushed_before_any_context_is_set_is_still_recorded() {
        let mut log = FindingLog::default();
        log.push(finding("oops"));
        assert_eq!(log.as_slice().len(), 1);
        assert_eq!(log.as_slice()[0].context, None);
    }

    #[test]
    fn collapsed_repeats_track_first_and_last_context_and_a_count() {
        let mut log = FindingLog::default();
        log.set_context(context(1));
        log.push(finding("oops"));
        log.set_context(context(2));
        log.push(finding("oops"));
        log.set_context(context(3));
        log.push(finding("oops"));

        assert_eq!(log.as_slice().len(), 1);
        let collapsed = &log.as_slice()[0];
        assert_eq!(collapsed.occurrences, 3);
        assert_eq!(
            collapsed.context,
            Some(context(1)),
            "first occurrence's context"
        );
        assert_eq!(
            collapsed.last_seen,
            Some(context(3)),
            "most recent occurrence's context"
        );
    }
}
