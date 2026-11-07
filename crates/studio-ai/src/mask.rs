//! Hides sensitive columns from the model.
//!
//! A query's result columns do not say which table they came from, so a rule matches by column
//! name wherever it appears. That can hide more than asked, which is the safe direction.

use std::collections::HashSet;
use studio_types::MaskingRule;

/// Column names hidden even without a rule.
const ALWAYS_HIDDEN: &[&str] = &[
    "password", "passwd", "pwd", "secret", "api_key", "apikey", "token",
];

/// The replacement value.
pub const MASK: &str = "***";

/// The columns to hide on one profile.
#[derive(Clone, Debug, Default)]
pub struct Masks {
    columns: HashSet<String>,
}

impl Masks {
    /// Builds from a profile's rules.
    pub fn new(rules: &[MaskingRule]) -> Self {
        Self {
            columns: rules.iter().map(|r| r.column_name.to_lowercase()).collect(),
        }
    }

    /// Whether a column's values must be hidden.
    pub fn hides(&self, column: &str) -> bool {
        let name = column.to_lowercase();
        self.columns.contains(&name) || ALWAYS_HIDDEN.iter().any(|word| name.contains(word))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn rules_match_any_case_and_sensitive_names_always_match() {
        let rule = MaskingRule {
            schema_name: "s".into(),
            table_name: "t".into(),
            column_name: "Email".into(),
        };
        let masks = Masks::new(&[rule]);
        assert!(masks.hides("email"));
        assert!(masks.hides("EMAIL"));
        assert!(masks.hides("password_hash"));
        assert!(masks.hides("reset_token"));
        assert!(!masks.hides("name"));
    }
}
