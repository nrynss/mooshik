# Mooshik bootstrap ingester — Cloud Run Job image.
#
# Multi-stage: builds the `mooshik` binary from THIS checkout, then a slim
# Python runtime with the ingester, the Cloud SQL Auth Proxy, and the fixture
# corpus baked in.
#
# One binary, deliberately. The writer speaks MCP to a `serve` child, and that
# child used to be a separately-installed `lambo` pinned by SHA — a second
# copy of the same code that could drift from the library Mooshik links. It
# did drift once: the image sat on a rev whose write params predated
# `event_time`, and because they are `deny_unknown_fields` that image would
# have rejected every historical write rather than ignoring the field.
#
# `mooshik serve` publishes lambo's whole MCP surface from the library this
# repo already compiles, so the skew is now impossible rather than merely
# tested for. It also stamps the Solo promotion policy in Rust, which is what
# lets a single-writer bootstrap promote at all.
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

FROM rust:1.97-slim AS mooshik-build

# libdbus is the OS keyring backend's link dependency (see the repo README).
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      pkg-config libssl-dev libdbus-1-dev ca-certificates git \
 && rm -rf /var/lib/apt/lists/*

WORKDIR /src
COPY rust-toolchain.toml Cargo.toml Cargo.lock ./
COPY src ./src
# --locked so the image cannot silently resolve a different lambo than the
# lockfile this repo tests against.
RUN cargo build --release --locked

FROM python:3.13-slim

RUN apt-get update \
 && apt-get install -y --no-install-recommends curl ca-certificates libdbus-1-3 \
 && rm -rf /var/lib/apt/lists/* \
 && curl -sS --fail -o /usr/local/bin/cloud-sql-proxy \
      https://storage.googleapis.com/cloud-sql-connectors/cloud-sql-proxy/v2.14.2/cloud-sql-proxy.linux.amd64 \
 && chmod +x /usr/local/bin/cloud-sql-proxy

COPY --from=mooshik-build /src/target/release/mooshik /usr/local/bin/mooshik

WORKDIR /app

# The shared package first: `ingester` declares mooshik-common as an exact
# pin, and pip must find it already installed rather than reaching for an
# index that has never heard of it. Copied and installed as its own layer so
# a change to the ingester alone does not rebuild it.
COPY mooshik-common ./mooshik-common
RUN pip install --no-cache-dir ./mooshik-common

COPY ingester/pyproject.toml ./
COPY ingester/README.md ./
COPY ingester/ingester ./ingester
COPY ingest-fixtures /corpus

# Replay each document's date from its filename onto its mtime. The build
# context arrives with every mtime set to checkout time, and the walker reads
# a file's historical event_time from its mtime — so without this the whole
# week collapses into one instant and no concept can ever recur.
RUN python3 /corpus/backdate.py /corpus

RUN pip install --no-cache-dir .

COPY ingester/deploy/entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh

ENTRYPOINT ["/entrypoint.sh"]
CMD ["--root", "/corpus"]
