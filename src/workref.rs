//! G2/G3 manual provenance references. No issue tracker, remote execution or delivery authority.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub mod cli;
#[cfg(test)]
mod tests;

pub const MAX_LEDGER_BYTES: usize = 512 * 1024;
pub const MAX_ITEMS: usize = 128;
const MAX_REFS: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkRefLedger {
    pub schema_version: u32,
    pub items: BTreeMap<String, WorkItem>,
}

impl Default for WorkRefLedger {
    fn default() -> Self {
        Self {
            schema_version: 1,
            items: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkItem {
    pub provider: String,
    pub space: String,
    pub kind: String,
    pub external_id: String,
    pub url: Option<String>,
    #[serde(default)]
    pub sessions: BTreeMap<String, SessionRef>,
    #[serde(default)]
    pub contracts: Vec<ContractRef>,
    #[serde(default)]
    pub evidence: Vec<EvidenceRef>,
    #[serde(default)]
    pub handoffs: Vec<HandoffRef>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionRef {
    pub target_identity: String,
    pub codex_home: String,
    pub thread_id: String,
    pub repository: String,
    pub role: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContractRef {
    pub kind: String,
    pub source: String,
    pub revision: String,
    pub git_sha: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EvidenceRef {
    pub kind: String,
    pub source: String,
    pub revision: String,
    pub result: String,
    pub environment: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HandoffRef {
    pub from_alias: String,
    pub to_alias: String,
    pub summary: String,
    /// Prepared is NOT transmitted. Acknowledged requires a manual receipt.
    pub status: String,
    pub receipt: Option<String>,
}

fn bounded(label: &str, value: &str, limit: usize) -> Result<()> {
    ensure!(
        !value.trim().is_empty() && value.len() <= limit && !value.chars().any(char::is_control),
        "{label} must be a nonempty single-line value within {limit} bytes"
    );
    Ok(())
}

fn one_of(label: &str, value: &str, allowed: &[&str]) -> Result<()> {
    ensure!(
        allowed.contains(&value),
        "unsupported {label}: use one of {}",
        allowed.join(", ")
    );
    Ok(())
}

fn credential_parameter(key: &str) -> bool {
    let key = key
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .map(|character| character.to_ascii_lowercase())
        .collect::<String>();
    matches!(
        key.as_str(),
        "token"
            | "accesstoken"
            | "refreshtoken"
            | "idtoken"
            | "authtoken"
            | "authorization"
            | "clientsecret"
            | "secret"
            | "password"
            | "session"
            | "sessionid"
            | "sid"
            | "cookie"
            | "signature"
            | "sig"
            | "jwt"
            | "credential"
            | "code"
            | "ticket"
            | "apikey"
    )
}

fn has_credential_parameter(url: &url::Url) -> bool {
    let query = url.query_pairs().any(|(key, _)| credential_parameter(&key));
    let fragment = url.fragment().is_some_and(|fragment| {
        let query = fragment
            .rsplit_once('?')
            .map_or(fragment, |(_, query)| query);
        url::form_urlencoded::parse(query.as_bytes()).any(|(key, _)| credential_parameter(&key))
    });
    query || fragment
}

fn https_reference(label: &str, value: &str, limit: usize) -> Result<()> {
    bounded(label, value, limit)?;
    let url = url::Url::parse(value)?;
    ensure!(
        url.scheme() == "https"
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none(),
        "{label} URL must be HTTPS without embedded credentials"
    );
    ensure!(
        !has_credential_parameter(&url),
        "{label} URL must not contain credential-like query or fragment parameters"
    );
    Ok(())
}

fn safe_reference(label: &str, value: &str, limit: usize) -> Result<()> {
    bounded(label, value, limit)?;
    if value.contains("://") {
        https_reference(label, value, limit)?;
    }
    Ok(())
}

fn key_valid(key: &str) -> Result<()> {
    bounded("local item key", key, 64)?;
    ensure!(
        key.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':')),
        "local item key must use ASCII letters, digits, _, - or :"
    );
    Ok(())
}

fn git_sha_valid(sha: &str) -> bool {
    sha.len() == 40 && sha.bytes().all(|byte| byte.is_ascii_hexdigit())
}

impl WorkItem {
    pub fn new(
        provider: String,
        space: String,
        kind: String,
        external_id: String,
        url: Option<String>,
    ) -> Result<Self> {
        let item = Self {
            provider,
            space,
            kind,
            external_id,
            url,
            sessions: BTreeMap::new(),
            contracts: Vec::new(),
            evidence: Vec::new(),
            handoffs: Vec::new(),
        };
        item.validate()?;
        Ok(item)
    }

    fn validate(&self) -> Result<()> {
        one_of(
            "item provider",
            &self.provider,
            &["feishu-project", "tapd", "local-draft"],
        )?;
        one_of(
            "item kind",
            &self.kind,
            &["requirement", "bug", "technical"],
        )?;
        bounded("workspace/space", &self.space, 160)?;
        bounded("external ID", &self.external_id, 160)?;
        if self.provider != "local-draft" {
            https_reference(
                "external item URL",
                self.url.as_deref().ok_or_else(|| {
                    anyhow::anyhow!("Feishu Project/TAPD require an explicit original HTTPS URL")
                })?,
                2048,
            )?;
        } else if let Some(url) = self.url.as_deref() {
            https_reference("external item URL", url, 2048)?;
        }
        ensure!(
            self.sessions.len() <= MAX_REFS
                && self.contracts.len() <= MAX_REFS
                && self.evidence.len() <= MAX_REFS
                && self.handoffs.len() <= MAX_REFS,
            "local reference limit exceeded"
        );
        for (alias, session) in &self.sessions {
            key_valid(alias)?;
            session.validate()?;
        }
        for contract in &self.contracts {
            contract.validate()?;
        }
        for evidence in &self.evidence {
            evidence.validate()?;
        }
        for handoff in &self.handoffs {
            handoff.validate(&self.sessions)?;
        }
        Ok(())
    }
}

impl SessionRef {
    fn validate(&self) -> Result<()> {
        for (label, value, limit) in [
            ("Target identity", self.target_identity.as_str(), 256),
            ("Codex home", self.codex_home.as_str(), 512),
            ("ThreadId", self.thread_id.as_str(), 160),
            ("repository", self.repository.as_str(), 512),
            ("session role", self.role.as_str(), 80),
        ] {
            bounded(label, value, limit)?;
        }
        Ok(())
    }
}

impl ContractRef {
    fn validate(&self) -> Result<()> {
        one_of(
            "contract kind",
            &self.kind,
            &[
                "protocol", "api", "abi", "schema", "config", "data", "model", "other",
            ],
        )?;
        safe_reference("contract authority/source", &self.source, 2048)?;
        bounded("contract revision", &self.revision, 160)?;
        if let Some(sha) = self.git_sha.as_deref() {
            ensure!(
                git_sha_valid(sha),
                "Git contract commit must be an exact 40-character SHA"
            );
        }
        Ok(())
    }
}

impl EvidenceRef {
    fn validate(&self) -> Result<()> {
        one_of(
            "evidence kind",
            &self.kind,
            &[
                "commit",
                "ci",
                "unit",
                "integration",
                "device",
                "acceptance",
                "other",
            ],
        )?;
        one_of(
            "evidence result",
            &self.result,
            &["passed", "failed", "partial", "unknown"],
        )?;
        safe_reference("evidence source", &self.source, 2048)?;
        bounded("evidence revision", &self.revision, 160)?;
        bounded("evidence environment", &self.environment, 256)?;
        Ok(())
    }
}

impl HandoffRef {
    fn validate(&self, sessions: &BTreeMap<String, SessionRef>) -> Result<()> {
        ensure!(
            self.from_alias != self.to_alias
                && sessions.contains_key(&self.from_alias)
                && sessions.contains_key(&self.to_alias),
            "handoff requires two distinct, previously linked session aliases"
        );
        bounded("handoff summary", &self.summary, 800)?;
        one_of(
            "handoff status",
            &self.status,
            &["prepared", "acknowledged", "unknown"],
        )?;
        if self.status == "acknowledged" {
            safe_reference(
                "manual acknowledgment receipt",
                self.receipt.as_deref().unwrap_or(""),
                2048,
            )?;
        } else {
            ensure!(
                self.receipt.is_none(),
                "receipt only valid for an acknowledged handoff"
            );
        }
        Ok(())
    }
}

impl WorkRefLedger {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.schema_version == 1,
            "unsupported work reference schema version"
        );
        ensure!(self.items.len() <= MAX_ITEMS, "too many linked work items");
        for (key, item) in &self.items {
            key_valid(key)?;
            item.validate()?;
        }
        Ok(())
    }

    pub fn add_item(&mut self, key: String, item: WorkItem) -> Result<()> {
        key_valid(&key)?;
        item.validate()?;
        if let Some(existing) = self.items.get(&key) {
            ensure!(
                existing.provider == item.provider
                    && existing.space == item.space
                    && existing.kind == item.kind
                    && existing.external_id == item.external_id
                    && existing.url == item.url,
                "item key already belongs to another external authority"
            );
            return Ok(());
        }
        ensure!(self.items.len() < MAX_ITEMS, "too many linked work items");
        self.items.insert(key, item);
        Ok(())
    }

    pub fn item_mut(&mut self, key: &str) -> Result<&mut WorkItem> {
        self.items.get_mut(key).ok_or_else(|| {
            anyhow::anyhow!("work item does not exist; add its exact external reference first")
        })
    }

    pub fn add_session(&mut self, key: &str, alias: String, session: SessionRef) -> Result<()> {
        key_valid(&alias)?;
        session.validate()?;
        let item = self.item_mut(key)?;
        if let Some(existing) = item.sessions.get(&alias) {
            ensure!(
                existing == &session,
                "session alias already refers to another Target/Thread"
            );
            return Ok(());
        }
        ensure!(item.sessions.len() < MAX_REFS, "too many sessions");
        item.sessions.insert(alias, session);
        Ok(())
    }

    pub fn add_contract(&mut self, key: &str, contract: ContractRef) -> Result<()> {
        contract.validate()?;
        let item = self.item_mut(key)?;
        if !item.contracts.contains(&contract) {
            ensure!(item.contracts.len() < MAX_REFS, "too many contracts");
            item.contracts.push(contract);
        }
        Ok(())
    }

    pub fn add_evidence(&mut self, key: &str, evidence: EvidenceRef) -> Result<()> {
        evidence.validate()?;
        let item = self.item_mut(key)?;
        if !item.evidence.contains(&evidence) {
            ensure!(
                item.evidence.len() < MAX_REFS,
                "too many evidence references"
            );
            item.evidence.push(evidence);
        }
        Ok(())
    }

    pub fn prepare_handoff(
        &mut self,
        key: &str,
        from_alias: String,
        to_alias: String,
        summary: String,
    ) -> Result<usize> {
        let item = self.item_mut(key)?;
        let candidate = HandoffRef {
            from_alias,
            to_alias,
            summary,
            status: "prepared".into(),
            receipt: None,
        };
        candidate.validate(&item.sessions)?;
        if let Some(index) = item.handoffs.iter().position(|h| {
            h.from_alias == candidate.from_alias
                && h.to_alias == candidate.to_alias
                && h.summary == candidate.summary
        }) {
            return Ok(index);
        }
        ensure!(item.handoffs.len() < MAX_REFS, "too many handoffs");
        item.handoffs.push(candidate);
        Ok(item.handoffs.len() - 1)
    }

    pub fn acknowledge_handoff(&mut self, key: &str, index: usize, receipt: String) -> Result<()> {
        let item = self.item_mut(key)?;
        let handoff = item
            .handoffs
            .get_mut(index)
            .ok_or_else(|| anyhow::anyhow!("handoff index not found"))?;
        safe_reference("manual acknowledgment receipt", &receipt, 2048)?;
        ensure!(
            handoff.status == "prepared"
                || (handoff.status == "acknowledged"
                    && handoff.receipt.as_deref() == Some(receipt.as_str())),
            "cannot overwrite a finalized handoff receipt"
        );
        handoff.status = "acknowledged".into();
        handoff.receipt = Some(receipt);
        Ok(())
    }
}
