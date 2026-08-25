# Adversarial review — Mooshik M5, round 2

**Reviewer**: independent, review-only. Wrote nothing under review except this file.
**Date**: 2026-08-26
**Scope**: commit `88abf81` on `m5-permissions` (remediation of `m5-round1.md`).
**Worktree**: `/tmp/mooshik-m5` @ `88abf81`
**Verdict**: **APPROVE — zero residue.** All three round-1 P1/P2 closures verified
by independent trace + mutation; the five cheap P3 fixes hold; no new P1/P2/P3
residue found.

## Method

Read `m5-round1.md` and `m5-remediation-round1.md`, then re-derived each claim
from source (`src/tools/{mod,tests,permissions}.rs`,
`src/config/permissions.rs`, `src/config/mod.rs`, `src/cli.rs`,
`src/text/en.toml`, `docs/SPEC.md`). Mutation-tested the two composition pins
and the gate check with transient edits fully reverted. Probed the real CLI
code path (`mooshik permissions` against a temp `MOOSHIK_HOME`) with the SPEC's
example block loaded verbatim, plus a transient render↔enforcement equivalence
test over every known tool member (removed after it passed; tree clean).

## Closure verification

### P1-M5-1 — choke point pin → VERIFIED CLOSED

* `executor_for_chat_wraps_its_inner_executor_in_the_gate` (structural):
  slices the production half of `src/tools/mod.rs` at `#[cfg(test)]`, extracts
  the `executor_for_chat` body, and requires both `GatedTools::new(inner` and
  `Arc::new(GatedTools::new(inner`. Not vacuous: if the function is renamed or
  removed the `.expect("executor_for_chat must exist")` fires; if the wrap is
  relocated out of this function the assertion fires.
* `executor_for_chat_gates_even_the_noop_fallback` (behavioral): drives the
  real factory on the MissingDsn path (`from_toml_and_env(..., [])` pins the
  env to empty → default Postgres store, no DSN) and asserts a denied
  `run_scratch_script` call returns `permissions.denied` while a granted
  `lambo_recall` call passes through to the inner Noop
  (`companion.unknown_tool`). This is the only test that exercises
  `executor_for_chat` itself — exactly the seam round 1 mutation 6 slipped
  through.

### P2-M5-1 — permissions render → VERIFIED CLOSED

`Grants::render` now resolves every known member through `decision_for`
(the same function the gate's `authorized`/`advertised` consult), so exact
entries and prefix rules surface as enforced. Scoped keys are classified by
`matches_known_tools` (prefix-only; validate already rejects `Tools` lists
under `*` keys, so no live-list case exists). Verified three ways:

1. Unit test `render_shows_the_effective_decision_for_per_tool_and_prefix_overrides`
   covers exact-beats-family, live prefix rendered resolved (not under the
   unmatched header), inert allow-list label, prefix-vs-exact and
   prefix-vs-family precedence.
2. Real CLI probe: binary built, temp home initialized, config replaced with
   the SPEC example **verbatim** — output was truthful end-to-end:
   `lambo_recall/lambo_derive allow (config)`, `lambo_stats deny (config)`
   (allow-list narrowing visible), `run_scratch_script prompt (config)`,
   `web deny (config)` and `filesystem_read deny-until-a-tool-matches [~/work]
   (config)` under the unmatched header.
3. Transient equivalence probe: for every memory+scratch member,
   `render()`'s per-member line equals `decision_for(tool)` mode/source;
   passed, then removed.

### P2-M5-2 — empty allow-lists → VERIFIED CLOSED

`PermissionsConfig::validate` rejects `RawGrant::Tools([])` for every key shape
(family, unknown scope); parse cases added to the malformed-table loop:
`memory = []`, `scratch = []`, `'mcp.github.*' = []`, `web = []`.

### Cheap P3s spot-checked

P3-M5-1 literal `GrantDecision { Deny, Default }` assertions present;
P3-M5-2 `permissions.inert_list` key present in en.toml and used;
P3-M5-3 precedence pinned in tests; P3-M5-4 panicking-confirm containment test
present and failed under the gate-check mutation below; P3-M5-5 seam pin
present (source-level, same technique as the structural P1 pin).

## New-residue hunt

* **decision_for-based rendering vs previously-correct output**: none changed.
  Default-config render still prints `allow (default)` / `prompt (default)`
  per member (probed); plain unknown-scope mode rules still print under the
  unmatched header exactly as before; only the two wrong cases from round 1
  (per-tool overrides, live prefixes) moved to their truthful rendering.
* **Noop-fallback test hygiene**: env is pinned empty via
  `from_toml_and_env(..., [])`; production code never reads env directly on
  this path (DSN resolution happens in Config; `grep` of `src/memory` shows
  env reads only inside ignored/live tests). MissingDsn fails fast, no network,
  no shared mutable state, no global registration. The stderr note
  (`tools.chat_memory_unavailable`) is cosmetic in tests.
* **validate() tightening vs documented shapes**: the SPEC's own `[permissions]`
  example block loads verbatim through `Config::load_at` + the real CLI
  (exit 0, truthful output). Repo-wide sweep: it is the only documented
  `[permissions]` shape. Empty lists were never documented as valid.
* **en.toml completeness**: all 55 `text::get("…")` keys used across `src/`
  resolve in en.toml (script-checked; zero missing).
* **File sizes**: largest `src/secure_path/mod.rs` 792 lines ≤ 1000 cap.

## Mutation table

Every transient edit reverted (`git checkout --`); tree clean afterwards
except this file.

| # | Pin | Mutation | Result |
| --- | --- | --- | --- |
| 1 | Gate wraps `executor_for_chat` output | return `inner` ungated | **CAUGHT** — BOTH named tests fail: behavioral answers `companion.unknown_tool` where `permissions.denied` is required; structural loses the `GatedTools::new(inner` match |
| 2 | Gate actually checks before execute | `authorized`: `Deny => true` (check skipped) | **CAUGHT** — `executor_for_chat_gates_even_the_noop_fallback` fails via the denied-divergence (execute assertion), plus `denied_execute_is_contained…`, `a_panicking_confirm_is_contained…`, `future_mcp_tools_pass_through_the_same_gate`, `prompt_fires_only_in_prompt_mode` |

Note on mutation 2: the behavioral pin's `specs().is_empty()` assertion cannot
see a bypassed filter on the fallback path (the Noop advertises nothing), but
its denied-execute assertion catches the bypass directly — coverage holds.

**Mutation score**: 2/2 attempted closures CAUGHT; no silent seams found this
round.

## Gate table

| Gate / probe | Result |
| --- | --- |
| `cargo fmt --all -- --check` | **PASS** |
| `cargo clippy --all-targets --locked -- -D warnings` | **PASS** |
| `cargo test --locked` | **PASS** — 153 passed, 0 failed, 1 ignored |
| ≤1000-line file cap | **PASS** — largest 792 lines |
| Worktree cleanliness after review | **CLEAN** — `git status` empty except this untracked file |

## Verdict

**APPROVE — zero residue.** Both composition pins bind to the real factory and
bite under mutation; the render reflects enforcement exactly (unit-pinned,
CLI-probed with the SPEC example verbatim, and equivalence-checked per
member); empty allow-lists fail closed across all families and scoped forms
without breaking any documented config shape. No new defects introduced by the
remediation. M10 may build on this surface.
