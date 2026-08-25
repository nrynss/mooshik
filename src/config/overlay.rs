use std::collections::HashMap;

use lambo::{store::FALLBACK_DSN_ENV, EmbedderKind, StoreKind};

use super::{
    non_empty, Config, ConfigError, VaultProvider, AGENT_ENV, EMBEDDER_ENV, EMBED_DIM_ENV,
    FLUSH_INTERVAL_ENV, GEMINI_CREDENTIALS_ENV, GEMINI_LOCATION_ENV, GEMINI_MODEL_ENV,
    GEMINI_PROJECT_ENV, POSTGRES_DSN_ENV, PROVIDER_ENV, SESSION_ENV, STORE_KIND_ENV,
};

const LAMBO_STORE_ENV: &str = "LAMBO_STORE";
const LAMBO_POSTGRES_DSN_ENV: &str = "LAMBO_POSTGRES_DSN";
const LAMBO_EMBEDDER_ENV: &str = "LAMBO_EMBEDDER";
const LAMBO_EMBED_DIM_ENV: &str = "LAMBO_EMBED_DIM";
const LAMBO_GEMINI_PROJECT_ENV: &str = "LAMBO_GEMINI_PROJECT";
const LAMBO_GEMINI_LOCATION_ENV: &str = "LAMBO_GEMINI_LOCATION";
const LAMBO_GEMINI_MODEL_ENV: &str = "LAMBO_GEMINI_MODEL";
const LAMBO_GEMINI_CREDENTIALS_ENV: &str = "LAMBO_GEMINI_CREDENTIALS";

impl Config {
    pub fn from_toml_and_env<I>(source: &str, environment: I) -> Result<Self, ConfigError>
    where
        I: IntoIterator<Item = (String, String)>,
    {
        let mut config = if source.trim().is_empty() {
            Self::default()
        } else {
            toml::from_str(source).map_err(|_| ConfigError::InvalidToml)?
        };
        let values: HashMap<String, String> = environment.into_iter().collect();
        overlay_vault(&mut config, &values)?;
        overlay_session(&mut config, &values);
        overlay_store_kind(&mut config, &values)?;
        overlay_embedder(&mut config, &values)?;
        overlay_dsn(&mut config, &values)?;
        overlay_flush(&mut config, &values)?;
        if config.daemon.flush_interval_ms == 0 {
            return Err(ConfigError::ZeroFlush);
        }
        Ok(config)
    }
}

fn overlay_vault(config: &mut Config, values: &HashMap<String, String>) -> Result<(), ConfigError> {
    if let Some(value) = non_empty(values, PROVIDER_ENV) {
        config.vault.provider = match value.to_ascii_lowercase().as_str() {
            "keyring" => VaultProvider::Keyring,
            "passphrase" => VaultProvider::Passphrase,
            _ => return Err(ConfigError::InvalidValue),
        };
    }
    Ok(())
}

fn overlay_session(config: &mut Config, values: &HashMap<String, String>) {
    if let Some(value) = non_empty(values, SESSION_ENV) {
        config.session.id = value;
    }
    if let Some(value) = non_empty(values, AGENT_ENV) {
        config.session.agent = value;
    }
}

fn overlay_store_kind(
    config: &mut Config,
    values: &HashMap<String, String>,
) -> Result<(), ConfigError> {
    if let Some(value) = non_empty(values, LAMBO_STORE_ENV) {
        config.store.kind = parse_store_kind(&value)?;
    }
    if let Some(value) = non_empty(values, STORE_KIND_ENV) {
        config.store.kind = parse_store_kind(&value)?;
    }
    Ok(())
}

fn overlay_embedder(
    config: &mut Config,
    values: &HashMap<String, String>,
) -> Result<(), ConfigError> {
    if let Some(value) = non_empty(values, LAMBO_EMBEDDER_ENV) {
        config.embedder.kind = parse_embedder_kind(&value)?;
    }
    if let Some(value) = non_empty(values, EMBEDDER_ENV) {
        config.embedder.kind = parse_embedder_kind(&value)?;
    }
    if let Some(value) = non_empty(values, LAMBO_EMBED_DIM_ENV) {
        config.embedder.dim = parse_usize(&value)?;
    }
    if let Some(value) = non_empty(values, EMBED_DIM_ENV) {
        config.embedder.dim = parse_usize(&value)?;
    }
    overlay_gemini(config, values, LAMBO_GEMINI_PROJECT_ENV, |cfg, value| {
        cfg.embedder.gemini_project = Some(value);
    });
    overlay_gemini(config, values, GEMINI_PROJECT_ENV, |cfg, value| {
        cfg.embedder.gemini_project = Some(value);
    });
    overlay_gemini(config, values, LAMBO_GEMINI_LOCATION_ENV, |cfg, value| {
        cfg.embedder.gemini_location = Some(value);
    });
    overlay_gemini(config, values, GEMINI_LOCATION_ENV, |cfg, value| {
        cfg.embedder.gemini_location = Some(value);
    });
    overlay_gemini(config, values, LAMBO_GEMINI_MODEL_ENV, |cfg, value| {
        cfg.embedder.gemini_model = Some(value);
    });
    overlay_gemini(config, values, GEMINI_MODEL_ENV, |cfg, value| {
        cfg.embedder.gemini_model = Some(value);
    });
    overlay_gemini(
        config,
        values,
        LAMBO_GEMINI_CREDENTIALS_ENV,
        |cfg, value| {
            cfg.embedder.gemini_credentials = Some(value.into());
        },
    );
    overlay_gemini(config, values, GEMINI_CREDENTIALS_ENV, |cfg, value| {
        cfg.embedder.gemini_credentials = Some(value.into());
    });
    Ok(())
}

fn overlay_gemini(
    config: &mut Config,
    values: &HashMap<String, String>,
    key: &str,
    apply: impl FnOnce(&mut Config, String),
) {
    if let Some(value) = non_empty(values, key) {
        apply(config, value);
    }
}

fn overlay_dsn(config: &mut Config, values: &HashMap<String, String>) -> Result<(), ConfigError> {
    if config.store.kind != StoreKind::Postgres {
        return Ok(());
    }
    let mooshik = non_empty(values, POSTGRES_DSN_ENV);
    let lambo =
        non_empty(values, LAMBO_POSTGRES_DSN_ENV).or_else(|| non_empty(values, FALLBACK_DSN_ENV));
    if let (Some(left), Some(right)) = (mooshik.as_deref(), lambo.as_deref()) {
        if !same_database(left, right) {
            return Err(ConfigError::DsnConflict);
        }
    }
    let file = config
        .store
        .dsn
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let (Some(file), Some(lambo), None) = (file, lambo.as_deref(), mooshik.as_deref()) {
        if !same_database(file, lambo) {
            return Err(ConfigError::DsnConflict);
        }
    }
    if let Some(value) = mooshik.or(lambo) {
        config.store.dsn = Some(value);
    }
    Ok(())
}

fn same_database(left: &str, right: &str) -> bool {
    lambo::store_dsn_identity(left) == lambo::store_dsn_identity(right)
}

fn overlay_flush(config: &mut Config, values: &HashMap<String, String>) -> Result<(), ConfigError> {
    if let Some(value) = non_empty(values, FLUSH_INTERVAL_ENV) {
        config.daemon.flush_interval_ms = parse_u64(&value)?;
    }
    Ok(())
}

fn parse_store_kind(value: &str) -> Result<StoreKind, ConfigError> {
    value.parse().map_err(|_| ConfigError::InvalidStoreKind)
}

fn parse_embedder_kind(value: &str) -> Result<EmbedderKind, ConfigError> {
    value.parse().map_err(|_| ConfigError::InvalidEmbedder)
}

fn parse_usize(value: &str) -> Result<usize, ConfigError> {
    value.parse().map_err(|_| ConfigError::InvalidNumber)
}

fn parse_u64(value: &str) -> Result<u64, ConfigError> {
    value.parse().map_err(|_| ConfigError::InvalidNumber)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::VaultProvider;
    use lambo::EmbedderKind;

    fn env(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect()
    }

    #[test]
    fn empty_toml_resolves_to_postgres_gemini_1536() {
        let config = Config::from_toml_and_env("", []).unwrap();
        assert_eq!(config.store.kind, StoreKind::Postgres);
        assert_eq!(config.store.dsn, None);
        assert_eq!(config.embedder.kind, EmbedderKind::Gemini);
        assert_eq!(config.embedder.dim, 1536);
        assert_eq!(
            config.embedder.gemini_model.as_deref(),
            Some("gemini-embedding-001")
        );
        assert_eq!(config.session.id, "mooshik");
        assert_eq!(config.session.agent, "mooshik");
        assert_eq!(config.daemon.flush_interval_ms, 1000);
    }

    #[test]
    fn non_empty_environment_value_wins() {
        let config = Config::from_toml_and_env(
            "[vault]\nprovider = 'passphrase'",
            env(&[(PROVIDER_ENV, "keyring")]),
        )
        .unwrap();
        assert_eq!(config.vault.provider, VaultProvider::Keyring);

        let config = Config::from_toml_and_env(
            "[session]\nid = 'file'\nagent = 'file'\n[store]\nkind = 'postgres'\n[embedder]\nkind = 'gemini'\ndim = 768\n[daemon]\nflush_interval_ms = 2000\n",
            env(&[
                (SESSION_ENV, "from-env"),
                (AGENT_ENV, "agent-env"),
                (STORE_KIND_ENV, "memory"),
                (EMBEDDER_ENV, "fixture"),
                (EMBED_DIM_ENV, "1024"),
                (FLUSH_INTERVAL_ENV, "1500"),
            ]),
        )
        .unwrap();
        assert_eq!(config.session.id, "from-env");
        assert_eq!(config.session.agent, "agent-env");
        assert_eq!(config.store.kind, StoreKind::Memory);
        assert_eq!(config.embedder.kind, EmbedderKind::Fixture);
        assert_eq!(config.embedder.dim, 1024);
        assert_eq!(config.daemon.flush_interval_ms, 1500);
    }

    #[test]
    fn empty_environment_value_preserves_file() {
        let config = Config::from_toml_and_env(
            "[vault]\nprovider = 'passphrase'",
            env(&[(PROVIDER_ENV, "")]),
        )
        .unwrap();
        assert_eq!(config.vault.provider, VaultProvider::Passphrase);

        let config = Config::from_toml_and_env(
            "[session]\nid = 'kept'\n[store]\nkind = 'memory'\n[embedder]\nkind = 'fixture'\ndim = 768\n[daemon]\nflush_interval_ms = 2500\n",
            env(&[
                (SESSION_ENV, ""),
                (STORE_KIND_ENV, ""),
                (EMBEDDER_ENV, ""),
                (EMBED_DIM_ENV, ""),
                (FLUSH_INTERVAL_ENV, ""),
                (POSTGRES_DSN_ENV, ""),
            ]),
        )
        .unwrap();
        assert_eq!(config.session.id, "kept");
        assert_eq!(config.store.kind, StoreKind::Memory);
        assert_eq!(config.embedder.kind, EmbedderKind::Fixture);
        assert_eq!(config.embedder.dim, 768);
        assert_eq!(config.daemon.flush_interval_ms, 2500);
        assert_eq!(config.store.dsn, None);
    }

    #[test]
    fn mooshik_env_wins_over_lambo_env_when_they_agree_on_intent() {
        let config = Config::from_toml_and_env(
            "",
            env(&[
                (LAMBO_STORE_ENV, "memory"),
                (STORE_KIND_ENV, "postgres"),
                (LAMBO_EMBEDDER_ENV, "fixture"),
                (EMBEDDER_ENV, "gemini"),
                (LAMBO_EMBED_DIM_ENV, "768"),
                (EMBED_DIM_ENV, "1536"),
            ]),
        )
        .unwrap();
        assert_eq!(config.store.kind, StoreKind::Postgres);
        assert_eq!(config.embedder.kind, EmbedderKind::Gemini);
        assert_eq!(config.embedder.dim, 1536);
    }

    #[test]
    fn unknown_and_malformed_values_are_rejected() {
        assert!(matches!(
            Config::from_toml_and_env("[other]\nx = 1", []),
            Err(ConfigError::InvalidToml)
        ));
        assert!(matches!(
            Config::from_toml_and_env("[vault]\nprovider = 'other'", []),
            Err(ConfigError::InvalidToml)
        ));
        assert!(matches!(
            Config::from_toml_and_env("[session]\nfoo = 1", []),
            Err(ConfigError::InvalidToml)
        ));
        assert!(matches!(
            Config::from_toml_and_env("[store]\nkind = 'postgres'\nunknown = 1", []),
            Err(ConfigError::InvalidToml)
        ));
        assert!(matches!(
            Config::from_toml_and_env("", env(&[(STORE_KIND_ENV, "nope")])),
            Err(ConfigError::InvalidStoreKind)
        ));
        assert!(matches!(
            Config::from_toml_and_env("", env(&[(EMBEDDER_ENV, "nope")])),
            Err(ConfigError::InvalidEmbedder)
        ));
        assert!(matches!(
            Config::from_toml_and_env("", env(&[(EMBED_DIM_ENV, "nope")])),
            Err(ConfigError::InvalidNumber)
        ));
    }

    #[test]
    fn dual_dsn_envs_that_disagree_fail_closed() {
        let error = Config::from_toml_and_env(
            "",
            env(&[
                (POSTGRES_DSN_ENV, "postgres://mooshik@localhost/db"),
                (LAMBO_POSTGRES_DSN_ENV, "postgres://lambo@localhost/db"),
            ]),
        )
        .unwrap_err();
        assert!(matches!(error, ConfigError::DsnConflict));
        assert!(!error.to_string().contains("postgres://"));
    }

    #[test]
    fn agreeing_dsn_envs_are_accepted() {
        let dsn = "postgres://app@localhost/db";
        let config = Config::from_toml_and_env(
            "",
            env(&[(POSTGRES_DSN_ENV, dsn), (LAMBO_POSTGRES_DSN_ENV, dsn)]),
        )
        .unwrap();
        assert_eq!(config.store.dsn.as_deref(), Some(dsn));
    }

    #[test]
    fn file_dsn_disagrees_with_lambo_env_fail_closed() {
        let error = Config::from_toml_and_env(
            "[store]\nkind = 'postgres'\ndsn = 'postgres://file@localhost/db'\n",
            env(&[(LAMBO_POSTGRES_DSN_ENV, "postgres://env@localhost/db")]),
        )
        .unwrap_err();
        assert!(matches!(error, ConfigError::DsnConflict));
        assert!(!error.to_string().contains("postgres://"));
    }

    #[test]
    fn zero_flush_interval_fails_closed() {
        assert!(matches!(
            Config::from_toml_and_env("[daemon]\nflush_interval_ms = 0\n", []),
            Err(ConfigError::ZeroFlush)
        ));
        assert!(matches!(
            Config::from_toml_and_env("", env(&[(FLUSH_INTERVAL_ENV, "0")])),
            Err(ConfigError::ZeroFlush)
        ));
    }

    #[test]
    fn lambo_postgres_dsn_fills_an_omitted_file_dsn() {
        let config = Config::from_toml_and_env(
            "",
            env(&[(LAMBO_POSTGRES_DSN_ENV, "postgres://lambo@localhost/db")]),
        )
        .unwrap();
        assert_eq!(
            config.store.dsn.as_deref(),
            Some("postgres://lambo@localhost/db")
        );
    }

    #[test]
    fn password_overlay_of_one_database_is_accepted() {
        let config = Config::from_toml_and_env(
            "[store]\ndsn = 'postgres://app@host/db'\n",
            env(&[(LAMBO_POSTGRES_DSN_ENV, "postgres://app:s3cret@host/db")]),
        )
        .unwrap();
        assert_eq!(config.store.kind, StoreKind::Postgres);
        assert_eq!(
            config.store.dsn.as_deref(),
            Some("postgres://app:s3cret@host/db")
        );
        let shown = config.redacted_toml();
        assert!(shown.contains("***REDACTED***"), "{shown}");
        assert!(!shown.contains("s3cret"), "{shown}");
    }

    #[test]
    fn omitted_postgres_port_is_the_same_database() {
        let config = Config::from_toml_and_env(
            "[store]\ndsn = 'postgres://u@host/db'\n",
            env(&[(LAMBO_POSTGRES_DSN_ENV, "postgres://u@host:5432/db")]),
        )
        .unwrap();
        assert_eq!(
            config.store.dsn.as_deref(),
            Some("postgres://u@host:5432/db")
        );
    }

    #[test]
    fn different_hosts_are_a_dsn_conflict_without_echoing_secrets() {
        let error = Config::from_toml_and_env(
            "[store]\ndsn = 'postgres://app:s3cret@host-a/db'\n",
            env(&[(LAMBO_POSTGRES_DSN_ENV, "postgres://app:hunter2@host-b/db")]),
        )
        .unwrap_err();
        assert!(matches!(error, ConfigError::DsnConflict));
        let message = error.to_string();
        assert!(!message.contains("s3cret"), "{message}");
        assert!(!message.contains("hunter2"), "{message}");
        assert!(!message.contains("postgres://"), "{message}");
    }

    #[test]
    fn empty_store_table_is_postgres() {
        let config = Config::from_toml_and_env("[store]\n", []).unwrap();
        assert_eq!(config.store.kind, StoreKind::Postgres);
        assert_eq!(config.store.dsn, None);
    }

    #[test]
    fn store_table_with_only_a_dsn_is_postgres() {
        let config = Config::from_toml_and_env(
            "[store]\ndsn = 'postgres://prod:s3cret@localhost/prod'\n",
            [],
        )
        .unwrap();
        assert_eq!(config.store.kind, StoreKind::Postgres);
        assert_eq!(
            config.store.dsn.as_deref(),
            Some("postgres://prod:s3cret@localhost/prod")
        );
        let shown = config.redacted_toml();
        assert!(!shown.contains("s3cret"), "{shown}");
        assert!(shown.contains("***REDACTED***"), "{shown}");
    }

    #[test]
    fn gemini_table_without_dim_uses_product_default() {
        let config = Config::from_toml_and_env("[embedder]\nkind = 'gemini'\n", []).unwrap();
        assert_eq!(config.embedder.kind, EmbedderKind::Gemini);
        assert_eq!(config.embedder.dim, 1536);
        assert_eq!(
            config.embedder.gemini_model.as_deref(),
            Some("gemini-embedding-001")
        );
        assert_eq!(
            config.embedder.gemini_location.as_deref(),
            Some("us-central1")
        );
    }
}
