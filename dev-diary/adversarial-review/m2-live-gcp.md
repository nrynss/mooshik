# M2 live Cloud SQL + Vertex (2026-08-25)

Operator run on the Lambo host. Credentials from the existing process env
(`LAMBO_POSTGRES_DSN`, `GCP_LAMBO_CREDENTIALS`); no secrets recorded here.

## What ran

1. `aiplatform.googleapis.com` was disabled on GCP project `mooshik`. Enabled
   it (`gcloud services enable aiplatform.googleapis.com --project=mooshik`).
2. `cargo test --locked --lib memory::ops::tests::live_postgres_and_gemini_round_trip -- --ignored`
   — **PASS** in 47.8s. Path: provision schema on Cloud SQL `lambo-pg` → open
   `Memory` with Vertex `gemini-embedding-001` dim 1536 → derive one observation
   → close → reopen → recall hits.
3. `mooshik init` in a fresh `MOOSHIK_HOME` (passphrase vault) against the same
   DSN — **PASS**. No dummy `mooshik.db`. `config show` prints `dsn = "***REDACTED***"`.

Default `cargo test` still skips the live test (`#[ignore]`). CI has no GCP
secrets and must not run it.

Identity: service account `cachy-nryn@mooshik.iam`, Cloud SQL instance
`lambo-pg` (Postgres 16 + pgvector, `us-central1-c`).
