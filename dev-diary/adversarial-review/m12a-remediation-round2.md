# M12a round-2 remediation

Remediates every finding in `m12a-round2.md` — the P1 on the unclaimed SQLite
side files, the P2 on the scratch-collision pin that passed with its fix
reverted, and all four P3s (two doc corrections in `tui_cmd`, one real fix plus
two doc corrections in the signal code, two doc corrections in `memory::view`).
No deferrals. Base and destination: branch `main`; the tree is left dirty for
the orchestrator, nothing committed.

## Per-finding fixes

### P1 R2-1 — the repair reached one of the three files SQLite leaves

`claim_local_store` claimed `graph.db` and stopped. SQLite copies the database's
mode onto `-wal`/`-shm` only when it **creates** them, so on an old home — where
database and side files all started at 0644 — the WAL kept its wide mode
indefinitely: it survives a clean `close()`, keeps growing, and carries concept
content in cleartext (the review's own probe found a concept's words in the 0644
file).

**Fix.** `claim_local_store` now claims the side files with the same parent-fd
primitive, `open_existing_at`, chmodding to 0600 when present and skipping when
absent — an absent `-wal` is SQLite's to make, and this claim never creates one.
The module doc's false sentence ("SQLite gives the `-wal` and `-shm` side files
the database's own mode, so the two of them follow without being named here")
is replaced by the true one: the mode is copied only on creation, so a side file
that predates the claim keeps the database's old mode until the repair reaches
it.

**Pin.** `the_local_database_is_created_and_repaired_private` exercises the
upgrade path rather than the born-private one. Fresh home: provision, a real
`derive` session, a close — then asserts **all three** of
`graph.db`/`graph.db-wal`/`graph.db-shm` exist and are 0600, and that the WAL
carries content. Then it widens **all three** to 0644, runs `provision` again,
and asserts **all three** are back to 0600 — the case the review ran and found
leaking.

*Mutation:* the side-file loop cut from `claim_local_store` →

```
assertion `left == right` failed: a widened graph.db-wal was left open
  left: 420    right: 384
```

The 0644 WAL survives the repair run. Reverted; tree verified clean.

### P2 R2-2 — the collision pin now observes the fault it names

The old pin called `Sandbox::create()` twice and asserted the paths differ —
but each call sampled `SystemTime::now()` itself, and the `create_dir` between
the two readings is tens of microseconds against a realtime clock that advances
in one, so the pin was green on the code it guards against (three runs, three
passes, with the counter removed).

**Fix.** The naming is extracted into a pure function so the clock can be held
still:

```rust
fn name(instant: std::time::SystemTime) -> String
```

with the same format string (`mooshik-scratch-{pid}-{subsec_nanos:x}-{counter:x}`);
`Sandbox::create` calls it with `SystemTime::now()`. The test samples the clock
**once**, builds both names from that one instant, and asserts they differ —
pid and instant are identical by construction, so only the counter can separate
them. The doc comment's sentence about sampling the clock once is now literally
true.

*Mutation:* the counter dropped from the format string →

```
assertion `left != right` failed: two names built from one instant must differ
  left: "mooshik-scratch-126725-25a28d0d"
 right: "mooshik-scratch-126725-25a28d0d"
```

Two names from one instant are equal. Reverted; tree verified clean.

### P3 R2-3 — `tui_cmd`'s header no longer advertises what was deleted

The header said the refusal carries "Lambo's own conflict sentence, which names
the holder and the override". False twice over: the rendered refusal is
Mooshik's own `memory.session_conflict` template with Lambo's facts
interpolated, and `memory::facts` deliberately cuts the override out — the test
`a_session_conflict_names_the_holder_and_no_page_this_product_does_not_ship`
asserts the rendered text contains no "takeover".

**Fix.** The two lines now read: "with Mooshik's own conflict sentence, which
names the holder and no override or page this product does not ship." Doc-only;
no behaviour change. Verified against the template and the test it quotes.

### P3 R2-4 — the signal handler is uninstalled before the close it exists to reach

`leave_on_signals` installed SIGTERM/SIGHUP and nothing ever put them back, so
after the loop every signal set a flag nobody read for the rest of the process —
and the window that matters is `tui_cmd::live`'s `runtime.block_on(memory.close())`
after `run()` returns, where a `kill` on a wedged close used to end the process
and now could not. The doc's "there is nowhere to put it back on" was wrong: the
command has not returned, it is closing.

**Fix.** `leave_on_signals` now captures the previous dispositions (the SAFETY
comment's "out-parameter is null because no previous disposition is wanted back"
is gone) and returns them in a `SignalDispositions` struct; `run()` restores
both after `event_loop` returns, so from the moment the command starts closing
the session a SIGTERM/SIGHUP behaves as it always did. The handler itself
remains a single `Relaxed` store — the only async-signal-safe work it does.
Docs of `run` and `leave_on_signals` updated; the test now restores the
disposition after itself so the suite is never left with a handler installed.

**Pin.** `a_termination_signal_disposition_is_restored_after_the_session` reads
the disposition back via `sigaction` before the install/restore pair and after
it, and asserts the readback equals what it was before — SIG_DFL in the suite.

*Mutation:* `restore_signals` emptied →

```
assertion `left == right` failed: signal 15 was left with the session's handler installed
  left: 94108650453360    right: 0
```

The handler pointer is still installed where the default was. Reverted; tree
verified clean.

### P3 R2-5 — `said`'s written limit now covers the case the rule catches

The doc admitted one over-approximation — a sentence *exactly* a concept is
read as an echo — but the second leg is wider: a prompt is dropped when every
piece of it, split on the join separator, is a concept, so a multi-clause
sentence disappears without ever being a concept itself (executed in round 2:
"Ship it; it rained" with both clauses in the graph empties the day's log).

**Fix.** Doc-only; rule unchanged. The sentence now reads: "a prompt is dropped
when it is *exactly* a concept in the graph, and also when every clause of it,
split on the join separator (`"; "`), is a concept — a multi-clause sentence
disappears without itself having been a concept." Verified against the code on
the same page:

```rust
if contents.contains(said) || said.split(JOIN).all(|piece| contents.contains(piece)) {
```

with `const JOIN: &str = "; "` — the doc's two legs are the code's two
disjuncts, word for word.

### P3 R2-6 — `action_nodes`' stated limit is no longer one-sided

The doc named the false negative (an action with no targets has no Causal edge
and reads as an ordinary thought) but not the false positive: `record_action`
resolves its action string through `canonicalize` and **reuses an existing node
on a key match**, so an action whose text canonicalizes onto a thought the user
already had turns that thought into the action node — and `bookkeeping` then
hides a real thought from both panels permanently, because the Causal edge never
goes away.

**Fix.** Doc-only; one sentence added beside the other limit, quoting the
mechanism. Verified against the pinned lambo crate at `4c6fc93`
(`src/graph/action.rs:315-321`):

```rust
CanonicalizeResult::Matched { key, node } => {
    reject_empty_key(content, &key)?;
    Ok(node)
}
```

`resolve`, which `record_action` walks its action string through, returns the
existing node on a match — a thought the user already had can become the action
node, which is exactly the sentence the doc now prints.

## Mutation summary

| Mutation | Pin | Result |
| --- | --- | --- |
| side-file loop cut from `claim_local_store` | `the_local_database_is_created_and_repaired_private` | **caught** — "a widened graph.db-wal was left open" (644 vs 600) |
| counter dropped from the scratch `name` format | `two_sandboxes_opened_in_the_same_instant_are_two_directories` | **caught** — two names from one instant are equal (the R2-2 fix's own missing half) |
| `restore_signals` emptied | `a_termination_signal_disposition_is_restored_after_the_session` | **caught** — the readback shows the handler pointer where SIG_DFL was |

Every mutation transient; each run against the pinned test, then reverted, and
`git diff` verified to return to the pre-mutation state.

## Gates

* `cargo test --locked` → **539 lib + 1 integration**, 0 failed, 2 ignored (both
  pre-existing and legitimately marked; no new `#[ignore]`).
* `cargo clippy --all-targets --all-features` → clean.
* `cargo fmt --check` → clean.
* File-size cap → clean. Largest touched file `view.rs` 875 lines
  (`tui_cmd.rs` 100, `scratch.rs` 642, `resolve.rs` 299, `ops.rs` 472,
  `tui/mod.rs` 731) — all under the 1000-line cap.