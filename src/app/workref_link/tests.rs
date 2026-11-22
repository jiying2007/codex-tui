use super::*;
    use super::*;
    use crate::{
        app::{Action, reduce},
        app_server_target::ResolvedAppServerTarget,
        backend::{CodexBackend, FakeBackend},
        command::Command,
        store::LocalStateV1,
    };

    fn configured() -> AppState {
        let target = ResolvedAppServerTarget::implicit_local(None);
        let mut app = AppState::new(FakeBackend::scaled(3).snapshot().threads);
        app.configure_operator_target(&target);
        app.apply_local_state(&LocalStateV1::default());
        let mut status = crate::backend::BackendStatus::starting(target.status_source());
        status.connected = true;
        status.codex_home = Some("/home/test/.codex".into());
        app.backend_status = status;
        app.bind_operator_status();
        let thread = app.threads[0].id.clone();
        app.view = View::Thread(thread);
        let mut ledger = crate::workref::WorkRefLedger::default();
        ledger
            .add_item(
                "BUG17".into(),
                crate::workref::WorkItem::new(
                    "local-draft".into(),
                    "robot".into(),
                    "technical".into(),
                    "BUG17".into(),
                    None,
                )
                .unwrap(),
            )
            .unwrap();
        app.install_workrefs(Some(ledger));
        app
    }

    #[test]
    fn workref_link_captures_exact_live_thread_and_generates_only_a_write_effect() {
        let mut app = configured();
        let expected = app.workref_link_snapshot().expect("exact binding");
        assert_eq!(expected.session.codex_home, "/home/test/.codex");
        assert_eq!(expected.session.role, "development");
        reduce(&mut app, Action::BeginWorkrefLink);
        assert_eq!(app.input_mode, InputMode::WorkrefLink);
        assert!(reduce(&mut app, Action::InputText("BUG17".into())).is_empty());
        let effect = reduce(&mut app, Action::CommitInput)
            .pop()
            .expect("link effect");
        assert!(matches!(
            effect,
            Effect::LinkWorkrefSession { key, session }
                if key == "BUG17" && session == expected.session
        ));
        assert_eq!(app.input_mode, InputMode::WorkrefLink);
        assert!(app.workref_link_pending);
        assert_eq!(app.input_buffer, "BUG17");

        // Once accepted by the app, the editor is frozen until the worker
        // confirms commit or explicitly reports rejection.
        reduce(&mut app, Action::InputText("X".into()));
        reduce(&mut app, Action::InputBackspace);
        reduce(&mut app, Action::CancelInput);
        assert!(reduce(&mut app, Action::CommitInput).is_empty());
        assert_eq!(app.input_buffer, "BUG17");
        assert!(app.workref_link_pending);

        reduce(
            &mut app,
            Action::WorkrefLinkSubmissionRejected("worker queue full".into()),
        );
        assert!(!app.workref_link_pending);
        assert_eq!(app.input_mode, InputMode::WorkrefLink);
        assert_eq!(app.input_buffer, "BUG17");
        let retry = reduce(&mut app, Action::CommitInput);
        assert!(matches!(
            retry.as_slice(),
            [Effect::LinkWorkrefSession { key, session }]
                if key == "BUG17" && *session == expected.session
        ));
        assert!(app.workref_link_pending);

        let committed = (*app.workref_ledger).clone();
        app.finish_workref_link_success("BUG17".into(), committed, "robot-app".into());
        assert_eq!(app.input_mode, InputMode::Normal);
        assert!(!app.workref_link_pending);
        assert!(app.input_buffer.is_empty());
    }

    #[test]
    fn workref_link_refuses_home_drift_and_unknown_keys_without_writing() {
        let mut app = configured();
        reduce(&mut app, Action::BeginWorkrefLink);
        app.input_buffer = "BUG17".into();
        app.backend_status.codex_home = Some("/home/other/.codex".into());
        assert!(reduce(&mut app, Action::CommitInput).is_empty());
        assert_eq!(app.input_mode, InputMode::Normal);
        assert!(
            app.mutation_notice
                .as_deref()
                .unwrap_or_default()
                .contains("changed")
        );

        let mut app = configured();
        reduce(&mut app, Action::BeginWorkrefLink);
        app.input_buffer = "MISSING".into();
        assert!(reduce(&mut app, Action::CommitInput).is_empty());
        assert_eq!(app.input_mode, InputMode::WorkrefLink);
        assert!(
            app.mutation_notice
                .as_deref()
                .unwrap_or_default()
                .contains("unknown")
        );
    }

    #[test]
    fn palette_and_context_menu_offer_link_only_for_exact_existing_local_workrefs() {
        let mut app = configured();
        reduce(&mut app, Action::OpenCommandPalette);
        assert!(
            app.command_palette_choices()
                .contains(&Command::LinkWorkItem)
        );
        app.command_palette_query = "link".into();
        assert_eq!(app.command_palette_choice(), Some(Command::LinkWorkItem));
        app.backend_status.codex_home = Some("/home/other/.codex".into());
        assert!(app.take_command_palette_choice().is_none());
        assert!(!app.command_palette_open);

        let mut app = configured();
        let choice = crate::app::ContextChoice::LinkWorkItem;
        assert!(app.context_choices().contains(&choice));
        reduce(&mut app, Action::OpenContext);
        app.context_selected = app
            .context_choices()
            .iter()
            .position(|candidate| *candidate == choice)
            .expect("link action in context menu");
        reduce(&mut app, Action::ExecuteContext);
        assert_eq!(app.input_mode, InputMode::WorkrefLink);

        let mut app = configured();
        app.install_workrefs(Some(crate::workref::WorkRefLedger::default()));
        assert!(!app.context_choices().contains(&choice));
    }

    #[test]
    fn accepted_link_with_unknown_worker_outcome_is_not_retryable_or_claimed_failed() {
        let mut app = configured();
        reduce(&mut app, Action::BeginWorkrefLink);
        app.input_buffer = "BUG17".into();
        assert_eq!(reduce(&mut app, Action::CommitInput).len(), 1);
        assert!(app.workref_link_pending);
        app.workref_link_outcome_unknown("local state worker stopped".into());
        assert!(!app.workref_link_pending);
        assert_eq!(app.input_mode, InputMode::Normal);
        assert!(app.workref_unavailable);
        assert!(app.mutation_notice.as_deref().unwrap_or_default().contains("unknown"));
    }

    #[test]
    fn workref_link_refuses_target_thread_and_view_drift_before_emitting_effect() {
        let mut app = configured();
        reduce(&mut app, Action::BeginWorkrefLink);
        app.input_buffer = "BUG17".into();
        app.view = View::Thread(app.threads[1].id.clone());
        assert!(reduce(&mut app, Action::CommitInput).is_empty());
        assert_eq!(app.input_mode, InputMode::Normal);
        assert!(app.mutation_notice.as_deref().unwrap_or_default().contains("changed"));

        let mut app = configured();
        reduce(&mut app, Action::BeginWorkrefLink);
        app.input_buffer = "BUG17".into();
        let target = ResolvedAppServerTarget {
            name: "other-target".into(),
            endpoint: crate::app_server_target::ResolvedAppServerEndpoint::Stdio {
                codex_bin: None,
            },
        };
        app.configure_operator_target(&target);
        assert!(reduce(&mut app, Action::CommitInput).is_empty());
        assert_eq!(app.input_mode, InputMode::Normal);
        assert!(app.mutation_notice.as_deref().unwrap_or_default().contains("changed"));
    }

    #[test]
    fn workref_link_is_unavailable_without_connected_scope_or_existing_items() {
        let mut app = configured();
        app.backend_status.connected = false;
        assert!(app.workref_link_snapshot().is_none());
        app.backend_status.connected = true;
        app.install_workrefs(Some(crate::workref::WorkRefLedger::default()));
        assert!(app.workref_link_snapshot().is_none());
        app.install_workrefs(None);
        assert!(app.workref_link_snapshot().is_none());
    }
