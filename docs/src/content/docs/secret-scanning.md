---
title: Secret Scanning
description: Whole-document credential scanning and egress redaction across tools.
---

Mooshik applies strict secret scanning at multiple stages to ensure sensitive tokens and private keys never reach model contexts or persistent memory graphs.

## Multi-Layer Scanning Architecture

```mermaid
flowchart TD
    subgraph Inbound ["Inbound Ingestion Gates"]
        Files["Workspace Files (.md, .txt)"] --> Gate1["Watcher Secret Gate"]
        Artifacts["Screenshots & Audio"] --> Gate2["Artifacts MCP Scanner"]
        Corpus["Historical Documents"] --> Gate3["Ingester Scanner"]
    end

    Gate1 -->|Secret Detected| Drop["Drop Entire Document"]
    Gate2 -->|Secret Detected| Drop
    Gate3 -->|Secret Detected| Drop

    Gate1 -->|Clean| Derive["Derive to Concept Graph"]
    Gate2 -->|Clean| Derive
    Gate3 -->|Clean| Derive

    subgraph Outbound ["Outbound Execution Gate"]
        ToolOut["Tool Output & Errors"] --> Redact["Egress Redactor (Replaces with [redacted])"]
        Redact --> Model["Companion Model / User Screen"]
    end
```

## Inbound Whole-Document Drop Policy

When scanning incoming documents or screenshots, Mooshik enforces a fail-closed **whole-document drop policy**.

If a credential pattern is detected anywhere within a file or extracted artifact, the entire document is dropped. Partial redaction is deliberately avoided on inbound data because corrupted fragments can still compromise security.

### Detected Token Patterns

The scanner checks for known credential signatures:
- PEM-encoded private key headers (`-----BEGIN RSA PRIVATE KEY-----`).
- AWS access key IDs (`AKIA...`).
- GitHub authentication tokens (`ghp_...`, `github_pat_...`).
- Slack API tokens (`xoxp-...`, `xoxb-...`).
- High-entropy assignment strings matching generic API key or password variables.
- Exact string matches against registered secrets in the local vault.

## Outbound Egress Redaction

Tool execution represents the primary path where secrets might escape (such as a shell command echoing environment variables or an API returning an error that contains a bearer token).

Before tool output is returned to the companion model or displayed to the operator, Mooshik compares the text against all known vault secrets. Any matching secret value is replaced with `[redacted]`.
