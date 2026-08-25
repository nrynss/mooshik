use serde_json::Value;

use crate::config::{CompanionConfig, Config};
use crate::text;

use super::cancel::Cancellation;
use super::client::CompanionClient;
use super::loop_tests::{config, session};
use super::mock::{Frame, MockServer, Script};
use super::session::Session;
use super::tools::{ToolExecutor, ToolSpec};
use super::CompanionError;

struct Pair;

impl ToolExecutor for Pair {
    fn specs(&self) -> Vec<ToolSpec> {
        let parameters = serde_json::json!({
            "type": "object",
            "properties": {"text": {"type": "string"}}
        });
        vec![
            ToolSpec {
                name: "echo".into(),
                description: "echo".into(),
                parameters: parameters.clone(),
            },
            ToolSpec {
                name: "shout".into(),
                description: "shout".into(),
                parameters,
            },
        ]
    }

    fn execute(&self, name: &str, arguments: &Value) -> String {
        let text = arguments.get("text").and_then(Value::as_str).unwrap_or("");
        match name {
            "echo" => text.to_owned(),
            "shout" => text.to_ascii_uppercase(),
            other => panic!("unexpected tool {other}"),
        }
    }
}

fn stop_script(text: &str) -> Script {
    Script::sse(vec![
        Frame::content_openai(text),
        Frame::finish("stop"),
        Frame::done(),
    ])
}

#[tokio::test]
async fn non_2xx_body_is_not_in_http_status_error() {
    let secret = "s3cret-http-body";
    let server = MockServer::spawn(vec![Script::error(
        401,
        &format!(r#"{{"error":"{secret}"}}"#),
    )])
    .await;
    let mut chat = session(&server, 32768);
    let error = chat
        .turn("hi", &Cancellation::new(), |_| {})
        .await
        .unwrap_err();
    assert!(matches!(error, CompanionError::HttpStatus));
    assert!(!error.to_string().contains(secret), "{error}");
    assert!(!format!("{error:?}").contains(secret));
    assert_eq!(error.to_string(), text::get("companion.http_status"));
    server.assert_all_streaming();
}

#[tokio::test]
async fn parallel_tool_calls_merged_by_index() {
    let first = Script::sse(vec![
        Frame::tool_head(0, "call_a", "echo"),
        Frame::tool_head(1, "call_b", "shout"),
        Frame::tool_args(0, "{\"text\":"),
        Frame::tool_args(1, "{\"text\":"),
        Frame::tool_args(0, "\"aa\"}"),
        Frame::tool_args(1, "\"bb\"}"),
        Frame::finish("tool_calls"),
        Frame::done(),
    ]);
    let server = MockServer::spawn(vec![first, stop_script("done")]).await;
    let client = CompanionClient::from_config(&config(&server.base_url)).unwrap();
    let mut chat = Session::new(client, 32768)
        .with_system("s")
        .with_executor(Pair);
    let reply = chat
        .turn("run", &Cancellation::new(), |_| {})
        .await
        .unwrap();
    assert_eq!(reply, "done");
    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    server.assert_all_streaming();
    let advertised: Value = serde_json::from_str(&requests[0].body).unwrap();
    assert_eq!(advertised["tools"][0]["function"]["name"], "echo");
    assert_eq!(advertised["tools"][1]["function"]["name"], "shout");
    let second: Value = serde_json::from_str(&requests[1].body).unwrap();
    let messages = second["messages"].as_array().unwrap();
    let assistant = messages
        .iter()
        .find(|msg| msg["role"] == "assistant" && msg.get("tool_calls").is_some())
        .unwrap();
    let calls = assistant["tool_calls"].as_array().unwrap();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0]["id"], "call_a");
    assert_eq!(calls[0]["function"]["name"], "echo");
    assert_eq!(calls[0]["function"]["arguments"], "{\"text\":\"aa\"}");
    assert_eq!(calls[1]["id"], "call_b");
    assert_eq!(calls[1]["function"]["name"], "shout");
    assert_eq!(calls[1]["function"]["arguments"], "{\"text\":\"bb\"}");
    let tools: Vec<&Value> = messages
        .iter()
        .filter(|msg| msg["role"] == "tool")
        .collect();
    assert_eq!(tools.len(), 2);
    assert_eq!(tools[0]["tool_call_id"], "call_a");
    assert_eq!(tools[0]["content"], "aa");
    assert_eq!(tools[1]["tool_call_id"], "call_b");
    assert_eq!(tools[1]["content"], "BB");
}

#[tokio::test]
async fn default_config_reaches_companion_without_a_dsn() {
    let server = MockServer::spawn(vec![stop_script("ok")]).await;
    let config = Config {
        companion: CompanionConfig {
            base_url: server.base_url.clone(),
            ..CompanionConfig::default()
        },
        ..Config::default()
    };
    assert!(config.store.dsn.is_none());
    let client = CompanionClient::from_config(&config.companion).unwrap();
    let mut chat = Session::new(client, config.companion.context_window).with_system("s");
    let reply = chat.turn("hi", &Cancellation::new(), |_| {}).await.unwrap();
    assert_eq!(reply, "ok");
    server.assert_all_streaming();
}
