---
title: Error Codes & Troubleshooting
description: Diagnostic exit codes and durable remediation steps for common error messages.
---

This page explains Mooshik's exit codes and diagnostic error messages.

## CLI Exit Codes

| Exit Code | Classification | Cause |
| :--- | :--- | :--- |
| `0` | Success | Command completed successfully. |
| `1` | Runtime Failure | Model timeout, network error, or tool execution failure. |
| `2` | Configuration Error | Missing settings, invalid arguments, or database session conflict. |

## Common Error Messages and Remediations

### "No Postgres DSN is configured"

- **Exit code:** 2
- **Cause:** `store.kind` is set to `postgres`, but no database connection string was provided.
- **Durable fix:** Store the DSN in the vault and link it in configuration:

```bash
mooshik secret set store-dsn
mooshik config set store.dsn_secret store-dsn
```

### "Workspace memory is held by another writer"

- **Exit code:** 2
- **Cause:** Another process currently holds the exclusive single-writer lease on this session.
- **Remediation:** Leave the conflicting session (`Esc` in `mooshik tui` or `Ctrl-C` in `mooshik serve`). If the process terminated unexpectedly, the lease expires automatically after a short timeout.

### "Changing store.kind requires confirmation"

- **Exit code:** 2
- **Cause:** Changing `store.kind` moves the active storage authority between SQLite and PostgreSQL.
- **Remediation:** Rerun the command with the confirmation flag:

```bash
mooshik config set store.kind postgres --confirm-database-change
```

### "Publisher model was not found or your project does not have access (404)"

- **Exit code:** 1
- **Cause:** Gemini 3.x Flash models are served exclusively from the `global` location on Vertex AI.
- **Remediation:** Ensure `companion.google_location` is set to `global`:

```bash
mooshik config set companion.google_location global
```

### "Setting would store a secret in config.toml"

- **Exit code:** 2
- **Cause:** Attempted to write a credential directly to a configuration key that expects a secret reference.
- **Remediation:** Store the secret in the vault with `mooshik secret set <name>`, then set the reference key.
