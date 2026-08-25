# Adversarial review — Mooshik M5, round 1

**Reviewer**: independent, review-only. Wrote nothing under review except this file.
**Date**: 2026-08-26
**Scope**: commit `bbb313e` on `m5-permissions`.
**Worktree**: `/tmp/mooshik-m5` @ `bbb313e`
**Verdict**: **APPROVE-with-minors** — 1 P1 / 2 P2 / 5 P3 (enforcement itself is correct everywhere probed; the gaps are one missing composition pin, one lying CLI render, and one fail-closed inconsistency)

## Method

Read PLAN.md M5, SPEC.md *Autonomy is granted, not configured*, the
implementation record (`m5-implementation.md`), then every touchpoint:
`src/config/{permissions,mod,overlay,show}.rs`, `src/tools/{permissions,mod,scratch}.rs`,
`src/cli.rs`, `src/companion/{session,chat}.rs`, `src/companion/loop_tests.rs`,
`src/text/en.toml`. Grepped the whole tree for every `ToolExecutor`
construction site. Probed grammar edges empirically (empty list, bare dotted
keys, duplicate keys, uppercase modes, non-string values, prefix-equal-to-name,
`*`-only rule) via a throwaway test, then reverted. Mutation-tested all seven
pins below with transient edits fully reverted (`git checkout --`; tree clean
except this file). Gates run once at the end.

## Attack-list coverage

1. **Resolution precedence** — no config shape produced a wider grant than
   written in any probe. Allow-list narrows and drops unlisted members to deny
   (never back to defaults); per-tool beats family beats default; quoted dotted
   keys are required and bare ones fail load as `InvalidToml`. Two deliberate-but-
   sharp edges recorded as P3-M5-3: a `*` prefix rule outranks an explicit
   family entry even when the prefix covers known tools (`'lambo_*' = 'allow'`
   overrides `memory = 'deny'`; `'lambo_recall*'` matches the exact name), and
   equal-length prefixes cannot tie (distinct equal-length prefixes cannot both
   match one string; BTreeMap order is deterministic). Longest-prefix wins;
   an exact quoted name beats its own prefix.
2. **Enforcement completeness** — `executor_for_chat` is the only production
   executor construction (`cli.rs:163` the only caller; `serve` exposes lambo's
   own MCP surface to *the user*, not to the companion model, so it is outside
   the M5 boundary; `MemoryTools::from_memory` is test-only). Session dispatches
   only advertised specs (`session.rs:120`) and the gate re-checks on execute —
   two layers over one choke point. An unknown tool name resolves through
   `DENIED_BY_DEFAULT` to the contained string. But see P1-M5-1: the choke-point
   composition line itself is unpinned.
3. **Prompt semantics** — prompt fires once per call, only in `Prompt` mode;
   allow and deny skip the callback (pinned); confirm panic is caught by
   `catch_unwind` around `authorized` and returned as `tool_internal_error`
   (unpinned — P3-M5-4); no await between check and execute, so no TOCTOU.
4. **Fail-closed parsing** — bad mode strings, whitespace-only modes, uppercase
   modes, empty entries, bogus aliases, lists where only a mode fits, wrong
   value types (int/bool/nested-table-from-bare-dotted-key), and duplicate TOML
   keys all fail load cleanly (`InvalidPermissions` / `InvalidToml`, message
   names no values). One deviation: the **empty** allow-list loads (P2-M5-2).
5. **Graph independence** — real pins in both new modules via `include_str!`;
   mutation 5 confirms they bite.
6. **Permissions CLI** — deterministic order pinned by test; source attribution
   present. But `render()` misrepresents per-tool overrides (P2-M5-1).
7. **Contained denial** — `permissions.denied` leaks no path/value/panic;
   gate panics go to stderr, model gets the generic string; unknown-tool calls
   from the session get `companion.unknown_tool`.
8. **M4 regression** — Decision-6 defaults yield prompt-on-use end to end for a
   default-config user: scratch is `Prompt(Default)`, hence advertised; the gate
   prompts once; the inner seam is `always_confirmed()` so no second ask
   (pinned at the unit level; the real-composition ask-once is unpinned,
   P3-M5-5).

## Findings

| ID | Severity | Finding | Evidence |
| --- | --- | --- | --- |
| P1-M5-1 | **P1** | The ONE choke point is unpinned. The entire M5 contract reduces to `Arc::new(GatedTools::new(inner, grants))` at the end of `executor_for_chat` (`src/tools/mod.rs:421`), and no test exercises `executor_for_chat` at all — `cli.rs:163` is the only caller and the loop tests compose `GatedTools` by hand. Mutation 6 (return `inner` directly, gate dropped) passes the **entire 147-test suite green**. Any refactor that drops or bypasses the wrap silently disables all permission enforcement while CI stays green. Fix direction: a test calling `executor_for_chat` against a Memory-store/fixture-embedder config asserting the result filters `specs()` and refuses a denied `execute` (or a structural pin like the M3 `chat_dispatch_does_not_open_memory` test). | mutation 6 output: `147 passed; 0 failed` with the gate deleted |
| P2-M5-1 | **P2** | `mooshik permissions` lies about per-tool overrides. Exact entries land in `Grants.exact`, but `render()` reads only `Grants.family` (`src/config/permissions.rs:366-370`). With `[permissions] memory = 'deny'` + `lambo_recall = 'allow'`, the gate enforces `Allow(Config)` while the command prints `lambo_recall deny (config)` (empirically reproduced). Active prefix rules over known tool names are likewise displayed under the "no matching tool yet" header though they match today. A security-relevant display bug on the exact surface meant to answer *"what may it do while I am not looking"*. Enforcement unaffected. Fix direction: resolve each member via `decision_for` (and classify scopes by whether they match any known tool) instead of reading raw maps. | probe output in review transcript; `render()` vs `decision_for` |
| P2-M5-2 | **P2** | Empty allow-list loads instead of failing. The task contract and the module's own doc ("an allow-list naming nothing in the family … fails config load", `permissions.rs:158-160`) name the empty list among malformed grants, but `memory = []` passes validation (the loop over entries is vacuous) and silently converts the whole family to `deny (config)`, discarding the Decision-6 allows. Direction is narrower, so nothing widens — but it contradicts documented behavior and differs from `memory = ['bogus']`, which does fail. Fix direction: reject `Tools([])` under a known family (and arguably everywhere) in `validate()`. | probe: `memory = []` → `Ok(GrantDecision { Deny, Config })` |
| P3-M5-1 | P3 | The Decision-6 "exactly" pin is partially tautological: `assert_eq!(grants.decision_for("web_fetch"), DENIED_BY_DEFAULT)` compares against the constant itself, so widening the constant leaves the headline assertion green (mutation 2 proved it — only the other two tests failed). Assert literal `GrantDecision { Deny, Default }` instead. | mutation 2 results |
| P3-M5-2 | P3 | Unknown-scope allow-lists render as `allow [names] (config)` while enforcing deny-by-default (`filesystem_read = ['~/work']` prints an allow label). Cosmetic today (no tool matches), misleading the day one does. | probe: `decision_for("fs_read") = Deny(Default)`, render contains `filesystem_read allow` |
| P3-M5-3 | P3 | Prefix rules outrank family entries for known tools and match the exactly-equal name (`'lambo_recall*' = 'prompt'` overrides `memory = 'deny'` for `lambo_recall`). This follows the documented resolution order and is config-authored, so not a widening *bug* — but the interaction is untested and surprising; a stale broad prefix can silently re-widen a tightened family. Recommend one pin test + a doc sentence. | probes 3/5; no test covers prefix-vs-family |
| P3-M5-4 | P3 | Gate-level confirm-panic containment has no named test: the `catch_unwind` around `authorized` (`tools/permissions.rs:98`) is unexercised (M4's equivalent gap took a remediation round). A panicking confirm closure should be pinned to return `tool_internal_error` without killing the loop. | source inspection |
| P3-M5-5 | P3 | Exactly-once prompting in the real composition is unpinned: if `MemoryTools::for_chat` regressed from `ScratchConfig::always_confirmed()` to `default()`, the user would get the gate prompt *and* the inner scratch prompt (double ask, still fail-closed) and no test would fail — the only `for_chat` test expects `None`. Relatedly, `loop_tests.rs:454` uses `from_memory` (default seam) under the gate rather than the production seam; fine for its purpose, but it means no test composes the production pair. | `tools/mod.rs:121`, `tests.rs:228-233` |
| P3-M5-6 | P3 | Scope notes, no defect found: `serve` bypasses the gate legitimately (user-initiated MCP exposure of lambo's own surface; PLAN routes MCP-tools-through-the-gate to M10); env-var grant overrides deliberately absent (documented); denial strings contain no config paths/values; `chat_after_help` accurately says "as granted". | grep + source |

## Mutation table

Every transient edit reverted; tree clean afterwards except this file.

| # | Pin | Mutation | Result |
| --- | --- | --- | --- |
| 1 | Ungranted tools are not advertised | `GatedTools::specs` returns `self.inner.specs()` unfiltered | **CAUGHT** `tools::permissions::tests::ungranted_tools_are_not_advertised`, `future_mcp_tools_pass_through_the_same_gate` |
| 2 | Unknown tools denied by default | `DENIED_BY_DEFAULT.mode` → `Allow` | **CAUGHT** `unknown_scopes_parse_round_trip_and_enforce_deny`, `future_mcp_tools_pass_through_the_same_gate` (note: `decision6_defaults_…` stayed green — tautology, P3-M5-1) |
| 3 | Per-tool entry beats family mode | `decision_for` checks `family` before `exact` | **CAUGHT** `per_tool_entry_beats_the_family_mode` |
| 4 | Malformed tables fail load | `validate()` body → `Ok(())` | **CAUGHT** `malformed_permissions_tables_fail_closed` |
| 5 | Graph never consulted | `use crate::memory;` injected into production halves of both new modules | **CAUGHT** `the_grant_model_is_graph_independent`, `the_gate_never_consults_the_graph` |
| 6 | Gate actually wraps `executor_for_chat` output | return `inner` ungated from `executor_for_chat` | **MISSED** — full suite `147 passed; 0 failed` (P1-M5-1) |
| 7 | Inner scratch seam held open under chat | `always_confirmed()` → `default()` in `for_chat` | **MISSED by inspection** — no test executes scratch through `for_chat` (P3-M5-5) |

**Mutation score**: 5/5 mandated pins CAUGHT; two additional seams (6, 7) are
undetected under mutation — one escalated to P1.

## Gate table

| Gate / probe | Result |
| --- | --- |
| `cargo fmt --all -- --check` | **PASS** |
| `cargo clippy --all-targets --locked -- -D warnings` | **PASS** |
| `cargo test --locked` | **PASS** — 147 passed, 0 failed, 1 ignored |
| ≤1000-line file cap | **PASS** — largest `src/secure_path/mod.rs` 792 lines |
| Worktree cleanliness after review | **CLEAN** — `git status` empty except this untracked file |

## Verdict

**APPROVE-with-minors.** Enforcement behavior is correct in every scenario
probed: precedence never widens beyond what is written, parsing fails closed
with clean messages (one empty-list deviation), denial is contained, prompting
is once-per-call and prompt-mode-only, the graph-independence pins have real
teeth, and the Decision-6 default really yields interactive prompt-on-use for a
default-config user. Required before M10 builds on this: add the
`executor_for_chat` composition pin (P1-M5-1) and fix the `mooshik permissions`
render to reflect exact/prefix overrides (P2-M5-1); P2-M5-2 is a two-line
validation tightening. Everything else is documentary.
