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
