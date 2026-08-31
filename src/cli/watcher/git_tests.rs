use super::tests::*;
use super::*;

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
fn failed_git_discovery_does_not_baseline_away_an_unknown_head() {
    let production = include_str!("mod.rs")
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
fn commit_metadata_parser_keeps_author_time_and_no_diff() {
    let repo = PathBuf::from("/workspace/project");
    let sha = "a".repeat(40);
    let metadata = parse_commit_headers(&format!("{sha}\02024-01-02T03:04:05+00:00\0")).unwrap();
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
    assert_eq!(commits[0].message, "fix parser\0\x1e\u{FFFD}");
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
