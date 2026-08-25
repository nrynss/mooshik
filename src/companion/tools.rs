use std::sync::Arc;

use serde_json::Value;

#[derive(Clone, Debug, PartialEq)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

pub trait ToolExecutor: Send + Sync {
    fn specs(&self) -> Vec<ToolSpec>;
    fn execute(&self, name: &str, arguments: &Value) -> String;
}

/// Blanket impl so a fresh `Arc<dyn ToolExecutor>` (already `Send + Sync`) can
/// serve directly as the `Session` executor type. The M4 injection seam hands
/// `run_chat` an `Arc<dyn ToolExecutor>` (the lambo tools or the No-op
/// fallback), and `Session<E, R>` must accept it as its `E`.
impl ToolExecutor for Arc<dyn ToolExecutor> {
    fn specs(&self) -> Vec<ToolSpec> {
        (**self).specs()
    }

    fn execute(&self, name: &str, arguments: &Value) -> String {
        (**self).execute(name, arguments)
    }
}

pub struct NoopExecutor;

impl ToolExecutor for NoopExecutor {
    fn specs(&self) -> Vec<ToolSpec> {
        Vec::new()
    }

    fn execute(&self, _name: &str, _arguments: &Value) -> String {
        crate::text::get("companion.unknown_tool").to_owned()
    }
}

pub fn parse_tool_object(arguments: &str) -> Result<Value, ()> {
    let value: Value = serde_json::from_str(arguments).map_err(|_| ())?;
    match value {
        Value::Object(_) => Ok(value),
        _ => Err(()),
    }
}