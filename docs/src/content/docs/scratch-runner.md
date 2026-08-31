---
title: Scratch Script Runner
description: Execute sandboxed Python and Bash helper scripts with timeouts and egress redaction.
---

Mooshik includes a built-in sandbox runner (`run_scratch_script`) that allows the companion to execute throwaway Python or Bash helper scripts.

## Sandboxed Execution

The scratch runner executes scripts under strict runtime limits:

- **Isolated working directory:** Scripts run in a temporary directory with `0700` permissions.
- **15-second execution timeout:** Long-running or hung commands terminate automatically.
- **Process isolation:** Scripts run as isolated child processes without interactive input streams.

## Permissions Policy

By default, scratch script execution requires interactive operator confirmation:

```toml
[permissions]
scratch = "prompt"
```

When set to `"prompt"`, Mooshik displays the generated script code and target interpreter in the terminal and waits for confirmation before execution.

## Injecting Vault Secrets

You can pass sensitive credentials to scratch scripts as environment variables without writing tokens to disk:

```toml
[tools.scratch.env]
GITHUB_TOKEN = "github-token"
DATABASE_PASSWORD = "db-password"
```

The key is the environment variable name exposed to the script. The value is the secret name stored in the local encrypted vault. Mooshik resolves the secret value at execution time.

## Egress Redaction

Tool output is where credentials can escape. Before stdout or stderr text is returned to the companion model, Mooshik scans the output against all active vault values.

Any matching token is replaced with `[redacted]` before reaching the language model or being recorded into memory.
