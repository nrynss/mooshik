use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::Tool => "tool",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Message {
    pub role: Role,
    pub content: String,
    pub tool_calls: Vec<ToolCall>,
    pub tool_call_id: Option<String>,
}

impl Message {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: content.into(),
            tool_calls: Vec::new(),
            tool_call_id: None,
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
            tool_calls: Vec::new(),
            tool_call_id: None,
        }
    }

    pub fn assistant(content: impl Into<String>, tool_calls: Vec<ToolCall>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
            tool_calls,
            tool_call_id: None,
        }
    }

    pub fn tool(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: Role::Tool,
            content: content.into(),
            tool_calls: Vec::new(),
            tool_call_id: Some(tool_call_id.into()),
        }
    }

    pub fn token_chars(&self) -> usize {
        let mut n = self.content.chars().count() + self.role.as_str().len();
        for call in &self.tool_calls {
            n += call.id.chars().count()
                + call.name.chars().count()
                + call.arguments.chars().count();
        }
        if let Some(id) = &self.tool_call_id {
            n += id.chars().count();
        }
        n
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Finish {
    Stop,
    ToolCalls,
}

#[derive(Serialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<WireMessage>,
    pub stream: bool,
    pub temperature: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<WireTool>>,
}

#[derive(Serialize)]
pub struct WireMessage {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<WireToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

#[derive(Serialize)]
pub struct WireToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub function: WireFunction,
}

#[derive(Serialize)]
pub struct WireFunction {
    pub name: String,
    pub arguments: String,
}

#[derive(Serialize)]
pub struct WireTool {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub function: WireToolFn,
}

#[derive(Serialize)]
pub struct WireToolFn {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

impl From<&Message> for WireMessage {
    fn from(message: &Message) -> Self {
        let tool_calls = if message.tool_calls.is_empty() {
            None
        } else {
            Some(
                message
                    .tool_calls
                    .iter()
                    .map(|call| WireToolCall {
                        id: call.id.clone(),
                        kind: "function",
                        function: WireFunction {
                            name: call.name.clone(),
                            arguments: call.arguments.clone(),
                        },
                    })
                    .collect(),
            )
        };
        let content = if message.content.is_empty() && tool_calls.is_some() {
            None
        } else {
            Some(message.content.clone())
        };
        Self {
            role: message.role.as_str().to_owned(),
            content,
            tool_calls,
            tool_call_id: message.tool_call_id.clone(),
        }
    }
}

/// Wire types omit `deny_unknown_fields`: OpenAI-compat endpoints add keys.
#[derive(Debug, Deserialize)]
pub struct ChatChunk {
    pub choices: Option<Vec<ChunkChoice>>,
}

#[derive(Debug, Deserialize)]
pub struct ChunkChoice {
    pub delta: Option<ChunkDelta>,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ChunkDelta {
    pub content: Option<String>,
    pub tool_calls: Option<Vec<ToolCallDelta>>,
}

#[derive(Debug, Deserialize)]
pub struct ToolCallDelta {
    pub index: usize,
    pub id: Option<String>,
    pub function: Option<FunctionDelta>,
}

#[derive(Debug, Deserialize)]
pub struct FunctionDelta {
    pub name: Option<String>,
    pub arguments: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct PartialToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

impl PartialToolCall {
    pub fn into_tool_call(self, index: usize) -> ToolCall {
        ToolCall {
            id: if self.id.is_empty() {
                format!("call_{index}")
            } else {
                self.id
            },
            name: self.name,
            arguments: self.arguments,
        }
    }
}
