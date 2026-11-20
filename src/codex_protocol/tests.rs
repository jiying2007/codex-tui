    use super::*;
    use serde_json::json;

    fn wire(status: Value) -> ThreadWire {
        ThreadWire {
            id: "0199-test".into(),
            lineage: ThreadLineage::default(),
            preview: "First user prompt\nsecond line".into(),
            project_id: None,
            model: Some("gpt-5.6".into()),
            updated_at: 42,
            recency_at: None,
            status,
            cwd: "/work/audio-pipeline".into(),
            source: json!("cli"),
            git_info: Some(GitInfoWire {
                origin_url: Some("https://github.com/jiying2007/audio-pipeline.git".into()),
            }),
            name: None,
        }
    }

    #[test]
    fn active_waiting_flags_become_attention_without_inventing_state() {
        let summary = normalize_thread(
            wire(json!({
                "type": "active",
                "activeFlags": ["waitingOnApproval", "waitingOnUserInput"]
            })),
            None,
        );
        assert_eq!(summary.runtime, RuntimeStatus::WaitingHuman);
        assert_eq!(
            summary.attention,
            vec![
                AttentionReason::ApprovalRequired,
                AttentionReason::UserInputRequired
            ]
        );
    }

    #[test]
    fn recency_at_is_preferred_for_registry_ordering_timestamp() {
        let mut thread = wire(json!({"type": "idle"}));
        thread.updated_at = 10;
        thread.recency_at = Some(25);
        let summary = normalize_thread(thread, None);
        assert_eq!(summary.metadata.updated_at, 25);
    }

    #[test]
    fn system_error_remains_a_needs_you_state() {
        let summary = normalize_thread(wire(json!({"type": "systemError"})), None);
        assert_eq!(summary.runtime, RuntimeStatus::SystemError);
        assert_eq!(summary.attention, vec![AttentionReason::SystemError]);
    }

    #[test]
    fn workspace_fallback_prefers_project_then_git_then_cwd() {
        let mut thread = wire(json!({"type": "idle"}));
        thread.project_id = Some("proj-1".into());
        let project = normalize_thread(thread.clone(), None);
        assert_eq!(project.metadata.workspace_basis, "codex-project");

        thread.project_id = None;
        let git = normalize_thread(thread.clone(), None);
        assert_eq!(git.workspace, "audio-pipeline");
        assert_eq!(git.metadata.workspace_basis, "git-origin");

        thread.git_info = None;
        let cwd = normalize_thread(thread, None);
        assert_eq!(cwd.workspace, "audio-pipeline");
        assert_eq!(cwd.metadata.workspace_basis, "cwd");
    }

    #[test]
    fn git_origins_are_identity_equivalent_and_strip_credential_suffices() {
        let plain = "https://github.com/Team/Repo.git";
        let credentialed =
            "https://user:secret@github.com/Team/Repo.git?token=supersecret#fragment";
        let scp = "git@github.com:Team/Repo.git";
        let expected = Some("github.com/team/repo".to_string());
        assert_eq!(normalize_git_origin(plain), expected);
        assert_eq!(normalize_git_origin(credentialed), expected);
        assert_eq!(normalize_git_origin(scp), expected);
        assert_eq!(
            normalize_git_origin("ssh://git@github.com:2222/Team/Repo.git"),
            Some("github.com:2222/team/repo".into())
        );
    }

    #[test]
    fn untrusted_remote_never_enters_workspace_or_search_identity() {
        let mut thread = wire(json!({"type": "idle"}));
        thread.git_info = Some(GitInfoWire {
            origin_url: Some(
                "https://alice:password@example.com/Team/Repo.git?access=topsecret#token".into(),
            ),
        });
        let view = normalize_thread(thread.clone(), None);
        assert_eq!(view.workspace, "repo");
        assert_eq!(view.metadata.workspace_key, "git:example.com/team/repo");
        for field in [&view.workspace, &view.metadata.workspace_key] {
            assert!(!field.contains("password"));
            assert!(!field.contains("topsecret"));
            assert!(!field.contains("token"));
        }

        thread.git_info = Some(GitInfoWire {
            origin_url: Some("file:///tmp/secret?token=private".into()),
        });
        let rejected = normalize_thread(thread, None);
        assert_eq!(rejected.metadata.workspace_basis, "cwd");
        assert_eq!(rejected.metadata.workspace_key, "cwd:/work/audio-pipeline");
    }

    #[test]
    fn exact_thread_id_is_preserved_as_identity() {
        let summary = normalize_thread(wire(json!({"type": "notLoaded"})), None);
        assert_eq!(summary.id.0, "0199-test");
    }

#[test]
fn optional_parent_and_provider_fields_roundtrip_without_confusing_forks() {
    let parsed = parse_thread_list(json!({
        "data": [{
            "id": "child", "sessionId": "family", "parentThreadId": "parent",
            "modelProvider": "openai", "originator": "vscode",
            "source": {"subAgent":"review"}, "status": {"type":"idle"}
        }],
        "nextCursor": null
    })).expect("upstream extended wire");
    let thread = normalize_thread(parsed.data.into_iter().next().unwrap(), None);
    assert_eq!(thread.metadata.lineage.parent_thread_id.as_deref(), Some("parent"));
    assert_eq!(thread.metadata.lineage.session_id.as_deref(), Some("family"));
    assert_eq!(thread.metadata.lineage.model_provider.as_deref(), Some("openai"));
    assert_eq!(thread.metadata.lineage.originator.as_deref(), Some("vscode"));

    let old = parse_thread_list(json!({
        "data": [{"id":"legacy"}], "nextCursor":null
    })).expect("old optional fields");
    assert!(old.data[0].lineage.parent_thread_id.is_none());
}
