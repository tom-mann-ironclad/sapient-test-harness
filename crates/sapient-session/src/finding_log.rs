//! Adjacent-duplicate-collapsing accumulator for [`Finding`]s, shared by
//! [`crate::state::DmmSession`] and [`crate::asm_state::AsmSession`].
//!
//! A long-running session (a library caller with no deadline, unlike the
//! CLI's own bounded runs) can receive many frames that each trigger the
//! exact same finding -- a peer resending the same malformed message, or
//! stuck in a bad state that keeps tripping the same session-level check.
//! Retaining a fresh [`Finding`] per occurrence grows without bound. This
//! type instead folds a repeat of the immediately preceding finding into
//! that entry's message (`"... (repeated N times)"`) rather than pushing a
//! duplicate, bounding the realistic "peer keeps making the same mistake"
//! pattern without discarding the fact that it happened, or how often.
//!
//! Deliberately adjacent-only, not a global dedup keyed by content: two
//! occurrences of the same finding with a different one in between are not
//! collapsed. A full content-keyed dedup would need to reorder findings
//! (or track insertion position separately) to still read chronologically,
//! and would obscure genuine alternation between two recurring problems.
//! Adjacent collapsing handles the common pathological case (the same
//! mistake, repeatedly) with a much simpler, easier-to-reason-about rule.

use sapient_conformance_core::finding::Finding;

#[derive(Debug, Default)]
pub(crate) struct FindingLog {
    findings: Vec<Finding>,
    /// The most recently pushed finding, in its original (unannotated)
    /// form, plus how many times it's occurred so far. `None` once
    /// consumed via `take` or before anything's been pushed.
    last: Option<(Finding, usize)>,
}

impl FindingLog {
    pub(crate) fn as_slice(&self) -> &[Finding] {
        &self.findings
    }

    /// Drain and return everything recorded so far, resetting the
    /// adjacent-duplicate tracking along with it.
    pub(crate) fn take(&mut self) -> Vec<Finding> {
        self.last = None;
        std::mem::take(&mut self.findings)
    }

    /// Record one finding, collapsing it into the previous entry if it's an
    /// exact repeat of the immediately preceding one.
    pub(crate) fn push(&mut self, finding: Finding) {
        if let Some((original, count)) = &mut self.last
            && original.rule_id == finding.rule_id
            && original.field_path == finding.field_path
            && original.severity == finding.severity
            && original.message == finding.message
        {
            *count += 1;
            let stored = self
                .findings
                .last_mut()
                .expect("`last` is Some only once at least one finding has been pushed");
            stored.message = format!("{} (repeated {count} times)", original.message);
            return;
        }

        self.last = Some((finding.clone(), 1));
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
    use sapient_conformance_core::finding::Severity;

    fn finding(message: &str) -> Finding {
        Finding {
            rule_id: "test.rule".to_string(),
            field_path: "test.field".to_string(),
            severity: Severity::Error,
            message: message.to_string(),
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
        assert_eq!(log.as_slice()[0].message, "oops (repeated 3 times)");
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
    }

    #[test]
    fn extend_collapses_within_the_batch_too() {
        let mut log = FindingLog::default();
        log.extend(vec![finding("oops"), finding("oops")]);
        assert_eq!(log.as_slice().len(), 1);
        assert_eq!(log.as_slice()[0].message, "oops (repeated 2 times)");
    }

    #[test]
    fn take_drains_and_resets_adjacency_tracking() {
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
        assert_eq!(log.as_slice()[0].message, "oops");
    }
}
