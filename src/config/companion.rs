use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

pub const COMPANION_BASE_URL_ENV: &str = "MOOSHIK_COMPANION_BASE_URL";
pub const COMPANION_MODEL_ENV: &str = "MOOSHIK_COMPANION_MODEL";
pub const COMPANION_API_KEY_ENV: &str = "MOOSHIK_COMPANION_API_KEY";
pub const COMPANION_CONTEXT_WINDOW_ENV: &str = "MOOSHIK_COMPANION_CONTEXT_WINDOW";
pub const COMPANION_TEMPERATURE_ENV: &str = "MOOSHIK_COMPANION_TEMPERATURE";

const DEFAULT_BASE_URL: &str = "http://127.0.0.1:8080/v1";
const DEFAULT_MODEL: &str = "local-model";
const DEFAULT_CONTEXT_WINDOW: u32 = 32768;
const DEFAULT_TEMPERATURE: f64 = 0.2;

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
            context_window: default_context_window(),
            temperature: default_temperature(),
        }
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
