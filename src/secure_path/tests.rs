use super::*;
use std::fs;
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;

fn test_root() -> PathBuf {
    std::env::temp_dir().join(format!(
        "mooshik-secure-path-{}-{}",
        std::process::id(),
        std::thread::current().name().unwrap_or("test")
    ))
}

#[test]
fn creation_replacement_between_stage_mkdir_and_open_fails_closed() {
    let parent_path = test_root();
    let root = parent_path.join("created-home");
    let _ = fs::remove_dir_all(&parent_path);
    fs::create_dir(&parent_path).unwrap();
    let mut options = fs::OpenOptions::new();
    options.read(true);
    options.custom_flags(libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW);
    let parent = options.open(&parent_path).unwrap();
    let name = root.file_name().unwrap();
    let result = create_or_open_directory_at_with_hooks(
        parent.as_raw_fd(),
        name,
        0o700,
        |_| {
            // This is the adversarial scheduling point: an ordinary
            // replacement arrives before the staging directory is opened
            // and installed. The no-replace install must reject it.
            fs::create_dir(&root).map_err(io::Error::other)
        },
        |_| Ok(()),
    );
    assert!(matches!(
        result,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists
    ));
    assert!(root.is_dir());
    assert!(fs::read_dir(&root).unwrap().next().is_none());
    assert!(fs::read_dir(&parent_path)
        .unwrap()
        .filter_map(Result::ok)
        .any(|entry| entry
            .file_name()
            .to_string_lossy()
            .starts_with(".mooshik-stage-")));
    drop(parent);
    let _ = fs::remove_dir_all(&parent_path);
}

#[test]
fn staging_replacement_after_open_before_install_fails_closed() {
    let parent_path = test_root();
    let root = parent_path.join("created-home");
    let moved = parent_path.join("moved-stage");
    let _ = fs::remove_dir_all(&parent_path);
    fs::create_dir(&parent_path).unwrap();
    let mut options = fs::OpenOptions::new();
    options.read(true);
    options.custom_flags(libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW);
    let parent = options.open(&parent_path).unwrap();
    let name = root.file_name().unwrap();
    let result = create_or_open_directory_at_with_hooks(
        parent.as_raw_fd(),
        name,
        0o700,
        |_| Ok(()),
        |temporary| {
            // This is exactly the vulnerable window: the checked/opened
            // source is moved away and an ordinary replacement gets the
            // source pathname before renameat2.
            let temporary_path = parent_path.join(temporary);
            fs::rename(&temporary_path, &moved).map_err(io::Error::other)?;
            fs::create_dir(&temporary_path).map_err(io::Error::other)
        },
    );
    assert!(matches!(
        result,
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied
    ));
    assert!(root.is_dir());
    assert!(moved.is_dir());
    assert!(fs::read_dir(&root).unwrap().next().is_none());
    assert!(fs::read_dir(&moved).unwrap().next().is_none());
    drop(parent);
    let _ = fs::remove_dir_all(&parent_path);
}

#[test]
fn staging_directory_is_preserved_after_injected_failure() {
    let parent_path = test_root();
    let root = parent_path.join("created-home");
    let _ = fs::remove_dir_all(&parent_path);
    fs::create_dir(&parent_path).unwrap();
    let mut options = fs::OpenOptions::new();
    options.read(true);
    options.custom_flags(libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW);
    let parent = options.open(&parent_path).unwrap();
    let name = root.file_name().unwrap();
    let result = create_or_open_directory_at_with_hooks(
        parent.as_raw_fd(),
        name,
        0o700,
        |_| Err(io::Error::other("injected staging failure")),
        |_| Ok(()),
    );
    assert!(result.is_err());
    assert!(!root.exists());
    assert!(fs::read_dir(&parent_path)
        .unwrap()
        .filter_map(Result::ok)
        .any(|entry| entry
            .file_name()
            .to_string_lossy()
            .starts_with(".mooshik-stage-")));
    drop(parent);
    let _ = fs::remove_dir_all(&parent_path);
}

#[test]
fn staging_cleanup_does_not_remove_a_replacement_after_identity_check() {
    let parent_path = test_root();
    let root = parent_path.join("created-home");
    let moved = parent_path.join("moved-stage");
    let _ = fs::remove_dir_all(&parent_path);
    fs::create_dir(&parent_path).unwrap();
    let mut options = fs::OpenOptions::new();
    options.read(true);
    options.custom_flags(libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW);
    let parent = options.open(&parent_path).unwrap();
    let name = root.file_name().unwrap();
    let result = create_or_open_directory_at_with_preserve_hook(
        parent.as_raw_fd(),
        name,
        0o700,
        |_| Err(io::Error::other("injected staging failure")),
        |_| Ok(()),
        |temporary| {
            // Barrier after the identity check, before any pathname
            // removal. A same-UID swap here must not be unlinked.
            let temporary_path = parent_path.join(temporary);
            fs::rename(&temporary_path, &moved).map_err(io::Error::other)?;
            fs::create_dir(&temporary_path).map_err(io::Error::other)
        },
    );
    assert!(result.is_err());
    assert!(!root.exists());
    assert!(moved.is_dir());
    let staging = fs::read_dir(&parent_path)
        .unwrap()
        .filter_map(Result::ok)
        .find(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".mooshik-stage-")
        })
        .expect("replacement at the staging name must remain");
    assert!(staging.path().is_dir());
    assert!(fs::read_dir(staging.path()).unwrap().next().is_none());
    drop(parent);
    let _ = fs::remove_dir_all(&parent_path);
}

#[test]
fn staging_directory_replacement_between_snapshot_and_open_fails_closed() {
    let parent_path = test_root();
    let root = parent_path.join("created-home");
    let moved = parent_path.join("moved-stage");
    let _ = fs::remove_dir_all(&parent_path);
    fs::create_dir(&parent_path).unwrap();
    let mut options = fs::OpenOptions::new();
    options.read(true);
    options.custom_flags(libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW);
    let parent = options.open(&parent_path).unwrap();
    let name = root.file_name().unwrap();
    let result = create_or_open_directory_at_with_hooks(
        parent.as_raw_fd(),
        name,
        0o700,
        |temporary| {
            let temporary_path = parent_path.join(temporary);
            fs::rename(&temporary_path, &moved).map_err(io::Error::other)?;
            fs::create_dir(&temporary_path).map_err(io::Error::other)
        },
        |_| Ok(()),
    );
    assert!(matches!(
        result,
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied
    ));
    assert!(!root.exists());
    assert!(moved.is_dir());
    drop(parent);
    let _ = fs::remove_dir_all(&parent_path);
}

#[test]
fn creation_protocol_reports_success_only_for_our_mkdir() {
    let parent_path = test_root();
    let root = parent_path.join("existing-home");
    let _ = fs::remove_dir_all(&parent_path);
    fs::create_dir(&parent_path).unwrap();
    fs::create_dir(&root).unwrap();
    let mut options = fs::OpenOptions::new();
    options.read(true);
    options.custom_flags(libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW);
    let parent = options.open(&parent_path).unwrap();
    let name = root.file_name().unwrap();
    let (directory, created) =
        create_or_open_directory_at(parent.as_raw_fd(), name, 0o700).unwrap();
    assert!(!created);
    drop(directory);
    drop(parent);
    let _ = fs::remove_dir_all(&parent_path);
}

#[test]
fn simultaneous_lock_opens_of_an_absent_name_all_return_the_same_file() {
    // macOS hands every loser of a simultaneous non-exclusive `O_CREAT` open
    // ENOENT instead of the file the winner just created, so the threads have
    // to enter the open together for this to reach the platform behaviour at
    // all; a staggered start lets the first caller create the file and hides
    // it. Every caller must come away with a descriptor on one inode.
    use std::os::unix::fs::MetadataExt;
    const THREADS: usize = 4;
    const ROUNDS: usize = 64;
    let parent_path = test_root();
    let _ = fs::remove_dir_all(&parent_path);
    fs::create_dir_all(&parent_path).unwrap();
    let mut options = fs::OpenOptions::new();
    options.read(true);
    options.custom_flags(libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW);
    let parent = std::sync::Arc::new(options.open(&parent_path).unwrap());
    for round in 0..ROUNDS {
        let leaf = OsString::from(format!(".round-{round}.lock"));
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(THREADS));
        let openers: Vec<_> = (0..THREADS)
            .map(|_| {
                let parent = std::sync::Arc::clone(&parent);
                let barrier = std::sync::Arc::clone(&barrier);
                let leaf = leaf.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    open_lock_at(&parent, &leaf)
                })
            })
            .collect();
        let inodes: Vec<u64> = openers
            .into_iter()
            .map(|opener| {
                opener
                    .join()
                    .unwrap()
                    .expect("a racing lock open must not report the name missing")
                    .metadata()
                    .unwrap()
                    .ino()
            })
            .collect();
        assert!(
            inodes.iter().all(|inode| *inode == inodes[0]),
            "racing callers must share one lock file, not one each"
        );
    }
    drop(parent);
    let _ = fs::remove_dir_all(&parent_path);
}
