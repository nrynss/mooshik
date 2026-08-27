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


# Canonization policy for the serve child. lambo defaults to Swarm, which
# promotes on independent agents converging — a bootstrap has one writer, so
# under Swarm it fills the graph and promotes nothing. Default to Solo and let
# an operator override; the writer's env allowlist passes this through.
export LAMBO_PROMOTION_POLICY="${LAMBO_PROMOTION_POLICY:-Solo}"

# Default serve command carries the ingester's own session id; operators can
# override with an explicit INGEST_LAMBO_SERVE (e.g. to match a holder).
if [[ -z "${INGEST_LAMBO_SERVE:-}" ]]; then
  INGEST_LAMBO_SERVE="/usr/local/bin/lambo serve --session ${INGEST_SESSION:-ingest-cloudrun}"
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

exec python3 -m ingester "$@"
