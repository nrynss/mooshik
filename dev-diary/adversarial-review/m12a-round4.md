# M12a round 4 — adversarial re-verification of the round-3 remediation

Reviewed at HEAD `47d4e91`, branch `main`, tree dirty with the round-2 and
round-3 remediations (6 modified source files + the untracked
`m12a-remediation-round2.md`, `m12a-remediation-round3.md`, `m12a-round3.md`).
Scope: both round-3 findings, mutation-tested by me against the working tree;
the six round-2 findings re-confirmed (the four doc-only ones by stamp check,
the two code pins by individual runs); the untouched-file files scanned for
accidental breakage; and the remediation records' honesty. All transient edits
reverted — each mutated file was restored from a byte copy and
`sha256sum`-verified identical to its pre-mutation state, `git status
--porcelain` shows exactly the same 6 modified + 3 untracked as before I
started, and now the round-4 record beside them.

## Verdict

**REMEDIATE** — 1 × P3.

Both round-3 findings are genuinely fixed, and both round-3 pins bite under
the machine's umask with the record's quoted output reproduced verbatim — but
the R3-1 pin's bite is **umask-dependent**, and I proved by execution that
under a hardened umask (`077`) the revert is invisible to it. That
contradicts the remediation record's claim that the pre-fix code "yields at
least 0755/0644 under any realistic umask, so the pin fails on the revert",
and it is the exact shape the round-3 reviewer prescribed against ("under a
deliberately wide umask (0o000-style, or by setting `set_permissions` to wide
first like the graph.db test does)") — neither was delivered. One P3, no P1/P2.

## What held up under attack

* **P2 R3-1 — the mode pins are set and read correctly; the cfg gates do not
  leak.** `Sandbox::create` (`src/tools/scratch.rs:443-448`) and
  `write_script` (`:461-466`) each pin their mode with
  `fs::set_permissions` + `Permissions::from_mode` inside a `#[cfg(unix)]`
  block, after `create_dir`/`File::create` respectively. The blocks are the
  only unix-only code added; `create`/`write_script` otherwise use
  cross-platform `std::fs` calls, so the non-unix build is untouched (no
  non-unix target is available here to compile it, same as prior rounds).
  `set_mode` is a chmod-equivalent, so the umask never touches the installed
  mode: the fix is correct under every umask. The pin
  `the_scratch_sandbox_and_script_stay_private` (`:668-684`) is
  `#[cfg(unix)]`-gated (matching the file's discipline — there is no mode
  concept to pin on non-unix) and asserts `0o700`/`0o600` over
  `fs::metadata` with the two named messages. Both quoted mutations
  reproduced verbatim at umask `022` (see table).
* **P3 R3-2 — the assert now names the signal, and the record's quote is
  reproducible.** The disposition pin (`src/tui/mod.rs:397-400`) carries
  `"signal {signal} was left with the session's handler installed"`; with
  `restore_signals` emptied the pin failed with exactly that message and
  `right: 0` (see table; the `left:` pointer varies with ASLR as the record
  predicts). Every signal-handling assert in the file names its subject
  (`"signal {signal}"`, `"signal {signal} did not end the session"`,
  `"reading the disposition of {signal} failed"`); the one bare boolean
  assert (`assert!(!asked_to_leave())`, line 366) is unpatched pre-HEAD
  context, not this patch's.
* **R2-1 through R2-6 survive.** `the_local_database_is_created_and_repaired_private`
  (ops.rs, untouched by R3) passes individually — fresh-home half, the
  `-wal` `len() > 0` guard, and the widen/re-provision half all green.
  `two_sandboxes_opened_in_the_same_instant_are_two_directories` (scratch.rs,
  R3-touched) passes: the extraction into `name(instant)` kept the format
  byte-identical (same string, same `fetch_add`), and the pin asserts the
  two names from one instant differ. Both signal pins (tui/mod.rs,
  R3-touched) pass individually. The four doc-only pins stamp-check against
  the current code: the `tui_cmd.rs` header ("Mooshik's own conflict
  sentence, which names the holder and no override or page this product does
  not ship") matches `memory::facts`' first-sentence cut and the shipped
  tests (`!contains(".mdx")`, `!contains("takeover")`); `said`'s doc
  ("two over-approximations … split on the join separator (`"; "`)") is the
  `contents.contains(said) || said.split(JOIN).all(...)` disjunction exactly;
  the `action_nodes` doc's canonicalization claim is unchanged by R3 and the
  lambo crate is still pinned at `4c6fc93` in `Cargo.lock`.
* **No behaviour change beyond the eight.** The R3 delta is exactly: two
  `set_permissions` calls in scratch.rs (cfg-gated), the new scratch pin, and
  the assert message in tui/mod.rs. The four other files are byte-identical
  to what round 3 reviewed; `git diff` shows no drift.
* **Record honesty.** The R3 mutation quotes reproduce verbatim from the
  shipped asserts (two for R3-1 under umask 022, one for R3-2 including the
  ASLR caveat). The one false note is the "any realistic umask" claim in the
  R3-1 pin write-up (below).

## Findings

### P3

**M12a-R4-1 — The R3-1 pin only bites when the suite runs under a wide-ish
  umask; under `077` the revert passes, contradicting the record.**

The pin asserts the exact modes `0o700`/`0o600` that `Sandbox::create` and
`write_script` produce. Those modes are the *output of the process umask at
creation time*: without the fix, `create_dir`/`File::create` yield `0o777 &
!umask` / `0o666 & !umask`. Under the ordinary `022` the pre-fix modes are
`0755`/`0644` and both mutations are caught (executed, verbatim). But under a
hardened umask `077` the pre-fix modes are `0700`/`0600` — identical to the
fixed ones — so with the script mode-setting dropped the pin **passes**
(executed: `umask 077; cargo test the_scratch_sandbox_and_script_stay_private`
→ 1 passed), and a full revert is equally invisible. The remediation record
claims the pin is "unconditional … because the pre-fix code yields at least
0755/0644 under any realistic umask, so the pin fails on the revert" — false
for any umask ≥ `077` (a real hardening configuration: it makes every new
file private by default); the machine's own CI/dev default is `022`, which is
where the quoted mutations were recorded. The round-3 review prescribed the
shape that removes this dependence — "under a deliberately wide umask
(0o000-style, or by setting `set_permissions` to wide first like the graph.db
test does)" — and it was not delivered; the R2-1 pin escaped the same
criticism only because its widen/re-provision half is umask-independent,
which the scratch sandbox (no repair path) cannot replicate. Impact: a revert
of the sandbox-mode fix ships whenever the test environment runs at umask
≥ `077` and the production process runs at a normal umask — the
world-readable `/tmp` sandbox returns undetected, and a reviewer rerunning
the suite under `077` sees the pin silently green.

*Remediation.* Give the pin a controlled creation-time umask, per the round-3
review: set `libc::umask(0)` around the `Sandbox::create`/`write_script`
calls in the test (restore afterwards, with a SAFETY comment noting the
process-wide race window against the suite's other file-creating tests), or
fork the creation under a wide umask; and drop "any realistic umask" from the
record. The fix itself needs no change — it is umask-correct.

## Mutation-tested pins

Every mutation transient; the mutated file restored from a byte copy and
`sha256sum`-verified identical to the pre-mutation state after each run
(`7b706a55…` scratch.rs, `c2296399…` tui/mod.rs).

| Mutation | Pin | Result |
| --- | --- | --- |
| dir mode-setting dropped from `Sandbox::create` | `the_scratch_sandbox_and_script_stay_private` | **caught** — "sandbox dir must not be readable by other accounts, got 755", `left: 493, right: 448`, verbatim |
| script mode-setting dropped from `write_script` | `the_scratch_sandbox_and_script_stay_private` | **caught** — "sandbox script must not be readable by other accounts, got 644", `left: 420, right: 384`, verbatim |
| script mode-setting dropped from `write_script`, suite run under `umask 077` | `the_scratch_sandbox_and_script_stay_private` | **NOT caught** — `1 passed`; the reverted code yields `0600`, indistinguishable from the fix (new finding R4-1) |
| `restore_signals` body emptied | `a_termination_signal_disposition_is_restored_after_the_session` | **caught** — "signal 15 was left with the session's handler installed", `left: 94167248498192, right: 0` (left varies with ASLR, as the record notes) |

## Gates

Run by me at the end, exactly as run, in a clean env (the ambient shell
exports `LAMBO_POSTGRES_DSN`; every `cargo` invocation ran under
`env -u LAMBO_POSTGRES_DSN -u MOOSHIK_POSTGRES_DSN -u DATABASE_URL`):

* `cargo test --locked` → **538 lib passed, 0 failed, 2 ignored** (540 total;
  the two ignored are the pre-existing live-Cloud ones) **+ 1 integration
  passed** (`report_pin`). Matches the remediation's "538 lib + 1
  integration, 0 failed, 2 ignored".
* The five pins individually, on the final tree: all pass (R2-1 ops, R2-2
  scratch, R2-4 both tui pins, R3-1 scratch pin).
* `cargo clippy --all-targets --all-features` → clean.
* `cargo fmt --check` → clean.
* File-size cap → clean. `tui_cmd.rs` 100, `ops.rs` 472, `resolve.rs` 299,
  `view.rs` 875, `scratch.rs` 685, `tui/mod.rs` 734 — all under 1000; the two
  R3-touched files match the remediation's numbers exactly.

## What was executed vs. only read

**Executed.** The four rows of the mutation table (three mutations plus the
umask-077 variant), each against its pinned test, each reverted and
hash-verified. The five pins individually. The full suite in a clean env,
clippy, fmt, and the file-size count. The umask arithmetic
(`0o777 & !umask` / `0o666 & !umask`) at both `022` and `077`.

**Read, not executed.** The non-unix stubs were type- and call-site-verified,
not compiled for a non-unix target (none is available). Lambo at `4c6fc93`
was re-confirmed by `Cargo.lock`, not re-read from source (the R2-6 mechanism
was read in round 3 and nothing touched `view.rs` since).