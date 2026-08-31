---
title: Permissions and Grants
description: Manage tool permission grants, execution boundaries, and immutable security rules.
---

Mooshik has no vague trust levels or autonomous modes. Its capabilities are strictly determined by explicit tool grants defined in `~/.mooshik/config.toml` and enforced at the tool execution boundary.

## Default Grants on a Fresh Install

On a clean installation, Mooshik enforces conservative defaults:

1. **Memory tools (`allow`):** `lambo_recall`, `lambo_derive`, and `lambo_stats` execute automatically.
2. **Scratch runner (`prompt`):** `run_scratch_script` requires operator confirmation before running.
3. **All other tools (`deny`):** External MCP tools and unlisted capabilities are denied by default.

## Configuring Tool Grants

Define grants under `[permissions]` in `~/.mooshik/config.toml`:

```toml
[permissions]
memory = ["recall", "derive"]
scratch = "prompt"
"mcp.news.*" = "allow"
"mcp.coder.*" = "prompt"
"mcp.coder.check" = "allow"
```

### Grant Modes

- **`"allow"`:** Executes the tool immediately without prompting.
- **`"prompt"`:** Prompts the operator for interactive approval in the terminal before execution.
- **`"deny"`:** Rejects tool execution immediately.

### Wildcards and Overrides

You can grant permissions using prefix wildcards (`"mcp.news.*"`). Specific tool grants override broader wildcard rules.

In the example above, `"mcp.coder.*"` requires confirmation for code delegation, while `"mcp.coder.check"` is allowed to poll process liveness without prompting.

## The Memory Invariant

> [!IMPORTANT]
> The memory graph is never a permission authority. No concept, regardless of how canonical or load-bearing it is, can expand or alter a tool grant. Grants are defined exclusively in `config.toml` and evaluated in Rust before tool dispatch.

## Inspecting Active Permissions

View active grants from your terminal:

```bash
mooshik permissions
```

Prints the resolved grant decision for every registered in-process and MCP tool.
