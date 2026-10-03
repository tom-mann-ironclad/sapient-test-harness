//! Data-driven parity test against the legacy BSI Flex 335 v2 Test Harness
//! validator's fixture set (`SapientServicesValidator.UnitTests/{True,False}`).
//!
//! Every `.json` file under `tests/fixtures/True` must validate as passing;
//! every file under `tests/fixtures/False` must validate as failing. Adding
//! coverage means dropping a new fixture file in one of those directories,
//! not writing Rust.

use std::{fs, path::Path};

use libtest_mimic::{Arguments, Failed, Trial};
use prost_reflect::MessageDescriptor;
use sapient_conformance_core::{
    fixture_json::{is_parse_only_fixture, sapient_message_descriptor},
    validation::sapient_message::validate_sapient_message,
};

fn main() {
    let args = Arguments::from_args();

    let message_descriptor = sapient_message_descriptor();

    let mut trials = fixture_trials("True", true, &message_descriptor);
    trials.extend(fixture_trials("False", false, &message_descriptor));

    libtest_mimic::run(&args, trials).exit();
}

fn fixture_trials(
    dir_name: &str,
    expected: bool,
    message_descriptor: &MessageDescriptor,
) -> Vec<Trial> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(dir_name);

    let mut entries: Vec<_> = fs::read_dir(&dir)
        .unwrap_or_else(|err| panic!("failed to read fixture dir {}: {err}", dir.display()))
        .map(|entry| entry.expect("failed to read fixture dir entry").path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json"))
        .collect();
    entries.sort();

    entries
        .into_iter()
        .map(|path| {
            let name = format!(
                "{dir_name}::{}",
                path.file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown")
            );
            let message_descriptor = message_descriptor.clone();
            let parse_only = is_parse_only_fixture(&name);
            Trial::test(name, move || {
                run_fixture(&path, expected, parse_only, &message_descriptor)
            })
        })
        .collect()
}

fn run_fixture(
    path: &Path,
    expected: bool,
    parse_only: bool,
    message_descriptor: &MessageDescriptor,
) -> Result<(), Failed> {
    let json = fs::read_to_string(path)
        .map_err(|err| format!("failed to read fixture {}: {err}", path.display()))?;

    // The legacy harness's own test loop (`SapientServiceValidatorTests.TestMessage`)
    // accepts any parse failure for a "False" fixture. That's stricter here: a parse
    // failure proves nothing about the rule a fixture is named for, so only fixtures
    // listed in `PARSE_ONLY_FIXTURES` may fail that way, and every listed one must.
    let message = match sapient_conformance_core::fixture_json::decode_sapient_message_json(
        &json,
        message_descriptor,
    ) {
        Ok(_) if parse_only => {
            return Err(format!(
                "fixture {} is listed in PARSE_ONLY_FIXTURES but decodes; remove it from the list",
                path.display()
            )
            .into());
        }
        Ok(message) => message,
        Err(_) if !expected && parse_only => return Ok(()),
        Err(err) if !expected => {
            return Err(format!(
                "fixture {} fails to parse as protobuf JSON, so it never reaches a validator \
                 rule: {err}. Fix the fixture, or list it in PARSE_ONLY_FIXTURES if the \
                 violation can only be expressed as a parse failure",
                path.display()
            )
            .into());
        }
        Err(err) => {
            return Err(format!(
                "failed to parse fixture {} as protobuf JSON: {err}",
                path.display()
            )
            .into());
        }
    };

    let outcome = validate_sapient_message(message);
    if outcome.passed != expected {
        let reason = outcome
            .findings
            .first()
            .map(|finding| format!("[{}] {}", finding.rule_id, finding.message))
            .unwrap_or_default();
        return Err(format!(
            "expected validation to {} but it {}: {reason}",
            if expected { "pass" } else { "fail" },
            if outcome.passed { "passed" } else { "failed" },
        )
        .into());
    }

    Ok(())
}
