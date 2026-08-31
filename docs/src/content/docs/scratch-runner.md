---
title: Scratch Script Runner
description: Sandboxed execution of quick diagnostic scripts and calculations.
---

The scratch runner executes ad-hoc scripts in a sandboxed temporary directory to inspect workspace state.

## Sandboxing & Isolation

Mooshik isolates scratch script execution:
- Runs inside a private directory with mode 0700 permissions.
- Enforces strict execution timeouts.
- Intercepts and redacts environment variables containing vault secrets.
- Prevents scripts from modifying primary repository files unexpectedly.

## Permission Gating

By default, Mooshik prompts the user before executing any scratch script.

You can adjust permissions in `config.toml` to permit automated execution when needed.
