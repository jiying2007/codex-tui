use super::trim_error;
#[derive(Debug)]
pub(crate) struct GlabHttpFailure {
    pub(crate) status: Option<u16>,
    safe_message: String,
}
impl std::fmt::Display for GlabHttpFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.safe_message)
    }
}
impl std::error::Error for GlabHttpFailure {}

// Parse status only from the HTTP response position, never from a URL,
// project ID, MR number or error text. Unknown formats fail closed.
fn glab_http_status(stderr: &str) -> Option<u16> {
    for raw_line in stderr.lines() {
        let raw = raw_line.trim();
        let from_glab = raw.starts_with("glab:");
        let line = raw.strip_prefix("glab:").unwrap_or(raw).trim();
        let candidate = line
            .strip_prefix("HTTP ")
            .or_else(|| line.strip_prefix("HTTP/1.1 "))
            .or_else(|| line.strip_prefix("HTTP/2 "))
            .or_else(|| {
                // A suffix such as ": HTTP 404" is no more trustworthy than
                // ": 404" unless it follows an explicit URL-bearing request.
                let (request, status) = line.rsplit_once(": ")?;
                let request_is_http = ["GET ", "POST ", "PUT ", "PATCH ", "DELETE "]
                    .iter()
                    .any(|method| {
                        request.strip_prefix(method).is_some_and(|url| {
                            (url.starts_with("https://") || url.starts_with("http://"))
                                && !url.chars().any(char::is_whitespace)
                        })
                    });
                request_is_http.then(|| {
                    status
                        .strip_prefix("HTTP ")
                        .or_else(|| status.strip_prefix("HTTP/1.1 "))
                        .or_else(|| status.strip_prefix("HTTP/2 "))
                        .unwrap_or(status)
                })
            })
            .or_else(|| from_glab.then_some(line))
            .unwrap_or("");
        let Some(code_text) = candidate.get(..3) else {
            continue;
        };
        if code_text.bytes().all(|b| b.is_ascii_digit())
            && candidate
                .as_bytes()
                .get(3)
                .is_none_or(|b| matches!(b, b' ' | b':' | b'\t'))
            && let Ok(code) = code_text.parse::<u16>()
            && (400..600).contains(&code)
        {
            return Some(code);
        }
    }
    None
}

pub(crate) fn glab_http_failure(stderr: &str, fallback: &str) -> GlabHttpFailure {
    let status = glab_http_status(stderr);
    GlabHttpFailure {
        status,
        safe_message: match status {
            Some(code) => format!("{fallback}: HTTP {code}"),
            None => trim_error(stderr, fallback),
        },
    }
}

/// Report a bounded, token-free reason from the typed GitLab CLI failure.
/// Never format the raw stderr or a URL. Keep unrelated provider diagnostics as-is.
pub(crate) fn safe_forge_failure(error: &anyhow::Error) -> String {
    let Some(failure) = error
        .chain()
        .find_map(|cause| cause.downcast_ref::<GlabHttpFailure>())
    else {
        return error.to_string();
    };
    let scope = if error.to_string() == "resolve GitLab project" {
        "GitLab project lookup"
    } else {
        "GitLab API"
    };
    let message = match failure.status {
        Some(401) => "authentication rejected (HTTP 401); check glab auth status".into(),
        Some(403) => "access denied (HTTP 403); check project permissions".into(),
        Some(404) => "resource not found (HTTP 404); check remote path and GitLab endpoint".into(),
        Some(status) => format!("request failed (HTTP {status})"),
        None => failure.to_string(),
    };
    format!("{scope}: {message}")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn contextual_gitlab_failures_show_safe_actionable_status() {
        for (stderr, expected) in [
            (
                "glab: 401 Unauthorized Bearer secret-token",
                "authentication rejected (HTTP 401)",
            ),
            (
                "glab: 403 Forbidden password=secret-token",
                "access denied (HTTP 403)",
            ),
            (
                "glab: 404 Not Found access_token=secret-token",
                "resource not found (HTTP 404)",
            ),
            ("glab: 502 Bad Gateway", "request failed (HTTP 502)"),
        ] {
            let error = anyhow::Error::new(glab_http_failure(stderr, "GitLab API request failed"))
                .context("resolve GitLab project");
            let visible = safe_forge_failure(&error);
            assert!(visible.starts_with("GitLab project lookup: "));
            assert!(visible.contains(expected), "{visible}");
            assert!(!visible.contains("secret-token"));
            assert!(!visible.contains("password="));
            assert!(!visible.contains("access_token="));
        }
        let unknown = anyhow::Error::new(glab_http_failure("unexpected secret-token", "provider"))
            .context("resolve GitLab project");
        assert_eq!(
            safe_forge_failure(&unknown),
            "GitLab project lookup: provider: error details withheld"
        );
        let unrelated = anyhow::anyhow!("ordinary Git error");
        assert_eq!(safe_forge_failure(&unrelated), "ordinary Git error");
    }
    #[test]
    fn glab_response_code_is_typed_and_not_inferred_from_path_ids() {
        assert_eq!(glab_http_status("glab: 404 Not Found"), Some(404));
        assert_eq!(
            glab_http_status(
                "glab: GET https://gitlab.example/api/v4/projects/1404/merge_requests/404: 403 Forbidden"
            ),
            Some(403)
        );
        assert_eq!(
            glab_http_status(
                "glab: GET https://gitlab.example/api/v4/projects/1404/merge_requests/404: permission denied"
            ),
            None
        );
        assert_eq!(glab_http_status("glab: HTTP/2 404 Not Found"), Some(404));
        // Arbitrary diagnostic numbers must not grant the legacy 404 fallback.
        for text in [
            "glab: network failed: 404 Not Found",
            "glab: network failed: HTTP 404 Not Found",
            "glab: request cancelled: HTTP 404",
            "connection refused: 404",
            "404 Not Found",
            "glab: GET https://gitlab.example/api/v4/projects/42: timeout: 404",
        ] {
            assert_eq!(glab_http_status(text), None, "{text:?}");
        }
        assert_eq!(
            glab_http_status(
                "glab: GET https://gitlab.example/api/v4/projects/42/merge_requests/7/approval_state: HTTP 404 Not Found"
            ),
            Some(404)
        );
        let failure = glab_http_failure("glab: HTTP/2 403 Forbidden Bearer secret", "glab");
        assert_eq!(failure.status, Some(403));
        assert!(!failure.to_string().contains("secret"));
        assert_eq!(
            glab_http_failure("Bearer secret: HTTP 404", "glab").status,
            None
        );
    }
}
