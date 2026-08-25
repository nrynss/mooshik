# M6 implementation

Vault injection into tools and egress redaction: the last two M6 obligations.
Secret values now reach *exactly one* place outside the vault — a child
scratch process's environment — and every tool result is scanned against the
vault before it can reach the model, the transcript, or the graph. A script
echoing `$TOKEN` returns `[REDACTED]`, and that is what gets derived.

## Scope

- `src/vault.rs` — `SharedVault` (`Arc<Mutex<Vault>>`) + `lock_shared`
  (poison-recovering) + `Vault::shared()`; `is_valid_name` predicate so config
  can reject impossible secret references at load time; opaque
  `SecretToken::len`/`is_empty` so redaction can order passes longest-first
  without exposing plaintext.
- `src/tools/redact.rs` (new) — `RedactingTools`, the egress decorator:
  post-execute, pre-history scan of every final tool result against all vault
  values (`list()` → `get()` each, tokens re-resolved per call), replacing
  every occurrence with `[REDACTED]`.
- `src/tools/mod.rs` — `executor_for_chat` composes **gate → redactor →
  tools** and takes the shared vault handle; `MemoryTools` carries the vault;
  `run_scratch` resolves injections after confirm, before spawn.
- `src/tools/scratch.rs` — `resolve_injection` (all-or-nothing, per-run),
  `ScratchConfig.secret_env` (env-var → secret-name table),
  `interpreter_command` — the single place `expose()` feeds `Command::env`.
- `src/config/mod.rs` / `overlay.rs` / `show.rs` — the fail-closed
  `[tools.scratch.env]` table (env identifier × vault name rules,
  `InvalidScratchEnv` fails the whole load); `config show` renders it when set.
- `src/cli.rs` — `chat` opens the vault once via `provider_for` +
  `Vault::open_at`; any failure degrades to `None`, never blocks chat.
- `src/companion/session.rs` — **not touched** (verified: zero diff). The
  decorator composes at injection, so the loop needs no redaction knowledge.

## Decisions taken

1. **One boundary again, documented order.** Composition is
   `GatedTools → RedactingTools → inner`: permission first (a denied call
   never executes anything, so there is nothing to scan), then execute, then
   redact the final string. Redaction lives in a decorator, not inside each
   tool, so every current and future tool — including MCP servers from M10 —
   is covered exactly once by construction. Pinned by source-pin
   (`executor_for_chat_composes_gate_then_redaction_then_tools`) and behavior
   through the full stack.
2. **Availability stance: not all-or-nothing.** A vault that cannot open
   cannot leak either, so chat proceeds unredacted-only-because-unopenable
   with one stderr notice (`tools.vault_unavailable`); an opened-but-empty
   vault makes redaction a cheap pass-through (no allocation beyond the token
   list check). Scratch injection is stricter than redaction by design: if the
   env table is non-empty but no vault handle exists, the script does not run
   (`scratch_env_unavailable`) — half-injected execution is never acceptable.
3. **Per-run resolution, not per-session snapshot.** Both redaction tokens and
   scratch injections resolve through `vault.get` at execution time, so
   `mooshik secret set` rotation is observed by the very next run or tool
   result (pinned by two tests). The mutex is held only for `get`/`list`,
   never across output scanning or process spawn.
4. **Plaintext scope.** Config stores only names. Errors name at most the
   missing secret *name*. The plaintext exists in exactly three places:
   decrypted vault memory (`Zeroizing`), the `SecretToken`, and the child's
   environment after the single `Command::env(var, token.expose())` inside
   `interpreter_command`. Nothing new `{:?}`-prints a carrier; `RedactingTools`'s
   Debug prints only whether a vault is attached.
5. **Longest-value-first redaction.** Overlapping prefixes ("sk-live" vs
   "sk-liveabc") resolve longest first, so a longer secret never leaves a
   mangled partial replacement behind. Ordering uses the opaque length
   accessor rather than leaking value comparisons.
6. **Fail-closed parsing.** `[tools.scratch.env]` keys must be valid env
   identifiers, values must satisfy the vault name rules; anything else fails
   the config load with `config.invalid_scratch_env`, matching the M5
   `[permissions]` posture. Unknown keys under `[tools]` are rejected by
   `deny_unknown_fields`.

## Test matrix (all green)

- Echo round trip: injection reaches the child env; composed executor returns
  `[REDACTED]`; multiple secrets all redacted.
- Overlapping prefixes, empty-token skip, zero-secrets passthrough,
  no-vault passthrough, rotation between calls (`tools::redact`).
- Transcript hygiene pin: after a tool turn carrying a value, `chat.history()`
  and the follow-up request body contain `[REDACTED]` only
  (`companion::loop_tests`).
- Derive-after-redaction pin: echoed `$TOKEN` → redacted result → derive →
  recall shows `[REDACTED]`, value absent from the graph.
- Per-run resolution across rotation; missing secret aborts before spawn with
  a contained error and no partial run; env table without a vault fails closed.
- Config: table parses; bad env names / bad secret names / unknown keys fail
  closed; `config show` round-trip.
- Vault-unavailable chat still starts and answers (behavioral) with the
  en.toml notice pinned (source).
- Gate composition order pinned (source) and gated fallback still answers.

## Adversarial notes

- *Could a tool bypass redaction?* Only by not going through the executor —
  which is the chat loop's only path to the model. `Session` was untouched;
  its tool messages come from `executor.execute`.
- *Could injection leak through stderr of a failed spawn?* Spawn failures map
  to `tools.scratch_spawn_failed` plus the OS error for the interpreter path;
  env contents never appear. Output caps still apply, and captured output is
  redacted at the boundary like everything else.
- *Deadlock/poisoning?* Locks are leaf-scoped; `lock_shared` recovers poisoned
  guards instead of panicking into chat.
