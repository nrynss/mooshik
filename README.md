# Mooshik

An ambient, local-first AI cowork partner and workspace orchestrator. Mooshik
runs continuously alongside you as a peer: it holds a lifelong memory of your
workspace, researches the web, connects to your tools over MCP, and hands heavy
code changes to a specialized coding agent.

- **Spec** (authority): [docs/SPEC.md](docs/SPEC.md)
- **Build plan**: [dev-diary/PLAN.md](dev-diary/PLAN.md)
- **License:** AGPLv3 · memory core (Lambo): Apache 2.0

## Status

Early — Phase 1 under construction. Milestones through M12d are in `dev-diary/PLAN.md`.

`mooshik tui` watches the current working directory for live changes while its pane is open.
Only `.md`, `.markdown`, `.txt`, and `.rst` files outside Git repositories are eligible; generated
directories and symlinks are excluded. File contents are secret-scanned but only the relative path
is remembered. Git repositories contribute commit metadata and author time, never working-tree
files or diffs. The watcher stops with the pane.

## Build

Requires Rust 1.97.1 (pinned in `rust-toolchain.toml`; rustup installs it).
On Linux, the OS keyring backend needs D-Bus headers:

```sh
sudo apt install libdbus-1-dev pkg-config   # Debian/Ubuntu
```

```sh
cargo build          # debug binary at target/debug/mooshik
cargo test           # unit tests live alongside their code
./target/debug/mooshik --help
```

## Backends

Which backends exist is a **build** decision; which one runs is a config one
(`~/.mooshik/config.toml`). This binary compiles both postures:

| Posture | `[store]` | `[embedder]` | Needs |
| --- | --- | --- | --- |
| **Local** | `sqlite` | `bge_m3` | a llama.cpp server; nothing else |
| **Shared** | `postgres` | `gemini` | a Postgres DSN + Vertex credentials |

The defaults written by `mooshik init` are the shared pair, because that is
what a desktop and a laptop flushing into one memory needs. For a standalone
machine, set `kind = "sqlite"` with a `path`, point `[embedder]` and
`[companion]` at local servers, and nothing leaves the box.

`memory` / `fixture` are compiled too, but they are **test doubles** — the
memory store keeps nothing across a restart and the fixture embedder's
vectors carry no meaning. Never point a real workspace at them.

Changing embedder mid-session means re-embedding: the embedding contract
(kind + model + dim) is stamped per session and a mismatched vector space is
refused rather than silently mixed.

## Conventions

- **User-facing strings live in TOML, not Rust source** — `src/text/en.toml`,
  resolved via dotted keys (`text::get("app.about")`). Localization later means
  another file with the same schema.
- **File-size discipline:** soft target ~600 lines per file including tests and
  doc text; CI fails past 1000.
- **CI actions are pinned to commit SHAs**, never tags.
