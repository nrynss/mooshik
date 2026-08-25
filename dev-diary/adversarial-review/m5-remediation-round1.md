# M5 remediation — round 1 findings

**Date**: 2026-08-26
**Base**: `bbb313e` (`m5-permissions`), findings from `m5-round1.md`.
**Scope**: P1-M5-1, P2-M5-1, P2-M5-2 fixed; cheap P3s (P3-M5-1/2/3/4/5) fixed;
no documented-tradeoff P3s touched (P3-M5-3's doc-order behavior is pinned, not
changed; P3-M5-6 needs nothing).

## Per-finding changes

### P1-M5-1 — the choke point is unpinned → FIXED
Two pins in `src/tools/tests.rs`:
* `executor_for_chat_wraps_its_inner_executor_in_the_gate` — source-level
  `include_str!("mod.rs")` pin: the production body of `executor_for_chat`
  must construct `Arc::new(GatedTools::new(inner, …))`.
* `executor_for_chat_gates_even_the_noop_fallback` — behavioral: calls
  `executor_for_chat` against a `[permissions] scratch = 'deny'` config whose
  memory cannot open (product default Postgres/no-DSN → `MissingDsn`, fast),
  asserts an ungranted tool name is refused with `permissions.denied` through
  the returned `Arc<dyn ToolExecutor>` while a granted name still passes
  through to the inner Noop (`companion.unknown_tool`).

**Mutation result**: returning `inner` ungated from `executor_for_chat` fails
BOTH named tests (behavioral: denied call answered `"That tool is not
available."` instead of the permission refusal) — **CAUGHT**, restored.

### P2-M5-1 — `mooshik permissions` misrenders per-tool overrides → FIXED
`Grants::render` (`src/config/permissions.rs`) now resolves every known member
through `decision_for` instead of reading `Grants.family`, so exact per-tool
entries and prefix rules surface in the effective mode/source lines exactly as
enforced. Scoped entries are classified by `matches_known_tools`: a prefix rule
that already decides a known tool renders in the resolved section (not under
the "no matching tool yet" header); scopes matching nothing stay under the
unmatched header.
Test: `render_shows_the_effective_decision_for_per_tool_and_prefix_overrides`
(`memory = 'deny'` + `lambo_recall = 'allow'` must render
`lambo_recall allow (config)`; live `'lambo_d*' = 'prompt'` renders resolved;
inert allow-list labelled per below).

### P2-M5-2 — empty allow-list loads → FIXED fail-closed
`PermissionsConfig::validate` rejects `Tools([])` everywhere (family, unknown
scope), so `memory = []` now fails config load as `InvalidPermissions` instead
of silently converting the family to deny-by-config. Parse cases added to the
malformed-table loop: `memory = []`, `scratch = []`, `'mcp.github.*' = []`,
`web = []`.

## Cheap P3s

| ID | Change |
| --- | --- |
| P3-M5-1 | `decision6_defaults_are_exactly_the_settled_grant_set` now asserts literal `GrantDecision { Deny, Default }` for `web_fetch`/`fs_read` instead of comparing against `DENIED_BY_DEFAULT` itself. |
| P3-M5-2 | Unknown-scope allow-lists render with a truthful label via new text key `permissions.inert_list`: `filesystem_read deny-until-a-tool-matches [~/work] (config)`. |
| P3-M5-3 | `prefix_equal_to_a_tool_name_loses_to_the_exact_entry_but_beats_the_family`: exact entry beats its own equal-name prefix; without one, the prefix outranks the family mode (documented order, now pinned). |
| P3-M5-4 | `a_panicking_confirm_is_contained_by_the_gate` (`src/tools/permissions.rs`): panicking confirm closure yields `tools.internal_error`, dispatches nothing, and the gate survives subsequent calls. |
| P3-M5-5 | `for_chat_holds_the_inner_scratch_seam_open_under_the_gate` — source-level pin that `for_chat` sets `ScratchConfig::always_confirmed()` (double-prompt regression now caught). **Mutation result**: seam mutated to `default()` fails the named test — **CAUGHT**, restored. |

## Mutation summary

| # | Mutation | Result |
| --- | --- | --- |
| 1 | `executor_for_chat` returns `inner` ungated | **CAUGHT** — both P1 pins fail |
| 2 | `for_chat` seam `always_confirmed()` → `default()` | **CAUGHT** — seam pin fails |

## Gates

```
cargo fmt --all -- --check                                  PASS
cargo clippy --all-targets --locked -- -D warnings          PASS
cargo test --locked                                         PASS — 153 passed, 0 failed, 1 ignored
```

Suite grew 147 → 153 (+6 tests). Tree clean after commit; review file left as-is.
