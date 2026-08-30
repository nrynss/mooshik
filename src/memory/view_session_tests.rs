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
        .split("pub fn of_graph")
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
