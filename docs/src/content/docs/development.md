---
title: Development & Testing
description: Build instructions, offline test suites, and repository architecture guidelines.
---

This guide covers building Mooshik from source and running test suites.

## Development Environment Setup

### Prerequisites

- **Rust:** Version 1.97.1 (managed via `rust-toolchain.toml`).
- **Python:** Python 3.10 or newer.
- **Linux Packages:** `libdbus-1-dev` and `pkg-config` for OS keyring integration.

```bash
sudo apt update
```

```bash
sudo apt install -y libdbus-1-dev pkg-config
```

## Running the Rust Test Suite

The Rust test suite covers configuration parsing, vault encryption, CLI subcommands, and `WriteLane` concurrency:

```bash
cargo test
```

Build the release binary:

```bash
cargo build --release
```

## Running Python Component Tests

All Python MCP servers and support packages include offline test suites with faked network seams:

### News MCP Server

```bash
pytest mcp-servers/news/tests -q
```

### Artifacts MCP Server

```bash
pytest mcp-servers/artifacts/tests -q
```

### Coder MCP Server

```bash
pytest mcp-servers/coder/tests -q
```

### Bootstrap Ingester

```bash
pytest ingester/tests -q
```

### Measurement Harness

```bash
pytest measurement/tests -q
```

## Engineering Conventions

- **Prose in TOML:** User-facing strings live in `src/text/en.toml` rather than Rust code. Missing or empty string keys fail unit tests.
- **File size discipline:** Source files maintain a soft target of roughly 600 lines including unit tests.
- **Hermetic tests:** Unit test suites do not require live network credentials or cloud connections.
