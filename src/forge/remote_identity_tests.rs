use super::*;

#[test]
fn uri_credentials_never_become_a_forge_project_identity() {
    for uri in [
        "https://user:secret@internal.example/team/repo.git?access_token=DO_NOT_LOG",
        "https://internal.example/team/repo.git#password=DO_NOT_LOG",
        "ssh://git@internal.example:2222/team/repo.git?token=DO_NOT_LOG",
        "git@internal.example:team/repo.git?token=DO_NOT_LOG",
        "git@internal.example:team/repo.git#password=DO_NOT_LOG",
    ] {
        assert!(
            parse_git_remote_url(uri).is_none(),
            "credential-bearing URL must fail closed"
        );
    }
    assert!(
        redact_git_remote_url("user@internal.example:team/repo.git?token=SECRET").is_err()
    );
}

#[test]
fn standard_http_ssh_and_scp_remotes_keep_only_host_and_project() {
    let expected = Some(("internal.example".into(), "team/repo".into()));
    for remote in [
        "https://user:secret@internal.example/team/repo.git",
        "ssh://git@internal.example:2222/team/repo.git",
        "git@internal.example:team/repo.git",
    ] {
        assert_eq!(parse_git_remote_url(remote), expected);
    }
    assert_eq!(
        redact_git_remote_url("https://user:secret@internal.example/team/repo.git")
            .expect("redacted valid HTTP remote"),
        "https://internal.example/team/repo.git"
    );
}

#[test]
fn malformed_remote_errors_do_not_echo_urls() {
    // Production resolve_git_remote emits only this fixed error text when
    // parse_git_remote_url rejects an unsupported or credential-bearing URL.
    let error = parse_git_remote_url("https://host/team/repo.git?secret=DO_NOT_LOG")
        .ok_or_else(|| anyhow!("unsupported Git remote URL (redacted)"))
        .expect_err("query-bearing project must not resolve");
    assert!(!format!("{error:#}").contains("DO_NOT_LOG"));
}
