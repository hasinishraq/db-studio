//! Conversation memory. It keeps words and statement outcomes, never rows.

use crate::provider::{AiError, ChatChunk, ChatMessage, ChatRequest, Provider};
use futures::StreamExt;
use std::sync::Arc;
use studio_store::Store;
use studio_types::{ConversationId, QueryRecord, TurnId, TurnRecord};
use tokio_util::sync::CancellationToken;

/// Stored text that triggers compaction.
pub const COMPACT_AFTER_CHARS: usize = 24_000;
/// Turns kept verbatim.
pub const KEEP_TURNS: u32 = 6;

const REDACTED: &str = "[value]";

fn store_error(error: impl std::fmt::Display) -> AiError {
    AiError::Memory(error.to_string())
}

/// Removes shared values from text. A model can quote a sample in its reply, and that reply is
/// stored, so this is what keeps rows out of memory when sharing is on.
pub fn scrub(text: &str, values: &[String]) -> String {
    let mut sorted: Vec<&String> = values.iter().collect();
    // Longest first, so a value inside a longer one cannot leave a fragment behind.
    sorted.sort_by_key(|value| std::cmp::Reverse(value.len()));
    let mut out = text.to_owned();
    for value in sorted {
        out = out.replace(value.as_str(), REDACTED);
    }
    out
}

/// Loads the conversation as chat messages, with its summary.
pub async fn load(
    store: &Store,
    conversation: ConversationId,
) -> Result<(Vec<ChatMessage>, Option<String>), AiError> {
    let turns = store.list_turns(conversation).await.map_err(store_error)?;
    let summary = store.get_summary(conversation).await.map_err(store_error)?;
    let mut messages = Vec::with_capacity(turns.len() * 2);
    for turn in turns {
        messages.push(ChatMessage::User(turn.user_text));
        messages.push(ChatMessage::Assistant {
            text: turn.assistant_text,
            calls: Vec::new(),
        });
    }
    Ok((messages, summary))
}

/// Stores a finished turn.
pub async fn save_turn(
    store: &Store,
    conversation: ConversationId,
    turn: TurnId,
    user_text: &str,
    assistant_text: &str,
    queries: &[QueryRecord],
) -> Result<(), AiError> {
    store
        .append_turn(conversation, turn, user_text, assistant_text, queries)
        .await
        .map_err(store_error)?;
    Ok(())
}

fn stored_chars(turns: &[TurnRecord]) -> usize {
    turns
        .iter()
        .map(|t| t.user_text.len() + t.assistant_text.len())
        .sum()
}

fn transcript(turns: &[TurnRecord]) -> String {
    let mut text = String::new();
    for turn in turns {
        text.push_str(&format!(
            "User: {}\nAssistant: {}\n",
            turn.user_text, turn.assistant_text
        ));
    }
    text
}

async fn summarise(provider: &Arc<dyn Provider>, transcript: String) -> Result<String, AiError> {
    let request = ChatRequest {
        messages: vec![
            ChatMessage::System(
                "Summarise this conversation for later reference. Keep decisions, table names and \
                 what was asked. Omit every data value."
                    .to_owned(),
            ),
            ChatMessage::User(transcript),
        ],
        tools: Vec::new(),
    };
    let mut stream = provider.stream(request, CancellationToken::new()).await?;
    let mut summary = String::new();
    while let Some(chunk) = stream.next().await {
        if let ChatChunk::Text(text) = chunk? {
            summary.push_str(&text);
        }
    }
    Ok(summary)
}

/// Summarises all but the last turns when the conversation has grown too large. Turns are replaced
/// only by a real summary, so a failure loses nothing.
pub async fn compact_if_needed(
    store: &Store,
    provider: &Arc<dyn Provider>,
    conversation: ConversationId,
) -> Result<bool, AiError> {
    let turns = store.list_turns(conversation).await.map_err(store_error)?;
    if stored_chars(&turns) <= COMPACT_AFTER_CHARS || turns.len() <= KEEP_TURNS as usize {
        return Ok(false);
    }
    let older = &turns[..turns.len() - KEEP_TURNS as usize];
    let mut text = String::new();
    if let Some(previous) = store.get_summary(conversation).await.map_err(store_error)? {
        text.push_str(&format!("Earlier summary: {previous}\n"));
    }
    text.push_str(&transcript(older));
    let summary = match summarise(provider, text).await {
        Ok(summary) if !summary.trim().is_empty() => summary,
        Ok(_) => return Ok(false),
        Err(error) => {
            tracing::warn!(%error, "could not compact a conversation");
            return Ok(false);
        }
    };
    store
        .compact(conversation, summary.trim(), KEEP_TURNS)
        .await
        .map_err(store_error)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn scrub_removes_values_longest_first() {
        let values = vec!["alice".to_owned(), "alice@example.com".to_owned()];
        let text = "The owner is alice@example.com, also known as alice.";
        assert_eq!(
            scrub(text, &values),
            "The owner is [value], also known as [value]."
        );
    }
}
