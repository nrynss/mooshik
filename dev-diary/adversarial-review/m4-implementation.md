# M4 implementation

The tool surface: `lambo_recall`, `lambo_derive`, `lambo_stats`, and
`run_scratch_script` behind the existing synchronous `companion::ToolExecutor`
seam, backed by in-process `lambo::Memory`, with lambo's own MCP schemas lifted
verbatim and a panic-contained tool wrapper.

## Files changed

- `Cargo.toml`, `Cargo.lock` — add `schemars = "1"` (JSON-schema generation for
  the lifted lambo parameter types). Lambo stays a git pin
  `f90a662…` with `store-postgres, store-memory, embed-gemini, embed-fixture`.
- `src/lib.rs` — `pub mod tools;`.
- `src/tools/{mod,schema,worker,scratch,tests}.rs` — the four in-scope tools,
  the shared tool runtime, sandboxed scratch runner, and tests.
- `src/companion/tools.rs` — `impl ToolExecutor for Arc<dyn ToolExecutor>`
  blanket, so the injected `Arc<dyn ToolExecutor>` serves directly as the
  `Session` executor type.
- `src/companion/chat.rs` — `run_chat(config, executor: Arc<dyn ToolExecutor>)`;
  the loop stays free of any `crate::memory` reference (M3 pin preserved).
- `src/cli.rs` — `chat()` opens the tool surface via
  `crate::tools::executor_for_chat(&config)` and injects it.
- `src/text/en.toml` — `[tools]` keys; `companion.chat_after_help` updated.

Not touched: `src/secure_path`, `src/vault.rs`, `src/memory` internals.

## Layout

- `src/tools/schema.rs` — lifted lambo schemas (`RecallParams`, `DeriveParams`,
  `WireConcept`, `WireConceptType`, `WireParentOf`, `StatsParams`, plus
  `WireResource` / `RecordActionParams` per M4's contract), the Mooshik-only
  `ScratchParams`, door-validation helpers, and `tool_parameters::<T>()`.
- `src/tools/worker.rs` — `ToolRuntime`: a dedicated Tokio runtime on an OS
  thread; jobs run `FnOnce(&Runtime) -> R` and `execute` blocks on a bounded
  `recv_timeout` per call; panic is caught on the worker and returned as
  `ToolRunError`, keeping the worker alive.
- `src/tools/scratch.rs` — `ScratchConfig` (confirm callback, output cap),
  `Sandbox` (temp dir, direct exec of `bash`/`python3`, never `sh -c`),
  `run_script` with hard timeout, process-group kill, and bounded output reads.
- `src/tools/mod.rs` — `MemoryTools` (the `ToolExecutor`), wiring, and
  `executor_for_chat`.
- `src/tools/tests.rs` — executor-level tests against a fixture embedder.

## Key decisions

1. **Async seam without converting `execute`.** `execute` is synchronous, but
   `Memory::recall`/`derive` are async and `Session` drives tool calls in an
   async loop. `ToolRuntime` pins one Tokio multi-thread runtime to a dedicated
   OS thread at construction; `execute` submits a `FnOnce(&Runtime) -> R` job
   over an mpsc channel and blocks on `recv_timeout(LAMBO_CALL_WAIT = 60s)`.
   A panic is caught on the worker (`catch_unwind`) and surfaces as a
   `ToolRunError::Panicked` (the reply-channel disconnect means the worker loop
   survives); `execute` further wraps dispatch in a second `catch_unwind` as a
   single containment entry point. The open-memory path builds a short-lived
   runtime in plain sync context — never `block_on` inside chat's async runtime,
   and never converting `ToolExecutor::execute` to async (which would ripple
   through the whole tool-call protocol and M3 pins).
2. **Memory injection + pin reconciliation.** `cli.rs::chat` calls
   `executor_for_chat`, which opens an in-process `Memory` via the public
   `crate::memory::open` under `tokio::time::timeout(OPEN_WAIT = 20s)`. On
   failure (e.g. no Postgres DSN) it degrades to `Arc::new(NoopExecutor)` and
   prints `tools.chat_memory_unavailable`, so chat still runs. `run_chat`/the
   chat loop carries zero `crate::memory` references, so the two M3 pins
   (`run_chat_does_not_open_memory`, `chat_dispatch_does_not_open_memory`), which
   scan production source text, hold literally.
3. **Schemas lifted, not rewritten.** The lambo structs are copied verbatim from
   `lambo/src/mcp/server.rs` with `deny_unknown_fields` + length/range caps.
   `tool_parameters::<T>()` resolves schemars 1.2.2's root `$ref` into a
   `type: object` (inlining the referenced definition, keeping `definitions` for
   nested refs). Because `#[schemars]` only shapes the generated schema, every
   executor re-validates at the door (`check_size`, `ranged()`, caps) so an
   over-length or wrongly-typed parameter is a tool-error string, never a panic.
4. **Sandbox scope is honest.** The sandbox is the isolated temp working
   directory plus direct exec of a whitelisted interpreter; content-level path
   checks refuse only a first token that is an absolute path or a `..`-escape,
   not ordinary absolute paths inside a multi-line script (`cat /etc/hostname`
   is allowed). The interpreter child runs in its own process group (`setsid` on
   unix) so a hard-timeout kill takes down the whole tree (e.g. a `sleep`
   grandchild holding the stdout pipe) and is reaped — no orphans, no zombies.
5. **`lambo_stats` reports `never-issued` receipts.** M4's `derive` is the
   synchronous `Memory::derive` (full `DeriveOutcome`), so there are no async
   receipts; `stats.receipt` is accepted for schema compatibility and reported as
   `"state": "never-issued"`.

## Tests added

Schema: inlined root object with required fields and surviving caps; scratch
caps; `check_size` counts characters, not bytes.

Worker: a panicking job is contained and the worker survives; a timed-out job
does not kill the worker.

Scratch: validation (empty code, escaping first paths, NUL bytes, oversized
code); runs successfully in the sandbox dir; non-zero exit with output;
semicolons are script content, not injection; output is capped; hard timeout
kills the child and reports `timed_out`.

Executor: the four in-scope tools in stable order with `type: object`
parameters; derive→recall round trip; `parent_of` creates both concepts;
`stats` observes derives; unknown field refused; over-length query refused;
wrong-typed knob refused; out-of-range `top_k` refused; scratch denied when
confirmation refuses and runs when confirmed; scratch timeout reported by the
executor; `for_chat` returns `None` when memory cannot open.

Final suite: `cargo test --locked` → `test result: ok. 125 passed; 0 failed; 1 ignored`
(the ignored test is the pre-existing live-CloudSQL/Gemini round trip, untouched).