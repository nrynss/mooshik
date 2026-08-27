//! Configuration loading and the environment overlay.

use std::{
    collections::BTreeMap,
    env,
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
};

use lambo::{EmbedderConfig, EmbedderKind, LamboFile, StoreConfig, StoreKind};
use serde::{Deserialize, Serialize};

use crate::{secure_path, text};

mod companion;
mod overlay;
mod permissions;
mod show;
mod write;

pub use companion::{
    vertex_base_url, ApiKey, CompanionAuth, CompanionConfig, COMPANION_API_KEY_ENV,
    COMPANION_AUTH_ENV, COMPANION_BASE_URL_ENV, COMPANION_CONTEXT_WINDOW_ENV,
    COMPANION_GOOGLE_LOCATION_ENV, COMPANION_GOOGLE_PROJECT_ENV, COMPANION_MODEL_ENV,
    COMPANION_TEMPERATURE_ENV, DEFAULT_GOOGLE_LOCATION,
};
pub use permissions::{
    GrantDecision, GrantMode, GrantSource, Grants, PermissionsConfig, RawGrant, ScopedGrant,
};
pub use write::{apply_setting, settable_keys, store_move_requires_confirmation};

const MAX_CONFIG_BYTES: u64 = 64 * 1024;

pub const HOME_ENV: &str = "MOOSHIK_HOME";
pub const PROVIDER_ENV: &str = "MOOSHIK_VAULT_PROVIDER";
pub const PASSPHRASE_ENV: &str = "MOOSHIK_VAULT_PASSPHRASE";
pub const SESSION_ENV: &str = "MOOSHIK_SESSION";
pub const AGENT_ENV: &str = "MOOSHIK_AGENT";
pub const STORE_KIND_ENV: &str = "MOOSHIK_STORE_KIND";
pub const POSTGRES_DSN_ENV: &str = "MOOSHIK_POSTGRES_DSN";
pub const EMBEDDER_ENV: &str = "MOOSHIK_EMBEDDER";
pub const EMBED_DIM_ENV: &str = "MOOSHIK_EMBED_DIM";
pub const GEMINI_PROJECT_ENV: &str = "MOOSHIK_GEMINI_PROJECT";
pub const GEMINI_LOCATION_ENV: &str = "MOOSHIK_GEMINI_LOCATION";
pub const GEMINI_MODEL_ENV: &str = "MOOSHIK_GEMINI_MODEL";
pub const GEMINI_CREDENTIALS_ENV: &str = "MOOSHIK_GEMINI_CREDENTIALS";
pub const FLUSH_INTERVAL_ENV: &str = "MOOSHIK_FLUSH_INTERVAL_MS";

const DEFAULT_SESSION: &str = "mooshik";
const DEFAULT_AGENT: &str = "mooshik";
const DEFAULT_EMBED_DIM: usize = 1536;
const DEFAULT_GEMINI_LOCATION: &str = "us-central1";
const DEFAULT_GEMINI_MODEL: &str = "gemini-embedding-001";
const DEFAULT_FLUSH_INTERVAL_MS: u64 = 1000;

const DEFAULT_TOML: &str = r#"[vault]
provider = "keyring"

[session]
id = "mooshik"
agent = "mooshik"

[store]
kind = "postgres"

[embedder]
kind = "gemini"
dim = 1536
gemini_location = "us-central1"
gemini_model = "gemini-embedding-001"

[daemon]
flush_interval_ms = 1000

[companion]
# Local posture (the default): any OpenAI-compatible /v1 endpoint. A bearer
# credential is never written into this file — store it with
# `mooshik secret set <name>`, then reference that name from the CLI
# (`mooshik config set --help` lists every settable key).
base_url = "http://127.0.0.1:8080/v1"
model = "local-model"
context_window = 32768
temperature = 0.2
# Google posture (Vertex's OpenAI-compatible endpoint). There is no URL to
# paste: the endpoint is derived from project and location, and the access
# token is minted from your Google credentials and refreshed before it
# expires. Reachable without hand-editing this file:
#   mooshik config set companion.auth google
#   mooshik config set companion.google_project my-project
#   mooshik config set companion.google_location us-central1
#   mooshik config set companion.model gemini-2.5-flash

[permissions]
# Autonomy is granted, not configured (docs/SPEC.md). Families: memory, scratch.
# A family takes "allow", "prompt", or "deny", or a list of granted tool names;
# per-tool entries override the family. Everything not granted here is denied.
# memory  = ["recall", "derive"]
# scratch = "prompt"
# web     = "deny"
# Inject vault secrets into scratch scripts as process environment variables:
# the key is the env-var name the script reads, the value is the secret NAME
# in the vault (never the value). Resolved fresh at every run.
[tools.scratch.env]
# MCP servers (M10): configured servers are surfaced to the companion as
# mcp.<server>.<tool> tools, gated by [permissions] ("mcp.github.*" = "allow").
# `expose` is an allowlist: only the named tools appear; an empty list leaves
# the server inert (never spawned). `env` values are vault secret NAMES,
# resolved at spawn time — never literal tokens here.
# [mcp_servers.github]
# command = "uvx"
# args = ["mcp-server-github"]
# env = { GITHUB_PERSONAL_ACCESS_TOKEN = "github-token" }
# expose = ["create_issue", "list_issues"]
# GITHUB_TOKEN = "github-token"
"#;

#[derive(Debug)]
pub enum ConfigError {
    Io,
    HomeUnavailable,
    InvalidToml,
    InvalidValue,
    InvalidStoreKind,
    InvalidEmbedder,
    InvalidNumber,
    ZeroFlush,
    ZeroContextWindow,
    DsnConflict,
    InvalidScratchEnv,
    InvalidMcp,
    InvalidPermissions,
    /// `[store]` names both a literal `dsn` and a `dsn_secret`. Two authorities
    /// for one database, and picking one silently is how a provision lands on
    /// the wrong cluster — so it fails the load instead.
    DsnAndSecret,
    InvalidStoreSecret,
    InvalidApiKeySecret,
    InvalidCompanionAuth,
    MissingGoogleProject,
    /// A key `mooshik config set` does not know. Carries the key so the
    /// message can name it (never a `Debug` dump of the value).
    UnknownKey(String),
    /// A known key handed a value it cannot accept. `expected` is the resolved
    /// "what is valid" sentence; the offending value is deliberately NOT
    /// carried, because a rejected value can be credential material.
    InvalidSetting {
        key: &'static str,
        expected: &'static str,
    },
    /// A key that would put a credential in `config.toml`. Carries the key and
    /// the reference key that takes a vault secret name instead.
    SecretKey {
        key: &'static str,
        reference: &'static str,
    },
    /// The edited file did not read back as the value that was asked for. The
    /// editor is surgical, so this is the fail-closed net: refuse rather than
    /// leave a user's configuration in a shape nobody asked for.
    WriteVerifyFailed,
    WriteFailed,
    /// The store would move to a different database and `--confirm-database-change`
    /// was not given.
    StoreMoveUnconfirmed,
    /// A referenced store DSN secret is not in the vault. Carries the *name*,
    /// which is configuration (`config show` prints secret names), never a value.
    MissingStoreSecret(String),
    /// A referenced companion API-key secret is not in the vault.
    MissingApiKeySecret(String),
    VaultUnavailable,
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::UnknownKey(key) => text::get("config.unknown_key")
                .replace("{key}", key)
                .replace("{keys}", &settable_keys().join(", ")),
            Self::InvalidSetting { key, expected } => text::get("config.set_invalid")
                .replace("{key}", key)
                .replace("{expected}", expected),
            Self::SecretKey { key, reference } => text::get("config.secret_key")
                .replace("{key}", key)
                .replace("{reference}", reference),
            Self::MissingStoreSecret(name) => {
                text::get("config.missing_store_secret").replace("{name}", name)
            }
            Self::MissingApiKeySecret(name) => {
                text::get("config.missing_api_key_secret").replace("{name}", name)
            }
            other => text::get(other.key()).to_owned(),
        };
        f.write_str(&message)
    }
}

impl ConfigError {
    /// The `en.toml` key for the fixed-message variants. The four variants
    /// that interpolate are handled by `Display` directly.
    fn key(&self) -> &'static str {
        match self {
            Self::Io => "config.read_failed",
            Self::HomeUnavailable => "config.home_unavailable",
            Self::InvalidToml => "config.invalid_toml",
            Self::InvalidValue => "config.invalid_value",
            Self::InvalidStoreKind => "config.invalid_store_kind",
            Self::InvalidEmbedder => "config.invalid_embedder",
            Self::InvalidNumber => "config.invalid_number",
            Self::ZeroFlush => "config.zero_flush",
            Self::ZeroContextWindow => "config.zero_context_window",
            Self::DsnConflict => "config.dsn_conflict",
            Self::InvalidMcp => "config.invalid_mcp",
            Self::InvalidScratchEnv => "config.invalid_scratch_env",
            Self::InvalidPermissions => "config.invalid_permissions",
            Self::DsnAndSecret => "config.dsn_and_secret",
            Self::InvalidStoreSecret => "config.invalid_store_secret",
            Self::InvalidApiKeySecret => "config.invalid_api_key_secret",
            Self::InvalidCompanionAuth => "config.invalid_companion_auth",
            Self::MissingGoogleProject => "config.missing_google_project",
            Self::WriteVerifyFailed => "config.write_verify_failed",
            Self::WriteFailed => "config.write_failed",
            Self::StoreMoveUnconfirmed => "config.store_move_unconfirmed",
            Self::VaultUnavailable => "config.vault_unavailable",
            Self::UnknownKey(_)
            | Self::InvalidSetting { .. }
            | Self::SecretKey { .. }
            | Self::MissingStoreSecret(_)
            | Self::MissingApiKeySecret(_) => {
                unreachable!("interpolating variants are rendered by Display")
            }
        }
    }
}

impl std::error::Error for ConfigError {}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum VaultProvider {
    #[default]
    Keyring,
    Passphrase,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VaultConfig {
    #[serde(default)]
    pub provider: VaultProvider,
}

impl Default for VaultConfig {
    fn default() -> Self {
        Self {
            provider: VaultProvider::Keyring,
        }
    }
}

/// The `[tools]` section: only what a tool needs from configuration lives
/// here; tool behavior stays in code.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolsSection {
    #[serde(default)]
    pub scratch: ScratchToolsSection,
}

impl ToolsSection {
    /// Fail closed on an unusable `[tools.scratch.env]` entry, matching the
    /// M5 posture for `[permissions]`: a table that cannot be honored must
    /// fail the load rather than silently start scripts without their
    /// secrets. Values are secret *names*, never values.
    pub fn validate(&self) -> Result<(), ConfigError> {
        for (env_var, secret_name) in &self.scratch.env {
            let valid_env = matches!(
                env_var.as_bytes().first(),
                Some(b'A'..=b'Z' | b'a'..=b'z' | b'_')
            ) && env_var
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_');
            if !valid_env || !crate::vault::is_valid_name(secret_name) {
                return Err(ConfigError::InvalidScratchEnv);
            }
        }
        Ok(())
    }
}

/// The `[tools.scratch]` section.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScratchToolsSection {
    /// Environment variable name -> vault secret *name*, resolved at each
    /// script run through the vault; the value itself never appears here.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

/// One configured MCP server under `[mcp_servers.<name>]`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpServerConfig {
    /// Executable to spawn; may be an absolute path or a name on `PATH`.
    pub command: String,
    /// Extra arguments passed to `command` (default: none).
    #[serde(default)]
    pub args: Vec<String>,
    /// Process-env-var name -> vault secret *name*. Resolved at spawn time;
    /// the value is injected into the child's environment, never written into
    /// a readable config file (M6 constraint).
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Allowlist of tool names surfaced to the companion. Fail-closed: a
    /// server whose list exposes nothing is inert.
    #[serde(default)]
    pub expose: Vec<String>,
}

impl McpServerConfig {
    /// A server exposes something only when its allowlist is non-empty; the
    /// `[mcp_servers]` table is data, held inert otherwise.
    pub fn exposed(&self) -> bool {
        !self.expose.is_empty()
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SessionConfig {
    #[serde(default = "default_session_id")]
    pub id: String,
    #[serde(default = "default_agent_id")]
    pub agent: String,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            id: default_session_id(),
            agent: default_agent_id(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DaemonConfig {
    #[serde(default = "default_flush_interval_ms")]
    pub flush_interval_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gc_interval: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonization_eval_interval_secs: Option<u64>,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            flush_interval_ms: default_flush_interval_ms(),
            gc_interval: None,
            canonization_eval_interval_secs: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StoreSection {
    #[serde(default = "default_store_kind")]
    pub kind: StoreKind,
    #[serde(default)]
    pub dsn: Option<String>,
    /// The *name* of a vault secret holding the DSN — never the DSN. This is
    /// the reference the CLI write path sets, so a connection string carrying
    /// a password never has to be typed into a readable config file.
    #[serde(default)]
    pub dsn_secret: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub vector_dim: Option<usize>,
}

impl Default for StoreSection {
    fn default() -> Self {
        Self {
            kind: default_store_kind(),
            dsn: None,
            dsn_secret: None,
            path: None,
            vector_dim: None,
        }
    }
}

impl StoreSection {
    /// Fail closed on a `[store]` table that names two DSN authorities, and on
    /// a `dsn_secret` the vault could never hold.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if let Some(name) = &self.dsn_secret {
            if self
                .dsn
                .as_deref()
                .map(str::trim)
                .is_some_and(|value| !value.is_empty())
            {
                return Err(ConfigError::DsnAndSecret);
            }
            if !crate::vault::is_valid_name(name) {
                return Err(ConfigError::InvalidStoreSecret);
            }
        }
        Ok(())
    }

    pub fn to_lambo(&self) -> StoreConfig {
        StoreConfig {
            kind: self.kind,
            dsn: self.dsn.clone(),
            path: self.path.clone(),
            vector_dim: self.vector_dim,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EmbedderSection {
    #[serde(default = "default_embedder_kind")]
    pub kind: EmbedderKind,
    #[serde(default = "default_embed_dim")]
    pub dim: usize,
    #[serde(default)]
    pub gemini_project: Option<String>,
    #[serde(default = "default_gemini_location")]
    pub gemini_location: Option<String>,
    #[serde(default = "default_gemini_model")]
    pub gemini_model: Option<String>,
    #[serde(default)]
    pub gemini_credentials: Option<PathBuf>,
}

impl Default for EmbedderSection {
    fn default() -> Self {
        Self {
            kind: default_embedder_kind(),
            dim: default_embed_dim(),
            gemini_project: None,
            gemini_location: default_gemini_location(),
            gemini_model: default_gemini_model(),
            gemini_credentials: None,
        }
    }
}

impl EmbedderSection {
    pub fn to_lambo(&self) -> EmbedderConfig {
        EmbedderConfig {
            kind: self.kind,
            dim: self.dim,
            gemini_project: self.gemini_project.clone(),
            gemini_location: self.gemini_location.clone(),
            gemini_model: self.gemini_model.clone(),
            gemini_credentials: self.gemini_credentials.clone(),
            ..EmbedderConfig::default()
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub vault: VaultConfig,
    #[serde(default)]
    pub session: SessionConfig,
    #[serde(default)]
    pub store: StoreSection,
    #[serde(default)]
    pub embedder: EmbedderSection,
    #[serde(default)]
    pub daemon: DaemonConfig,
    #[serde(default)]
    pub companion: CompanionConfig,
    #[serde(default)]
    pub permissions: PermissionsConfig,
    #[serde(default)]
    pub mcp_servers: BTreeMap<String, McpServerConfig>,
    #[serde(default)]
    pub tools: ToolsSection,
}

impl Config {
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let source = match secure_path::open_parent(path, false) {
            Ok((parent, leaf)) => {
                match secure_path::read_private_at(&parent, &leaf, MAX_CONFIG_BYTES) {
                    Ok(bytes) => String::from_utf8(bytes).map_err(|_| ConfigError::Io)?,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
                    Err(_) => return Err(ConfigError::Io),
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(_) => return Err(ConfigError::Io),
        };
        Self::from_toml_and_env(&source, env::vars())
    }

    pub fn load_at(parent: &fs::File) -> Result<Self, ConfigError> {
        let source =
            match secure_path::read_private_at(parent, OsStr::new("config.toml"), MAX_CONFIG_BYTES)
            {
                Ok(bytes) => String::from_utf8(bytes).map_err(|_| ConfigError::Io)?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
                Err(_) => return Err(ConfigError::Io),
            };
        Self::from_toml_and_env(&source, env::vars())
    }

    pub fn default_toml() -> &'static str {
        DEFAULT_TOML
    }

    pub fn to_lambo_file(&self) -> LamboFile {
        LamboFile {
            store: self.store.to_lambo(),
            embedder: self.embedder.to_lambo(),
            daemon: lambo::DaemonConfig {
                gc_interval: self.daemon.gc_interval,
                canonization_eval_interval_secs: self.daemon.canonization_eval_interval_secs,
            },
            // `None` = "leave the product default alone". Mooshik's policy is
            // Solo and `memory::resolve_product` stamps it after this, so it
            // stays the single authority; naming it here too would be a second
            // place to keep in step.
            promotion_policy: None,
        }
    }

    /// Fail closed on an `[mcp_servers.*]` entry that could never spawn: an
    /// empty command, or an env ref naming an impossible secret name. The
    /// `expose` allowlist being empty is *not* an error — such a server is
    /// simply inert (never spawned), which is M10's fail-closed default.
    pub fn validate_mcp(&self) -> Result<(), ConfigError> {
        for (name, config) in &self.mcp_servers {
            if name.contains('.') {
                // `mcp.<server>.<tool>` is ambiguous for a dotted server key —
                // fail config load rather than resolve silently to the wrong
                // server.
                return Err(ConfigError::InvalidMcp);
            }
            if config.command.trim().is_empty() {
                return Err(ConfigError::InvalidMcp);
            }
            for (env_var, secret_name) in &config.env {
                let valid_env = matches!(
                    env_var.as_bytes().first(),
                    Some(b'A'..=b'Z' | b'a'..=b'z' | b'_')
                ) && env_var
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_');
                if !valid_env || !crate::vault::is_valid_name(secret_name) {
                    return Err(ConfigError::InvalidMcp);
                }
            }
        }
        Ok(())
    }
}

pub fn resolve_home(
    environment: impl IntoIterator<Item = (String, String)>,
) -> Result<PathBuf, ConfigError> {
    let values: std::collections::HashMap<String, String> = environment.into_iter().collect();

    if let Some(path) = non_empty(&values, HOME_ENV) {
        return Ok(PathBuf::from(path));
    }
    values
        .get("HOME")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .ok_or(ConfigError::HomeUnavailable)
        .map(|path| path.join(".mooshik"))
}

fn default_session_id() -> String {
    DEFAULT_SESSION.to_owned()
}

fn default_agent_id() -> String {
    DEFAULT_AGENT.to_owned()
}

fn default_flush_interval_ms() -> u64 {
    DEFAULT_FLUSH_INTERVAL_MS
}

fn default_store_kind() -> StoreKind {
    StoreKind::Postgres
}

fn default_embedder_kind() -> EmbedderKind {
    EmbedderKind::Gemini
}

fn default_embed_dim() -> usize {
    DEFAULT_EMBED_DIM
}

fn default_gemini_location() -> Option<String> {
    Some(DEFAULT_GEMINI_LOCATION.to_owned())
}

fn default_gemini_model() -> Option<String> {
    Some(DEFAULT_GEMINI_MODEL.to_owned())
}

fn non_empty(values: &std::collections::HashMap<String, String>, key: &str) -> Option<String> {
    values.get(key).filter(|value| !value.is_empty()).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_home_is_an_error_instead_of_current_directory() {
        assert!(matches!(
            resolve_home([]),
            Err(ConfigError::HomeUnavailable)
        ));
    }

    #[test]
    fn default_toml_round_trips_to_product_defaults() {
        let parsed = Config::from_toml_and_env(Config::default_toml(), []).unwrap();
        assert_eq!(parsed, Config::default());
        assert!(Config::default_toml().contains("kind = \"postgres\""));
        assert!(!Config::default_toml().contains("dsn"));
        assert!(Config::default_toml().contains("[companion]"));
        assert!(!Config::default_toml().contains("api_key"));
        assert_eq!(parsed.companion.base_url, "http://127.0.0.1:8080/v1");
        assert_eq!(parsed.companion.model, "local-model");
        assert_eq!(parsed.companion.context_window, 32768);
        assert_eq!(parsed.companion.temperature, 0.2);
        assert_eq!(parsed.companion.api_key, None);
    }

    #[cfg(unix)]
    #[test]
    fn config_load_rejects_symlink_and_repairs_mode() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let root = crate::secure_path::canonical_temp_dir()
            .join(format!("mooshik-config-{}", std::process::id()));
        let outside = root.with_extension("outside");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_file(&outside);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&outside, Config::default_toml()).unwrap();
        let link = root.join("config.toml");
        symlink(&outside, &link).unwrap();
        assert!(matches!(Config::load(&link), Err(ConfigError::Io)));
        std::fs::remove_file(&link).unwrap();
        std::fs::write(&link, Config::default_toml()).unwrap();
        std::fs::set_permissions(&link, std::fs::Permissions::from_mode(0o644)).unwrap();
        Config::load(&link).unwrap();
        assert_eq!(
            std::fs::metadata(&link).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_file(outside);
    }

    #[cfg(unix)]
    #[test]
    fn config_load_rejects_symlinked_parent() {
        use std::os::unix::fs::symlink;
        let root = crate::secure_path::canonical_temp_dir()
            .join(format!("mooshik-config-parent-{}", std::process::id()));
        let outside = root.with_extension("outside");
        let _ = std::fs::remove_file(&root);
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&outside);
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("config.toml"), Config::default_toml()).unwrap();
        symlink(&outside, &root).unwrap();
        assert!(matches!(
            Config::load(&root.join("config.toml")),
            Err(ConfigError::Io)
        ));
        let _ = std::fs::remove_file(&root);
        let _ = std::fs::remove_dir_all(&outside);
    }

    #[test]
    fn mcp_servers_parse_and_validate() {
        let toml = r#"
            [mcp_servers.github]
            command = "uvx"
            args = ["mcp-server-github"]
            env = { GITHUB_PERSONAL_ACCESS_TOKEN = "github-token" }
            expose = ["create_issue", "list_issues"]
        "#;
        let config = Config::from_toml_and_env(toml, []).unwrap();
        let github = config.mcp_servers.get("github").unwrap();
        assert_eq!(github.command, "uvx");
        assert_eq!(github.args, vec!["mcp-server-github"]);
        assert_eq!(
            github
                .env
                .get("GITHUB_PERSONAL_ACCESS_TOKEN")
                .map(String::as_str),
            Some("github-token")
        );
        assert_eq!(github.expose, vec!["create_issue", "list_issues"]);
    }

    #[test]
    fn a_dotted_server_key_fails_closed() {
        // `mcp.<server>.<tool>` is ambiguous for a dotted server name — fail
        // config load rather than resolve silently to the wrong server (P3-M10-1).
        let toml = r#"
            [mcp_servers."github.app"]
            command = "uvx"
            expose = ["create_issue"]
        "#;
        assert!(matches!(
            Config::from_toml_and_env(toml, []),
            Err(ConfigError::InvalidMcp)
        ));
    }

    #[test]
    fn mcp_missing_command_fails_closed() {
        let toml = r#"
            [mcp_servers.bad]
            command = ""
            expose = ["x"]
        "#;
        assert!(matches!(
            Config::from_toml_and_env(toml, []),
            Err(ConfigError::InvalidMcp)
        ));
    }

    #[test]
    fn mcp_invalid_env_ref_fails_closed() {
        let toml = r#"
            [mcp_servers.bad]
            command = "uvx"
            env = { "1X" = "not-a-valid-secret!" }
            expose = ["x"]
        "#;
        assert!(matches!(
            Config::from_toml_and_env(toml, []),
            Err(ConfigError::InvalidMcp)
        ));
    }

    #[test]
    fn empty_expose_is_legal_and_inert() {
        let toml = r#"
            [mcp_servers.silent]
            command = "uvx"
            expose = []
        "#;
        let config = Config::from_toml_and_env(toml, []).unwrap();
        assert!(!config.mcp_servers["silent"].exposed());
    }
}
