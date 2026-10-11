use super::*;
use crate::operation::OperationPlan;
use crate::worktree_git::parse_worktree_porcelain;
use tempfile::tempdir;

#[test]
fn nonzero_git_exit_is_reconciled_without_persisting_hook_secrets() {
    let repo = LocalRepoIdentity {
        git_common_dir: "/repo/.git".into(),
        primary_root: "/repo".into(),
    };
    let plan = OperationPlan::delete_branch(repo, "/repo".into(), "feature".into(), 1);
    let mut receipt = OperationReceipt::planned(plan);
    record_nonzero_mutation(
        &mut receipt,
        &crate::worktree_git::MutationOutput {
            success: false,
            code: Some(1),
            stdout: String::new(),
            stderr: "Bearer sk-private https://alice:secret@internal.invalid/".into(),
        },
    );
    assert_eq!(receipt.state, OperationState::OutcomeUnknown);
    let failure = receipt.failure.expect("classified failure");
    for secret in ["sk-private", "alice", "secret", "internal.invalid"] {
        assert!(!failure.contains(secret));
    }
}

#[tokio::test]
async fn queued_destructive_mutation_requires_fresh_admission_proof() {
    let temp = tempfile::tempdir().expect("tempdir");
    let store = SqliteStore::at(temp.path());
    let repo = LocalRepoIdentity {
        git_common_dir: "/repo/.git".into(),
        primary_root: "/repo".into(),
    };
    let plan =
        OperationPlan::remove_worktree(repo, "/repo".into(), "/repo/stale-target".into(), 1);
    let error = check_preconditions(
        &store,
        &plan,
        &[],
        Instant::now() - SCOPE_ADMISSION_MAX_AGE - Duration::from_secs(1),
    )
    .await
    .expect_err("expired scope proof must fail before git access");
    assert!(error.to_string().contains("snapshot expired"));
}

fn git(cwd: &Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .status()
        .expect("spawn git");
    assert!(status.success(), "git command failed: {args:?}");
}

fn init_repo(root: &Path) -> LocalRepoIdentity {
    std::fs::create_dir_all(root).expect("create root");
    git(root, &["init"]);
    git(root, &["config", "user.email", "ci@example.invalid"]);
    git(root, &["config", "user.name", "CI"]);
    std::fs::write(root.join("tracked.txt"), "base\n").expect("write");
    git(root, &["add", "tracked.txt"]);
    git(root, &["commit", "-m", "base"]);
    futures_lite_probe(root)
}

fn futures_lite_probe(root: &Path) -> LocalRepoIdentity {
    let common = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .output()
        .expect("git rev-parse");
    assert!(common.status.success());
    let common = String::from_utf8_lossy(&common.stdout).trim().to_string();
    LocalRepoIdentity {
        git_common_dir: canonical_path(&common),
        primary_root: canonical_path(root.to_string_lossy().as_ref()),
    }
}

#[tokio::test]
async fn same_repo_reuses_one_mutation_lock_while_other_repo_does_not() {
    let locks: RepoLocks = Arc::new(Mutex::new(BTreeMap::new()));
    let repo_a = LocalRepoIdentity {
        git_common_dir: "/repo-a/.git".into(),
        primary_root: "/repo-a".into(),
    };
    let repo_a_alias = repo_a.clone();
    let repo_b = LocalRepoIdentity {
        git_common_dir: "/repo-b/.git".into(),
        primary_root: "/repo-b".into(),
    };

    let first = repo_lock(&locks, &repo_a).await;
    let same = repo_lock(&locks, &repo_a_alias).await;
    let other = repo_lock(&locks, &repo_b).await;
    assert!(Arc::ptr_eq(&first, &same));
    assert!(!Arc::ptr_eq(&first, &other));
}

#[test]
fn mutation_coordinator_channels_are_bounded() {
    let source = include_str!("../worktree.rs");
    let production = source
        .split("#[cfg(test)]")
        .next()
        .expect("production source");
    assert!(!production.contains("unbounded_channel"));
    assert!(!production.contains("UnboundedSender"));
    assert!(!production.contains("UnboundedReceiver"));
    assert!(production.contains("JoinSet"));
}

#[test]
fn mutation_command_queue_reports_backpressure() {
    let (tx, _rx) = mpsc::channel(1);
    queue_mutation_command(&tx, MutationCommand::RefreshInventory).expect("first command");
    let error = queue_mutation_command(&tx, MutationCommand::RefreshInventory)
        .expect_err("second command must hit bounded queue");
    assert!(error.to_string().contains("queue is full"));
}

#[test]
fn porcelain_parser_tolerates_crlf_and_incidental_whitespace() {
    let parsed = parse_worktree_porcelain(
        "worktree C:/repo\r\nHEAD deadbeef\r\nbranch refs/heads/main\r\n\r\n  worktree C:/repo-feature  \r\nHEAD cafe\r\n  branch refs/heads/feature\r\n\r\n",
    );
    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed[1].path, "C:/repo-feature");
    assert_eq!(parsed[1].branch.as_deref(), Some("feature"));
}

#[test]
fn porcelain_parser_preserves_worktree_branch() {
    let parsed = parse_worktree_porcelain(
        "worktree /repo
HEAD deadbeef
branch refs/heads/main

         worktree /repo-feature
HEAD cafe
branch refs/heads/feature

",
    );
    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed[1].path, "/repo-feature");
    assert_eq!(parsed[1].branch.as_deref(), Some("feature"));
}

#[tokio::test]
async fn create_and_remove_managed_worktree_preserve_branch_separation() {
    let temp = tempdir().expect("tempdir");
    let repo_root = temp.path().join("repo");
    let repo = init_repo(&repo_root);
    let store = SqliteStore::at(temp.path().join("store"));
    let target = temp.path().join("feature-wt");
    let target_text = target.to_string_lossy().into_owned();

    let create = OperationPlan::create_worktree(
        repo.clone(),
        repo_root.to_string_lossy().into_owned(),
        target_text.clone(),
        "feature".into(),
        "HEAD".into(),
        1,
    );
    let receipt = execute_request(
        &store,
        MutationRequest {
            admitted_at: Instant::now(),
            plan: create,
            active_scopes: vec![],
        },
    )
    .await
    .expect("create receipt");
    assert_eq!(receipt.state, OperationState::Succeeded);
    assert!(target.exists());
    assert!(
        branch_exists(repo_root.to_string_lossy().as_ref(), "feature")
            .await
            .expect("branch")
    );

    let remove = OperationPlan::remove_worktree(
        repo.clone(),
        repo_root.to_string_lossy().into_owned(),
        canonical_path(&target_text),
        2,
    );
    let receipt = execute_request(
        &store,
        MutationRequest {
            admitted_at: Instant::now(),
            plan: remove,
            active_scopes: vec![],
        },
    )
    .await
    .expect("remove receipt");
    assert_eq!(receipt.state, OperationState::Succeeded);
    assert!(!target.exists());
    assert!(
        branch_exists(repo_root.to_string_lossy().as_ref(), "feature")
            .await
            .expect("branch preserved")
    );
}

#[tokio::test]
async fn uncertain_remove_requires_both_git_and_filesystem_absence() {
    let temp = tempdir().expect("tempdir");
    let repo_root = temp.path().join("repo");
    let repo = init_repo(&repo_root);
    let store = SqliteStore::at(temp.path().join("store"));
    let target = temp.path().join("uncertain-wt");
    let target_text = target.to_string_lossy().into_owned();
    git(
        &repo_root,
        &["worktree", "add", "-b", "uncertain", &target_text],
    );
    let remove = OperationPlan::remove_worktree(
        repo,
        repo_root.to_string_lossy().into_owned(),
        canonical_path(&target_text),
        1,
    );

    // Even when Git still registers the target, an attempted removal
    // could have partially deleted files: do not claim side-effect-free failure.
    let registered = reconcile_outcome(&store, &remove)
        .await
        .expect("registered");
    assert!(matches!(registered, ReconciledOutcome::Unknown(_)));

    git(&repo_root, &["worktree", "remove", &target_text]);
    // Simulate a failed cleanup after the metadata record was removed.
    std::fs::create_dir_all(&target).expect("restore orphan directory");
    std::fs::write(target.join("orphan.txt"), "not deleted").expect("orphan file");
    let leftover = reconcile_outcome(&store, &remove).await.expect("leftover");
    assert!(matches!(leftover, ReconciledOutcome::Unknown(_)));
    let error = verify_success(&store, &remove)
        .await
        .expect_err("existing target cannot be a verified removal");
    assert!(error.to_string().contains("filesystem entry still exists"));
}

#[cfg(unix)]
#[test]
fn dangling_symlink_is_not_treated_as_absent_worktree() {
    let temp = tempdir().expect("tempdir");
    let link = temp.path().join("orphan-link");
    std::os::unix::fs::symlink(temp.path().join("missing"), &link).expect("dangling test link");
    assert!(!worktree_path_absent(link.to_str().expect("utf8 path")).expect("stat link"));
}

#[tokio::test]
async fn dirty_managed_worktree_is_refused_without_force() {
    let temp = tempdir().expect("tempdir");
    let repo_root = temp.path().join("repo");
    let repo = init_repo(&repo_root);
    let store = SqliteStore::at(temp.path().join("store"));
    let target = temp.path().join("dirty-wt");
    let target_text = target.to_string_lossy().into_owned();

    let create = OperationPlan::create_worktree(
        repo.clone(),
        repo_root.to_string_lossy().into_owned(),
        target_text.clone(),
        "dirty".into(),
        "HEAD".into(),
        1,
    );
    let create = execute_request(
        &store,
        MutationRequest {
            admitted_at: Instant::now(),
            plan: create,
            active_scopes: vec![],
        },
    )
    .await
    .expect("create");
    assert_eq!(create.state, OperationState::Succeeded);
    std::fs::write(target.join("tracked.txt"), "dirty\n").expect("dirty");

    let remove = OperationPlan::remove_worktree(
        repo,
        repo_root.to_string_lossy().into_owned(),
        canonical_path(&target_text),
        2,
    );
    let remove = execute_request(
        &store,
        MutationRequest {
            admitted_at: Instant::now(),
            plan: remove,
            active_scopes: vec![],
        },
    )
    .await
    .expect("remove");
    assert_eq!(remove.state, OperationState::Failed);
    assert!(
        remove
            .failure
            .as_deref()
            .is_some_and(|failure| failure.contains("dirty"))
    );
    assert!(target.exists());
}

#[tokio::test]
async fn overlapping_active_scope_blocks_removal() {
    let temp = tempdir().expect("tempdir");
    let repo_root = temp.path().join("repo");
    let repo = init_repo(&repo_root);
    let store = SqliteStore::at(temp.path().join("store"));
    let target = temp.path().join("active-wt");
    let target_text = target.to_string_lossy().into_owned();

    let create = OperationPlan::create_worktree(
        repo.clone(),
        repo_root.to_string_lossy().into_owned(),
        target_text.clone(),
        "active".into(),
        "HEAD".into(),
        1,
    );
    let create = execute_request(
        &store,
        MutationRequest {
            admitted_at: Instant::now(),
            plan: create,
            active_scopes: vec![],
        },
    )
    .await
    .expect("create");
    assert_eq!(create.state, OperationState::Succeeded);

    let remove = OperationPlan::remove_worktree(
        repo.clone(),
        repo_root.to_string_lossy().into_owned(),
        canonical_path(&target_text),
        2,
    );
    let remove = execute_request(
        &store,
        MutationRequest {
            admitted_at: Instant::now(),
            plan: remove,
            active_scopes: vec![MutationScope {
                repo: Some(repo),
                writable_roots: vec![canonical_path(&target_text)],
                confidence: crate::operation::MutationScopeConfidence::ExactWorktree,
            }],
        },
    )
    .await
    .expect("remove");
    assert_eq!(remove.state, OperationState::Failed);
    assert!(
        remove
            .failure
            .as_deref()
            .is_some_and(|failure| failure.contains("active mutation scope"))
    );
}

#[tokio::test]
async fn stale_managed_record_does_not_authorize_unregistered_worktree_removal() {
    let temp = tempdir().expect("tempdir");
    let repo_root = temp.path().join("repo");
    let repo = init_repo(&repo_root);
    let store = SqliteStore::at(temp.path().join("store"));
    let target = temp.path().join("stale-wt");
    let target_text = target.to_string_lossy().into_owned();

    let create = OperationPlan::create_worktree(
        repo.clone(),
        repo_root.to_string_lossy().into_owned(),
        target_text.clone(),
        "stale-wt-branch".into(),
        "HEAD".into(),
        1,
    );
    let created = execute_request(
        &store,
        MutationRequest {
            admitted_at: Instant::now(),
            plan: create,
            active_scopes: vec![],
        },
    )
    .await
    .expect("create");
    assert_eq!(created.state, OperationState::Succeeded);
    // macOS may canonicalize /var to /private/var. Retain the exact
    // persisted path before deleting its target on disk.
    let managed_path = created.result_ref.expect("canonical managed path");
    git(&repo_root, &["worktree", "remove", &target_text]);

    let remove = OperationPlan::remove_worktree(
        repo,
        repo_root.to_string_lossy().into_owned(),
        managed_path,
        2,
    );
    let rejected = execute_request(
        &store,
        MutationRequest {
            admitted_at: Instant::now(),
            plan: remove,
            active_scopes: vec![],
        },
    )
    .await
    .expect("conservative reject");
    assert_eq!(rejected.state, OperationState::Failed);
    assert!(
        rejected
            .failure
            .as_deref()
            .is_some_and(|message| { message.contains("no longer registered") })
    );
}

#[tokio::test]
async fn adopt_records_existing_worktree_without_running_git_mutation() {
    let temp = tempdir().expect("tempdir");
    let repo_root = temp.path().join("repo");
    let repo = init_repo(&repo_root);
    let store = SqliteStore::at(temp.path().join("store"));
    let target = temp.path().join("existing-wt");
    let target_text = target.to_string_lossy().into_owned();
    git(
        &repo_root,
        &["worktree", "add", "-b", "existing", &target_text],
    );

    let plan = OperationPlan::adopt_worktree(
        repo.clone(),
        repo_root.to_string_lossy().into_owned(),
        canonical_path(&target_text),
        1,
    );
    let receipt = execute_request(
        &store,
        MutationRequest {
            admitted_at: Instant::now(),
            plan,
            active_scopes: vec![],
        },
    )
    .await
    .expect("adopt");
    assert_eq!(receipt.state, OperationState::Succeeded);

    let managed = store
        .managed_worktree(&repo.git_common_dir, &canonical_path(&target_text))
        .expect("lookup")
        .expect("managed");
    assert!(managed.adopted);
    assert_eq!(managed.branch.as_deref(), Some("existing"));
}

#[tokio::test]
async fn recovery_reconciles_executing_create_instead_of_retrying() {
    let temp = tempdir().expect("tempdir");
    let repo_root = temp.path().join("repo");
    let repo = init_repo(&repo_root);
    let store = SqliteStore::at(temp.path().join("store"));
    let target = temp.path().join("recovered-wt");
    let target_text = target.to_string_lossy().into_owned();

    git(
        &repo_root,
        &["worktree", "add", "-b", "recovered", &target_text, "HEAD"],
    );
    let plan = OperationPlan::create_worktree(
        repo,
        repo_root.to_string_lossy().into_owned(),
        canonical_path(&target_text),
        "recovered".into(),
        "HEAD".into(),
        1,
    );
    let mut receipt = OperationReceipt::planned(plan);
    receipt.start(2);
    store
        .save_operation_receipt(&receipt)
        .expect("save executing");

    let (tx, mut rx) = mpsc::channel(MUTATION_EVENT_QUEUE_CAPACITY);
    recover_incomplete(&store, &Arc::new(Mutex::new(BTreeMap::new())), &tx)
        .await
        .expect("recover");
    let recovered = match rx.recv().await.expect("event") {
        MutationEvent::Receipt(receipt) => *receipt,
        other => panic!("unexpected event: {other:?}"),
    };
    assert_eq!(recovered.state, OperationState::Succeeded);
    assert!(
        recovered
            .verification
            .as_deref()
            .is_some_and(|value| value.contains("reconciled"))
    );
}
