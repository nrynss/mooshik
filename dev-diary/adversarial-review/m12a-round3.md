# M12a round 3 — adversarial re-verification of the round-2 remediation

Reviewed at HEAD `47d4e91`, branch `main`, tree dirty with the round-2
remediation (6 modified source files + the untracked `m12a-remediation-round2.md`).
Scope: every one of the six findings in `m12a-round2.md`, each pin
mutation-tested by me against the working tree, the new code the remediation
introduced (`SignalDispositions`/`restore_signals`, the widened three-file pin),
the record's own honesty, and the outside-round decision on `Sandbox::create`'s
modes in `/tmp`. All transient edits reverted; after each mutation the three
mutated files were restored from byte copies and verified by `sha256sum`;
`git status --porcelain` showed exactly the same 6 modified + 1 untracked as
before I started, and now the round-3 record beside them.

## Verdict

**REMEDIATE** — 1 × P2, 1 × P3.

All six round-2 findings are genuinely fixed, and every pin bites — I ran the
three mutations myself and the failures reproduce. The two findings below are
new this round: the `/tmp` sandbox exposure round 2 noted in passing becomes
**formal (P2)** under the 0600 standard, and the disposition pin's record
quote is not reproducible from the shipped assert (**P3**). Nothing else
survives attack.

## What held up under attack

* **P1 R2-1 — the side files are claimed, present-only, and the widened pin
  catches the revert.** `claim_local_store` walks `-wal`/`-shm` through
  `secure_path::open_existing_at`, which is `open_file_at(parent, leaf,
  create=false, truncate=false)` with `O_RDWR|O_NOFOLLOW|O_CLOEXEC` — no
  `O_CREAT`, `Ok(None)` on `NotFound`, and the loop `continue`s. An absent
  side file is never created; an existing one is `set_permissions(0o600)`-ed
  (fchmod on the fd). I confirmed the helper in the tree (`secure_path/mod.rs`
  lines 568-581, 640-652). And the fresh-home question is settled by
  execution, not assumption: after `provision` + a real `open`/`derive`/`close`
  session, all three of `graph.db`, `graph.db-wal` and `graph.db-shm` exist,
  are 0600, and the WAL carries content (the test's own `len() > 0` assertion,
  which I ran twice). The widening stage then set all three to 0644,
  re-provisioned, and read all three back at 0600.
* **P2 R2-2 — the pin observes the fault it names, and the format is
  unchanged.** `name(instant)` is a pure function; `Sandbox::create` calls it
  with `SystemTime::now()`. The format string is byte-identical to the old
  inline one (`mooshik-scratch-{pid}-{subsec_nanos:x}-{counter:x}`, same
  `fetch_add`). The test samples the clock once, builds both names from one
  instant, and asserts they differ — only the counter can separate them, which
  is now literally what the doc comment claims.
* **P3 R2-3 — the header is true of the code.** `session_conflict` in
  `en.toml` is Mooshik's own template; `memory::facts` cuts at the first
  sentence boundary, keeping the holder/age facts and dropping "takeover" and
  the `.mdx` page; the shipped test asserts `!contains("takeover")` and
  `!contains(".mdx")`. The header's "names the holder and no override or page
  this product does not ship" matches all three.
* **P3 R2-4 — the restore lands before the window that justified it.**
  `run()` restores both dispositions after `ratatui::restore()`, and
  `tui_cmd::live` really does close after `run` returns: `draw()` (line 72)
  returns through `tui::run`'s restore, then line 77 runs
  `runtime.block_on(memory.close())`. The readback pin uses a null-`act`
  `sigaction` query (installs nothing) and asserts `sa_sigaction` equality
  around install/restore. `a_termination_signal_asks_the_session_to_leave`
  restores after itself (line 371), so neither the handler nor the disposition
  leaks into the rest of the suite. Non-unix stubs `leave_on_signals() {}` /
  `restore_signals(_previous: ()) {}` match the only call site (`run()` binds
  `()` then passes it). SAFETY comments describe exactly what each block does
  — the out-parameter receives the previous disposition; `previous` is what
  `sigaction` wrote; the query with null `act` installs nothing. Restoring via
  `sigaction(sig, &previous.X, null)` is the correct inverse: the captured
  struct carries handler, flags and mask, and it is what was in force before.
* **P3 R2-5 — the doc matches the code, both legs and the order.** `said` at
  line 464: `contents.contains(said) || said.split(JOIN).all(...)` with
  `JOIN = "; "`. The doc's two over-approximations are the two disjuncts, and
  the surrounding paragraph still says the whole prompt is tried before the
  split — the `||` short-circuit, exactly as written.
* **P3 R2-6 — the false positive is stated and the mechanism is real.**
  Read from the pinned lambo at `4c6fc93` (`git show` from the checkout,
  `src/graph/action.rs:316-321`): `CanonicalizeResult::Matched { key, node }`
  returns `Ok(node)` — the existing node — and `plan` routes the action string
  through exactly this `resolve` with `ConceptType::Resource`, then plans every
  edge with that node as source. So an action that canonicalizes onto an
  existing thought turns that thought into the action node, and the `Causal`
  edge from it never goes away. The doc's sentence is accurate, not hopeful.
* **New code.** `SignalDispositions` carries the two pre-install `sigaction`
  structs; nothing else in `tui/mod.rs` changed shape. The widened pin does
  not rely on a session that never ran — see R2-1. The remediation's own
  mutation quotes are reproduced below; two of three are verbatim, one is not.
* **No behaviour change beyond the six.** The whole diff is: the side-file
  claim loop, the widened pin, two doc paragraphs in `view.rs`, the `name`
  extraction in `scratch.rs`, the signal capture/restore in `tui/mod.rs`, and
  the `tui_cmd.rs` header. Nothing else moved.

## Findings

### P2

**M12a-R3-1 — The scratch sandbox is created world-readable in `/tmp`,
  model-authored code included, on every run.**

`Sandbox::create` makes its directory with `fs::create_dir` and `write_script`
writes the code with `fs::File::create` (`src/tools/scratch.rs:442, 453-456`).
Both take the process umask: under the ordinary 022 (this machine's), the dir
is `0755` and the script `0644` — computed from `0o777 & !umask` / `0o666 &
!umask` — inside `std::env::temp_dir()` (`/tmp`, world-writable and sticky; no
`TMPDIR` here). The sandbox's whole purpose is to hold model-authored code,
which may embed whatever the conversation contained, and nothing in the design
requires it to be readable: the interpreter is exec'd as the invoking user into
the sandbox cwd (direct exec, no shell), so `0700`/`0600` changes nothing
functionally. The exposure window is the run — and longer, because the `Drop`
that removes the directory does not run on a kill (the `name` doc says so in
as many words), so a killed process leaves the script readable until a tmp
reaper runs. Round 2 made the graph 0600 and wrote it up as the standard; this
is the same confidentiality class on a surface that runs on every scratch
tool-call. Pre-existing and not one of the six, which is why it was "noted in
passing" then; it becomes formal now per the cycle-clean mandate.

*Remediation (this cycle's).* Dir-only is sufficient and minimal: create the
directory with `DirBuilder` `mode(0o700)` — or `fs::create_dir` followed by
`PermissionsExt::set_mode(0o700)` — cfg-gated to unix, and either leave the
script at `0600` under the `0700` dir (unreachable to other accounts, since
the dir blocks traversal) or also pin it `0600` via
`OpenOptions::new().write(true).create(true).truncate(true)` + `set_mode`.
Non-unix untouched, matching the file's existing cfg discipline. Pin: a test
that creates a `Sandbox`, writes a script, and asserts dir mode `0700` and
script mode `0600` under a deliberately wide umask (`0o000`-style, or by
setting `set_permissions` to wide first like the graph.db test does) — the
graph.db `0600` test's shape, one directory over.

### P3

**M12a-R3-2 — The disposition-restore pin's assert carries no message, so the
  remediation's quoted mutation output is not reproducible.**

The shipped assertion is `assert_eq!(after.sa_sigaction, before.sa_sigaction,)`
— trailing comma, no message — while the remediation record's mutation table
quotes the failure as "signal 15 was left with the session's handler
installed". No such string exists anywhere in the tree (grepped). I ran the
identical mutation (body of `restore_signals` emptied) and the pin failed with
the bare `assertion 'left == right' failed` / `left: 94363243923696, right: 0`
— handler pointer vs `SIG_DFL`, the substance the record describes, but not
the message it prints. Every other assert in the same file names its subject
("signal {signal} did not end the session", "reading the disposition of
{signal} failed"); this one reports an unlabelled pair of pointer-sized
integers and does not say which signal. The mutation is caught either way, but
the pin's own diagnostics should tell the next reader what failed, and the
record should be reproducible as written.

*Remediation.* Give the assert the message the record already uses:

```rust
assert_eq!(
    after.sa_sigaction,
    before.sa_sigaction,
    "signal {signal} was left with the session's handler installed",
);
```

## Mutation-tested pins

Every mutation transient; the mutated file restored from a byte copy and
`sha256sum`-verified identical to the pre-mutation state after each.

| Mutation | Pin | Result |
| --- | --- | --- |
| side-file loop cut from `claim_local_store` | `the_local_database_is_created_and_repaired_private` | **caught** — "a widened graph.db-wal was left open", `left: 420, right: 384` (0o644 vs 0o600), verbatim |
| counter dropped from the scratch `name` format | `two_sandboxes_opened_in_the_same_instant_are_two_directories` | **caught** — "two names built from one instant must differ" with both names equal (`mooshik-scratch-185155-27aa6e81`), verbatim |
| `restore_signals` emptied | `a_termination_signal_disposition_is_restored_after_the_session` | **caught** — readback shows the handler pointer (`left: 94363243923696`) where `SIG_DFL` was (`right: 0`); the record's quoted message text does not exist in the shipped assert (P3, R3-2) |

## Gates

Run by me, exactly as run:

* `cargo test --locked` (clean env: `LAMBO_POSTGRES_DSN`/`MOOSHIK_POSTGRES_DSN`/
  `DATABASE_URL` unset) → **537 lib passed, 0 failed, 2 ignored** (539 total;
  the two ignored are the pre-existing live-Cloud ones) **+ 1 integration
  passed** (`report_pin`). Matches the remediation's "539 lib + 1 integration,
  0 failed, 2 ignored".
* `cargo test --locked` in my ambient shell (which exports a live
  `LAMBO_POSTGRES_DSN`) → 536 passed, **1 failed**:
  `cli::tests::moving_the_store_is_refused_until_confirmed_and_never_echoes_the_dsn`
  — a pre-existing, patch-untouched test that asserts a `config set` against a
  fixture DSN; the config overlay's `same_database` check then sees my exported
  `LAMBO_POSTGRES_DSN` name a different database and refuses. Re-run with the
  variable unset: **passes**. Environmental, not a regression; recorded so the
  next clean-env run is not surprised.
* `cargo clippy --all-targets --all-features` → clean.
* `cargo fmt --check` → clean.
* File-size cap → clean. `resolve.rs` 299, `ops.rs` 472, `view.rs` 875,
  `scratch.rs` 642, `tui/mod.rs` 731, `tui_cmd.rs` 100 — all under 1000,
  matching the remediation's numbers.
* The three pins individually: all pass on the remediation tree (and the
  file-mode/`-shm` assertions are real — the fresh home truly produced all
  three files, WAL content included).

## What was executed vs. only read

**Executed.** The three mutations above, each against its pinned test, each
reverted and hash-verified. The three pins individually on the remediation
tree. The full suite twice (ambient env and clean env), the failing env test
alone with the variable unset, the integration test. Clippy, fmt, and the
file-size count. The sandbox mode arithmetic (`0o777 & !umask` / `0o666 &
!umask` with this machine's `022`) and the `/tmp`/`TMPDIR` check. The
`restore_signals` mutation twice (once confirming the record's substance).

**Read, not executed.** The pty behaviour of the restore ordering against a
real lease (the round-2 remediation's table; the ordering was traced through
`tui_cmd::live` lines 72/77 and `run()`'s restore — a kill during `close()`
now hits the old disposition by construction). The non-unix stubs were type-
and call-site-verified, not compiled for a non-unix target (none is
available). Lambo at `4c6fc93` read by `git show` from the git checkout:
`action.rs:316-321` (`CanonicalizeResult::Matched` reuse) — the R2-6 mechanism.

## Note for a later round

The **R2-2 pin now tests the extracted `name` directly, not `create`'s
integration with it** — a future change that kept `name` correct but made
`create` bypass it (inline a different format in `create`) would not fail the
pin. Acceptable: the fault the pin guards is the name format, and `create`'s
use of `name` is one call beside `fs::create_dir`. Worth a line in a later
review of this file, same as round 1's standing note that `of_graph` is `pub`
and the lock-order pin reads only `of_memory`.

Also standing from earlier rounds, unchanged: the M12b deferral is on the
record (`PLAN.md` still lacks the guard-duration item and no `M12b` marker
exists in `src/`).