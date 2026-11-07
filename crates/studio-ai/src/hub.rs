//! Active turns and pending reviews.

use dashmap::DashMap;
use std::sync::Arc;
use studio_types::{ApprovalToken, QueryId, SessionId, TurnId};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

/// What a human decided.
#[derive(Debug, PartialEq, Eq)]
pub enum Decision {
    /// Run it. Holds the token for a statement that is not an unattended read.
    Approved(Option<ApprovalToken>),
    /// Do not run it.
    Declined,
}

/// A statement waiting for a human. The sender lives only here, so only `ai_review`, which takes it
/// after a click, can complete the wait.
pub struct PendingReview {
    /// Exact SQL shown on the card.
    pub sql: String,
    /// Target session.
    pub session: SessionId,
    /// Whether it may run as an unattended read once a human agrees.
    pub auto_read: bool,
    /// Completes the wait.
    pub reply: oneshot::Sender<Decision>,
}

#[derive(Default)]
struct Turn {
    cancel: CancellationToken,
    reviews: DashMap<QueryId, PendingReview>,
}

/// Every active turn.
#[derive(Default)]
pub struct AgentHub {
    turns: DashMap<TurnId, Arc<Turn>>,
}

impl AgentHub {
    /// Registers a turn and returns its cancel token.
    pub fn begin(&self, turn: TurnId) -> CancellationToken {
        let entry = Arc::new(Turn::default());
        let token = entry.cancel.clone();
        self.turns.insert(turn, entry);
        token
    }

    /// Ends a turn. Dropping its senders resolves every pending review as declined.
    pub fn finish(&self, turn: TurnId) {
        self.turns.remove(&turn);
    }

    /// Parks a statement for review and returns the receiver to wait on.
    pub fn await_review(
        &self,
        turn: TurnId,
        query: QueryId,
        sql: String,
        session: SessionId,
        auto_read: bool,
    ) -> Option<oneshot::Receiver<Decision>> {
        let entry = self.turns.get(&turn)?;
        let (reply, answer) = oneshot::channel();
        entry.reviews.insert(
            query,
            PendingReview {
                sql,
                session,
                auto_read,
                reply,
            },
        );
        Some(answer)
    }

    /// Takes a pending review for a human's decision.
    pub fn take_review(&self, turn: TurnId, query: QueryId) -> Option<PendingReview> {
        let entry = self.turns.get(&turn)?;
        entry.reviews.remove(&query).map(|(_, pending)| pending)
    }

    /// Cancels a turn, declining its pending reviews.
    pub fn cancel(&self, turn: TurnId) {
        if let Some(entry) = self.turns.get(&turn) {
            entry.cancel.cancel();
            entry.reviews.clear();
        }
    }

    /// Declines one pending query, if it is still waiting.
    pub fn cancel_review(&self, turn: TurnId, query: QueryId) {
        if let Some(pending) = self.take_review(turn, query) {
            let _ = pending.reply.send(Decision::Declined);
        }
    }

    /// Whether a turn is running.
    pub fn is_active(&self, turn: TurnId) -> bool {
        self.turns.contains_key(&turn)
    }
}
