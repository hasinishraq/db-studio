//! Manual transactions.

use crate::manager::{SessionError, SessionManager};
use studio_types::{SessionId, TxMode, TxState};

impl SessionManager {
    /// Switches commit mode. Leaving manual mode with uncommitted work is refused.
    pub async fn set_tx_mode(&self, id: SessionId, mode: TxMode) -> Result<TxState, SessionError> {
        let open = self.get(id)?;
        let mut tx = open.tx.lock().await;
        match (tx.mode, mode) {
            (TxMode::Auto, TxMode::Manual) => {
                open.session.begin(open.policy.read_only).await?;
                *tx = TxState {
                    mode: TxMode::Manual,
                    dirty: false,
                };
            }
            (TxMode::Manual, TxMode::Auto) => {
                if tx.dirty {
                    return Err(SessionError::DirtyTransaction);
                }
                open.session.rollback().await?;
                *tx = TxState::default();
            }
            _ => {}
        }
        Ok(*tx)
    }

    /// The current state.
    pub async fn tx_state(&self, id: SessionId) -> Result<TxState, SessionError> {
        Ok(*self.get(id)?.tx.lock().await)
    }

    /// Commits and opens the next manual transaction.
    pub async fn commit(&self, id: SessionId) -> Result<TxState, SessionError> {
        self.finish_tx(id, true).await
    }

    /// Rolls back and opens the next manual transaction.
    pub async fn rollback(&self, id: SessionId) -> Result<TxState, SessionError> {
        self.finish_tx(id, false).await
    }

    async fn finish_tx(&self, id: SessionId, commit: bool) -> Result<TxState, SessionError> {
        let open = self.get(id)?;
        let mut tx = open.tx.lock().await;
        if tx.mode != TxMode::Manual {
            return Err(SessionError::Refused("no manual transaction is open".to_owned()));
        }
        if commit {
            open.session.commit().await?;
        } else {
            open.session.rollback().await?;
        }
        tx.dirty = false;
        if let Err(error) = open.session.begin(open.policy.read_only).await {
            *tx = TxState::default();
            return Err(error.into());
        }
        Ok(*tx)
    }
}
