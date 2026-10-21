#!/usr/bin/env python3
"""Smoke the final binary against local TLS peers, with certificate checks intact."""
import base64
import hashlib
import json
import os
from pathlib import Path
import socket
import ssl
import struct
import subprocess
import sys
import tempfile
import threading


FIXTURE_TOKEN = "fixture-bearer-value"


def require(value, message):
    if not value:
        raise RuntimeError(message)


def certificates(root):
    def openssl(*args):
        subprocess.run(
            ["openssl", *args], cwd=root, check=True,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=15,
        )

    for name in ("ca", "untrusted"):
        openssl(
            "req", "-x509", "-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:prime256v1",
            "-nodes", "-keyout", name + ".key", "-out", name + ".pem", "-days", "1",
            "-subj", "/CN=codex-tui fixture " + name,
            "-addext", "basicConstraints=critical,CA:TRUE",
            "-addext", "keyUsage=critical,keyCertSign,cRLSign",
        )
    openssl(
        "req", "-new", "-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:prime256v1",
        "-nodes", "-keyout", "server.key", "-out", "server.csr", "-subj", "/CN=localhost",
    )
    (root / "server.ext").write_text(
        "basicConstraints=critical,CA:FALSE\n"
        "keyUsage=critical,digitalSignature\n"
        "extendedKeyUsage=serverAuth\nsubjectAltName=DNS:localhost\n", encoding="utf-8",
    )
    openssl(
        "x509", "-req", "-in", "server.csr", "-CA", "ca.pem", "-CAkey", "ca.key",
        "-CAcreateserial", "-out", "server.pem", "-days", "1", "-extfile", "server.ext",
    )


def receive(stream, count):
    data = bytearray()
    while len(data) < count:
        chunk = stream.recv(count - len(data))
        if not chunk:
            raise EOFError("fixture peer closed")
        data.extend(chunk)
    return bytes(data)


def read_frame(stream):
    first, second = receive(stream, 2)
    require(first & 0x80, "fragmented fixture request")
    require(second & 0x80, "client frame must be masked")
    length = second & 0x7F
    if length == 126:
        length = struct.unpack("!H", receive(stream, 2))[0]
    elif length == 127:
        length = struct.unpack("!Q", receive(stream, 8))[0]
    require(length <= 65536, "fixture request exceeds bound")
    mask = receive(stream, 4)
    payload = receive(stream, length)
    return first & 0x0F, bytes(value ^ mask[index % 4] for index, value in enumerate(payload))


def send_json(stream, value):
    payload = json.dumps(value, separators=(",", ":")).encode("utf-8")
    require(len(payload) < 126, "fixture response exceeds short frame")
    stream.sendall(bytes((0x81, len(payload))) + payload)


def serve(listener, context, mode, events, failures):
    try:
        events.append("waiting-for-tcp")
        connection, _ = listener.accept()
        with connection:
            events.append("tcp-accepted")
            connection.settimeout(6)
            if mode == "peer-close":
                events.append("tcp-closed-before-tls")
                return
            try:
                secured = context.wrap_socket(connection, server_side=True)
            except ssl.SSLError:
                if mode in ("unknown-ca", "wrong-host"):
                    events.append("certificate-rejected")
                    return
                raise
            with secured:
                events.append(secured.version())
                headers = bytearray()
                while not headers.endswith(b"\r\n\r\n"):
                    headers.extend(receive(secured, 1))
                    require(len(headers) <= 16384, "oversized fixture upgrade")
                lines = headers.decode("ascii").split("\r\n")
                fields = {
                    name.lower(): value.strip()
                    for name, value in (line.split(":", 1) for line in lines[1:] if ":" in line)
                }
                require(fields.get("authorization") == "Bearer " + FIXTURE_TOKEN, "missing bearer")
                key = fields["sec-websocket-key"] + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"
                accept = base64.b64encode(hashlib.sha1(key.encode("ascii")).digest()).decode("ascii")
                secured.sendall((
                    "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\n"
                    "Connection: Upgrade\r\nSec-WebSocket-Accept: " + accept + "\r\n\r\n"
                ).encode("ascii"))
                for _ in range(12):
                    try:
                        opcode, payload = read_frame(secured)
                    except EOFError:
                        return
                    if opcode == 8:
                        return
                    require(opcode == 1, "expected JSON text frame")
                    request = json.loads(payload)
                    method = request.get("method")
                    events.append(method)
                    if "id" not in request:
                        continue
                    if method == "initialize":
                        result = {"serverInfo": {"name": "tls-fixture", "version": "0.0.0"}}
                    elif method in ("thread/list", "thread/loaded/list"):
                        result = {"data": [], "nextCursor": None}
                    else:
                        raise RuntimeError("unexpected TLS fixture method: " + str(method))
                    send_json(secured, {"id": request["id"], "result": result})
                raise RuntimeError("fixture request count exceeded")
    except Exception as error:
        failures.append(repr(error))


def run_case(binary, output, root, context, mode):
    case = root / mode
    config = case / "config" / "codex-tui"
    config.mkdir(parents=True)
    empty_roots = case / "empty-roots"
    empty_roots.mkdir()
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        listener.listen(1)
        listener.settimeout(8)
        host = "127.0.0.1" if mode == "wrong-host" else "localhost"
        url = "wss://{}:{}/rpc".format(host, listener.getsockname()[1])
        (config / "config.toml").write_text(
            '[app_server]\nactive = "tls-fixture"\n'
            '[app_server.targets.tls-fixture]\ntransport = "websocket"\n'
            'url = "' + url + '"\nauth_token_env = "CODEX_TUI_TLS_TEST_TOKEN"\n', encoding="utf-8",
        )
        env = os.environ.copy()
        env.update({
            "XDG_CONFIG_HOME": str(case / "config"), "XDG_DATA_HOME": str(case / "data"),
            "XDG_CACHE_HOME": str(case / "cache"), "XDG_STATE_HOME": str(case / "state"),
            "SSL_CERT_FILE": str(root / ("untrusted.pem" if mode == "unknown-ca" else "ca.pem")),
            "SSL_CERT_DIR": str(empty_roots), "CODEX_TUI_TLS_TEST_TOKEN": FIXTURE_TOKEN,
        })
        events, failures = [], []
        server = threading.Thread(target=serve, args=(listener, context, mode, events, failures), daemon=True)
        server.start()
        command = [str(binary), "doctor", "codex", "--target", "tls-fixture"]
        process_error = None
        try:
            completed = subprocess.run(
                command, cwd=case, env=env, stdin=subprocess.DEVNULL,
                stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=10,
            )
        except (OSError, subprocess.TimeoutExpired) as error:
            process_error = repr(error)
            # No exit status was returned. Preserve any output captured before
            # a timeout without misreporting it as a normal degraded exit.
            completed = subprocess.CompletedProcess(
                command, None, getattr(error, "output", None) or b"",
                getattr(error, "stderr", None) or b"",
            )
        server.join(timeout=8)

        def redacted(value):
            return value.decode("utf-8", errors="replace").replace(FIXTURE_TOKEN, "<redacted-fixture-token>")

        report = {
            "case": mode, "url": url, "listener": listener.getsockname(),
            "configPath": str(config / "config.toml"), "trustRoot": env["SSL_CERT_FILE"],
            "python": sys.version, "openssl": ssl.OPENSSL_VERSION,
            "returncode": completed.returncode, "processError": process_error,
            "stdout": redacted(completed.stdout), "stderr": redacted(completed.stderr),
            "events": list(events), "errors": list(failures), "serverAlive": server.is_alive(),
        }
        diagnostic = json.dumps(report, indent=2).replace(FIXTURE_TOKEN, "<redacted-fixture-token>")
        (output / (mode + ".stdout.txt")).write_text(report["stdout"], encoding="utf-8")
        (output / (mode + ".stderr.txt")).write_text(report["stderr"], encoding="utf-8")
        (output / (mode + ".server.json")).write_text(diagnostic + "\n", encoding="utf-8")

        def check(value, message):
            require(value, mode + ": " + message + "\n" + diagnostic)

        check(process_error is None, "doctor process did not complete")
        check(not server.is_alive(), "fixture thread did not finish")
        check(not failures, "fixture failure")
    text = (completed.stdout + completed.stderr).decode("utf-8", errors="replace")
    check("panicked" not in text and FIXTURE_TOKEN not in text, "panic or token disclosure")
    if mode == "trusted":
        check(completed.returncode == 0, "doctor failed")
        check("connected: true" in text and "thread/list" in events, "TLS did not carry real RPC")
        check(any(event in ("TLSv1.2", "TLSv1.3") for event in events), "TLS was not negotiated")
    else:
        check(completed.returncode == 3 and "connected: false" in text, "failure was not degraded")
        check("initialize" not in events, "application data sent after rejected TLS")
        if mode in ("unknown-ca", "wrong-host"):
            # rustls may render certificate failures as enum names or user-facing prose.
            markers = (("unknownissuer", "unknown issuer") if mode == "unknown-ca"
                       else ("notvalidforname", "certificate not valid for name"))
            check(any(marker in text.lower() for marker in markers), "missing certificate diagnostic")
    return {"case": mode, "passed": True, "exitCode": completed.returncode, "events": events}


def main():
    binary, output = (Path(value).resolve() for value in sys.argv[1:])
    output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="codex-tui-tls-") as temporary:
        root = Path(temporary)
        certificates(root)
        context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        context.load_cert_chain(root / "server.pem", root / "server.key")
        cases = [run_case(binary, output, root, context, mode)
                 for mode in ("trusted", "unknown-ca", "wrong-host", "peer-close")]
    receipt = {"schema": "codex-tui/wss-transport-smoke/v1", "status": "passed", "cases": cases,
               "sourceSha": os.environ.get("CODEX_TUI_GIT_SHA") or os.environ.get("GITHUB_SHA"),
               "binarySha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
               "realCodexQualification": False}
    (output / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(receipt))


if __name__ == "__main__":
    main()
