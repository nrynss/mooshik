# M12d round-9 remediation

Remediates the one finding in `m12d-round9.md` — P3 (the README
availability reject still lost to a comma plus a determiner other than
`the`/`a`). Documentation/pin only: production README and `tui_cmd::live`
are unchanged. No deferrals. Base: branch `m12d-watcher` at `14135e0`;
round-6 through round-8 remediations already in the dirty tree were
kept. Nothing committed.

## M12d-R9-1 (P3) — flatten punctuation, then `available` … `without` … `watcher`

### What was wrong

`readme_claims_available_without_watcher` rejected exact `without the
watcher` / `without a watcher`, or `split_once("available without")`
then later `watcher`. A comma between `available` and `without` broke
the split; swapping the article for `our`/`this`/`any` missed the
three-word phrases. Executed: `The pane remains available, without our
watcher` (and `this` / `any`) left all 36 watcher tests green.

Adding more exact determiners is how rounds 7–8 lost. The pin has to
stop matching phrases.

### The fix

Non-alphanumeric characters become spaces before tokenising. The helper
then treats word order `available` … `without` … `watcher` as an
availability claim regardless of the determiner, and still rejects
`without the watcher` / `without a watcher`. Positive conjuncts
(`fails closed at TUI startup`, `The watcher stops with the pane`) and
`pane.close()` are unchanged. README production text is unchanged.

`readme_reject_sees_available_without_watcher_through_punct_and_determiners`
feeds the three executed dodges directly into the helper so the pin
does not depend on mutating README.md.

### Pins

| Claim | Result |
|---|---|
| `The pane remains available, without our watcher` | **caught** (new helper test) |
| `available, without this watcher` | **caught** |
| `Live watching is available, without any watcher` | **caught** |
| production README | helper returns false; `live_watching_fails_closed_at_tui_startup` still green |

## Gates

Clean env (`LAMBO_POSTGRES_DSN` / `MOOSHIK_POSTGRES_DSN` / `DATABASE_URL`
unset):

* `cargo test --locked --lib cli::watcher` → **37 passed, 0 failed, 0
  ignored**
* `cargo fmt --check` → clean

## Residue

None of the same class that is worth another review round: this was a
documentation pin. Production fail-closed and Unknown-head remediations
from rounds 6–8 are untouched. `src/tui/` untouched.
