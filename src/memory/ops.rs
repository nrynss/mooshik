use lambo::{mcp::ServeOptions, Memory};

use super::{resolve::resolve_product, MemoryError};
use crate::config::Config;

/// Open an in-process [`Memory`] for the configured session.
///
/// Does not bind a session endpoint: that belongs to [`serve`], which both
/// publishes the lease address and actually listens on it.
pub async fn open(config: &Config) -> Result<Memory, MemoryError> {
    let backends = resolve_product(config)?;
    let session = config.session.id.clone();
    let agent = config.session.agent.clone();
    let lambo_config = backends.config.clone();
    Ok(Memory::builder()
        .session(session)
        .agent(agent)
        .config(lambo_config)
        .backends(backends)
        .build()
        .await?)
}

/// Idempotent store DDL. Memory stores succeed without a DSN; Postgres needs one.
pub async fn provision(config: &Config) -> Result<(), MemoryError> {
    let backends = resolve_product(config)?;
    lambo::GraphStore::init_schema(backends.store.as_ref())
        .await
        .map_err(lambo::LamboError::Store)?;
    Ok(())
}

/// Long-running holder: provision schema, then serve Lambo's MCP surface.
pub async fn serve(config: &Config) -> Result<(), MemoryError> {
    lambo::mcp::init_tracing();
    let backends = resolve_product(config)?;
    lambo::GraphStore::init_schema(backends.store.as_ref())
        .await
        .map_err(lambo::LamboError::Store)?;
    let opts = ServeOptions::new(config.session.id.clone(), config.session.agent.clone());
    lambo::mcp::serve(opts, backends).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lambo::{EmbedderKind, PromotionPolicy, StoreKind};

    fn fixture_config() -> Config {
        let mut config = Config::default();
        config.store.kind = StoreKind::Memory;
        config.embedder.kind = EmbedderKind::Fixture;
        config.embedder.dim = 1024;
        config.session.id = "mooshik".to_owned();
        config.session.agent = "mooshik".to_owned();
        config
    }

    #[tokio::test]
    async fn fixture_memory_provisions_and_opens() {
        let root = std::env::temp_dir().join(format!("mooshik-memory-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let _home = crate::home::HomeLayout::new(&root);
        #[cfg(unix)]
        {
            _home.init().unwrap();
        }
        let config = fixture_config();
        provision(&config).await.unwrap();
        let memory = open(&config).await.unwrap();
        assert_eq!(memory.session().as_str(), "mooshik");
        assert_eq!(memory.agent().as_str(), "mooshik");
        assert_eq!(memory.config().promotion_policy, PromotionPolicy::Solo);
        assert_eq!(memory.embedding_contract().kind, "fixture");
        assert_eq!(memory.embedding_contract().dim, 1024);
        memory.close().await.unwrap();
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn second_open_on_the_same_memory_store_is_a_new_graph() {
        let config = fixture_config();
        provision(&config).await.unwrap();
        let first = open(&config).await.unwrap();
        first.close().await.unwrap();
        let second = open(&config).await.unwrap();
        assert_eq!(second.embedding_contract().kind, "fixture");
        second.close().await.unwrap();
    }
}
