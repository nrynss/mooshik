---
title: Exit Codes & Failures
description: Understand Mooshik exit codes, errors, and failure handling.
---

Mooshik follows standardized process exit codes to communicate command outcomes clearly.

## Process Exit Codes

| Code | Meaning | Example Scenarios |
| :--- | :--- | :--- |
| `0` | Success | Normal command completion, clean exit from TUI. |
| `1` | Internal Failure | Backend error, network failure, unhandled crash. |
| `2` | User / Usage Error | Configuration syntax error, single-writer lease conflict. |

## Single-Writer Lease Conflicts

If you run a command against an open session already held by another process:
- Mooshik detects the active lease.
- It prints a clear conflict message identifying the holder.
- It exits immediately with status code `2`.
- It never overwrites or corrupts active database state.

## Safe Error Reporting

Mooshik prints only the top-level error summary to standard error.

It hides nested cause chains and connection details to prevent leaking passwords, tokens, or private endpoints into terminal logs.
