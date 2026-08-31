---
title: The Vault & Security
description: Encrypted local secret storage, two-store separation, and egress redaction.
---

Mooshik enforces clear security boundaries to ensure sensitive credentials never leak into language models, prompts, or synchronized memory graphs.

## Two-Store Separation

Mooshik separates memory from secrets at the architectural level:

- **The Memory Graph:** Embedded, queryable, synced across machines, and readable by language models. It never stores credential values.
- **The Vault:** Encrypted, local only, never synchronized to external databases, and never embedded.

The memory graph may record that a secret handle exists (such as `secret://github/token`), which is safe autobiographical knowledge. The actual value resides strictly in the local vault and resolves only at tool execution time.

## Vault Providers

Configure the vault provider under `[vault]` in `~/.mooshik/config.toml`:

```toml
[vault]
provider = "keyring"   # "keyring" or "passphrase"
```

### 1. Keyring Provider (`provider = "keyring"`)

The default provider on desktop systems. It uses the operating system credential manager:
- **Linux:** Secret Service via D-Bus (`libdbus-1-dev`).
- **macOS:** Apple Keychain Services.

Secrets decrypt automatically for local user sessions without requiring master password prompts.

### 2. Passphrase Provider (`provider = "passphrase"`)

Suitable for headless servers, Docker containers, and CI environments where a system keyring is unavailable.

Set the master encryption passphrase via an environment variable:

```bash
export MOOSHIK_VAULT_PASSPHRASE="your-master-passphrase"
```

The vault file at `~/.mooshik/vault` is protected with strict `0600` file permissions.

## Managing Vault Secrets

Use the CLI to create and inspect secret handles:

### Store a Secret

```bash
mooshik secret set github-token
```

Prompts for the secret value with terminal echo disabled.

### Retrieve a Secret

```bash
mooshik secret get github-token
```

Prints the decrypted secret value directly to your terminal.

### List Stored Secrets

```bash
mooshik secret list
```

Prints the names of all registered secrets without exposing their values.

## Egress Redaction

Tool execution is the primary path where secrets could escape (such as a script echoing an environment variable).

Mooshik scans all tool output against known vault values before passing data to language models or writing observations to the graph. Any matching substring is replaced with `[redacted]`.
