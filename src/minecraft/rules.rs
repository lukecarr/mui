//! Evaluate OS/architecture rules for library inclusion.
//!
//! Each library can have `rules` that determine whether it should be
//! included on the current platform. Rules follow allow/disallow logic.

use super::version::Rule;

/// Get the current OS name in Minecraft's format.
pub fn current_os() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "osx"
    } else {
        "linux"
    }
}

/// Get the current architecture in Minecraft's format.
pub fn current_arch() -> &'static str {
    if cfg!(target_arch = "x86") {
        "x86"
    } else if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else if cfg!(target_arch = "arm") {
        "arm"
    } else {
        "unknown"
    }
}

/// Evaluate whether a set of rules allows the current platform.
///
/// Rules logic: if no rules exist, the library is included.
/// If rules exist, they are evaluated in order. The default state is
/// "disallow". Each rule can flip the state to "allow" or "disallow"
/// if its conditions match.
pub fn rules_match(rules: &[Rule]) -> bool {
    if rules.is_empty() {
        return true;
    }

    let mut allowed = false;

    for rule in rules {
        let matches = rule_applies(rule);
        if matches {
            allowed = rule.action == "allow";
        }
    }

    allowed
}

/// Check if a single rule's conditions apply to the current platform.
fn rule_applies(rule: &Rule) -> bool {
    // Check OS conditions
    if let Some(ref os) = rule.os {
        if let Some(ref name) = os.name
            && name != current_os()
        {
            return false;
        }
        if let Some(ref arch) = os.arch
            && arch != current_arch()
        {
            return false;
        }
        // os.version is a regex pattern; we skip it for now (rarely used)
    }

    // Features: we ignore feature-based rules in MVP (demo mode, quick play, etc.)
    if let Some(ref features) = rule.features {
        // If any feature is required as true, and we don't provide it, skip
        for &required in features.values() {
            if required {
                return false; // We don't support feature flags in MVP
            }
        }
    }

    true
}
