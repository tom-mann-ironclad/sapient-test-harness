//! Run results combine conformance findings with required scenario completion.
//! An incomplete check prevents PASS without inventing a protocol violation.

use std::process::ExitCode;

use sapient_conformance_core::finding::{Finding, Severity};
use serde::Serialize;

use crate::cli::Role;
use crate::completion::{CheckStatus, OperationalError, ScenarioCheck, ScenarioResult};

/// Conformance/coverage verdict. Operational failures are reported separately.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RunOutcome {
    /// Required checks completed (or were explicitly skipped), with no error findings.
    Passed,
    /// At least one error finding exists, even if checks are also incomplete.
    Failed,
    /// No error findings exist, but execution failed or a required check did not complete.
    Incomplete,
}

/// User-facing run report combining protocol findings with scenario coverage.
/// Construct with `new` to derive consistent `outcome` and `passed` values;
/// callers should treat the finalized report as a snapshot.
#[derive(Serialize)]
pub struct RunReport {
    /// Role played by the harness, rather than the target.
    pub role: Role,
    /// Name of the bundled scenario suite that ran.
    pub suite: String,
    /// Configured connect address (ASM) or listen address (DMM).
    pub target: String,
    /// Compatibility summary for CI: true exactly when outcome is Passed.
    pub passed: bool,
    /// Verdict derived from findings and check completion, with errors taking precedence.
    pub outcome: RunOutcome,
    /// Coverage of each declared scenario step, including reasons for skips.
    pub checks: Vec<ScenarioCheck>,
    /// Session findings; warnings alone do not prevent a completed run from passing.
    pub findings: Vec<Finding>,
    /// Progress and termination observations, retained for diagnosis.
    pub notes: Vec<String>,
    /// Execution failure, if any; forces exit 2 and prevents PASS.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operational_error: Option<OperationalError>,
}

impl RunReport {
    /// Finalize a scenario: error findings yield Failed, otherwise unfinished
    /// checks or operational errors yield Incomplete, otherwise Passed.
    /// Notes never affect the verdict.
    pub fn new(
        role: Role,
        suite: String,
        target: String,
        findings: Vec<Finding>,
        scenario: ScenarioResult,
    ) -> Self {
        let outcome = if findings.iter().any(|f| f.severity == Severity::Error) {
            RunOutcome::Failed
        } else if scenario.operational_error.is_some() || !scenario.is_complete() {
            RunOutcome::Incomplete
        } else {
            RunOutcome::Passed
        };
        let passed = outcome == RunOutcome::Passed;
        RunReport {
            role,
            suite,
            target,
            passed,
            findings,
            outcome,
            checks: scenario.checks,
            notes: scenario.notes,
            operational_error: scenario.operational_error,
        }
    }

    /// Return 0 only for a passing report; failed and incomplete reports return 1.
    /// Operational errors take precedence and return 2, retaining conformance findings.
    pub fn exit_code(&self) -> ExitCode {
        if self.operational_error.is_some() {
            ExitCode::from(2)
        } else if self.passed {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        }
    }

    /// Print the verdict, findings, per-step completion, and notes to stdout.
    pub fn print_text(&self) {
        println!(
            "sapient-harness run -- role={} suite={} target={}",
            self.role, self.suite, self.target
        );
        println!();
        if self.passed {
            println!("PASS -- required scenario checks completed; no conformance errors.");
        } else if self.outcome == RunOutcome::Incomplete {
            println!("INCOMPLETE -- execution stopped or required scenario checks did not finish.");
        } else {
            let error_count = self
                .findings
                .iter()
                .filter(|f| f.severity == Severity::Error)
                .count();
            println!("FAIL -- {error_count} finding(s).");
        }

        if let Some(error) = &self.operational_error {
            println!(
                "Execution stopped during {} ({}) -- {}",
                error.stage, error.kind, error.message
            );
        }

        for severity in [Severity::Error, Severity::Warning] {
            let group: Vec<&Finding> = self
                .findings
                .iter()
                .filter(|f| f.severity == severity)
                .collect();
            if group.is_empty() {
                continue;
            }
            println!();
            println!("{severity:?}:");
            for finding in group {
                println!(
                    "  [{}] {}: {}",
                    finding.rule_id,
                    finding.field_path,
                    sanitize_for_terminal(&finding.message)
                );
            }
        }

        println!();
        println!("Scenario checks:");
        for check in &self.checks {
            println!("  {:?}: {:?}", check.check, check.status);
            if let Some(reason) = &check.reason {
                println!("    {reason}");
            } else if check.status == CheckStatus::Incomplete {
                println!("    Required step was not completed before the run ended.");
            }
        }

        if !self.notes.is_empty() {
            println!();
            println!("Notes:");
            for note in &self.notes {
                println!("  - {}", sanitize_for_terminal(note));
            }
        }
    }

    /// Serialize this finalized report to stdout for CI consumers.
    pub fn print_json(&self) {
        print_json(self);
    }
}

/// The result of `selftest`: does the shipped binary's bundled fixture set
/// still classify the way `sapient-conformance-core`'s own `tests/parity.rs`
/// asserts it does at build time? A mismatch here means the compiled
/// binary's validation logic and its bundled fixtures have drifted apart
/// somehow -- a health check a developer (or CI) can run before trusting a
/// `run` result against it.
#[derive(Serialize)]
pub struct SelftestReport {
    pub total: usize,
    pub passed: bool,
    pub mismatches: Vec<FixtureMismatch>,
}

#[derive(Serialize)]
pub struct FixtureMismatch {
    pub fixture: String,
    pub expected_pass: bool,
    pub actual_pass: bool,
    pub reason: String,
}

impl SelftestReport {
    pub fn new(total: usize, mismatches: Vec<FixtureMismatch>) -> Self {
        SelftestReport {
            total,
            passed: mismatches.is_empty(),
            mismatches,
        }
    }

    pub fn print_text(&self) {
        println!(
            "sapient-harness selftest -- {} bundled fixtures",
            self.total
        );
        println!();
        if self.passed {
            println!("PASS -- every fixture classified as expected.");
        } else {
            println!(
                "FAIL -- {} of {} fixtures classified unexpectedly:",
                self.mismatches.len(),
                self.total
            );
            println!();
            for mismatch in &self.mismatches {
                println!(
                    "  [{}] expected to {} but {}: {}",
                    mismatch.fixture,
                    if mismatch.expected_pass {
                        "pass"
                    } else {
                        "fail"
                    },
                    if mismatch.actual_pass {
                        "passed"
                    } else {
                        "failed"
                    },
                    mismatch.reason,
                );
            }
        }
    }

    pub fn print_json(&self) {
        print_json(self);
    }
}

/// Maximum characters of a single peer-influenced text-report line before
/// it's truncated. Bounds display only -- `print_json`'s `Finding`s and
/// `notes` retain the full text for diagnosis, since `serde_json` already
/// escapes control characters safely and doesn't need this.
const MAX_DISPLAY_CHARS: usize = 500;

/// Escapes control characters (including ANSI/CSI escape sequences and
/// embedded newlines/carriage returns) and bounds the length of text that
/// may embed unbounded peer-supplied content -- a `Finding`'s message or a
/// scenario note -- before it reaches a terminal via `print_text`. Without
/// this, a peer (or a bug reflecting peer input) can manipulate the
/// terminal (e.g. an ANSI clear-screen sequence) or forge report lines by
/// embedding a newline that starts what looks like a new one. JSON output
/// isn't affected: `serde_json` already escapes controls as `\uXXXX`,
/// which stays inert literal text even if the raw JSON is later `cat`'d to
/// a terminal. `pub(crate)`: `scenario.rs` reuses this to sanitize findings
/// streamed live to stderr, not just the final report.
pub(crate) fn sanitize_for_terminal(text: &str) -> String {
    let mut sanitized = String::with_capacity(text.len());
    for ch in text.chars() {
        if ch.is_control() {
            sanitized.extend(ch.escape_default());
        } else {
            sanitized.push(ch);
        }
    }

    if sanitized.chars().count() > MAX_DISPLAY_CHARS {
        let truncated: String = sanitized.chars().take(MAX_DISPLAY_CHARS).collect();
        format!("{truncated}... (truncated to {MAX_DISPLAY_CHARS} characters for display)")
    } else {
        sanitized
    }
}

fn print_json(report: &impl Serialize) {
    match serde_json::to_string_pretty(report) {
        Ok(json) => println!("{json}"),
        Err(err) => eprintln!("failed to serialize report as JSON: {err}"),
    }
}

#[cfg(test)]
mod sanitize_for_terminal_tests {
    use super::sanitize_for_terminal;

    #[test]
    fn ordinary_text_is_unchanged() {
        assert_eq!(
            sanitize_for_terminal("Registration declares mode \"Alternate\"."),
            "Registration declares mode \"Alternate\"."
        );
    }

    #[test]
    fn ansi_escape_sequences_are_escaped() {
        // ESC [ 2 J is an ANSI "clear screen" sequence.
        let peer_text = "harmless prefix\x1b[2Jmalicious suffix";
        let sanitized = sanitize_for_terminal(peer_text);
        assert!(
            !sanitized.contains('\x1b'),
            "raw ESC byte must not survive sanitization, got {sanitized:?}"
        );
        assert!(sanitized.contains("\\u{1b}"));
    }

    #[test]
    fn embedded_newlines_cannot_forge_report_lines() {
        let peer_text = "real message\nPASS -- forged line";
        let sanitized = sanitize_for_terminal(peer_text);
        assert!(
            !sanitized.contains('\n'),
            "a raw newline must not survive sanitization, got {sanitized:?}"
        );
        assert_eq!(sanitized, "real message\\nPASS -- forged line");
    }

    #[test]
    fn carriage_returns_are_escaped() {
        assert_eq!(sanitize_for_terminal("abc\rdef"), "abc\\rdef");
    }

    #[test]
    fn long_text_is_truncated_for_display() {
        let long_text = "x".repeat(1000);
        let sanitized = sanitize_for_terminal(&long_text);
        assert!(sanitized.len() < long_text.len());
        assert!(sanitized.contains("truncated"));
        assert!(sanitized.starts_with(&"x".repeat(500)));
    }

    #[test]
    fn text_at_the_boundary_is_not_truncated() {
        let exactly_max = "x".repeat(500);
        assert_eq!(sanitize_for_terminal(&exactly_max), exactly_max);
    }
}
