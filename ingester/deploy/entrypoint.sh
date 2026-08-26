#!/usr/bin/env bash
# Cloud Run Job entrypoint: bring up the Cloud SQL Auth Proxy (IAM via the
# attached service account, or GOOGLE_APPLICATION_CREDENTIALS if present),
# wait for it, then hand off to the ingester pipeline.
set -euo pipefail

CREDS_FLAG=()
if [[ -n "${GOOGLE_APPLICATION_CREDENTIALS:-}" ]]; then
  CREDS_FLAG=(--credentials-file "$GOOGLE_APPLICATION_CREDENTIALS")
fi

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
