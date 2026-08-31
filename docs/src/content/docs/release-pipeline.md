---
title: Release Pipeline
description: Overview of multi-target native binary builds, Python packaging, and GitHub Actions CI.
---

Mooshik automates release builds and packaging through GitHub Actions workflows.

## Release Artifacts

Every tagged release produces three primary distribution assets:

1. **`mooshik-<version>-x86_64-unknown-linux-gnu.tar.gz`:** Native 64-bit Linux binary.
2. **`mooshik-<version>-aarch64-apple-darwin.tar.gz`:** Native Apple Silicon macOS binary.
3. **`mooshik-python-<version>.tar.gz`:** Universal bundle containing prebuilt Python wheels for `mooshik-common`, `mooshik-news-mcp`, `mooshik-artifacts-mcp`, and `mooshik-coder-mcp`.

A signed `checksums.txt` file contains SHA-256 checksums for all published archives.

## Automated Verification

The release workflow performs automated checks before publishing:

- **Checksum verification:** The installer verifies SHA-256 digests against `checksums.txt` before unpacking binaries.
- **Cross-platform compilation:** Native runners compile release binaries on Ubuntu and macOS machines.
- **Documentation deployment:** GitHub Pages builds and publishes the Astro Starlight site located in `docs/`.
