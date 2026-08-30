use std::time::Duration;

use lambo::{PromotionPolicy, ResolvedBackends, StoreKind};

use super::MemoryError;
use crate::config::Config;
#[cfg(unix)]
use crate::secure_path;

pub fn resolve_product(config: &Config) -> Result<ResolvedBackends, MemoryError> {
    require_postgres_dsn(config)?;
    claim_local_store(config)?;
    let mut backends = lambo::resolve_backends(config.to_lambo_file())?;
    backends.config.promotion_policy = PromotionPolicy::Solo;
    backends.config.backend_flush_interval = Duration::from_millis(config.daemon.flush_interval_ms);
    backends.config.validate()?;
    Ok(backends)
}

/// Store-only construction for `init` / provision. Does not build an embedder.
pub fn resolve_store(config: &Config) -> Result<Box<dyn lambo::GraphStore>, MemoryError> {
    require_postgres_dsn(config)?;
    claim_local_store(config)?;
    lambo::store::build_store_with_vector_dim(
        config.store.to_lambo(),
        Some(config.embedder.dim).filter(|dim| *dim > 0),
    )
    .map_err(|error| lambo::LamboError::Config(error.to_string()).into())
}

/// Bring the local database into existence privately, before anything opens it.
///
/// The sqlite store is opened with `create_if_missing`, so on a first run the
/// file is created by sqlx and takes the process umask — `0644` on an ordinary
/// account. Everything else `mooshik init` writes is `0600`: the config, the
/// vault, the marker, the logs directory at `0700`. This one file holds
/// everything the user has ever remembered, and it was the only world-readable
/// thing in the home.
///
/// Creating it first, through the same primitive the config and the vault use,
/// means it never exists at a wider mode; reopening one that already exists
/// repairs it, which is what `init` does for the other files on every run.
/// SQLite gives the `-wal` and `-shm` side files the database's own mode, so the
/// two of them follow without being named here.
///
/// A store that names no local file — Postgres, and sqlite's in-memory
/// spellings — has nothing to claim. The in-memory grammar is sqlx's, mirrored
/// from `SqliteStore::is_in_memory_uri`: the database part is `:memory:`, or a
/// query parameter says `mode=memory`.
#[cfg(unix)]
fn claim_local_store(config: &Config) -> Result<(), MemoryError> {
    use std::{ffi::OsStr, path::Path};

    if config.store.kind != StoreKind::Sqlite {
        return Ok(());
    }
    let Some(target) = config
        .store
        .path
        .as_deref()
        .map(str::trim)
        .filter(|path| !path.is_empty())
    else {
        return Ok(());
    };
    let stripped = target
        .trim_start_matches("sqlite://")
        .trim_start_matches("sqlite:");
    let (database, params) = match stripped.split_once('?') {
        Some((database, query)) => (database, Some(query)),
        None => (stripped, None),
    };
    let in_memory = database == ":memory:"
        || params.is_some_and(|query| query.split('&').any(|param| param == "mode=memory"));
    if in_memory || database.is_empty() {
        return Ok(());
    }

    let file = |error: std::io::Error| -> MemoryError {
        // Through `Backend`, whose `Display` prints fixed advice and never its
        // source, so the path never reaches the terminal.
        lambo::LamboError::Config(format!("workspace database: {error}")).into()
    };
    let (parent, leaf) = secure_path::open_parent(Path::new(database), false).map_err(file)?;
    let leaf: &OsStr = &leaf;
    secure_path::ensure_private_file_at(&parent, leaf, b"").map_err(file)?;
    Ok(())
}

#[cfg(not(unix))]
fn claim_local_store(_config: &Config) -> Result<(), MemoryError> {
    Ok(())
}

fn require_postgres_dsn(config: &Config) -> Result<(), MemoryError> {
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
    Ok(())
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

    /// The spec promises a local posture — "Standalone, offline, no services
    /// to run", and a companion that "works on a plane". That is a **build**
    /// property: `StoreKind`/`EmbedderKind` accept every variant whatever this
    /// binary compiled, so dropping `store-sqlite` or `embed-bge` from the
    /// dependency's feature list would leave the config parsing happily and
    /// failing only at runtime, on a user's machine, as an internal error.
    ///
    /// Constructing both here is what makes the claim testable. Neither call
    /// touches the network: the sqlite store opens a file, and the BGE
    /// embedder only holds a URL until something asks it to embed.
    #[test]
    fn the_offline_backends_this_build_promises_are_actually_compiled_in() {
        let dir = crate::secure_path::canonical_temp_dir()
            .join(format!("mooshik-offline-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let mut config = Config::default();
        config.store.kind = StoreKind::Sqlite;
        config.store.path = Some(dir.join("mooshik.db").to_string_lossy().into_owned());
        config.embedder.kind = EmbedderKind::BgeM3;
        config.embedder.dim = 1024;

        // No DSN, no credentials, no network — and it must still resolve.
        let backends = resolve_product(&config).expect("offline backends resolve");
        assert_eq!(backends.embedding.dim, 1024);
        assert_eq!(backends.config.promotion_policy, PromotionPolicy::Solo);
        resolve_store(&config).expect("sqlite store builds");

        let _ = std::fs::remove_dir_all(&dir);
    }

    fn fake_gemini_credentials() -> (std::path::PathBuf, Config) {
        let dir = crate::secure_path::canonical_temp_dir().join(format!(
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
    fn open_path_still_constructs_the_embedder() {
        let mut config = fixture_config();
        config.embedder.kind = EmbedderKind::Gemini;
        config.embedder.dim = 1024;
        config.embedder.gemini_credentials = None;
        let error = match resolve_product(&config) {
            Err(error) => error,
            Ok(_) => panic!("open/serve resolve must still construct Gemini"),
        };
        use std::error::Error;
        let message = error.source().map(ToString::to_string).unwrap_or_default();
        assert!(
            message.contains("768")
                || message.contains("credentials")
                || message.contains("Gemini"),
            "{message}"
        );
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
