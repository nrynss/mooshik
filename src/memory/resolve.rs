use std::time::Duration;

use lambo::{PromotionPolicy, ResolvedBackends, StoreKind};

use super::MemoryError;
use crate::config::Config;

pub fn resolve_product(config: &Config) -> Result<ResolvedBackends, MemoryError> {
    if config.store.kind == StoreKind::Postgres
        && config
            .store
            .dsn
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_none()
    {
        return Err(MemoryError::MissingDsn);
    }
    let mut backends = lambo::resolve_backends(config.to_lambo_file())?;
    backends.config.promotion_policy = PromotionPolicy::Solo;
    backends.config.backend_flush_interval = Duration::from_millis(config.daemon.flush_interval_ms);
    backends.config.validate()?;
    Ok(backends)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, POSTGRES_DSN_ENV};
    use lambo::{EmbedderKind, StoreKind};

    fn fixture_config() -> Config {
        let mut config = Config::default();
        config.store.kind = StoreKind::Memory;
        config.embedder.kind = EmbedderKind::Fixture;
        config.embedder.dim = 1024;
        config
    }

    fn fake_gemini_credentials() -> (std::path::PathBuf, Config) {
        let dir = std::env::temp_dir().join(format!(
            "mooshik-gemini-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("sa.json");
        let body = serde_json::json!({
            "client_email": "test@example.com",
            "private_key": "not-used-until-token-mint",
            "token_uri": "https://oauth2.googleapis.com/token",
            "project_id": "proj",
        });
        std::fs::write(&path, body.to_string()).unwrap();
        let mut config = Config::default();
        config.store.kind = StoreKind::Memory;
        config.embedder.kind = EmbedderKind::Gemini;
        config.embedder.dim = 1536;
        config.embedder.gemini_project = Some("proj".to_owned());
        config.embedder.gemini_credentials = Some(path.clone());
        (dir, config)
    }

    #[test]
    fn product_resolve_is_solo_and_uses_configured_flush() {
        let mut config = fixture_config();
        config.daemon.flush_interval_ms = 2500;
        let backends = resolve_product(&config).unwrap();
        assert_eq!(backends.config.promotion_policy, PromotionPolicy::Solo);
        assert_eq!(
            backends.config.backend_flush_interval,
            Duration::from_millis(2500)
        );
        assert!(!backends.allow_embedding_mismatch);
        assert_eq!(backends.embedding.kind, "fixture");
        assert_eq!(backends.embedding.dim, 1024);
    }

    #[test]
    fn gemini_dim_outside_supported_set_fails_at_resolve() {
        use std::error::Error;
        let mut config = fixture_config();
        config.embedder.kind = EmbedderKind::Gemini;
        config.embedder.dim = 1024;
        let error = match resolve_product(&config) {
            Err(error) => error,
            Ok(_) => panic!("gemini dim 1024 must fail at resolve"),
        };
        let message = error.source().map(ToString::to_string).unwrap_or_default();
        assert!(
            message.contains("768") && message.contains("1536") && message.contains("3072"),
            "{message}"
        );
        assert!(message.contains("1024"), "{message}");
    }

    #[test]
    fn resolve_gemini_stamps_embedding_model() {
        let (dir, config) = fake_gemini_credentials();
        let backends = resolve_product(&config).unwrap();
        assert_eq!(backends.embedding.kind, "gemini");
        assert_eq!(
            backends.embedding.model.as_deref(),
            Some("gemini-embedding-001")
        );
        assert_eq!(backends.embedding.dim, 1536);
        assert_eq!(backends.config.promotion_policy, PromotionPolicy::Solo);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn postgres_without_dsn_fails_before_construction() {
        let config = Config::default();
        assert!(matches!(
            resolve_product(&config),
            Err(MemoryError::MissingDsn)
        ));
        let message = MemoryError::MissingDsn.to_string();
        assert!(message.contains(POSTGRES_DSN_ENV), "{message}");
        assert!(!message.contains("postgres://"), "{message}");
    }

    #[test]
    fn error_source_is_available_for_backend_failures() {
        use std::error::Error;
        let mut config = fixture_config();
        config.embedder.kind = EmbedderKind::Gemini;
        config.embedder.dim = 1;
        let error = match resolve_product(&config) {
            Err(error) => error,
            Ok(_) => panic!("invalid gemini dim must fail at resolve"),
        };
        assert!(error.source().is_some());
    }
}
