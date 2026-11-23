//! The person's cleanup rules for project leftovers (#504).
//!
//! Rules widen what one-button Clean may include, so every rule that adds a
//! new kind of removal defaults to off and is turned on by the person in
//! Settings. Missing fields deserialize to these defaults, so an upgrade never
//! turns a rule on.

use serde::{Deserialize, Serialize};

/// Fewest idle days a dependency-store rule accepts: a project that was
/// touched last week is not a leftover.
pub const MIN_DEPENDENCY_IDLE_DAYS: u16 = 7;
/// Longest idle period any rule accepts.
pub const MAX_RULE_IDLE_DAYS: u16 = 365;
/// Default idle period for dependency stores.
pub const DEFAULT_DEPENDENCY_IDLE_DAYS: u16 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(default)]
pub struct CleanupRules {
    /// One-button Clean leaves a project's compiler build output alone until
    /// the project has been inactive this many days. `0` keeps the #494
    /// behavior: any build output nothing has open is included.
    pub build_output_min_idle_days: u16,
    /// Dependency stores of inactive projects (`node_modules`, Python
    /// environments, `vendor`, bundles, `deps`).
    pub dependency_stores: IdleRule,
}

impl Default for CleanupRules {
    fn default() -> Self {
        Self {
            build_output_min_idle_days: 0,
            dependency_stores: IdleRule {
                enabled: false,
                idle_days: DEFAULT_DEPENDENCY_IDLE_DAYS,
            },
        }
    }
}

/// A rule that includes a kind once its project has been inactive long enough.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(default)]
pub struct IdleRule {
    pub enabled: bool,
    pub idle_days: u16,
}

impl Default for IdleRule {
    fn default() -> Self {
        Self {
            enabled: false,
            idle_days: DEFAULT_DEPENDENCY_IDLE_DAYS,
        }
    }
}

impl CleanupRules {
    /// Clamps every period into the range the rules accept.
    pub fn sanitize(mut self) -> Self {
        self.build_output_min_idle_days = self.build_output_min_idle_days.min(MAX_RULE_IDLE_DAYS);
        self.dependency_stores.idle_days = self
            .dependency_stores
            .idle_days
            .clamp(MIN_DEPENDENCY_IDLE_DAYS, MAX_RULE_IDLE_DAYS);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_rules_deserialize_to_off_and_todays_build_output_behavior() {
        let rules: CleanupRules = serde_json::from_str("{}").unwrap();
        assert_eq!(rules, CleanupRules::default());
        assert!(!rules.dependency_stores.enabled);
        assert_eq!(rules.build_output_min_idle_days, 0);

        let partial: CleanupRules =
            serde_json::from_str(r#"{"dependency_stores":{"enabled":true}}"#).unwrap();
        assert!(partial.dependency_stores.enabled);
        assert_eq!(
            partial.dependency_stores.idle_days,
            DEFAULT_DEPENDENCY_IDLE_DAYS
        );
    }

    #[test]
    fn sanitize_clamps_periods() {
        let rules = CleanupRules {
            build_output_min_idle_days: 9_000,
            dependency_stores: IdleRule {
                enabled: true,
                idle_days: 1,
            },
        }
        .sanitize();
        assert_eq!(rules.build_output_min_idle_days, MAX_RULE_IDLE_DAYS);
        assert_eq!(rules.dependency_stores.idle_days, MIN_DEPENDENCY_IDLE_DAYS);

        let long = CleanupRules {
            build_output_min_idle_days: 3,
            dependency_stores: IdleRule {
                enabled: false,
                idle_days: 4_000,
            },
        }
        .sanitize();
        assert_eq!(long.build_output_min_idle_days, 3);
        assert_eq!(long.dependency_stores.idle_days, MAX_RULE_IDLE_DAYS);
    }
}
