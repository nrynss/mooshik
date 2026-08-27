#!/usr/bin/env bash
# Cloud Run Job entrypoint: bring up the Cloud SQL Auth Proxy (IAM via the
# attached service account, or GOOGLE_APPLICATION_CREDENTIALS if present),
# wait for it, then hand off to the ingester pipeline.
set -euo pipefail

CREDS_FLAG=()

# On Cloud Run the SA json arrives as a secret env value (not a file), but
# both the auth proxy and lambo's Gemini embedder need a FILE path. Write it
# once, then point every credential variable at it.
if [[ -n "${MOOSHIK_GCP_CREDENTIALS:-}" ]]; then
  mkdir -p /tmp/creds
  printf '%s' "$MOOSHIK_GCP_CREDENTIALS" > /tmp/creds/sa.json
  export GOOGLE_APPLICATION_CREDENTIALS=/tmp/creds/sa.json
  export GCP_LAMBO_CREDENTIALS=/tmp/creds/sa.json
  export LAMBO_GEMINI_CREDENTIALS=/tmp/creds/sa.json
fi

if [[ -n "${GOOGLE_APPLICATION_CREDENTIALS:-}" ]]; then
  CREDS_FLAG=(--credentials-file "$GOOGLE_APPLICATION_CREDENTIALS")
fi


# The serve child is `mooshik serve`, not a separately-installed lambo: one
# binary, so the library Mooshik links and the server it talks to cannot
# drift. It also stamps the Solo promotion policy in Rust — a raw lambo would
# default to Swarm, which promotes on independent agents converging, and a
# bootstrap has exactly one writer.
#
# `mooshik serve` takes its session from configuration, not a flag.
export MOOSHIK_HOME="${MOOSHIK_HOME:-/tmp/mooshik-home}"
export MOOSHIK_SESSION="${MOOSHIK_SESSION:-${INGEST_SESSION:-ingest-cloudrun}}"

if [[ -z "${INGEST_LAMBO_SERVE:-}" ]]; then
  INGEST_LAMBO_SERVE="/usr/local/bin/mooshik serve"
  export INGEST_LAMBO_SERVE
fi
/usr/local/bin/cloud-sql-proxy mooshik:us-central1:lambo-pg --port 5432 "${CREDS_FLAG[@]}" &
PROXY_PID=$!
trap 'kill "$PROXY_PID" 2>/dev/null || true' EXIT

for i in $(seq 1 60); do
  (exec 3<>/dev/tcp/127.0.0.1/5432) 2>/dev/null && break || sleep 1
done
(exec 3<>/dev/tcp/127.0.0.1/5432) || {
  echo "cloud sql proxy did not open the port" >&2
  exit 1
}
echo "cloud sql proxy ready"

# `mooshik init` creates the home and provisions the store schema
# (idempotent). It needs the proxy up, so it runs here rather than at build
# time. It also creates a vault unconditionally, and there is no OS keyring in
# a container — so use the passphrase provider with a throwaway value. Nothing
# is ever stored in this vault: the DSN arrives by environment, and the serve
# child only opens a vault when configuration *references* one, which it does
# not. The passphrase is unset before the child is spawned, and the writer's
# env allowlist excludes it in any case.
export MOOSHIK_VAULT_PROVIDER=passphrase
MOOSHIK_VAULT_PASSPHRASE="$(head -c 32 /dev/urandom | base64 | tr -d '\n')"
export MOOSHIK_VAULT_PASSPHRASE
/usr/local/bin/mooshik init
unset MOOSHIK_VAULT_PASSPHRASE

exec python3 -m ingester "$@"
