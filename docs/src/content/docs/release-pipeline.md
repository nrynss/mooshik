---
title: Release Pipeline
description: Automated cross-platform release builds and binary distribution.
---

Mooshik automates release builds and distribution through GitHub Actions.

## Release Trigger

The release workflow triggers automatically whenever a new version tag (such as `v0.1.0`) is pushed to the repository.

```sh
git tag v0.1.0
git push origin v0.1.0
```

## Build Matrix

The release pipeline builds optimized binaries across four target architectures:

1. **`x86_64-unknown-linux-gnu`**: Linux 64-bit on x86 processors.
2. **`aarch64-unknown-linux-gnu`**: Linux 64-bit on ARM processors.
3. **`aarch64-apple-darwin`**: macOS Apple Silicon.
4. **`x86_64-apple-darwin`**: macOS Intel.

## Python Wheel Bundle

A fifth job runs in parallel with those four and builds the Python MCP servers. It builds the `news`, `artifacts`, and `coder` servers plus the shared `mooshik-common` package as wheels, and ships them as a **single** additional asset:

```
mooshik-python-<version>.tar.gz
```

One asset, not four copies embedded in the platform tarballs. Every wheel is `py3-none-any`, so nothing about them is per-target, and duplicating identical bytes across four archives would imply otherwise. This also keeps the platform tarballs at exactly their historical shape, one `mooshik` binary and nothing else, so the binary-only install path cannot regress.

The job vendors Mooshik's own four packages with `--no-deps`. Pip resolves the third-party pins (`mcp`, `google-genai`, `google-adk`) from PyPI on the user's machine at install time.

Before packaging, the job reads `entry_points.txt` out of the built wheels. It fails the release if any of the three console scripts is missing. A wheel that lost its entry point would install cleanly and then answer to no name in `[mcp_servers.*]`, which is the exact failure this asset exists to prevent.

## Packaging and Checksums

For each target:
- Strips debug symbols on Linux targets.
- Compresses the binary into a `.tar.gz` archive.
- Generates a SHA256 checksum file.
- Consolidates all checksums into `checksums.txt`.
- Attaches the archives and checksums directly to the GitHub Release.

The consolidation step matches on shape rather than name, so the same two lines gather the Python bundle and its `.sha256` alongside the platform tarballs. It then asserts **5 archives and 5 checksum lines** before publishing. Without that check, a job that silently produced nothing would surface as a failed checksum lookup on a user's machine, rather than as a failed release.
