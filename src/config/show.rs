use std::collections::BTreeMap;

use lambo::{EmbedderKind, StoreKind};
use serde::Serialize;

use crate::text;
use super::Config;

/// The shipped local-default endpoint and model — the "local posture nobody
/// has running" state the missing report has to end. Mirrors
/// `cli::init_flow`'s constants.
const PLACEHOLDER_BASE_URL: &str = "http://127.0.0.1:8080/v1";
const LOCAL_MODEL: &str = "local-model";

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
    /// What a working setup still lacks, one bullet per item, in the order
    /// `mooshik init` would ask them.
    ///
    /// The resolved configuration (file plus environment overlay) is the
    /// judgment basis, exactly as the interactive flow judges: a value present
    /// only in the environment is "configured" for this machine. The bullets
    /// still lead with the durable fix, because an environment value is the
    /// "worked before I rebooted" failure the milestone exists to end.
    pub fn missing_config(&self) -> Vec<String> {
        let mut missing = Vec::new();
        match self.store.kind {
            StoreKind::Postgres | StoreKind::Cockroach => {
                let has_dsn = self
                    .store
                    .dsn
                    .as_deref()
                    .map(str::trim)
                    .is_some_and(|value| !value.is_empty())
                    || self
                        .store
                        .dsn_secret
                        .as_deref()
                        .map(str::trim)
                        .is_some_and(|value| !value.is_empty());
                if !has_dsn {
                    missing.push(text::get("config.missing_store_dsn").to_owned());
                }
            }
            StoreKind::Sqlite => {
                if self
                    .store
                    .path
                    .as_deref()
                    .map(str::trim)
                    .is_none_or(|value| value.is_empty())
                {
                    missing.push(text::get("config.missing_store_path").to_owned());
                }
            }
            // `memory` is a test double that keeps nothing; nothing to report.
            StoreKind::Memory => {}
        }
        if self.embedder.kind == EmbedderKind::Gemini {
            if self
                .embedder
                .gemini_project
                .as_deref()
                .map(str::trim)
                .is_none_or(|value| value.is_empty())
            {
                missing.push(text::get("config.missing_gemini_project").to_owned());
            }
            if self
                .embedder
                .gemini_credentials
                .as_ref()
                .is_none_or(|path| path.as_os_str().is_empty())
            {
                missing.push(text::get("config.missing_gemini_credentials").to_owned());
            }
        }
        if self.companion.auth == super::CompanionAuth::Google {
            if self
                .companion
                .google_project
                .as_deref()
                .map(str::trim)
                .is_none_or(|value| value.is_empty())
            {
                missing.push(text::get("config.missing_companion_project").to_owned());
            }
            if self
                .companion
                .google_credentials
                .as_ref()
                .is_none_or(|path| path.as_os_str().is_empty())
            {
                missing.push(text::get("config.missing_companion_credentials").to_owned());
            }
        }
        if self.companion.auth == super::CompanionAuth::Static
            && (self.companion.base_url.trim().trim_end_matches('/') == PLACEHOLDER_BASE_URL
                || self.companion.model == LOCAL_MODEL)
        {
            missing.push(text::get("config.missing_companion_endpoint").to_owned());
        }
        missing
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
    #[test]
    fn missing_config_reports_the_default_postgres_store() {
        let missing = Config::default().missing_config();
        let joined = missing.join("\n");
        assert!(joined.contains("store DSN"), "{joined}");
        assert!(joined.contains("mooshik secret set store-dsn"), "{joined}");
        assert!(joined.contains("store.dsn_secret"), "{joined}");
    }

    #[test]
    fn missing_config_names_only_what_is_actually_unset() {
        let config = Config::from_toml_and_env(
            "[store]\nkind = 'postgres'\ndsn_secret = 'store-dsn'\n\
             [embedder]\nkind = 'gemini'\ngemini_project = 'proj'\n\
             [companion]\nauth = 'google'\ngoogle_project = 'proj'\ngoogle_credentials = '/k.json'\n",
            [],
        )
        .unwrap();
        let missing = config.missing_config();
        let joined = missing.join("\n");
        assert!(!joined.contains("store DSN"), "{joined}");
        assert!(!joined.contains("gemini_project"), "{joined}");
        assert!(!joined.contains("google_project"), "{joined}");
        // The credentials path is still unset, and it is the key M12h added
        // to the settable surface so a guided run can write it.
        assert!(joined.contains("gemini_credentials"), "{joined}");
    }

    #[test]
    fn missing_config_reports_the_local_posture_sqlite_path() {
        let config = Config::from_toml_and_env(
            "[store]\nkind = 'sqlite'\n[embedder]\nkind = 'bge_m3'\n",
            [],
        )
        .unwrap();
        let missing = config.missing_config();
        let joined = missing.join("\n");
        assert!(joined.contains("store.path"), "{joined}");
        assert!(!joined.contains("store DSN"), "{joined}");
        assert!(!joined.contains("gemini"), "{joined}");
    }

    #[test]
    fn missing_config_flags_the_placeholder_companion() {
        // The shipped default is static auth at the placeholder endpoint —
        // the "local posture nobody has running" state — and the missing
        // report must say so, or `init` and `config show` disagree about
        // the same file.
        let missing = Config::default().missing_config();
        let joined = missing.join("\n");
        assert!(joined.contains("companion.base_url/model"), "{joined}");
    }

    #[test]
    fn missing_config_does_not_flag_a_real_static_endpoint() {
        let config = Config::from_toml_and_env(
            "[companion]\nbase_url = 'https://my-llm.example/v1'\nmodel = 'my-model'\n",
            [],
        )
        .unwrap();
        let missing = config.missing_config();
        let joined = missing.join("\n");
        assert!(!joined.contains("companion.base_url/model"), "{joined}");
    }
}

