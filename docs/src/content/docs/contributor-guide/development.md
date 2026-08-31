---
title: Development & Testing
description: Codebase conventions, line budget limits, and quality gates.
---

We enforce strict quality gates to maintain reliability and performance.

## Prerequisites

- Rust 1.97.1 pinned in `rust-toolchain.toml`.
- Python 3.10+ with `pytest`.
- Node.js 20+ for documentation.

## Running Tests

Run the complete Rust test suite:

```sh
cargo test
```

Run Python offline MCP server tests:

```sh
pytest mcp-servers/artifacts/tests -q
pytest mcp-servers/news/tests -q
pytest mooshik-common/tests -q
```

## Quality Gates

Before submitting changes, ensure all checks pass:

```sh
cargo fmt --check
cargo clippy --all-targets --all-features
```

## Conventions

1. **TOML String Storage**: All user-facing strings live in `src/text/en.toml` rather than Rust source code.
2. **File Size Budget**: Files must target roughly 600 lines. CI fails if any Rust file exceeds 1000 lines.
3. **SHA Pinned Actions**: CI workflows pin all GitHub Actions by commit hash rather than tags.
