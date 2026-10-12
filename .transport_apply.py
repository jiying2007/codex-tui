from pathlib import Path
import subprocess
root=Path('.')
def edit(name,old,new):
    p=root/name; s=p.read_text(encoding='utf-8')
    if s.count(old)!=1: raise RuntimeError(f'{name}: non-unique patch target {old[:80]!r}')
    p.write_text(s.replace(old,new,1),encoding='utf-8')
for name,expected in [('src/app_server.rs','0ea5d997dd05407cdc72c1099e8dd339fcedd0b3'),('src/app_server_transport.rs','6dd3e1e3a8a08d6f95874a6cb5cebbeb2aa919a5')]:
    actual=subprocess.check_output(['git','hash-object',name],text=True).strip()
    if actual!=expected: raise RuntimeError(f'{name}: base blob mismatch')
edit('src/app_server_transport.rs','pub struct AppServerTransport {\n    inner: Transport,\n}', 'mod health;\nuse health::TransportHealth;\n\npub struct AppServerTransport {\n    inner: Transport,\n    health: TransportHealth,\n}')
edit('src/app_server_transport.rs','const APP_SERVER_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);','const APP_SERVER_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);\nconst APP_SERVER_WRITE_TIMEOUT: Duration = Duration::from_secs(5);')
edit('src/app_server_transport.rs','Ok(Self { inner })','Ok(Self { inner, health: TransportHealth::default() })')
edit('src/app_server_transport.rs','    pub async fn read_json(&mut self) -> Result<Option<Value>> {\n        match &mut self.inner {','    pub(crate) fn is_quarantined(&self) -> bool {\n        self.health.is_quarantined()\n    }\n\n    pub async fn read_json(&mut self) -> Result<Option<Value>> {\n        self.health.check()?;\n        let result = match &mut self.inner {')
edit('src/app_server_transport.rs','            Transport::UnixSocket { stream } => read_websocket_json(stream.as_mut()).await,\n        }\n    }','            Transport::UnixSocket { stream } => read_websocket_json(stream.as_mut()).await,\n        };\n        self.health.observe_read(&result);\n        result\n    }')
p=root/'src/app_server_transport.rs';s=p.read_text();start=s.index('    pub async fn write_json(');end=s.index('\nasync fn read_stdio_json',start)
s=s[:start]+'''    pub async fn write_json(&mut self, value: &Value) -> Result<()> {
        self.health.write(APP_SERVER_WRITE_TIMEOUT, async {
            match &mut self.inner {
                Transport::Stdio { writer, .. } => write_stdio_json(writer.as_mut(), value).await,
                Transport::WebSocket { stream } => write_websocket_json(stream.as_mut(), value).await,
                #[cfg(unix)]
                Transport::UnixSocket { stream } => write_websocket_json(stream.as_mut(), value).await,
            }
        }).await
    }
}

async fn write_stdio_json<W: tokio::io::AsyncWrite + Unpin>(writer: &mut W, value: &Value) -> Result<()> {
    let mut encoded = serde_json::to_vec(value).context("encode App Server stdio JSON")?;
    encoded.push(b'\\n');
    writer.write_all(&encoded).await.context("write App Server stdio")?;
    writer.flush().await.context("flush App Server stdio")
}
'''+s[end:]
s+='\n#[cfg(all(test, target_os = "linux"))]\nmod cancelled_write_test;\n';p.write_text(s)
edit('src/app_server.rs','            status.error = Some(format!("Loaded inventory incomplete: {error}"));\n        }\n    }\n\n    Ok(BackendSnapshot','            status.error = Some(format!("Loaded inventory incomplete: {error}"));\n            status.connected = !rpc.transport.is_quarantined();\n        }\n    }\n\n    Ok(BackendSnapshot')
edit('src/app_server.rs','        self.fatal_error.is_some() || !self.queued_messages.is_empty()','        self.fatal_error.is_some() || self.transport.is_quarantined() || !self.queued_messages.is_empty()')
edit('src/app_server.rs','        if let Some(message) = self.queued_messages.pop_front() {','        if self.transport.is_quarantined() {\n            return self.read_wire_message().await;\n        }\n        if let Some(message) = self.queued_messages.pop_front() {')
p=root/'src/app_server.rs';p.write_text(p.read_text()+'\n#[cfg(test)]\nmod transport_fault_tests;\n')
edit('src/app_server/transport_fault_tests.rs','            if let Some(snapshot) = started.handle.try_recv() {\n                if !snapshot.status.connected { break snapshot; }\n            }','            if let Some(snapshot) = started.handle.try_recv()\n                && !snapshot.status.connected { break snapshot; }')
edit('src/app_server_transport/cancelled_write_test.rs','observed.write_text(str(len(first)))','observed.with_suffix(".tmp").write_text(str(len(first)))\nos.replace(observed.with_suffix(".tmp"), observed)')
edit('scripts/testing/app_server_transport_regression.py',"    elif method == 'thread/loaded/list':\n        result","    elif method == 'thread/loaded/list':\n        if mode == 'loaded-eof':\n            os._exit(0)\n        if mode == 'loaded-malformed':\n            os.write(1, b'{invalid-json}\\n')\n            time.sleep(1)\n            continue\n        result")
p=root/'scripts/testing/app_server_transport_regression.py';s=p.read_text();idx=s.index('\ndef main():');s=s[:idx]+'''
def probe_fault_case(binary, output, mode):
    log = output / (mode + '.server.log')
    with tempfile.TemporaryDirectory(prefix='ctui-probe-fault-') as temporary:
        home = Path(temporary)
        process = subprocess.Popen([str(binary), 'headless', 'threads', '--json'],
                                   stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                   env=environment(home, log, mode), cwd=home, start_new_session=True)
        try:
            stdout, stderr = process.communicate(timeout=8)
        finally:
            stop(process)
        (output / (mode + '.json')).write_bytes(stdout)
        result = json.loads(stdout)
        require(process.returncode == 3 and result.get('degraded') is True, mode + ': fault not degraded')
        require(result.get('source', {}).get('connected') is False, mode + ': broken wire reported connected')
        require(bool(result.get('source', {}).get('error')), mode + ': missing fault diagnostic')
        require(len(result.get('threads', [])) == 1, mode + ': valid partial inventory was discarded')
        require(not stderr, mode + ': unexpected process error')
    return {'mode':mode, 'passed':True, 'connected':False, 'partialRows':1}

'''+s[idx:];p.write_text(s)
edit('scripts/testing/app_server_transport_regression.py',"        receipt['status'] = 'passed'","        for mode in ('loaded-eof', 'loaded-malformed'):\n            receipt['checks'].append(probe_fault_case(binary, output, mode))\n        receipt['status'] = 'passed'")
edit('tests/app_server_transport_regression.rs','receipt["checks"].as_array().unwrap().len(), 5','receipt["checks"].as_array().unwrap().len(), 7')
notes={
'docs/guides/troubleshooting.md':'A cancelled or failed outbound write quarantines the connection: partial delivery is unknown, so later requests cannot append to an unfinished frame and writes are never automatically replayed. Every outbound write also has a five-second deadline, including notifications. Wire read/decode failure or EOF during an RPC cannot be misreported as a healthy connection. A valid optional-method RPC rejection and cancellation while merely waiting for a response do not poison an otherwise intact wire.',
'docs/zh-CN/guides/troubleshooting.md':'发送被取消或失败后，连接进入隔离状态：部分交付结果未知，后续请求不能接到未完成帧上，也不会自动重放写操作。包括通知在内的每次发送都受五秒截止时间限制。RPC 期间的底层读取、JSON 解码失败或 EOF 不能再被报告为连接正常。合法的可选方法拒绝，以及仅等待响应时被取消，不会隔离本来完整的连接。'
}
for name,note in notes.items():
    p=root/name;s=p.read_text();at=s.index('## GitLab/GitHub');p.write_text(s[:at]+note+'\n\n'+s[at:])
p=root/'CHANGELOG.md';s=p.read_text();at=s.index('### Audit hardening\n')+len('### Audit hardening\n');p.write_text(s[:at]+'\n- Quarantine cancelled/failed outbound frames and broken read/decode/EOF transports; bound every write, preserve healthy RPC rejections/response-wait cancellation, and report disconnected headless probes truthfully. Add deterministic partial-write/flush regressions, local wire fault tests and two packaged-binary probe cases.\n'+s[at:])
# Preserve the existing actor Unknown/no-retry assertions, but isolate a real
# sink failure rather than pre-quarantining the wire by observing inbound EOF.
p=root/'src/app_server_transport/tests.rs'
p.write_text(p.read_text()+'''\n// Test-only real-sink fault injection; never compiled into a production binary.
impl AppServerTransport {
    pub(crate) async fn close_websocket_writer_for_test(&mut self) -> Result<()> {
        match &mut self.inner {
            Transport::WebSocket { stream } => stream.close(None).await.context("close fixture writer"),
            _ => anyhow::bail!("fixture requires WebSocket"),
        }
    }
}
''')
name='src/app_server/user_response/approval_tests.rs'
edit(name,'    use futures_util::SinkExt;','    use futures_util::{SinkExt, StreamExt};')
edit(name,'            ws.close(None).await.unwrap();','            let close = ws.next().await.unwrap().unwrap();\n            assert!(close.is_close());')
edit(name,'        assert!(rpc.read_wire_message().await.unwrap().is_none());','        rpc.transport.close_websocket_writer_for_test().await.unwrap();')
edit(name,'        // The real transport has consumed Close. Keep the observed request queued,\n        // making SendAfterClosing deterministic rather than relying on TCP timing.','        // Only the real sink is closed by a test-only helper. No inbound EOF has\n        // been observed: that is now quarantined earlier and covered separately.\n        // SendAfterClosing still exercises the production Unknown/no-retry path.')
subprocess.run(['cargo','fmt','--all'],check=True)
import json
p=root/'release/v1.4-plan.json'; plan=json.loads(p.read_text())
modules=['src/app_server_transport/tests.rs',name]
for module in modules: plan['moduleRatchet'][module]=len((root/module).read_text().splitlines())
plan['ratchetReviews'].append({'date':'2026-10-12','kind':'defect-and-security-fix','scope':'Retain existing actor write-failure Unknown/disconnect/no-retry assertions with a deterministic test-only real WebSocket sink-close fixture; inbound read quarantine is covered independently. No assertion removal or production fault injection.','modules':modules})
p.write_text(json.dumps(plan,indent=2)+'\n')
