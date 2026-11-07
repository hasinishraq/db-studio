//! Real models through `rig-core`. If rig's API changes, only this file moves.

use crate::provider::{AiError, ChatChunk, ChatMessage, ChatRequest, Provider};
use futures::stream::{BoxStream, StreamExt};
use rig_core::completion::message::{
    AssistantContent, AssistantMessage, CallId, Message, ToolCall as RigCall, ToolFunction, ToolName,
};
use rig_core::completion::{CompletionRequest, ToolDefinition};
use rig_core::error::ProviderError;
use rig_core::operation::Completion;
use rig_core::providers::ollama::OllamaConfig;
use rig_core::providers::openai::OpenAIConfig;
use rig_core::streaming::{Item, StreamEvent};
use rig_core::DynModel;
use secrecy::{ExposeSecret, SecretString};
use std::sync::Arc;
use studio_types::AiProviderKind;
use tokio_util::sync::CancellationToken;

/// A model behind rig.
pub struct RigProvider {
    model: DynModel<Completion>,
}

/// Builds the provider for the settings.
pub fn build(
    kind: AiProviderKind,
    endpoint: &str,
    model: &str,
    api_key: Option<SecretString>,
) -> Arc<dyn Provider> {
    let key = api_key.map(|k| k.expose_secret().to_owned());
    let model = match kind {
        AiProviderKind::Ollama => {
            let mut config = OllamaConfig::new().with_base_url(endpoint);
            if let Some(key) = key {
                config = config.with_api_key(key);
            }
            config.client().completion(model).erase()
        }
        AiProviderKind::OpenAi => {
            // Local servers such as LM Studio accept any key.
            let key = key.unwrap_or_else(|| "none".to_owned());
            OpenAIConfig::new(key)
                .with_base_url(endpoint)
                .client()
                .chat(model)
                .erase()
        }
    };
    Arc::new(RigProvider { model })
}

fn rig_name(name: &str) -> Result<ToolName, AiError> {
    ToolName::new(name).map_err(|e| AiError::Provider(e.to_string()))
}

fn to_rig(message: &ChatMessage) -> Result<Message, AiError> {
    Ok(match message {
        ChatMessage::System(text) => Message::System {
            content: text.clone(),
        },
        ChatMessage::User(text) => Message::user(text),
        ChatMessage::Assistant { text, calls } => {
            let mut content = Vec::with_capacity(calls.len() + 1);
            if !text.is_empty() {
                content.push(AssistantContent::text(text));
            }
            for call in calls {
                let arguments = serde_json::from_str(&call.args_json).unwrap_or(serde_json::Value::Null);
                let function = ToolFunction::new(rig_name(&call.name)?, arguments);
                content.push(AssistantContent::ToolCall(RigCall::new(
                    CallId::from_wire(&call.id),
                    function,
                )));
            }
            Message::Assistant(AssistantMessage::new(content))
        }
        ChatMessage::ToolResult { id, name, content } => {
            Message::tool_result(CallId::from_wire(id), rig_name(name)?, content)
        }
    })
}

fn to_request(req: &ChatRequest) -> Result<CompletionRequest, AiError> {
    let mut messages = req.messages.iter().map(to_rig).collect::<Result<Vec<_>, _>>()?;
    let first = messages.remove(0);
    let mut request = CompletionRequest::new(first);
    request.chat_history.extend(messages.into_iter().map(Arc::new));
    for tool in &req.tools {
        request.tools.push(Arc::new(ToolDefinition::new(
            rig_name(&tool.name)?,
            tool.description.clone(),
            tool.parameters.clone(),
        )));
    }
    Ok(request)
}

/// Sorts a failure into what the agent can act on.
fn classify(error: ProviderError) -> AiError {
    let text = error.to_string();
    let lower = text.to_lowercase();
    if lower.contains("does not support tools") || lower.contains("tools is not supported") {
        return AiError::ToolsUnsupported;
    }
    if matches!(error, ProviderError::Http(_)) {
        return AiError::Unreachable(text);
    }
    AiError::Provider(text)
}

fn to_chunk(item: Item<StreamEvent>) -> Option<Result<ChatChunk, AiError>> {
    match item {
        Item::Event(StreamEvent::Text { text, .. }) => Some(Ok(ChatChunk::Text(text))),
        Item::Event(StreamEvent::End {
            content: AssistantContent::ToolCall(call),
            ..
        }) => {
            let args_json = serde_json::Value::Object(call.function.arguments).to_string();
            Some(Ok(ChatChunk::ToolCall {
                id: call.id.to_string(),
                name: call.function.name.to_string(),
                args_json,
            }))
        }
        _ => None,
    }
}

#[async_trait::async_trait]
impl Provider for RigProvider {
    fn supports_tools(&self) -> bool {
        true
    }

    async fn stream(
        &self,
        req: ChatRequest,
        _cancel: CancellationToken,
    ) -> Result<BoxStream<'static, Result<ChatChunk, AiError>>, AiError> {
        let streamed = self.model.stream(to_request(&req)?).map_err(classify)?;
        let chunks = streamed.filter_map(|item| async move {
            match item {
                Ok(item) => to_chunk(item),
                Err(error) => Some(Err(classify(error))),
            }
        });
        Ok(chunks.boxed())
    }
}
