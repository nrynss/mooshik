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

The release pipeline builds optimized binaries for two target architectures, both natively on a standard runner:

1. **`x86_64-unknown-linux-gnu`**: Linux 64-bit on x86 processors.
2. **`aarch64-apple-darwin`**: macOS on Apple Silicon.

ARM Linux and Intel macOS carry no prebuilt binary. Users on those platforms
build from source, which the README covers.

ARM Linux is the one worth explaining. It needs `cross`, and the keyring
dependency reaches `libdbus-sys`, which wants libdbus built for the target
architecture inside the cross container. Nothing supplies it there, so that leg
fails to link. The publish guard then blocks the entire release over one
missing archive, which turns a single broken target into no release at all.

## Python Wheel Bundle

A third job runs in parallel with those two and builds the Python MCP servers. It builds the `news`, `artifacts`, and `coder` servers plus the shared `mooshik-common` package as wheels, and ships them as a **single** additional asset:

```
mooshik-python-<version>.tar.gz
```

One asset, not a copy embedded in each platform tarball. Every wheel is `py3-none-any`, so nothing about them is per-target, and duplicating identical bytes across archives would imply otherwise. This also keeps the platform tarballs at exactly their historical shape, one `mooshik` binary and nothing else, so the binary-only install path cannot regress.

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
