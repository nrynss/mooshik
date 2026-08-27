use std::time::Duration;

use lambo::gcp_auth::{
    build_client, credentials_path_from_env, load_credentials, GoogleAuthError,
    GoogleOAuthTokenSource,
};
use zeroize::Zeroizing;

use crate::config::{CompanionAuth, CompanionConfig};

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

/// How this client authenticates every request.
///
/// The distinction is not cosmetic. A static key is minted once by a human and
/// is still valid tomorrow; a Google access token expires in about an hour, so
/// a header built once at construction authenticates for one hour and 401s
/// after that — on a companion whose whole premise is running beside you all
/// day. The Google arm therefore asks for a token *per request* and lets
/// `lambo::gcp_auth` cache and refresh it ahead of expiry.
enum Auth {
    /// A fixed bearer key, or none at all. The local and generic
    /// OpenAI-compatible path, unchanged.
    Static(Option<crate::config::ApiKey>),
    /// Google OAuth. The `Mutex` is `tokio`'s because `access_token` is async
    /// and takes `&mut self`, while `complete` only has `&self`.
    /// Boxed: the token source dwarfs the static key, and an unboxed
    /// variant makes every `Auth` that size.
    Google(Box<tokio::sync::Mutex<GoogleOAuthTokenSource>>),
}

pub struct CompanionClient {
    http: reqwest::Client,
    completions_url: String,
    model: String,
    temperature: f64,
    auth: Auth,
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
            // Derived, not pasted: under the Google posture the endpoint is a
            // pure function of project and location (`resolved_base_url`).
            completions_url: chat_completions_url(&config.resolved_base_url()),
            model: config.model.clone(),
            temperature: config.temperature,
            auth: build_auth(config)?,
        })
    }

    /// The `Authorization` header for one request, or `None` when the endpoint
    /// takes no credential (the local default).
    ///
    /// The Google arm mints through `lambo::gcp_auth`, which caches until
    /// roughly a minute before expiry and re-mints after — so a day-long chat
    /// keeps working rather than 401ing an hour in. The token is wrapped in
    /// `Zeroizing` the instant it arrives: it is a live credential for the
    /// next hour and must not be left in our memory once the request is built.
    async fn authorization(&self) -> Result<Option<Zeroizing<String>>, CompanionError> {
        match &self.auth {
            Auth::Static(key) => Ok(bearer_header(key.as_ref())),
            Auth::Google(source) => {
                let mut guard = source.lock().await;
                let token = Zeroizing::new(guard.access_token().await.map_err(map_google)?);
                Ok(Some(Zeroizing::new(format!("Bearer {}", token.as_str()))))
            }
        }
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
        // Minting can block on Google's token endpoint, so Ctrl-C reaches it
        // too rather than only the completion that follows.
        let authorization = tokio::select! {
            _ = cancel.cancelled() => return Err(CompanionError::Cancelled),
            header = self.authorization() => header?,
        };
        let mut builder = self.http.post(&self.completions_url).json(&request);
        if let Some(header) = &authorization {
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

/// Build the auth arm from configuration. Nothing here reaches the network:
/// the token source only reads the credential file and keeps a client, so a
/// test can construct a Google client from a dummy credential and never mint.
fn build_auth(config: &CompanionConfig) -> Result<Auth, CompanionError> {
    match config.auth {
        CompanionAuth::Static => Ok(Auth::Static(config.api_key.clone())),
        CompanionAuth::Google => {
            let path = config
                .google_credentials
                .clone()
                .or_else(credentials_path_from_env)
                .ok_or(CompanionError::AuthUnavailable)?;
            let credentials = load_credentials(&path).map_err(map_google)?;
            let http = build_client().map_err(map_google)?;
            let source =
                GoogleOAuthTokenSource::for_vertex(credentials, http).map_err(map_google)?;
            Ok(Auth::Google(Box::new(tokio::sync::Mutex::new(source))))
        }
    }
}

/// Map Google's two-way classification onto this module's, keeping the split
/// (transient vs. operator-fixable) and dropping the *message*.
///
/// The message is dropped deliberately, not lazily: `GoogleAuthError::Backend`
/// formats the token endpoint's response body into itself, and a body is not
/// something this crate gets to promise is free of credential material. The
/// terminal sees `en.toml` and nothing else.
fn map_google(error: GoogleAuthError) -> CompanionError {
    match error {
        GoogleAuthError::Unavailable(_) => CompanionError::AuthUnavailable,
        GoogleAuthError::Backend(_) => CompanionError::AuthRefused,
    }
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
        if let Some(extra) = call.extra_content {
            slot.thought_signature = extra.google.and_then(|google| google.thought_signature);
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
