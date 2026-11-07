# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
"""Helpers for the Python e2e checks of terminumd (standard library only): a private
terminumd (its own state folders, port and tmux socket, never the user's), its CLI socket,
a sign-in with the password, a page's WebSocket with its Origin and sign-in, and the
checks' record and summary."""
import base64
import hashlib
import hmac
import http.client
import json
import os
import shutil
import signal
import socket
import struct
import subprocess
import sys
import tempfile
import time
import urllib.request

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from testrun import short_tmp  # noqa: E402


def _ended(sig, _frame):
    """A run ended by a signal (a timeout, a closed terminal) still runs its `finally` blocks, so
    it ends the terminumd, tmux servers and ssh stand-ins it started (CLAUDE.md §6)."""
    sys.exit(128 + sig)


for _sig in (signal.SIGTERM, signal.SIGHUP):
    signal.signal(_sig, _ended)

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BIN = os.environ.get("TERMINUMD_BIN", os.path.join(ROOT, "terminumd", "target", "debug", "terminumd"))
results = []


def check(name, ok, extra=""):
    results.append(bool(ok))
    print(f"{'PASS' if ok else 'FAIL'} {name}{' — ' + str(extra) if extra else ''}")


def summary():
    """Prints "N/M passed" and exits 0 when there were checks and all passed, else 1."""
    print(f"{sum(results)}/{len(results)} passed")
    sys.exit(0 if results and all(results) else 1)


def tmux_on(socket_name, *args):
    """What tmux prints for args on the private server `socket_name`."""
    return subprocess.run(["tmux", "-L", socket_name, *args], capture_output=True, text=True).stdout


def end_tmux(socket_name):
    """Ends the private server `socket_name` and its panes' programs: a login shell of macOS's bash
    can outlive kill-server, its terminal gone, ignoring SIGHUP and spinning (a Dispatcher's window)."""
    pids = [int(p) for p in tmux_on(socket_name, "list-panes", "-a", "-F", "#{pane_pid}").split() if p.isdigit()]
    subprocess.run(["tmux", "-L", socket_name, "kill-server"], stderr=subprocess.DEVNULL, check=False)
    end = time.monotonic() + 2
    while pids and time.monotonic() < end:
        time.sleep(0.1)
        pids = [p for p in pids if subprocess.run(["ps", "-o", "stat=", "-p", str(p)], capture_output=True, text=True).stdout.strip()[:1] not in ("", "Z")]
    for p in pids:
        try:
            os.kill(p, signal.SIGKILL)
        except ProcessLookupError:
            pass


def wait_for(read, predicate, seconds=15, every=0.1, last=False):
    """read() until predicate(value) holds; its value, else an error with the last one (`last`: that one, for the caller to judge)."""
    end = time.monotonic() + seconds
    value = None
    while time.monotonic() < end:
        value = read()
        if predicate(value):
            return value
        time.sleep(every)
    if last:
        return value
    raise AssertionError(f"The expected private test state did not arrive; last: {str(value)[:300]}")


def host_group(groups, name="devbox"):
    """The host's own group (its `terminum` session or its state), not a session attached there (AC-89)."""
    return next((g for g in groups if g.get("host") == name and not g.get("attached")), None)


def wait_layout(ws, pred, timeout=5):
    """The groups of the first layout message for which pred(groups) holds, or None."""
    m = ws.until(lambda m: m["t"] == "layout" and pred(m["groups"]), timeout)
    return m["groups"] if m else None


def wait_host(ws, test, timeout=5, name="devbox"):
    """The groups of the first layout in which the host's own group passes test(group), or None."""
    return wait_layout(ws, lambda groups: (g := host_group(groups, name)) is not None and test(g), timeout)


class WS:
    """A page's WebSocket: JSON messages in and out."""

    def __init__(self, port, path="/ws", origin=None, protocols=None):
        self.s = socket.create_connection(("127.0.0.1", port), timeout=10)
        key = base64.b64encode(os.urandom(16)).decode()
        head = [f"GET {path} HTTP/1.1", f"Host: 127.0.0.1:{port}", "Upgrade: websocket", "Connection: Upgrade",
                f"Sec-WebSocket-Key: {key}", "Sec-WebSocket-Version: 13", f"Origin: {origin or f'http://127.0.0.1:{port}'}"]
        if protocols:
            head.append("Sec-WebSocket-Protocol: " + ", ".join(protocols))
        self.s.sendall(("\r\n".join(head) + "\r\n\r\n").encode())
        resp = b""
        while b"\r\n\r\n" not in resp:
            byte = self.s.recv(1)
            if not byte:
                break
            resp += byte
        self.status = int(resp.split(b" ")[1]) if resp else 0
        self.buf = b""

    def _read(self, n):
        while len(self.buf) < n:
            data = self.s.recv(65536)
            if not data:
                raise EOFError("closed")
            self.buf += data
        out, self.buf = self.buf[:n], self.buf[n:]
        return out

    def recv(self, timeout=10):
        end = time.time() + timeout
        while True:
            # anew for each frame: terminumd's pings, every 15 s, must not keep a wait going for good
            left = end - time.time()
            if left <= 0:
                raise TimeoutError("no message in time")
            self.s.settimeout(left)
            b1, b2 = self._read(2)
            n = b2 & 0x7F
            if n == 126:
                n = struct.unpack("!H", self._read(2))[0]
            elif n == 127:
                n = struct.unpack("!Q", self._read(8))[0]
            data = self._read(n)
            if b1 & 0x0F == 1:
                return json.loads(data)
            if b1 & 0x0F == 8:
                raise EOFError("closed")

    def until(self, pred, timeout=10):
        """The first message for which pred(m) holds, or None after `timeout` seconds."""
        end = time.time() + timeout
        while time.time() < end:
            try:
                m = self.recv(max(0.1, end - time.time()))
            except (socket.timeout, TimeoutError):
                return None
            if pred(m):
                return m
        return None

    def send(self, obj):
        data = json.dumps(obj).encode()
        mask, n = os.urandom(4), len(data)
        if n < 126:
            head = bytes([0x81, 0x80 | n])
        elif n < 65536:
            head = bytes([0x81, 0x80 | 126]) + struct.pack("!H", n)
        else:
            head = bytes([0x81, 0x80 | 127]) + struct.pack("!Q", n)
        self.s.sendall(head + mask + bytes(b ^ mask[i % 4] for i, b in enumerate(data)))

    def close(self):
        self.s.close()


def unix_request(path, method, url, body=None):
    """(status, decoded JSON or text) of a request on a Unix socket."""
    class Conn(http.client.HTTPConnection):
        def connect(self):
            self.sock = socket.socket(socket.AF_UNIX)
            self.sock.settimeout(30)
            self.sock.connect(path)
    c = Conn("localhost")
    c.request(method, url, body=json.dumps(body) if body is not None else None, headers={"Content-Type": "application/json"})
    r = c.getresponse()
    text = r.read().decode()
    try:
        return r.status, json.loads(text)
    except ValueError:
        return r.status, text


class Terminumd:
    """A private terminumd: temporary TM_HOME and TM_APP_DIR, its own port and tmux socket."""

    def __init__(self, port, tmux_socket, env=None):
        self.port, self.tmux_socket = port, tmux_socket
        self.root = tempfile.mkdtemp(prefix="tme-", dir=short_tmp())   # short: Unix socket paths end at 104 bytes
        self.home, self.app = os.path.join(self.root, "h"), os.path.join(self.root, "a")
        self.env = {**os.environ, "TM_HOME": self.home, "TM_APP_DIR": self.app, "TM_PORT": str(port),
                    "TM_TMUX_SOCKET": tmux_socket, "TM_LOG": "info", **(env or {})}
        self.env.pop("TMUX", None)
        self.env["HISTFILE"] = "/dev/null"   # an ending shell would write it into the folder being removed
        self.proc = None
        self.log_path = os.path.join(self.root, "terminumd.log")

    def through_fake_ssh(self, home, socket_name, pids, **more):
        """Its ssh is fake_ssh.py: `home` the far side's, `socket_name` its tmux, `pids` the file of its stand-ins; `more`: more of the environment."""
        self.env.update(TM_SSH=os.path.join(ROOT, "scripts", "fake_ssh.py"), FAKE_SSH_HOME=home, FAKE_SSH_TMUX_SOCKET=socket_name, FAKE_SSH_PIDS=pids, **more)

    @property
    def sock(self):
        return os.path.join(self.app, "terminumd.sock")

    def start(self):
        subprocess.run(["tmux", "-L", self.tmux_socket, "kill-server"], stderr=subprocess.DEVNULL)
        log = open(self.log_path, "a")
        self.proc = subprocess.Popen([BIN], env=self.env, stdout=log, stderr=log)
        for _ in range(100):
            if os.path.exists(self.sock):
                try:
                    if unix_request(self.sock, "GET", "/health")[0] == 200:
                        return self
                except OSError:
                    pass
            time.sleep(0.1)
        raise RuntimeError("terminumd did not start: " + self.log())

    def stop(self):
        if self.proc and self.proc.poll() is None:
            self.proc.terminate()
            self.proc.wait(10)

    def cleanup(self):
        self.stop()
        subprocess.run(["tmux", "-L", self.tmux_socket, "kill-server"], stderr=subprocess.DEVNULL)
        shutil.rmtree(self.root, ignore_errors=True)

    def log(self):
        try:
            return open(self.log_path).read()[-4000:]
        except OSError:
            return ""

    def health(self):
        return unix_request(self.sock, "GET", "/health")[1]

    PASSWORD = "correct horse battery"

    def signin(self):
        """A page's sign-in secret, as the password form gets it; the password is set first
        (as `terminum password`) when there is none (a new one would sign every page out)."""
        if not self.health()["password"]["set"]:
            st, body = unix_request(self.sock, "POST", "/password", {"password": self.PASSWORD})
            assert st == 200, f"no password: {body}"
        req = urllib.request.Request(f"http://127.0.0.1:{self.port}/auth/login", method="POST",
                                     data=json.dumps({"password": self.PASSWORD}).encode(),
                                     headers={"Content-Type": "application/json"})
        return json.load(urllib.request.urlopen(req))["secret"]

    def page(self, secret=None):
        """A signed-in page's WebSocket (its first messages are not read yet)."""
        ws = WS(self.port, protocols=["tm", secret or self.signin()])
        assert ws.status == 101, f"WebSocket refused: {ws.status}"
        return ws


def proof(secret, n):
    """What a page expects from /api/hello for its sign-in (AC-09)."""
    key = hmac.new(secret.encode(), b"tm-proof-key", hashlib.sha256).digest()
    return hmac.new(key, ("tm-hello-v1:" + n).encode(), hashlib.sha256).hexdigest()
