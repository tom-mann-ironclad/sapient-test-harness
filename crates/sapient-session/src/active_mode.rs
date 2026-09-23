//! Resolves which declared mode a `Registration` establishes as the
//! session's initial active mode -- shared by `DmmSession::handle_registration`
//! (accepting an ASM's registration) and `AsmSession::handle_registration_ack`
//! (the self-consistency check on our own outgoing registration), so both
//! sides apply the exact same backward-compatibility rule rather than two
//! copies that could drift apart.
//!
//! `MODE_TYPE_DEFAULT` is the modern, unambiguous way to declare this, but
//! real-world compatibility testing found it's widely unsupported by
//! deployed DMMs -- the enum value was added to replace an older
//! convention where DMMs looked for a `MODE_TYPE_PERMANENT` mode literally
//! named "Default", and that rollout was never completed everywhere. So a
//! registration with no `MODE_TYPE_DEFAULT` mode isn't treated as invalid
//! outright; it falls back to that older convention, recorded as a warning
//! rather than silently accepted or hard-rejected.

use sapient_conformance_core::bsi_flex_335_v2_0::registration::{ModeDefinition, ModeType};

/// Where the resolved active mode came from -- callers use this to decide
/// whether (and what) to warn about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveModeSource {
    /// Exactly one `MODE_TYPE_DEFAULT` mode was declared. The modern,
    /// unambiguous case -- no warning.
    Explicit,
    /// No `MODE_TYPE_DEFAULT` mode; fell back to a `MODE_TYPE_PERMANENT`
    /// mode whose name matches "default" case-insensitively -- the
    /// historical DMM convention `MODE_TYPE_DEFAULT` was introduced to
    /// replace.
    PermanentNamedDefault,
    /// No `MODE_TYPE_DEFAULT` mode and no `MODE_TYPE_PERMANENT` mode named
    /// "default"; fell back to the first declared `MODE_TYPE_PERMANENT`
    /// mode.
    FirstPermanentMode,
}

/// Why no active mode could be resolved at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveModeError {
    /// No `MODE_TYPE_DEFAULT` mode, and no `MODE_TYPE_PERMANENT` mode to
    /// fall back to either -- there's nothing to establish an active mode
    /// from.
    NoCandidate,
    /// More than one `MODE_TYPE_DEFAULT` mode was declared. Unambiguous
    /// but invalid -- the Permanent fallback only applies when there are
    /// zero, not when there's a conflicting excess.
    MultipleDefaultModes(usize),
}

/// Resolves `modes` (a `Registration.mode_definition` list) to the single
/// mode that establishes a session's initial active mode, and how it was
/// resolved. Ties (multiple qualifying `MODE_TYPE_PERMANENT` modes) always
/// go to whichever was declared first in `modes`.
pub fn resolve_active_mode(
    modes: &[ModeDefinition],
) -> Result<(ModeDefinition, ActiveModeSource), ActiveModeError> {
    let default_modes: Vec<&ModeDefinition> = modes
        .iter()
        .filter(|mode| mode.mode_type == Some(ModeType::Default as i32))
        .collect();

    match default_modes.as_slice() {
        [single] => return Ok(((*single).clone(), ActiveModeSource::Explicit)),
        [] => {} // fall through to the Permanent fallback below
        multiple => return Err(ActiveModeError::MultipleDefaultModes(multiple.len())),
    }

    let permanent_modes: Vec<&ModeDefinition> = modes
        .iter()
        .filter(|mode| mode.mode_type == Some(ModeType::Permanent as i32))
        .collect();

    if let Some(named_default) = permanent_modes.iter().find(|mode| {
        mode.mode_name
            .as_deref()
            .is_some_and(|name| name.eq_ignore_ascii_case("default"))
    }) {
        return Ok((
            (*named_default).clone(),
            ActiveModeSource::PermanentNamedDefault,
        ));
    }

    if let Some(first_permanent) = permanent_modes.first() {
        return Ok((
            (*first_permanent).clone(),
            ActiveModeSource::FirstPermanentMode,
        ));
    }

    Err(ActiveModeError::NoCandidate)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mode(name: &str, mode_type: ModeType) -> ModeDefinition {
        ModeDefinition {
            mode_name: Some(name.to_string()),
            mode_type: Some(mode_type as i32),
            mode_description: None,
            settle_time: None,
            maximum_latency: None,
            scan_type: None,
            tracking_type: None,
            duration: None,
            mode_parameter: vec![],
            detection_definition: vec![],
            task: None,
        }
    }

    #[test]
    fn exactly_one_default_mode_is_explicit() {
        let modes = vec![
            mode("Alt", ModeType::Permanent),
            mode("Main", ModeType::Default),
        ];
        let (resolved, source) = resolve_active_mode(&modes).unwrap();
        assert_eq!(resolved.mode_name.as_deref(), Some("Main"));
        assert_eq!(source, ActiveModeSource::Explicit);
    }

    #[test]
    fn multiple_default_modes_is_an_error() {
        let modes = vec![mode("A", ModeType::Default), mode("B", ModeType::Default)];
        assert_eq!(
            resolve_active_mode(&modes),
            Err(ActiveModeError::MultipleDefaultModes(2))
        );
    }

    #[test]
    fn falls_back_to_permanent_mode_named_default_case_insensitively() {
        for name in ["Default", "DEFAULT", "default", "DeFaUlT"] {
            let modes = vec![
                mode("Other", ModeType::Temporary),
                mode(name, ModeType::Permanent),
            ];
            let (resolved, source) = resolve_active_mode(&modes).unwrap();
            assert_eq!(resolved.mode_name.as_deref(), Some(name));
            assert_eq!(source, ActiveModeSource::PermanentNamedDefault);
        }
    }

    #[test]
    fn falls_back_to_first_declared_permanent_mode_when_no_name_matches() {
        let modes = vec![
            mode("Wide", ModeType::Permanent),
            mode("Narrow", ModeType::Permanent),
        ];
        let (resolved, source) = resolve_active_mode(&modes).unwrap();
        assert_eq!(resolved.mode_name.as_deref(), Some("Wide"));
        assert_eq!(source, ActiveModeSource::FirstPermanentMode);
    }

    #[test]
    fn no_default_and_no_permanent_mode_is_an_error() {
        let modes = vec![mode("Only", ModeType::Temporary)];
        assert_eq!(
            resolve_active_mode(&modes),
            Err(ActiveModeError::NoCandidate)
        );
    }

    #[test]
    fn empty_mode_list_is_an_error() {
        assert_eq!(resolve_active_mode(&[]), Err(ActiveModeError::NoCandidate));
    }
}
