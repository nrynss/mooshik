use super::*;
use std::{process::Command, sync::atomic::AtomicUsize};

struct CancelAfterFirstLoad {
    loads: AtomicUsize,
    git: AtomicBool,
}

impl CancelAfterFirstLoad {
    fn new() -> Self {
        Self {
            loads: AtomicUsize::new(0),
            git: AtomicBool::new(false),
        }
    }
}

impl CollectCancel for CancelAfterFirstLoad {
    fn load(&self, _order: Ordering) -> bool {
        let cancelled = self.loads.fetch_add(1, Ordering::AcqRel) >= 1;
        self.git.store(cancelled, Ordering::Release);
        cancelled
    }

    fn git_cancel_flag(&self) -> &AtomicBool {
        &self.git
    }
}

fn flatten_readme_words(readme: &str) -> Vec<String> {
    let lowered: String = readme
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c.is_whitespace() {
                c.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect();
    lowered.split_whitespace().map(str::to_string).collect()
}

fn readme_claims_available_without_watcher(readme: &str) -> bool {
    let words = flatten_readme_words(readme);
    let has_without_article_watcher = words
        .windows(3)
        .any(|w| w[0] == "without" && (w[1] == "the" || w[1] == "a") && w[2] == "watcher");
    if has_without_article_watcher {
        return true;
    }
    // Punctuation is already spaces, so "available, without our watcher"
    // is the word order available … without … watcher regardless of
    // determiner.
    words.iter().enumerate().any(|(i, word)| {
        *word == "available"
            && words[i + 1..].iter().enumerate().any(|(j, w)| {
                *w == "without" && words[i + 1 + j + 1..].iter().any(|rest| rest == "watcher")
            })
    })
}

#[test]
fn discovery_uses_allowlist_and_skips_generated_dirs_and_symlinks() {
    let root = test_root("discovery");
    fs::create_dir_all(root.join("target")).unwrap();
    fs::create_dir_all(root.join("nested")).unwrap();
    fs::write(root.join("ok.MD"), "safe").unwrap();
    fs::write(root.join("no.json"), "ignored").unwrap();
    fs::write(root.join("target/hidden.md"), "ignored").unwrap();
    fs::write(root.join("nested/note.txt"), "safe").unwrap();
    let snapshot = Snapshot::discover(&root);
    assert!(snapshot.files.contains_key(&root.join("ok.MD")));
    assert!(snapshot.files.contains_key(&root.join("nested/note.txt")));
    assert!(!snapshot.files.contains_key(&root.join("no.json")));
    assert!(!snapshot.files.contains_key(&root.join("target/hidden.md")));
    remove_test_root(&root);
}

#[cfg(unix)]
#[test]
fn discovery_does_not_follow_file_symlinks() {
    let root = test_root("symlink");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("outside.md"), "safe").unwrap();
    std::os::unix::fs::symlink(root.join("outside.md"), root.join("link.md")).unwrap();
    assert!(!Snapshot::discover(&root)
        .files
        .contains_key(&root.join("link.md")));
    remove_test_root(&root);
}

#[test]
fn debounce_coalesces_repeated_path_keys() {
    let root = test_root("debounce");
    fs::create_dir_all(&root).unwrap();
    let path = root.join("note.md");
    fs::write(&path, "one").unwrap();
    let before = Snapshot::discover(&root);
    fs::write(&path, "two").unwrap();
    let after = Snapshot::discover(&root);
    let mut pending = BTreeMap::new();
    let mut changed = None;
    collect_changes(&before, &after, &mut pending, &mut changed);
    fs::write(&path, "three").unwrap();
    let latest = Snapshot::discover(&root);
    collect_changes(&after, &latest, &mut pending, &mut changed);
    assert_eq!(pending.len(), 1);
    remove_test_root(&root);
}

#[test]
fn file_payload_is_metadata_only_and_keeps_the_save_mtime() {
    let root = test_root("file-payload");
    let path = root.join("notes.md");
    fs::write(&path, "a useful note").unwrap();
    let before = Snapshot::discover(&root);
    fs::write(&path, "a newer useful note").unwrap();
    let after = Snapshot::discover(&root);
    let mut pending = BTreeMap::new();
    let mut changed = None;
    collect_changes(&before, &after, &mut pending, &mut changed);
    let event = pending.values().next().unwrap();
    let (content, event_time) = event_to_concept(event, &None, &root).unwrap().unwrap();
    assert_eq!(content, "workspace file changed: notes.md");
    assert_eq!(
        event_time,
        system_time_to_utc(after.files[&path].modified).unwrap()
    );
    assert!(!content.contains("newer useful note"));
    remove_test_root(&root);
}

#[test]
fn secret_policy_drops_content_without_exposing_the_match() {
    assert!(find_secret("AWS=AKIA1234567890ABCDEF", &None));
    assert!(find_secret("TOKEN = 'abcdefghijklmnopqrstuvwxyz'", &None));
    assert!(find_secret("TOKEN=abcdefghijklmnopqrst!", &None));
    assert!(find_secret(
        "API_KEY: abcdefghijklmnopqrst # comment",
        &None
    ));
    assert!(find_secret("-----BEGIN PRIVATE KEY-----", &None));
    assert!(!find_secret("ordinary project notes", &None));
}

#[test]
fn cancelled_file_walk_keeps_unknown_for_a_late_failed_repository() {
    let repo = PathBuf::from("/workspace/late");
    let path = PathBuf::from("/workspace/note.md");
    let modified = SystemTime::UNIX_EPOCH + Duration::from_secs(1);
    let previous = Snapshot::default();
    let current = Snapshot {
        files: [(path, FileState { modified, len: 1 })]
            .into_iter()
            .collect(),
        heads: [(repo.clone(), GitHead::Unknown)].into_iter().collect(),
        git_failures: vec![repo.clone()],
    };
    let mut pending = BTreeMap::new();
    let mut changed = None;
    let cancelled = CancelAfterFirstLoad::new();
    let result =
        collect_changes_with_cancel(&previous, &current, &mut pending, &mut changed, &cancelled);
    assert_eq!(
        cancelled.loads.load(Ordering::Acquire),
        2,
        "cancel must be false at the first load and flip during the file walk"
    );
    assert!(result.retry);
    assert!(!result.changed);
    assert_eq!(
        result.snapshot.heads.get(&repo),
        Some(&GitHead::Unknown),
        "file-walk cancel must not drop a failed discovery that already ran"
    );
}

#[test]
fn cancelled_atomicbool_with_files_keeps_unknown_without_walking() {
    let repo = PathBuf::from("/workspace/late");
    let path = PathBuf::from("/workspace/note.md");
    let modified = SystemTime::UNIX_EPOCH + Duration::from_secs(1);
    let previous = Snapshot::default();
    let current = Snapshot {
        files: [(path, FileState { modified, len: 1 })]
            .into_iter()
            .collect(),
        heads: [(repo.clone(), GitHead::Unknown)].into_iter().collect(),
        git_failures: vec![repo.clone()],
    };
    let mut pending = BTreeMap::new();
    let mut changed = None;
    let result = collect_changes_with_cancel(
        &previous,
        &current,
        &mut pending,
        &mut changed,
        &AtomicBool::new(true),
    );
    assert!(result.retry);
    assert!(
        !result.changed,
        "a no-op AtomicBool load would walk the new file and look retained"
    );
    assert!(
        pending.is_empty(),
        "production AtomicBool cancel must not enqueue the file walk"
    );
    assert!(
        result.snapshot.files.is_empty(),
        "cancel must keep previous file state, not apply a walk that ignored the flag"
    );
    assert_eq!(
        result.snapshot.heads.get(&repo),
        Some(&GitHead::Unknown),
        "production AtomicBool cancel must not drop a failed discovery that already ran"
    );
}

#[test]
fn atomicbool_collect_cancel_forwards_load_and_git_flag() {
    let impl_src = include_str!("mod.rs")
        .split("impl CollectCancel for AtomicBool {")
        .nth(1)
        .expect("CollectCancel for AtomicBool")
        .split("fn collect_changes_with_cancel")
        .next()
        .unwrap();
    let load = impl_src
        .split("fn load(")
        .nth(1)
        .expect("AtomicBool load")
        .split("fn git_cancel_flag")
        .next()
        .unwrap();
    let load_flat = load.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        load_flat.contains("AtomicBool::load(self, order) }"),
        "CollectCancel::load for AtomicBool must forward to AtomicBool::load"
    );
    assert!(
        !load_flat.contains("false"),
        "CollectCancel::load for AtomicBool must not discard the real load"
    );
    let git = impl_src
        .split("fn git_cancel_flag")
        .nth(1)
        .expect("git_cancel_flag");
    let git_flat = git.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        git_flat.contains("-> &AtomicBool { self }"),
        "git_cancel_flag must return self so git subprocesses see the production flag"
    );
    assert!(
        !git_flat.contains("AtomicBool::new"),
        "git_cancel_flag must not substitute a static never-set flag"
    );
}

#[test]
fn live_watching_fails_closed_at_tui_startup() {
    let start = include_str!("mod.rs")
        .split("impl Watcher {")
        .nth(1)
        .expect("Watcher impl")
        .split("pub(crate) async fn stop")
        .next()
        .unwrap();
    assert!(
        start.contains("#[cfg(not(unix))]"),
        "non-Unix must refuse Watcher::start"
    );
    assert!(
        start.contains("WatchError::WorkspaceUnavailable"),
        "refusal is WorkspaceUnavailable so live() cannot open the pane without a watcher"
    );
    let live = include_str!("../tui_cmd.rs")
        .split("let watcher = match watcher::Watcher::start(")
        .nth(1)
        .expect("live starts the watcher")
        .split("let workspace =")
        .next()
        .unwrap();
    assert!(
        live.contains("pane.close()"),
        "Watcher::start failure must close the pane"
    );
    assert!(
        live.contains("return Err(anyhow::Error::new(error))"),
        "Watcher::start failure must not continue into draw"
    );
    let readme = include_str!("../../../README.md");
    let readme_flat = readme.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        readme_flat.contains("fails closed at TUI startup"),
        "README must say live watching fails closed at TUI startup"
    );
    assert!(
        readme.contains("The watcher stops with the pane"),
        "README must say the watcher stops with the pane"
    );
    assert!(
        !readme_claims_available_without_watcher(readme),
        "README must not claim the pane runs without the watcher"
    );
}

#[test]
fn readme_reject_sees_available_without_watcher_through_punct_and_determiners() {
    for claim in [
        "The pane remains available, without our watcher",
        "The pane remains available, without this watcher",
        "Live watching is available, without any watcher",
    ] {
        assert!(
            readme_claims_available_without_watcher(claim),
            "comma plus a determiner other than the/a is still an availability claim: {claim}"
        );
    }
    assert!(!readme_claims_available_without_watcher(include_str!(
        "../../../README.md"
    )));
}

#[test]
fn bounded_read_rejects_a_file_that_grows_past_the_limit() {
    let root = test_root("read-limit");
    let path = root.join("large.md");
    let file = fs::File::create(&path).unwrap();
    file.set_len(MAX_FILE_BYTES + 1).unwrap();
    assert!(read_nofollow(&path, &root).is_err());
    remove_test_root(&root);
}

#[cfg(unix)]
#[test]
fn descriptor_relative_read_rejects_a_symlinked_parent() {
    let root = test_root("read-parent-link");
    let outside = root.with_extension("read-parent-outside");
    let _ = fs::remove_dir_all(&outside);
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("note.md"), "outside").unwrap();
    std::os::unix::fs::symlink(&outside, root.join("linked")).unwrap();
    assert!(read_nofollow(&root.join("linked/note.md"), &root).is_err());
    remove_test_root(&root);
    remove_test_root(&outside);
}

#[test]
fn pending_queue_cap_retains_the_snapshot_for_retry() {
    let root = test_root("pending-cap");
    let modified = SystemTime::UNIX_EPOCH + Duration::from_secs(1);
    let mut current = Snapshot::default();
    for index in 0..=MAX_PENDING_EVENTS {
        current.files.insert(
            root.join(format!("note-{index}.md")),
            FileState { modified, len: 1 },
        );
    }
    let mut pending = BTreeMap::new();
    let mut changed = None;
    assert!(!collect_changes(
        &Snapshot::default(),
        &current,
        &mut pending,
        &mut changed
    ));
    assert_eq!(pending.len(), MAX_PENDING_EVENTS);
    remove_test_root(&root);
}

#[test]
fn successful_batches_are_removed_while_later_batches_remain_pending() {
    let root = test_root("partial-flush");
    let first = root.join("first");
    let second = root.join("second");
    let mut pending = BTreeMap::from([
        (
            "first".to_owned(),
            Pending::File {
                path: first,
                event_time: Utc::now(),
            },
        ),
        (
            "second".to_owned(),
            Pending::File {
                path: second,
                event_time: Utc::now(),
            },
        ),
    ]);
    let batch = vec![("first".to_owned(), "first concept".to_owned())];
    remove_successful_batch(&mut pending, &batch);
    assert!(!pending.contains_key("first"));
    assert!(pending.contains_key("second"));
    remove_test_root(&root);
}

#[test]
fn system_time_to_utc_preserves_pre_epoch_mtimes() {
    let time = SystemTime::UNIX_EPOCH - Duration::new(1, 500_000_000);
    assert_eq!(
        system_time_to_utc(time).unwrap().to_rfc3339(),
        "1969-12-31T23:59:58.500+00:00"
    );
}

#[tokio::test]
async fn cancellation_joins_the_child_task() {
    let (cancel, cancelled) = oneshot::channel();
    let task = tokio::spawn(async move {
        tokio::select! {
            _ = cancelled => Ok::<(), WatchError>(()),
            _ = tokio::time::sleep(Duration::from_secs(60)) => Err(WatchError::TaskFailed),
        }
    });
    let watcher = Watcher {
        cancel: Some((cancel, Arc::new(AtomicBool::new(false)))),
        task,
    };
    watcher.stop().await.unwrap();
}

#[tokio::test]
async fn cancellation_interrupts_an_in_flight_derive_future() {
    let (cancel, mut cancelled) = oneshot::channel();
    let task = tokio::spawn(async move {
        await_or_cancel(&mut cancelled, tokio::time::sleep(Duration::from_secs(60))).await
    });
    tokio::time::sleep(Duration::from_millis(10)).await;
    cancel.send(()).unwrap();
    assert!(tokio::time::timeout(Duration::from_millis(100), task)
        .await
        .unwrap()
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn live_file_change_reaches_the_graph_before_cancellation() {
    let root = test_root("live");
    let path = root.join("note.md");
    fs::write(&path, "initial note").unwrap();
    let mut config = crate::config::Config::default();
    config.store.kind = lambo::StoreKind::Memory;
    config.embedder.kind = lambo::EmbedderKind::Fixture;
    config.embedder.dim = 1024;
    config.session.id = format!("mooshik-watcher-{}", std::process::id());
    let memory = Arc::new(crate::memory::open(&config).await.unwrap());
    let watcher = Watcher::start(
        &tokio::runtime::Handle::current(),
        Arc::clone(&memory),
        WriteLane::new(),
        root.clone(),
        "watcher".to_owned(),
        None,
    )
    .unwrap();
    tokio::time::sleep(Duration::from_millis(20)).await;
    fs::write(&path, "a changed note that is now longer").unwrap();
    tokio::time::sleep(Duration::from_millis(700)).await;
    watcher.stop().await.unwrap();
    let found = memory
        .graph()
        .read()
        .concepts()
        .any(|concept| concept.content == "workspace file changed: note.md");
    memory.close().await.unwrap();
    remove_test_root(&root);
    assert!(found, "the live file event must be derived before stop");
}

pub(crate) fn test_root(name: &str) -> PathBuf {
    let root = crate::secure_path::canonical_temp_dir()
        .join(format!("mooshik-m12d-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root
}

pub(crate) fn remove_test_root(root: &Path) {
    let _ = fs::remove_dir_all(root);
}

#[cfg(unix)]
pub(crate) fn initialize_repo(root: &Path, message: &str) {
    fs::create_dir_all(root).unwrap();
    run_git(root, &["init", "-q"]);
    run_git(root, &["config", "user.email", "test@example.invalid"]);
    run_git(root, &["config", "user.name", "Mooshik Test"]);
    fs::write(root.join("note.md"), message).unwrap();
    run_git(root, &["add", "note.md"]);
    run_git(root, &["commit", "-qm", message]);
}

#[cfg(unix)]
pub(crate) fn git_output(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "git command failed: {args:?}");
    String::from_utf8(output.stdout).unwrap()
}

#[cfg(unix)]
pub(crate) fn run_git(root: &Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .status()
        .unwrap();
    assert!(status.success(), "git command failed: {args:?}");
}

#[cfg(unix)]
pub(crate) fn run_git_with_date(root: &Path, args: &[&str], date: &str) {
    let status = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_AUTHOR_DATE", date)
        .env("GIT_COMMITTER_DATE", date)
        .status()
        .unwrap();
    assert!(status.success(), "git command failed: {args:?}");
}
