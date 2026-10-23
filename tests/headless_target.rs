//! Actual CLI target selection, error precedence and honest local-store scope.
#![cfg(target_os = "linux")]

use codex_tui::{
    sqlite_store::SqliteStore,
    store::{LocalStateV1, LocalStore},
};
use serde_json::{Value, json};
use std::{
    net::{SocketAddr, TcpListener},
    process::{Command, Output, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};
use tempfile::TempDir;
use tokio_tungstenite::tungstenite::{self, Message};

struct Target {
    address: SocketAddr,
    connections: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    task: Option<JoinHandle<()>>,
}

impl Target {
    fn new(id: &str, title: &str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let connections = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let task_stop = stop.clone();
        let task_connections = connections.clone();
        let row = json!({"id": id, "name": title, "preview": title, "cwd": "/fixture/project", "createdAt": 1, "updatedAt": 2, "status": {"type": "idle"}});
        let task = thread::spawn(move || {
            while !task_stop.load(Ordering::SeqCst) {
                let stream = match listener.accept() {
                    Ok((stream, _)) => stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2));
                        continue;
                    }
                    Err(error) => panic!("accept fixture: {error}"),
                };
                task_connections.fetch_add(1, Ordering::SeqCst);
                stream
                    .set_read_timeout(Some(Duration::from_millis(200)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut socket = tungstenite::accept(stream).unwrap();
                while !task_stop.load(Ordering::SeqCst) {
                    let message = match socket.read() {
                        Ok(Message::Text(message)) => message,
                        Ok(Message::Close(_)) => break,
                        Ok(_) => continue,
                        Err(tungstenite::Error::Io(error))
                            if matches!(
                                error.kind(),
                                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                            ) =>
                        {
                            continue;
                        }
                        Err(_) => break,
                    };
                    let request: Value = serde_json::from_str(message.as_str()).unwrap();
                    let Some(request_id) = request.get("id") else {
                        continue;
                    };
                    let result = match request["method"].as_str().unwrap() {
                        "initialize" => json!({"serverInfo": {"version": "fixture"}}),
                        "thread/list" => json!({"data": [row.clone()], "nextCursor": null}),
                        "thread/loaded/list" => json!({"data": [], "nextCursor": null}),
                        other => panic!("unexpected RPC {other}"),
                    };
                    socket
                        .send(Message::Text(
                            json!({"id": request_id, "result": result})
                                .to_string()
                                .into(),
                        ))
                        .unwrap();
                }
            }
        });
        Self {
            address,
            connections,
            stop,
            task: Some(task),
        }
    }
}

impl Drop for Target {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.task.take().unwrap().join().unwrap();
    }
}

fn config(home: &TempDir, active: &str, first: &Target, second: &Target) -> String {
    let text = format!(
        "[app_server]\nactive = {active:?}\n[app_server.targets.first]\ntransport = \"websocket\"\nurl = \"ws://{}\"\n[app_server.targets.second]\ntransport = \"websocket\"\nurl = \"ws://{}\"\n",
        first.address, second.address
    );
    let directory = home.path().join("config/codex-tui");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("config.toml"), &text).unwrap();
    text
}

fn invoke(home: &TempDir, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_codex-tui"))
        .args(args)
        .current_dir(home.path())
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path().join("config"))
        .env("XDG_DATA_HOME", home.path().join("data"))
        .env("XDG_STATE_HOME", home.path().join("state"))
        .env("CODEX_HOME", home.path().join("codex"))
        .env("PATH", home.path())
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn report(home: &TempDir, args: &[&str], exit: i32) -> Value {
    let output = invoke(home, args);
    assert_eq!(output.status.code(), Some(exit), "{args:?}: {output:?}");
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| panic!("{error}: {output:?}"))
}

fn source(value: &Value, name: &str) {
    assert_eq!(value["source"]["targetId"], name);
    assert_eq!(value["source"]["transport"], "ws");
    assert_eq!(value["source"]["scope"], "single-target");
}

#[test]
fn configured_selection_and_override_contact_only_the_selected_target() {
    let home = TempDir::new().unwrap();
    let first = Target::new("first-id", "First");
    let second = Target::new("second-id", "Second");
    let original = config(&home, "second", &first, &second);
    let active = report(&home, &["headless", "threads", "--json"], 0);
    source(&active, "second");
    assert_eq!(active["threads"][0]["id"], "second-id");
    assert_eq!(first.connections.load(Ordering::SeqCst), 0);
    assert_eq!(second.connections.load(Ordering::SeqCst), 1);
    let override_value = report(&home, &["thread", "list", "--target", "first", "--json"], 0);
    source(&override_value, "first");
    assert_eq!(override_value["threads"][0]["id"], "first-id");
    assert_eq!(first.connections.load(Ordering::SeqCst), 1);
    assert_eq!(second.connections.load(Ordering::SeqCst), 1);
    assert_eq!(
        std::fs::read_to_string(home.path().join("config/codex-tui/config.toml")).unwrap(),
        original
    );
    assert!(!home.path().join("data").exists());
}

#[test]
fn unknown_active_or_override_fails_before_connection_or_store_loading() {
    let home = TempDir::new().unwrap();
    let first = Target::new("first-id", "First");
    let second = Target::new("second-id", "Second");
    config(&home, "missing", &first, &second);
    for args in [
        vec!["headless", "work", "--json"],
        vec!["status", "--target", "other", "--json"],
    ] {
        let value = report(&home, &args, 3);
        assert_eq!(value["localStoreScope"], "not-loaded");
        assert!(
            value["source"]["error"]
                .as_str()
                .unwrap()
                .contains("unknown App Server target")
        );
        assert!(value["localStoreError"].is_null());
        assert!(!home.path().join("data").exists());
    }
    assert_eq!(first.connections.load(Ordering::SeqCst), 0);
    assert_eq!(second.connections.load(Ordering::SeqCst), 0);
}

#[test]
fn missing_config_uses_local_identity_without_materializing_defaults() {
    let home = TempDir::new().unwrap();
    let value = report(&home, &["headless", "threads", "--json"], 3);
    assert_eq!(value["source"]["targetId"], "local");
    assert_eq!(value["source"]["transport"], "stdio");
    assert_eq!(value["source"]["scope"], "single-target");
    assert!(!value["source"]["connected"].as_bool().unwrap());
    assert_eq!(std::fs::read_dir(home.path()).unwrap().count(), 0);
}

#[test]
fn malformed_config_is_degraded_but_explicit_fixture_does_not_read_it() {
    let home = TempDir::new().unwrap();
    let directory = home.path().join("config/codex-tui");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("config.toml"), "[broken").unwrap();
    let value = report(&home, &["headless", "work", "--json"], 3);
    assert_eq!(value["localStoreScope"], "not-loaded");
    assert!(
        value["source"]["error"]
            .as_str()
            .unwrap()
            .contains("parse config")
    );
    let fixture = report(&home, &["headless", "work", "--fake", "--json"], 0);
    assert_eq!(fixture["source"]["scope"], "fixture");
    assert_eq!(fixture["localStoreScope"], "fixture");
    assert!(!home.path().join("data").exists());
    assert_eq!(
        std::fs::read_to_string(directory.join("config.toml")).unwrap(),
        "[broken"
    );
}

#[test]
fn unreachable_selected_target_retains_identity_and_text_diagnostic() {
    let home = TempDir::new().unwrap();
    let unused = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = unused.local_addr().unwrap();
    drop(unused);
    let directory = home.path().join("config/codex-tui");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("config.toml"), format!("[app_server]\nactive = \"alternate\"\n[app_server.targets.alternate]\ntransport = \"websocket\"\nurl = \"ws://{address}\"\n")).unwrap();
    let value = report(&home, &["headless", "threads", "--json"], 3);
    source(&value, "alternate");
    assert_eq!(value["source"]["connected"], false);
    assert!(value["source"]["error"].as_str().is_some());
    let output = invoke(&home, &["attention", "list"]);
    assert_eq!(output.status.code(), Some(3));
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("target: alternate transport=ws scope=single-target"));
    assert!(text.contains("source-error:"));
    assert!(text.contains("local-store-scope: shared-across-targets"));
}

#[test]
fn every_codex_work_surface_preserves_target_and_shared_local_scope() {
    let home = TempDir::new().unwrap();
    let first = Target::new("first-id", "First");
    let second = Target::new("second-id", "Second");
    config(&home, "first", &first, &second);
    for command in ["work", "status", "attention", "board"] {
        let value = report(
            &home,
            &["headless", command, "--target", "second", "--json"],
            0,
        );
        source(&value, "second");
        assert_eq!(value["localStoreScope"], "shared-across-targets");
        assert_eq!(value["degraded"], false);
    }
    assert_eq!(first.connections.load(Ordering::SeqCst), 0);
    assert_eq!(second.connections.load(Ordering::SeqCst), 4);
}

#[test]
fn same_id_targets_truthfully_share_existing_aliases_without_inventory_overlay() {
    let home = TempDir::new().unwrap();
    let first = Target::new("shared-id", "First");
    let second = Target::new("shared-id", "Second");
    config(&home, "first", &first, &second);
    let seed = home.path().join("seed");
    let store = SqliteStore::at(&seed);
    let mut local = LocalStateV1::default();
    local
        .aliases
        .insert("shared-id".into(), "Shared local alias".into());
    local.pins.insert("shared-id".into());
    store.save_state(&local).unwrap();
    drop(store);
    std::fs::create_dir_all(home.path().join("data")).unwrap();
    std::fs::rename(seed.join("state"), home.path().join("data/codex-tui")).unwrap();
    for target in ["first", "second"] {
        let work = report(
            &home,
            &["headless", "work", "--target", target, "--json"],
            0,
        );
        source(&work, target);
        assert_eq!(work["localStoreScope"], "shared-across-targets");
        assert_eq!(work["work"][0]["title"], "Shared local alias");
    }
    let inventory = report(
        &home,
        &["thread", "list", "--target", "second", "--json"],
        0,
    );
    assert_eq!(inventory["threads"][0]["title"], "Second");
    assert_eq!(inventory["threads"][0]["pinned"], false);
    assert!(inventory.get("localStoreScope").is_none());
}
