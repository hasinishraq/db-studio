//! Answers simple questions when no model is reachable, through the same loop and the same gates.

use crate::provider::{AiError, ChatChunk, ChatMessage, ChatRequest, Provider};
use crate::tools::{LIST_TABLES, RUN_SQL};
use futures::stream::{self, BoxStream, StreamExt};
use serde_json::json;
use studio_db::sql::quote_ident;
use studio_types::Engine;
use tokio_util::sync::CancellationToken;

/// What the engine knows about the schema.
#[derive(Clone, Debug, Default)]
pub struct KnownTable {
    /// Schema.
    pub schema: String,
    /// Table.
    pub name: String,
    /// Columns.
    pub columns: Vec<String>,
}

/// A stand-in model driven by keywords.
pub struct OfflineProvider {
    tables: Vec<KnownTable>,
    engine: Option<Engine>,
}

fn words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|w| !w.is_empty())
        .map(str::to_owned)
        .collect()
}

fn mentions(question: &[String], name: &str) -> bool {
    let name = name.to_lowercase();
    question
        .iter()
        .any(|w| *w == name || w.trim_end_matches('s') == name.trim_end_matches('s'))
}

impl OfflineProvider {
    /// Creates the stand-in.
    pub fn new(tables: Vec<KnownTable>, engine: Option<Engine>) -> Self {
        Self { tables, engine }
    }

    fn table_named(&self, question: &[String]) -> Option<&KnownTable> {
        self.tables.iter().find(|t| mentions(question, &t.name))
    }

    fn target(&self, table: &KnownTable) -> Option<String> {
        let engine = self.engine?;
        Some(studio_db::sql::qualified(engine, &table.schema, &table.name))
    }

    /// The statement that answers a question, if one does.
    pub fn plan(&self, question: &str) -> Option<String> {
        let question = words(question);
        let table = self.table_named(&question)?;
        let target = self.target(table)?;
        let engine = self.engine?;
        let has = |word: &str| question.iter().any(|w| w == word);
        if has("count") || has("many") || has("number") || has("total") {
            return Some(format!("SELECT COUNT(*) FROM {target}"));
        }
        let picked: Vec<String> = table
            .columns
            .iter()
            .filter(|c| mentions(&question, c))
            .map(|c| quote_ident(engine, c))
            .collect();
        let select = if picked.is_empty() {
            "*".to_owned()
        } else {
            picked.join(", ")
        };
        if has("latest") || has("newest") || has("recent") || has("last") {
            let first = table.columns.first()?;
            let order = quote_ident(engine, first);
            return Some(format!(
                "SELECT {select} FROM {target} ORDER BY {order} DESC LIMIT 10"
            ));
        }
        if has("top") || has("first") {
            let limit = question
                .iter()
                .find_map(|w| w.parse::<u32>().ok())
                .unwrap_or(10)
                .min(1000);
            return Some(format!("SELECT {select} FROM {target} LIMIT {limit}"));
        }
        Some(format!("SELECT {select} FROM {target} LIMIT 100"))
    }

    fn wants_tables(question: &str) -> bool {
        let question = words(question);
        let has = |word: &str| question.iter().any(|w| w == word);
        (has("tables") || has("table")) && (has("list") || has("show") || has("what") || has("which"))
    }
}

fn last_user(req: &ChatRequest) -> &str {
    req.messages
        .iter()
        .rev()
        .find_map(|m| match m {
            ChatMessage::User(text) => Some(text.as_str()),
            _ => None,
        })
        .unwrap_or_default()
}

fn already_answered(req: &ChatRequest) -> bool {
    matches!(req.messages.last(), Some(ChatMessage::ToolResult { .. }))
}

#[async_trait::async_trait]
impl Provider for OfflineProvider {
    fn supports_tools(&self) -> bool {
        true
    }

    async fn stream(
        &self,
        req: ChatRequest,
        _cancel: CancellationToken,
    ) -> Result<BoxStream<'static, Result<ChatChunk, AiError>>, AiError> {
        let question = last_user(&req);
        let chunks = if already_answered(&req) {
            vec![
                ChatChunk::Text("Done. The results are shown above.".to_owned()),
                ChatChunk::Done,
            ]
        } else if Self::wants_tables(question) {
            let call = ChatChunk::ToolCall {
                id: "offline".to_owned(),
                name: LIST_TABLES.to_owned(),
                args_json: "{}".to_owned(),
            };
            vec![
                ChatChunk::Text("The model is unreachable, so here is a simple answer.\n".to_owned()),
                call,
            ]
        } else if let Some(sql) = self.plan(question) {
            let call = ChatChunk::ToolCall {
                id: "offline".to_owned(),
                name: RUN_SQL.to_owned(),
                args_json: json!({ "sql": sql }).to_string(),
            };
            vec![
                ChatChunk::Text("The model is unreachable, so here is a simple answer.\n".to_owned()),
                call,
            ]
        } else {
            vec![
                ChatChunk::Text(
                    "The model is unreachable and this question needs it. Check the AI settings.".to_owned(),
                ),
                ChatChunk::Done,
            ]
        };
        Ok(stream::iter(chunks.into_iter().map(Ok)).boxed())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    fn offline() -> OfflineProvider {
        let table = KnownTable {
            schema: "public".into(),
            name: "users".into(),
            columns: vec!["id".into(), "email".into()],
        };
        OfflineProvider::new(vec![table], Some(Engine::Postgres))
    }

    #[test]
    fn plans_common_questions() {
        let o = offline();
        assert_eq!(
            o.plan("how many users are there").as_deref(),
            Some("SELECT COUNT(*) FROM \"public\".\"users\"")
        );
        assert_eq!(
            o.plan("show the latest users").as_deref(),
            Some("SELECT * FROM \"public\".\"users\" ORDER BY \"id\" DESC LIMIT 10")
        );
        assert_eq!(
            o.plan("top 5 users").as_deref(),
            Some("SELECT * FROM \"public\".\"users\" LIMIT 5")
        );
        assert_eq!(
            o.plan("show email of users").as_deref(),
            Some("SELECT \"email\" FROM \"public\".\"users\" LIMIT 100")
        );
        assert_eq!(o.plan("what is the meaning of life"), None);
    }
}
