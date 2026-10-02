//! Run results combine conformance findings with required scenario completion.
//! An incomplete check prevents PASS without inventing a protocol violation.

use std::process::ExitCode;

use sapient_conformance_core::finding::{Direction, Finding, Severity};
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

/// Bumped on any change to `RunReport`'s serialized JSON shape that could
/// break a downstream consumer (a field removed, renamed, or changing
/// meaning/type), so a beta consumer parsing this report can
/// detect an incompatible change instead of silently misreading a new
/// shape. Adding a new field is compatible and doesn't need a bump.
pub const REPORT_SCHEMA_VERSION: u32 = 1;

/// User-facing run report combining protocol findings with scenario coverage.
/// Construct with `new` to derive consistent `outcome` and `passed` values;
/// callers should treat the finalized report as a snapshot.
#[derive(Serialize)]
pub struct RunReport {
    /// See [`REPORT_SCHEMA_VERSION`].
    pub report_schema_version: u32,
    /// This binary's own version (`CARGO_PKG_VERSION`), so a report can be
    /// matched back to the exact harness build that produced it.
    pub harness_version: &'static str,
    /// Node ID this run stamped on its own outgoing messages -- an explicit
    /// `--node-id`, or the freshly generated default -- needed to correlate
    /// this run's traffic against target-side logs.
    pub harness_node_id: String,
    /// Role played by the harness, rather than the target.
    pub role: Role,
    /// Name of the bundled scenario suite that ran.
    pub suite: String,
    /// Configured connect address (ASM) or listen address (DMM).
    pub target: String,
    /// Wall-clock start of the run, in milliseconds since the Unix epoch --
    /// for correlating against target-side logs.
    pub started_at_unix_millis: u64,
    /// Wall-clock end of the run (report construction time), same units.
    pub ended_at_unix_millis: u64,
    /// `ended_at_unix_millis - started_at_unix_millis`, for convenience.
    pub duration_millis: u64,
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
    /// Notes never affect the verdict. `started_at_unix_millis` is the
    /// caller's own clock reading from when the run began (e.g. before
    /// connecting); `ended_at_unix_millis` is read here, at construction.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        role: Role,
        suite: String,
        target: String,
        harness_node_id: String,
        started_at_unix_millis: u64,
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
        let ended_at_unix_millis = now_unix_millis();
        RunReport {
            report_schema_version: REPORT_SCHEMA_VERSION,
            harness_version: env!("CARGO_PKG_VERSION"),
            harness_node_id,
            role,
            suite,
            target,
            started_at_unix_millis,
            ended_at_unix_millis,
            duration_millis: ended_at_unix_millis.saturating_sub(started_at_unix_millis),
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
        println!(
            "harness_version={} harness_node_id={} duration={}ms",
            self.harness_version, self.harness_node_id, self.duration_millis
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
                    "  [{}] {}: {}{}",
                    finding.rule_id,
                    finding.field_path,
                    sanitize_for_terminal(&finding.message),
                    format_context_suffix(finding)
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
    /// Fixtures expected to fail that currently do so only because they
    /// failed to *decode* as a `SapientMessage` at all -- not a
    /// mismatch (a message that isn't even valid SAPIENT JSON is certainly
    /// non-conformant, matching the legacy harness's own convention), but
    /// this proves nothing about whether the specific rule the fixture is
    /// named for actually fires, unlike one the validator itself rejects.
    /// Always empty is the healthy state; a non-empty list here means a
    /// bundled fixture needs attention even though `passed` is still true.
    pub decode_only_failures: Vec<String>,
}

#[derive(Serialize)]
pub struct FixtureMismatch {
    pub fixture: String,
    pub expected_pass: bool,
    pub actual_pass: bool,
    pub reason: String,
}

impl SelftestReport {
    pub fn new(
        total: usize,
        mismatches: Vec<FixtureMismatch>,
        decode_only_failures: Vec<String>,
    ) -> Self {
        SelftestReport {
            total,
            passed: mismatches.is_empty(),
            mismatches,
            decode_only_failures,
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

        if !self.decode_only_failures.is_empty() {
            println!();
            println!(
                "NOTE: {} fixture(s) expected to fail did so only because they never decoded \
                 as a SapientMessage -- this doesn't confirm the rule they're named for \
                 actually fires:",
                self.decode_only_failures.len()
            );
            for fixture in &self.decode_only_failures {
                println!("  - {fixture}");
            }
        }
    }

    pub fn print_json(&self) {
        print_json(self);
    }
}

/// Renders a `Finding`'s message-context suffix for the text report,
/// e.g. `" (inbound #3 StatusReport)"`, or, when collapsed from
/// several adjacent identical occurrences (see `sapient_session`'s
/// `FindingLog`), `" (inbound #2..#51 StatusReport, repeated 50 times)"` --
/// so two findings sharing a rule_id can still be told apart and matched
/// back to a specific wire message. Empty when `finding.context` is `None`,
/// which is always true for a `Finding` from bare single-message
/// validation (outside any session, e.g. `send`/`selftest`).
pub(crate) fn format_context_suffix(finding: &Finding) -> String {
    let Some(context) = &finding.context else {
        return String::new();
    };
    let direction = match context.direction {
        Direction::Inbound => "inbound",
        Direction::Outbound => "outbound",
    };
    match &finding.last_seen {
        Some(last) if finding.occurrences > 1 => format!(
            " ({direction} #{}..#{} {}, repeated {} times)",
            context.sequence, last.sequence, context.message_type, finding.occurrences
        ),
        _ => format!(
            " ({direction} #{} {})",
            context.sequence, context.message_type
        ),
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

/// Wall-clock time, in milliseconds since the Unix epoch, for
/// [`RunReport::started_at_unix_millis`]/`ended_at_unix_millis`. Pre-1970
/// system clocks (or any other `SystemTime::now` error) fall back to the
/// epoch rather than panicking over a reporting detail.
pub(crate) fn now_unix_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
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
