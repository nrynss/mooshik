---
title: Installation & Releases
description: Install pre-compiled Mooshik binaries or build from source.
---

You can install Mooshik using our one-line installer script or build the binary directly from source.

## One-Line Shell Installer

Run the install script in your terminal to fetch the latest pre-compiled release for Linux or macOS:

```sh
curl -fsSL https://raw.githubusercontent.com/nrynss/mooshik/main/install.sh | sh
```

The script detects your operating system and CPU architecture. It downloads the verified release archive and places the binary in `~/.local/bin/mooshik`.

Ensure `~/.local/bin` exists in your shell PATH environment variable:

```sh
export PATH="$HOME/.local/bin:$PATH"
```

## Pre-Compiled Binary Releases

We publish standalone release archives on GitHub Releases for every version tag.

Supported targets include:
- `x86_64-unknown-linux-gnu` (Linux 64-bit x86)
- `aarch64-unknown-linux-gnu` (Linux 64-bit ARM)
- `aarch64-apple-darwin` (macOS Apple Silicon)
- `x86_64-apple-darwin` (macOS Intel)

Download the archive for your system, extract the `mooshik` binary, and place it in your system PATH.

## Build From Source

To compile Mooshik from source, install Rust 1.97.1 using rustup.

### Linux Prerequisites

On Ubuntu and Debian systems, install the D-Bus header packages for OS keyring access:

```sh
sudo apt update
sudo apt install -y libdbus-1-dev pkg-config
```

### Compiling with Cargo

Clone the repository and build the release binary:

```sh
git clone https://github.com/nrynss/mooshik.git
cd mooshik
cargo build --release
```

Move the compiled binary into your PATH:

```sh
cp target/release/mooshik ~/.local/bin/
```
