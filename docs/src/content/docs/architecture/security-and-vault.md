---
title: Security & Secret Vault
description: Encrypted vault storage, egress redaction, and pre-wire secret scanning.
---

Mooshik protects developer credentials and tokens through encryption, runtime redaction, and strict boundary scanning.

## Encrypted Local Vault

Mooshik stores credentials in a local encrypted vault file at `~/.mooshik/vault.bin`.

Key vault properties:
- Uses the OS keyring on Linux and macOS by default.
- Falls back to a passphrase-derived key when the keyring is unavailable.
- Uses ChaCha20-Poly1305 authenticated encryption.
- Enforces strict file permissions with mode 0600 on Unix systems.

## Egress Redaction

Mooshik tracks all secret values loaded from the vault.

Before any tool output or context passes to the language model or external loggers, Mooshik replaces matched secret values with `***REDACTED***`.

## Pre-Wire Secret Scanning

Non-text artifacts like screenshots and audio notes can accidentally capture sensitive tokens or private keys.

Mooshik runs secret pattern detection on all extracted concepts before they cross the tool boundary:
- PEM certificates and private key blocks
- AWS access keys (`AKIA...`)
- GitHub personal access tokens (`ghp_...`, `github_pat_...`)
- Slack API tokens (`xox...`)
- Generic high-entropy assignment patterns
- Injected vault secrets

If the scanner detects any secret, it drops the entire artifact immediately. No partial content or corrupted fragments enter the graph.
