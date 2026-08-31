---
title: Installation
description: Install prebuilt Mooshik binaries or build from source.
---

Install Mooshik using the shell installer or build the binary from source.

## One-Line Shell Installer

Run the installer in your terminal on x86_64 Linux or Apple Silicon macOS:

```bash
curl -fsSL https://raw.githubusercontent.com/nrynss/mooshik/main/install.sh | sh
```

The script verifies downloaded archives against `checksums.txt` and installs two components.

### 1. The `mooshik` binary

The binary is placed in `~/.local/bin/mooshik`. This step is required. If binary installation fails, the script exits immediately with an error.

Ensure `~/.local/bin` is in your `PATH`:

```bash
export PATH="$HOME/.local/bin:$PATH"
```

### 2. The Python MCP servers

The `news`, `artifacts`, and `coder` servers are written in Python. The installer creates a dedicated virtualenv:

```
~/.local/share/mooshik/venv
```

The installer pins dependencies (`mcp`, `google-genai`, `google-adk`) inside this dedicated virtualenv to avoid modifying system packages.

The installer finishes in five lines and directs you to `mooshik init`.

### Systems without Python 3.10+

If Python 3.10 or newer is missing, the installer installs the binary and exits cleanly with status 0. It reports that MCP servers were skipped.

Install Python 3.10+ and rerun the installer to add the servers. Rerunning is safe and updates the virtualenv in place.

### Installer environment overrides

| Variable | Default | Purpose |
| :--- | :--- | :--- |
| `INSTALL_DIR` | `~/.local/bin` | Target directory for the `mooshik` binary. |
| `MOOSHIK_VENV_DIR` | `~/.local/share/mooshik/venv` | Target virtualenv directory for MCP servers. |
| `MOOSHIK_PYTHON` | `python3` | Python interpreter used to create the virtualenv. |
| `MOOSHIK_SKIP_PYTHON` | *(unset)* | Set to `1` to skip Python MCP server installation. |
| `MOOSHIK_VERSION` | *(latest)* | Explicit version tag to install. |
| `MOOSHIK_BASE_URL` | *(GitHub Releases)* | Base download URL for release assets. |

## Prebuilt Release Targets

Releases publish prebuilt native binaries for two platforms:
- `x86_64-unknown-linux-gnu` (64-bit x86 Linux)
- `aarch64-apple-darwin` (Apple Silicon macOS)

Every release also publishes `mooshik-python-<version>.tar.gz`, containing wheels for `mooshik-common`, `news`, `artifacts`, and `coder`.

## Build From Source

To compile Mooshik from source, install Rust 1.97.1 using rustup.

### Linux prerequisites

On Debian and Ubuntu, install the D-Bus development headers for OS keyring access:

```bash
sudo apt update
```

```bash
sudo apt install -y libdbus-1-dev pkg-config
```

### Compiling the binary

Clone the repository and build the release binary:

```bash
git clone https://github.com/nrynss/mooshik.git
```

```bash
cd mooshik
```

```bash
cargo build --release
```

Copy the compiled binary into your `PATH`:

```bash
cp target/release/mooshik ~/.local/bin/
```

### Building MCP servers from source

Create a virtualenv and install the local packages:

```bash
python3 -m venv ~/.local/share/mooshik/venv
```

```bash
~/.local/share/mooshik/venv/bin/pip install ./mooshik-common
```

```bash
~/.local/share/mooshik/venv/bin/pip install ./mcp-servers/news ./mcp-servers/artifacts ./mcp-servers/coder
```

## Next Step

Run guided setup to configure your environment:

```bash
mooshik init
```
