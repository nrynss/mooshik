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
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant, SystemTime},
};

use chrono::{DateTime, Utc};
use lambo::{graph::derive::ParentOf, ConceptType, Memory};
use tokio::{sync::oneshot, task::JoinHandle};

use crate::{memory::WriteLane, text, vault::SharedVault};

mod git;
#[cfg(test)]
mod git_tests;
#[cfg(test)]
mod tests;

pub(crate) use git::*;

const POLL_INTERVAL: Duration = Duration::from_millis(100);
const QUIET_POLL_INTERVAL: Duration = Duration::from_millis(250);
const DEBOUNCE: Duration = Duration::from_millis(250);
const MAX_CONCEPTS_PER_DERIVE: usize = 64;
pub(crate) const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;
pub(crate) const MAX_GIT_OUTPUT_BYTES: usize = 2 * 1024 * 1024;
pub(crate) const MAX_GIT_COMMITS_PER_POLL: usize = 256;
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
pub(crate) struct Commit {
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
pub(crate) enum GitHead {
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
