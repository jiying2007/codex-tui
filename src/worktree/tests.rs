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
    let plan = OperationPlan::remove_worktree(repo, "/repo".into(), "/repo/stale-target".into(), 1);
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
        .args(["rev-parse", "--git-common-dir"])
        .output()
        .expect("git rev-parse");
    assert!(common.status.success());
    let output = String::from_utf8(common.stdout).expect("UTF-8 test repo path");
    let path = output.strip_suffix('\n').expect("git output newline");
    let common = std::fs::canonicalize(root.join(path)).expect("canonical Git common dir");
    LocalRepoIdentity {
        git_common_dir: common.to_string_lossy().into_owned(),
        primary_root: canonical_path(root.to_string_lossy().as_ref()),
    }
}

#[cfg(unix)]
#[tokio::test]
async fn changed_cwd_repository_is_rejected_before_branch_mutation_or_recovery() {
    use std::os::unix::fs::symlink;
    let temp = tempdir().unwrap();
    let source = temp.path().join("source");
    let foreign = temp.path().join("foreign");
    let selected = temp.path().join("selected");
    let planned_repo = init_repo(&source);
    init_repo(&foreign);
    git(&foreign, &["branch", "victim"]);
    symlink(&source, &selected).unwrap();
    let plan = OperationPlan::delete_branch(
        planned_repo,
        selected.to_str().unwrap().into(),
        "victim".into(),
        1,
    );
    ensure_plan_repository_identity(&plan).await.unwrap();
    std::fs::remove_file(&selected).unwrap();
    symlink(&foreign, &selected).unwrap();
    let error = ensure_plan_repository_identity(&plan).await.unwrap_err();
    assert!(error.to_string().contains("planned repository identity"));
    let store = SqliteStore::at(temp.path().join("store"));
    let result = execute_request(
        &store,
        MutationRequest {
            plan: plan.clone(),
            active_scopes: vec![],
            admitted_at: Instant::now(),
        },
    )
    .await
    .unwrap();
    assert_eq!(result.state, OperationState::Failed);
    assert!(
        branch_exists(&foreign.to_string_lossy(), "victim")
            .await
            .unwrap()
    );
    assert!(reconcile_outcome(&store, &plan).await.is_err());
}

#[cfg(unix)]
#[tokio::test]
async fn git_cwd_retarget_after_preconditions_is_rejected_before_write() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().unwrap();
    let original = temp.path().join("original");
    let foreign = temp.path().join("foreign");
    let selected = temp.path().join("selected");
    let identity = init_repo(&original);
    init_repo(&foreign);
    git(&original, &["branch", "victim"]);
    git(&foreign, &["branch", "victim"]);
    symlink(&original, &selected).unwrap();

    let plan = OperationPlan::delete_branch(
        identity,
        selected.to_str().unwrap().into(),
        "victim".into(),
        1,
    );
    let store = SqliteStore::at(temp.path().join("state"));
    check_preconditions(&store, &plan, &[], Instant::now())
        .await
        .expect("before retargeting, admission accepts the original repository");
    std::fs::remove_file(&selected).unwrap();
    symlink(&foreign, &selected).unwrap();

    assert!(
        ensure_plan_repository_identity(&plan).await.is_err(),
        "a post-admission retarget must fail the final pre-write guard"
    );
    assert!(
        branch_exists(foreign.to_str().unwrap(), "victim")
            .await
            .unwrap()
    );
    assert!(
        branch_exists(original.to_str().unwrap(), "victim")
            .await
            .unwrap()
    );
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
fn nul_porcelain_parser_preserves_worktree_paths_and_branch_identity() {
    let parsed = parse_worktree_porcelain(
        "worktree C:/repo\0HEAD deadbeef\0branch refs/heads/main\0\0worktree C:/repo-feature  \0HEAD cafe\0branch refs/heads/feature\0\0",
    )
    .expect("complete NUL-delimited Git inventory");
    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed[0].path, "C:/repo");
    assert_eq!(parsed[1].path, "C:/repo-feature  ");
    assert_eq!(parsed[1].branch.as_deref(), Some("feature"));
}

#[test]
fn nul_porcelain_parser_preserves_newline_paths_and_detached_bare_state() {
    let parsed = parse_worktree_porcelain(
        "worktree /repo\0bare\0\0worktree /repo/with\nnewline\0HEAD cafe\0detached\0\0",
    )
    .expect("complete NUL-delimited Git inventory");
    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed[0].branch, None);
    assert_eq!(parsed[1].path, "/repo/with\nnewline");
    assert_eq!(parsed[1].branch, None);
}

#[test]
fn worktree_inventory_refuses_incomplete_or_ambiguous_porcelain() {
    for value in [
        "",
        "worktree /repo\0HEAD cafe\0branch refs/heads/main\0",
        "worktree /repo\0\0",
        "HEAD cafe\0\0",
        "worktree /repo\0worktree /other\0HEAD cafe\0\0",
        "worktree /repo\0HEAD cafe\0unexpected future field\0\0",
    ] {
        assert!(
            parse_worktree_porcelain(value).is_err(),
            "incomplete/ambiguous worktree inventory accepted: {value:?}"
        );
    }
}

#[test]
fn strict_legacy_git_porcelain_keeps_trailing_space_but_rejects_ambiguous_lines() {
    let parsed = crate::worktree_git::parse_worktree_porcelain_lines(
        "worktree /repo space  \nHEAD deadbeef\nbranch refs/heads/main\n\n",
    )
    .expect("legacy Git complete plaintext inventory");
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0].path, "/repo space  ");

    for malformed in [
        "worktree /repo\nHEAD deadbeef\nbranch refs/heads/main\n",
        "worktree /repo\nspoofed-line\nHEAD deadbeef\nbranch refs/heads/main\n\n",
        "worktree /repo\nlocked\nHEAD deadbeef\nbranch refs/heads/main\n\n",
        "worktree /repo\nprunable\nHEAD deadbeef\nbranch refs/heads/main\n\n",
        "worktree /repo\nHEAD deadbeef\nHEAD cafe\nbranch refs/heads/main\n\n",
        "worktree /repo\nHEAD deadbeef\nbranch refs/heads/main\nbranch refs/heads/other\n\n",
    ] {
        assert!(
            crate::worktree_git::parse_worktree_porcelain_lines(malformed).is_err(),
            "ambiguous legacy Git inventory accepted: {malformed:?}"
        );
    }
}

#[cfg(unix)]
#[tokio::test]
async fn real_git_worktree_inventory_preserves_whitespace_in_directory_names() {
    let temp = tempdir().expect("tempdir");
    let repo_root = temp.path().join("repo");
    init_repo(&repo_root);
    let target = temp.path().join("feature with newline\ntrailing ");
    let target_text = target.to_str().expect("UTF-8 temporary path");
    git(
        &repo_root,
        &["worktree", "add", "-b", "whitespace", target_text],
    );

    let supports_nul = crate::worktree_git::run_git(
        repo_root.to_str().unwrap(),
        ["worktree", "list", "--porcelain", "-z"],
    )
    .await
    .expect("probe Git capability")
    .success;
    let inventory = list_worktrees(repo_root.to_str().unwrap()).await;
    if supports_nul {
        let inventory = inventory.expect("complete NUL-delimited Git inventory");
        let canonical_target = canonical_path(target_text);
        assert!(
            inventory.iter().any(|entry| {
                entry.path == canonical_target && entry.branch.as_deref() == Some("whitespace")
            }),
            "NUL Git inventory must preserve canonical directory bytes"
        );
    } else {
        assert!(
            inventory.is_err(),
            "legacy Git newline output cannot safely validate newline-bearing worktree paths"
        );
    }
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
async fn default_head_plan_uses_selected_worktree_commit_even_after_head_moves() {
    use crate::app::{Action, AppState, View, reduce};
    use crate::backend::{CodexBackend, FakeBackend};
    for kind in ["main", "linked", "detached"] {
        let temp = tempdir().unwrap();
        let primary = temp.path().join("repo");
        init_repo(&primary);
        let selected = if kind == "main" {
            primary.clone()
        } else {
            let path = temp.path().join("selected");
            if kind == "detached" {
                git(
                    &primary,
                    &["worktree", "add", "--detach", path.to_str().unwrap()],
                );
            } else {
                git(
                    &primary,
                    &["worktree", "add", "-b", "selected", path.to_str().unwrap()],
                );
            }
            git(
                &path,
                &["commit", "--allow-empty", "-m", "selected worktree commit"],
            );
            path
        };
        let mut threads = FakeBackend::scaled(1).snapshot().threads;
        threads[0].metadata.cwd = selected.to_string_lossy().into_owned();
        let thread_id = threads[0].id.clone();
        let context = crate::git::probe_context(thread_id.clone(), threads[0].metadata.cwd.clone())
            .await
            .unwrap();
        let observed = context.head.clone().unwrap();
        let mut app = AppState::new(threads);
        app.view = View::ManagedWorktrees(thread_id.clone());
        app.git_contexts.insert(thread_id.0.clone(), context);
        reduce(&mut app, Action::BeginCreateWorktree);
        app.input_buffer = "created".into();
        assert!(reduce(&mut app, Action::CommitInput).is_empty());
        let target = temp.path().join("created");
        app.input_buffer = target.to_string_lossy().into_owned();
        assert!(reduce(&mut app, Action::CommitInput).is_empty());
        assert_eq!(app.input_buffer, "HEAD");
        assert!(reduce(&mut app, Action::CommitInput).is_empty());
        let plan = app.pending_operation.take().expect("reviewable plan");
        assert_eq!(
            plan.expected_start_commit(),
            Some(observed.as_str()),
            "{kind}"
        );
        assert!(plan.expected_side_effect.contains("observed HEAD commit"));
        git(
            &selected,
            &["commit", "--allow-empty", "-m", "after preview"],
        );
        let store = SqliteStore::at(temp.path().join("store"));
        let receipt = execute_request(
            &store,
            MutationRequest {
                plan: plan.clone(),
                active_scopes: vec![],
                admitted_at: Instant::now(),
            },
        )
        .await
        .unwrap();
        assert_eq!(
            receipt.state,
            OperationState::Succeeded,
            "{kind}: {receipt:?}"
        );
        let created = crate::git::probe_context(thread_id, target.to_string_lossy().into_owned())
            .await
            .unwrap();
        assert_eq!(created.head.as_deref(), Some(observed.as_str()), "{kind}");
        git(
            &target,
            &["commit", "--allow-empty", "-m", "different result commit"],
        );
        assert!(
            verify_success(&store, &plan)
                .await
                .unwrap_err()
                .to_string()
                .contains("confirmed start commit")
        );
        assert!(
            matches!(
                reconcile_receipt(&store, OperationReceipt::planned(plan))
                    .await
                    .unwrap()
                    .state,
                OperationState::OutcomeUnknown
            ),
            "a different HEAD is never proof of this create"
        );
    }
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
async fn ignored_local_assets_added_after_preview_block_managed_worktree_removal() {
    let temp = tempdir().unwrap();
    let primary = temp.path().join("repo");
    let repo = init_repo(&primary);
    std::fs::write(primary.join(".gitignore"), ".env\ncache/\n").unwrap();
    git(&primary, &["add", ".gitignore"]);
    git(&primary, &["commit", "-m", "ignore local assets"]);
    let target = temp.path().join("managed");
    let store = SqliteStore::at(temp.path().join("store"));
    let create = OperationPlan::create_worktree(
        repo.clone(),
        repo.primary_root.clone(),
        target.to_string_lossy().into_owned(),
        "managed".into(),
        "HEAD".into(),
        1,
    );
    assert_eq!(
        execute_request(
            &store,
            MutationRequest {
                plan: create,
                active_scopes: vec![],
                admitted_at: Instant::now(),
            }
        )
        .await
        .unwrap()
        .state,
        OperationState::Succeeded
    );
    let remove = OperationPlan::remove_worktree(
        repo.clone(),
        repo.primary_root.clone(),
        canonical_path(target.to_str().unwrap()),
        2,
    );
    assert!(
        remove
            .preconditions
            .iter()
            .any(|condition| condition.expected.contains("ignored files"))
    );
    assert!(!remove.argv.iter().any(|argument| argument == "--force"));
    std::fs::write(target.join(".env"), "LOCAL_ASSET_SENTINEL=1").unwrap();
    std::fs::create_dir(target.join("cache")).unwrap();
    std::fs::write(target.join("cache/asset"), "retained ignored asset").unwrap();
    let receipt = execute_request(
        &store,
        MutationRequest {
            plan: remove,
            active_scopes: vec![],
            admitted_at: Instant::now(),
        },
    )
    .await
    .unwrap();
    assert_eq!(receipt.state, OperationState::Failed);
    assert!(receipt.failure.unwrap().contains("ignored files"));
    assert_eq!(
        std::fs::read_to_string(target.join(".env")).unwrap(),
        "LOCAL_ASSET_SENTINEL=1"
    );
    assert!(target.join("cache/asset").exists());
    assert!(
        store
            .managed_worktree(
                &repo.git_common_dir,
                &canonical_path(target.to_str().unwrap())
            )
            .unwrap()
            .is_some()
    );
    std::fs::remove_file(target.join(".env")).unwrap();
    std::fs::remove_dir_all(target.join("cache")).unwrap();
    std::fs::write(target.join("untracked.txt"), "untracked asset").unwrap();
    let remove = OperationPlan::remove_worktree(
        repo.clone(),
        repo.primary_root.clone(),
        canonical_path(target.to_str().unwrap()),
        3,
    );
    assert_eq!(
        execute_request(
            &store,
            MutationRequest {
                plan: remove,
                active_scopes: vec![],
                admitted_at: Instant::now(),
            }
        )
        .await
        .unwrap()
        .state,
        OperationState::Failed
    );
    assert!(target.join("untracked.txt").exists());
    std::fs::remove_file(target.join("untracked.txt")).unwrap();
    let remove = OperationPlan::remove_worktree(
        repo.clone(),
        repo.primary_root.clone(),
        canonical_path(target.to_str().unwrap()),
        4,
    );
    assert_eq!(
        execute_request(
            &store,
            MutationRequest {
                plan: remove,
                active_scopes: vec![],
                admitted_at: Instant::now(),
            }
        )
        .await
        .unwrap()
        .state,
        OperationState::Succeeded
    );
    assert!(!target.exists());
    assert!(branch_exists(&repo.primary_root, "managed").await.unwrap());
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
