//! The M4 tool schemas, lifted from Lambo's own MCP surface.
//!
//! Mooshik exposes the *same* lambo tools its MCP server does, backed by
//! in-process `Memory` instead of JSON-RPC. The parameter structs are plain
//! `serde` + `schemars`, exactly as they appear in
//! `lambo/src/mcp/server.rs` (`RecallParams`, `DeriveParams`, `WireConcept`,
//! `WireConceptType`, `WireParentOf`, `WireResource`, `RecordActionParams`,
//! `StatsParams`) — the `deny_unknown_fields` discipline, the length caps, and
//! the range caps are preserved rather than re-negotiated. `WireResource` and
//! `RecordActionParams` are lifted because M4's contract names them; the
//! `lambo_record_action` tool itself is out of scope and returns in a later
//! milestone.
//!
//! `run_scratch_script` is a Mooshik-only tool (Lambo has no such surface), so
//! its `ScratchParams` is authored here to the same discipline.
//!
//! Because `#[schemars(...)]` attributes only shape the *generated JSON schema*
//! and do not enforce anything at deserialization time, every executor also
//! re-validates at the door with the helpers in this module — an over-length or
//! wrongly-typed parameter must come back as a tool error string, never a panic.

use serde::Deserialize;
use serde_json::Value;

/// Uniform per-string cap (characters), matching lambo's `schemars(length(max =
/// 16_384))` on every lifted field.
pub const MAX_STRING_CHARS: usize = 16_384;
pub const MAX_TOP_K: usize = 100;
pub const MAX_MAX_TOKENS: usize = 100_000;
pub const MAX_TRAVERSAL_DEPTH: usize = 5;
pub const MAX_CONCEPTS_PER_DERIVE: usize = 64;

/// Reject an over-length (or, at the caller's discretion, malformed) string
/// field with a readable one-line tool error, mirroring lambo's `check_size`.
pub fn check_size(field: &str, value: &str) -> Result<(), String> {
    if value.chars().count() > MAX_STRING_CHARS {
        Err(format!("{field} must be at most {MAX_STRING_CHARS} characters"))
    } else {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Lifted lambo schemas
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WireConceptType {
    Entity,
    Logic,
    Constraint,
    Resource,
    Observation,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RecallParams {
    /// Id of the agent making this call. Caller-asserted and unverified: work
    /// is recorded under exactly the id you send. Use one stable id per agent —
    /// callers sharing an id share its memory attribution and its soft locks.
    #[schemars(length(max = 16_384))]
    pub agent_id: String,
    /// Natural-language query.
    #[schemars(length(max = 16_384))]
    pub query: String,
    /// Hits to return. Defaults to the session config's `default_top_k`.
    #[schemars(range(min = 1, max = 100))]
    pub top_k: Option<usize>,
    /// Token budget for the rendered context block.
    #[schemars(range(min = 1, max = 100_000))]
    pub max_tokens: Option<usize>,
    /// Graph traversal depth for phase 2 expansion.
    #[schemars(range(min = 0, max = 5))]
    pub traversal_depth: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WireConcept {
    /// The concept text.
    #[schemars(length(max = 16_384))]
    pub content: String,
    /// One of `entity`, `logic`, `constraint`, `resource`, `observation`.
    pub concept_type: WireConceptType,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WireParentOf {
    #[schemars(length(max = 16_384))]
    pub parent: String,
    #[schemars(length(max = 16_384))]
    pub child: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeriveParams {
    /// Id of the agent making this call. Caller-asserted and unverified: work
    /// is recorded under exactly the id you send. Use one stable id per agent —
    /// callers sharing an id share its memory attribution and its soft locks.
    #[schemars(length(max = 16_384))]
    pub agent_id: String,
    /// Concepts to derive from this interaction.
    pub concepts: Vec<WireConcept>,
    /// Optional `(parent, child)` hierarchy pairs. Both ends resolve (and may
    /// be created) as concepts.
    pub parent_of: Option<Vec<WireParentOf>>,
}

/// One entry in a `lambo_record_action` resource list (`produces`, `modifies`,
/// `depends_on`). A plain string on the wire, with the same per-string size cap
/// the runtime enforces.
///
/// Lifted per M4's contract; nothing constructs it until the
/// `lambo_record_action` tool lands (out of M4 scope).
#[allow(dead_code)]
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
pub struct WireResource(#[schemars(length(max = 16_384))] pub String);

/// `lambo_record_action` parameters, lifted per M4's contract. The tool itself
/// is out of scope for M4; the schema ships now so it is ready and reviewed.
#[allow(dead_code)]
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RecordActionParams {
    /// Id of the agent making this call. Caller-asserted and unverified: work
    /// is recorded under exactly the id you send. Use one stable id per agent —
    /// callers sharing an id share its memory attribution and its soft locks.
    #[schemars(length(max = 16_384))]
    pub agent_id: String,
    /// The action taken — becomes a `Resource` concept.
    #[schemars(length(max = 16_384))]
    pub action: String,
    /// Resources this action creates (`Causal` edges).
    pub produces: Option<Vec<WireResource>>,
    /// Resources this action mutates (`Causal` edges).
    pub modifies: Option<Vec<WireResource>>,
    /// Things this action depends on (`Dependency` edges).
    pub depends_on: Option<Vec<WireResource>>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StatsParams {
    /// Id of the agent making this call. Caller-asserted and unverified: work
    /// is recorded under exactly the id you send. Use one stable id per agent —
    /// callers sharing an id share its memory attribution and its soft locks.
    #[schemars(length(max = 16_384))]
    pub agent_id: String,
    /// A write receipt id from a `lambo_derive` or `lambo_record_action` ack.
    /// Accepted for schema compatibility; M4's synchronous `derive` never issues
    /// receipts (that is the async ack path), so this is reported as unknown.
    #[schemars(length(max = 16_384))]
    pub receipt: Option<String>,
    /// With `receipt`, wait up to this many milliseconds for the write to be
    /// applied before answering. A no-op in M4: there is no pending async write
    /// to wait on when `derive` is synchronous. Kept so the wire shape stays
    /// identical to lambo's.
    #[allow(dead_code)]
    #[schemars(range(min = 0, max = 4_000))]
    pub wait_ms: Option<u64>,
}

// ---------------------------------------------------------------------------
// Mooshik-only schema: run_scratch_script
// ---------------------------------------------------------------------------

/// Default hard wall-clock timeout for a scratch script (seconds).
pub const SCRATCH_DEFAULT_TIMEOUT_SECS: u64 = 30;
/// Upper bound an operator can request (seconds).
pub const SCRATCH_MAX_TIMEOUT_SECS: u64 = 300;
/// Cap on the script body size (bytes).
pub const SCRATCH_MAX_SCRIPT_BYTES: usize = 64 * 1024;
/// Cap on captured stdout+stderr (bytes).
pub const SCRATCH_MAX_OUTPUT_BYTES: usize = 64 * 1024;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScratchParams {
    /// The interpreter to run the script with. `bash` or `python3` only; the
    /// runner never passes the code through a shell.
    pub language: ScratchLanguage,
    /// The script body, written to an isolated sandbox directory and exec'd.
    #[schemars(length(max = 65_536))]
    pub code: String,
    /// Hard wall-clock timeout in seconds (default 30, max 300). The child is
    /// killed on expiry and never left orphaned.
    #[schemars(range(min = 1, max = 300))]
    pub timeout_secs: Option<u64>,
}

#[derive(Clone, Copy, Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ScratchLanguage {
    Bash,
    Python3,
}

impl ScratchLanguage {
    pub fn interpreter(self) -> &'static str {
        match self {
            Self::Bash => "bash",
            Self::Python3 => "python3",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Bash => "sh",
            Self::Python3 => "py",
        }
    }
}

// ---------------------------------------------------------------------------
// Tool-spec parameter generation
// ---------------------------------------------------------------------------

/// Build an OpenAI-compatible `parameters` JSON object for a lifted/derived
/// `schemars::JsonSchema` type.
///
/// `schemars` emits the type as a root `$ref` into a definitions map. Some
/// OpenAI-compatible servers reject a bare root `$ref`, so we resolve the root
/// reference to its definition and keep the definitions map in place for any
/// nested references to resolve against.
pub fn tool_parameters<T: schemars::JsonSchema>() -> Value {
    let schema: schemars::Schema =
        schemars::SchemaGenerator::default().into_root_schema_for::<T>();
    let mut value = serde_json::to_value(&schema).unwrap_or(Value::Object(Default::default()));
    inline_root_ref(&mut value);
    value
}

/// Resolve a root `$ref` (`#/definitions/NAME` or `#/$defs/NAME`) to its
/// definition, so the schema opens with `type: object`.
fn inline_root_ref(root: &mut Value) {
    let object = match root.as_object_mut() {
        Some(object) => object,
        None => return,
    };
    let reference = match object.get("$ref").and_then(Value::as_str) {
        Some(reference) => reference.to_owned(),
        None => return,
    };
    let Some((kind, name)) = reference
        .strip_prefix("#/")
        .and_then(|rest| rest.split_once('/'))
    else {
        return;
    };
    let Some(definitions) = object.get(kind).and_then(Value::as_object) else {
        return;
    };
    let Some(definition) = definitions.get(name).cloned() else {
        return;
    };
    if let Some(target) = definition.as_object() {
        for (key, value) in target {
            object.insert(key.clone(), value.clone());
        }
    }
    object.remove("$ref");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recall_params_schema_is_an_object_with_required_fields() {
        let value = tool_parameters::<RecallParams>();
        let object = value.as_object().unwrap();
        assert_eq!(object["type"], "object");
        let required = object["required"].as_array().unwrap();
        let names: Vec<_> = required.iter().map(|v| v.as_str().unwrap()).collect();
        assert!(names.contains(&"agent_id"));
        assert!(names.contains(&"query"));
        assert!(!names.contains(&"top_k"), "optional knobs are not required");
        // Caps survive into the generated schema.
        let props = object["properties"].as_object().unwrap();
        assert_eq!(props["query"]["maxLength"], 16384);
        assert_eq!(props["top_k"]["maximum"], 100);
    }

    #[test]
    fn scratch_params_schema_carries_its_caps() {
        let value = tool_parameters::<ScratchParams>();
        let object = value.as_object().unwrap();
        let props = object["properties"].as_object().unwrap();
        assert_eq!(props["code"]["maxLength"], 65536);
        assert_eq!(props["timeout_secs"]["maximum"], 300);
    }

    #[test]
    fn check_size_enforces_the_uniform_cap() {
        assert!(check_size("f", &"x".repeat(16_384)).is_ok());
        assert!(check_size("f", &"x".repeat(16_385)).is_err());
        // Cap counts characters, not bytes (schemars `length` semantics).
        let multi_byte = "λ".repeat(8_200);
        assert!(check_size("f", &multi_byte).is_ok());
    }
}