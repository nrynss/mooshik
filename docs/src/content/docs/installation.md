---
title: Installation & Releases
description: Install pre-compiled Mooshik binaries or build from source.
---

You can install Mooshik using our one-line installer script or build the binary directly from source.

## One-Line Shell Installer

Run the install script in your terminal to fetch the latest release for x86_64 Linux or Apple Silicon macOS:

```sh
curl -fsSL https://raw.githubusercontent.com/nrynss/mooshik/main/install.sh | sh
```

The script detects your operating system and CPU architecture, verifies every archive it downloads against the release's `checksums.txt`, and then installs **two** things.

### 1. The `mooshik` binary

The script places it in `~/.local/bin/mooshik`. This part is required. If it fails, the installer fails and installs nothing.

Ensure `~/.local/bin` exists in your shell PATH environment variable:

```sh
export PATH="$HOME/.local/bin:$PATH"
```

### 2. The Python MCP servers

The `news`, `artifacts`, and `coder` servers are Python, not Rust. A binary-only install cannot run any of them. The script installs all three, plus the shared `mooshik-common` package, into a virtualenv of their own:

```
~/.local/share/mooshik/venv
```

A dedicated virtualenv rather than the system interpreter or `pip install --user`, for two reasons. Mooshik pins its Python dependencies exactly (`mcp`, `google-genai`, `google-adk`), and exact pins in a shared `site-packages` silently break unrelated projects on the same machine. The virtualenv also gives each server a stable executable name on disk, so your configuration can address one directly:

```toml
[mcp_servers.news]
command = "/home/you/.local/share/mooshik/venv/bin/mooshik-news-mcp"
expose = ["search_news", "fetch_article"]
```

The installer prints the full `[mcp_servers.*]` block for all three servers when it finishes, paths already substituted. Paste it into `~/.mooshik/config.toml`. Two things to note. `expose` is an allowlist, and Mooshik never spawns a server whose allowlist is empty. Values under `[mcp_servers.*.env]` name vault **secrets**, they are not literal values, so store each one with `mooshik secret set <name>`.

The equivalent path-addressable invocation from a source checkout keeps working, unchanged:

```toml
command = "python3"
args = ["/abs/path/to/mooshik/mcp-servers/news/server.py"]
```

This step needs network access to PyPI. The release ships only Mooshik's own four packages as wheels. Pip resolves their third-party pins on your machine at install time.

### When Python is missing

If the machine has no `python3`, or has one older than 3.10, the installer skips the servers. It still installs the binary, and **it exits 0**. It names the three servers you do not have and says what each of them does.

To add them later, install Python 3.10 or newer and run the same one-liner again. Re-running is safe. The installer reuses a virtualenv that still works and upgrades it in place, never rebuilding it, so a failed second run cannot cost you a working first one.

### The coder server needs an agent you install yourself

The coder server takes one extra argument, naming the agent it delegates to:

```toml
[mcp_servers.coder]
command = "/home/you/.local/share/mooshik/venv/bin/mooshik-coder-mcp"
args = ["--agent", "claude"]
expose = ["delegate", "check"]

[mcp_servers.coder.env]
ANTHROPIC_API_KEY = "anthropic-api-key"
```

The agent name travels as an argument rather than an env value on purpose. Mooshik resolves everything under `[mcp_servers.*.env]` as a vault secret name, and an agent name is not a secret. Credentials still travel only through the environment, and only as secret names.

The server contains no coding agent of its own. It shells out to one. You install and authenticate that CLI yourself, whichever one you name (Claude Code, OMP, Cursor Agent CLI, or Antigravity), before `mcp.coder.delegate` can do anything.

### Installer environment overrides

All optional.

| Variable | Effect |
| :--- | :--- |
| `INSTALL_DIR` | Where the binary goes. Default `~/.local/bin`. |
| `MOOSHIK_VENV_DIR` | Where the virtualenv goes. Default `$XDG_DATA_HOME/mooshik/venv`. |
| `MOOSHIK_PYTHON` | The interpreter the script builds the virtualenv with. Default `python3`. |
| `MOOSHIK_SKIP_PYTHON=1` | Install the binary only. |
| `MOOSHIK_VERSION` | Install this version instead of querying for the latest. |
| `MOOSHIK_BASE_URL` | Where the release assets live. Accepts `file:///abs/dir`, which is how we test a locally built release before a tag exists. |

## Pre-Compiled Binary Releases

We publish standalone release archives on GitHub Releases for every version tag.

Supported targets include:
- `x86_64-unknown-linux-gnu` (Linux 64-bit x86)
- `aarch64-unknown-linux-gnu` (Linux 64-bit ARM)
- `aarch64-apple-darwin` (macOS Apple Silicon)
- `x86_64-apple-darwin` (macOS Intel)

Download the archive for your system, extract the `mooshik` binary, and place it in your system PATH.

Alongside those two, every release carries one further asset: `mooshik-python-<version>.tar.gz`, a flat archive of the four `py3-none-any` wheels. It is one asset rather than a copy per platform, because nothing about those wheels is platform-specific. `checksums.txt` covers it like everything else.

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

### The Python servers from source

A source build gives you the binary only. To get the MCP servers, install the four Python packages into a virtualenv yourself. `mooshik-common` is an exact pin that exists on no index, so install it first, from the checkout:

```sh
python3 -m venv .venv
./.venv/bin/pip install ./mooshik-common
./.venv/bin/pip install ./mcp-servers/news ./mcp-servers/artifacts ./mcp-servers/coder
```

That produces the same three console scripts the installer does (`.venv/bin/mooshik-news-mcp` and friends). The `server.py` files in the checkout also stay directly usable, via `python3 /abs/path/server.py`.
