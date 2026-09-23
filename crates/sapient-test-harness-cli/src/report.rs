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
        match serde_json::to_string_pretty(self) {
            Ok(json) => println!("{json}"),
            Err(err) => eprintln!("failed to serialize report as JSON: {err}"),
        }
    }
}
