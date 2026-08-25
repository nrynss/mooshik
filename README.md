# Mooshik

An ambient, local-first AI cowork partner and workspace orchestrator. Mooshik
runs continuously alongside you as a peer: it holds a lifelong memory of your
workspace, researches the web, connects to your tools over MCP, and hands heavy
code changes to a specialized coding agent.

- **Spec** (authority): [docs/SPEC.md](docs/SPEC.md)
- **Build plan**: [dev-diary/PLAN.md](dev-diary/PLAN.md)
- **License:** AGPLv3 · memory core (Lambo): Apache 2.0

## Status

Early — Phase 1 under construction. Milestones M0–M11 in `dev-diary/PLAN.md`.

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

## Conventions

- **User-facing strings live in TOML, not Rust source** — `src/text/en.toml`,
  resolved via dotted keys (`text::get("app.about")`). Localization later means
  another file with the same schema.
- **File-size discipline:** soft target ~600 lines per file including tests and
  doc text; CI fails past 1000.
- **CI actions are pinned to commit SHAs**, never tags.
