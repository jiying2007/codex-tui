use super::*;

#[test]
fn legacy_rules_fallback_rejects_access_denied_and_ambiguous_errors() {
    let missing = crate::forge::glab_http_failure(
        "glab: GET https://gitlab.example/api/v4/projects/1404/merge_requests/404/approval_state: 404 Not Found",
        "provider",
    );
    assert!(legacy_gitlab_rules_unavailable(&anyhow::Error::new(
        missing
    )));
    for untrusted in [
        "glab: network request failed: 404 Not Found",
        "glab: network request failed: HTTP 404 Not Found",
        "glab: response decoding failed: 404",
    ] {
        assert!(!legacy_gitlab_rules_unavailable(&anyhow::Error::new(
            crate::forge::glab_http_failure(untrusted, "provider")
        )));
    }
    let explicit_http = crate::forge::glab_http_failure(
        "glab: GET https://gitlab.example/api/v4/projects/42/merge_requests/7/approval_state: HTTP 404 Not Found",
        "provider",
    );
    assert!(legacy_gitlab_rules_unavailable(&anyhow::Error::new(
        explicit_http
    )));
    for error in [
        "glab api failed: 403 Forbidden",
        "connection timed out",
        "decode glab api response",
        "401 Unauthorized",
        "resource not found (status not given)",
        "glab api /projects/1404/merge_requests/404: 403 Forbidden",
        "HTTP 404 in a formatted, untyped error",
    ] {
        assert!(!legacy_gitlab_rules_unavailable(&anyhow::anyhow!(
            "{error}"
        )));
    }
    for status in [
        "401 Unauthorized",
        "403 Forbidden",
        "500 Internal Server Error",
    ] {
        let stderr = format!(
            "glab: GET https://gitlab.example/api/v4/projects/1404/merge_requests/404/approval_state: {status}"
        );
        let failure = crate::forge::glab_http_failure(&stderr, "provider");
        assert!(!legacy_gitlab_rules_unavailable(&anyhow::Error::new(
            failure
        )));
    }
}
