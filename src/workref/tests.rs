use super::*;

fn item(provider: &str, kind: &str) -> WorkItem {
    WorkItem::new(
        provider.into(),
        "robot-project".into(),
        kind.into(),
        "item-17".into(),
        Some("https://www.tapd.cn/".into()),
    )
    .unwrap()
}

fn session(target: &str, repo: &str) -> SessionRef {
    SessionRef {
        target_identity: target.into(),
        codex_home: "/home/robot/.codex".into(),
        thread_id: "same-thread-uuid".into(),
        repository: repo.into(),
        role: "developer".into(),
    }
}

#[test]
fn genuine_feishu_or_tapd_item_needs_explicit_provider_space_id_and_url() {
    assert!(WorkItem::new(
        "tapd".into(), "robot".into(), "bug".into(), "55".into(), None
    ).is_err());
    assert!(WorkItem::new(
        "feishu-project".into(), "robot".into(), "requirement".into(), "55".into(),
        Some("http://example.org/".into())
    ).is_err());
    assert!(WorkItem::new(
        "feishu-project".into(), "robot".into(), "requirement".into(), "55".into(),
        Some("https://username:secret@example.org/".into())
    ).is_err());
    assert!(WorkItem::new(
        "local-draft".into(), "robot".into(), "technical".into(), "local-1".into(), None
    ).is_ok());
    let mut db = WorkRefLedger::default();
    db.add_item("BUG:17".into(), item("tapd", "bug")).unwrap();
    db.add_item("BUG:17".into(), item("tapd", "bug")).unwrap();
    assert_eq!(db.items.len(), 1);
    let wrong = WorkItem::new(
        "feishu-project".into(), "robot-project".into(), "requirement".into(),
        "item-17".into(), Some("https://project.feishu.cn/".into())
    ).unwrap();
    assert!(db.add_item("BUG:17".into(), wrong).is_err());
    assert!(db.add_item("../bad".into(), item("tapd", "bug")).is_err());
}

#[test]
fn two_repositories_and_targets_with_same_thread_id_remain_distinct() {
    let mut db = WorkRefLedger::default();
    db.add_item("BUG17".into(), item("tapd", "bug")).unwrap();
    db.add_session("BUG17", "soc".into(), session("target-A", "soc-app")).unwrap();
    db.add_session("BUG17", "mcu".into(), session("target-B", "mcu-fw")).unwrap();
    let work = &db.items["BUG17"];
    assert_eq!(work.sessions.len(), 2);
    assert_eq!(work.sessions["soc"].thread_id, work.sessions["mcu"].thread_id);
    assert_ne!(work.sessions["soc"].target_identity, work.sessions["mcu"].target_identity);
    assert!(db.add_session("BUG17", "soc".into(), session("target-C", "soc-app")).is_err());
    db.validate().unwrap();
}

#[test]
fn evidence_is_not_joint_success_and_handoff_is_never_automatically_delivered() {
    let mut db = WorkRefLedger::default();
    db.add_item("LINK".into(), item("tapd", "bug")).unwrap();
    db.add_session("LINK", "soc".into(), session("target-A", "soc-app")).unwrap();
    db.add_session("LINK", "mcu".into(), session("target-B", "mcu-fw")).unwrap();
    let sha = "a".repeat(40);
    let contract = ContractRef {
        kind: "protocol".into(),
        source: "soc-app/proto/wire.md".into(),
        revision: "rev-17".into(),
        git_sha: Some(sha),
    };
    db.add_contract("LINK", contract.clone()).unwrap();
    db.add_contract("LINK", contract).unwrap();
    assert_eq!(db.items["LINK"].contracts.len(), 1);
    let evidence = EvidenceRef {
        kind: "unit".into(),
        source: "https://github.com/example/ci/runs/7".into(),
        revision: "r7".into(),
        result: "passed".into(),
        environment: "ubuntu".into(),
    };
    db.add_evidence("LINK", evidence).unwrap();
    assert_eq!(db.items["LINK"].evidence.len(), 1);
    assert!(!db.items["LINK"].evidence.iter().any(|e| e.kind == "integration"));
    let n = db.prepare_handoff(
        "LINK", "soc".into(), "mcu".into(), "Review protocol revision r17".into()
    ).unwrap();
    assert_eq!(n, 0);
    assert_eq!(db.items["LINK"].handoffs[0].status, "prepared");
    assert!(db.items["LINK"].handoffs[0].receipt.is_none());
    assert!(db.acknowledge_handoff("LINK", 0, "".into()).is_err());
    db.acknowledge_handoff("LINK", 0, "https://example.com/explicit-ack".into()).unwrap();
    assert_eq!(db.items["LINK"].handoffs[0].status, "acknowledged");
    assert!(db.acknowledge_handoff("LINK", 0, "another-receipt".into()).is_err());
    assert!(db.prepare_handoff("LINK", "soc".into(), "missing".into(), "x".into()).is_err());
    db.validate().unwrap();
}

#[test]
fn contract_versions_and_integration_outcomes_fail_closed() {
    let invalid = ContractRef {
        kind: "api".into(),
        source: "robot-app/api/openapi.yaml".into(),
        revision: "branch-main".into(),
        git_sha: Some("deadbeef".into()),
    };
    assert!(invalid.validate().is_err());
    let invalid = EvidenceRef {
        kind: "integration".into(),
        source: "test-report".into(),
        revision: "1".into(),
        result: "green".into(),
        environment: "joint bench".into(),
    };
    assert!(invalid.validate().is_err());
    let mut ledger = WorkRefLedger::default();
    ledger.schema_version = 2;
    assert!(ledger.validate().is_err());
}
