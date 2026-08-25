//! Configuration loading and the environment overlay.

use std::{
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
mod show;

pub use companion::{
    ApiKey, CompanionConfig, COMPANION_API_KEY_ENV, COMPANION_BASE_URL_ENV,
    COMPANION_CONTEXT_WINDOW_ENV, COMPANION_MODEL_ENV, COMPANION_TEMPERATURE_ENV,
};

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
base_url = "http://127.0.0.1:8080/v1"
model = "local-model"
context_window = 32768
temperature = 0.2
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
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let key = match self {
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
        };
        f.write_str(text::get(key))
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
            path: None,
            vector_dim: None,
        }
    }
}

impl StoreSection {
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
        }
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
        let root = std::env::temp_dir().join(format!("mooshik-config-{}", std::process::id()));
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
        let root =
            std::env::temp_dir().join(format!("mooshik-config-parent-{}", std::process::id()));
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
        let _ = std::fs::remove_file(root);
        let _ = std::fs::remove_dir_all(outside);
    }
}
