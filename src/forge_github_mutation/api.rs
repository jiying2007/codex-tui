use crate::forge::{run_command, trim_error};
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::path::Path;

pub(super) async fn gh_api_json<T>(cwd: &str, host: &str, endpoint: &str) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let output = run_command(
        "gh",
        &["api", "--hostname", host, endpoint],
        Some(Path::new(cwd)),
    )
    .await
    .with_context(|| format!("run gh api {endpoint}"))?;
    if !output.success {
        bail!(
            "gh api {endpoint} failed: {}",
            trim_error(&output.stderr, "unknown gh error")
        );
    }
    serde_json::from_str(&output.stdout)
        .with_context(|| format!("decode gh api response for {endpoint}"))
}

pub(super) async fn gh_api_mutation_json<T>(
    cwd: &str,
    host: &str,
    method: &str,
    endpoint: &str,
    fields: &[(&str, &str)],
) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let mut args = vec![
        "api".to_string(),
        "--hostname".to_string(),
        host.to_string(),
        "--method".to_string(),
        method.to_string(),
        endpoint.to_string(),
    ];
    for (key, value) in fields {
        args.push("-f".into());
        args.push(format!("{key}={value}"));
    }
    let borrowed = args.iter().map(String::as_str).collect::<Vec<_>>();
    let output = run_command("gh", &borrowed, Some(Path::new(cwd)))
        .await
        .with_context(|| format!("run GitHub mutation {method} {endpoint}"))?;
    if !output.success {
        bail!(
            "GitHub mutation {method} {endpoint} failed: {}",
            trim_error(&output.stderr, "unknown gh error")
        );
    }
    serde_json::from_str(&output.stdout)
        .with_context(|| format!("decode GitHub mutation response for {endpoint}"))
}
