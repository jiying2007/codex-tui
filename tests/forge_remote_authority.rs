use codex_tui::forge::parse_git_remote_url;

#[test]
fn http_remote_keeps_api_port_without_userinfo_and_normalizes_default_ports() {
    for (remote, authority) in [
        (
            "https://gitlab.example:8443/group/repo.git",
            "gitlab.example:8443",
        ),
        (
            "http://gitlab.example:8080/group/repo.git",
            "gitlab.example:8080",
        ),
        (
            "https://gitlab.example:443/group/repo.git",
            "gitlab.example",
        ),
        ("http://gitlab.example:80/group/repo.git", "gitlab.example"),
        (
            "https://user:password@gitlab.example:8443/group/repo.git",
            "gitlab.example:8443",
        ),
        (
            "https://[2001:db8::1]:8443/group/repo.git",
            "[2001:db8::1]:8443",
        ),
        ("https://[2001:db8::1]:443/group/repo.git", "[2001:db8::1]"),
    ] {
        assert_eq!(
            parse_git_remote_url(remote),
            Some((authority.to_string(), "group/repo".to_string())),
            "HTTP authority {authority}"
        );
    }
}

#[test]
fn git_transport_port_does_not_override_api_authority() {
    for (remote, authority) in [
        (
            "ssh://git@gitlab.example:2222/group/repo.git",
            "gitlab.example",
        ),
        ("git://gitlab.example:9418/group/repo.git", "gitlab.example"),
        (
            "ssh://git@[2001:db8::1]:2222/group/repo.git",
            "[2001:db8::1]",
        ),
        ("git@gitlab.example:group/repo.git", "gitlab.example"),
    ] {
        assert_eq!(
            parse_git_remote_url(remote),
            Some((authority.to_string(), "group/repo".to_string()))
        );
    }
}
