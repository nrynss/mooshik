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

## Packaging and Checksums

For each target:
- Strips debug symbols on Linux targets.
- Compresses the binary into a `.tar.gz` archive.
- Generates a SHA256 checksum file.
- Consolidates all checksums into `checksums.txt`.
- Attaches the archives and checksums directly to the GitHub Release.
