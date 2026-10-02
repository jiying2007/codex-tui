use anyhow::{Context, Result};
use serde_json::Value;

pub(crate) fn decode_wire_line(line: &str) -> Result<Value> {
    serde_json::from_str(line)
        .with_context(|| format!("decode app-server JSON line ({} bytes)", line.len()))
}
