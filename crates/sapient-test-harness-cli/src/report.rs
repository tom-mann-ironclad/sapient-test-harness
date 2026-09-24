//! Run results combine conformance findings with required scenario completion.
//! An incomplete check prevents PASS without inventing a protocol violation.

use std::process::ExitCode;

use sapient_conformance_core::finding::{Finding, Severity};
use serde::Serialize;

use crate::cli::Role;
use crate::completion::{CheckStatus, ScenarioCheck, ScenarioResult};

/// Final verdict for a scenario that returned a report. Operational I/O failures
/// still follow the CLI's separate exit-2 path and are not represented here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RunOutcome {
    /// Required checks completed (or were explicitly skipped), with no error findings.
    Passed,
    /// At least one error finding exists, even if checks are also incomplete.
    Failed,
    /// No error findings exist, but at least one required check did not complete.
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
}

impl RunReport {
    /// Finalize a scenario: error findings yield Failed, otherwise unfinished
    /// checks yield Incomplete, otherwise Passed. Notes never affect the verdict.
    pub fn new(
        role: Role,
        suite: String,
        target: String,
        findings: Vec<Finding>,
        scenario: ScenarioResult,
    ) -> Self {
        let outcome = if findings.iter().any(|f| f.severity == Severity::Error) {
            RunOutcome::Failed
        } else if !scenario.is_complete() {
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
        }
    }

    /// Return 0 only for a passing report; failed and incomplete reports return 1.
    /// The command handles operational errors separately with exit 2.
    pub fn exit_code(&self) -> ExitCode {
        if self.passed {
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
            println!("INCOMPLETE -- required scenario checks did not finish.");
        } else {
            let error_count = self
                .findings
                .iter()
                .filter(|f| f.severity == Severity::Error)
                .count();
            println!("FAIL -- {error_count} finding(s).");
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
                    finding.rule_id, finding.field_path, finding.message
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
                println!("  - {note}");
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

fn print_json(report: &impl Serialize) {
    match serde_json::to_string_pretty(report) {
        Ok(json) => println!("{json}"),
        Err(err) => eprintln!("failed to serialize report as JSON: {err}"),
    }
}
