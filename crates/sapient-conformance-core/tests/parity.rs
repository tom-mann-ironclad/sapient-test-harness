//! Data-driven parity test against the legacy BSI Flex 335 v2 Test Harness
//! validator's fixture set (`SapientServicesValidator.UnitTests/{True,False}`).
//!
//! Every `.json` file under `tests/fixtures/True` must validate as passing;
//! every file under `tests/fixtures/False` must validate as failing. Adding
//! coverage means dropping a new fixture file in one of those directories,
//! not writing Rust.

use std::{error::Error, fs, path::Path};

use libtest_mimic::{Arguments, Failed, Trial};
use prost::Message;
use prost_for_reflect::Message as _;
use prost_reflect::{DescriptorPool, DynamicMessage, MessageDescriptor};
use sapient_conformance_core::{
    bsi_flex_335_v2_0::SapientMessage, validation::sapient_message::validate_sapient_message,
};

const SAPIENT_MESSAGE_TYPE: &str = "sapient_msg.bsi_flex_335_v2_0.SapientMessage";

fn main() {
    let args = Arguments::from_args();

    let pool = DescriptorPool::decode(sapient_rs::FILE_DESCRIPTOR_SET_BYTES)
        .expect("sapient-rs's embedded file descriptor set should be valid");
    let message_descriptor = pool
        .get_message_by_name(SAPIENT_MESSAGE_TYPE)
        .unwrap_or_else(|| panic!("descriptor pool is missing {SAPIENT_MESSAGE_TYPE}"));

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
            Trial::test(name, move || {
                run_fixture(&path, expected, &message_descriptor)
            })
        })
        .collect()
}

fn run_fixture(
    path: &Path,
    expected: bool,
    message_descriptor: &MessageDescriptor,
) -> Result<(), Failed> {
    let json = fs::read_to_string(path)
        .map_err(|err| format!("failed to read fixture {}: {err}", path.display()))?;

    // Mirrors the legacy harness's own test loop (`SapientServiceValidatorTests.TestMessage`):
    // it parses each fixture with `SapientMessage.Parser.ParseJson`, and a parse failure
    // (`InvalidProtocolBufferException`) is treated as an acceptable outcome for a "False"
    // fixture -- a message that isn't even valid SAPIENT JSON is certainly non-conformant --
    // but as a hard failure for a "True" fixture, which must both parse and validate.
    let message = match decode_fixture(&json, message_descriptor) {
        Ok(message) => message,
        Err(_) if !expected => return Ok(()),
        Err(err) => {
            return Err(format!(
                "failed to parse fixture {} as protobuf JSON: {err}",
                path.display()
            )
            .into());
        }
    };

    let (passed, reason) = validate_sapient_message(message);
    if passed != expected {
        return Err(format!(
            "expected validation to {} but it {}: {reason}",
            if expected { "pass" } else { "fail" },
            if passed { "passed" } else { "failed" },
        )
        .into());
    }

    Ok(())
}

fn decode_fixture(
    json: &str,
    message_descriptor: &MessageDescriptor,
) -> Result<SapientMessage, Box<dyn Error>> {
    let mut deserializer = serde_json::Deserializer::from_str(json);
    let dynamic_message =
        DynamicMessage::deserialize(message_descriptor.clone(), &mut deserializer)?;
    deserializer.end()?;

    let bytes = dynamic_message.encode_to_vec();
    Ok(SapientMessage::decode(bytes.as_slice())?)
}
