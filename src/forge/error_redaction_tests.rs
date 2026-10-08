use super::*;

#[test]
fn native_provider_error_never_echoes_sensitive_stderr() {
    for secret in [
        "Bearer sk-protected https://alice:password@internal.example/api 401 Unauthorized",
        "glab auth token=sensitive?access_token=bad 403 Forbidden",
        "git credential helper: password=do-not-log",
        "connection refused for https://bob:private@localhost/",
    ] {
        let safe = trim_error(secret, "unknown glab error");
        for sensitive in [
            "sk-protected",
            "password",
            "alice",
            "private",
            "sensitive",
            "do-not-log",
            "access_token",
        ] {
            assert!(!safe.contains(sensitive), "leaked sensitive stderr field");
        }
        assert!(safe.starts_with("unknown glab error"));
        assert!(safe.len() < 100);
    }
}

#[test]
fn stderr_reason_codes_remain_useful_without_raw_content() {
    assert_eq!(
        trim_error("HTTP 401 Unauthorized, Bearer SECRET", "provider"),
        "provider: authentication rejected"
    );
    assert_eq!(
        trim_error("HTTP 403 Forbidden", "provider"),
        "provider: access denied"
    );
    assert_eq!(
        trim_error("upstream request timed out", "provider"),
        "provider: request timed out"
    );
    assert_eq!(trim_error("", "provider"), "provider");
}
