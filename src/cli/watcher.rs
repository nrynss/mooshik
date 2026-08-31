//! M12d: the live workspace watcher.
//!
//! The watcher is deliberately a child of the TUI runtime. It polls a bounded
//! workspace snapshot rather than installing a daemon or watching arbitrary
//! filesystem events. Polling also gives us one place to apply the ingester's
//! source rules: symlinks and generated directories are ignored, repositories
//! contribute commit metadata only, and files outside repositories are
//! admitted only by the existing `.md`, `.markdown`, `.txt`, and `.rst`
//! extension allowlist.
//!
//! File events use a metadata-only payload. The complete file is scanned with
//! the ingester's whole-document secret policy, but its contents never cross
//! into the graph. A git event carries commit metadata (SHA, repository path,
//! author time, and message) and never a patch or diff. Both event kinds are
//! coalesced for 250 ms and every graph write takes the pane's [`WriteLane`].
//! Discovery runs in one dedicated blocking worker at a time. Quiet workspaces use
//! an adaptive 100--250 ms poll interval, trading a bounded amount of latency
//! for keeping recursive walks and git subprocesses off the TUI runtime.
//! Git output is capped at 2 MiB and each poll admits at most 256 commits;
//! pending events are capped at 2,048. A cap retains only the affected
//! repository's old head and retries it, so pressure is explicit backpressure
//! rather than silent loss; unrelated file state still advances. A worker
//! stuck in an uninterruptible filesystem syscall is detached after a
//! 250-ms shutdown grace period; it owns no memory or write resources.

use std::{
    collections::BTreeMap,
    fs,
    future::Future,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant, SystemTime},
};

#[cfg(unix)]
use std::os::fd::{AsRawFd, FromRawFd};

use chrono::{DateTime, Utc};
use lambo::{graph::derive::ParentOf, ConceptType, Memory};
use tokio::{sync::oneshot, task::JoinHandle};

use crate::{memory::WriteLane, text, vault::SharedVault};

const POLL_INTERVAL: Duration = Duration::from_millis(100);
const QUIET_POLL_INTERVAL: Duration = Duration::from_millis(250);
const DEBOUNCE: Duration = Duration::from_millis(250);
const MAX_CONCEPTS_PER_DERIVE: usize = 64;
const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_GIT_OUTPUT_BYTES: usize = 2 * 1024 * 1024;
const MAX_GIT_COMMITS_PER_POLL: usize = 256;
const MAX_PENDING_EVENTS: usize = 2048;
const DISCOVERY_SHUTDOWN_TIMEOUT: Duration = Duration::from_millis(250);
const ALLOWED_EXTENSIONS: &[&str] = &["md", "markdown", "txt", "rst"];
const SKIP_DIRS: &[&str] = &[
    ".git",
    ".ingest",
    ".venv",
    "venv",
    "node_modules",
    "target",
    "__pycache__",
    ".pytest_cache",
];

/// A live watcher and its cancellation handle. The task has no independent
/// runtime: dropping the pane's runtime cancels it, while the normal path calls
/// [`Watcher::stop`] and waits for the task before closing `Memory`.
pub(crate) struct Watcher {
    cancel: Option<(oneshot::Sender<()>, Arc<AtomicBool>)>,
    task: JoinHandle<Result<(), WatchError>>,
}

impl Watcher {
    pub(crate) fn start(
        handle: &tokio::runtime::Handle,
        memory: Arc<Memory>,
        writes: WriteLane,
        root: PathBuf,
        agent: String,
        vault: Option<SharedVault>,
    ) -> Result<Self, WatchError> {
        let root = fs::canonicalize(root).map_err(|_| WatchError::WorkspaceUnavailable)?;
        if !root.is_dir() {
            return Err(WatchError::WorkspaceUnavailable);
        }
        #[cfg(not(unix))]
        {
            // There is no portable descriptor-relative, no-follow traversal
            // primitive in std on these targets. Refuse the live watcher
            // instead of turning a path race into a workspace escape. The
            // live command closes the pane and returns this error; the TUI
            // does not remain available without the watcher.
            return Err(WatchError::WorkspaceUnavailable);
        }
        let (cancel, cancelled) = oneshot::channel();
        let discovery_cancelled = Arc::new(AtomicBool::new(false));
        let task = handle.spawn(run(
            memory,
            writes,
            root,
            agent,
            vault,
            cancelled,
            Arc::clone(&discovery_cancelled),
        ));
        Ok(Self {
            cancel: Some((cancel, discovery_cancelled)),
            task,
        })
    }

    pub(crate) async fn stop(mut self) -> Result<(), WatchError> {
        if let Some((cancel, discovery_cancelled)) = self.cancel.take() {
            discovery_cancelled.store(true, Ordering::Release);
            let _ = cancel.send(());
        }
        match self.task.await {
            Ok(result) => result,
            Err(_) => Err(WatchError::TaskFailed),
        }
    }
}

#[derive(Debug)]
pub(crate) enum WatchError {
    WorkspaceUnavailable,
    TaskFailed,
    Memory,
}

impl std::fmt::Display for WatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::WorkspaceUnavailable => text::get("tui.watcher_workspace_unavailable"),
            Self::TaskFailed => text::get("tui.watcher_task_failed"),
            Self::Memory => text::get("tui.watcher_memory_failed"),
        })
    }
}

impl std::error::Error for WatchError {}

#[derive(Clone, Debug, Eq, PartialEq)]
struct FileState {
    modified: SystemTime,
    len: u64,
}

#[derive(Clone, Debug)]
struct Commit {
    repo: PathBuf,
    sha: String,
    author_time: DateTime<Utc>,
    message: String,
}

#[derive(Clone, Debug)]
enum Pending {
    File {
        path: PathBuf,
        event_time: DateTime<Utc>,
    },
    Commit(Commit),
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum GitHead {
    Commit(String),
    Unborn,
    /// Discovery could not establish a trustworthy HEAD. This is retained so
    /// recovery can scan the repository from an unknown baseline instead of
    /// silently treating its current history as already seen.
    Unknown,
}

#[derive(Clone, Default, Eq, PartialEq)]
struct Snapshot {
    files: BTreeMap<PathBuf, FileState>,
    heads: BTreeMap<PathBuf, GitHead>,
    git_failures: Vec<PathBuf>,
}

struct DiscoveryResult {
    snapshot: Snapshot,
    pending: BTreeMap<String, Pending>,
    changed: bool,
    retry: bool,
}

struct ChangeResult {
    snapshot: Snapshot,
    changed: bool,
    retry: bool,
}

/// A discovery worker is intentionally separate from Tokio's blocking pool.
/// The pool waits for blocking tasks during runtime shutdown, while a
/// filesystem syscall cannot be forcibly interrupted portably. The worker is
/// therefore awaited for a short grace period and then detached if necessary;
/// it owns only discovery data and the cancellation flag, never `Memory`.
struct DiscoveryTask {
    result: oneshot::Receiver<DiscoveryResult>,
    worker: Option<thread::JoinHandle<()>>,
}

fn start_discovery(
    root: PathBuf,
    previous: Option<Snapshot>,
    pending: BTreeMap<String, Pending>,
    cancelled: Arc<AtomicBool>,
) -> DiscoveryTask {
    let (sender, result) = oneshot::channel();
    let worker = thread::spawn(move || {
        let discovered = discover_and_collect(&root, previous.as_ref(), pending, &cancelled);
        let _ = sender.send(discovered);
    });
    DiscoveryTask {
        result,
        worker: Some(worker),
    }
}

impl Snapshot {
    #[cfg(test)]
    fn discover(root: &Path) -> Self {
        static NEVER_CANCELLED: AtomicBool = AtomicBool::new(false);
        Self::discover_with_cancel(root, &NEVER_CANCELLED)
    }

    fn discover_with_cancel(root: &Path, cancelled: &AtomicBool) -> Self {
        let mut snapshot = Self::default();
        let mut dirs = Vec::new();
        if is_git_marker(root) {
            match git_head_with_cancel(root, cancelled) {
                Ok(head) => {
                    snapshot.heads.insert(root.to_path_buf(), head);
                }
                Err(_) => {
                    snapshot.heads.insert(root.to_path_buf(), GitHead::Unknown);
                    snapshot.git_failures.push(root.to_path_buf());
                }
            }
        } else {
            dirs.push(root.to_path_buf());
        }
        while let Some(dir) = dirs.pop() {
            if cancelled.load(Ordering::Acquire) {
                return snapshot;
            }
            let entries = match fs::read_dir(&dir) {
                Ok(entries) => entries,
                Err(_) => continue,
            };
            for entry in entries.flatten() {
                if cancelled.load(Ordering::Acquire) {
                    return snapshot;
                }
                let path = entry.path();
                let file_type = match entry.file_type() {
                    Ok(file_type) => file_type,
                    Err(_) => continue,
                };
                if file_type.is_symlink() {
                    continue;
                }
                if file_type.is_dir() {
                    if path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| SKIP_DIRS.contains(&name))
                    {
                        continue;
                    }
                    if is_git_marker(&path) {
                        match git_head_with_cancel(&path, cancelled) {
                            Ok(head) => {
                                snapshot.heads.insert(path.clone(), head);
                            }
                            Err(_) => {
                                snapshot.heads.insert(path.clone(), GitHead::Unknown);
                                snapshot.git_failures.push(path.clone());
                            }
                        }
                        continue; // repositories are metadata-only sources
                    }
                    dirs.push(path);
                    continue;
                }
                if file_type.is_file() && is_allowed_file(&path) {
                    if let Ok(metadata) = entry.metadata() {
                        if let Ok(modified) = metadata.modified() {
                            snapshot.files.insert(
                                path,
                                FileState {
                                    modified,
                                    len: metadata.len(),
                                },
                            );
                        }
                    }
                }
            }
        }
        snapshot
    }
}

fn discover_and_collect(
    root: &Path,
    previous: Option<&Snapshot>,
    mut pending: BTreeMap<String, Pending>,
    cancelled: &AtomicBool,
) -> DiscoveryResult {
    let snapshot = Snapshot::discover_with_cancel(root, cancelled);
    let Some(previous) = previous else {
        return DiscoveryResult {
            retry: !snapshot.git_failures.is_empty(),
            snapshot,
            pending,
            changed: false,
        };
    };
    let mut changed_at = None;
    let result = collect_changes_with_cancel(
        previous,
        &snapshot,
        &mut pending,
        &mut changed_at,
        cancelled,
    );
    DiscoveryResult {
        snapshot: result.snapshot,
        pending,
        changed: result.changed,
        retry: result.retry,
    }
}

async fn run(
    memory: Arc<Memory>,
    writes: WriteLane,
    root: PathBuf,
    agent: String,
    vault: Option<SharedVault>,
    mut cancelled: oneshot::Receiver<()>,
    discovery_cancelled: Arc<AtomicBool>,
) -> Result<(), WatchError> {
    let mut pending = BTreeMap::<String, Pending>::new();
    let mut changed_at = None;
    let mut previous = None;
    let mut poll_interval = POLL_INTERVAL;
    loop {
        if previous.is_some() {
            tokio::select! {
                _ = &mut cancelled => return Ok(()),
                _ = tokio::time::sleep(poll_interval) => {}
            }
        }

        // Directory walks and git subprocesses are blocking operations. Keep
        // them off the TUI runtime, and never start a second walk while the
        // first is still running. The handle is retained through cancellation
        // and joined below; the atomic flag lets directory traversal and git
        // subprocesses stop before that join completes.
        let prior = previous.clone();
        let pending_for_discovery = std::mem::take(&mut pending);
        let mut discovery = start_discovery(
            root.clone(),
            prior,
            pending_for_discovery,
            Arc::clone(&discovery_cancelled),
        );
        let discovered = tokio::select! {
            _ = &mut cancelled => {
                discovery_cancelled.store(true, Ordering::Release);
                // Git subprocesses observe the flag and are killed/joined by
                // the worker. A directory syscall itself may be
                // uninterruptible, so shutdown is bounded and the worker is
                // then detached with no access to pane-owned resources.
                let _ = tokio::time::timeout(
                    DISCOVERY_SHUTDOWN_TIMEOUT,
                    &mut discovery.result,
                )
                .await;
                return Ok(());
            },
            result = &mut discovery.result => result.map_err(|_| WatchError::TaskFailed)?,
        };
        if let Some(worker) = discovery.worker.take() {
            // The worker sends its result immediately before returning, so
            // this join cannot encounter an arbitrary filesystem syscall. It
            // ensures the normal path never leaves a detached worker behind.
            if worker.join().is_err() {
                return Err(WatchError::TaskFailed);
            }
        }
        pending = discovered.pending;
        if discovered.changed {
            changed_at = Some(Instant::now());
        }

        if previous.is_none() {
            previous = Some(discovered.snapshot);
            poll_interval = if discovered.retry {
                POLL_INTERVAL
            } else {
                QUIET_POLL_INTERVAL
            };
            continue;
        }

        previous = Some(discovered.snapshot);
        poll_interval = if discovered.retry || discovered.changed {
            POLL_INTERVAL
        } else {
            (poll_interval * 2).min(QUIET_POLL_INTERVAL)
        };

        if changed_at.is_some_and(|at| at.elapsed() >= DEBOUNCE) && !pending.is_empty() {
            match flush_pending(
                &memory,
                &writes,
                &agent,
                &vault,
                &root,
                &mut pending,
                &mut cancelled,
            )
            .await
            {
                Ok(true) => return Ok(()),
                Ok(false) => {}
                Err(error) => {
                    // A transient embedder/store failure must not kill the
                    // ambient task. Pending events stay queued and are
                    // retried on the next poll; cancellation still wins.
                    if !matches!(error, WatchError::Memory) {
                        return Err(error);
                    }
                }
            }
            changed_at = (!pending.is_empty()).then(Instant::now);
        }
    }
}

#[cfg(test)]
fn collect_changes(
    previous: &Snapshot,
    current: &Snapshot,
    pending: &mut BTreeMap<String, Pending>,
    changed_at: &mut Option<Instant>,
) -> bool {
    static NEVER_CANCELLED: AtomicBool = AtomicBool::new(false);
    !collect_changes_with_cancel(previous, current, pending, changed_at, &NEVER_CANCELLED).retry
}

/// Cancel checks in [`collect_changes_with_cancel`]. Production uses
/// [`AtomicBool`]; tests may flip after the first load so the file-walk
/// return is actually entered.
trait CollectCancel {
    fn load(&self, order: Ordering) -> bool;
    fn git_cancel_flag(&self) -> &AtomicBool;
}

impl CollectCancel for AtomicBool {
    fn load(&self, order: Ordering) -> bool {
        AtomicBool::load(self, order)
    }

    fn git_cancel_flag(&self) -> &AtomicBool {
        self
    }
}

fn collect_changes_with_cancel<C: CollectCancel>(
    previous: &Snapshot,
    current: &Snapshot,
    pending: &mut BTreeMap<String, Pending>,
    changed_at: &mut Option<Instant>,
    cancelled: &C,
) -> ChangeResult {
    let mut next = current.clone();
    let mut retry = false;
    let mut changed = false;

    // Git failures are per repository. Keep the old head only for the
    // affected repository while allowing unrelated file state to advance.
    for repo in &current.git_failures {
        retry = true;
        if let Some(old_head) = previous.heads.get(repo) {
            next.heads.insert(repo.clone(), old_head.clone());
        } else {
            // The repository appeared after the previous snapshot but its
            // first Git read failed. Keep an explicit marker so recovery is
            // replayed from an unknown baseline rather than silently
            // baselining the history that existed during the failure.
            next.heads.insert(repo.clone(), GitHead::Unknown);
        }
    }

    if cancelled.load(Ordering::Acquire) {
        return ChangeResult {
            snapshot: snapshot_retaining_failed_discoveries(previous, current),
            changed: false,
            retry: true,
        };
    }
    for (path, state) in &current.files {
        if cancelled.load(Ordering::Acquire) {
            return ChangeResult {
                snapshot: snapshot_retaining_failed_discoveries(previous, current),
                changed: false,
                retry: true,
            };
        }
        if previous.files.get(path) != Some(state) {
            let Some(event_time) = system_time_to_utc(state.modified) else {
                continue;
            };
            let key = format!("file:{}", path.display());
            if !enqueue_pending(
                pending,
                key,
                Pending::File {
                    path: path.clone(),
                    event_time,
                },
            ) {
                next.files.remove(path);
                if let Some(old_state) = previous.files.get(path) {
                    next.files.insert(path.clone(), old_state.clone());
                }
                retry = true;
                continue;
            }
            changed = true;
            *changed_at = Some(Instant::now());
        }
    }
    for (repo, head) in &current.heads {
        if current.git_failures.contains(repo) {
            continue;
        }
        let Some(previous_head) = previous.heads.get(repo) else {
            continue;
        };
        if previous_head == head {
            continue;
        }
        let (old, new) = match (previous_head, head) {
            (GitHead::Commit(old), GitHead::Commit(new)) => (Some(old.as_str()), new),
            (GitHead::Unborn, GitHead::Commit(new)) | (GitHead::Unknown, GitHead::Commit(new)) => {
                (None, new)
            }
            (GitHead::Unknown, GitHead::Unborn) => continue,
            (GitHead::Commit(_), GitHead::Unborn) | (GitHead::Unborn, GitHead::Unborn) => continue,
            (_, GitHead::Unknown) => continue,
        };
        let Ok(commits) =
            git_commits_between_with_cancel(repo, old, new, cancelled.git_cancel_flag())
        else {
            retry = true;
            next.heads.insert(repo.clone(), previous_head.clone());
            continue;
        };
        for commit in commits {
            let key = format!("git:{}#{}", repo.display(), commit.sha);
            if !enqueue_pending(pending, key, Pending::Commit(commit)) {
                retry = true;
                next.heads.insert(repo.clone(), previous_head.clone());
                break;
            }
            changed = true;
            *changed_at = Some(Instant::now());
        }
    }
    // A newly-created repository is baselined by `Snapshot::discover`, but a
    // repository whose prior discovery failed carries Unknown and is replayed
    // from its current head when it recovers.
    ChangeResult {
        snapshot: next,
        changed,
        retry,
    }
}

/// A cancelled poll must not apply a partial file walk, but it must still
/// remember repositories whose Git read already failed. Dropping those
/// markers would baseline history that existed during the failure once the
/// next healthy poll arrives.
fn snapshot_retaining_failed_discoveries(previous: &Snapshot, current: &Snapshot) -> Snapshot {
    let mut snapshot = previous.clone();
    for repo in &current.git_failures {
        if !snapshot.heads.contains_key(repo) {
            snapshot.heads.insert(repo.clone(), GitHead::Unknown);
        }
    }
    snapshot
}

fn enqueue_pending(pending: &mut BTreeMap<String, Pending>, key: String, event: Pending) -> bool {
    if !pending.contains_key(&key) && pending.len() >= MAX_PENDING_EVENTS {
        return false;
    }
    pending.insert(key, event);
    true
}

async fn flush_pending(
    memory: &Memory,
    writes: &WriteLane,
    agent: &str,
    vault: &Option<SharedVault>,
    root: &Path,
    pending: &mut BTreeMap<String, Pending>,
    cancelled: &mut oneshot::Receiver<()>,
) -> Result<bool, WatchError> {
    let mut groups = BTreeMap::<DateTime<Utc>, Vec<(String, String)>>::new();
    let mut ignored = Vec::new();
    for (key, event) in pending.iter() {
        match event_to_concept(event, vault, root) {
            Ok(Some((content, event_time))) => {
                groups
                    .entry(event_time)
                    .or_default()
                    .push((key.clone(), content));
            }
            Ok(None) => ignored.push(key.clone()), // secret hit or an ignored save
            Err(()) => {}                          // unreadable file: retry it
        }
    }
    for key in ignored {
        pending.remove(&key);
    }

    for (event_time, events) in groups {
        for batch in events.chunks(MAX_CONCEPTS_PER_DERIVE) {
            let contents: Vec<(&str, ConceptType)> = batch
                .iter()
                .map(|(_, content)| (content.as_str(), ConceptType::Observation))
                .collect();
            let Some(_lane) = await_or_cancel(cancelled, writes.enter()).await else {
                return Ok(true);
            };
            let result = await_or_cancel(
                cancelled,
                memory.derive_for_ingest_as(
                    &lambo::AgentId::new(agent),
                    Some(event_time),
                    &contents,
                    &ParentOf::none(),
                ),
            )
            .await;
            let Some(result) = result else {
                return Ok(true);
            };
            result.map_err(|_| WatchError::Memory)?;
            // Remove each successful batch immediately. A later failed batch
            // is retried, but a successful batch is never replayed.
            remove_successful_batch(pending, batch);
        }
    }
    Ok(false)
}

fn remove_successful_batch(pending: &mut BTreeMap<String, Pending>, batch: &[(String, String)]) {
    for (key, _) in batch {
        pending.remove(key);
    }
}

async fn await_or_cancel<T>(
    cancelled: &mut oneshot::Receiver<()>,
    future: impl Future<Output = T>,
) -> Option<T> {
    tokio::select! {
        _ = cancelled => None,
        result = future => Some(result),
    }
}

fn event_to_concept(
    event: &Pending,
    vault: &Option<SharedVault>,
    root: &Path,
) -> Result<Option<(String, DateTime<Utc>)>, ()> {
    match event {
        Pending::File { path, event_time } => {
            let bytes = read_nofollow(path, root).map_err(|_| ())?;
            let text = String::from_utf8_lossy(&bytes);
            let relative = path
                .strip_prefix(root)
                .unwrap_or(path)
                .to_string_lossy()
                .replace(std::path::MAIN_SEPARATOR, "/");
            if find_secret(&text, vault) || find_secret(&relative, vault) {
                return Ok(None);
            }
            Ok(Some((
                format!("workspace file changed: {relative}"),
                *event_time,
            )))
        }
        Pending::Commit(commit) => {
            if find_secret(&commit.message, vault)
                || find_secret(&commit.repo.to_string_lossy(), vault)
            {
                return Ok(None);
            }
            let repo = commit
                .repo
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("workspace");
            let message = commit.message.trim().replace('\n', " ");
            Ok(Some((
                format!("git commit {} in {repo}: {message}", commit.sha),
                commit.author_time,
            )))
        }
    }
}

fn is_allowed_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            let extension = extension.to_ascii_lowercase();
            ALLOWED_EXTENSIONS.contains(&extension.as_str())
        })
}

fn system_time_to_utc(time: SystemTime) -> Option<DateTime<Utc>> {
    match time.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(duration) => DateTime::<Utc>::from_timestamp(
            i64::try_from(duration.as_secs()).ok()?,
            duration.subsec_nanos(),
        ),
        Err(error) => {
            let duration = error.duration();
            let seconds = i64::try_from(duration.as_secs()).ok()?;
            if duration.subsec_nanos() == 0 {
                DateTime::<Utc>::from_timestamp(-seconds, 0)
            } else {
                DateTime::<Utc>::from_timestamp(
                    seconds.checked_neg()?.checked_sub(1)?,
                    1_000_000_000 - duration.subsec_nanos(),
                )
            }
        }
    }
}

fn is_git_marker(path: &Path) -> bool {
    fs::symlink_metadata(path.join(".git"))
        .map(|metadata| {
            let file_type = metadata.file_type();
            !file_type.is_symlink() && (file_type.is_dir() || file_type.is_file())
        })
        .unwrap_or(false)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GitFailure {
    Command,
    Cleanup,
    MalformedOutput,
    OutputTooLarge,
    TooManyCommits,
}

#[cfg(unix)]
struct SecureGitRepo {
    repo_dir: fs::File,
    git_dir: fs::File,
}

#[cfg(unix)]
impl SecureGitRepo {
    fn open(path: &Path) -> Result<Self, GitFailure> {
        let repo_dir = open_directory_path(path).map_err(|_| GitFailure::Command)?;
        let marker_fd = openat_no_follow(&repo_dir, std::ffi::OsStr::new(".git"), false)
            .map_err(|_| GitFailure::Command)?;
        let marker_metadata = marker_fd.metadata().map_err(|_| GitFailure::Command)?;
        let git_dir = if marker_metadata.is_dir() {
            marker_fd
        } else if marker_metadata.is_file() {
            let marker = marker_fd;
            if marker_metadata.len() > 4096 {
                return Err(GitFailure::Command);
            }
            let mut bytes = Vec::new();
            marker
                .take(4097)
                .read_to_end(&mut bytes)
                .map_err(|_| GitFailure::Command)?;
            let pointer = std::str::from_utf8(&bytes)
                .map_err(|_| GitFailure::Command)?
                .lines()
                .next()
                .and_then(|line| line.strip_prefix("gitdir:"))
                .map(str::trim)
                .filter(|target| !target.is_empty())
                .ok_or(GitFailure::Command)?;
            let target = Path::new(pointer);
            if target.is_absolute() {
                open_directory_path(target).map_err(|_| GitFailure::Command)?
            } else {
                open_directory_relative(&repo_dir, target).map_err(|_| GitFailure::Command)?
            }
        } else {
            return Err(GitFailure::Command);
        };
        if !git_dir
            .metadata()
            .map_err(|_| GitFailure::Command)?
            .is_dir()
        {
            return Err(GitFailure::Command);
        }
        Ok(Self { repo_dir, git_dir })
    }

    fn make_inheritable(&self) -> io::Result<()> {
        for file in [&self.repo_dir, &self.git_dir] {
            let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFD) };
            if flags < 0 {
                return Err(io::Error::last_os_error());
            }
            if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFD, flags & !libc::FD_CLOEXEC) }
                < 0
            {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(())
    }

    fn command(&self, args: &[&str]) -> Command {
        self.command_with_git_dir(args, None)
    }

    fn command_with_git_dir(&self, args: &[&str], git_dir_override: Option<&Path>) -> Command {
        let mut command = Command::new("git");
        if let Some(git_dir) = git_dir_override {
            // Test-only injection point used to prove that the sanitizer wins
            // over an inherited repository-selection variable.
            command.env("GIT_DIR", git_dir);
        }
        isolate_git_environment(&mut command);
        command
            .arg("-C")
            .arg(fd_path(self.repo_dir.as_raw_fd()))
            .arg("--git-dir")
            .arg(fd_path(self.git_dir.as_raw_fd()))
            .args(args);
        command
    }
}

#[cfg(unix)]
fn isolate_git_environment(command: &mut Command) {
    // Git's repository and object discovery is environment-controlled. Remove
    // every GIT_* variable, not only the currently known subset, before
    // supplying stable descriptor paths explicitly.
    for key in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_INDEX_FILE",
        "GIT_NAMESPACE",
        "GIT_CEILING_DIRECTORIES",
        "GIT_DISCOVERY_ACROSS_FILESYSTEM",
    ] {
        command.env_remove(key);
    }
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            command.env_remove(key);
        }
    }
}

#[cfg(unix)]
fn fd_path(fd: i32) -> PathBuf {
    #[cfg(target_os = "linux")]
    let prefix = "/proc/self/fd";
    #[cfg(not(target_os = "linux"))]
    let prefix = "/dev/fd";
    PathBuf::from(prefix).join(fd.to_string())
}

#[cfg(unix)]
fn openat_no_follow(
    directory: &fs::File,
    name: &std::ffi::OsStr,
    require_directory: bool,
) -> io::Result<fs::File> {
    use std::os::unix::ffi::OsStrExt;

    let name = std::ffi::CString::new(name.as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NUL in path component"))?;
    let mut flags = libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW;
    if require_directory {
        flags |= libc::O_DIRECTORY;
    }
    let fd = unsafe { libc::openat(directory.as_raw_fd(), name.as_ptr(), flags) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { fs::File::from_raw_fd(fd) })
}

#[cfg(unix)]
fn open_directory_path(path: &Path) -> io::Result<fs::File> {
    use std::path::Component;

    if !path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "repository path must be absolute",
        ));
    }
    let root_name = std::ffi::CString::new("/").expect("literal has no NUL");
    let root_fd = unsafe {
        libc::open(
            root_name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    if root_fd < 0 {
        return Err(io::Error::last_os_error());
    }
    let root = unsafe { fs::File::from_raw_fd(root_fd) };
    let relative = path
        .components()
        .filter(|component| !matches!(component, Component::RootDir));
    open_directory_components(root, relative)
}

#[cfg(unix)]
fn open_directory_relative(base: &fs::File, path: &Path) -> io::Result<fs::File> {
    use std::path::Component;

    let relative = path.components().filter(|component| {
        matches!(
            component,
            Component::CurDir | Component::ParentDir | Component::Normal(_)
        )
    });
    let base = base.try_clone()?;
    open_directory_components(base, relative)
}

#[cfg(unix)]
fn open_directory_components<'a, I>(mut directory: fs::File, components: I) -> io::Result<fs::File>
where
    I: IntoIterator<Item = std::path::Component<'a>>,
{
    // This helper is fed by owned paths in this module. Keep the actual
    // descriptor walk in one place so every component uses O_NOFOLLOW.
    for component in components {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                directory = openat_no_follow(&directory, std::ffi::OsStr::new(".."), true)?;
            }
            std::path::Component::Normal(name) => {
                directory = openat_no_follow(&directory, name, true)?;
            }
            std::path::Component::RootDir | std::path::Component::Prefix(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "unexpected path component",
                ));
            }
        }
    }
    Ok(directory)
}

#[cfg(unix)]
fn git_head_with_cancel(repo: &Path, cancelled: &AtomicBool) -> Result<GitHead, GitFailure> {
    let repo = SecureGitRepo::open(repo)?;
    match git_with_cancel(&repo, &["rev-parse", "--verify", "HEAD"], cancelled) {
        Ok(output) => {
            let head = output.trim();
            if head.is_empty() {
                Ok(GitHead::Unborn)
            } else {
                Ok(GitHead::Commit(head.to_owned()))
            }
        }
        Err(GitFailure::Command) => {
            // `rev-parse --verify HEAD` returns a command failure for an
            // empty, otherwise healthy repository. A symbolic HEAD is the
            // specific unborn marker; a detached/broken HEAD must not be
            // converted into an empty repository merely because `status`
            // happens to tolerate it.
            let inside =
                git_with_cancel(&repo, &["rev-parse", "--is-inside-work-tree"], cancelled)?;
            if inside.trim() != "true" {
                return Err(GitFailure::Command);
            }
            let symbolic_head = git_with_cancel(&repo, &["symbolic-ref", "HEAD"], cancelled)?;
            if symbolic_head.trim().is_empty() {
                return Err(GitFailure::MalformedOutput);
            }
            let (ref_status, _) =
                git_process_with_cancel_input(&repo, &["show-ref", "--head"], &[], cancelled)?;
            match ref_status.code() {
                // `show-ref --head` uses status 1 for a healthy repository
                // with no refs. Any refs (status 0), malformed refs, or
                // another failure mean the HEAD transition is not healthy.
                Some(1) => Ok(GitHead::Unborn),
                _ => Err(GitFailure::Command),
            }
        }
        Err(error) => Err(error),
    }
}

#[cfg(not(unix))]
fn git_head_with_cancel(_repo: &Path, _cancelled: &AtomicBool) -> Result<GitHead, GitFailure> {
    Err(GitFailure::Command)
}

#[cfg(unix)]
fn git_commits_between_with_cancel(
    repo: &Path,
    old: Option<&str>,
    new: &str,
    cancelled: &AtomicBool,
) -> Result<Vec<Commit>, GitFailure> {
    let secure_repo = SecureGitRepo::open(repo)?;
    let range = old.map_or_else(|| new.to_owned(), |old| format!("{old}..{new}"));
    let output = git_with_cancel(
        &secure_repo,
        &["log", "--no-patch", "--pretty=format:%H%x00%aI%x00", &range],
        cancelled,
    )?;
    let metadata = parse_commit_headers(&output)?;
    if metadata.is_empty() {
        return Ok(Vec::new());
    }
    let request: String = metadata.iter().map(|(sha, _)| format!("{sha}\n")).collect();
    let objects = git_bytes_with_cancel_input(
        &secure_repo,
        &["cat-file", "--batch"],
        request.as_bytes(),
        cancelled,
    )?;
    parse_commit_messages(repo, &metadata, &objects)
}

#[cfg(not(unix))]
fn git_commits_between_with_cancel(
    _repo: &Path,
    _old: Option<&str>,
    _new: &str,
    _cancelled: &AtomicBool,
) -> Result<Vec<Commit>, GitFailure> {
    Err(GitFailure::Command)
}

fn parse_commit_headers(output: &str) -> Result<Vec<(String, DateTime<Utc>)>, GitFailure> {
    let mut fields = output.split('\0').collect::<Vec<_>>();
    if fields.last() == Some(&"") {
        fields.pop();
    }
    if fields.len() % 2 != 0 {
        return Err(GitFailure::MalformedOutput);
    }
    let mut metadata = Vec::with_capacity(fields.len() / 2);
    for pair in fields.chunks_exact(2) {
        let sha = pair[0];
        let date = pair[1];
        if !is_object_id(sha) || date.is_empty() {
            return Err(GitFailure::MalformedOutput);
        }
        let author_time = DateTime::parse_from_rfc3339(date)
            .map_err(|_| GitFailure::MalformedOutput)?
            .with_timezone(&Utc);
        if metadata.len() == MAX_GIT_COMMITS_PER_POLL {
            return Err(GitFailure::TooManyCommits);
        }
        metadata.push((sha.to_owned(), author_time));
    }
    Ok(metadata)
}

fn parse_commit_messages(
    repo: &Path,
    metadata: &[(String, DateTime<Utc>)],
    output: &[u8],
) -> Result<Vec<Commit>, GitFailure> {
    let mut cursor = 0;
    let mut commits = Vec::with_capacity(metadata.len());
    for (expected_sha, author_time) in metadata {
        let header_end = output[cursor..]
            .iter()
            .position(|byte| *byte == b'\n')
            .ok_or(GitFailure::MalformedOutput)?
            + cursor;
        let header = std::str::from_utf8(&output[cursor..header_end])
            .map_err(|_| GitFailure::MalformedOutput)?;
        let mut fields = header.split(' ');
        let sha = fields.next().ok_or(GitFailure::MalformedOutput)?;
        let kind = fields.next().ok_or(GitFailure::MalformedOutput)?;
        let size = fields
            .next()
            .ok_or(GitFailure::MalformedOutput)?
            .parse::<usize>()
            .map_err(|_| GitFailure::MalformedOutput)?;
        if fields.next().is_some()
            || sha != expected_sha
            || kind != "commit"
            || size > MAX_GIT_OUTPUT_BYTES
        {
            return Err(GitFailure::MalformedOutput);
        }
        cursor = header_end + 1;
        let content_end = cursor
            .checked_add(size)
            .ok_or(GitFailure::MalformedOutput)?;
        if content_end >= output.len() || output[content_end] != b'\n' {
            return Err(GitFailure::MalformedOutput);
        }
        let content = &output[cursor..content_end];
        let separator = content
            .windows(2)
            .position(|window| window == b"\n\n")
            .ok_or(GitFailure::MalformedOutput)?;
        // Git commit objects are byte strings. Preserve NULs and framing
        // bytes, while replacing invalid UTF-8 only at the graph boundary so
        // an otherwise valid commit does not block head advancement forever.
        let message = String::from_utf8_lossy(&content[separator + 2..]).into_owned();
        commits.push(Commit {
            repo: repo.to_path_buf(),
            sha: expected_sha.clone(),
            author_time: *author_time,
            message,
        });
        cursor = content_end + 1;
    }
    if cursor != output.len() {
        return Err(GitFailure::MalformedOutput);
    }
    Ok(commits)
}

fn is_object_id(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(unix)]
fn git_with_cancel(
    repo: &SecureGitRepo,
    args: &[&str],
    cancelled: &AtomicBool,
) -> Result<String, GitFailure> {
    let bytes = git_bytes_with_cancel_input(repo, args, &[], cancelled)?;
    String::from_utf8(bytes).map_err(|_| GitFailure::MalformedOutput)
}

#[cfg(unix)]
fn git_bytes_with_cancel_input(
    repo: &SecureGitRepo,
    args: &[&str],
    input: &[u8],
    cancelled: &AtomicBool,
) -> Result<Vec<u8>, GitFailure> {
    let (status, bytes) = git_process_with_cancel_input(repo, args, input, cancelled)?;
    if !status.success() {
        return Err(GitFailure::Command);
    }
    Ok(bytes)
}

#[cfg(unix)]
fn git_process_with_cancel_input(
    repo: &SecureGitRepo,
    args: &[&str],
    input: &[u8],
    cancelled: &AtomicBool,
) -> Result<(std::process::ExitStatus, Vec<u8>), GitFailure> {
    if cancelled.load(Ordering::Acquire) {
        return Err(GitFailure::Command);
    }
    repo.make_inheritable().map_err(|_| GitFailure::Cleanup)?;
    let mut command = repo.command(args);
    command.stdout(Stdio::piped()).stderr(Stdio::null());
    if input.is_empty() {
        command.stdin(Stdio::null());
    } else {
        command.stdin(Stdio::piped());
    }
    let mut child = command.spawn().map_err(|_| GitFailure::Command)?;
    let Some(mut stdout) = child.stdout.take() else {
        return match reap_child(&mut child, true) {
            Ok(_) => Err(GitFailure::Command),
            Err(error) => Err(error),
        };
    };
    let output_too_large = Arc::new(AtomicBool::new(false));
    let output_too_large_reader = Arc::clone(&output_too_large);
    let reader_failed = Arc::new(AtomicBool::new(false));
    let reader_failed_reader = Arc::clone(&reader_failed);
    let reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stdout
            .by_ref()
            .take((MAX_GIT_OUTPUT_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map(|_| ());
        if result.is_err() {
            reader_failed_reader.store(true, Ordering::Release);
        }
        if bytes.len() > MAX_GIT_OUTPUT_BYTES {
            output_too_large_reader.store(true, Ordering::Release);
        }
        (result, bytes)
    });
    if !input.is_empty() {
        let Some(mut stdin) = child.stdin.take() else {
            return finish_git_process(child, reader, true, Some(GitFailure::Command));
        };
        if stdin.write_all(input).is_err() {
            return finish_git_process(child, reader, true, Some(GitFailure::Command));
        }
    }
    let mut termination_error = None;
    let status = loop {
        if cancelled.load(Ordering::Acquire)
            || output_too_large.load(Ordering::Acquire)
            || reader_failed.load(Ordering::Acquire)
        {
            let reason = if output_too_large.load(Ordering::Acquire)
                && !cancelled.load(Ordering::Acquire)
                && !reader_failed.load(Ordering::Acquire)
            {
                GitFailure::OutputTooLarge
            } else {
                GitFailure::Command
            };
            termination_error = Some(reason);
            // Leave all reap/kill behavior to `finish_git_process`, including
            // this cancellation/overflow path. In particular, do not turn a
            // wait error into `None` here and then accidentally lose the
            // cleanup diagnosis.
            break None;
        }
        match child.try_wait() {
            Ok(Some(result)) => {
                break Some(result);
            }
            Ok(None) => thread::sleep(Duration::from_millis(5)),
            Err(_) => {
                termination_error = Some(GitFailure::Command);
                break None;
            }
        }
    };
    finish_git_process(child, reader, status.is_none(), termination_error)
}

#[cfg(unix)]
fn reap_child(
    child: &mut std::process::Child,
    terminate: bool,
) -> Result<std::process::ExitStatus, GitFailure> {
    let kill_error = if terminate { child.kill().err() } else { None };
    let wait_result = child.wait();
    if kill_error.is_some_and(|error| error.kind() != io::ErrorKind::NotFound) {
        return Err(GitFailure::Cleanup);
    }
    wait_result.map_err(|_| GitFailure::Cleanup)
}

#[cfg(unix)]
fn finish_git_process(
    mut child: std::process::Child,
    reader: thread::JoinHandle<(io::Result<()>, Vec<u8>)>,
    terminate: bool,
    original_error: Option<GitFailure>,
) -> Result<(std::process::ExitStatus, Vec<u8>), GitFailure> {
    let child_cleanup = reap_child(&mut child, terminate);
    let reader_result = reader.join().map_err(|_| GitFailure::Cleanup);
    let status = child_cleanup?;
    let (read_result, bytes) = reader_result?;
    if let Some(error) = original_error {
        return Err(error);
    }
    read_result.map_err(|_| GitFailure::Command)?;
    if bytes.len() > MAX_GIT_OUTPUT_BYTES {
        return Err(GitFailure::OutputTooLarge);
    }
    Ok((status, bytes))
}

fn read_nofollow(path: &Path, root: &Path) -> io::Result<Vec<u8>> {
    #[cfg(unix)]
    return read_nofollow_unix(path, root);
    #[cfg(not(unix))]
    {
        let _ = (path, root);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "live watching requires race-safe descriptor traversal",
        ))
    }
}

#[cfg(unix)]
fn read_nofollow_unix(path: &Path, root: &Path) -> io::Result<Vec<u8>> {
    use std::{
        os::fd::{AsRawFd, FromRawFd},
        os::unix::ffi::OsStrExt,
        path::Component,
    };

    let relative = path.strip_prefix(root).map_err(|_| {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            "watch path escapes workspace",
        )
    })?;
    let components: Vec<_> = relative.components().collect();
    if components.is_empty()
        || components
            .iter()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "watch path is not a normal workspace path",
        ));
    }
    let root_name = std::ffi::CString::new(root.as_os_str().as_bytes()).map_err(|_| {
        io::Error::new(io::ErrorKind::InvalidInput, "workspace path contains a NUL")
    })?;
    let directory_flags = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW;
    let root_fd = unsafe { libc::open(root_name.as_ptr(), directory_flags) };
    if root_fd < 0 {
        return Err(io::Error::last_os_error());
    }
    let mut directory = unsafe { fs::File::from_raw_fd(root_fd) };
    for component in &components[..components.len() - 1] {
        let name = std::ffi::CString::new(component.as_os_str().as_bytes()).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidInput, "watch path contains a NUL")
        })?;
        let fd = unsafe { libc::openat(directory.as_raw_fd(), name.as_ptr(), directory_flags) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        directory = unsafe { fs::File::from_raw_fd(fd) };
    }
    let name = std::ffi::CString::new(
        components
            .last()
            .expect("non-empty components")
            .as_os_str()
            .as_bytes(),
    )
    .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "watch path contains a NUL"))?;
    let file_flags = libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW;
    let file_fd = unsafe { libc::openat(directory.as_raw_fd(), name.as_ptr(), file_flags) };
    if file_fd < 0 {
        return Err(io::Error::last_os_error());
    }
    let mut file = unsafe { fs::File::from_raw_fd(file_fd) };
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > MAX_FILE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "file is not a watchable text file",
        ));
    }
    let mut bytes = Vec::with_capacity((MAX_FILE_BYTES as usize).min(64 * 1024));
    Read::by_ref(&mut file)
        .take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "watch file exceeds size limit",
        ));
    }
    Ok(bytes)
}

/// The Rust-side copy of `ingester.secretscan`: match classes, never matched
/// values. Extra vault values are inspected while the vault lock is held and
/// are not copied into watcher state.
fn find_secret(text: &str, vault: &Option<SharedVault>) -> bool {
    if text.contains("-----BEGIN ")
        && [
            "PRIVATE KEY",
            "CERTIFICATE",
            "CERTIFICATE REQUEST",
            "ENCRYPTED PRIVATE KEY",
            "OPENSSH PRIVATE KEY",
            "EC PRIVATE KEY",
            "DSA PRIVATE KEY",
            "PGP PRIVATE KEY BLOCK",
        ]
        .iter()
        .any(|kind| text.contains(kind))
    {
        return true;
    }
    if contains_prefixed_token(text, "AKIA", 16, |byte| {
        byte.is_ascii_uppercase() || byte.is_ascii_digit()
    }) || contains_prefixed_token(text, "ghp_", 36, |byte| byte.is_ascii_alphanumeric())
        || contains_prefixed_token(text, "gho_", 36, |byte| byte.is_ascii_alphanumeric())
        || contains_prefixed_token(text, "ghu_", 36, |byte| byte.is_ascii_alphanumeric())
        || contains_prefixed_token(text, "ghs_", 36, |byte| byte.is_ascii_alphanumeric())
        || contains_prefixed_token(text, "ghr_", 36, |byte| byte.is_ascii_alphanumeric())
        || contains_prefixed_token(text, "github_pat_", 22, |byte| {
            byte.is_ascii_alphanumeric() || byte == b'_'
        })
        || contains_slack_token(text)
        || contains_assignment(text)
    {
        return true;
    }
    let Some(vault) = vault else { return false };
    let vault = crate::vault::lock_shared(vault);
    vault.list().iter().any(|name| {
        vault
            .get(name)
            .map(|secret| !secret.is_empty() && text.contains(secret.expose()))
            .unwrap_or(false)
    })
}

fn contains_prefixed_token(
    text: &str,
    prefix: &str,
    minimum: usize,
    valid: fn(u8) -> bool,
) -> bool {
    let bytes = text.as_bytes();
    let mut start = 0;
    while let Some(offset) = text[start..].find(prefix) {
        let begin = start + offset;
        let end = bytes[begin + prefix.len()..]
            .iter()
            .position(|byte| !valid(*byte))
            .map_or(bytes.len(), |offset| begin + prefix.len() + offset);
        let before_ok = begin == 0 || !bytes[begin - 1].is_ascii_alphanumeric();
        if before_ok && end.saturating_sub(begin + prefix.len()) >= minimum {
            return true;
        }
        start = begin + prefix.len();
    }
    false
}

fn contains_slack_token(text: &str) -> bool {
    text.split_whitespace().any(|word| {
        let mut parts = word.split('-');
        let prefix = parts.next().unwrap_or_default();
        prefix.starts_with("xox") && parts.map(str::len).sum::<usize>() >= 10
    })
}

fn contains_assignment(text: &str) -> bool {
    let keys = [
        "SECRET",
        "TOKEN",
        "PASSWORD",
        "PASSPHRASE",
        "API_KEY",
        "APIKEY",
    ];
    for line in text.lines() {
        let upper = line.to_ascii_uppercase();
        for key in keys {
            let mut search_from = 0;
            while let Some(offset) = upper[search_from..].find(key) {
                let begin = search_from + offset;
                let end = begin + key.len();
                let before_ok = begin == 0
                    || !upper.as_bytes()[begin - 1].is_ascii_alphanumeric()
                        && upper.as_bytes()[begin - 1] != b'_';
                let after_ok = end == upper.len()
                    || !upper.as_bytes()[end].is_ascii_alphanumeric()
                        && upper.as_bytes()[end] != b'_';
                if before_ok && after_ok {
                    let mut suffix = &line[end..];
                    suffix = strip_optional_quote(suffix);
                    suffix = suffix.trim_start();
                    let Some(separator) = suffix
                        .strip_prefix('=')
                        .or_else(|| suffix.strip_prefix(':'))
                    else {
                        search_from = end;
                        continue;
                    };
                    let mut value = separator.trim_start();
                    value = strip_optional_quote(value);
                    let valid_prefix = value
                        .bytes()
                        .take_while(|byte| byte.is_ascii_alphanumeric() || b"+/=_-".contains(byte))
                        .count();
                    if valid_prefix >= 20 {
                        return true;
                    }
                }
                search_from = end;
            }
        }
    }
    false
}

fn strip_optional_quote(value: &str) -> &str {
    match value.as_bytes().first() {
        Some(b'\'' | b'"') => &value[1..],
        _ => value,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

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

    #[cfg(unix)]
    #[test]
    fn discovery_does_not_import_metadata_from_a_symlinked_git_entry() {
        let root = test_root("symlinked-git");
        let outside = root.with_extension("outside");
        let _ = fs::remove_dir_all(&outside);
        fs::create_dir_all(root.join("sub")).unwrap();
        fs::create_dir_all(&outside).unwrap();
        run_git(&outside, &["init", "-q"]);
        run_git(&outside, &["config", "user.email", "test@example.invalid"]);
        run_git(&outside, &["config", "user.name", "Mooshik Test"]);
        fs::write(outside.join("note.md"), "outside").unwrap();
        run_git(&outside, &["add", "note.md"]);
        run_git(
            &outside,
            &["commit", "-qm", "external metadata must stay external"],
        );
        std::os::unix::fs::symlink(outside.join(".git"), root.join("sub/.git")).unwrap();

        let snapshot = Snapshot::discover(&root);
        assert!(!snapshot.heads.contains_key(&root.join("sub")));
        assert!(snapshot.git_failures.is_empty());
        remove_test_root(&root);
        remove_test_root(&outside);
    }

    #[cfg(unix)]
    #[test]
    fn git_environment_cannot_redirect_repository_discovery() {
        let root = test_root("git-env-root");
        let outside = root.with_extension("git-env-outside");
        initialize_repo(&root, "workspace commit");
        initialize_repo(&outside, "outside commit");
        let expected = git_output(&root, &["rev-parse", "HEAD"]);
        let outside_head = git_output(&outside, &["rev-parse", "HEAD"]);
        assert_ne!(expected, outside_head);

        let secure = SecureGitRepo::open(&root).unwrap();
        secure.make_inheritable().unwrap();
        let mut command =
            secure.command_with_git_dir(&["rev-parse", "HEAD"], Some(&outside.join(".git")));
        let output = command.output().unwrap();
        assert!(output.status.success());
        let actual = String::from_utf8(output.stdout).unwrap();
        assert_eq!(actual, expected);
        assert_ne!(actual, outside_head);
        remove_test_root(&root);
        remove_test_root(&outside);
    }

    #[cfg(unix)]
    #[test]
    fn secure_git_context_survives_repository_rename_and_replacement() {
        let root = test_root("git-rename");
        let moved = root.with_extension("git-rename-moved");
        initialize_repo(&root, "stable commit");
        let expected = git_output(&root, &["rev-parse", "HEAD"]);
        let secure = SecureGitRepo::open(&root).unwrap();

        fs::rename(&root, &moved).unwrap();
        initialize_repo(&root, "replacement commit");
        let cancelled = AtomicBool::new(false);
        let actual = git_with_cancel(&secure, &["rev-parse", "HEAD"], &cancelled).unwrap();
        assert_eq!(actual.trim(), expected.trim());

        drop(secure);
        remove_test_root(&root);
        remove_test_root(&moved);
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
    fn failed_git_transition_does_not_advance_the_previous_snapshot() {
        let repo = test_root("missing-git").join("repo");
        let previous = Snapshot {
            heads: [(repo.clone(), GitHead::Commit("old-head".to_owned()))]
                .into_iter()
                .collect(),
            ..Snapshot::default()
        };
        let current = Snapshot {
            heads: [(repo.clone(), GitHead::Commit("new-head".to_owned()))]
                .into_iter()
                .collect(),
            ..Snapshot::default()
        };
        let mut pending = BTreeMap::new();
        let mut changed = None;

        assert!(!collect_changes(
            &previous,
            &current,
            &mut pending,
            &mut changed
        ));
        assert!(pending.is_empty());
        assert_eq!(
            previous.heads.get(&repo),
            Some(&GitHead::Commit("old-head".to_owned()))
        );
        remove_test_root(repo.parent().unwrap());
    }

    #[test]
    fn failed_git_transition_does_not_repeat_successful_file_events() {
        let root = test_root("git-retry-file");
        let repo = root.join("missing-repo");
        let path = root.join("outside.md");
        let modified = SystemTime::UNIX_EPOCH + Duration::from_secs(1);
        let previous = Snapshot {
            files: [(path.clone(), FileState { modified, len: 1 })]
                .into_iter()
                .collect(),
            heads: [(repo.clone(), GitHead::Commit("old-head".to_owned()))]
                .into_iter()
                .collect(),
            ..Snapshot::default()
        };
        let current = Snapshot {
            files: [(path.clone(), FileState { modified, len: 2 })]
                .into_iter()
                .collect(),
            heads: [(repo.clone(), GitHead::Commit("new-head".to_owned()))]
                .into_iter()
                .collect(),
            ..Snapshot::default()
        };
        let never_cancelled = AtomicBool::new(false);
        let mut pending = BTreeMap::new();
        let mut changed = None;
        let first = collect_changes_with_cancel(
            &previous,
            &current,
            &mut pending,
            &mut changed,
            &never_cancelled,
        );
        assert!(first.retry);
        assert!(first.snapshot.files.contains_key(&path));
        assert_eq!(pending.len(), 1);

        let mut changed_again = None;
        let second = collect_changes_with_cancel(
            &first.snapshot,
            &current,
            &mut pending,
            &mut changed_again,
            &never_cancelled,
        );
        assert!(second.retry);
        assert!(!second.changed);
        assert_eq!(pending.len(), 1);
        remove_test_root(&root);
    }

    #[cfg(unix)]
    #[test]
    fn unborn_nested_repository_does_not_block_files_or_its_first_commit() {
        let root = test_root("unborn-repo");
        let repo = root.join("nested");
        fs::create_dir_all(&repo).unwrap();
        run_git(&repo, &["init", "-q"]);
        run_git(&repo, &["config", "user.email", "test@example.invalid"]);
        run_git(&repo, &["config", "user.name", "Mooshik Test"]);

        let before = Snapshot::discover(&root);
        assert_eq!(before.heads.get(&repo), Some(&GitHead::Unborn));
        assert!(before.git_failures.is_empty());
        fs::write(root.join("outside.md"), "changed outside the repo").unwrap();
        fs::write(repo.join("note.md"), "first commit").unwrap();
        run_git(&repo, &["add", "note.md"]);
        run_git_with_date(
            &repo,
            &["commit", "-qm", "first nested commit"],
            "2022-03-04T05:06:07+0000",
        );
        let after = Snapshot::discover(&root);
        let mut pending = BTreeMap::new();
        let mut changed = None;
        assert!(collect_changes(&before, &after, &mut pending, &mut changed));
        assert!(pending
            .values()
            .any(|event| matches!(event, Pending::Commit(_))));
        assert!(pending
            .keys()
            .any(|key| key == &format!("file:{}", root.join("outside.md").display())));
        remove_test_root(&root);
    }

    #[cfg(unix)]
    #[test]
    fn corrupt_head_is_retried_instead_of_classified_as_unborn() {
        let root = test_root("corrupt-head");
        run_git(&root, &["init", "-q"]);
        fs::write(root.join(".git/HEAD"), "ref: refs/heads/broken\n").unwrap();
        fs::create_dir_all(root.join(".git/refs/heads")).unwrap();
        fs::write(root.join(".git/refs/heads/broken"), "not-an-object-id\n").unwrap();

        let snapshot = Snapshot::discover(&root);
        assert_eq!(snapshot.heads.get(&root), Some(&GitHead::Unknown));
        assert_eq!(snapshot.git_failures, vec![root.clone()]);
        remove_test_root(&root);
    }

    #[cfg(unix)]
    #[test]
    fn failed_initial_git_discovery_replays_history_after_recovery() {
        let root = test_root("git-failure-recovery");
        initialize_repo(&root, "commit missed during discovery failure");
        let original_head = fs::read_to_string(root.join(".git/HEAD")).unwrap();
        fs::write(root.join(".git/HEAD"), "ref: refs/heads/broken\n").unwrap();
        fs::write(root.join(".git/refs/heads/broken"), "not-an-object-id\n").unwrap();

        let failed = Snapshot::discover(&root);
        assert_eq!(failed.heads.get(&root), Some(&GitHead::Unknown));
        assert_eq!(failed.git_failures, vec![root.clone()]);

        fs::write(root.join(".git/HEAD"), original_head).unwrap();
        fs::remove_file(root.join(".git/refs/heads/broken")).unwrap();
        let recovered = Snapshot::discover(&root);
        let mut pending = BTreeMap::new();
        let mut changed = None;
        let result = collect_changes_with_cancel(
            &failed,
            &recovered,
            &mut pending,
            &mut changed,
            &AtomicBool::new(false),
        );
        assert!(!result.retry);
        assert!(
            pending.values().any(|event| {
                matches!(event, Pending::Commit(commit) if commit.message.trim_end() == "commit missed during discovery failure")
            }),
            "pending={pending:?} failed={:?} recovered={:?} result_heads={:?}",
            failed.heads,
            recovered.heads,
            result.snapshot.heads
        );
        remove_test_root(&root);
    }

    #[test]
    fn late_git_failure_without_a_previous_head_keeps_unknown() {
        let repo = PathBuf::from("/workspace/late");
        let previous = Snapshot::default();
        let current = Snapshot {
            heads: [(repo.clone(), GitHead::Unknown)].into_iter().collect(),
            git_failures: vec![repo.clone()],
            ..Snapshot::default()
        };
        let mut pending = BTreeMap::new();
        let mut changed = None;
        let result = collect_changes_with_cancel(
            &previous,
            &current,
            &mut pending,
            &mut changed,
            &AtomicBool::new(false),
        );
        assert!(result.retry);
        assert!(pending.is_empty());
        assert_eq!(
            result.snapshot.heads.get(&repo),
            Some(&GitHead::Unknown),
            "removing the head would baseline history that existed during the failure"
        );
    }

    #[test]
    fn git_failure_list_without_a_head_entry_still_records_unknown() {
        let repo = PathBuf::from("/workspace/late");
        let previous = Snapshot::default();
        let current = Snapshot {
            git_failures: vec![repo.clone()],
            ..Snapshot::default()
        };
        let mut pending = BTreeMap::new();
        let mut changed = None;
        let result = collect_changes_with_cancel(
            &previous,
            &current,
            &mut pending,
            &mut changed,
            &AtomicBool::new(false),
        );
        assert_eq!(result.snapshot.heads.get(&repo), Some(&GitHead::Unknown));
    }

    #[test]
    fn cancelled_collect_keeps_unknown_for_a_late_failed_repository() {
        let repo = PathBuf::from("/workspace/late");
        let previous = Snapshot::default();
        let current = Snapshot {
            heads: [(repo.clone(), GitHead::Unknown)].into_iter().collect(),
            git_failures: vec![repo.clone()],
            ..Snapshot::default()
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
        assert!(!result.changed);
        assert_eq!(
            result.snapshot.heads.get(&repo),
            Some(&GitHead::Unknown),
            "cancel must not drop a failed discovery that already ran"
        );
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
        let result = collect_changes_with_cancel(
            &previous,
            &current,
            &mut pending,
            &mut changed,
            &cancelled,
        );
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
        let impl_src = include_str!("watcher.rs")
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
    fn failed_git_discovery_does_not_baseline_away_an_unknown_head() {
        let production = include_str!("watcher.rs")
            .split("fn collect_changes_with_cancel")
            .nth(1)
            .expect("collect_changes_with_cancel")
            .split("fn snapshot_retaining_failed_discoveries(")
            .next()
            .unwrap();
        let failures = production
            .split("for repo in &current.git_failures")
            .nth(1)
            .expect("git_failures loop")
            .split("if cancelled.load")
            .next()
            .unwrap();
        assert!(
            failures.contains("GitHead::Unknown"),
            "a late failed repository must keep Unknown"
        );
        assert!(
            !failures.contains("heads.remove"),
            "next.heads.remove(repo) baselines away history that existed during the failure"
        );
        let cancel_returns: Vec<&str> = production
            .split("if cancelled.load(Ordering::Acquire)")
            .skip(1)
            .map(|rest| {
                rest.split("return ChangeResult")
                    .nth(1)
                    .and_then(|body| body.split("};").next())
                    .expect("cancel returns ChangeResult")
            })
            .collect();
        assert_eq!(
            cancel_returns.len(),
            2,
            "collect_changes_with_cancel must pin both the first-load cancel and the file-walk cancel"
        );
        for (i, block) in cancel_returns.iter().enumerate() {
            assert!(
                block.contains("snapshot_retaining_failed_discoveries(previous, current)"),
                "cancel return {i} must retain Unknown markers from git_failures, not return previous.clone()"
            );
            assert!(
                !block.contains("previous.clone()"),
                "cancel return {i} must not snapshot previous.clone()"
            );
        }
    }

    #[test]
    fn live_watching_fails_closed_at_tui_startup() {
        let start = include_str!("watcher.rs")
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
        let live = include_str!("tui_cmd.rs")
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
        let readme = include_str!("../../README.md");
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
            "../../README.md"
        )));
    }

    #[cfg(unix)]
    #[test]
    fn late_failed_repository_keeps_unknown_marker_for_first_commit_recovery() {
        let root = test_root("late-git-failure");
        let repo = root.join("nested");
        let before = Snapshot::discover(&root);
        assert!(!before.heads.contains_key(&repo));

        fs::create_dir_all(&repo).unwrap();
        run_git(&repo, &["init", "-q"]);
        run_git(&repo, &["config", "user.email", "test@example.invalid"]);
        run_git(&repo, &["config", "user.name", "Mooshik Test"]);
        let original_head = fs::read_to_string(repo.join(".git/HEAD")).unwrap();
        fs::write(repo.join(".git/HEAD"), "ref: refs/heads/broken\n").unwrap();
        fs::write(repo.join(".git/refs/heads/broken"), "not-an-object-id\n").unwrap();

        let failed = Snapshot::discover(&root);
        let mut pending = BTreeMap::new();
        let mut changed = None;
        let failed_result = collect_changes_with_cancel(
            &before,
            &failed,
            &mut pending,
            &mut changed,
            &AtomicBool::new(false),
        );
        assert!(failed_result.retry);
        assert!(pending.is_empty());
        assert_eq!(
            failed_result.snapshot.heads.get(&repo),
            Some(&GitHead::Unknown),
            "removing the head would baseline history that existed during the failure"
        );

        fs::write(repo.join(".git/HEAD"), original_head).unwrap();
        fs::remove_file(repo.join(".git/refs/heads/broken")).unwrap();
        fs::write(repo.join("first.md"), "first commit after recovery").unwrap();
        run_git(&repo, &["add", "first.md"]);
        run_git(&repo, &["commit", "-qm", "first commit after recovery"]);
        let recovered = Snapshot::discover(&root);
        let mut recovered_pending = BTreeMap::new();
        let mut recovered_changed = None;
        let recovered_result = collect_changes_with_cancel(
            &failed_result.snapshot,
            &recovered,
            &mut recovered_pending,
            &mut recovered_changed,
            &AtomicBool::new(false),
        );
        assert!(!recovered_result.retry);
        assert!(
            recovered_pending.values().any(|event| {
                matches!(event, Pending::Commit(commit) if commit.message.trim_end() == "first commit after recovery")
            }),
            "recovery must enqueue the first commit after the repo becomes readable; pending={recovered_pending:?}"
        );
        remove_test_root(&root);
    }

    #[cfg(unix)]
    #[test]
    fn cancelled_late_failed_repository_replays_first_commit_after_recovery() {
        let root = test_root("late-git-failure-cancel");
        let repo = root.join("nested");
        let before = Snapshot::discover(&root);

        fs::create_dir_all(&repo).unwrap();
        run_git(&repo, &["init", "-q"]);
        run_git(&repo, &["config", "user.email", "test@example.invalid"]);
        run_git(&repo, &["config", "user.name", "Mooshik Test"]);
        let original_head = fs::read_to_string(repo.join(".git/HEAD")).unwrap();
        fs::write(repo.join(".git/HEAD"), "ref: refs/heads/broken\n").unwrap();
        fs::write(repo.join(".git/refs/heads/broken"), "not-an-object-id\n").unwrap();

        let failed = Snapshot::discover(&root);
        let mut pending = BTreeMap::new();
        let mut changed = None;
        let cancelled_result = collect_changes_with_cancel(
            &before,
            &failed,
            &mut pending,
            &mut changed,
            &AtomicBool::new(true),
        );
        assert_eq!(
            cancelled_result.snapshot.heads.get(&repo),
            Some(&GitHead::Unknown)
        );

        fs::write(repo.join(".git/HEAD"), original_head).unwrap();
        fs::remove_file(repo.join(".git/refs/heads/broken")).unwrap();
        fs::write(
            repo.join("first.md"),
            "first commit after cancelled failure",
        )
        .unwrap();
        run_git(&repo, &["add", "first.md"]);
        run_git(
            &repo,
            &["commit", "-qm", "first commit after cancelled failure"],
        );
        let recovered = Snapshot::discover(&root);
        let mut recovered_pending = BTreeMap::new();
        let mut recovered_changed = None;
        let recovered_result = collect_changes_with_cancel(
            &cancelled_result.snapshot,
            &recovered,
            &mut recovered_pending,
            &mut recovered_changed,
            &AtomicBool::new(false),
        );
        assert!(!recovered_result.retry);
        assert!(recovered_pending.values().any(|event| {
            matches!(event, Pending::Commit(commit) if commit.message.trim_end() == "first commit after cancelled failure")
        }));
        remove_test_root(&root);
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
    fn commit_metadata_parser_keeps_author_time_and_no_diff() {
        let repo = PathBuf::from("/workspace/project");
        let sha = "a".repeat(40);
        let metadata =
            parse_commit_headers(&format!("{sha}\02024-01-02T03:04:05+00:00\0")).unwrap();
        let content = b"tree abc\nauthor Test <test@example.invalid> 0 +0000\ncommitter Test <test@example.invalid> 0 +0000\n\nfix parser\0\x1e\xff";
        let mut object = format!("{sha} commit {}\n", content.len()).into_bytes();
        object.extend_from_slice(content);
        object.push(b'\n');
        let commits = parse_commit_messages(&repo, &metadata, &object).unwrap();
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].sha, sha);
        assert_eq!(
            commits[0].author_time.to_rfc3339(),
            "2024-01-02T03:04:05+00:00"
        );
        assert_eq!(commits[0].message, "fix parser\0\x1e�");
        assert!(!format!("{:?}", commits[0]).contains("diff"));
    }

    #[test]
    fn git_commit_parser_caps_one_poll() {
        let output: String = (0..=MAX_GIT_COMMITS_PER_POLL)
            .map(|index| format!("{index:0>40}\02024-01-02T03:04:05+00:00\0"))
            .collect();
        assert!(matches!(
            parse_commit_headers(&output),
            Err(GitFailure::TooManyCommits)
        ));
    }

    #[test]
    fn malformed_git_metadata_is_retried_instead_of_skipped() {
        let sha = "b".repeat(40);
        assert!(matches!(
            parse_commit_headers(&format!("{sha}\0not-a-date\0")),
            Err(GitFailure::MalformedOutput)
        ));
    }

    #[test]
    fn commit_message_framing_preserves_the_record_separator_byte() {
        let repo = PathBuf::from("/workspace/project");
        let sha = "c".repeat(40);
        let metadata = vec![(
            sha.clone(),
            DateTime::parse_from_rfc3339("2024-01-02T03:04:05+00:00")
                .unwrap()
                .with_timezone(&Utc),
        )];
        let content = b"tree abc\n\na message\x1e still one message";
        let mut object = format!("{sha} commit {}\n", content.len()).into_bytes();
        object.extend_from_slice(content);
        object.push(b'\n');
        let commits = parse_commit_messages(&repo, &metadata, &object).unwrap();
        assert_eq!(commits[0].message, "a message\x1e still one message");
    }

    #[test]
    fn system_time_to_utc_preserves_pre_epoch_mtimes() {
        let time = SystemTime::UNIX_EPOCH - Duration::new(1, 500_000_000);
        assert_eq!(
            system_time_to_utc(time).unwrap().to_rfc3339(),
            "1969-12-31T23:59:58.500+00:00"
        );
    }

    #[cfg(unix)]
    #[test]
    fn git_poll_emits_new_commit_metadata_with_author_time() {
        let root = test_root("git");
        run_git(&root, &["init", "-q"]);
        run_git(&root, &["config", "user.email", "test@example.invalid"]);
        run_git(&root, &["config", "user.name", "Mooshik Test"]);
        fs::write(root.join("note.md"), "first").unwrap();
        run_git(&root, &["add", "note.md"]);
        run_git_with_date(
            &root,
            &["commit", "-qm", "initial"],
            "2020-01-02T03:04:05+0000",
        );
        let before = Snapshot::discover(&root);
        assert!(!before.files.contains_key(&root.join("note.md")));
        fs::write(root.join("note.md"), "second").unwrap();
        run_git(&root, &["add", "note.md"]);
        run_git_with_date(
            &root,
            &["commit", "-qm", "second change"],
            "2021-02-03T04:05:06+0000",
        );
        let after = Snapshot::discover(&root);
        let mut pending = BTreeMap::new();
        let mut changed = None;
        collect_changes(&before, &after, &mut pending, &mut changed);
        let event = pending
            .values()
            .find_map(|event| match event {
                Pending::Commit(commit) => Some(commit),
                Pending::File { .. } => None,
            })
            .unwrap();
        assert_eq!(event.author_time.to_rfc3339(), "2021-02-03T04:05:06+00:00");
        assert!(
            event_to_concept(&Pending::Commit(event.clone()), &None, &root,)
                .unwrap()
                .unwrap()
                .0
                .contains("second change")
        );
        remove_test_root(&root);
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

    fn test_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("mooshik-m12d-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn remove_test_root(root: &Path) {
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    fn initialize_repo(root: &Path, message: &str) {
        fs::create_dir_all(root).unwrap();
        run_git(root, &["init", "-q"]);
        run_git(root, &["config", "user.email", "test@example.invalid"]);
        run_git(root, &["config", "user.name", "Mooshik Test"]);
        fs::write(root.join("note.md"), message).unwrap();
        run_git(root, &["add", "note.md"]);
        run_git(root, &["commit", "-qm", message]);
    }

    #[cfg(unix)]
    fn git_output(root: &Path, args: &[&str]) -> String {
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
    fn run_git(root: &Path, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git command failed: {args:?}");
    }

    #[cfg(unix)]
    fn run_git_with_date(root: &Path, args: &[&str], date: &str) {
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
}
