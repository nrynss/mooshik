use lambo::{EmbedderKind, StoreKind};
use serde::Serialize;

use super::Config;

#[derive(Serialize)]
struct ShowConfig<'a> {
    vault: &'a super::VaultConfig,
    session: &'a super::SessionConfig,
    store: ShowStore,
    embedder: ShowEmbedder<'a>,
    daemon: &'a super::DaemonConfig,
}

#[derive(Serialize)]
struct ShowStore {
    kind: StoreKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    dsn: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    vector_dim: Option<usize>,
}

#[derive(Serialize)]
struct ShowEmbedder<'a> {
    kind: EmbedderKind,
    dim: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    gemini_project: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    gemini_location: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    gemini_model: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    gemini_credentials: Option<String>,
}

impl Config {
    /// Resolved config as TOML, with any DSN replaced so `config show` cannot leak it.
    pub fn redacted_toml(&self) -> String {
        let store = ShowStore {
            kind: self.store.kind,
            dsn: self
                .store
                .dsn
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(|_| "***REDACTED***".to_owned()),
            path: self.store.path.clone().filter(|value| !value.is_empty()),
            vector_dim: self.store.vector_dim,
        };
        let embedder = ShowEmbedder {
            kind: self.embedder.kind,
            dim: self.embedder.dim,
            gemini_project: self
                .embedder
                .gemini_project
                .as_deref()
                .filter(|v| !v.is_empty()),
            gemini_location: self
                .embedder
                .gemini_location
                .as_deref()
                .filter(|v| !v.is_empty()),
            gemini_model: self
                .embedder
                .gemini_model
                .as_deref()
                .filter(|v| !v.is_empty()),
            gemini_credentials: self
                .embedder
                .gemini_credentials
                .as_ref()
                .map(|path| path.display().to_string())
                .filter(|value| !value.is_empty()),
        };
        toml::to_string_pretty(&ShowConfig {
            vault: &self.vault,
            session: &self.session,
            store,
            embedder,
            daemon: &self.daemon,
        })
        .expect("resolved config must serialize")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_show_redacts_a_configured_dsn() {
        let config = Config::from_toml_and_env(
            "[store]\nkind = 'postgres'\ndsn = 'postgres://user:s3cret@db.example/mooshik'\n",
            [],
        )
        .unwrap();
        let shown = config.redacted_toml();
        assert!(shown.contains("***REDACTED***"), "{shown}");
        assert!(!shown.contains("s3cret"), "{shown}");
        assert!(!shown.contains("postgres://user:"), "{shown}");
        assert!(!shown.contains("db.example"), "{shown}");
    }

    #[test]
    fn config_show_omits_dsn_when_unset() {
        let shown = Config::default().redacted_toml();
        assert!(!shown.contains("dsn"), "{shown}");
        assert!(shown.contains("postgres"), "{shown}");
        assert!(shown.contains("gemini"), "{shown}");
        assert!(shown.contains("1536"), "{shown}");
    }
}
