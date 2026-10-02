#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

mkdir -p "$tmp_dir/src"

cat >"$tmp_dir/Cargo.toml" <<'EOF'
[package]
name = "sapient-test-harness-rules-generator"
version = "0.1.0"
edition = "2024"
publish = false

[dependencies]
regex = "1"
EOF

cat >"$tmp_dir/src/main.rs" <<'RUST_EOF'
//! Extracts the conformance rule catalog from
//! `crates/sapient-conformance-core/src/validation/*.rs` (payload-level
//! rules) and `crates/sapient-session/src/{state,asm_state}.rs`
//! (session-level rules, KI-028) and writes `RULES.md`. Not part of the
//! shipped crate -- run via `scripts/generate-rules.sh`, mirroring
//! `sapient-rs`'s own `regenerate-*-bindings.sh` convention of a
//! throwaway codegen program rather than a permanent dependency of the
//! library.
//!
//! # How extraction works
//!
//! Every rule ID anywhere in this codebase is a `snake_case.dotted.like.this`
//! string literal (see `sapient-conformance-core/src/finding.rs`'s naming
//! convention), which is a visibly different shape from the human-readable
//! message strings next to it (those have spaces, capitals, punctuation).
//! That difference is reliable enough to extract mechanically, but the two
//! source locations use it differently:
//!
//! **`validation/*.rs`** (payload rules, `<type>.<field>.<violation_kind>`):
//! 1. Strip each file's `#[cfg(test)]` module (test-only rule IDs, e.g.
//!    `common.rs`'s `"test.*"` placeholders, aren't real rules).
//! 2. Find every string literal shaped like a rule ID.
//! 3. One whose last `.segment` is `missing`/`invalid`/`malformed`/`empty`
//!    is a fully-resolved rule ID -- pair it with the next prose-shaped
//!    string literal that follows it (best-effort; the two are adjacent
//!    call arguments in every current call site).
//! 4. Anything else (e.g. `"alert.location"`) is a *prefix* passed to one
//!    of `common.rs`'s composite validators (`validate_location`,
//!    `validate_associated_file`, ...), which appends a fixed suffix
//!    internally to build the real rule IDs at runtime -- those are
//!    listed separately with the suffixes they combine with.
//!
//! **`state.rs`/`asm_state.rs`** (session rules): a much wider, freeform
//! vocabulary of trailing words (`interval_exceeded`, `duplicate_id`,
//! `correlation_mismatch`, ...), so the `missing`/`invalid`/`malformed`/
//! `empty` convention doesn't apply, and there's no equivalent of the
//! prefix-plus-runtime-suffix pattern either -- every `rule_id:` found in
//! a `Finding { .. }` literal is treated as fully resolved directly,
//! paired with that same literal's `message:` field.
//!
//! Both passes rely on every rule ID actually being a literal, verified by
//! a completeness sweep at the end: anywhere else under `crates/` with a
//! rule-ID-shaped string literal this generator didn't already account
//! for fails the run loudly, naming the file -- catching both a
//! dynamically-*constructed* rule ID (which no regex over source text can
//! extract the value of) and a genuinely new, unlisted source file in one
//! check, rather than RULES.md silently going stale again.

use std::{
    collections::BTreeMap,
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
};

use regex::Regex;

/// Files in the order they should appear in the generated document.
const FILE_ORDER: &[&str] = &[
    "sapient_message.rs",
    "registration.rs",
    "registration_ack.rs",
    "status_report.rs",
    "detection_report.rs",
    "task.rs",
    "task_ack.rs",
    "alert.rs",
    "alert_ack.rs",
    "error.rs",
    "common.rs",
];

/// Session-layer files scanned separately (see module docs): each entry is
/// (file name under `crates/sapient-session/src/`, a short role label used
/// as that section's heading).
const SESSION_FILES: &[(&str, &str)] = &[
    ("state.rs", "DMM role (`DmmSession`)"),
    ("asm_state.rs", "ASM role (`AsmSession`)"),
];

/// Composite validators (in `common.rs`, reused across files, or local to
/// one file) that take a caller-supplied `rule_id_prefix` and append one
/// of these fixed suffixes internally. Kept as a small hand-maintained
/// table since these functions are few and stable -- see the named
/// source file for ground truth.
const PREFIX_SUFFIXES: &[(&str, &[&str])] = &[
    (
        "common.rs::validate_location",
        &[".x.missing", ".y.missing", ".coordinate_system.invalid", ".datum.missing"],
    ),
    (
        "common.rs::validate_range_bearing",
        &[".coordinate_system.invalid", ".datum.missing"],
    ),
    (
        "common.rs::validate_range_bearing_cone",
        &[".coordinate_system.invalid", ".datum.missing"],
    ),
    (
        "common.rs::validate_location_or_range_bearing",
        &[".missing", "(delegates to validate_range_bearing_cone / validate_location_list)"],
    ),
    (
        "common.rs::validate_location_list",
        &[".locations.empty", "(delegates to validate_location with \"{prefix}.locations\")"],
    ),
    (
        "common.rs::validate_associated_detection",
        &[".node_id.invalid", ".object_id.invalid"],
    ),
    (
        "common.rs::validate_associated_file",
        &[".type.missing", ".url.missing"],
    ),
    (
        "task.rs::validate_parameter (local, not common.rs)",
        &[".name.missing", ".operator.missing", ".value.missing"],
    ),
    (
        "detection_report.rs::validate_sub_class (local, not common.rs)",
        &[".type.missing", ".level.missing"],
    ),
    (
        "detection_report.rs::validate_track_object_info (local, not common.rs)",
        &[".type.missing", ".value.missing"],
    ),
    (
        "registration.rs::validate_duration_units / validate_duration_value (local, not \
         common.rs; KI-025) -- shared by validate_status_interval and validate_settle_time, \
         called with each of the four prefixes below",
        &[".units.missing", ".value.missing", ".value.invalid"],
    ),
];

struct Rule {
    id: String,
    message: Option<String>,
}

/// Extensions of test-fixture file names (e.g. `"send_cli_ki031_good.json"`
/// in `send_cli.rs`) that are structurally indistinguishable from a real
/// `<segment>.<segment>` rule ID -- both sides of the one dot are lowercase
/// `[a-z0-9_]*`. Only used by [`check_completeness`]'s sweep, which has no
/// other way to tell the two apart; the two extraction passes above never
/// see this ambiguity since they only look inside files/contexts a rule ID
/// is actually defined or referenced in.
const FILENAME_EXTENSIONS: &[&str] = &["json", "txt", "log", "csv", "lock"];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let repo_root = PathBuf::from(std::env::args().nth(1).ok_or("missing repo root")?);
    let validation_dir =
        repo_root.join("crates/sapient-conformance-core/src/validation");

    let rule_id_re = Regex::new(r#""([a-z][a-z0-9_]*(?:\.[a-z][a-z0-9_]*)+)""#)?;
    let violation_kinds = ["missing", "invalid", "malformed", "empty"];

    let mut out = String::new();
    writeln!(out, "# Conformance rule catalog")?;
    writeln!(out)?;
    writeln!(
        out,
        "Generated by `scripts/generate-rules.sh` from `crates/sapient-conformance-core/src/validation/*.rs`. \
         Do not hand-edit -- rerun the script instead."
    )?;
    writeln!(out)?;
    writeln!(
        out,
        "Every rule ID follows `<message_type>.<field>.<violation_kind>`, where \
         `violation_kind` is one of `missing` (a mandatory field is absent), `invalid` \
         (present but fails a format/range/enum check), `malformed` (present but \
         structurally broken -- mainly out-of-range timestamp nanos), or `empty` \
         (present but an empty collection/string where at least one entry is required). \
         See `src/finding.rs` for the full contract."
    )?;
    writeln!(out)?;

    let mut known_payload_rule_ids: Vec<String> = Vec::new();
    let mut known_prefixes: Vec<String> = Vec::new();

    for file_name in FILE_ORDER {
        let path = validation_dir.join(file_name);
        let content = fs::read_to_string(&path)
            .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
        let production_code = strip_test_module(&content);

        let (mut resolved, mut prefixes) = (Vec::new(), Vec::new());
        let mut seen_resolved = BTreeMap::new();
        let mut seen_prefixes = Vec::new();

        for capture in rule_id_re.captures_iter(production_code) {
            let literal = capture.get(1).unwrap();
            let text = literal.as_str();
            let is_resolved = text
                .rsplit('.')
                .next()
                .map(|last| violation_kinds.contains(&last))
                .unwrap_or(false);

            if is_resolved {
                // Search from after the literal's closing quote, not after
                // the inner capture group (which ends *before* it) --
                // otherwise the leftover closing quote becomes the opening
                // quote of a bogus first "match".
                let search_from = capture.get(0).unwrap().end();
                let message = find_paired_message(production_code, search_from, &rule_id_re);
                seen_resolved.entry(text.to_string()).or_insert(message);
            } else if !seen_prefixes.contains(&text.to_string()) {
                seen_prefixes.push(text.to_string());
            }
        }

        for (id, message) in seen_resolved {
            known_payload_rule_ids.push(id.clone());
            resolved.push(Rule { id, message });
        }
        for prefix in seen_prefixes {
            known_prefixes.push(prefix.clone());
            prefixes.push(prefix);
        }

        if resolved.is_empty() && prefixes.is_empty() {
            continue;
        }

        writeln!(out, "## `{file_name}`")?;
        writeln!(out)?;

        if !resolved.is_empty() {
            writeln!(out, "| Rule ID | Message |")?;
            writeln!(out, "|---|---|")?;
            for rule in &resolved {
                let message = rule.message.as_deref().unwrap_or("*(see source)*");
                writeln!(out, "| `{}` | {} |", rule.id, message)?;
            }
            writeln!(out)?;
        }

        if !prefixes.is_empty() {
            writeln!(
                out,
                "Rule ID prefixes passed to `common.rs`'s composite validators (each \
                 combines with a fixed suffix at runtime to build the real rule IDs):"
            )?;
            writeln!(out)?;
            for prefix in &prefixes {
                writeln!(out, "- `{prefix}.*`")?;
            }
            writeln!(out)?;
        }
    }

    writeln!(out, "## Composite validator suffixes")?;
    writeln!(out)?;
    writeln!(
        out,
        "Each prefix above is combined with these suffixes by the named function \
         (in `common.rs` unless noted otherwise) to build the fully-resolved rule IDs \
         actually produced at runtime."
    )?;
    writeln!(out)?;
    for (function, suffixes) in PREFIX_SUFFIXES {
        writeln!(out, "- `{function}`: {}", suffixes.join(", "))?;
    }
    writeln!(out)?;

    writeln!(out, "## Session-layer findings")?;
    writeln!(out)?;
    writeln!(
        out,
        "Findings the session state machines (`crates/sapient-session/src/\
         {{state,asm_state}}.rs`) raise directly, independent of the payload-level \
         checks above -- sequencing, cross-message correlation, and declared-vs-actual \
         contract enforcement that a single message in isolation can't check. These \
         don't follow the `<type>.<field>.<violation_kind>` convention above; see each \
         rule ID's own source for exactly what it means."
    )?;
    writeln!(out)?;

    let session_dir = repo_root.join("crates/sapient-session/src");
    let mut session_rule_ids: Vec<String> = Vec::new();
    for (file_name, role_label) in SESSION_FILES {
        let path = session_dir.join(file_name);
        let content = fs::read_to_string(&path)
            .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
        let production_code = strip_test_module(&content);

        let rules = extract_session_rules(production_code);
        session_rule_ids.extend(rules.iter().map(|rule| rule.id.clone()));

        if rules.is_empty() {
            continue;
        }

        writeln!(out, "### `{file_name}` -- {role_label}")?;
        writeln!(out)?;
        writeln!(out, "| Rule ID | Message |")?;
        writeln!(out, "|---|---|")?;
        for rule in &rules {
            let message = rule.message.as_deref().unwrap_or("*(see source)*");
            writeln!(out, "| `{}` | {} |", rule.id, message)?;
        }
        writeln!(out)?;
    }

    let mut known_rule_ids = known_payload_rule_ids;
    known_rule_ids.extend(session_rule_ids);
    check_completeness(
        &repo_root,
        &rule_id_re,
        &validation_dir,
        &session_dir,
        &known_rule_ids,
        &known_prefixes,
    )?;

    fs::write(repo_root.join("RULES.md"), out)?;

    Ok(())
}

/// Everything in `content` up to (not including) the file's `#[cfg(test)]`
/// module, so test-only placeholder rule IDs aren't picked up.
fn strip_test_module(content: &str) -> &str {
    match content.find("#[cfg(test)]") {
        Some(index) => &content[..index],
        None => content,
    }
}

/// Best-effort pairing: the next string literal after `rule_id`'s closing
/// quote that isn't itself rule-ID-shaped is treated as its message.
/// `(?s)` (dotall) lets this match a message that's backslash-continued
/// onto a second physical source line (common for the longer session
/// messages, and needed once a message no longer fits one line under
/// rustfmt's width limit) -- `unescape_continuations` then collapses each
/// `\`-newline-whitespace sequence the same way `rustc` does, so the
/// extracted text reads as the one continuous rendered string, not raw
/// source with a literal backslash and indentation baked in.
fn find_paired_message(content: &str, search_from: usize, rule_id_re: &Regex) -> Option<String> {
    const SEARCH_WINDOW: usize = 400;
    let window_end = (search_from + SEARCH_WINDOW).min(content.len());
    let window = &content[search_from..window_end];

    let string_re = Regex::new(r#"(?s)"((?:[^"\\]|\\.)*)""#).ok()?;
    for capture in string_re.captures_iter(window) {
        let text = capture.get(1)?.as_str();
        if !rule_id_re.is_match(&format!("\"{text}\"")) {
            return Some(unescape_continuations(text));
        }
    }
    None
}

/// Renders the raw source text of a Rust string literal's contents as the
/// value it evaluates to at runtime -- the same text a Finding's `message`
/// field actually holds -- rather than leaving `rustc`-level escape syntax
/// visible in a document meant to be read directly. Handles the two escape
/// forms these messages actually use: a `\` immediately before a newline
/// (plus any run of leading whitespace on the next line) collapses to
/// nothing, the standard continuation rule for a string literal that wraps
/// across source lines; `\"` collapses to a literal `"` (needed once a
/// message itself quotes a word, e.g. a mode named `"default"`). Other
/// escape forms (`\n`, `\\`, ...) don't currently appear in any message and
/// aren't handled -- extend this if one starts using them.
fn unescape_continuations(text: &str) -> String {
    let line_continuation = Regex::new(r"\\\r?\n[ \t]*").unwrap();
    let unescaped_quote = Regex::new(r#"\\""#).unwrap();
    let text = line_continuation.replace_all(text, "");
    unescaped_quote.replace_all(&text, "\"").into_owned()
}

/// Finds every `rule_id: "..."` in a `Finding { .. }` literal and pairs it
/// with that same literal's `message:` field, via [`find_labeled_message`].
/// Unlike the payload-rule pass, every match here is treated as fully
/// resolved -- see the module docs for why the `<violation_kind>`
/// convention and the prefix/suffix concept both don't apply to this
/// source.
///
/// This can't reuse [`find_paired_message`]'s "next string literal that
/// isn't rule-ID-shaped" heuristic: a `Finding`'s `field_path` sometimes
/// has no dots at all (e.g. `field_path: "session"`, for a finding with no
/// more specific field to blame), which doesn't match `rule_id_re` either
/// and would otherwise be mistaken for the message that follows it.
/// `Finding`'s fields are labeled (`field_path:`, `severity:`, `message:`),
/// so anchoring on the `message:` label directly sidesteps that ambiguity
/// entirely instead of trying to out-guess it.
fn extract_session_rules(content: &str) -> Vec<Rule> {
    let field_re = Regex::new(r#"rule_id:\s*"([a-z][a-z0-9_]*(?:\.[a-z0-9_]*)+)""#).unwrap();
    let mut seen: BTreeMap<String, Option<String>> = BTreeMap::new();

    for capture in field_re.captures_iter(content) {
        let id = capture.get(1).unwrap().as_str().to_string();
        let search_from = capture.get(0).unwrap().end();
        let message = find_labeled_message(content, search_from);
        seen.entry(id).or_insert(message);
    }

    seen.into_iter().map(|(id, message)| Rule { id, message }).collect()
}

/// Finds the string literal in the `message:` field of the `Finding { .. }`
/// literal starting at or after `search_from` -- the first `"..."` (or
/// `format!("...", ...)`'s leading literal) after the next `message:`
/// label, however far away that label is (a `Finding`'s `field_path` and
/// `severity` fields come first and vary in length, so this can't use a
/// small fixed-size window the way [`find_paired_message`] does).
fn find_labeled_message(content: &str, search_from: usize) -> Option<String> {
    let label = "message:";
    let label_at = content[search_from..].find(label)? + search_from + label.len();

    let string_re = Regex::new(r#"(?s)"((?:[^"\\]|\\.)*)""#).ok()?;
    let capture = string_re.captures(&content[label_at..])?;
    Some(unescape_continuations(capture.get(1)?.as_str()))
}

/// Scans everywhere else under `crates/` for a rule-ID-shaped string
/// literal this generator didn't already account for, and fails loudly if
/// it finds one -- either a genuinely new, unlisted source file (add it to
/// `FILE_ORDER` or `SESSION_FILES`), or evidence that a rule ID is built
/// with `format!`/concatenation rather than as a plain literal (which no
/// regex over source text can extract the real value of; that rule needs
/// documenting by hand instead). `validation_dir`/`session_dir` are
/// excluded since those are the files already scanned above; everything
/// else under `crates/` -- including each crate's own `tests/` -- is
/// fair game, since a real Finding could in principle be constructed
/// anywhere.
fn check_completeness(
    repo_root: &Path,
    rule_id_re: &Regex,
    validation_dir: &Path,
    session_dir: &Path,
    known_rule_ids: &[String],
    known_prefixes: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    let crates_dir = repo_root.join("crates");
    let mut unaccounted: Vec<String> = Vec::new();
    let mut stack = vec![crates_dir.clone()];

    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                if path.file_name().is_some_and(|name| name == "target") {
                    continue;
                }
                stack.push(path);
                continue;
            }
            if path.extension().is_none_or(|ext| ext != "rs") {
                continue;
            }
            // Already scanned above by name, wherever it lives: a file
            // named e.g. `common.rs` outside `validation/` would otherwise
            // false-positive here for containing a coincidental
            // rule-ID-shaped string unrelated to findings at all.
            let file_name = path.file_name().and_then(|name| name.to_str());
            let already_scanned = path.starts_with(validation_dir)
                || (path.starts_with(session_dir)
                    && file_name.is_some_and(|name| {
                        SESSION_FILES.iter().any(|(scanned, _)| *scanned == name)
                    }));
            if already_scanned {
                continue;
            }

            let content = fs::read_to_string(&path)
                .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
            let production_code = strip_test_module(&content);
            for capture in rule_id_re.captures_iter(production_code) {
                let text = capture.get(1).unwrap().as_str();
                // A rule ID legitimately *referenced* (not defined) outside
                // its home file -- e.g. a session rule ID asserted against
                // in a `sapient-test-harness-cli` integration test -- isn't
                // new, unaccounted-for evidence; only flag it once. Likewise
                // a prefix-combined rule ID (e.g. "alert.location.x.missing",
                // built at runtime from prefix "alert.location" plus a
                // `common.rs` composite validator's runtime suffix) is
                // already documented under "Composite validator suffixes"
                // even though this literal text never appears in
                // `validation/*.rs` itself -- it's only ever written out by
                // a test asserting against it.
                if known_rule_ids.iter().any(|known| known == text) {
                    continue;
                }
                if known_prefixes
                    .iter()
                    .any(|prefix| text == prefix || text.starts_with(&format!("{prefix}.")))
                {
                    continue;
                }
                // A test-fixture file name (e.g. "send_cli_ki031_good.json")
                // is structurally identical to a real rule ID and isn't one.
                if text
                    .rsplit('.')
                    .next()
                    .is_some_and(|ext| FILENAME_EXTENSIONS.contains(&ext))
                {
                    continue;
                }
                let display = format!(
                    "{} (found {:?} in {})",
                    text,
                    text,
                    path.display()
                );
                if !unaccounted.contains(&display) {
                    unaccounted.push(display);
                }
            }
        }
    }

    if !unaccounted.is_empty() {
        return Err(format!(
            "found rule-ID-shaped string literal(s) outside the files this generator \
             scans -- add the containing file to FILE_ORDER or SESSION_FILES (or, if the \
             rule ID is genuinely built dynamically rather than as a plain literal, \
             document it by hand and adjust this check):\n{}",
            unaccounted.join("\n")
        )
        .into());
    }

    Ok(())
}
RUST_EOF

echo "Generating RULES.md..."
cargo run --quiet --manifest-path "$tmp_dir/Cargo.toml" -- "$repo_root"
echo "RULES.md generated at $repo_root/RULES.md"
