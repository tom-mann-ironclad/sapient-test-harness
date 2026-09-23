//! Implements the `selftest` subcommand: runs the shipped binary's
//! validation logic against its own bundled fixture set (embedded at
//! compile time from `sapient-conformance-core/tests/fixtures`, the same
//! set `tests/parity.rs` runs at build time) and confirms every fixture
//! still classifies the way it's expected to. No network needed -- this
//! answers "is the harness itself healthy", not "does some target
//! conform", so a developer (or CI) can run it before trusting a `run`
//! result.

use std::process::ExitCode;

use include_dir::{Dir, include_dir};
use sapient_conformance_core::{
    fixture_json::{decode_sapient_message_json, sapient_message_descriptor},
    validation::sapient_message::validate_sapient_message,
};

use crate::cli::OutputFormat;
use crate::report::{FixtureMismatch, SelftestReport};

static FIXTURES: Dir =
    include_dir!("$CARGO_MANIFEST_DIR/../sapient-conformance-core/tests/fixtures");

pub fn selftest(format: OutputFormat) -> ExitCode {
    let message_descriptor = sapient_message_descriptor();

    let mut total = 0usize;
    let mut mismatches = Vec::new();

    for (dir_name, expected_pass) in [("True", true), ("False", false)] {
        let Some(dir) = FIXTURES.get_dir(dir_name) else {
            eprintln!(
                "error: bundled fixture set is missing its {dir_name}/ directory -- the binary \
                 was built without it"
            );
            return ExitCode::from(2);
        };

        let mut files: Vec<_> = dir.files().collect();
        files.sort_by_key(|file| file.path());

        for file in files {
            total += 1;
            let fixture = format!(
                "{dir_name}::{}",
                file.path()
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .unwrap_or("unknown")
            );

            let Some(json) = file.contents_utf8() else {
                mismatches.push(FixtureMismatch {
                    fixture,
                    expected_pass,
                    actual_pass: false,
                    reason: "bundled fixture is not valid UTF-8".to_string(),
                });
                continue;
            };

            // A decode failure is an acceptable outcome for a fixture
            // expected to fail -- a message that isn't even valid SAPIENT
            // JSON is certainly non-conformant -- so it only counts as a
            // mismatch when `expected_pass` disagrees, exactly like
            // `tests/parity.rs`'s own rule.
            let (actual_pass, reason) = match decode_sapient_message_json(json, &message_descriptor)
            {
                Ok(message) => {
                    let outcome = validate_sapient_message(message);
                    let reason = outcome
                        .findings
                        .first()
                        .map(|finding| format!("[{}] {}", finding.rule_id, finding.message))
                        .unwrap_or_default();
                    (outcome.passed, reason)
                }
                Err(err) => (false, err.to_string()),
            };

            if actual_pass != expected_pass {
                mismatches.push(FixtureMismatch {
                    fixture,
                    expected_pass,
                    actual_pass,
                    reason,
                });
            }
        }
    }

    let report = SelftestReport::new(total, mismatches);
    match format {
        OutputFormat::Text => report.print_text(),
        OutputFormat::Json => report.print_json(),
    }

    if report.passed {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
