# Mooshik

Mooshik is an ambient, local-first AI cowork partner and workspace orchestrator. It runs continuously alongside you as a peer. It holds lifelong memory of your workspace, researches the web, and connects to your tools over MCP. When you need heavy code edits, Mooshik delegates them to specialized coding agents.

- **Documentation:** [https://nrynss.github.io/mooshik/](https://nrynss.github.io/mooshik/)
- **Authority Specification:** [docs/SPEC.md](docs/SPEC.md)
- **Build Plan:** [dev-diary/PLAN.md](dev-diary/PLAN.md)
- **License:** AGPLv3 (Application and Orchestration), Apache 2.0 (Lambo Memory Core)

---

## Architecture

```mermaid
graph TB
    subgraph Clients ["Clients"]
        TUI["Terminal UI (mooshik tui)"]
        CLI["CLI Commands"]
    end

    subgraph Core ["Mooshik Core Runtime"]
        Router["Event & Intent Router"]
        Companion["Companion Adapter (/v1)"]
        Scratch["Scratch Script Runner"]
        MCPHost["MCP Client & Tool Aggregator"]
        Watcher["Workspace & Git Watcher"]
    end

    subgraph Memory ["Lambo Graph Memory Substrate"]
        Graph["In-Memory Concept Graph"]
        WriteLane["WriteLane (Serialized Commits)"]
        Seam["GraphStore Seam"]
    end

    subgraph Vault ["Secret Vault"]
        V[("Local Encrypted Vault")]
    end

    subgraph Stores ["Persistent Stores"]
        SQLite[("Local SQLite")]
        PG[("Shared Postgres")]
    end

    subgraph MCPServers ["External MCP Servers"]
        News["News & Search Grounding"]
        Artifacts["Multimodal Artifact Ingestion"]
        Coder["Coding Contractor (Claude / OMP / Cursor / Antigravity)"]
        Tools["Filesystem & Dev Tools"]
    end

    Clients <--> Core
    Core --> Router
    Router --> Vault
    Core --> WriteLane --> Graph --> Seam --> Stores
    Core --> Watcher
    MCPHost <--> MCPServers
    Core -.->|Delegate Tasks| Coding["Coding Contractor Agent"]
```

---

## The Triad

Mooshik separates memory, orchestration, and coding execution into three clear roles.

| Role | Repository / Engine | Responsibility |
| :--- | :--- | :--- |
| **Mooshik** | `nrynss/mooshik` | Ambient awareness, fast local chat, research, MCP aggregation, and terminal UI. |
| **Lambo** | `nrynss/lambo` | Living graph memory substrate, in-memory graph, write-behind, and vector recall. |
| **Coding Contractor** | Delegated Agent | Surgical code edits in repositories under constraints provided by Lambo memory. |

---

## Installation

### One-Line Shell Installer

Run this command in your terminal on Linux or macOS:

```sh
curl -fsSL https://raw.githubusercontent.com/nrynss/mooshik/main/install.sh | sh
```

The script installs two things, and they fail independently.

1. **The `mooshik` binary**, into `~/.local/bin`. Required. If this step fails, the whole install fails.
2. **The three Python MCP servers** (`news`, `artifacts`, `coder`), into a virtualenv of their own at `~/.local/share/mooshik/venv`. Optional.

The virtualenv is not incidental. Mooshik pins its Python dependencies exactly (`mcp`, `google-genai`, `google-adk`), and exact pins in a shared `site-packages` silently break unrelated projects on the same machine. The virtualenv also gives each server a stable executable name. Your `~/.mooshik/config.toml` can then name `command = "~/.local/share/mooshik/venv/bin/mooshik-news-mcp"`, rather than an absolute path into a source checkout that a binary install never had.

The installer prints the exact `[mcp_servers.*]` block to paste, paths already filled in. Values under `[mcp_servers.*.env]` name vault **secrets**, they are not literal values. Store each one with `mooshik secret set <name>`.

**Without Python 3.10 or newer**, the script still installs the binary and exits 0. It names the three servers you are missing and says what each one does. Install Python and re-run the same command to add them later. Re-running is safe: the installer upgrades a working virtualenv in place instead of rebuilding it.

The Python step needs network access to PyPI. The release ships only Mooshik's own four packages. Pip resolves their third-party pins on your machine at install time.

**The coder server contains no coding agent.** It shells out to one. You install and authenticate that CLI yourself, whichever one you name in the server's `--agent` argument (Claude Code, OMP, Cursor Agent CLI, or Antigravity).

Overrides, all optional: `INSTALL_DIR`, `MOOSHIK_VENV_DIR`, `MOOSHIK_PYTHON`, `MOOSHIK_VERSION`, `MOOSHIK_SKIP_PYTHON=1`.

### Build from Source

Install Rust 1.97.1 using rustup.

On Linux, install required D-Bus development headers:

```sh
sudo apt install libdbus-1-dev pkg-config
```

Build and test the binary:

```sh
cargo build --release
cargo test
./target/release/mooshik --help
```

---

## Quickstart

Initialize your Mooshik workspace:

```sh
mooshik init
```

Open the terminal user interface:

```sh
mooshik tui
```

Search your memory graph:

```sh
mooshik recall "deployment checklist"
```

Consolidate memory and generate prose summaries:

```sh
mooshik reflect
```

---

## Core Capabilities

### Ambient Workspace Awareness
The workspace watcher observes file modifications and git commits. It extracts durable concepts automatically while the terminal UI stays open. Live watching is currently Unix-only. On unsupported non-Unix platforms it fails closed at TUI startup. The watcher stops with the pane.

### Terminal User Interface
The terminal UI renders weekly logs, active threads, ribbons, and notes. It rebuilds the view model on every 250 millisecond tick.

### Encrypted Local Secret Vault
Mooshik stores credentials in an encrypted local vault. It redacts secrets before tool calls reach external models or processes.

### Pre-Wire Secret Scanning
The artifact extractor scans extracted text for credentials and tokens. It drops the entire document when it detects a secret.

### Multimodal Artifact Ingestion
The artifacts MCP server processes screenshots and audio recordings. It extracts structured decisions, relations, and values without polluting the graph with visual captions.

### Coding Contractor Delegation
The coder MCP server delegates heavy repository edits to specialized coding agents (Claude Code, OMP, Cursor Agent, or Antigravity / agy) without blocking the companion. Tasks run under standing constraints (`AGENTS.md`) written from the memory graph, and the workspace watcher observes the ambient results.

---

## Storage and Embedder Postures

Mooshik supports two deployment postures through `~/.mooshik/config.toml`:

| Posture | Store Kind | Embedder Kind | Requirements |
| :--- | :--- | :--- | :--- |
| **Local** | `sqlite` | `bge_m3` | Local llama.cpp server. No data leaves your machine. |
| **Shared** | `postgres` | `gemini` | Postgres connection string and Vertex AI credentials. |

---

## CLI Reference

| Command | Description |
| :--- | :--- |
| `mooshik init` | Creates home directory layout and configuration files. |
| `mooshik tui` | Launches the interactive terminal user interface. |
| `mooshik chat` | Starts a command-line conversation session. |
| `mooshik recall <query>` | Searches the concept graph and returns relevant context. |
| `mooshik stats` | Displays graph node counts and session health metrics. |
| `mooshik reflect` | Runs consolidation and writes prose descriptions. |
| `mooshik config show` | Displays active configuration with redacted secrets. |
| `mooshik config set <key> <val>` | Updates a configuration setting. |
| `mooshik configure coder --agent <name>` | Configures coding contractor MCP block and vault secrets. |
| `mooshik secret set <name>` | Stores a secret in the encrypted local vault. |
| `mooshik permissions` | Lists all tool permission grants. |

---

## Documentation Site

Explore the full documentation in the `docs/` directory:

- [Product Overview](docs/src/content/docs/overview.md)
- [Installation & Releases](docs/src/content/docs/installation.md)
- [System Architecture](docs/src/content/docs/system-overview.md)
- [Memory & WriteLane Concurrency](docs/src/content/docs/writelane-concurrency.md)
- [MCP Servers & Tools](docs/src/content/docs/mcp-host.md)
- [CLI Reference](docs/src/content/docs/cli.md)
