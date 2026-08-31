//! The view against an open session: the store the product runs on, the lock
//! order the workspace is read in, and the status bar that reports the session
//! itself.
//!
//! Split from `view_tests.rs` because these are the tests that hold a
//! [`Memory`] rather than a graph built by hand — and because that file reached
//! the repo's own thousand-line cap, which is the point at which a file stops
//! being one thing.

use super::tests::{before, draws_everywhere, figures, long_ago, threaded, trickled, Corpus};
use super::*;

use lambo::ConceptType;
use std::collections::{HashMap, HashSet};
use syn::visit::Visit;

/// End to end, through the code `mooshik tui` runs and the store it runs on: an
/// open session, written to, **closed, reopened**, and read back as the
/// workspace the screens draw.
///
/// The offline suite above builds graphs by hand, which is fast and pins the
/// arithmetic — and cannot notice if `of_memory` reads the wrong handle, or if
/// `derive` does not leave behind the `Derives` edges the recurrence count is
/// made of. This one goes through `memory::open`, Lambo's own write path and
/// `Memory::close`, which is the ladder the post-M10 review says a fixture-only
/// suite cannot climb.
///
/// **On sqlite, which is the local store the product runs on.** The in-memory
/// store serializes nothing and reloads nothing, so it cannot see an adapter
/// that drops `prompt_text` or `event_time` on the way through — and post-M10's
/// own lesson is one sentence long: a test that runs only against the in-memory
/// store cannot catch an adapter bug, and the product store is the one that was
/// broken.
#[tokio::test]
async fn a_live_session_survives_the_store_and_fills_the_workspace() {
    let home = crate::secure_path::canonical_temp_dir().join(format!(
        "mooshik-view-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&home).unwrap();

    let mut config = crate::config::Config::default();
    config.store.kind = lambo::StoreKind::Sqlite;
    config.store.path = Some(home.join("graph.db").to_string_lossy().into_owned());
    config.embedder.kind = lambo::EmbedderKind::Fixture;
    config.embedder.dim = 1024;
    config.session.id = "mooshik".to_owned();
    config.session.agent = "mooshik".to_owned();

    crate::memory::provision(&config).await.unwrap();
    let memory = crate::memory::open(&config).await.unwrap();

    // Two turns reaching the same thought, and one reaching another — the
    // shape a thread is made of, written the way the product writes it.
    for _ in 0..2 {
        memory
            .derive(
                &[("block, never drop", ConceptType::Entity)],
                &lambo::graph::derive::ParentOf::none(),
            )
            .await
            .unwrap();
    }
    memory
        .derive(
            &[("the cache lives on the NAS", ConceptType::Entity)],
            &lambo::graph::derive::ParentOf::none(),
        )
        .await
        .unwrap();
    memory.close().await.unwrap();

    // Reopened from the file: everything below came back off a disk.
    let memory = crate::memory::open(&config).await.unwrap();
    // Drawn at the wall clock the writes were stamped with, because the live
    // path has no event time and falls back to it.
    let workspace = of_memory(&memory, chrono::Local::now());
    memory.close().await.unwrap();
    let _ = std::fs::remove_dir_all(&home);

    assert_eq!(
        threaded(&workspace),
        ["block, never drop"],
        "the re-derived thought is not the thread"
    );
    assert!(
        workspace.threads[0].days[WEEK - 1],
        "today's mark is not set on a thought derived today"
    );
    let trickle = trickled(&workspace);
    assert!(
        trickle.contains(&"the cache lives on the NAS".to_owned()),
        "{trickle:?}"
    );
    // And the day's log is empty, because a `derive` restates its own concepts
    // and nothing in this product has said anything else yet. This is the
    // assertion the panel's honesty rests on: it used to read
    // "block, never drop; block, never drop" off the wire.
    assert!(
        workspace.today.entries.is_empty(),
        "a derive's echo reached the day's log: {:?}",
        workspace.today.entries
    );
    assert!(!workspace.now.time.is_empty());
    draws_everywhere(&workspace);
}

/// The figures are read before the graph guard is taken, and that is not a
/// preference.
///
/// `Memory::stats` takes the graph lock itself and `parking_lot`'s read lock is
/// not recursion-safe, so a writer queued between the two acquires deadlocks a
/// thread already holding one reader — no error, no timeout, the pane simply
/// stops.
///
/// **Pinned as source order, because the fault cannot be executed by a test that
/// has to return.** A watchdog test was written and measured against the
/// reversed order first: it either wedges the whole suite — the leaked reader
/// keeps the guard, so the writer thread it was racing blocks behind it and
/// never joins — or it races the collision window and reports green on code that
/// hangs the pane. `of_graph`'s parameter order is the other half of this: the
/// figures come first, so the one-expression form of the call is the safe one.
#[test]
fn the_figures_are_read_before_the_graph_guard() {
    let source = include_str!("view.rs");
    let body = source
        .split("pub fn of_memory")
        .nth(1)
        .expect("of_memory is defined")
        .split("fn of_graph")
        .next()
        .expect("of_graph follows it");
    let figures = body.find("memory.stats()").expect("the figures are read");
    let guard = body
        .find("memory.graph().read()")
        .expect("the graph guard is taken");
    assert!(
        figures < guard,
        "of_memory takes the graph guard before reading the figures, which deadlocks \
         under a queued writer"
    );
}

/// The build runs against the copied data, not the guard, so a writer is never
/// starved for the length of a rebuild.
///
/// [`of_graph`] takes [`ViewData`] and nothing else, so the span hazard is
/// entirely [`of_memory`]'s body: the guard must be taken only to copy the
/// graph out, and the build must follow the copy. The pin proves that shape
/// on the **parsed AST** of the body — `syn` tokenizes the slice, so parens,
/// whitespace, line breaks and comments are gone as disguise, and the checks
/// are structural, not textual. What they prove:
///
/// 1. **No macro invocation anywhere in the fn** (`Expr::Macro`). An
///    invocation's expansion is invisible to this AST pin too, so it could
///    acquire the guard with no acquisition tokens of its own; a unary
///    `!(x)` parses as `Expr::Unary`, never a macro, so a legitimate
///    not-expression passes.
/// 2. **The copy is one top-level `let graph = { … }` statement** — the only
///    statement of the body binding a block to `graph` — the figures are
///    read first (`let stats = memory.stats();`), and a top-level
///    `of_graph(…)` statement follows the copy. The guard binding's scope is
///    the block, so "block closes before the build statement" is "guard
///    dropped before the build runs" for every natural use of the binding (a
///    deliberate value-escape of it is the second documented limit below).
/// 3. **The `memory` parameter is confined to the copy's block.** No
///    expression outside that block may reference `memory`; the single
///    exception is the pre-block `let stats = memory.stats();` call,
///    whitelisted exactly. Every acquisition outside the block references
///    `memory` however it is spelled — the receiver respelled with parens,
///    derefs, blocks or casts, a UFCS path (`Memory::graph(&memory)`,
///    `parking_lot::RwLock::read(memory.graph())`), a binding alias
///    (`let g = &memory;`), a read-family method name (`try_read()`), or a
///    helper call taking the receiver (`read_lock(&memory)`) — so the
///    confinement catches the whole receiver-respelling and indirection
///    family at the token level, and no guard can be bound at function scope
///    after the block's close.
/// 4. **Exactly one graph-guard acquisition exists, inside the copy's
///    block, and it is the initializer of a local binding.** An acquisition
///    is a read-family method call (`read`, `read_recursive`, `try_read`,
///    `try_read_recursive`) on the memory graph's lock, or a read-family
///    call taking the graph as its first argument; the receiver is resolved
///    by unwrapping parens, single-expression blocks (an `unsafe` one
///    included), references, derefs and casts, through a one-pass alias map
///    (`let g = &memory;` collected over the body), and the UFCS spellings
///    are resolved the same way. An acquisition counts only as a `let`
///    initializer, so one consumed as a call argument — `std::mem::forget(
///    memory.graph().read())` — is not the guard and fails the count. The
///    guard the count sees is a bound local whose binding scope is the
///    block.
///
/// Together: the one guard binding's scope is the copy's block, the block
/// closes before the build statement, and no other acquisition can exist in
/// the body. A flat guard-copy-build form has no `let graph = { … }` at all
/// (check 2); a hoisted guard, a decoy, a nested closure/fn/match-arm copy,
/// and a spaced, multiline or `unsafe`-wrapped acquisition after the close
/// all reference `memory` outside the block (check 3); a second acquisition,
/// an in-block helper call that could hide one, or an acquisition consumed
/// as a call argument fails check 4 (the count).
///
/// What no token-level pin can see is two deliberate sabotage classes, both
/// changes an author makes on purpose rather than a natural refactor of the
/// shipped shape. One moves the acquisition **out of** [`of_memory`]'s body
/// entirely — a helper that returns the guard, or a caller that takes the
/// copy with it — because the body then contains no acquisition tokens at
/// all. The other escapes the **value** of the one bound guard:
/// `std::mem::forget(guard)`, `Box::leak(Box::new(guard))` or
/// `ManuallyDrop::new(guard)` after the binding consumes the value and never
/// runs `Drop`, so the read lock is held across the build and forever —
/// while the count still sees the binding and passes. The first is a change
/// to *where* the guard is taken; the second consumes what the body already
/// binds; both are beyond the named fault — the pin pins the body's shape
/// and the guard's binding, not what a sabotage author does to the value —
/// and the R2-2 precedent for a documented limit applies to both, exactly as
/// the module-level `macro_rules!` did for the text pin.
#[test]
fn the_build_runs_against_the_copy_and_not_the_guard() {
    let source = include_str!("view.rs");
    // The same body slice the text pin used: from `pub fn of_memory` to just
    // before `fn of_graph`. The slice also carries of_graph's doc comment
    // after of_memory's own close, so it is truncated at the body's closing
    // brace before parsing — the slice must be exactly one item for the
    // parser to judge it as a whole.
    let slice = source
        .split("pub fn of_memory")
        .nth(1)
        .expect("of_memory is defined")
        .split("fn of_graph")
        .next()
        .expect("of_graph follows it");
    let close = body_close(slice);
    let item = if close < slice.len() {
        &slice[..=close]
    } else {
        slice
    };
    let of_memory: syn::ItemFn = syn::parse_str(&format!("pub fn of_memory{item}")).expect(
        "of_memory's body slice parses as a function item: the pin judges \
                 the AST, and cannot judge source it cannot parse",
    );

    // Check 1: no macro invocation anywhere in the fn. An invocation's
    // expansion is invisible to this AST pin, so it could acquire the graph
    // guard at function scope and hold it across the build. A unary `!(x)`
    // is an `Expr::Unary`, never a macro, so legitimate not-expressions
    // pass.
    let mut macros = MacroHunter(false);
    macros.visit_item_fn(&of_memory);
    assert!(
        !macros.0,
        "no macro invocation may appear in of_memory's body: an invocation's \
         expansion is invisible to this AST pin, so it could acquire the graph \
         guard at function scope and hold it across the build"
    );

    // Check 2: the copy is one top-level `let graph = { … }` statement; the
    // figures statement precedes it; the build statement follows it.
    let stmts = &of_memory.block.stmts;
    let copy_at = stmts.iter().position(is_copy_block_stmt).expect(
        "the copy is a block: of_memory must copy the graph out from \
                 under the guard inside a `let graph = { … }` statement, so the \
                 guard's binding scope ends at the block's close",
    );
    assert_eq!(
        stmts.iter().filter(|s| is_copy_block_stmt(s)).count(),
        1,
        "exactly one `let graph = {{ … }}` statement may appear at of_memory's top \
         level: the copy is anchored by structure, so a decoy block cannot be the \
         block the guard drops inside"
    );
    let stats_at = stmts.iter().position(is_stats_stmt).expect(
        "the figures are read first: a `let stats = memory.stats();` \
                 statement must precede the copy",
    );
    assert!(
        stats_at < copy_at,
        "the figures must be read before the copy: `Memory::stats` takes the \
         graph lock itself, and the read lock is not recursion-safe"
    );
    let build_at = stmts.iter().position(is_build_stmt).expect(
        "the build follows the copy: the body must end with a top-level \
                 `of_graph(…)` statement",
    );
    assert!(
        build_at > copy_at,
        "the copy's block must close before the build: a guard bound inside the \
         block drops at its close, so a build folded into the block (or a build \
         before it) holds the guard across the work"
    );

    // Check 3: the `memory` parameter is confined to the copy's block. Every
    // expression outside the block — except the whitelisted pre-block
    // `let stats = memory.stats();` — must contain no reference to `memory`.
    // An acquisition outside the block references `memory` however it is
    // spelled, so the confinement catches the receiver-respelling and
    // indirection family at the token level, and no guard can be bound at
    // function scope after the block's close.
    for (at, stmt) in stmts.iter().enumerate() {
        if at == copy_at || at == stats_at {
            continue;
        }
        let mut hunter = MemoryRefHunter(false);
        hunter.visit_stmt(stmt);
        assert!(
            !hunter.0,
            "no expression outside the copy's block may reference the `memory` \
             parameter: the guard is taken only inside the block, so a memory \
             reference after its close is an acquisition (or the receiver, alias \
             or argument of one) bound at function scope and held across the build"
        );
    }

    // Check 4: exactly one graph-guard acquisition, inside the copy's block,
    // and it is the initializer of a local binding. The alias map is
    // collected in one pass over the body, then every statement is measured
    // against the structural definition — an acquisition counts only as a
    // `let` initializer, so one consumed as a call argument is not the guard
    // and fails the count. (What the count cannot see is a deliberate
    // value-escape of the bound guard — the second documented limit in the
    // pin's doc.)
    let aliases = collect_aliases(&of_memory);
    let mut all = AcquisitionHunter {
        aliases: &aliases,
        found: 0,
    };
    all.visit_item_fn(&of_memory);
    assert_eq!(
        all.found, 1,
        "exactly one graph-guard acquisition may appear in of_memory's body: a \
         read-family method call (`read`, `read_recursive`, `try_read`, \
         `try_read_recursive`) on the memory graph's lock, or a read-family call \
         taking the graph as its first argument, however the receiver is spelled"
    );
    let mut in_block = AcquisitionHunter {
        aliases: &aliases,
        found: 0,
    };
    in_block.visit_stmt(&stmts[copy_at]);
    assert_eq!(
        in_block.found, 1,
        "the one acquisition must sit inside the copy's block, so the guard it \
         binds drops at the block's close, before the build runs"
    );
}

/// The byte offset of the closing brace of the fn's body within the slice,
/// scanning brace depth with comments and literals skipped, so a brace inside
/// a string or a comment cannot close the body early. The slice runs from
/// of_memory's signature to just before `fn of_graph` and also carries
/// of_graph's doc comment after of_memory's own close; truncating at the
/// body's close makes the slice exactly one item for the parser.
fn body_close(slice: &str) -> usize {
    let bytes = slice.as_bytes();
    let mut depth = 0usize;
    let mut at = 0usize;
    while at < bytes.len() {
        if bytes[at] == b'/' && bytes.get(at + 1) == Some(&b'/') {
            at = slice[at..].find('\n').map_or(bytes.len(), |i| at + i + 1);
        } else if bytes[at] == b'/' && bytes.get(at + 1) == Some(&b'*') {
            at = slice[at + 2..]
                .find("*/")
                .map_or(bytes.len(), |i| at + 2 + i + 2);
        } else if literal_len(&slice[at..]) > 0 {
            at += literal_len(&slice[at..]);
        } else {
            match bytes[at] {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return at;
                    }
                }
                _ => {}
            }
            at += 1;
        }
    }
    slice.len()
}

/// The byte length of the literal at the start of `rest`, or `0` when there
/// is none: `"…"` strings, `'…'` char literals (which never span a line, so
/// a bare lifetime `'a` is left alone), byte strings `b"…"` / `b'…'`, and
/// raw strings `r"…"`, `r#"…"#`, `br"…"`, `br#"…"#`, `cr"…"`, `cr#"…"#`.
/// Escapes (`\"`, `\\`, `\u{…}`) are consumed so a quote inside a literal
/// cannot end it early; an unterminated literal consumes the rest of the
/// body.
fn literal_len(rest: &str) -> usize {
    let bytes = rest.as_bytes();
    // Raw strings open with `r`, `br` or `cr`, then `#`s, then `"`, and
    // close with `"` followed by the same run of `#`s.
    if bytes.first() == Some(&b'r')
        || (matches!(bytes.first(), Some(&b'b') | Some(&b'c')) && bytes.get(1) == Some(&b'r'))
    {
        let prefix = if matches!(bytes.first(), Some(&b'b') | Some(&b'c')) {
            2
        } else {
            1
        };
        let hashes = bytes[prefix..].iter().take_while(|&&b| b == b'#').count();
        if bytes.get(prefix + hashes) != Some(&b'"') {
            return 0;
        }
        let body = &bytes[prefix + hashes + 1..];
        let end = body
            .windows(1 + hashes)
            .position(|win| win[0] == b'"' && win[1..].iter().all(|&b| b == b'#'))
            .map_or(body.len(), |i| i);
        return prefix + hashes + 1 + end + 1 + hashes;
    }
    let (quote, start) = match bytes.first() {
        Some(&b'"') => (b'"', 0),
        Some(&b'\'') => (b'\'', 0),
        Some(&b'b') if bytes.get(1) == Some(&b'"') => (b'"', 1),
        Some(&b'b') if bytes.get(1) == Some(&b'\'') => (b'\'', 1),
        Some(&b'c') if bytes.get(1) == Some(&b'"') => (b'"', 1),
        _ => return 0,
    };
    let mut at = start + 1;
    while at < bytes.len() {
        match bytes[at] {
            b'\\' => {
                if bytes.get(at + 1) == Some(&b'u') {
                    let close = bytes[at + 2..].iter().position(|&b| b == b'}');
                    at = close.map_or(bytes.len(), |i| at + 3 + i);
                } else {
                    at += 2;
                }
            }
            b'\n' if quote == b'\'' => return 0, // a char literal never spans a line
            b if b == quote => return at + 1,
            _ => at += 1,
        }
    }
    bytes.len()
}

/// The `memory` parameter with parens, single-expression blocks (an
/// `unsafe` one included), references, derefs and casts peeled off, so
/// every receiver respelling resolves to the same core: `&memory`,
/// `(*memory)`, `{ memory }`, `unsafe { memory }` and `memory as _` all
/// yield `memory`.
fn unwrap(expr: &syn::Expr) -> &syn::Expr {
    match expr {
        syn::Expr::Paren(e) => unwrap(&e.expr),
        syn::Expr::Group(e) => unwrap(&e.expr),
        syn::Expr::Reference(e) => unwrap(&e.expr),
        syn::Expr::Unary(e) if matches!(e.op, syn::UnOp::Deref(_)) => unwrap(&e.expr),
        syn::Expr::Cast(e) => unwrap(&e.expr),
        syn::Expr::Block(e) if e.block.stmts.len() == 1 => {
            if let syn::Stmt::Expr(inner, None) = &e.block.stmts[0] {
                unwrap(inner)
            } else {
                expr
            }
        }
        syn::Expr::Unsafe(e) if e.block.stmts.len() == 1 => {
            if let syn::Stmt::Expr(inner, None) = &e.block.stmts[0] {
                unwrap(inner)
            } else {
                expr
            }
        }
        _ => expr,
    }
}

/// Whether the expression is the `memory` parameter or a body-level alias of
/// it, after unwrapping. The alias map is collected in one pass over the
/// body (`let g = &memory;` records `g → memory`); following it is recursive
/// so aliases of aliases resolve too.
fn resolves_to_memory(expr: &syn::Expr, aliases: &HashMap<String, String>) -> bool {
    match unwrap(expr) {
        syn::Expr::Path(p) => match p.path.get_ident() {
            Some(id) if id == "memory" => true,
            Some(id) => alias_resolves_to_memory(&id.to_string(), aliases),
            None => false,
        },
        _ => false,
    }
}

/// Whether the expression is the memory graph's lock: a `graph()` method
/// call on the memory parameter (or an alias of it), or a call whose callee
/// ends in `graph` taking the memory parameter as its first argument — the
/// `Memory::graph(&memory)` UFCS spelling.
fn resolves_to_memory_graph(expr: &syn::Expr, aliases: &HashMap<String, String>) -> bool {
    match unwrap(expr) {
        syn::Expr::MethodCall(call) if call.method == "graph" => {
            resolves_to_memory(&call.receiver, aliases)
        }
        syn::Expr::Call(call) => {
            let last = match &*call.func {
                syn::Expr::Path(p) => p.path.segments.last().map(|s| s.ident.to_string()),
                _ => None,
            };
            last.as_deref() == Some("graph")
                && call
                    .args
                    .first()
                    .is_some_and(|arg| resolves_to_memory(arg, aliases))
        }
        _ => false,
    }
}

/// Follow the alias chain (`g → memory`, or `h → g → memory`) with a cycle
/// guard.
fn alias_resolves_to_memory(name: &str, aliases: &HashMap<String, String>) -> bool {
    let mut current = name;
    let mut seen = HashSet::new();
    while seen.insert(current.to_string()) {
        match aliases.get(current) {
            Some(target) if target == "memory" => return true,
            Some(target) => current = target,
            None => return false,
        }
    }
    false
}

/// Whether the expression is a graph-guard acquisition, structurally defined:
/// a read-family method call (`read`, `read_recursive`, `try_read`,
/// `try_read_recursive`) on the memory graph's lock, or a read-family call
/// taking the graph as its first argument
/// (`parking_lot::RwLock::read(memory.graph())`).
fn is_acquisition(expr: &syn::Expr, aliases: &HashMap<String, String>) -> bool {
    const READ_FAMILY: [&str; 4] = ["read", "read_recursive", "try_read", "try_read_recursive"];
    match expr {
        syn::Expr::MethodCall(call) => {
            READ_FAMILY.contains(&call.method.to_string().as_str())
                && resolves_to_memory_graph(&call.receiver, aliases)
        }
        syn::Expr::Call(call) => {
            let last = match &*call.func {
                syn::Expr::Path(p) => p.path.segments.last().map(|s| s.ident.to_string()),
                _ => None,
            };
            matches!(last.as_deref(), Some(name) if READ_FAMILY.contains(&name))
                && call
                    .args
                    .first()
                    .is_some_and(|arg| resolves_to_memory_graph(arg, aliases))
        }
        _ => false,
    }
}

/// One pass over the body collecting the alias map: every `let` binding
/// whose initializer resolves to `memory` (through the aliases seen so far)
/// records `name → <core ident>`. Shadowing overwrites; the map is
/// deliberately scope-blind, which errs toward fail-closed (an out-of-scope
/// name still resolves to memory and is counted).
fn collect_aliases(item: &syn::ItemFn) -> HashMap<String, String> {
    struct Collector<'a>(&'a mut HashMap<String, String>);
    impl<'a, 'ast> syn::visit::Visit<'ast> for Collector<'a> {
        fn visit_stmt(&mut self, node: &'ast syn::Stmt) {
            if let syn::Stmt::Local(local) = node {
                if let (Some(init), syn::Pat::Ident(pat)) = (&local.init, &local.pat) {
                    if let syn::Expr::Path(p) = unwrap(&init.expr) {
                        if let Some(id) = p.path.get_ident() {
                            let target = id.to_string();
                            if target == "memory" || self.0.contains_key(&target) {
                                self.0.insert(pat.ident.to_string(), target);
                            }
                        }
                    }
                }
            }
            syn::visit::visit_stmt(self, node);
        }
    }
    let mut aliases = HashMap::new();
    Collector(&mut aliases).visit_item_fn(item);
    aliases
}

/// Whether the statement is the copy: a top-level `let graph = { … }`.
fn is_copy_block_stmt(stmt: &syn::Stmt) -> bool {
    matches!(
        stmt,
        syn::Stmt::Local(local)
            if matches!(&local.pat, syn::Pat::Ident(pat) if pat.ident == "graph")
                && matches!(&local.init, Some(init) if matches!(&*init.expr, syn::Expr::Block(_)))
    )
}

/// Whether the statement is the whitelisted figures call:
/// `let stats = memory.stats();`.
fn is_stats_stmt(stmt: &syn::Stmt) -> bool {
    matches!(
        stmt,
        syn::Stmt::Local(local)
            if matches!(&local.pat, syn::Pat::Ident(pat) if pat.ident == "stats")
                && matches!(
                    &local.init,
                    Some(init)
                        if matches!(
                            &*init.expr,
                            syn::Expr::MethodCall(call)
                                if call.method == "stats"
                                    && matches!(
                                        &*call.receiver,
                                        syn::Expr::Path(p) if p.path.is_ident("memory")
                                    )
                        )
                )
    )
}

/// Whether the statement is the build call: a top-level `of_graph(…)`.
fn is_build_stmt(stmt: &syn::Stmt) -> bool {
    matches!(
        stmt,
        syn::Stmt::Expr(expr, _)
            if matches!(
                expr,
                syn::Expr::Call(call)
                    if matches!(&*call.func, syn::Expr::Path(p) if p.path.is_ident("of_graph"))
            )
    )
}

/// Finds every `Expr::Macro` anywhere in the fn — an invocation's expansion
/// is invisible to the AST, so any macro fails the pin closed.
struct MacroHunter(bool);

impl<'ast> syn::visit::Visit<'ast> for MacroHunter {
    fn visit_expr_macro(&mut self, _node: &'ast syn::ExprMacro) {
        self.0 = true;
    }
}

/// Finds any expression whose tokens include the `memory` identifier.
struct MemoryRefHunter(bool);

impl<'ast> syn::visit::Visit<'ast> for MemoryRefHunter {
    fn visit_expr(&mut self, node: &'ast syn::Expr) {
        if self.0 {
            return;
        }
        if matches!(node, syn::Expr::Path(p) if p.path.is_ident("memory")) {
            self.0 = true;
            return;
        }
        syn::visit::visit_expr(self, node);
    }
}

/// Counts the graph-guard acquisitions in the walked tree. An acquisition
/// counts only as the initializer of a local binding (after unwrapping
/// parens, single-expression blocks, references, derefs and casts), so an
/// acquisition consumed as a call argument — `std::mem::forget(memory.graph().
/// read())` — is not the guard and is invisible to the count.
struct AcquisitionHunter<'a> {
    aliases: &'a HashMap<String, String>,
    found: usize,
}

impl<'ast> syn::visit::Visit<'ast> for AcquisitionHunter<'_> {
    fn visit_stmt(&mut self, node: &'ast syn::Stmt) {
        if let syn::Stmt::Local(local) = node {
            if let Some(init) = &local.init {
                if is_acquisition(unwrap(&init.expr), self.aliases) {
                    self.found += 1;
                }
            }
        }
        syn::visit::visit_stmt(self, node);
    }
}

/// A rebuild reads the graph again, so a write from anywhere else appears in
/// the pane without a keystroke.
///
/// This is the live rebuild path in miniature: `mooshik tui` hands the event
/// loop a closure that answers `of_memory(&memory, now)`, and the loop calls
/// it once per quiet tick. Here the same closure is called twice with a write
/// in between — the derive is the write the ingester or an MCP client makes
/// from elsewhere — and the second answer must show it. The terminal the loop
/// runs on is not this test's concern; the seam is.
#[tokio::test]
async fn a_rebuild_sees_a_write_from_elsewhere_without_a_keystroke() {
    let home = crate::secure_path::canonical_temp_dir().join(format!(
        "mooshik-tick-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&home).unwrap();

    let mut config = crate::config::Config::default();
    config.store.kind = lambo::StoreKind::Sqlite;
    config.store.path = Some(home.join("graph.db").to_string_lossy().into_owned());
    config.embedder.kind = lambo::EmbedderKind::Fixture;
    config.embedder.dim = 1024;
    config.session.id = "mooshik".to_owned();
    config.session.agent = "mooshik".to_owned();

    crate::memory::provision(&config).await.unwrap();
    let memory = crate::memory::open(&config).await.unwrap();

    // The seam `tui_cmd::live` hands the loop: the graph as of now.
    let rebuild = || of_memory(&memory, chrono::Local::now());
    let before = rebuild();
    assert!(
        !trickled(&before).contains(&"the tick saw the write".to_owned()),
        "{:?}",
        trickled(&before)
    );

    // The write from elsewhere lands between two ticks.
    memory
        .derive(
            &[("the tick saw the write", lambo::ConceptType::Entity)],
            &lambo::graph::derive::ParentOf::none(),
        )
        .await
        .unwrap();

    let after = rebuild();
    assert!(
        trickled(&after).contains(&"the tick saw the write".to_owned()),
        "a rebuild after a write does not show it: {:?}",
        trickled(&after)
    );
    draws_everywhere(&after);
    memory.close().await.unwrap();
    let _ = std::fs::remove_dir_all(&home);
}

/// The status bar says what the session is doing, and only red-free words.
#[test]
fn the_status_bar_reports_the_session_rather_than_flattering_it() {
    let keeping_up = health(&figures(), None);
    assert!(keeping_up.well);
    assert_eq!(keeping_up.state, "Keeping up");

    let behind = health(
        &MemoryStats {
            log_depth: 12,
            ..figures()
        },
        None,
    );
    assert!(!behind.well);
    assert_eq!(behind.state, "Catching up");

    let broken = health(
        &MemoryStats {
            degraded: true,
            log_depth: 12,
            ..figures()
        },
        None,
    );
    assert!(!broken.well);
    assert_eq!(broken.state, "Not saving");
}

/// Both scopes are written, and they are two different sentences.
///
/// The model documents them as such and says why the short one is not a
/// truncation: cutting "214 things remembered, back to 21 August" to the
/// 80-column slot yields "214 things remembered, back t…", which reads as a bug.
/// Both fields used to hold the long form, so the narrow screens drew the wide
/// string; and the long form had nothing to say about how far back the session
/// went, which M12a is the milestone that can answer.
#[test]
fn the_scope_says_how_far_back_the_session_goes_and_the_short_form_does_not() {
    let mut corpus = Corpus::new();
    corpus.turn(Some("Six days ago"), before(0, 0), Some(before(6, 0)));
    corpus.turn(Some("This morning"), before(0, 0), Some(before(0, 2)));
    // Older than the week on screen: the far end of the session, not of the week.
    corpus.turn(Some("Long ago"), before(0, 0), Some(long_ago()));

    let health = corpus.view().health;
    assert_eq!(health.scope, "214 things remembered, back to 15 June");
    assert_eq!(health.short_scope, "214 remembered");

    // A session with nothing in it has no far end to name, and says the shorter
    // true thing rather than an invented date.
    assert_eq!(
        Corpus::new().view().health.scope,
        "214 things remembered",
        "an empty graph named a day it does not have"
    );
}
