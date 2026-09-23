//! The result of a `run`: findings plus informational notes about what the
//! scenario actually did (skipped steps, timeouts, etc.), rendered as
//! either human-readable text or JSON.

use sapient_conformance_core::finding::{Finding, Severity};
use serde::Serialize;

use crate::cli::Role;

#[derive(Serialize)]
pub struct RunReport {
    pub role: Role,
    pub suite: String,
    pub target: String,
    pub passed: bool,
    pub findings: Vec<Finding>,
    pub notes: Vec<String>,
}

impl RunReport {
    pub fn new(
        role: Role,
        suite: String,
        target: String,
        findings: Vec<Finding>,
        notes: Vec<String>,
    ) -> Self {
        let passed = !findings.iter().any(|f| f.severity == Severity::Error);
        RunReport {
            role,
            suite,
            target,
            passed,
            findings,
            notes,
        }
    }

    pub fn print_text(&self) {
        println!(
            "sapient-harness run -- role={} suite={} target={}",
            self.role, self.suite, self.target
        );
        println!();
        if self.passed {
            println!("PASS -- no conformance findings.");
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

        if !self.notes.is_empty() {
            println!();
            println!("Notes:");
            for note in &self.notes {
                println!("  - {note}");
            }
        }
    }

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
