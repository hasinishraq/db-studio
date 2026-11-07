//! Our own small model interface, so the agent does not depend on any provider library.

use futures::stream::BoxStream;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

/// What can go wrong in a turn.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AiError {
    /// The model rejected tool calling.
    #[error("this model does not support tools")]
    ToolsUnsupported,
    /// The model could not be reached.
    #[error("could not reach the model: {0}")]
    Unreachable(String),
    /// The provider failed.
    #[error("the model failed: {0}")]
    Provider(String),
    /// The profile's policy forbids this.
    #[error("{0}")]
    Policy(String),
    /// The turn was cancelled.
    #[error("the turn was cancelled")]
    Cancelled,
    /// Memory could not be read or written.
    #[error("memory error: {0}")]
    Memory(String),
}

/// A tool call from the model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolCall {
    /// Provider's id for the call.
    pub id: String,
    /// Tool name.
    pub name: String,
    /// Arguments as JSON text.
    pub args_json: String,
}

/// One message of the conversation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChatMessage {
    /// Instructions.
    System(String),
    /// The user's words.
    User(String),
    /// The model's reply.
    Assistant {
        /// Text.
        text: String,
        /// Tool calls.
        calls: Vec<ToolCall>,
    },
    /// A tool's output.
    ToolResult {
        /// The call answered.
        id: String,
        /// Tool name.
        name: String,
        /// Output.
        content: String,
    },
}

/// A tool the model may call.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolSpec {
    /// Name.
    pub name: String,
    /// What it does.
    pub description: String,
    /// JSON schema of its arguments.
    pub parameters: Value,
}

/// One request to the model.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChatRequest {
    /// The conversation so far.
    pub messages: Vec<ChatMessage>,
    /// Tools on offer; empty in fenced-SQL mode.
    pub tools: Vec<ToolSpec>,
}

/// One piece of a streamed reply.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChatChunk {
    /// Text.
    Text(String),
    /// A complete tool call.
    ToolCall {
        /// Provider's id.
        id: String,
        /// Tool name.
        name: String,
        /// Arguments as JSON text.
        args_json: String,
    },
    /// The reply ended.
    Done,
}

/// A model.
#[async_trait::async_trait]
pub trait Provider: Send + Sync {
    /// Whether to offer tools at all.
    fn supports_tools(&self) -> bool;

    /// Streams a reply.
    async fn stream(
        &self,
        req: ChatRequest,
        cancel: CancellationToken,
    ) -> Result<BoxStream<'static, Result<ChatChunk, AiError>>, AiError>;
}
