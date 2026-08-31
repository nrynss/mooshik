use std::{
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};

#[cfg(unix)]
use std::os::fd::{AsRawFd, FromRawFd};

use chrono::{DateTime, Utc};

use super::{Commit, GitHead, MAX_FILE_BYTES, MAX_GIT_COMMITS_PER_POLL, MAX_GIT_OUTPUT_BYTES};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GitFailure {
    Command,
    Cleanup,
    MalformedOutput,
    OutputTooLarge,
    TooManyCommits,
}

#[cfg(unix)]
pub(crate) struct SecureGitRepo {
    repo_dir: fs::File,
    git_dir: fs::File,
}

#[cfg(unix)]
impl SecureGitRepo {
    pub(crate) fn open(path: &Path) -> Result<Self, GitFailure> {
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

    pub(crate) fn make_inheritable(&self) -> io::Result<()> {
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

    pub(crate) fn command(&self, args: &[&str]) -> Command {
        self.command_with_git_dir(args, None)
    }

    pub(crate) fn command_with_git_dir(
        &self,
        args: &[&str],
        git_dir_override: Option<&Path>,
    ) -> Command {
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
pub(crate) fn isolate_git_environment(command: &mut Command) {
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
pub(crate) fn fd_path(fd: i32) -> PathBuf {
    #[cfg(target_os = "linux")]
    let prefix = "/proc/self/fd";
    #[cfg(not(target_os = "linux"))]
    let prefix = "/dev/fd";
    PathBuf::from(prefix).join(fd.to_string())
}

#[cfg(unix)]
pub(crate) fn openat_no_follow(
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
pub(crate) fn open_directory_path(path: &Path) -> io::Result<fs::File> {
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
pub(crate) fn open_directory_relative(base: &fs::File, path: &Path) -> io::Result<fs::File> {
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
pub(crate) fn open_directory_components<'a, I>(
    mut directory: fs::File,
    components: I,
) -> io::Result<fs::File>
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
pub(crate) fn git_head_with_cancel(
    repo: &Path,
    cancelled: &AtomicBool,
) -> Result<GitHead, GitFailure> {
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
pub(crate) fn git_head_with_cancel(
    _repo: &Path,
    _cancelled: &AtomicBool,
) -> Result<GitHead, GitFailure> {
    Err(GitFailure::Command)
}

#[cfg(unix)]
pub(crate) fn git_commits_between_with_cancel(
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
pub(crate) fn git_commits_between_with_cancel(
    _repo: &Path,
    _old: Option<&str>,
    _new: &str,
    _cancelled: &AtomicBool,
) -> Result<Vec<Commit>, GitFailure> {
    Err(GitFailure::Command)
}

pub(crate) fn parse_commit_headers(
    output: &str,
) -> Result<Vec<(String, DateTime<Utc>)>, GitFailure> {
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

pub(crate) fn parse_commit_messages(
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

pub(crate) fn is_object_id(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(unix)]
pub(crate) fn git_with_cancel(
    repo: &SecureGitRepo,
    args: &[&str],
    cancelled: &AtomicBool,
) -> Result<String, GitFailure> {
    let bytes = git_bytes_with_cancel_input(repo, args, &[], cancelled)?;
    String::from_utf8(bytes).map_err(|_| GitFailure::MalformedOutput)
}

#[cfg(unix)]
pub(crate) fn git_bytes_with_cancel_input(
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
pub(crate) fn git_process_with_cancel_input(
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
pub(crate) fn reap_child(
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
pub(crate) fn finish_git_process(
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

pub(crate) fn read_nofollow(path: &Path, root: &Path) -> io::Result<Vec<u8>> {
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
pub(crate) fn read_nofollow_unix(path: &Path, root: &Path) -> io::Result<Vec<u8>> {
    use std::os::unix::ffi::OsStrExt;
    use std::path::Component;

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
