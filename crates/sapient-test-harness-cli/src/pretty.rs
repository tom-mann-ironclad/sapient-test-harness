//! The `run --verbose` report: the same `RunReport` as `print_text`, laid
//! out as a checklist for a human watching a terminal rather than for
//! grepping. Colour is applied only when stdout is a terminal and
//! `NO_COLOR` is unset, so redirecting the output still yields plain text.

use std::fmt::Write as _;
use std::io::IsTerminal;

use sapient_conformance_core::finding::Severity;

use crate::completion::{Check, CheckStatus};
use crate::report::{RunOutcome, RunReport, format_context_suffix, sanitize_for_terminal};

/// Operational-error stages raised before a connection exists (see `run.rs`):
/// the "Connected" line fails on these, and passes on any later stage.
const PRE_CONNECTION_STAGES: &[&str] = &["connect", "listen_or_accept", "configure_node_id"];

#[derive(Clone, Copy)]
pub(crate) enum Style {
    Bold,
    Dim,
    Green,
    Red,
    Yellow,
}

impl Style {
    fn code(self) -> &'static str {
        match self {
            Style::Bold => "1",
            Style::Dim => "2",
            Style::Green => "32",
            Style::Red => "31",
            Style::Yellow => "33",
        }
    }
}

pub(crate) struct Painter {
    pub(crate) color: bool,
}

impl Painter {
    pub(crate) fn paint(&self, style: Style, text: &str) -> String {
        if self.color {
            format!("\x1b[{}m{text}\x1b[0m", style.code())
        } else {
            text.to_string()
        }
    }

    pub(crate) fn pass(&self) -> String {
        self.paint(Style::Green, "✓")
    }

    pub(crate) fn fail(&self) -> String {
        self.paint(Style::Red, "✗")
    }

    pub(crate) fn skip(&self) -> String {
        self.paint(Style::Yellow, "–")
    }

    pub(crate) fn warn(&self) -> String {
        self.paint(Style::Yellow, "!")
    }
}

/// Whether stdout output should be coloured: a terminal, with `NO_COLOR` unset.
pub(crate) fn stdout_color() -> bool {
    std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none()
}

/// Print the verbose report to stdout, coloured when stdout is a terminal.
pub fn print_verbose(report: &RunReport) {
    print!("{}", render_verbose(report, stdout_color()));
}

/// Render the verbose report. Peer-influenced text (finding messages,
/// operational error detail) goes through `sanitize_for_terminal`, so
/// colouring here never mixes with escape sequences a peer supplied.
pub fn render_verbose(report: &RunReport, color: bool) -> String {
    let p = Painter { color };
    let mut out = String::new();

    let _ = writeln!(
        out,
        "{}",
        p.paint(
            Style::Bold,
            &format!("SAPIENT Harness v{}", report.harness_version)
        )
    );
    let _ = writeln!(out, "Target: {}", report.target);
    let _ = writeln!(out, "Role: {}", report.role.to_string().to_uppercase());
    let _ = writeln!(out, "Suite: {}", suite_label(&report.suite));
    let _ = writeln!(out);

    let connection_failed = report
        .operational_error
        .as_ref()
        .is_some_and(|e| PRE_CONNECTION_STAGES.contains(&e.stage.as_str()));
    if connection_failed {
        let _ = writeln!(out, "{} Connection failed", p.fail());
    } else {
        let _ = writeln!(out, "{} Connected", p.pass());
    }

    for check in &report.checks {
        let label = check_label(check.check, check.status);
        match check.status {
            CheckStatus::Completed => {
                let _ = writeln!(out, "{} {label}", p.pass());
            }
            CheckStatus::Incomplete => {
                let _ = writeln!(
                    out,
                    "{} {label} {}",
                    p.fail(),
                    p.paint(Style::Dim, "(not completed)")
                );
            }
            CheckStatus::Skipped => {
                let reason = check
                    .reason
                    .as_deref()
                    .map(|r| format!(" ({})", sanitize_for_terminal(r)))
                    .unwrap_or_default();
                let _ = writeln!(
                    out,
                    "{} {label}{}",
                    p.skip(),
                    p.paint(Style::Dim, &format!(" skipped{reason}"))
                );
            }
        }
    }

    if !report.findings.is_empty() {
        let _ = writeln!(out);
        let _ = writeln!(out, "{}", p.paint(Style::Bold, "Findings:"));
        for severity in [Severity::Error, Severity::Warning] {
            for finding in report.findings.iter().filter(|f| f.severity == severity) {
                let marker = if severity == Severity::Error {
                    p.fail()
                } else {
                    p.warn()
                };
                let _ = writeln!(
                    out,
                    "{marker} {} {}",
                    p.paint(Style::Bold, &finding.rule_id),
                    p.paint(Style::Dim, &finding.field_path)
                );
                let _ = writeln!(
                    out,
                    "    {}{}",
                    sanitize_for_terminal(&finding.message),
                    p.paint(Style::Dim, &format_context_suffix(finding))
                );
            }
        }
    }

    if let Some(error) = &report.operational_error {
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "{} Execution stopped during {} ({})",
            p.fail(),
            error.stage,
            error.kind
        );
        let _ = writeln!(out, "    {}", sanitize_for_terminal(&error.message));
    }

    let _ = writeln!(out);
    let result = match report.outcome {
        RunOutcome::Passed => p.paint(Style::Green, "PASS"),
        RunOutcome::Failed => p.paint(Style::Red, "FAIL"),
        RunOutcome::Incomplete => p.paint(Style::Yellow, "INCOMPLETE"),
    };
    let _ = writeln!(out, "Result: {}", p.paint(Style::Bold, &result));
    let _ = writeln!(out, "Duration: {} ms", report.duration_millis);
    let _ = writeln!(out);

    let total = report.checks.len();
    let completed = count_status(report, CheckStatus::Completed);
    let skipped = count_status(report, CheckStatus::Skipped);
    let skipped_suffix = if skipped > 0 {
        format!(" ({skipped} skipped)")
    } else {
        String::new()
    };
    let _ = writeln!(
        out,
        "{completed}/{total} scenario checks completed{skipped_suffix}"
    );

    let errors = count_severity(report, Severity::Error);
    let warnings = count_severity(report, Severity::Warning);
    let _ = writeln!(out, "{errors} conformance {}", plural(errors, "error"));
    if warnings > 0 {
        let _ = writeln!(out, "{warnings} {}", plural(warnings, "warning"));
    }

    out
}

/// "accepted" is a claim about the outcome, so it's only added on completion.
fn check_label(check: Check, status: CheckStatus) -> &'static str {
    match check {
        Check::Registration if status == CheckStatus::Completed => "Registration accepted",
        Check::Registration => "Registration",
        Check::StatusReport => "StatusReport",
        Check::DetectionReport => "DetectionReport",
        Check::TaskAck => "TaskAck",
        Check::AlertAck => "AlertAck",
        Check::Goodbye => "Graceful Goodbye",
    }
}

fn suite_label(suite: &str) -> String {
    match suite {
        "v2.0" => "BSI Flex 335 v2.0".to_string(),
        other => other.to_string(),
    }
}

fn count_status(report: &RunReport, status: CheckStatus) -> usize {
    report.checks.iter().filter(|c| c.status == status).count()
}

fn count_severity(report: &RunReport, severity: Severity) -> usize {
    report
        .findings
        .iter()
        .filter(|f| f.severity == severity)
        .count()
}

fn plural(count: usize, word: &str) -> String {
    if count == 1 {
        word.to_string()
    } else {
        format!("{word}s")
    }
}

#[cfg(test)]
mod tests {
    use std::io;

    use super::render_verbose;
    use crate::cli::Role;
    use crate::completion::{Check, ScenarioResult};
    use crate::report::RunReport;

    fn report(scenario: ScenarioResult) -> RunReport {
        RunReport::new(
            Role::Asm,
            "v2.0".into(),
            "localhost:12002".into(),
            "00000000-0000-4000-8000-000000000000".into(),
            crate::report::now_unix_millis(),
            Vec::new(),
            scenario,
        )
    }

    #[test]
    fn passing_run_renders_full_checklist() {
        let mut scenario = ScenarioResult::for_role(Role::Asm);
        for check in [
            Check::Registration,
            Check::StatusReport,
            Check::DetectionReport,
            Check::AlertAck,
            Check::Goodbye,
        ] {
            scenario.complete(check);
        }
        let text = render_verbose(&report(scenario), false);
        assert!(text.starts_with("SAPIENT Harness v"));
        assert!(text.contains("Target: localhost:12002\nRole: ASM\nSuite: BSI Flex 335 v2.0\n"));
        assert!(text.contains(
            "✓ Connected\n✓ Registration accepted\n✓ StatusReport\n✓ DetectionReport\n\
             ✓ AlertAck\n✓ Graceful Goodbye\n"
        ));
        assert!(text.contains("Result: PASS\n"));
        assert!(text.contains("5/5 scenario checks completed\n0 conformance errors\n"));
        assert!(!text.contains('\x1b'), "no ANSI codes without colour");
    }

    #[test]
    fn connection_failure_marks_everything_failed() {
        let mut scenario = ScenarioResult::for_role(Role::Asm);
        scenario.record_error(
            "connect",
            io::Error::new(io::ErrorKind::TimedOut, "could not connect"),
        );
        let text = render_verbose(&report(scenario), false);
        assert!(text.contains("✗ Connection failed\n"));
        assert!(text.contains("✗ Registration (not completed)\n"));
        assert!(text.contains("Execution stopped during connect (TimedOut)"));
        assert!(text.contains("Result: INCOMPLETE\n"));
        assert!(text.contains("0/5 scenario checks completed\n"));
    }

    #[test]
    fn colour_wraps_markers_in_ansi_codes() {
        let scenario = ScenarioResult::for_role(Role::Asm);
        let text = render_verbose(&report(scenario), true);
        assert!(text.contains("\x1b[31m✗\x1b[0m"));
    }
}
