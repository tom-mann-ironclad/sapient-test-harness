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
//! 4. Anything else (e.g. `"alert.location"`) is a *prefix* passed to a
//!    composite validator (`validate_location`, `validate_associated_file`,
//!    ...), which appends fixed suffixes to it at runtime. Those rules are
//!    listed in full under the file that passes the prefix -- see below.
//!
//! **Composite validators** are found and followed mechanically, so their
//! rules can't drift from the code the way a hand-kept suffix list did. A
//! composite is any function in `validation/*.rs` with a `rule_id_prefix`
//! parameter. Starting from each call that passes it a literal prefix,
//! [`Validators::expand`] collects:
//! - every `format!("{rule_id_prefix}<suffix>")` it fails with, paired with
//!   the message argument after it -- a literal, or a parameter the caller
//!   fills in (e.g. `validate_associated_file`'s `type_error`);
//! - `{field}` suffixes, expanded from the `for (value, field) in [...]`
//!   list just before them;
//! - calls on to other composites (by local name, or by a `use ... as ...`
//!   alias of a `common.rs` function), with the prefix passed through as is
//!   or extended (`&format!("{rule_id_prefix}.locations")`).
//!
//! The run fails if a prefix or message can't be resolved, or if the
//! literal prefixes step 4 finds differ from the ones followed here.
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
    collections::{BTreeMap, BTreeSet},
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
    ("state.rs", "Fusion Node role (`DmmSession`)"),
    ("asm_state.rs", "Edge Node role (`AsmSession`)"),
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

    let validators = Validators::load(&validation_dir)?;
    let (composite_rules, followed_prefixes) = validators.expand_entry_points()?;

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

        let (mut resolved, mut prefixes) = (Vec::new(), Vec::<String>::new());
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

        let composite = composite_rules.get(*file_name);
        if resolved.is_empty() && composite.is_none() {
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

        if let Some(composite) = composite {
            let file_prefixes: Vec<String> =
                prefixes.iter().map(|prefix| format!("`{prefix}`")).collect();
            writeln!(
                out,
                "Built at runtime by shared validators, from the prefixes this file passes \
                 them ({}) plus fixed suffixes:",
                file_prefixes.join(", ")
            )?;
            writeln!(out)?;
            writeln!(out, "| Rule ID | Message |")?;
            writeln!(out, "|---|---|")?;
            for (id, messages) in composite {
                // One ID reached by more than one path (e.g. a location or a
                // range-bearing) can carry more than one message.
                writeln!(out, "| `{id}` | {} |", messages.join(" / "))?;
            }
            writeln!(out)?;
        }
    }

    // Step 4's literal prefixes and the calls followed must be the same set:
    // a prefix only one side sees is a call the expansion missed or misread.
    let literal_prefixes: BTreeSet<&String> = known_prefixes.iter().collect();
    let followed: BTreeSet<&String> = followed_prefixes.iter().collect();
    if literal_prefixes != followed {
        return Err(format!(
            "composite expansion disagrees with the literal rule ID prefixes:\n  \
             literal but not followed: {:?}\n  followed but not literal: {:?}",
            literal_prefixes.difference(&followed).collect::<Vec<_>>(),
            followed.difference(&literal_prefixes).collect::<Vec<_>>()
        )
        .into());
    }

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
    known_rule_ids.extend(composite_rules.values().flat_map(|rules| rules.keys().cloned()));
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

/// A composite validator: a function with a `rule_id_prefix` parameter that
/// builds rule IDs at runtime by appending fixed suffixes to it.
struct Composite {
    file: String,
    params: Vec<String>,
    /// Everything after the parameter list, up to the next function.
    body: String,
}

/// (file name, function name) of a composite validator.
type CompositeKey = (String, String);

/// A call to a composite validator.
struct Call {
    callee: CompositeKey,
    args: Vec<String>,
    /// Positions of the argument list's opening and closing parentheses.
    span: (usize, usize),
}

/// Where a composite's rule message comes from: a literal next to the rule
/// ID, or one of its parameters, which the caller fills in.
#[derive(Clone)]
enum MessageSource {
    Literal(String),
    Param(String),
}

/// Rule ID -> its message(s).
type ExpandedRules = BTreeMap<String, Vec<String>>;

/// Per file, the rules its calls to composites produce; plus every prefix
/// those calls pass.
type Expansion = (BTreeMap<String, ExpandedRules>, Vec<String>);

/// The composite validators in `validation/*.rs`, each file's production
/// code, and the names each file can call them by.
struct Validators {
    composites: BTreeMap<CompositeKey, Composite>,
    /// Per file: callable name -> composite. A local function shadows an
    /// imported one of the same name, as in Rust.
    scopes: BTreeMap<String, BTreeMap<String, CompositeKey>>,
    code: BTreeMap<String, String>,
}

impl Validators {
    fn load(validation_dir: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let fn_re = Regex::new(r"(?m)^(?:pub )?fn (\w+)")?;
        let import_re =
            Regex::new(r"(?s)use crate::validation::common::(\{[^}]*\}|\w+(?:\s+as\s+\w+)?)\s*;")?;
        let mut composites = BTreeMap::new();
        let mut code = BTreeMap::new();
        let mut imports: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();

        for file_name in FILE_ORDER {
            let path = validation_dir.join(file_name);
            let content = fs::read_to_string(&path)
                .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
            let production_code = strip_test_module(&content).to_string();

            let starts: Vec<(usize, String)> = fn_re
                .captures_iter(&production_code)
                .map(|capture| (capture.get(0).unwrap().start(), capture[1].to_string()))
                .collect();
            for (index, (start, name)) in starts.iter().enumerate() {
                let end = starts.get(index + 1).map_or(production_code.len(), |(next, _)| *next);
                let item = &production_code[*start..end];
                let open = item
                    .find('(')
                    .ok_or_else(|| format!("{file_name}: no parameter list for fn {name}"))?;
                let close = matching_close(item, open)
                    .ok_or_else(|| format!("{file_name}: unbalanced parameters for fn {name}"))?;
                let params: Vec<String> = split_top_level(&item[open + 1..close])
                    .iter()
                    .map(|param| {
                        let name = param.split(':').next().unwrap_or_default().trim();
                        name.trim_start_matches("mut ").to_string()
                    })
                    .collect();
                if params.iter().any(|param| param == "rule_id_prefix") {
                    composites.insert(
                        (file_name.to_string(), name.clone()),
                        Composite {
                            file: file_name.to_string(),
                            params,
                            body: item[close + 1..].to_string(),
                        },
                    );
                }
            }

            for capture in import_re.captures_iter(&production_code) {
                let list = capture[1].trim_matches(|c| c == '{' || c == '}');
                for item in list.split(',').map(str::trim).filter(|item| !item.is_empty()) {
                    let words: Vec<&str> = item.split_whitespace().collect();
                    let alias = match words.as_slice() {
                        [_, "as", alias] => alias.to_string(),
                        _ => words[0].to_string(),
                    };
                    imports
                        .entry(file_name.to_string())
                        .or_default()
                        .push((alias, words[0].to_string()));
                }
            }
            code.insert(file_name.to_string(), production_code);
        }

        let mut scopes: BTreeMap<String, BTreeMap<String, CompositeKey>> = BTreeMap::new();
        for file_name in FILE_ORDER {
            let scope = scopes.entry(file_name.to_string()).or_default();
            for (alias, original) in imports.get(*file_name).into_iter().flatten() {
                let key = ("common.rs".to_string(), original.clone());
                if composites.contains_key(&key) {
                    scope.insert(alias.clone(), key);
                }
            }
            for (file, name) in composites.keys() {
                if file == file_name {
                    scope.insert(name.clone(), (file.clone(), name.clone()));
                }
            }
        }

        Ok(Self { composites, scopes, code })
    }

    /// Follows every call that passes a literal prefix to a composite, and
    /// returns the rules each file's calls produce, plus the prefixes used.
    fn expand_entry_points(
        &self,
    ) -> Result<Expansion, Box<dyn std::error::Error>> {
        let mut rules: BTreeMap<String, ExpandedRules> = BTreeMap::new();
        let mut prefixes = Vec::new();
        for (file_name, code) in &self.code {
            for call in self.find_calls(file_name, code) {
                let callee = &self.composites[&call.callee];
                let index = callee.params.iter().position(|param| param == "rule_id_prefix");
                let arg = index.and_then(|index| call.args.get(index)).map_or("", String::as_str);
                if string_literal(arg).is_none() {
                    // A composite passing on its own prefix is followed from
                    // its caller; any other prefix can't be read from source.
                    if prefix_value(arg, Some("")).is_some() {
                        continue;
                    }
                    return Err(format!(
                        "{file_name}: {} is passed a rule ID prefix this generator can't \
                         read ({arg:?}); pass a string literal",
                        call.callee.1
                    )
                    .into());
                }
                let (prefix, env) = self
                    .bind(&call, None, &BTreeMap::new())
                    .map_err(|err| format!("{file_name}: {err}"))?;
                if !prefixes.contains(&prefix) {
                    prefixes.push(prefix.clone());
                }
                let file_rules = rules.entry(file_name.clone()).or_default();
                self.expand(&call.callee, &prefix, &env, &mut Vec::new(), file_rules)?;
            }
        }
        Ok((rules, prefixes))
    }

    /// Collects the rules `key` produces when called with `prefix`, and with
    /// `env` binding its message parameters, including through the
    /// composites it calls.
    fn expand(
        &self,
        key: &CompositeKey,
        prefix: &str,
        env: &BTreeMap<String, String>,
        stack: &mut Vec<(CompositeKey, String)>,
        out: &mut ExpandedRules,
    ) -> Result<(), String> {
        let frame = (key.clone(), prefix.to_string());
        if stack.contains(&frame) {
            return Ok(()); // recursion with the same prefix, e.g. nested sub-classes
        }
        stack.push(frame);

        let composite = &self.composites[key];
        let calls = self.find_calls(&composite.file, &composite.body);
        for (suffix, source) in emitted_rules(composite, &calls)? {
            let message = match source {
                MessageSource::Literal(text) => text,
                MessageSource::Param(param) => env.get(&param).cloned().ok_or_else(|| {
                    format!("{}::{}: no caller passes `{param}` (prefix {prefix:?})", key.0, key.1)
                })?,
            };
            let messages = out.entry(format!("{prefix}{suffix}")).or_default();
            if !messages.contains(&message) {
                messages.push(message);
            }
        }
        for call in &calls {
            let (callee_prefix, callee_env) = self
                .bind(call, Some(prefix), env)
                .map_err(|err| format!("{}::{}: {err}", key.0, key.1))?;
            self.expand(&call.callee, &callee_prefix, &callee_env, stack, out)?;
        }

        stack.pop();
        Ok(())
    }

    /// The prefix and message parameters `call` passes to its callee, given
    /// the caller's own prefix and bound parameters.
    fn bind(
        &self,
        call: &Call,
        current_prefix: Option<&str>,
        env: &BTreeMap<String, String>,
    ) -> Result<(String, BTreeMap<String, String>), String> {
        let callee = &self.composites[&call.callee];
        let mut prefix = None;
        let mut callee_env = BTreeMap::new();
        for (param, arg) in callee.params.iter().zip(&call.args) {
            if param == "rule_id_prefix" {
                prefix = prefix_value(arg, current_prefix);
            } else if let Some(text) = string_literal(arg) {
                callee_env.insert(param.clone(), text);
            } else if let Some(text) = env.get(arg.as_str()) {
                callee_env.insert(param.clone(), text.clone());
            }
        }
        let prefix = prefix.ok_or_else(|| {
            format!("can't resolve the rule ID prefix passed to {}", call.callee.1)
        })?;
        Ok((prefix, callee_env))
    }

    /// Calls in `text` (from `file_name`) to composite validators.
    fn find_calls(&self, file_name: &str, text: &str) -> Vec<Call> {
        let call_re = Regex::new(r"\b([a-z_][a-z0-9_]*)\s*\(").unwrap();
        let scope = &self.scopes[file_name];
        let mut calls = Vec::new();
        for capture in call_re.captures_iter(text) {
            let name = capture.get(1).unwrap();
            let Some(callee) = scope.get(name.as_str()) else {
                continue;
            };
            if text[..name.start()].trim_end().ends_with("fn") {
                continue; // a definition, not a call
            }
            let open = capture.get(0).unwrap().end() - 1;
            let Some(close) = matching_close(text, open) else {
                continue;
            };
            calls.push(Call {
                callee: callee.clone(),
                args: split_top_level(&text[open + 1..close]),
                span: (open, close),
            });
        }
        calls
    }
}

/// The `(suffix, message)` pairs a composite fails with directly, i.e. its
/// `format!("{rule_id_prefix}...")` rule IDs that aren't arguments to a
/// further composite call (those are prefixes, followed separately).
fn emitted_rules(
    composite: &Composite,
    calls: &[Call],
) -> Result<Vec<(String, MessageSource)>, String> {
    let emit_re = Regex::new(r#"format!\(\s*"\{rule_id_prefix\}([^"]*)"\s*\)"#).unwrap();
    let message_re =
        Regex::new(r#"(?s)^\s*,\s*(?:"((?:[^"\\]|\\.)*)"|([a-z_][a-z0-9_]*)\b)"#).unwrap();
    let body = &composite.body;
    let mut out = Vec::new();
    for capture in emit_re.captures_iter(body) {
        let whole = capture.get(0).unwrap();
        if calls.iter().any(|call| call.span.0 < whole.start() && whole.end() <= call.span.1) {
            continue;
        }
        let suffix = &capture[1];
        let message = message_re.captures(&body[whole.end()..]).ok_or_else(|| {
            format!("{}: no message argument after the rule ID ending {suffix:?}", composite.file)
        })?;
        let source = match (message.get(1), message.get(2)) {
            (Some(text), _) => MessageSource::Literal(unescape_continuations(text.as_str())),
            (_, Some(param)) => MessageSource::Param(param.as_str().to_string()),
            _ => unreachable!("message_re matches one alternative"),
        };
        if suffix.contains("{field}") {
            let fields = loop_fields(body, whole.start()).ok_or_else(|| {
                format!("{}: no `in [...]` field list before {suffix:?}", composite.file)
            })?;
            for field in fields {
                out.push((suffix.replace("{field}", &field), source.clone()));
            }
        } else {
            out.push((suffix.to_string(), source));
        }
    }
    Ok(out)
}

/// The field names in the `for (value, field) in [(.., "x"), ...]` list
/// that precedes `position` in `body`.
fn loop_fields(body: &str, position: usize) -> Option<Vec<String>> {
    let open = body[..position].rfind("in [")? + 3;
    let close = matching_close(body, open)?;
    let field_re = Regex::new(r#""(\w+)"\s*,?\s*\)"#).unwrap();
    let fields: Vec<String> = field_re
        .captures_iter(&body[open..close])
        .map(|capture| capture[1].to_string())
        .collect();
    (!fields.is_empty()).then_some(fields)
}

/// The prefix an argument passes on: a literal, the caller's own
/// `rule_id_prefix`, or `&format!("{rule_id_prefix}<more>")`.
fn prefix_value(arg: &str, current: Option<&str>) -> Option<String> {
    if let Some(text) = string_literal(arg) {
        return Some(text);
    }
    let current = current?;
    if arg == "rule_id_prefix" {
        return Some(current.to_string());
    }
    let extended_re = Regex::new(r#"^&?format!\(\s*"\{rule_id_prefix\}([^"]*)"\s*\)$"#).unwrap();
    extended_re
        .captures(arg)
        .map(|capture| format!("{current}{}", &capture[1]))
}

/// The value of `arg` if it is a single string literal.
fn string_literal(arg: &str) -> Option<String> {
    let literal_re = Regex::new(r#"(?s)^"((?:[^"\\]|\\.)*)"$"#).unwrap();
    literal_re
        .captures(arg.trim())
        .map(|capture| unescape_continuations(&capture[1]))
}

/// The index of the bracket that closes the one at `open` (`(`, `[` or
/// `{`), skipping over string literals.
fn matching_close(text: &str, open: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let (mut depth, mut in_string, mut index) = (0usize, false, open);
    while index < bytes.len() {
        match (in_string, bytes[index]) {
            (true, b'\\') => index += 1,
            (true, b'"') => in_string = false,
            (true, _) => {}
            (false, b'"') => in_string = true,
            (false, b'(' | b'[' | b'{') => depth += 1,
            (false, b')' | b']' | b'}') => {
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
        index += 1;
    }
    None
}

/// `text` split at its top-level commas (not inside brackets or strings),
/// each part trimmed, empty parts (a trailing comma) dropped.
fn split_top_level(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let (mut depth, mut in_string, mut start, mut index) = (0usize, false, 0, 0);
    let mut parts = Vec::new();
    while index < bytes.len() {
        match (in_string, bytes[index]) {
            (true, b'\\') => index += 1,
            (true, b'"') => in_string = false,
            (true, _) => {}
            (false, b'"') => in_string = true,
            (false, b'(' | b'[' | b'{') => depth += 1,
            (false, b')' | b']' | b'}') => depth = depth.saturating_sub(1),
            (false, b',') if depth == 0 => {
                parts.push(text[start..index].trim().to_string());
                start = index + 1;
            }
            _ => {}
        }
        index += 1;
    }
    parts.push(text[start..].trim().to_string());
    parts.retain(|part| !part.is_empty());
    parts
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
                // anything under a composite prefix (e.g.
                // "alert.location.x.missing", or the field path
                // "alert.location.x") is covered by the composite expansion
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
