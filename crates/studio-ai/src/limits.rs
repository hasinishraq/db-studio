//! Per-turn limits.

use std::collections::HashSet;

/// Model steps per turn.
pub const MAX_STEPS: usize = 8;
/// Distinct statements per turn.
pub const MAX_QUERIES: usize = 6;

/// What a turn has run so far.
#[derive(Debug, Default)]
pub struct Limits {
    seen: HashSet<String>,
}

impl Limits {
    /// Admits a statement, or says why not. The message goes back to the model.
    pub fn admit(&mut self, sql: &str) -> Result<(), String> {
        let key = sql.trim().trim_end_matches(';').trim().to_lowercase();
        if self.seen.contains(&key) {
            return Err("This exact statement already ran in this turn. Use its earlier result.".to_owned());
        }
        if self.seen.len() >= MAX_QUERIES {
            return Err(format!(
                "The limit of {MAX_QUERIES} statements per turn was reached. Answer now."
            ));
        }
        self.seen.insert(key);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn duplicates_are_refused_whatever_the_spacing() {
        let mut limits = Limits::default();
        assert!(limits.admit("SELECT 1").is_ok());
        assert!(limits.admit("  select 1 ;").is_err());
    }

    #[test]
    fn the_seventh_statement_is_refused() {
        let mut limits = Limits::default();
        for i in 0..MAX_QUERIES {
            assert!(limits.admit(&format!("SELECT {i}")).is_ok());
        }
        assert!(limits.admit("SELECT 99").is_err());
    }
}
