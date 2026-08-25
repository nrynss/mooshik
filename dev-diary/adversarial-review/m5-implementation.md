# M5 implementation

The `[permissions]` grant table, enforced **in Rust at the tool-call boundary**
through one decorator in front of every tool, plus `mooshik permissions`
printing the resolved set. Autonomy is exactly the sum of grants; the
enforcement point is one place because a check duplicated per tool is a check
that will be forgotten by the fourth tool.

## Scope

- `src/config/permissions.rs` (new) — the grammar, fail-closed validation,
  resolution against the Decision 6 defaults, and the deterministic renderer.
- `src/config/mod.rs` — `Config.permissions` field (serde default), the
  `InvalidPermissions` error variant, a commented `[permissions]` block in
  `DEFAULT_TOML`.
- `src/config/overlay.rs` — `validate()` runs inside `from_toml_and_env`, so an
  uninterpretable table fails the whole config load.
- `src/config/show.rs` — permissions round-trip through `config show`; omitted
  when unconfigured.
- `src/tools/permissions.rs` (new) — `GatedTools`, the decorator that owns all
  enforcement.
- `src/tools/mod.rs` — `executor_for_chat` composes inner executor → gate;
  nothing else in the chat path changed.
- `src/tools/scratch.rs` — `ScratchConfig::always_confirmed()`: the M4 confirm
  seam stays but is pinned open under chat, because prompting moved to the gate.
- `src/companion/loop_tests.rs` — the composition pin through `Session::turn`.
- `src/cli.rs` — the `permissions` subcommand.
- `src/text/en.toml` — `[permissions]` keys and `config.invalid_permissions`;
  `companion.chat_after_help` now says tools are exposed *as granted*.

Not touched: `src/companion/session.rs` (verified: zero diff — the gate composes
at injection, so the loop needs no permission knowledge), `src/memory`,
`src/secure_path`, `src/vault.rs`.

## The grant grammar

```toml
[permissions]
memory             = ["recall", "derive"]  # allow-list of family members
scratch            = "prompt"              # family-wide mode
run_scratch_script = "allow"               # per-tool entry, beats the family
"mcp.github.*"     = "allow"               # prefix rule for future MCP servers
web                = "deny"                # unknown scope: parses, enforces deny
```

Resolution per tool name, most specific first: **explicit tool entry → exact
name entry → longest `*` prefix → family → deny**. An explicit family entry
defines the whole family: an allow-list grants its members and drops the rest
to deny (it does not fall back to defaults), so autonomy stays exactly the sum
of what is written. Namespaced names must be quoted (`"mcp.github.create_issue"`)
because bare dots are TOML nested-table syntax — the same spelling M10 will use,
so there is no format break ahead.

Defaults (Decision 6): `lambo_recall` / `lambo_derive` / `lambo_stats` granted;
`run_scratch_script` prompts; everything else denied. Sources tracked as
default | config; env overrides were deliberately **not** added — file-first
keeps the surface minimal for M5 and the attribution enum can grow `Env` when
an override earns it.

## Decisions taken

- **Gate as decorator at composition.** `GatedTools` wraps whatever
  `MemoryTools::for_chat` produced (even the No-op fallback). It filters
  `specs()` — ungranted tools are neither seen nor callable by a small model —
  and checks the resolved set before dispatching `execute`. One choke point;
  `session.rs` untouched.
- **Prompting moved into the gate.** In prompt mode the gate asks once via its
  own confirm callback (fail-closed y/N); `allow` skips the prompt, `deny`
  refuses without asking. The inner scratch seam is held open
  (`always_confirmed`) so a prompted run asks exactly once. A panicking confirm
  closure is contained like any other tool failure.
- **Denial is contained.** Deny returns the dedicated `permissions.denied`
  string; no panics, no config paths or values in the model-visible message.
  Through `Session::turn` the refusal surfaces as the loop's unknown-tool
  string, because the session only dispatches advertised specs — two layers
  over the same choke point, both pinned by tests.
- **Fail closed everywhere.** Bad mode strings, empty list entries, lists
  naming nothing in the family, or a list where only a mode fits →
  `ConfigError::InvalidPermissions` (clean message, exit 1). A wrong value type
  fails as invalid TOML. Post-validation lookups still resolve unrecognized
  input to deny, never wider.
- **Unknown scopes are data.** Arbitrary keys parse and render; they enforce
  deny simply because no tool matches them today. Prefix matching (longest
  prefix wins) already works against hypothetical names, proven by tests that
  grant/deny `"mcp.github.*"` against `mcp.github.create_issue`.

## Gates (how this stays honest)

- Decision 6 defaults asserted exactly, source-by-source.
- Per-tool override beats family; allow-list narrows; unknown scopes parse,
  round-trip through `config show`, enforce deny; longest-prefix matching.
- Ungranted tools absent from `specs()`; denied `execute` returns the contained
  string without reaching the inner executor (recording stub proves it).
- Prompt fires only in prompt mode: allow/deny skip it, yes executes, no refuses.
- Graph-independence pins on both new modules (the `include_str!` technique from
  the M3 seams): enforcement reads configuration only — no concept widens a grant.
- Composition through `Session::turn` against the mock server and a fixture
  memory: scratch unadvertised, called anyway, refused without running, while a
  granted derive flows into the real graph in the same turn.
- `mooshik permissions` output shape pinned: fixed family order, then scopes
  sorted, each with mode and source.
