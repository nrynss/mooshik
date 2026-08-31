---
title: Quickstart
description: Install Mooshik, run guided setup, and open the terminal interface.
---

Get up and running with Mooshik in three steps.

## 1. Install Mooshik

Run the installer to download the `mooshik` binary and set up the Python MCP servers:

```bash
curl -fsSL https://raw.githubusercontent.com/nrynss/mooshik/main/install.sh | sh
```

Verify the installation:

```bash
mooshik --help
```

## 2. Run Guided Setup

Run the interactive setup wizard:

```bash
mooshik init
```

The wizard guides you through choosing a posture, connecting storage, and configuring model endpoints. All secrets are stored directly in the local encrypted vault.

## 3. Launch the Terminal Pane

Navigate to the parent directory containing your repositories or notes:

```bash
cd ~/work
```

Launch the interface:

```bash
mooshik tui
```

### Why Directory Location Matters

Mooshik watches the directory where you launch it. There is no configuration key for the workspace root.

- **Launching from a project parent (`~/work`):** Optimal. Watches multiple project repositories efficiently.
- **Launching inside a single repository (`~/work/mooshik`):** Tracks changes within that single repository.
- **Launching from your home directory (`~`):** Avoid this. Walking your entire home directory exceeds the poll budget.

## Using the Pane

The pane is the primary way to interact with Mooshik.

- **Conversation input:** Type your question or task and press `Enter`. Tokens stream directly into the conversation view.
- **Cancel in-flight generation:** Press `Esc` to cancel a response without closing the interface.
- **Live timeline:** The view model updates every 250 milliseconds. File edits, commits, and reflections appear automatically.
- **Exit:** Press `Ctrl-C` or `q` to close the session.

## Secondary CLI Commands

Mooshik provides secondary commands for scripting and quick checks:

- [CLI Reference](/mooshik/cli/)
- Search memory: `mooshik recall "architecture decisions"`
- Command-line chat: `mooshik chat`
- Consolidate graph prose: `mooshik reflect`
- Check memory metrics: `mooshik stats`
