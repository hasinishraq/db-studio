//! The step loop and the review gate.

use crate::describe::describe;
use crate::hub::{AgentHub, Decision};
use crate::limits::{Limits, MAX_STEPS};
use crate::mask::Masks;
use crate::memory;
use crate::offline::{KnownTable, OfflineProvider};
use crate::prompt::{build_system_prompt, PromptContext};
use crate::provider::{AiError, ChatChunk, ChatMessage, ChatRequest, Provider, ToolCall};
use crate::tools::{self, fenced_sql, SchemaArgs, SqlArgs, TableArgs};
use dashmap::DashSet;
use futures::StreamExt;
use std::sync::Arc;
use studio_safety::{ai_auto_allowed, classify_script, Risk};
use studio_session::{AiQueryResult, SessionError, SessionManager};
use studio_store::Store;
use studio_types::{
    AiEvent, AiPolicy, AiSendRequest, ApprovalToken, ConnectionProfile, Engine as DbEngine, Environment,
    QueryId, QueryRecord, SchemaRef, SessionId, TableRef, TurnId,
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// How a turn reaches its model.
pub struct TurnOptions {
    /// The model.
    pub provider: Arc<dyn Provider>,
    /// Identifies the model, so a no-tools discovery survives between turns.
    pub model_key: String,
    /// The endpoint, checked against the profile's policy.
    pub endpoint: String,
}

/// Whether an endpoint is on this machine. The host is parsed, so `http://localhost@evil.example`
/// is not local.
pub fn is_local_endpoint(endpoint: &str) -> bool {
    let rest = endpoint.split_once("://").map_or(endpoint, |(_, rest)| rest);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host_port = authority.rsplit('@').next().unwrap_or_default();
    let host = if let Some(end) = host_port.strip_prefix('[').and_then(|h| h.find(']')) {
        &host_port[1..=end]
    } else {
        host_port.split(':').next().unwrap_or_default()
    };
    matches!(
        host.to_ascii_lowercase().as_str(),
        "localhost" | "127.0.0.1" | "::1"
    )
}

/// Whether rows may reach the model, given the user's choice and the profile. Production profiles
/// default to no sharing.
pub fn effective_sharing(requested: bool, profile: Option<&ConnectionProfile>) -> bool {
    match profile {
        Some(p) if p.ai_policy == AiPolicy::NoRowSharing => false,
        Some(p) if p.environment == Environment::Production && p.ai_policy == AiPolicy::Default => false,
        _ => requested,
    }
}

fn check_policy(profile: Option<&ConnectionProfile>, endpoint: &str) -> Result<(), AiError> {
    let Some(profile) = profile else { return Ok(()) };
    match profile.ai_policy {
        AiPolicy::Disabled => Err(AiError::Policy(
            "The assistant is disabled for this connection.".to_owned(),
        )),
        AiPolicy::LocalOnly if !is_local_endpoint(endpoint) => Err(AiError::Policy(
            "This connection only allows a local model, but the endpoint is not local.".to_owned(),
        )),
        _ => Ok(()),
    }
}

/// State of one running turn.
struct Turn {
    id: TurnId,
    session: Option<SessionId>,
    dialect: Option<DbEngine>,
    auto_run: bool,
    share_rows: bool,
    masks: Masks,
    cancel: CancellationToken,
    events: mpsc::Sender<AiEvent>,
    limits: Limits,
    queries: Vec<QueryRecord>,
    /// Every value exposed to the model, so memory can scrub it.
    exposed: Vec<String>,
    /// Whether anything reached the user yet; fallbacks are only safe before that.
    streamed: bool,
}

impl Turn {
    async fn emit(&self, event: AiEvent) {
        let _ = self.events.send(event).await;
    }

    fn record(&mut self, sql: &str, outcome: &str, row_count: Option<i64>) {
        self.queries.push(QueryRecord {
            sql: sql.to_owned(),
            outcome: outcome.to_owned(),
            row_count,
        });
    }
}

fn risk_reasons(sql: &str, dialect: DbEngine) -> Vec<String> {
    classify_script(sql, dialect)
        .into_iter()
        .flat_map(|c| match c.verdict.risk {
            Risk::Normal => Vec::new(),
            Risk::High { reasons } => reasons,
        })
        .collect()
}

/// Runs AI turns.
pub struct Engine {
    sessions: Arc<SessionManager>,
    store: Store,
    hub: Arc<AgentHub>,
    no_tools: DashSet<String>,
}

impl Engine {
    /// Creates the engine.
    pub fn new(sessions: Arc<SessionManager>, store: Store, hub: Arc<AgentHub>) -> Self {
        Self {
            sessions,
            store,
            hub,
            no_tools: DashSet::new(),
        }
    }

    /// The hub, for review decisions.
    pub fn hub(&self) -> &Arc<AgentHub> {
        &self.hub
    }

    fn profile(&self, session: Option<SessionId>) -> Option<ConnectionProfile> {
        session
            .and_then(|id| self.sessions.get(id).ok())
            .map(|open| open.profile.clone())
    }

    async fn schema_text(&self, session: SessionId) -> (String, Vec<KnownTable>) {
        let mut text = String::new();
        let mut tables: Vec<KnownTable> = Vec::new();
        let Ok(schemas) = self.sessions.list_schemas(session).await else {
            return (text, tables);
        };
        for schema in schemas {
            let Ok(columns) = self.sessions.columns(session, &schema).await else {
                continue;
            };
            for column in columns {
                if tables
                    .last()
                    .is_none_or(|t| t.name != column.table || t.schema != schema.name)
                {
                    if !text.is_empty() {
                        text.push_str(")\n");
                    }
                    text.push_str(&format!("{}.{}(", schema.name, column.table));
                    tables.push(KnownTable {
                        schema: schema.name.clone(),
                        name: column.table.clone(),
                        columns: Vec::new(),
                    });
                } else {
                    text.push_str(", ");
                }
                text.push_str(&format!("{} {}", column.name, column.data_type));
                if let Some(table) = tables.last_mut() {
                    table.columns.push(column.name);
                }
            }
        }
        if !text.is_empty() {
            text.push_str(")\n");
        }
        if let Some(database) = self.sessions.current_database(session).await {
            text.insert_str(0, &format!("Current database: {database}\n"));
        }
        (text, tables)
    }

    /// Runs a turn to its end. Always finishes with [`AiEvent::Done`].
    pub async fn run_turn(&self, req: AiSendRequest, options: TurnOptions, events: mpsc::Sender<AiEvent>) {
        let turn_id = req.turn_id;
        let cancel = self.hub.begin(turn_id);
        let result = self.drive(req, options, cancel, events.clone()).await;
        self.hub.finish(turn_id);
        let error = match result {
            Ok(()) | Err(AiError::Cancelled) => None,
            Err(error) => Some(error.to_string()),
        };
        let _ = events.send(AiEvent::Done { error }).await;
    }

    async fn drive(
        &self,
        req: AiSendRequest,
        options: TurnOptions,
        cancel: CancellationToken,
        events: mpsc::Sender<AiEvent>,
    ) -> Result<(), AiError> {
        let profile = self.profile(req.session_id);
        check_policy(profile.as_ref(), &options.endpoint)?;
        let share_rows = effective_sharing(req.share_rows, profile.as_ref());
        let masks = match &profile {
            Some(p) => Masks::new(&self.store.list_masking_rules(p.id).await.unwrap_or_default()),
            None => Masks::default(),
        };
        let (schema, known) = match req.session_id {
            Some(id) if profile.is_some() => self.schema_text(id).await,
            _ => (String::new(), Vec::new()),
        };

        let title: String = req.text.chars().take(60).collect();
        self.store
            .ensure_conversation(req.conversation_id, profile.as_ref().map(|p| p.id), &title)
            .await
            .map_err(|e| AiError::Memory(e.to_string()))?;
        let (history, summary) = memory::load(&self.store, req.conversation_id).await?;

        let mut turn = Turn {
            id: req.turn_id,
            session: req.session_id.filter(|_| profile.is_some()),
            dialect: profile.as_ref().map(|p| p.engine),
            auto_run: req.auto_run,
            share_rows,
            masks,
            cancel,
            events,
            limits: Limits::default(),
            queries: Vec::new(),
            exposed: Vec::new(),
            streamed: false,
        };

        let mut use_tools = options.provider.supports_tools() && !self.no_tools.contains(&options.model_key);
        let prompt = |tools: bool| {
            build_system_prompt(&PromptContext {
                connection: profile.as_ref().map(|p| p.name.as_str()),
                dialect: profile.as_ref().map(|p| p.engine),
                schema: &schema,
                summary: summary.as_deref(),
                share_rows,
                tools,
            })
        };
        let mut messages = vec![ChatMessage::System(prompt(use_tools))];
        messages.extend(history);
        messages.push(ChatMessage::User(req.text.clone()));

        let mut provider = options.provider.clone();
        let mut offline = false;
        let mut assistant_text = String::new();
        let mut steps = 0;
        let outcome = loop {
            if steps >= MAX_STEPS {
                let note = "\nI stopped here: the limit of steps for one answer was reached.";
                assistant_text.push_str(note);
                turn.emit(AiEvent::Text {
                    text: note.to_owned(),
                })
                .await;
                break Ok(());
            }
            let specs = if use_tools { tools::specs() } else { Vec::new() };
            let request = ChatRequest {
                messages: messages.clone(),
                tools: specs,
            };
            let (text, calls) = match self.one_step(&provider, request, &mut turn).await {
                Ok(step) => step,
                Err(AiError::ToolsUnsupported) if use_tools && !turn.streamed => {
                    self.no_tools.insert(options.model_key.clone());
                    use_tools = false;
                    messages[0] = ChatMessage::System(prompt(false));
                    continue;
                }
                Err(AiError::Unreachable(_)) if !offline && !turn.streamed => {
                    provider = Arc::new(OfflineProvider::new(known.clone(), turn.dialect));
                    offline = true;
                    use_tools = true;
                    continue;
                }
                Err(error) => break Err(error),
            };
            steps += 1;
            assistant_text.push_str(&text);

            let calls = if use_tools { calls } else { fenced_calls(&text) };
            if calls.is_empty() {
                break Ok(());
            }
            let assistant_calls = if use_tools { calls.clone() } else { Vec::new() };
            messages.push(ChatMessage::Assistant {
                text,
                calls: assistant_calls,
            });
            for call in calls {
                if turn.cancel.is_cancelled() {
                    break;
                }
                let output = self.handle_call(&mut turn, &call).await;
                messages.push(if use_tools {
                    ChatMessage::ToolResult {
                        id: call.id,
                        name: call.name,
                        content: output,
                    }
                } else {
                    ChatMessage::User(format!("Result of the statement:\n{output}"))
                });
            }
            if turn.cancel.is_cancelled() {
                break Err(AiError::Cancelled);
            }
        };

        // A cancelled turn is still remembered, without anything it never finished saying.
        let stored = memory::scrub(&assistant_text, &turn.exposed);
        memory::save_turn(
            &self.store,
            req.conversation_id,
            turn.id,
            &req.text,
            &stored,
            &turn.queries,
        )
        .await?;
        if outcome.is_ok() {
            memory::compact_if_needed(&self.store, &options.provider, req.conversation_id).await?;
        }
        outcome
    }

    async fn one_step(
        &self,
        provider: &Arc<dyn Provider>,
        request: ChatRequest,
        turn: &mut Turn,
    ) -> Result<(String, Vec<ToolCall>), AiError> {
        let mut stream = tokio::select! {
            stream = provider.stream(request, turn.cancel.clone()) => stream?,
            () = turn.cancel.cancelled() => return Err(AiError::Cancelled),
        };
        let (mut text, mut calls) = (String::new(), Vec::new());
        loop {
            let chunk = tokio::select! {
                chunk = stream.next() => chunk,
                () = turn.cancel.cancelled() => return Err(AiError::Cancelled),
            };
            match chunk {
                None | Some(Ok(ChatChunk::Done)) => break,
                Some(Ok(ChatChunk::Text(piece))) => {
                    turn.streamed = true;
                    text.push_str(&piece);
                    turn.emit(AiEvent::Text { text: piece }).await;
                }
                Some(Ok(ChatChunk::ToolCall { id, name, args_json })) => {
                    calls.push(ToolCall { id, name, args_json });
                }
                Some(Err(error)) => return Err(error),
            }
        }
        Ok((text, calls))
    }

    async fn handle_call(&self, turn: &mut Turn, call: &ToolCall) -> String {
        let parsed_sql = || serde_json::from_str::<SqlArgs>(&call.args_json);
        match call.name.as_str() {
            tools::RUN_SQL => match parsed_sql() {
                Ok(args) => self.handle_run_sql(turn, args.sql).await,
                Err(_) => "The arguments were invalid. Pass {\"sql\": \"...\"}.".to_owned(),
            },
            tools::EXPLAIN_QUERY => match parsed_sql() {
                Ok(args) => self.handle_explain(turn, &args.sql).await,
                Err(_) => "The arguments were invalid. Pass {\"sql\": \"...\"}.".to_owned(),
            },
            tools::DESCRIBE_TABLE => match serde_json::from_str::<TableArgs>(&call.args_json) {
                Ok(args) => self.handle_describe(turn, args).await,
                Err(_) => "The arguments were invalid. Pass {\"table\": \"...\"}.".to_owned(),
            },
            tools::LIST_TABLES => {
                self.handle_list(turn, serde_json::from_str(&call.args_json).unwrap_or_default())
                    .await
            }
            other => format!("There is no tool named {other}."),
        }
    }

    async fn handle_explain(&self, turn: &Turn, sql: &str) -> String {
        let Some(session) = turn.session else {
            return no_connection();
        };
        match self.sessions.explain(session, sql).await {
            Ok(plan) => plan,
            Err(error) => format!("EXPLAIN failed: {error}"),
        }
    }

    async fn handle_describe(&self, turn: &Turn, args: TableArgs) -> String {
        let Some(session) = turn.session else {
            return no_connection();
        };
        let schema = match args.schema {
            Some(schema) => schema,
            None => match self.sessions.list_schemas(session).await {
                Ok(schemas) => {
                    let chosen = self.sessions.current_database(session).await;
                    let chosen = schemas.iter().find(|s| Some(&s.name) == chosen.as_ref());
                    chosen
                        .or(schemas.first())
                        .map(|s| s.name.clone())
                        .unwrap_or_default()
                }
                Err(error) => return format!("Could not read the schema: {error}"),
            },
        };
        let table = TableRef {
            schema,
            name: args.table,
        };
        match self.sessions.table_structure(session, &table).await {
            Ok(structure) => {
                let columns: Vec<String> = structure
                    .columns
                    .iter()
                    .map(|c| {
                        let key = if c.primary_key { " PRIMARY KEY" } else { "" };
                        let null = if c.nullable { "" } else { " NOT NULL" };
                        format!("{} {}{null}{key}", c.name, c.data_type)
                    })
                    .collect();
                let indexes: Vec<String> = structure
                    .indexes
                    .iter()
                    .map(|i| format!("{} ({})", i.name, i.columns.join(", ")))
                    .collect();
                format!("Columns: {}\nIndexes: {}", columns.join("; "), indexes.join("; "))
            }
            Err(error) => format!("Could not describe the table: {error}"),
        }
    }

    async fn handle_list(&self, turn: &Turn, args: SchemaArgs) -> String {
        let Some(session) = turn.session else {
            return no_connection();
        };
        let schemas = match args.schema {
            Some(name) => vec![SchemaRef { name }],
            None => match self.sessions.list_schemas(session).await {
                Ok(schemas) => schemas,
                Err(error) => return format!("Could not list schemas: {error}"),
            },
        };
        let mut names = Vec::new();
        for schema in schemas {
            match self.sessions.list_objects(session, &schema).await {
                Ok(objects) => names.extend(objects.into_iter().map(|o| format!("{}.{}", o.schema, o.name))),
                Err(error) => return format!("Could not list tables: {error}"),
            }
        }
        format!("Tables: {}", names.join(", "))
    }

    /// The gate. Nothing the model sends can skip it: the only statements that run unattended are
    /// those `studio-safety` allows, and anything else waits on a oneshot only a click can complete.
    async fn handle_run_sql(&self, turn: &mut Turn, sql: String) -> String {
        let (Some(session), Some(dialect)) = (turn.session, turn.dialect) else {
            return no_connection();
        };
        if let Err(message) = turn.limits.admit(&sql) {
            return message;
        }
        let id = QueryId::new();
        let auto = ai_auto_allowed(&sql, dialect);
        let needs_review = auto.is_err() || !turn.auto_run;
        turn.emit(AiEvent::QueryStart {
            id,
            sql: sql.clone(),
            read_only: auto.is_ok(),
            reason: auto.as_ref().err().cloned(),
            needs_review,
            risk_reasons: risk_reasons(&sql, dialect),
        })
        .await;

        let mut token: Option<ApprovalToken> = None;
        if needs_review {
            match self.wait_for_review(turn, id, &sql, session, auto.is_ok()).await {
                Some(approved) => token = approved,
                None => {
                    turn.emit(AiEvent::QueryDeclined { id }).await;
                    turn.record(&sql, "declined", None);
                    return "The user declined this statement. It did not run. Do not submit it again."
                        .to_owned();
                }
            }
            turn.emit(AiEvent::QueryApproved { id }).await;
        }

        let result = match token {
            Some(token) => self.sessions.run_ai_approved(session, &sql, id, &token).await,
            None => self.sessions.run_ai_read(session, &sql, id).await,
        };
        self.finish_query(turn, id, &sql, result).await
    }

    /// `Some` when approved, carrying a token unless the statement is an unattended read.
    async fn wait_for_review(
        &self,
        turn: &Turn,
        id: QueryId,
        sql: &str,
        session: SessionId,
        auto_read: bool,
    ) -> Option<Option<ApprovalToken>> {
        let answer = self
            .hub
            .await_review(turn.id, id, sql.to_owned(), session, auto_read)?;
        let decision = tokio::select! {
            decision = answer => decision.unwrap_or(Decision::Declined),
            () = turn.cancel.cancelled() => Decision::Declined,
        };
        match decision {
            Decision::Approved(token) if token.is_some() || auto_read => Some(token),
            _ => None,
        }
    }

    async fn finish_query(
        &self,
        turn: &mut Turn,
        id: QueryId,
        sql: &str,
        result: Result<AiQueryResult, SessionError>,
    ) -> String {
        match result {
            Ok(done) => {
                let (text, exposed) = describe(&done, turn.share_rows, &turn.masks);
                turn.exposed.extend(exposed);
                let count = done.rows_affected.unwrap_or(done.rows.len() as u64);
                turn.record(sql, "ok", i64::try_from(count).ok());
                turn.emit(AiEvent::QueryResult {
                    id,
                    columns: done.columns,
                    rows: done.rows,
                    rows_affected: done.rows_affected,
                    truncated: done.truncated,
                    elapsed_ms: done.elapsed_ms,
                })
                .await;
                text
            }
            Err(SessionError::Db(studio_db::DbError::Cancelled)) => {
                turn.emit(AiEvent::QueryCancelled { id }).await;
                turn.record(sql, "cancelled", None);
                "The user cancelled this statement.".to_owned()
            }
            Err(error) => {
                let message = error.to_string();
                turn.emit(AiEvent::QueryError {
                    id,
                    message: message.clone(),
                })
                .await;
                turn.record(sql, "error", None);
                format!("The statement failed: {message}")
            }
        }
    }
}

fn no_connection() -> String {
    "No database is connected, so this cannot run.".to_owned()
}

fn fenced_calls(text: &str) -> Vec<ToolCall> {
    fenced_sql(text)
        .into_iter()
        .enumerate()
        .map(|(index, sql)| ToolCall {
            id: format!("fenced-{index}"),
            name: tools::RUN_SQL.to_owned(),
            args_json: serde_json::json!({ "sql": sql }).to_string(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn local_endpoints_are_recognised_by_host() {
        for local in [
            "http://localhost:11434",
            "http://127.0.0.1:1234/v1",
            "https://LOCALHOST",
            "http://[::1]:8080",
            "localhost:11434",
        ] {
            assert!(is_local_endpoint(local), "{local}");
        }
        for remote in [
            "https://api.openai.com/v1",
            "http://localhost@evil.example",
            "http://localhost.evil.example",
            "http://127.0.0.1.evil.example",
            "http://evil.example/localhost",
        ] {
            assert!(!is_local_endpoint(remote), "{remote}");
        }
    }
}
