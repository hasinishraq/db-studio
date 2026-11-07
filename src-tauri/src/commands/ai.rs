//! AI commands. `ai_review` is the only AI-side caller of `ApprovalBook::issue`.

use super::check_text;
use crate::error::AppResult;
use crate::state::AppState;
use secrecy::SecretString;
use studio_ai::hub::Decision;
use studio_ai::providers::rig_adapter;
use studio_ai::{AgentHub, ChatChunk, ChatMessage, ChatRequest, Provider, TurnOptions};
use studio_safety::ApprovalBook;
use studio_store::{SecretKey, Store};
use studio_types::{
    AiEvent, AiProviderKind, AiSendRequest, AppError, ConversationId, ConversationSummary, ErrorCode,
    MaskingRule, Origin, ProfileId, QueryId, Settings, TurnId, TurnRecord,
};
use tauri::ipc::Channel;
use tauri::State;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

const MAX_MESSAGE: usize = 100_000;

fn provider_name(kind: AiProviderKind) -> &'static str {
    match kind {
        AiProviderKind::Ollama => "ollama",
        AiProviderKind::OpenAi => "openai",
    }
}

async fn api_key(store: &Store, kind: AiProviderKind) -> Result<Option<SecretString>, AppError> {
    let key = SecretKey::AiKey(provider_name(kind).to_owned());
    store.secrets().get(&key).await.app()
}

async fn provider_for(store: &Store, settings: &Settings) -> Result<std::sync::Arc<dyn Provider>, AppError> {
    let key = api_key(store, settings.ai_provider).await?;
    Ok(rig_adapter::build(
        settings.ai_provider,
        &settings.ai_endpoint,
        &settings.ai_model,
        key,
    ))
}

/// Applies a human's decision. A token is minted only here, only for the statement the card showed,
/// and only when that statement needs one.
pub fn review(
    hub: &AgentHub,
    approvals: &ApprovalBook,
    turn: TurnId,
    query: QueryId,
    approved: bool,
) -> Result<(), AppError> {
    let pending = hub
        .take_review(turn, query)
        .ok_or_else(|| AppError::invalid("that statement is not waiting for review"))?;
    let decision = match (approved, pending.auto_read) {
        (false, _) => Decision::Declined,
        (true, true) => Decision::Approved(None),
        (true, false) => Decision::Approved(Some(approvals.issue(pending.session, &pending.sql, Origin::Ai))),
    };
    pending
        .reply
        .send(decision)
        .map_err(|_| AppError::invalid("the turn already ended"))
}

/// Starts a turn, streaming its events.
#[tauri::command]
#[specta::specta]
pub async fn ai_send(
    state: State<'_, AppState>,
    request: AiSendRequest,
    on_event: Channel<AiEvent>,
) -> Result<(), AppError> {
    check_text("the message", &request.text, MAX_MESSAGE)?;
    let settings = state.store.get_settings().await.app()?;
    let provider = provider_for(&state.store, &settings).await?;
    let options = TurnOptions {
        provider,
        model_key: format!("{}:{}", settings.ai_endpoint, settings.ai_model),
        endpoint: settings.ai_endpoint.clone(),
    };
    let (events, mut inbox) = mpsc::channel::<AiEvent>(32);
    let relay = tokio::spawn(async move {
        while let Some(event) = inbox.recv().await {
            let _ = on_event.send(event);
        }
    });
    state.ai.run_turn(request, options, events).await;
    let _ = relay.await;
    Ok(())
}

/// Records a human's decision on a review card.
#[tauri::command]
#[specta::specta]
pub async fn ai_review(
    state: State<'_, AppState>,
    turn_id: TurnId,
    query_id: QueryId,
    approved: bool,
) -> Result<(), AppError> {
    review(state.ai.hub(), &state.approvals, turn_id, query_id, approved)
}

/// Stops a turn.
#[tauri::command]
#[specta::specta]
pub async fn ai_cancel(state: State<'_, AppState>, turn_id: TurnId) -> Result<(), AppError> {
    state.ai.hub().cancel(turn_id);
    Ok(())
}

/// Stops one query of a turn.
#[tauri::command]
#[specta::specta]
pub async fn ai_cancel_query(
    state: State<'_, AppState>,
    turn_id: TurnId,
    query_id: QueryId,
) -> Result<(), AppError> {
    state.ai.hub().cancel_review(turn_id, query_id);
    state.sessions.cancel_query(query_id).await.app()
}

/// Forgets a conversation, or the part of it from a turn onward.
#[tauri::command]
#[specta::specta]
pub async fn ai_forget(
    state: State<'_, AppState>,
    conversation_id: ConversationId,
    from_turn: Option<TurnId>,
) -> Result<(), AppError> {
    match from_turn {
        Some(turn) => state.store.forget_from(conversation_id, turn).await.app(),
        None => state.store.clear_conversation(conversation_id).await.app(),
    }
}

/// Lists conversations.
#[tauri::command]
#[specta::specta]
pub async fn ai_conversations(state: State<'_, AppState>) -> Result<Vec<ConversationSummary>, AppError> {
    state.store.list_conversations().await.app()
}

/// Reads a conversation's turns. They hold no rows.
#[tauri::command]
#[specta::specta]
pub async fn ai_turns(
    state: State<'_, AppState>,
    conversation_id: ConversationId,
) -> Result<Vec<TurnRecord>, AppError> {
    state.store.list_turns(conversation_id).await.app()
}

/// Renames a conversation.
#[tauri::command]
#[specta::specta]
pub async fn ai_rename(
    state: State<'_, AppState>,
    conversation_id: ConversationId,
    title: String,
) -> Result<(), AppError> {
    check_text("the title", &title, 200)?;
    state
        .store
        .rename_conversation(conversation_id, &title)
        .await
        .app()
}

/// Sends a small request to the configured model and reports what it said.
#[tauri::command]
#[specta::specta]
pub async fn ai_test_provider(state: State<'_, AppState>) -> Result<String, AppError> {
    use futures::StreamExt;
    let settings = state.store.get_settings().await.app()?;
    let provider = provider_for(&state.store, &settings).await?;
    let request = ChatRequest {
        messages: vec![ChatMessage::User("Reply with the single word: ready".to_owned())],
        tools: Vec::new(),
    };
    let failed = |e: studio_ai::AiError| AppError::new(ErrorCode::Ai, e.to_string());
    let mut stream = provider
        .stream(request, CancellationToken::new())
        .await
        .map_err(failed)?;
    let mut text = String::new();
    while let Some(chunk) = stream.next().await {
        if let ChatChunk::Text(piece) = chunk.map_err(failed)? {
            text.push_str(&piece);
        }
    }
    Ok(text)
}

/// Lists a profile's masking rules.
#[tauri::command]
#[specta::specta]
pub async fn masking_list(
    state: State<'_, AppState>,
    profile_id: ProfileId,
) -> Result<Vec<MaskingRule>, AppError> {
    state.store.list_masking_rules(profile_id).await.app()
}

/// Replaces a profile's masking rules.
#[tauri::command]
#[specta::specta]
pub async fn masking_set(
    state: State<'_, AppState>,
    profile_id: ProfileId,
    rules: Vec<MaskingRule>,
) -> Result<(), AppError> {
    for rule in &rules {
        check_text("the column", &rule.column_name, 255)?;
    }
    state.store.set_masking_rules(profile_id, &rules).await.app()
}

#[cfg(test)]
mod ai_tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use studio_types::SessionId;

    #[tokio::test]
    async fn a_token_is_minted_only_for_an_approved_write() {
        let hub = AgentHub::default();
        let approvals = ApprovalBook::new();
        let (turn, query, session) = (TurnId::new(), QueryId::new(), SessionId::new());
        hub.begin(turn);
        let answer = hub
            .await_review(turn, query, "DELETE FROM t".into(), session, false)
            .unwrap();
        review(&hub, &approvals, turn, query, true).unwrap();
        let Ok(Decision::Approved(Some(token))) = answer.await else {
            panic!("expected a token")
        };
        assert_eq!(
            approvals.consume(&token, session, "DELETE FROM t"),
            Ok(Origin::Ai)
        );
    }

    #[tokio::test]
    async fn declining_mints_nothing() {
        let hub = AgentHub::default();
        let approvals = ApprovalBook::new();
        let (turn, query) = (TurnId::new(), QueryId::new());
        hub.begin(turn);
        let answer = hub
            .await_review(turn, query, "DELETE FROM t".into(), SessionId::new(), false)
            .unwrap();
        review(&hub, &approvals, turn, query, false).unwrap();
        assert_eq!(answer.await.unwrap(), Decision::Declined);
        assert_eq!(approvals.outstanding(), 0);
    }

    #[tokio::test]
    async fn an_approved_read_needs_no_token() {
        let hub = AgentHub::default();
        let approvals = ApprovalBook::new();
        let (turn, query) = (TurnId::new(), QueryId::new());
        hub.begin(turn);
        let answer = hub
            .await_review(turn, query, "SELECT 1".into(), SessionId::new(), true)
            .unwrap();
        review(&hub, &approvals, turn, query, true).unwrap();
        assert_eq!(answer.await.unwrap(), Decision::Approved(None));
        assert_eq!(approvals.outstanding(), 0);
    }

    #[test]
    fn a_review_with_nothing_pending_mints_nothing() {
        let hub = AgentHub::default();
        let approvals = ApprovalBook::new();
        let error = review(&hub, &approvals, TurnId::new(), QueryId::new(), true)
            .err()
            .unwrap();
        assert_eq!(error.code, ErrorCode::InvalidInput);
        assert_eq!(approvals.outstanding(), 0);
    }

    #[tokio::test]
    async fn a_decision_applies_once() {
        let hub = AgentHub::default();
        let approvals = ApprovalBook::new();
        let (turn, query) = (TurnId::new(), QueryId::new());
        hub.begin(turn);
        let _answer = hub.await_review(turn, query, "DELETE FROM t".into(), SessionId::new(), false);
        review(&hub, &approvals, turn, query, true).unwrap();
        assert!(review(&hub, &approvals, turn, query, true).is_err());
        assert_eq!(approvals.outstanding(), 1);
    }
}
