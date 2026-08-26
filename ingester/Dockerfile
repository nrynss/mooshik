# Mooshik bootstrap ingester — Cloud Run Job image.
#
# Multi-stage: builds the pinned `lambo` binary (the MCP writer's subprocess)
# from the same rev Mooshik consumes, then a slim Python runtime with the
# ingester, the Cloud SQL Auth Proxy, and the fixture corpus baked in.
#
# Build (from the repo root):
#   docker build -f ingester/Dockerfile -t ingester .
#
# Run locally against the real store:
#   docker run --rm \
#     -e MOOSHIK_POSTGRES_DSN="postgresql://lambo:PW@127.0.0.1:5432/lambo?sslmode=disable" \
#     -e GOOGLE_APPLICATION_CREDENTIALS=/creds/sa.json \
#     -v /path/to/sa.json:/creds/sa.json:ro \
#     ingester --root /corpus
#
# Deploy as a Cloud Run Job — see README.md "Cloud Run deployment".

FROM rust:1.97-slim AS lambo-build

RUN apt-get update \
 && apt-get install -y --no-install-recommends pkg-config libssl-dev ca-certificates git \
 && rm -rf /var/lib/apt/lists/*

RUN cargo install --git https://github.com/nrynss/lambo \
      --rev f90a66227b9bff52bdff23bfcffe38b2a1bf7541 \
      lambo \
      --no-default-features --features store-postgres,embed-gemini

FROM python:3.13-slim

RUN apt-get update \
 && apt-get install -y --no-install-recommends curl ca-certificates \
 && rm -rf /var/lib/apt/lists/* \
 && curl -sS --fail -o /usr/local/bin/cloud-sql-proxy \
      https://storage.googleapis.com/cloud-sql-connectors/cloud-sql-proxy/v2.14.2/cloud-sql-proxy.linux.amd64 \
 && chmod +x /usr/local/bin/cloud-sql-proxy

COPY --from=lambo-build /usr/local/cargo/bin/lambo /usr/local/bin/lambo

WORKDIR /app

COPY ingester/pyproject.toml ./
COPY ingester/README.md ./
COPY ingester/ingester ./ingester
COPY ingest-fixtures /corpus

RUN pip install --no-cache-dir .

COPY ingester/deploy/entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh

ENTRYPOINT ["/entrypoint.sh"]
CMD ["--root", "/corpus"]
