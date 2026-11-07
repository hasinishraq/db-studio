//! Per-session rules taken from the profile.

use crate::manager::SessionError;
use studio_safety::ClassifiedStatement;
use studio_types::{ConnectionProfile, Environment};

/// What a profile allows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SessionPolicy {
    /// Refuse every non-read statement, even with an approval.
    pub read_only: bool,
    /// Environment.
    pub environment: Environment,
}

impl SessionPolicy {
    /// The policy of a profile.
    pub fn of(profile: &ConnectionProfile) -> Self {
        Self {
            read_only: profile.read_only,
            environment: profile.environment,
        }
    }

    /// Refuses a script that holds a non-read on a read-only profile. Runs before any approval is
    /// looked at, so a valid token changes nothing.
    pub(crate) fn check(&self, statements: &[ClassifiedStatement]) -> Result<(), SessionError> {
        if self.read_only && statements.iter().any(|s| !s.verdict.read_only) {
            return Err(SessionError::ReadOnlyProfile);
        }
        Ok(())
    }
}
