use std::time::Duration;

use zeroize::Zeroizing;

use crate::config::CompanionConfig;

use super::cancel::Cancellation;
use super::sse::{parse_chunk, SseEvent, SseParser};
use super::tools::ToolSpec;
use super::types::{
    ChatRequest, Finish, Message, PartialToolCall, ToolCall, WireMessage, WireTool, WireToolFn,
};
use super::CompanionError;

/// Whole-request stall limit, including the SSE body. Caller cancel still aborts first.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

pub struct CompanionClient {
    http: reqwest::Client,
    completions_url: String,
    model: String,
    temperature: f64,
    api_key: Option<crate::config::ApiKey>,
}

#[derive(Debug)]
pub struct Completion {
    pub content: String,
    pub tool_calls: Vec<ToolCall>,
    pub finish: Finish,
}

impl CompanionClient {
    pub fn from_config(config: &CompanionConfig) -> Result<Self, CompanionError> {
        let http = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(|_| CompanionError::Unreachable)?;
        Ok(Self {
            http,
            completions_url: chat_completions_url(&config.base_url),
            model: config.model.clone(),
            temperature: config.temperature,
            api_key: config.api_key.clone(),
        })
    }

    pub async fn complete(
        &self,
        messages: &[Message],
        tools: &[ToolSpec],
        cancel: &Cancellation,
        mut on_token: impl FnMut(&str),
    ) -> Result<Completion, CompanionError> {
        let request = ChatRequest {
            model: self.model.clone(),
            messages: messages.iter().map(WireMessage::from).collect(),
            stream: true,
            temperature: self.temperature,
            tools: wire_tools(tools),
        };
        let mut builder = self.http.post(&self.completions_url).json(&request);
        if let Some(header) = bearer_header(self.api_key.as_ref()) {
            builder = builder.header(reqwest::header::AUTHORIZATION, header.as_str());
        }

        let mut response = tokio::select! {
            _ = cancel.cancelled() => return Err(CompanionError::Cancelled),
            result = builder.send() => map_send(result)?,
        };
        if !response.status().is_success() {
            let _ = response.bytes().await;
            return Err(CompanionError::HttpStatus);
        }

        let mut parser = SseParser::new();
        let mut content = String::new();
        let mut partial = Vec::new();
        let mut finish = Finish::Stop;
        let mut done = false;
        while !done {
            tokio::select! {
                _ = cancel.cancelled() => return Err(CompanionError::Cancelled),
                chunk = response.chunk() => {
                    match chunk {
                        Ok(None) => break,
                        Ok(Some(bytes)) => {
                            for event in parser.push(&bytes) {
                                if apply_event(
                                    event,
                                    &mut content,
                                    &mut partial,
                                    &mut finish,
                                    &mut on_token,
                                )? {
                                    done = true;
                                    break;
                                }
                            }
                        }
                        Err(error) => return Err(map_reqwest(&error)),
                    }
                }
            }
        }
        if !done {
            for event in parser.finish() {
                let _ = apply_event(
                    event,
                    &mut content,
                    &mut partial,
                    &mut finish,
                    &mut on_token,
                )?;
            }
        }
        if cancel.is_cancelled() {
            return Err(CompanionError::Cancelled);
        }
        Ok(Completion {
            content,
            tool_calls: assemble(partial),
            finish,
        })
    }
}

pub fn chat_completions_url(base_url: &str) -> String {
    format!("{}/chat/completions", base_url.trim().trim_end_matches('/'))
}

fn wire_tools(tools: &[ToolSpec]) -> Option<Vec<WireTool>> {
    if tools.is_empty() {
        return None;
    }
    Some(
        tools
            .iter()
            .map(|spec| WireTool {
                kind: "function",
                function: WireToolFn {
                    name: spec.name.clone(),
                    description: spec.description.clone(),
                    parameters: spec.parameters.clone(),
                },
            })
            .collect(),
    )
}

fn bearer_header(key: Option<&crate::config::ApiKey>) -> Option<Zeroizing<String>> {
    let value = key.map(|k| k.expose()).filter(|s| !s.is_empty())?;
    Some(Zeroizing::new(format!("Bearer {value}")))
}

fn assemble(partial: Vec<PartialToolCall>) -> Vec<ToolCall> {
    partial
        .into_iter()
        .enumerate()
        .filter(|(_, call)| !call.name.is_empty() || !call.arguments.is_empty())
        .map(|(index, call)| call.into_tool_call(index))
        .collect()
}

fn apply_event(
    event: SseEvent,
    content: &mut String,
    partial: &mut Vec<PartialToolCall>,
    finish: &mut Finish,
    on_token: &mut impl FnMut(&str),
) -> Result<bool, CompanionError> {
    match event {
        SseEvent::Done => Ok(true),
        SseEvent::Data(data) => {
            let chunk = parse_chunk(&data)?;
            let Some(choice) = chunk.choices.and_then(|choices| choices.into_iter().next()) else {
                return Ok(false);
            };
            if let Some(delta) = choice.delta {
                if let Some(piece) = delta.content {
                    if !piece.is_empty() {
                        on_token(&piece);
                        content.push_str(&piece);
                    }
                }
                if let Some(calls) = delta.tool_calls {
                    merge_tool_deltas(partial, calls);
                }
            }
            if choice.finish_reason.as_deref() == Some("tool_calls") {
                *finish = Finish::ToolCalls;
            }
            Ok(false)
        }
    }
}

fn merge_tool_deltas(partial: &mut Vec<PartialToolCall>, calls: Vec<super::types::ToolCallDelta>) {
    for call in calls {
        if call.index >= partial.len() {
            partial.resize(call.index + 1, PartialToolCall::default());
        }
        let slot = &mut partial[call.index];
        if let Some(id) = call.id {
            slot.id = id;
        }
        if let Some(function) = call.function {
            if let Some(name) = function.name {
                slot.name.push_str(&name);
            }
            if let Some(args) = function.arguments {
                slot.arguments.push_str(&args);
            }
        }
    }
}

fn map_send(
    result: Result<reqwest::Response, reqwest::Error>,
) -> Result<reqwest::Response, CompanionError> {
    result.map_err(|error| map_reqwest(&error))
}

fn map_reqwest(error: &reqwest::Error) -> CompanionError {
    if error.is_timeout() {
        CompanionError::Timeout
    } else {
        CompanionError::Unreachable
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_trailing_slash_on_base_url() {
        assert_eq!(
            chat_completions_url("http://127.0.0.1:8080/v1"),
            "http://127.0.0.1:8080/v1/chat/completions"
        );
        assert_eq!(
            chat_completions_url("http://127.0.0.1:8080/v1/"),
            "http://127.0.0.1:8080/v1/chat/completions"
        );
    }
}
