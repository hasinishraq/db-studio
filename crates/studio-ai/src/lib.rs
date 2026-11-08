//! The AI assistant: answers questions, runs safe reads by itself, and pauses every other statement
//! for a human. It reaches databases only through `studio-session`.

pub mod agent;
pub mod chart;
pub mod describe;
pub mod hub;
pub mod limits;
pub mod mask;
pub mod memory;
pub mod offline;
pub mod prompt;
pub mod provider;
pub mod providers;
pub mod tools;

pub use agent::{effective_sharing, is_local_endpoint, Engine, TurnOptions};
pub use hub::{AgentHub, Decision, PendingReview};
pub use provider::{AiError, ChatChunk, ChatMessage, ChatRequest, Provider, ToolCall, ToolSpec};
