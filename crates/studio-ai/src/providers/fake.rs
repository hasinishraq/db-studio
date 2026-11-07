//! A scripted model for tests.

use crate::provider::{AiError, ChatChunk, ChatRequest, Provider};
use futures::stream::{self, BoxStream, StreamExt};
use std::collections::VecDeque;
use std::sync::Mutex;
use tokio_util::sync::CancellationToken;

/// One scripted reply.
pub enum Reply {
    /// Stream these chunks.
    Chunks(Vec<ChatChunk>),
    /// Fail before streaming.
    Fail(AiError),
}

/// Replays replies in order and records every request it receives.
pub struct FakeProvider {
    script: Mutex<VecDeque<Reply>>,
    requests: Mutex<Vec<ChatRequest>>,
    tools: bool,
}

impl FakeProvider {
    /// A provider that replays `script`, then ends every further reply at once.
    pub fn new(script: Vec<Reply>) -> Self {
        Self {
            script: Mutex::new(script.into()),
            requests: Mutex::new(Vec::new()),
            tools: true,
        }
    }

    /// Stops offering tools.
    pub fn without_tools(mut self) -> Self {
        self.tools = false;
        self
    }

    /// Everything the model was sent.
    pub fn requests(&self) -> Vec<ChatRequest> {
        self.requests.lock().map(|r| r.clone()).unwrap_or_default()
    }

    /// All text sent to the model, for leak checks.
    pub fn everything_sent(&self) -> String {
        format!("{:?}", self.requests())
    }
}

#[async_trait::async_trait]
impl Provider for FakeProvider {
    fn supports_tools(&self) -> bool {
        self.tools
    }

    async fn stream(
        &self,
        req: ChatRequest,
        _cancel: CancellationToken,
    ) -> Result<BoxStream<'static, Result<ChatChunk, AiError>>, AiError> {
        if let Ok(mut requests) = self.requests.lock() {
            requests.push(req);
        }
        let next = self.script.lock().ok().and_then(|mut s| s.pop_front());
        match next {
            Some(Reply::Fail(error)) => Err(error),
            Some(Reply::Chunks(chunks)) => Ok(stream::iter(chunks.into_iter().map(Ok)).boxed()),
            None => Ok(stream::iter(vec![Ok(ChatChunk::Done)]).boxed()),
        }
    }
}
