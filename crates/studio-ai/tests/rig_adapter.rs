//! Drives the real rig path against a local server speaking the OpenAI streaming format.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use futures::StreamExt;
use secrecy::SecretString;
use std::sync::{Arc, Mutex};
use studio_ai::providers::rig_adapter::build;
use studio_ai::{AiError, ChatChunk, ChatMessage, ChatRequest, ToolCall};
use studio_types::AiProviderKind;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

/// Serves one canned response and captures the request.
async fn server(status: &'static str, body: String) -> (String, Arc<Mutex<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let seen = Arc::new(Mutex::new(String::new()));
    let captured = seen.clone();
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buffer = vec![0u8; 65536];
        let mut request = Vec::new();
        loop {
            let read = socket.read(&mut buffer).await.unwrap();
            request.extend_from_slice(&buffer[..read]);
            let text = String::from_utf8_lossy(&request).into_owned();
            if let Some((head, rest)) = text.split_once("\r\n\r\n") {
                let length = head
                    .lines()
                    .find_map(|l| {
                        l.to_lowercase()
                            .strip_prefix("content-length:")
                            .map(|v| v.trim().to_owned())
                    })
                    .and_then(|v| v.parse::<usize>().ok())
                    .unwrap_or(0);
                if rest.len() >= length {
                    *captured.lock().unwrap() = text.clone();
                    break;
                }
            }
            if read == 0 {
                break;
            }
        }
        let response = format!(
            "HTTP/1.1 {status}\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        socket.write_all(response.as_bytes()).await.unwrap();
        socket.shutdown().await.unwrap();
    });
    (format!("http://{address}/v1"), seen)
}

fn sse(events: &[&str]) -> String {
    let mut body: String = events.iter().map(|e| format!("data: {e}\n\n")).collect();
    body.push_str("data: [DONE]\n\n");
    body
}

async fn collect(endpoint: &str, request: ChatRequest) -> Vec<Result<ChatChunk, AiError>> {
    let key = Some(SecretString::from("test-key".to_owned()));
    let provider = build(AiProviderKind::OpenAi, endpoint, "test-model", key);
    match provider.stream(request, CancellationToken::new()).await {
        Ok(stream) => stream.collect().await,
        Err(error) => vec![Err(error)],
    }
}

fn hello() -> ChatRequest {
    ChatRequest {
        messages: vec![ChatMessage::System("sys".into()), ChatMessage::User("hi".into())],
        tools: studio_ai::tools::specs(),
    }
}

fn delta(content: &str) -> String {
    format!(
        r#"{{"id":"1","object":"chat.completion.chunk","created":1,"model":"m","choices":[{{"index":0,"delta":{content},"finish_reason":null}}]}}"#
    )
}

fn finish(reason: &str) -> String {
    format!(
        r#"{{"id":"1","object":"chat.completion.chunk","created":1,"model":"m","choices":[{{"index":0,"delta":{{}},"finish_reason":"{reason}"}}]}}"#
    )
}

#[tokio::test]
async fn text_streams_in_pieces() {
    let (a, b, c) = (
        delta(r#"{"content":"Hel"}"#),
        delta(r#"{"content":"lo"}"#),
        finish("stop"),
    );
    let (endpoint, _) = server("200 OK", sse(&[&a, &b, &c])).await;
    let chunks = collect(&endpoint, hello()).await;
    let text: String = chunks
        .iter()
        .filter_map(|c| match c {
            Ok(ChatChunk::Text(t)) => Some(t.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(text, "Hello", "{chunks:?}");
}

#[tokio::test]
async fn tool_call_fragments_are_reassembled() {
    let a = delta(
        r#"{"tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"run_sql","arguments":"{\"sq"}}]}"#,
    );
    let b = delta(r#"{"tool_calls":[{"index":0,"function":{"arguments":"l\":\"SELECT 1\"}"}}]}"#);
    let c = finish("tool_calls");
    let (endpoint, _) = server("200 OK", sse(&[&a, &b, &c])).await;
    let chunks = collect(&endpoint, hello()).await;
    let call = chunks.iter().find_map(|c| match c {
        Ok(ChatChunk::ToolCall { id, name, args_json }) => {
            Some((id.clone(), name.clone(), args_json.clone()))
        }
        _ => None,
    });
    let (id, name, args) = call.unwrap_or_else(|| panic!("no tool call in {chunks:?}"));
    assert_eq!(id, "call_1");
    assert_eq!(name, "run_sql");
    let parsed: serde_json::Value = serde_json::from_str(&args).unwrap();
    assert_eq!(parsed["sql"], "SELECT 1");
}

#[tokio::test]
async fn the_request_carries_history_tools_and_the_key() {
    let (endpoint, seen) = server("200 OK", sse(&[&finish("stop")])).await;
    let request = ChatRequest {
        messages: vec![
            ChatMessage::System("be brief".into()),
            ChatMessage::User("first".into()),
            ChatMessage::Assistant {
                text: "looking".into(),
                calls: vec![ToolCall {
                    id: "c9".into(),
                    name: "run_sql".into(),
                    args_json: r#"{"sql":"SELECT 1"}"#.into(),
                }],
            },
            ChatMessage::ToolResult {
                id: "c9".into(),
                name: "run_sql".into(),
                content: "Rows: 1".into(),
            },
        ],
        tools: studio_ai::tools::specs(),
    };
    collect(&endpoint, request).await;
    let raw = seen.lock().unwrap().clone();
    let lower = raw.to_lowercase();
    assert!(lower.contains("authorization: bearer test-key"), "{raw}");
    let body: serde_json::Value = serde_json::from_str(raw.split_once("\r\n\r\n").unwrap().1).unwrap();
    assert_eq!(body["model"], "test-model");
    assert_eq!(body["tools"].as_array().unwrap().len(), 4);
    let text = body["messages"].to_string();
    for needle in ["be brief", "first", "looking", "c9", "Rows: 1"] {
        assert!(text.contains(needle), "missing {needle} in {text}");
    }
}

#[tokio::test]
async fn a_refused_port_is_unreachable() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let chunks = collect(&format!("http://{address}/v1"), hello()).await;
    assert!(
        chunks.iter().any(|c| matches!(c, Err(AiError::Unreachable(_)))),
        "the offline fallback depends on this: {chunks:?}"
    );
}

#[tokio::test]
async fn a_tools_rejection_is_recognised() {
    let body = r#"{"error":{"message":"registry.ollama.ai/library/x does not support tools"}}"#.to_owned();
    let (endpoint, _) = server("400 Bad Request", body).await;
    let chunks = collect(&endpoint, hello()).await;
    assert!(
        chunks.iter().any(|c| matches!(c, Err(AiError::ToolsUnsupported))),
        "the no-tools fallback depends on this: {chunks:?}"
    );
}
