use super::*;

#[test]
fn legacy_rules_fallback_rejects_access_denied_and_ambiguous_errors() {
    let missing = crate::forge::glab_http_failure(
        "glab: GET https://gitlab.example/api/v4/projects/1404/merge_requests/404/approval_state: 404 Not Found",
        "provider",
    );
    assert!(legacy_gitlab_rules_unavailable(&anyhow::Error::new(missing)));
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
    for status in ["401 Unauthorized", "403 Forbidden", "500 Internal Server Error"] {
        let stderr = format!(
            "glab: GET https://gitlab.example/api/v4/projects/1404/merge_requests/404/approval_state: {status}"
        );
        let failure = crate::forge::glab_http_failure(&stderr, "provider");
        assert!(!legacy_gitlab_rules_unavailable(&anyhow::Error::new(failure)));
    }
}

