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
//! Discovery runs in one `spawn_blocking` task at a time. Quiet workspaces use
//! an adaptive 100--250 ms poll interval, trading a bounded amount of latency
//! for keeping recursive walks and git subprocesses off the TUI runtime.

use std::{
    collections::BTreeMap,
    fs,
    future::Future,
    io::{self, Read},
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::{Duration, Instant, SystemTime},
};

use chrono::{DateTime, Utc};
use lambo::{graph::derive::ParentOf, ConceptType, Memory};
use tokio::{sync::oneshot, task::JoinHandle};

use crate::{memory::WriteLane, text, vault::SharedVault};

const POLL_INTERVAL: Duration = Duration::from_millis(100);
const QUIET_POLL_INTERVAL: Duration = Duration::from_millis(250);
const DEBOUNCE: Duration = Duration::from_millis(250);
const MAX_CONCEPTS_PER_DERIVE: usize = 64;
const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;
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
    cancel: Option<oneshot::Sender<()>>,
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
        let (cancel, cancelled) = oneshot::channel();
        let task = handle.spawn(run(memory, writes, root, agent, vault, cancelled));
        Ok(Self {
            cancel: Some(cancel),
            task,
        })
    }

    pub(crate) async fn stop(mut self) -> Result<(), WatchError> {
        if let Some(cancel) = self.cancel.take() {
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

#[derive(Default, Eq, PartialEq)]
struct Snapshot {
    files: BTreeMap<PathBuf, FileState>,
    heads: BTreeMap<PathBuf, String>,
    git_failures: Vec<PathBuf>,
}

impl Snapshot {
    fn discover(root: &Path) -> Self {
        let mut snapshot = Self::default();
        let mut dirs = Vec::new();
        if is_git_marker(root) {
            match git_head(root) {
                Ok(head) => {
                    snapshot.heads.insert(root.to_path_buf(), head);
                }
                Err(()) => snapshot.git_failures.push(root.to_path_buf()),
            }
        } else {
            dirs.push(root.to_path_buf());
        }
        while let Some(dir) = dirs.pop() {
            let entries = match fs::read_dir(&dir) {
                Ok(entries) => entries,
                Err(_) => continue,
            };
            for entry in entries.flatten() {
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
                        match git_head(&path) {
                            Ok(head) => {
                                snapshot.heads.insert(path.clone(), head);
                            }
                            Err(()) => snapshot.git_failures.push(path.clone()),
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

async fn run(
    memory: Arc<Memory>,
    writes: WriteLane,
    root: PathBuf,
    agent: String,
    vault: Option<SharedVault>,
    mut cancelled: oneshot::Receiver<()>,
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
        // first is still running. If the pane closes during a walk, dropping
        // this handle detaches the blocking task; it has no Memory or write
        // lane access and therefore cannot write after shutdown.
        let discovery = tokio::task::spawn_blocking({
            let root = root.clone();
            move || Snapshot::discover(&root)
        });
        let current = tokio::select! {
            _ = &mut cancelled => return Ok(()),
            result = discovery => result.map_err(|_| WatchError::TaskFailed)?,
        };

        if previous.is_none() {
            if current.git_failures.is_empty() {
                previous = Some(current);
                poll_interval = QUIET_POLL_INTERVAL;
            } else {
                // Do not baseline a repository whose head could not be read.
                // The next discovery must succeed before any state advances.
                poll_interval = POLL_INTERVAL;
            }
            continue;
        }

        let old = previous.as_ref().expect("initialized above");
        let valid = collect_changes(old, &current, &mut pending, &mut changed_at);
        if valid {
            let changed = old != &current;
            previous = Some(current);
            poll_interval = if changed {
                POLL_INTERVAL
            } else {
                (poll_interval * 2).min(QUIET_POLL_INTERVAL)
            };
        } else {
            // A failed git head/log read must be retried against the old
            // snapshot, otherwise a commit can disappear between polls.
            poll_interval = POLL_INTERVAL;
        }

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

fn collect_changes(
    previous: &Snapshot,
    current: &Snapshot,
    pending: &mut BTreeMap<String, Pending>,
    changed_at: &mut Option<Instant>,
) -> bool {
    if !current.git_failures.is_empty() {
        return false;
    }
    for (path, state) in &current.files {
        if previous.files.get(path) != Some(state) {
            let Some(event_time) = system_time_to_utc(state.modified) else {
                continue;
            };
            let key = format!("file:{}", path.display());
            pending.insert(
                key,
                Pending::File {
                    path: path.clone(),
                    event_time,
                },
            );
            *changed_at = Some(Instant::now());
        }
    }
    for (repo, head) in &current.heads {
        let Some(previous_head) = previous.heads.get(repo) else {
            continue;
        };
        if previous_head == head {
            continue;
        }
        let Ok(commits) = git_commits_between(repo, previous_head, head) else {
            return false;
        };
        for commit in commits {
            let key = format!("git:{}#{}", repo.display(), commit.sha);
            pending.insert(key, Pending::Commit(commit));
            *changed_at = Some(Instant::now());
        }
    }
    // A newly-created repository is baselined by `Snapshot::discover`, so its
    // existing history is never replayed into the open pane.
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
    time.duration_since(SystemTime::UNIX_EPOCH)
        .ok()
        .and_then(|duration| {
            DateTime::<Utc>::from_timestamp(duration.as_secs() as i64, duration.subsec_nanos())
        })
}

fn is_git_marker(path: &Path) -> bool {
    fs::symlink_metadata(path.join(".git"))
        .map(|metadata| {
            let file_type = metadata.file_type();
            !file_type.is_symlink() && (file_type.is_dir() || file_type.is_file())
        })
        .unwrap_or(false)
}

fn git_head(repo: &Path) -> Result<String, ()> {
    git(repo, &["rev-parse", "--verify", "HEAD"])
        .map(|output| output.trim().to_owned())
        .ok_or(())
}

fn git_commits_between(repo: &Path, old: &str, new: &str) -> Result<Vec<Commit>, ()> {
    let range = format!("{old}..{new}");
    let output = git(
        repo,
        &["log", "--no-patch", "--format=%H%x00%aI%x00%B%x1e", &range],
    )
    .ok_or(())?;
    Ok(parse_commits(repo, &output))
}

fn parse_commits(repo: &Path, output: &str) -> Vec<Commit> {
    output
        .split('\x1e')
        .filter_map(|record| {
            let mut fields = record.trim().splitn(3, '\0');
            let sha = fields.next()?.trim();
            let date = fields.next()?.trim();
            let message = fields.next()?.trim();
            let author_time = DateTime::parse_from_rfc3339(date).ok()?.with_timezone(&Utc);
            Some(Commit {
                repo: repo.to_path_buf(),
                sha: sha.to_owned(),
                author_time,
                message: message.to_owned(),
            })
        })
        .collect()
}

fn git(repo: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

fn read_nofollow(path: &Path, root: &Path) -> io::Result<Vec<u8>> {
    // Canonicalizing before opening rejects symlinked parents and keeps the
    // read within the canonical workspace. On Unix, O_NOFOLLOW and inode
    // checks below close the final-component and common parent-swap races.
    let canonical = fs::canonicalize(path)?;
    if !canonical.starts_with(root) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "watch path escapes workspace",
        ));
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        use std::os::unix::fs::OpenOptionsExt;
        let metadata = fs::symlink_metadata(path)?;
        if !metadata.file_type().is_file() || metadata.len() > MAX_FILE_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "file is not a watchable text file",
            ));
        }
        let mut file = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)?;
        let opened = file.metadata()?;
        let current = fs::symlink_metadata(path)?;
        if !current.file_type().is_file()
            || current.dev() != opened.dev()
            || current.ino() != opened.ino()
            || metadata.dev() != current.dev()
            || metadata.ino() != current.ino()
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "watch path changed while opening",
            ));
        }
        let mut bytes = Vec::with_capacity((MAX_FILE_BYTES as usize).min(64 * 1024));
        io::Read::by_ref(&mut file)
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
    #[cfg(not(unix))]
    {
        let metadata = fs::symlink_metadata(path)?;
        if metadata.file_type().is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "symbolic links are not watchable",
            ));
        }
        if !metadata.is_file() || metadata.len() > MAX_FILE_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "file is not a watchable text file",
            ));
        }
        let mut file = fs::File::open(path)?;
        let opened = file.metadata()?;
        let current = fs::symlink_metadata(path)?;
        if !current.is_file()
            || current.len() != opened.len()
            || current.modified().ok() != opened.modified().ok()
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "watch path changed while opening",
            ));
        }
        let mut bytes = Vec::with_capacity((MAX_FILE_BYTES as usize).min(64 * 1024));
        io::Read::by_ref(&mut file)
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
            heads: [(repo.clone(), "old-head".to_owned())]
                .into_iter()
                .collect(),
            ..Snapshot::default()
        };
        let current = Snapshot {
            heads: [(repo.clone(), "new-head".to_owned())]
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
        assert_eq!(previous.heads.get(&repo), Some(&"old-head".to_owned()));
        remove_test_root(repo.parent().unwrap());
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
        let commits = parse_commits(&repo, "abc\x002024-01-02T03:04:05+00:00\0fix parser\x1e");
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].sha, "abc");
        assert_eq!(
            commits[0].author_time.to_rfc3339(),
            "2024-01-02T03:04:05+00:00"
        );
        assert!(!format!("{:?}", commits[0]).contains("diff"));
    }

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
            cancel: Some(cancel),
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

    fn run_git(root: &Path, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git command failed: {args:?}");
    }

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
