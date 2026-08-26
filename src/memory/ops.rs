use lambo::{mcp::ServeOptions, Memory, MemoryStats, RecallQuery, RecallResult};

use super::{
    resolve::{resolve_product, resolve_store},
    MemoryError,
};
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

/// Idempotent store DDL. Does not construct an embedder.
pub async fn provision(config: &Config) -> Result<(), MemoryError> {
    let store = resolve_store(config)?;
    lambo::GraphStore::init_schema(store.as_ref())
        .await
        .map_err(lambo::LamboError::Store)?;
    Ok(())
}

/// Recall defaults for the one-shot `mooshik recall` command: a small page of
/// the most relevant concepts, no graph expansion. Chat's recall injection has
/// its own budget; this is the operator-facing read path.
pub const RECALL_TOP_K: usize = 5;
pub const RECALL_MAX_TOKENS: usize = 200;
pub const RECALL_TRAVERSAL_DEPTH: usize = 0;

/// Search the session's concept graph for the operator. Opens and closes its
/// own [`Memory`] handle: `recall` is one-shot, not a long-lived session.
///
/// This is the local-operator read path, deliberately NOT routed through chat's
/// egress redaction: nothing recalled here reaches a model or history, so a
/// vault value that happens to match concept text stays visible to the person
/// who owns the machine.
pub async fn recall(config: &Config, query: String) -> Result<RecallResult, MemoryError> {
    let memory = open(config).await?;
    let outcome = memory
        .recall(RecallQuery {
            query,
            top_k: RECALL_TOP_K,
            max_tokens: RECALL_MAX_TOKENS,
            traversal_depth: RECALL_TRAVERSAL_DEPTH,
        })
        .await;
    let closed = memory.close().await;
    let recalled = outcome?;
    closed?;
    Ok(recalled)
}

/// Session health for the one-shot `mooshik stats` command. Same contract as
/// [`recall`]: local-operator output only.
pub async fn stats(config: &Config) -> Result<MemoryStats, MemoryError> {
    let memory = open(config).await?;
    let stats = memory.stats();
    memory.close().await?;
    Ok(stats)
}

/// What `serve` will ask Lambo to run. Extracted so tests can pin session,
/// transport, and endpoint publication without blocking on MCP stdio.
#[derive(Debug)]
pub struct ServePlan {
    pub session: String,
    pub agent: String,
    pub transport: lambo::mcp::Transport,
    pub endpoint: Option<String>,
}

pub fn serve_plan(config: &Config) -> ServePlan {
    let opts = ServeOptions::new(config.session.id.clone(), config.session.agent.clone());
    let endpoint = lambo::mcp::SessionEndpoint::for_store(&opts.session, &config.store.to_lambo())
        .map(|endpoint| endpoint.published());
    ServePlan {
        session: opts.session,
        agent: opts.agent,
        transport: opts.transport,
        endpoint,
    }
}

/// Long-running holder: provision schema, then serve Lambo's MCP surface.
pub async fn serve(config: &Config) -> Result<(), MemoryError> {
    lambo::mcp::init_tracing();
    let plan = serve_plan(config);
    provision(config).await?;
    let backends = resolve_product(config)?;
    let opts = ServeOptions::new(plan.session, plan.agent);
    lambo::mcp::serve(opts, backends).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lambo::{
        graph::derive::ParentOf, ConceptType, EmbedderKind, PromotionPolicy, RecallQuery, StoreKind,
    };

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
            assert!(!_home.database.exists());
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
        first
            .derive(
                &[("mooshik-m2-round1-marker", ConceptType::Entity)],
                &ParentOf::none(),
            )
            .await
            .unwrap();
        assert!(!first.graph().read().is_empty());
        first.close().await.unwrap();
        let second = open(&config).await.unwrap();
        assert!(second.graph().read().is_empty());
        let recalled = second
            .recall(RecallQuery {
                query: "mooshik-m2-round1-marker".into(),
                top_k: 5,
                max_tokens: 200,
                traversal_depth: 0,
            })
            .await
            .unwrap();
        assert!(
            recalled.hits.is_empty(),
            "a new MemoryStore must not see the first handle's write"
        );
        second.close().await.unwrap();
    }

    #[tokio::test]
    async fn one_shot_recall_and_stats_run_against_fixture_memory() {
        let config = fixture_config();
        provision(&config).await.unwrap();

        // Same-handle recall finds what derive wrote: the keyword leg must see
        // a freshly derived concept without any flush in between.
        let memory = open(&config).await.unwrap();
        memory
            .derive(
                &[("m7 cli sweep marker", ConceptType::Entity)],
                &ParentOf::none(),
            )
            .await
            .unwrap();
        let live = memory
            .recall(RecallQuery {
                query: "m7 cli sweep marker".into(),
                top_k: RECALL_TOP_K,
                max_tokens: RECALL_MAX_TOKENS,
                traversal_depth: RECALL_TRAVERSAL_DEPTH,
            })
            .await
            .unwrap();
        assert!(
            live.hits
                .iter()
                .any(|hit| hit.content == "m7 cli sweep marker"),
            "{live:?}"
        );
        memory.close().await.unwrap();

        // The one-shot wrappers open their own handle and answer from whatever
        // the configured store holds — empty here, because an in-memory store
        // lives and dies with its handle (durable stores carry the graph).
        let recalled = recall(&config, "m7 cli sweep marker".to_owned())
            .await
            .unwrap();
        assert!(recalled.hits.is_empty());
        assert!(recalled.context.is_empty());

        let health = stats(&config).await.unwrap();
        assert_eq!(health.session.as_str(), "mooshik");
        assert_eq!(health.agent.as_str(), "mooshik");
        assert_eq!(health.concept_count, 0);
    }

    #[tokio::test]
    async fn provision_does_not_construct_an_embedder() {
        let mut config = Config::default();
        config.store.kind = StoreKind::Memory;
        config.embedder.kind = EmbedderKind::Gemini;
        config.embedder.dim = 1024;
        config.embedder.gemini_credentials = None;
        provision(&config).await.unwrap();
    }

    #[tokio::test]
    async fn provision_postgres_without_dsn_fails() {
        assert!(matches!(
            provision(&Config::default()).await,
            Err(MemoryError::MissingDsn)
        ));
    }

    #[test]
    fn serve_plan_is_stdio_without_an_endpoint_on_memory() {
        let plan = serve_plan(&fixture_config());
        assert_eq!(plan.session, "mooshik");
        assert_eq!(plan.agent, "mooshik");
        assert!(matches!(plan.transport, lambo::mcp::Transport::Stdio));
        assert!(plan.endpoint.is_none());
    }

    #[test]
    fn serve_plan_publishes_an_endpoint_for_postgres() {
        let mut config = Config::default();
        config.store.dsn = Some("postgres://app@localhost/mooshik".to_owned());
        let plan = serve_plan(&config);
        assert!(plan.endpoint.is_some(), "a shareable store must publish");
        assert!(matches!(plan.transport, lambo::mcp::Transport::Stdio));
    }

    #[test]
    fn open_does_not_call_endpoint_on_the_builder() {
        let src = include_str!("ops.rs");
        let open = src
            .split("pub async fn open")
            .nth(1)
            .unwrap()
            .split("pub async fn provision")
            .next()
            .unwrap();
        assert!(
            !open.contains("endpoint("),
            "open must not publish a session endpoint"
        );
    }

    fn redact_secrets(message: &str) -> String {
        let mut redacted = message.to_owned();
        for scheme in ["postgres://", "postgresql://"] {
            if let Some(start) = redacted.find(scheme) {
                let after = start + scheme.len();
                if let Some(at) = redacted[after..].find('@') {
                    redacted.replace_range(after..after + at, "***");
                }
            }
        }
        redacted
    }

    /// Operator-runnable: Cloud SQL + Vertex Gemini.
    ///
    ///     cargo test --locked --lib memory::ops::tests::live_postgres_and_gemini_round_trip \
    ///       -- --ignored --nocapture
    ///
    /// Needs `LAMBO_POSTGRES_DSN` and `GCP_LAMBO_CREDENTIALS` (or
    /// `GOOGLE_APPLICATION_CREDENTIALS`). Skips if either is unset.
    #[tokio::test]
    #[ignore = "live Cloud SQL + Vertex; needs LAMBO_POSTGRES_DSN and GCP credentials"]
    async fn live_postgres_and_gemini_round_trip() {
        let dsn = std::env::var("LAMBO_POSTGRES_DSN")
            .ok()
            .filter(|value| !value.is_empty());
        let creds = std::env::var("GCP_LAMBO_CREDENTIALS")
            .ok()
            .filter(|value| !value.is_empty())
            .or_else(|| {
                std::env::var("GOOGLE_APPLICATION_CREDENTIALS")
                    .ok()
                    .filter(|value| !value.is_empty())
            });
        if dsn.is_none() || creds.is_none() {
            eprintln!("skipping: set LAMBO_POSTGRES_DSN and GCP_LAMBO_CREDENTIALS");
            return;
        }
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let session = format!("mooshik-live-{stamp}");
        let marker = format!("mooshik live gemini marker {stamp}");
        let mut config = Config::from_toml_and_env("", std::env::vars()).unwrap();
        config.session.id = session;
        config.session.agent = "mooshik-live".to_owned();
        provision(&config).await.unwrap_or_else(|error| {
            panic!("provision: {}", redact_secrets(&error.to_string()));
        });
        let memory = open(&config).await.unwrap_or_else(|error| {
            panic!("open: {}", redact_secrets(&error.to_string()));
        });
        assert_eq!(memory.embedding_contract().kind, "gemini");
        assert_eq!(
            memory.embedding_contract().model.as_deref(),
            Some("gemini-embedding-001")
        );
        assert_eq!(memory.embedding_contract().dim, 1536);
        memory
            .derive(
                &[(marker.as_str(), ConceptType::Observation)],
                &ParentOf::none(),
            )
            .await
            .unwrap_or_else(|error| {
                panic!("derive: {}", redact_secrets(&error.to_string()));
            });
        memory.close().await.unwrap_or_else(|error| {
            panic!("close: {}", redact_secrets(&error.to_string()));
        });
        let reopened = open(&config).await.unwrap_or_else(|error| {
            panic!("reopen: {}", redact_secrets(&error.to_string()));
        });
        let recalled = reopened
            .recall(RecallQuery {
                query: marker.clone(),
                top_k: 5,
                max_tokens: 200,
                traversal_depth: 0,
            })
            .await
            .unwrap_or_else(|error| {
                panic!("recall: {}", redact_secrets(&error.to_string()));
            });
        assert!(
            !recalled.hits.is_empty(),
            "durable postgres+gemini write must survive reopen"
        );
        reopened.close().await.unwrap_or_else(|error| {
            panic!("final close: {}", redact_secrets(&error.to_string()));
        });
    }
}
