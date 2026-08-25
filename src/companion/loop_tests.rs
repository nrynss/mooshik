use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::Value;

use crate::config::{ApiKey, CompanionConfig};
use crate::text;

use super::cancel::Cancellation;
use super::client::CompanionClient;
use super::mock::{Frame, MockServer, Script};
use super::pack::{message_tokens, RecallInjector};
use super::session::Session;
use super::tools::{ToolExecutor, ToolSpec};
use super::types::{Message, Role};
use super::CompanionError;

struct Echo;

impl ToolExecutor for Echo {
    fn specs(&self) -> Vec<ToolSpec> {
        vec![ToolSpec {
            name: "echo".into(),
            description: "echo".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {"text": {"type": "string"}}
            }),
        }]
    }

    fn execute(&self, name: &str, arguments: &Value) -> String {
        assert_eq!(name, "echo");
        arguments
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned()
    }
}

struct Recording {
    dropped: Arc<Mutex<Vec<Vec<Message>>>>,
}

impl RecallInjector for Recording {
    fn inject(&self, dropped: &[Message], _current_user: &str) -> Option<Message> {
        self.dropped.lock().unwrap().push(dropped.to_vec());
        None
    }
}

fn config(base_url: &str) -> CompanionConfig {
    CompanionConfig {
        base_url: base_url.to_owned(),
        ..CompanionConfig::default()
    }
}

fn session(server: &MockServer, window: u32) -> Session {
    let client = CompanionClient::from_config(&config(&server.base_url)).unwrap();
    Session::new(client, window).with_system("s")
}

fn stop_script(parts: &[&str]) -> Script {
    let mut frames: Vec<Frame> = parts.iter().map(|part| Frame::content(part)).collect();
    frames.push(Frame::finish("stop"));
    frames.push(Frame::done());
    Script::sse(frames)
}

#[tokio::test]
async fn streams_content_tokens_in_order() {
    let server = MockServer::spawn(vec![stop_script(&["Hel", "lo"])]).await;
    let mut chat = session(&server, 32768);
    let mut tokens = Vec::new();
    let reply = chat
        .turn("hi", &Cancellation::new(), |token| {
            tokens.push(token.to_owned())
        })
        .await
        .unwrap();
    assert_eq!(tokens, ["Hel", "lo"]);
    assert_eq!(reply, "Hello");
    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert!(
        requests[0].path.ends_with("/v1/chat/completions"),
        "{}",
        requests[0].path
    );
    assert_eq!(requests[0].authorization, None);
}

#[tokio::test]
async fn content_then_tool_calls_assembled_from_split_chunks() {
    let first = Script::sse(vec![
        Frame::content("Let me "),
        Frame::content("check."),
        Frame::tool_head(0, "call_1", "echo"),
        Frame::tool_args(0, "{\"text\":"),
        Frame::tool_args(0, "\"hi\"}"),
        Frame::finish("tool_calls"),
        Frame::done(),
    ]);
    let server = MockServer::spawn(vec![first, stop_script(&["done-now"])]).await;
    let client = CompanionClient::from_config(&config(&server.base_url)).unwrap();
    let mut chat = Session::new(client, 32768)
        .with_system("s")
        .with_executor(Echo);
    let mut tokens = Vec::new();
    let reply = chat
        .turn("run", &Cancellation::new(), |token| {
            tokens.push(token.to_owned())
        })
        .await
        .unwrap();
    assert_eq!(tokens, ["Let me ", "check.", "done-now"]);
    assert_eq!(reply, "done-now");
    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    let second: Value = serde_json::from_str(&requests[1].body).unwrap();
    let messages = second["messages"].as_array().unwrap();
    let tool = messages
        .iter()
        .find(|msg| msg["role"] == "tool")
        .expect("tool result posted back");
    assert_eq!(tool["content"], "hi");
    assert_eq!(tool["tool_call_id"], "call_1");
    let history_roles: Vec<_> = chat.history().iter().map(|m| m.role).collect();
    assert!(history_roles.contains(&Role::Tool));
}

#[tokio::test]
async fn malformed_tool_arguments_yield_error_result_and_loop_continues() {
    let first = Script::sse(vec![
        Frame::tool_head(0, "call_bad", "echo"),
        Frame::tool_args(0, "not-json"),
        Frame::finish("tool_calls"),
        Frame::done(),
    ]);
    let server = MockServer::spawn(vec![first, stop_script(&["recovered"])]).await;
    let client = CompanionClient::from_config(&config(&server.base_url)).unwrap();
    let mut chat = Session::new(client, 32768)
        .with_system("s")
        .with_executor(Echo);
    let reply = chat
        .turn("run", &Cancellation::new(), |_| {})
        .await
        .unwrap();
    assert_eq!(reply, "recovered");
    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    let second: Value = serde_json::from_str(&requests[1].body).unwrap();
    let tool = second["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|msg| msg["role"] == "tool")
        .unwrap();
    assert_eq!(
        tool["content"].as_str().unwrap(),
        text::get("companion.malformed_tool_args")
    );
    assert!(!chat.history().iter().any(|m| m.content == "not-json"));
}

#[tokio::test]
async fn cancel_mid_stream_does_not_commit_incomplete_assistant() {
    let script = Script::sse(vec![
        Frame::content("partial-"),
        Frame::content_delayed(Duration::from_millis(800), "secret-tail"),
        Frame::finish("stop"),
        Frame::done(),
    ]);
    let server = MockServer::spawn(vec![script]).await;
    let mut chat = session(&server, 32768);
    let cancel = Cancellation::new();
    let (seen_tx, seen_rx) = tokio::sync::oneshot::channel();
    let mut seen = Some(seen_tx);
    let cancel_for_turn = cancel.clone();
    let task = tokio::spawn(async move {
        let result = chat
            .turn("hi", &cancel_for_turn, move |token| {
                if token == "partial-" {
                    if let Some(tx) = seen.take() {
                        let _ = tx.send(());
                    }
                }
            })
            .await;
        (result, chat)
    });
    seen_rx.await.unwrap();
    cancel.cancel();
    let (result, chat) = task.await.unwrap();
    assert!(matches!(result, Err(CompanionError::Cancelled)));
    assert!(!chat.history().iter().any(|m| m.role == Role::Assistant));
    assert!(!chat
        .history()
        .iter()
        .any(|m| m.content.contains("partial") || m.content.contains("secret-tail")));
    let started = tokio::time::Instant::now();
    while !server.aborted() {
        if started.elapsed() > Duration::from_secs(3) {
            panic!("HTTP body was not aborted");
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn context_pressure_drops_oldest_turns_and_invokes_injector() {
    let marker = "UNIQUE_OLD_TURN_xyz";
    let old_user = Message::user(marker);
    let old_assistant = Message::assistant("old-reply", Vec::new());
    let current = Message::user("now-please");
    let system = Message::system("s");
    let window = (message_tokens(&system) + message_tokens(&current) + 4) as u32;
    let server = MockServer::spawn(vec![stop_script(&["ok"])]).await;
    let dropped = Arc::new(Mutex::new(Vec::new()));
    let client = CompanionClient::from_config(&config(&server.base_url)).unwrap();
    let mut chat = Session::new(client, window)
        .with_system("s")
        .with_recall(Recording {
            dropped: dropped.clone(),
        });
    chat.seed([old_user, old_assistant]);
    let _ = chat
        .turn("now-please", &Cancellation::new(), |_| {})
        .await
        .unwrap();
    let body = &server.requests()[0].body;
    assert!(!body.contains(marker), "{body}");
    assert!(!body.contains("old-reply"), "{body}");
    assert!(body.contains("now-please"), "{body}");
    assert!(!body.to_lowercase().contains("summary"), "{body}");
    let seen = dropped.lock().unwrap();
    assert_eq!(seen.len(), 1);
    assert!(seen[0].iter().any(|m| m.content.contains(marker)));
}

#[tokio::test]
async fn empty_tool_list_omits_tools_field() {
    let server = MockServer::spawn(vec![stop_script(&["ok"])]).await;
    let mut chat = session(&server, 32768);
    chat.turn("hi", &Cancellation::new(), |_| {}).await.unwrap();
    let body: Value = serde_json::from_str(&server.requests()[0].body).unwrap();
    assert!(body.get("tools").is_none(), "{body}");
}

#[tokio::test]
async fn api_key_never_appears_in_client_errors() {
    let secret = "s3cret-companion-key";
    let cfg = CompanionConfig {
        base_url: "http://127.0.0.1:1".into(),
        api_key: Some(ApiKey::new(secret)),
        ..CompanionConfig::default()
    };
    let client = CompanionClient::from_config(&cfg).unwrap();
    let error = client
        .complete(&[Message::user("hi")], &[], &Cancellation::new(), |_| {})
        .await
        .unwrap_err();
    assert!(!error.to_string().contains(secret), "{error}");
    assert!(!format!("{error:?}").contains(secret));
    assert!(!format!("{cfg:?}").contains(secret));

    let server = MockServer::spawn(vec![stop_script(&["ok"])]).await;
    let cfg = CompanionConfig {
        api_key: Some(ApiKey::new(secret)),
        ..config(&server.base_url)
    };
    let client = CompanionClient::from_config(&cfg).unwrap();
    client
        .complete(&[Message::user("hi")], &[], &Cancellation::new(), |_| {})
        .await
        .unwrap();
    assert_eq!(
        server.requests()[0].authorization.as_deref(),
        Some("Bearer s3cret-companion-key")
    );
}
