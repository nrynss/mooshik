use std::collections::BTreeMap;

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
    companion: ShowCompanion<'a>,
    permissions: Option<&'a super::PermissionsConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<ShowTools<'a>>,
}

#[derive(Serialize)]
struct ShowCompanion<'a> {
    /// The base URL actually used, which under the Google posture is *derived*
    /// from project and location. `config show` prints the resolved
    /// configuration, so it prints the endpoint the next request will hit
    /// rather than the placeholder still sitting in the file.
    base_url: String,
    model: &'a str,
    auth: super::CompanionAuth,
    context_window: u32,
    temperature: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    api_key: Option<&'static str>,
    /// A secret *name* is configuration, not a secret; it prints as-is, the
    /// same way `[tools.scratch.env]` names do.
    #[serde(skip_serializing_if = "Option::is_none")]
    api_key_secret: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    google_project: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    google_location: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    google_credentials: Option<String>,
}

#[derive(Serialize)]
struct ShowTools<'a> {
    scratch: ShowScratchTools<'a>,
}

#[derive(Serialize)]
struct ShowScratchTools<'a> {
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    env: &'a BTreeMap<String, String>,
}

#[derive(Serialize)]
struct ShowStore {
    kind: StoreKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    dsn: Option<String>,
    /// A secret *name*, not a DSN — safe to print, and the whole point of the
    /// reference: `config show` can tell you *where* the connection string
    /// lives without telling you what it is.
    #[serde(skip_serializing_if = "Option::is_none")]
    dsn_secret: Option<String>,
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
            dsn_secret: self
                .store
                .dsn_secret
                .clone()
                .filter(|value| !value.is_empty()),
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
        let companion = ShowCompanion {
            base_url: self.companion.resolved_base_url(),
            model: &self.companion.model,
            auth: self.companion.auth,
            context_window: self.companion.context_window,
            temperature: self.companion.temperature,
            api_key: self
                .companion
                .api_key
                .as_ref()
                .map(|key| key.expose())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(|_| "***REDACTED***"),
            api_key_secret: self
                .companion
                .api_key_secret
                .as_deref()
                .filter(|value| !value.is_empty()),
            google_project: self
                .companion
                .google_project
                .as_deref()
                .filter(|value| !value.is_empty()),
            google_location: self
                .companion
                .google_location
                .as_deref()
                .filter(|value| !value.is_empty()),
            google_credentials: self
                .companion
                .google_credentials
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
            companion,
            permissions: (!self.permissions.entries.is_empty()).then_some(&self.permissions),
            // Secret *names* only; the values live in the vault.
            tools: (!self.tools.scratch.env.is_empty()).then_some(ShowTools {
                scratch: ShowScratchTools {
                    env: &self.tools.scratch.env,
                },
            }),
        })
        .expect("resolved config must serialize")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_show_includes_configured_permission_scopes() {
        let config =
            Config::from_toml_and_env("[permissions]\nmemory = ['recall']\nweb = 'deny'\n", [])
                .unwrap();
        let shown = config.redacted_toml();
        assert!(shown.contains("[permissions]"), "{shown}");
        assert!(shown.contains("memory"), "{shown}");
        assert!(shown.contains("recall"), "{shown}");
        assert!(shown.contains("web"), "{shown}");
    }

    #[test]
    fn config_show_omits_an_unconfigured_permissions_table() {
        let shown = Config::default().redacted_toml();
        assert!(!shown.contains("[permissions]"), "{shown}");
    }

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
        assert!(shown.contains("[companion]"), "{shown}");
        assert!(!shown.contains("api_key"), "{shown}");
    }

    #[test]
    fn config_show_redacts_api_key_and_never_contains_the_secret() {
        let secret = "s3cret-companion-key";
        let config =
            Config::from_toml_and_env(&format!("[companion]\napi_key = '{secret}'\n"), []).unwrap();
        let shown = config.redacted_toml();
        assert!(shown.contains("***REDACTED***"), "{shown}");
        assert!(!shown.contains(secret), "{shown}");
        assert!(shown.contains("api_key"), "{shown}");
    }

    #[test]
    fn config_show_includes_configured_scratch_env_names() {
        let config =
            Config::from_toml_and_env("[tools.scratch.env]\nGITHUB_TOKEN = 'github-token'\n", [])
                .unwrap();
        let shown = config.redacted_toml();
        assert!(shown.contains("[tools.scratch.env]"), "{shown}");
        assert!(shown.contains("GITHUB_TOKEN"), "{shown}");
        // Secret names are configuration, not secrets; they print as-is.
        assert!(shown.contains("github-token"), "{shown}");
    }

    #[test]
    fn config_show_omits_an_unconfigured_tools_section() {
        let shown = Config::default().redacted_toml();
        assert!(!shown.contains("[tools"), "{shown}");
    }
}
