//! Approval tokens: random, single use, short-lived, bound to the exact SQL and session.

use dashmap::DashMap;
use rand::rngs::OsRng;
use rand::RngCore;
use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};
use studio_types::{ApprovalToken, Origin, SessionId};

/// How long a token lives.
const TOKEN_TTL: Duration = Duration::from_secs(60);

/// Why a token was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SafetyError {
    /// The token does not exist, or was already used.
    #[error("no approval for this statement")]
    NoApproval,
    /// The token is too old.
    #[error("the approval expired")]
    Expired,
    /// The token was issued for other SQL or another session.
    #[error("the approval does not match this statement")]
    Mismatch,
}

type Digest32 = [u8; 32];

fn sha256(text: &str) -> Digest32 {
    Sha256::digest(text.as_bytes()).into()
}

/// SHA-256 of `text` as hex, for audit records.
pub fn sha256_hex(text: &str) -> String {
    hex::encode(sha256(text))
}

struct Pending {
    session: SessionId,
    sql_hash: Digest32,
    expires: Instant,
    origin: Origin,
}

/// Outstanding approvals. Only hashes are kept, so a memory dump reveals no usable token.
pub struct ApprovalBook {
    pending: DashMap<Digest32, Pending>,
    ttl: Duration,
}

impl Default for ApprovalBook {
    fn default() -> Self {
        Self::new()
    }
}

impl ApprovalBook {
    /// A book whose tokens last 60 seconds.
    pub fn new() -> Self {
        Self::with_ttl(TOKEN_TTL)
    }

    /// A book with a custom lifetime.
    pub fn with_ttl(ttl: Duration) -> Self {
        Self {
            pending: DashMap::new(),
            ttl,
        }
    }

    /// Issues a token. Call this only from the command that handles a human's click.
    pub fn issue(&self, session: SessionId, sql: &str, origin: Origin) -> ApprovalToken {
        let now = Instant::now();
        self.pending.retain(|_, pending| pending.expires >= now);
        let mut raw = [0u8; 32];
        OsRng.fill_bytes(&mut raw);
        let token = hex::encode(raw);
        self.pending.insert(
            sha256(&token),
            Pending {
                session,
                sql_hash: sha256(sql),
                expires: now + self.ttl,
                origin,
            },
        );
        ApprovalToken(token)
    }

    /// Spends a token. It is removed first, so even a mismatch burns it.
    pub fn consume(
        &self,
        token: &ApprovalToken,
        session: SessionId,
        sql: &str,
    ) -> Result<Origin, SafetyError> {
        let (_, pending) = self
            .pending
            .remove(&sha256(&token.0))
            .ok_or(SafetyError::NoApproval)?;
        if pending.expires < Instant::now() {
            return Err(SafetyError::Expired);
        }
        if pending.session != session || pending.sql_hash != sha256(sql) {
            return Err(SafetyError::Mismatch);
        }
        Ok(pending.origin)
    }

    /// How many approvals are outstanding, including any not yet swept.
    pub fn outstanding(&self) -> usize {
        self.pending.len()
    }
}
