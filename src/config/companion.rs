use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use super::ConfigError;

pub const COMPANION_BASE_URL_ENV: &str = "MOOSHIK_COMPANION_BASE_URL";
pub const COMPANION_MODEL_ENV: &str = "MOOSHIK_COMPANION_MODEL";
pub const COMPANION_API_KEY_ENV: &str = "MOOSHIK_COMPANION_API_KEY";
pub const COMPANION_CONTEXT_WINDOW_ENV: &str = "MOOSHIK_COMPANION_CONTEXT_WINDOW";
pub const COMPANION_TEMPERATURE_ENV: &str = "MOOSHIK_COMPANION_TEMPERATURE";
pub const COMPANION_AUTH_ENV: &str = "MOOSHIK_COMPANION_AUTH";
pub const COMPANION_GOOGLE_PROJECT_ENV: &str = "MOOSHIK_COMPANION_GOOGLE_PROJECT";
pub const COMPANION_GOOGLE_LOCATION_ENV: &str = "MOOSHIK_COMPANION_GOOGLE_LOCATION";

const DEFAULT_BASE_URL: &str = "http://127.0.0.1:8080/v1";
const DEFAULT_MODEL: &str = "local-model";
const DEFAULT_CONTEXT_WINDOW: u32 = 32768;
const DEFAULT_TEMPERATURE: f64 = 0.2;
/// The one Vertex location every Gemini 3.x model is served from, and the one
/// whose endpoint is *not* host-prefixed. See `vertex_base_url`.
pub const GLOBAL_LOCATION: &str = "global";

/// Vertex region used when the Google posture names a project but no location.
///
/// `global`, not the embedder's region. Every Gemini 3.x flash model is served
/// from `global` only: asking for `gemini-3.7-flash` (or 3.6, or 3.5) in
/// `us-central1` returns `404 NOT_FOUND: Publisher model ... was not found or
/// your project does not have access to it`, verified live 2026-08-31. The
/// embedder keeps `us-central1` because `gemini-embedding-001` lives there —
/// the two models are in different places, so this is deliberately not the
/// same constant as the embedder's.
pub const DEFAULT_GOOGLE_LOCATION: &str = GLOBAL_LOCATION;

/// How the companion authenticates to its `/v1` endpoint.
///
/// The two postures are genuinely different in kind, which is why this is an
/// enum rather than "an api_key that happens to be a Google token": a static
/// key is minted once by a human and lives until they rotate it, while a
/// Google access token expires in about an hour and has to be re-minted for
/// every request that outlives it.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CompanionAuth {
    /// A fixed `Authorization: Bearer <api_key>` header, or no header at all
    /// when no key is configured. The local and generic OpenAI-compatible
    /// posture, and the default.
    #[default]
    Static,
    /// A Google OAuth access token minted from the credential file and
    /// refreshed ahead of expiry (`lambo::gcp_auth`).
    Google,
}

/// The OpenAI-compatible base URL Vertex serves, derived rather than pasted.
///
/// It is a pure function of project and location, so an operator supplies
/// those two facts and never a URL — one fewer thing to get subtly wrong in a
/// hand-edited file. `chat_completions_url` appends `/chat/completions`.
/// `global` is spelled differently from every region: the host is the bare
/// `aiplatform.googleapis.com`, while a region prefixes it. The path carries
/// `locations/global` either way. `https://global-aiplatform.googleapis.com`
/// is not an API host at all — it answers 404 with an HTML body, checked live
/// 2026-08-31 — so the prefix cannot simply be applied uniformly.
pub fn vertex_base_url(project: &str, location: &str) -> String {
    let host = if location == GLOBAL_LOCATION {
        "aiplatform.googleapis.com".to_owned()
    } else {
        format!("{location}-aiplatform.googleapis.com")
    };
    format!("https://{host}/v1beta1/projects/{project}/locations/{location}/endpoints/openapi")
}

/// Optional companion credential. Debug and Display never print the value.
#[derive(Clone, Deserialize, Serialize)]
#[serde(transparent)]
pub struct ApiKey(Zeroizing<String>);

impl ApiKey {
    pub fn new(value: impl Into<String>) -> Self {
        Self(Zeroizing::new(value.into()))
    }

    pub(crate) fn expose(&self) -> &str {
        self.0.as_str()
    }
}

impl PartialEq for ApiKey {
    fn eq(&self, other: &Self) -> bool {
        self.0.as_str() == other.0.as_str()
    }
}

impl std::fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("***REDACTED***")
    }
}

impl std::fmt::Display for ApiKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("***REDACTED***")
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompanionConfig {
    #[serde(default = "default_base_url")]
    pub base_url: String,
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<ApiKey>,
    /// The *name* of a vault secret holding the API key — never the key. The
    /// same reference shape `[mcp_servers.*.env]` already uses, so the CLI
    /// write path has somewhere to put a credential that is not this file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_secret: Option<String>,
    #[serde(default)]
    pub auth: CompanionAuth,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub google_project: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub google_location: Option<String>,
    /// Credential file for the Google posture. Unset falls back to lambo's own
    /// chain (`GCP_LAMBO_CREDENTIALS`, then `GOOGLE_APPLICATION_CREDENTIALS`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub google_credentials: Option<PathBuf>,
    #[serde(default = "default_context_window")]
    pub context_window: u32,
    #[serde(default = "default_temperature")]
    pub temperature: f64,
}

impl Default for CompanionConfig {
    fn default() -> Self {
        Self {
            base_url: default_base_url(),
            model: default_model(),
            api_key: None,
            api_key_secret: None,
            auth: CompanionAuth::Static,
            google_project: None,
            google_location: None,
            google_credentials: None,
            context_window: default_context_window(),
            temperature: default_temperature(),
        }
    }
}

impl CompanionConfig {
    /// The Vertex region for the Google posture: what was configured, or the
    /// product default.
    pub fn resolved_google_location(&self) -> &str {
        self.google_location
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(DEFAULT_GOOGLE_LOCATION)
    }

    /// The base URL actually used. Under the Google posture it is derived from
    /// project and location; otherwise it is the configured `base_url`, which
    /// keeps the local default (`http://127.0.0.1:8080/v1`) working untouched.
    pub fn resolved_base_url(&self) -> String {
        match (self.auth, self.google_project.as_deref()) {
            (CompanionAuth::Google, Some(project)) if !project.trim().is_empty() => {
                vertex_base_url(project.trim(), self.resolved_google_location())
            }
            _ => self.base_url.clone(),
        }
    }

    /// Fail closed on a Google posture that cannot produce an endpoint: the
    /// URL is derived from the project, so a missing project is not a runtime
    /// 404 to discover mid-chat, it is a configuration error to name now.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.auth == CompanionAuth::Google
            && !self
                .google_project
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty())
        {
            return Err(ConfigError::MissingGoogleProject);
        }
        if let Some(name) = &self.api_key_secret {
            if !crate::vault::is_valid_name(name) {
                return Err(ConfigError::InvalidApiKeySecret);
            }
        }
        Ok(())
    }
}

fn default_base_url() -> String {
    DEFAULT_BASE_URL.to_owned()
}

fn default_model() -> String {
    DEFAULT_MODEL.to_owned()
}

fn default_context_window() -> u32 {
    DEFAULT_CONTEXT_WINDOW
}

fn default_temperature() -> f64 {
    DEFAULT_TEMPERATURE
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, ConfigError};

    fn env(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect()
    }

    fn assert_local_defaults(config: &CompanionConfig) {
        assert_eq!(config.base_url, DEFAULT_BASE_URL);
        assert_eq!(config.model, DEFAULT_MODEL);
        assert_eq!(config.context_window, DEFAULT_CONTEXT_WINDOW);
        assert_eq!(config.temperature, DEFAULT_TEMPERATURE);
        assert_eq!(config.api_key, None);
    }

    #[test]
    fn empty_toml_companion_uses_local_defaults() {
        let config = Config::from_toml_and_env("", []).unwrap();
        assert_local_defaults(&config.companion);
        let missing = Config::from_toml_and_env("[vault]\nprovider = 'keyring'\n", []).unwrap();
        assert_local_defaults(&missing.companion);
        let empty_table = Config::from_toml_and_env("[companion]\n", []).unwrap();
        assert_local_defaults(&empty_table.companion);
    }

    #[test]
    fn partial_companion_table_keeps_default_base_url_and_window() {
        let config =
            Config::from_toml_and_env("[companion]\nmodel = 'hosted-model'\n", []).unwrap();
        assert_eq!(config.companion.model, "hosted-model");
        assert_eq!(config.companion.base_url, DEFAULT_BASE_URL);
        assert_eq!(config.companion.context_window, DEFAULT_CONTEXT_WINDOW);
        assert_eq!(config.companion.temperature, DEFAULT_TEMPERATURE);
        assert_eq!(config.companion.api_key, None);
    }

    #[test]
    fn unknown_companion_key_is_rejected() {
        assert!(matches!(
            Config::from_toml_and_env("[companion]\nfoo = 1\n", []),
            Err(ConfigError::InvalidToml)
        ));
    }

    #[test]
    fn companion_env_overlay_wins_and_empty_preserves_file() {
        let config = Config::from_toml_and_env(
            "[companion]\nbase_url = 'http://file.example/v1'\nmodel = 'file-model'\ncontext_window = 4096\ntemperature = 0.5\n",
            env(&[
                (COMPANION_BASE_URL_ENV, "http://env.example/v1"),
                (COMPANION_MODEL_ENV, "env-model"),
                (COMPANION_CONTEXT_WINDOW_ENV, "8192"),
                (COMPANION_TEMPERATURE_ENV, "0.9"),
                (COMPANION_API_KEY_ENV, "env-secret"),
            ]),
        )
        .unwrap();
        assert_eq!(config.companion.base_url, "http://env.example/v1");
        assert_eq!(config.companion.model, "env-model");
        assert_eq!(config.companion.context_window, 8192);
        assert_eq!(config.companion.temperature, 0.9);
        assert_eq!(
            config.companion.api_key.as_ref().map(ApiKey::expose),
            Some("env-secret")
        );

        let kept = Config::from_toml_and_env(
            "[companion]\nbase_url = 'http://file.example/v1'\nmodel = 'file-model'\ncontext_window = 4096\ntemperature = 0.5\napi_key = 'file-secret'\n",
            env(&[
                (COMPANION_BASE_URL_ENV, ""),
                (COMPANION_MODEL_ENV, ""),
                (COMPANION_CONTEXT_WINDOW_ENV, ""),
                (COMPANION_TEMPERATURE_ENV, ""),
                (COMPANION_API_KEY_ENV, ""),
            ]),
        )
        .unwrap();
        assert_eq!(kept.companion.base_url, "http://file.example/v1");
        assert_eq!(kept.companion.model, "file-model");
        assert_eq!(kept.companion.context_window, 4096);
        assert_eq!(kept.companion.temperature, 0.5);
        assert_eq!(
            kept.companion.api_key.as_ref().map(ApiKey::expose),
            Some("file-secret")
        );
    }

    #[test]
    fn garbage_companion_env_values_fail_closed() {
        assert!(matches!(
            Config::from_toml_and_env("", env(&[(COMPANION_CONTEXT_WINDOW_ENV, "nope")])),
            Err(ConfigError::InvalidNumber)
        ));
        assert!(matches!(
            Config::from_toml_and_env("", env(&[(COMPANION_TEMPERATURE_ENV, "inf")])),
            Err(ConfigError::InvalidNumber)
        ));
        assert!(matches!(
            Config::from_toml_and_env("", env(&[(COMPANION_TEMPERATURE_ENV, "nope")])),
            Err(ConfigError::InvalidNumber)
        ));
    }

    #[test]
    fn zero_context_window_fails_closed() {
        assert!(matches!(
            Config::from_toml_and_env("[companion]\ncontext_window = 0\n", []),
            Err(ConfigError::ZeroContextWindow)
        ));
        assert!(matches!(
            Config::from_toml_and_env("", env(&[(COMPANION_CONTEXT_WINDOW_ENV, "0")])),
            Err(ConfigError::ZeroContextWindow)
        ));
        let message = ConfigError::ZeroContextWindow.to_string();
        assert!(message.contains("greater than zero"));
    }

    #[test]
    fn api_key_never_appears_in_display_error_or_config_show() {
        let secret = "s3cret-companion-key";
        let config =
            Config::from_toml_and_env(&format!("[companion]\napi_key = '{secret}'\n"), []).unwrap();
        let shown = config.redacted_toml();
        assert!(shown.contains("***REDACTED***"), "{shown}");
        assert!(!shown.contains(secret), "{shown}");
        let debug = format!("{config:?}");
        assert!(!debug.contains(secret), "{debug}");
        assert!(debug.contains("***REDACTED***"), "{debug}");
        let key = config.companion.api_key.as_ref().unwrap();
        assert_eq!(key.to_string(), "***REDACTED***");
        assert!(!format!("{key:?}").contains(secret));
        assert!(!ConfigError::ZeroContextWindow.to_string().contains(secret));
        assert!(!crate::text::get("companion.http_status").contains(secret));
    }
}
