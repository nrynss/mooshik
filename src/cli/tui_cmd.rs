//! `mooshik tui` — M11's terminal interface, M12a's data.
//!
//! Two ways in, and the difference matters:
//!
//! * `--demo` draws the design's own Thursday. It touches nothing — no config,
//!   no database, no credentials — so the artboards can be seen on a machine
//!   that has never run `mooshik init`. It takes an optional scene: `--demo
//!   recall` adds `1c`'s quoted words and `--demo caution` adds `1d`'s one
//!   careful sentence, because both artboards are states of the conversation and
//!   nothing else can reach them until the chat loop lands.
//! * Without it, the workspace is the graph — [`crate::memory::view`] reads
//!   the open session into the same view model the artboards are drawn from,
//!   and M12b's tick reads it again: the loop rebuilds on every 250 ms tick,
//!   so a write from the ingester, an MCP client or the reflect pass appears
//!   in the pane without a keystroke.
//!
//! Both paths hand a [`Workspace`](crate::tui::model::Workspace) to
//! `crate::tui::run`; the live path also hands it the rebuild — a closure
//! that answers with the graph as of now — and the turn drive that spawns
//! `Session::turn`. `--demo` passes `None` for both. The screens stay a pure
//! function of the model.
//!
//! **The live path is an ordinary holder of the single-writer lease, exactly as
//! `chat` is.** It takes it for the length of the session and gives it back on
//! the way out, so a `mooshik tui` left open in a tmux split is a writer another
//! process can see and be refused by — with Mooshik's own conflict sentence,
//! which names the holder and no override or page this product does not ship.
//! That refusal is a user error and leaves with exit code 2. There is no
//! read-only or proxied second view; a workspace served through somebody
//! else's lease is a different design and not this one.
//!
//! The session is closed **after** the terminal is put back, on every way out of
//! the loop. Closing is what makes the write-behind tail durable, so `Esc`, a
//! failed draw, a broken pipe and — since M12a's first review round —
//! `SIGTERM`/`SIGHUP` all reach it; the last of those used to kill the process
//! where it stood, which left the alternate screen up and the lease held for its
//! whole TTL by a pid that no longer existed. `crate::tui::run` takes both
//! signals for the length of the session and ends the loop instead. Close's own
//! failure is reported rather than swallowed, because a session that would not
//! close is the one thing here that can lose what was remembered.
//!
//! **A panic is the exception, deliberately.** The panic hook puts the terminal
//! back, the unwind drops the `Memory`, and Lambo's `Drop` aborts the heartbeat
//! without releasing the lease — a handle dropped without a clean close is the
//! crash-shaped path, and the lease is what makes a crash safe. So a panic
//! leaves the lease to lapse on its own TTL, which is the correct answer for a
//! crash and the wrong one for a closed window; only the second is a signal.
//!
//! **What the live path owns is [`Pane`].** The runtime, the one `Memory` and
//! its lease, and the lane that keeps two writers on that handle off each
//! other. It stays private to this module on purpose: everything the pane
//! grows — M12d's watcher, M12e's turn — is *driven from* [`live`], which
//! hands the pieces into the closures `crate::tui::run` takes. A `Pane`
//! reachable from elsewhere in the crate would be a second way to name the
//! lease, which is the thing this module exists to have exactly one of.

use std::sync::{mpsc, Arc};

use lambo::Memory;

use crate::{
    companion::{
        compose_session, Cancellation, CompanionClient, CompanionError, RecallInjector, Session,
        ToolExecutor,
    },
    config::Config,
    home::HomeLayout,
    memory::{MemoryError, WriteLane},
    text,
    tools::{ChatStack, Confirm, Diagnostics},
    tui::{
        app::{stamp, TurnOutcome},
        model::{Speaker, Turn},
        Scene, TurnDrive,
    },
    vault::{SharedVault, Vault},
};

use super::{resolve, runtime};

/// What the live pane owns for its whole life, and hands to whatever the pane
/// grows next.
///
/// M11 needed none of this: the pane opened a graph, drew it, and closed. M12b
/// added a rebuild, still a plain closure. M12d and M12e are the two that stop
/// being closures — a watcher deriving filesystem and git changes as they
/// happen, and a conversation whose turn is spawned rather than blocked on —
/// and both of them need the same three things at once. That is what makes
/// this a type rather than three locals.
///
/// **One runtime.** `run_chat` builds its own and `block_on`s the whole
/// session; the pane cannot, because the 250 ms tick is the one live behaviour
/// M12b exists for and a turn that blocks the loop takes it with it. So the
/// pane builds a runtime once, at open, and spawns onto it.
///
/// **One `Memory`, and one lease.** The module header above is explicit that
/// the live path is an ordinary lease holder, and that serving a workspace
/// through somebody else's lease is a different design. `tools::executor_for_chat`
/// opens a `Memory` of its own; if the pane called it the process would hold
/// the lease twice and refuse itself. [`Pane::tools`] is the way around that,
/// and it is a *method* precisely so the handle cannot come from anywhere else.
///
/// **One write lane.** Lambo does not serialize two writers on one handle — see
/// [`WriteLane`] for what it does instead and why the pane pays for the lane
/// anyway.
///
/// **Field order is drop order, and drop order is the panic contract.**
/// `memory` is declared before `runtime` so it is dropped first, which is what
/// lets lambo's background tasks be shut down on a runtime that is still there
/// to shut them down on — the property the two locals this replaced carried by
/// being declared in the opposite order. And nothing here implements [`Drop`]:
/// a panic must leave the lease to lapse on its TTL, so the *only* clean close
/// is the explicit [`Pane::close`] on the ordinary way out. A `Drop` that
/// closed would be the crash path quietly doing the closed-window thing.
struct Pane {
    memory: Arc<Memory>,
    writes: WriteLane,
    runtime: tokio::runtime::Runtime,
}

impl Pane {
    /// Open the graph and take the lease for the length of the pane.
    fn open(config: &Config) -> anyhow::Result<Self> {
        let runtime = runtime()?;
        let memory = runtime
            .block_on(crate::memory::open(config))
            .map_err(anyhow::Error::new)?;
        Ok(Self {
            memory: Arc::new(memory),
            writes: WriteLane::new(),
            runtime,
        })
    }

    /// The one open handle. `&Arc` rather than `&Memory` because a consumer
    /// that spawns work has to clone it into the task; a consumer that only
    /// reads (the rebuild seam) derefs and never notices.
    fn memory(&self) -> &Arc<Memory> {
        &self.memory
    }

    /// The lane every writer through [`Pane::memory`] takes. M12d's watcher
    /// holds it around its derive; [`Pane::tools`] hands the same one to the
    /// tool stack, so the two cannot be planning against each other's epoch.
    #[cfg_attr(not(test), allow(dead_code))]
    fn writes(&self) -> &WriteLane {
        &self.writes
    }

    /// Where the pane's work runs.
    ///
    /// A [`tokio::runtime::Handle`], not `&Runtime`, and the difference is the
    /// point: a `Handle` clone does not keep the runtime alive, so the pane
    /// dropping its `Runtime` shuts every spawned task down whether or not
    /// anything still holds a spawner. **That is the panic contract for
    /// spawned work** — a turn task holding `Memory` must not outlive the pane,
    /// or it keeps writing after the terminal has been put back.
    ///
    /// Rejected: handing out `&Runtime`. It is not more capable (both can
    /// `block_on`), it borrows the pane for as long as the loop holds it, and
    /// it invites `block_on` on the tick's own thread — the one thing the turn
    /// path must never do.
    fn spawner(&self) -> tokio::runtime::Handle {
        self.runtime.handle().clone()
    }

    /// The chat tool stack, built over the handle the pane already holds.
    ///
    /// This is `tools::executor_for_chat` with the open taken out: the same
    /// gate, the same egress redactor, the same MCP layer, over this `Memory`
    /// and this lane. Its notices come back as values because under the
    /// alternate screen there is nowhere to print them.
    ///
    /// `confirm` is required rather than defaulted for the reason the M12e
    /// notes give: the gate's own prompt reads stdin, and a gate reading stdin
    /// while ratatui owns the terminal hangs the pane with no way out. The
    /// choice — deny the prompt class outright, or make approval a turn in the
    /// conversation — is the caller's, and there is no default that is safe to
    /// make silently.
    fn tools(
        &self,
        config: &Config,
        vault: Option<SharedVault>,
        confirm: Confirm,
        diagnostics: Diagnostics,
    ) -> ChatStack {
        crate::tools::executor_over_memory(
            config,
            vault,
            Arc::clone(&self.memory),
            self.writes.clone(),
            confirm,
            diagnostics,
        )
    }

    /// Close the session, on the runtime that opened it.
    ///
    /// Consumes the pane, so the runtime is dropped immediately afterwards and
    /// anything still spawned on it goes with it. A task that outlives the
    /// close by the width of that drop is not a correctness problem — lambo
    /// refuses a write on a closed handle deterministically — but it is why
    /// this takes `self` rather than `&self`.
    fn close(self) -> Result<(), MemoryError> {
        self.runtime
            .block_on(self.memory.close())
            .map_err(MemoryError::from)
    }
}

pub(crate) fn tui(layout: &HomeLayout, args: &clap::ArgMatches) -> anyhow::Result<()> {
    match args.get_one::<String>("demo").map(String::as_str) {
        // `--demo` opens no database and rebuilds nothing: its workspace is
        // the design's own Thursday, fixed for the life of the loop.
        Some(scene) => draw(crate::tui::demo(Scene::named(Some(scene))), None, None),
        None => live(layout),
    }
}

/// The live session: open the graph, draw it — rebuilding the view on every
/// tick — and put both back.
fn live(layout: &HomeLayout) -> anyhow::Result<()> {
    // Same resolution as `mooshik chat`: open the vault once and keep it for
    // the tool stack (egress redaction, MCP env). `load_with_secrets` would
    // drop the handle before tools could use it, and a second open is refused
    // by the exclusive lock.
    let root = layout.open_existing_root().map_err(anyhow::Error::new)?;
    let mut config = Config::load_at(&root).map_err(anyhow::Error::new)?;
    let vault = resolve::open_vault(layout, &config, &root).ok();
    match &vault {
        Some(vault) => resolve::resolve_secrets(&mut config, vault).map_err(anyhow::Error::new)?,
        None if resolve::needs_vault(&config) => {
            return Err(anyhow::Error::new(
                crate::config::ConfigError::VaultUnavailable,
            ));
        }
        None => {}
    }

    // The runtime, the handle and the lease, for the length of the pane. What
    // used to be two locals whose declaration order carried the drop order is
    // now [`Pane`], which carries it in its field order and says why.
    let pane = Pane::open(&config)?;

    let drawn = converse(&pane, &config, vault.map(Vault::shared));

    // Both outcomes, in the order they happened: a session that failed to close
    // may have lost the tail of what it remembered, which is worse than a draw
    // that failed, so it is reported when the drawing succeeded.
    let closed = pane.close();
    drawn?;
    closed?;
    Ok(())
}

/// Wire the companion onto the open pane and run the loop.
///
/// Isolated so `live` can still close the pane if composition fails: the
/// lease must not be left held for its TTL by a process that never drew.
fn converse(pane: &Pane, config: &Config, vault: Option<SharedVault>) -> anyhow::Result<()> {
    let (events_tx, events_rx) = mpsc::channel();
    let diagnostics = Diagnostics::sink({
        let events_tx = events_tx.clone();
        move |message: &str| {
            let _ = events_tx.send(TurnEvent::Notice(message.to_owned()));
        }
    });
    // Prompt-class tools are denied here: a Confirm that reads stdin hangs
    // the pane, and making approval a `Turn::Cautioned` (artboard 1d) is a
    // bigger shape than this milestone's contract. The caller still supplies
    // the callback — the gate never falls through to stdin.
    let confirm: Confirm = Box::new(|_| false);
    let ChatStack { tools, notices } = pane.tools(config, vault, confirm, diagnostics);
    let recall = crate::tools::recall_for_chat(config, Arc::clone(&tools));
    let client = CompanionClient::from_config(&config.companion).map_err(anyhow::Error::new)?;
    let session = compose_session(client, config.companion.context_window, tools, recall);
    let mut drive = PaneTurn::new(pane.spawner(), session, events_tx, events_rx);

    let mut workspace = crate::memory::view::of_memory(pane.memory(), chrono::Local::now());
    for notice in notices {
        workspace.conversation.turns.push(Turn::Said {
            time: stamp(),
            speaker: Speaker::Mooshik,
            text: notice,
        });
    }
    let mut refresh = || crate::memory::view::of_memory(pane.memory(), chrono::Local::now());
    draw(workspace, Some(&mut refresh), Some(&mut drive))
}

/// Take the terminal, run the loop, and give the terminal back.
///
/// `refresh` is the live path's rebuild seam; `--demo` passes `None` and its
/// fixed workspace is never rebuilt. `turn` is the live path's companion seam
/// and `--demo` passes `None` for that too.
fn draw(
    workspace: crate::tui::model::Workspace,
    refresh: Option<&mut dyn FnMut() -> crate::tui::model::Workspace>,
    turn: Option<&mut dyn TurnDrive>,
) -> anyhow::Result<()> {
    // The context, not the `io::Error`: taking a terminal that is not there
    // fails with "Device not configured", which says nothing about what to do.
    // `Failure::rendered` prints the top-level `Display` only, so this sentence
    // is what the operator sees.
    //
    // Two calls, two sentences. It used to be one, and "this process has no
    // terminal" was then attached to every error the whole session could return —
    // including a `terminal.draw`, `event::poll` or `event::read` that failed an
    // hour in, on a terminal that plainly existed. A diagnosis has to be about
    // the failure it is printed under, so the handshake and the loop are separate
    // calls with separate contexts.
    let terminal = crate::tui::start()
        .map_err(|error| anyhow::Error::new(error).context(text::get("tui.needs_a_terminal")))?;
    crate::tui::run(terminal, workspace, refresh, turn)
        .map_err(|error| anyhow::Error::new(error).context(text::get("tui.session_failed")))
}

/// One message from a spawned turn, or from execute-time diagnostics.
enum TurnEvent {
    Token(String),
    Finished(Result<String, CompanionError>),
    Notice(String),
}

type ChatSession = Session<Arc<dyn ToolExecutor>, Arc<dyn RecallInjector>>;

/// The live path's [`TurnDrive`]: spawns `Session::turn` on the pane runtime
/// and drains tokens into the app. Owned by `converse`, borrowed by the loop.
struct PaneTurn {
    spawner: tokio::runtime::Handle,
    session: Arc<tokio::sync::Mutex<ChatSession>>,
    events_tx: mpsc::Sender<TurnEvent>,
    events_rx: mpsc::Receiver<TurnEvent>,
    cancel: Option<Cancellation>,
}

impl PaneTurn {
    fn new(
        spawner: tokio::runtime::Handle,
        session: ChatSession,
        events_tx: mpsc::Sender<TurnEvent>,
        events_rx: mpsc::Receiver<TurnEvent>,
    ) -> Self {
        Self {
            spawner,
            session: Arc::new(tokio::sync::Mutex::new(session)),
            events_tx,
            events_rx,
            cancel: None,
        }
    }
}

impl TurnDrive for PaneTurn {
    fn start(&mut self, user_text: &str) {
        let cancel = Cancellation::new();
        self.cancel = Some(cancel.clone());
        let session = Arc::clone(&self.session);
        let events_tx = self.events_tx.clone();
        let user_text = user_text.to_owned();
        self.spawner.spawn(async move {
            let mut session = session.lock().await;
            let result = session
                .turn(&user_text, &cancel, |token| {
                    let _ = events_tx.send(TurnEvent::Token(token.to_owned()));
                })
                .await;
            let _ = events_tx.send(TurnEvent::Finished(result));
        });
    }

    fn cancel(&mut self) {
        if let Some(cancel) = &self.cancel {
            cancel.cancel();
        }
    }

    fn drain(&mut self, app: &mut crate::tui::app::App) {
        while let Ok(event) = self.events_rx.try_recv() {
            match event {
                TurnEvent::Token(token) => app.append_token(&token),
                TurnEvent::Finished(result) => {
                    self.cancel = None;
                    app.finish_turn(match result {
                        Ok(reply) => TurnOutcome::Completed(reply),
                        Err(CompanionError::Cancelled) => TurnOutcome::Cancelled,
                        Err(error) => TurnOutcome::Failed(error.to_string()),
                    });
                }
                TurnEvent::Notice(message) => app.note(&message),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use std::time::Duration;

    use lambo::{graph::derive::ParentOf, ConceptType, EmbedderKind, StoreKind};
    use serde_json::json;

    use super::*;

    /// A pane over an in-process fixture graph: no network, no database, no
    /// model. The session id is per-test because lambo's active-session
    /// registry is process-wide and `cargo test` runs these in parallel.
    fn fixture_pane(session: &str) -> Pane {
        Pane::open(&fixture_config(session)).unwrap()
    }

    fn fixture_config(session: &str) -> Config {
        let mut config = Config::default();
        config.store.kind = StoreKind::Memory;
        config.embedder.kind = EmbedderKind::Fixture;
        config.embedder.dim = 1024;
        config.session.id = format!("mooshik-pane-{session}");
        config
    }

    /// The production half of this module, for the source pins below.
    fn production() -> &'static str {
        include_str!("tui_cmd.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap()
    }

    #[test]
    fn demo_opens_no_database_and_never_reaches_the_pane() {
        // M11's safety valve: `--demo` touches no config, no database and no
        // credentials, so the artboards can be seen on a machine that has
        // never run `mooshik init`. The seam must not have quietly moved the
        // open above the branch.
        let dispatch = production()
            .split("pub(crate) fn tui(")
            .nth(1)
            .expect("the dispatch must exist")
            .split("\nfn live")
            .next()
            .unwrap();
        assert!(
            dispatch.contains("Some(scene) => draw(crate::tui::demo("),
            "the demo arm must draw a fixed workspace directly: {dispatch}"
        );
        for opener in ["Pane::open", "memory::open", "load_with_secrets"] {
            assert!(
                !dispatch.contains(opener),
                "the demo arm must not reach {opener}: {dispatch}"
            );
        }
    }

    #[test]
    fn the_pane_is_the_only_thing_in_the_process_that_opens_memory() {
        // The lease is claimed once. `executor_for_chat` opens a `Memory` of
        // its own and takes the lease with it; a pane that called it would be
        // refused by its own session. `Pane::tools` is the way the stack gets
        // built, and it goes through the over-an-open-handle factory.
        let source = production();
        for forbidden in ["executor_for_chat(", "MemoryTools::for_chat("] {
            assert!(
                !source.contains(forbidden),
                "the pane must not call the opening factory ({forbidden})"
            );
        }
        assert_eq!(
            source.matches("crate::memory::open(").count(),
            1,
            "exactly one open, and it is `Pane::open`'s"
        );
        assert!(
            source.contains("crate::tools::executor_over_memory("),
            "the tool stack must be built over the handle already open"
        );
    }

    #[test]
    fn a_panic_leaves_the_lease_to_lapse() {
        // The module header's contract: the panic hook puts the terminal back,
        // the unwind drops the `Memory`, and lambo's `Drop` aborts the
        // heartbeat WITHOUT releasing the lease — expiry is the right release
        // for a crash. A `Drop` impl on the pane would turn every panic into a
        // clean close and lose that distinction, so there must not be one.
        let source = production();
        assert!(
            !source.contains("impl Drop for Pane"),
            "a Drop impl would close on the crash path too"
        );
        assert_eq!(
            source.matches("self.memory.close()").count(),
            1,
            "the one clean close is `Pane::close`'s, on the ordinary way out"
        );
        // Field order is drop order: memory before runtime, so lambo's
        // background tasks are shut down on a runtime that still exists.
        let fields = source
            .split("struct Pane {")
            .nth(1)
            .expect("Pane must exist")
            .split('}')
            .next()
            .unwrap();
        let memory = fields.find("memory:").expect("a memory field");
        let runtime = fields.find("runtime:").expect("a runtime field");
        assert!(
            memory < runtime,
            "memory must be declared before runtime so it drops first: {fields}"
        );
    }

    #[test]
    fn work_spawned_on_the_pane_cannot_outlive_it() {
        // M12e's panic contract for the spawned half: a turn task holding
        // `Memory` must not keep writing after the terminal is restored. The
        // pane owns the runtime, so dropping the pane shuts the task down —
        // the task's own references are gone by the time the drop returns.
        let pane = fixture_pane("outlive");
        let held = Arc::new(AtomicUsize::new(0));
        let task = Arc::clone(&held);
        let memory = Arc::clone(pane.memory());
        pane.spawner().spawn(async move {
            loop {
                task.fetch_add(1, Ordering::SeqCst);
                let _ = &memory;
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        });
        while held.load(Ordering::SeqCst) == 0 {
            std::thread::sleep(Duration::from_millis(1));
        }
        let memory = Arc::clone(pane.memory());
        assert!(
            Arc::strong_count(&memory) >= 3,
            "the task must be holding a handle while the pane lives"
        );
        drop(pane);
        // Dropping the runtime is what cancels the task; once the drop has
        // returned the task is gone and so is its clone of the handle.
        assert_eq!(
            Arc::strong_count(&memory),
            1,
            "the spawned task must not survive the pane"
        );
        let before = held.load(Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(
            held.load(Ordering::SeqCst),
            before,
            "the task must have stopped running"
        );
    }

    #[test]
    fn the_write_lane_keeps_two_writers_off_each_other() {
        // Lambo's writers gate is a READ permit — N writers hold it at once —
        // and concurrency between them is resolved by replanning the whole
        // gather, embedder call included, with a finite budget. M12d's watcher
        // and M12e's tool calls are two writers on one handle, so the pane
        // carries the lane lambo deliberately does not.
        let pane = fixture_pane("lane");
        let inside = Arc::new(AtomicUsize::new(0));
        let overlapped = Arc::new(AtomicUsize::new(0));
        let mut running = Vec::new();
        for _ in 0..4 {
            let lane = pane.writes().clone();
            let inside = Arc::clone(&inside);
            let overlapped = Arc::clone(&overlapped);
            running.push(pane.spawner().spawn(async move {
                let _held = lane.enter().await;
                if inside.fetch_add(1, Ordering::SeqCst) != 0 {
                    overlapped.fetch_add(1, Ordering::SeqCst);
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
                inside.fetch_sub(1, Ordering::SeqCst);
            }));
        }
        let handle = pane.spawner();
        for task in running {
            handle.block_on(task).unwrap();
        }
        assert_eq!(
            overlapped.load(Ordering::SeqCst),
            0,
            "two writers were inside the lane at once"
        );
        pane.close().unwrap();
    }

    #[test]
    fn the_tool_stack_writes_through_the_handle_the_pane_already_holds() {
        // The whole point of the seam: a derive issued by the tool stack lands
        // in the pane's own graph, which is the graph the tick reads. A second
        // `Memory` would be a second lease and a graph the pane never sees.
        let pane = fixture_pane("stack");
        let stack = pane.tools(
            &fixture_config("stack"),
            None,
            Box::new(|_| panic!("the gate must not prompt on this path")),
            Diagnostics::stderr(),
        );
        let answer = stack.tools.execute(
            crate::tools::TOOL_DERIVE,
            &json!({
                "agent_id": "pane-seam-test",
                "concepts": [{ "content": "pane seam marker", "concept_type": "entity" }],
            }),
        );
        assert!(answer.contains("created"), "{answer}");
        let found = pane
            .memory()
            .graph()
            .read()
            .concepts()
            .any(|concept| concept.content == "pane seam marker");
        assert!(found, "the derive must be visible in the pane's own graph");
        drop(stack);
        pane.close().unwrap();
    }

    #[test]
    fn the_tool_stack_returns_its_notices_instead_of_printing_them() {
        // Under the alternate screen any write to stdout or stderr corrupts the
        // frame, so the vault notice `mooshik chat` prints has to come back as
        // a value the pane can render.
        let pane = fixture_pane("notices");
        let stack = pane.tools(
            &fixture_config("notices"),
            None,
            Box::new(|_| false),
            Diagnostics::stderr(),
        );
        assert_eq!(
            stack.notices,
            vec![crate::text::get("tools.vault_unavailable").to_owned()],
            "an unopenable vault must come back as a notice, not a print"
        );
        drop(stack);
        pane.close().unwrap();
    }

    #[test]
    fn close_actually_closes_the_session() {
        // `close` is what makes the write-behind tail durable, and the pane's
        // is ordered after the terminal is put back. Proof that it ran: the
        // handle refuses a write afterwards.
        let pane = fixture_pane("close");
        let memory = Arc::clone(pane.memory());
        pane.close().unwrap();
        let refused = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                memory
                    .derive(&[("after close", ConceptType::Entity)], &ParentOf::none())
                    .await
            });
        assert!(refused.is_err(), "a closed handle must refuse a write");
    }

    #[test]
    fn the_live_path_wires_send_to_session_turn() {
        // An implementation that only fills App::apply without spawning is
        // the pre-M12e hole with a prettier model. The production half must
        // compose through compose_session and spawn Session::turn on the pane
        // runtime — never block_on the turn on the event-loop thread.
        let source = production();
        let converse = source
            .split("fn converse(")
            .nth(1)
            .expect("converse must exist")
            .split("fn draw(")
            .next()
            .unwrap();
        assert!(
            converse.contains("compose_session("),
            "the live path must compose through compose_session: {converse}"
        );
        assert!(
            converse.contains("pane.tools("),
            "the live path must build tools over the pane's handle: {converse}"
        );
        assert!(
            converse.contains("pane.spawner()"),
            "the turn must be spawned on the pane runtime: {converse}"
        );
        assert!(
            !converse.contains("std::io::stdin") && !converse.contains("interactive_confirm"),
            "the pane path must not read stdin for confirm: {converse}"
        );
        let drive = source
            .split("impl TurnDrive for PaneTurn")
            .nth(1)
            .expect("PaneTurn must implement TurnDrive")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        assert!(
            drive.contains("self.spawner.spawn"),
            "start must spawn on the pane handle: {drive}"
        );
        assert!(
            drive.contains(".turn(") && drive.contains("&cancel"),
            "the spawned work must be Session::turn with Cancellation: {drive}"
        );
        assert!(
            !drive.contains("block_on"),
            "the turn must not block_on the event-loop thread: {drive}"
        );
    }

    #[test]
    fn the_pane_turn_path_does_not_print() {
        // Under the alternate screen a print corrupts the frame. Assembly
        // notices are values; execute-time diagnostics go through the sink.
        let source = production();
        for chunk in [
            source
                .split("fn live(")
                .nth(1)
                .unwrap()
                .split("fn draw(")
                .next()
                .unwrap(),
            source
                .split("impl TurnDrive for PaneTurn")
                .nth(1)
                .unwrap()
                .split("#[cfg(test)]")
                .next()
                .unwrap(),
        ] {
            for forbidden in ["eprintln!", "print!", "eprint!"] {
                assert!(
                    !chunk.contains(forbidden),
                    "the pane turn path must not {forbidden}: {chunk}"
                );
            }
        }
    }

    #[tokio::test]
    async fn a_failed_session_turn_becomes_a_turn() {
        // Drive a real Session::turn against MockServer (no Vertex, no
        // Cloud SQL) and land the classified error on the conversation.
        use crate::companion::mock::{MockServer, Script};
        use crate::companion::{NoopExecutor, NoopRecall};
        use crate::tui::app::{Action, App};

        let server = MockServer::spawn(vec![Script::error(404, r#"{"error":"missing"}"#)]).await;
        let companion = crate::config::CompanionConfig {
            base_url: server.base_url.clone(),
            ..crate::config::CompanionConfig::default()
        };
        let client = CompanionClient::from_config(&companion).unwrap();
        let mut session = compose_session(
            client,
            32768,
            Arc::new(NoopExecutor) as Arc<dyn ToolExecutor>,
            Arc::new(NoopRecall) as Arc<dyn RecallInjector>,
        );
        let mut app = App::new(crate::tui::model::Workspace::default());
        for character in "hello".chars() {
            app.apply(Action::Type(character));
        }
        app.apply(Action::Send);
        let error = session
            .turn("hello", &Cancellation::new(), |token| {
                app.append_token(token);
            })
            .await
            .unwrap_err();
        assert!(matches!(error, CompanionError::HttpStatus));
        app.finish_turn(TurnOutcome::Failed(error.to_string()));
        match app.workspace.conversation.turns.last() {
            Some(Turn::Said {
                speaker: Speaker::Mooshik,
                text,
                ..
            }) => assert_eq!(text, crate::text::get("companion.http_status")),
            other => panic!("the failure must be a turn, not {other:?}"),
        }
        assert!(!app.turn_in_flight());
        assert!(app.running);
    }
}
