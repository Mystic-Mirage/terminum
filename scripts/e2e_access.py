#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
"""Who may use terminumd, end to end (terminum.md AC-09) against a private terminumd: the
password (none set, too short, its hash, brakes), the sign-in it gets, the table of refused
requests, what a forwarded request (tailscale serve) may not do, Tailscale Funnel, the hello
proof, the private socket and files, sign-out, a new password, the files of 0.5.0 dropped.

    python3 scripts/e2e_access.py      (needs terminumd built: cd terminumd && cargo build)
"""
import json
import os
import socket
import stat
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from e2e_ws import WS, Terminumd, check, proof, results, summary, unix_request  # noqa: E402
from testrun import free_port, own  # noqa: E402

PORT, SOCKET = free_port(), own("tm-test-access")


def raw(port, method, path, headers=None, body=b""):
    """(status, JSON body) of a request sent exactly as given (urllib would set Host itself)."""
    s = socket.create_connection(("127.0.0.1", port), timeout=10)
    head = [f"{method} {path} HTTP/1.1", "Connection: close", f"Content-Length: {len(body)}"]
    head += [f"{k}: {v}" for k, v in (headers or {}).items()]
    s.sendall(("\r\n".join(head) + "\r\n\r\n").encode() + body)
    data = b""
    while chunk := s.recv(65536):
        data += chunk
    s.close()
    head, _, rest = data.partition(b"\r\n\r\n")
    status = int(head.split(b" ")[1])
    try:
        return status, json.loads(rest or b"null"), head.decode()
    except ValueError:
        return status, rest.decode(errors="replace"), head.decode()


def closed(ws, timeout=3):
    """The page's WebSocket is closed by terminumd within `timeout` seconds."""
    try:
        while True:
            ws.recv(timeout)
    except EOFError:
        return True
    except (socket.timeout, TimeoutError):
        return False


def login(password, headers=None):
    """POST /auth/login on the Mac's own name: (status, body)."""
    h = {"Host": f"127.0.0.1:{PORT}", "Content-Type": "application/json", **(headers or {})}
    st, body, _ = raw(PORT, "POST", "/auth/login", h, json.dumps({"password": password}).encode())
    return st, body


def main():
    t = Terminumd(PORT, SOCKET)
    try:
        # the files of 0.5.0: a code sign-in, cookie sign-ins, the second listener's keys
        os.makedirs(t.app, mode=0o700)
        for name, content in [("local-sessions.json", {"items": []}), ("web-sessions.json", {"salt": "s", "sessions": {}}),
                              ("web.json", {"enabled": True, "host": "0.0.0.0", "port": 48080, "history": 10000, "allow_hosts": []})]:
            with open(os.path.join(t.app, name), "w") as f:
                json.dump(content, f)
        t.start()
        host = f"127.0.0.1:{PORT}"
        mode = lambda p: stat.S_IMODE(os.stat(p).st_mode)
        check("AC-09 the sign-ins of 0.5.0 are dropped: local-sessions.json and web-sessions.json are gone",
              not os.path.exists(os.path.join(t.app, "local-sessions.json")) and not os.path.exists(os.path.join(t.app, "web-sessions.json")))

        # no password yet
        st, body, _ = raw(PORT, "GET", "/auth/check", {"Host": host})
        check("AC-09 with no password set the page is told how to set one", st == 401 and body.get("login") == "none"
              and "terminum password" in body.get("message", ""), (st, body))
        st, body = login("anything at all")
        check("AC-09 with no password set no one signs in (503 no_password)", st == 503 and body.get("reason") == "no_password", (st, body))
        check("AC-09 terminum status says no password is set", t.health()["password"] == {"set": False, "error": None}, t.health()["password"])
        st, body = unix_request(t.sock, "POST", "/password", {"password": "short"})
        check("AC-09 a password under 8 characters is refused", st == 409, body)
        st, body = unix_request(t.sock, "POST", "/password", {"password": "correct horse"})
        cfg = os.path.join(t.app, "web.json")
        web = json.load(open(cfg))
        check("AC-09 web.json holds a salted PBKDF2 hash, 0600, without 0.5.0's listener keys",
              st == 200 and web["password"]["iterations"] >= 100000 and "correct" not in open(cfg).read() and mode(cfg) == 0o600
              and not {"enabled", "host", "port", "history"} & set(web), (st, sorted(web)))

        # the password and its brakes
        t0 = time.time()
        st, body = login("wrong pass")
        st2, body2 = login("wrong again")
        check("AC-09 a wrong password is refused and holds the next one", st == 401 and body["reason"] == "wrong" and time.time() - t0 >= 1.9,
              (st, round(time.time() - t0, 1)))
        st, body = login("correct horse")
        secret = body.get("secret", "") if isinstance(body, dict) else ""
        check("AC-09 the right password gets a 256-bit sign-in in the body, no cookie", st == 200 and len(secret) == 64, (st, body))
        sess = {"Host": host, "X-TM-Session": secret}

        cases = [
            ("GET /ws (no sign-in)", "GET", "/ws", {"Host": host, "Upgrade": "websocket", "Connection": "Upgrade", "Origin": f"http://{host}"}, 401, "bad_session"),
            ("GET /fb/api/ls, Host: evil.example", "GET", "/fb/api/ls?path=/tmp", {**sess, "Host": f"evil.example:{PORT}"}, 403, "bad_host"),
            ("GET /ws with a sign-in, Origin: http://evil.example", "GET", "/ws", {**sess, "Upgrade": "websocket", "Connection": "Upgrade",
                                                                                      "Sec-WebSocket-Key": "x", "Sec-WebSocket-Version": "13", "Origin": "http://evil.example"}, 403, "bad_origin"),
            ("POST /fb/api/fs/mkdir with a sign-in, Content-Type: text/plain", "POST", "/fb/api/fs/mkdir", {**sess, "Content-Type": "text/plain"}, 415, "bad_content_type"),
            ("POST /fb/api/terminal/cd forwarded by tailscale serve", "POST", "/fb/api/terminal/cd",
             {**sess, "Content-Type": "application/json", "Origin": f"http://{host}", "X-Forwarded-Host": "mac.tail1234.ts.net"}, 403, "not_here"),
            ("GET / through Tailscale Funnel", "GET", "/", {"Host": "mac.tail1234.ts.net", "Tailscale-Funnel-Request": "?1"}, 403, "funnel"),
            ("GET /internal/anything over TCP", "GET", "/internal/state", sess, 404, "not_found"),
            ("GET /fb/api/ls without a sign-in", "GET", "/fb/api/ls?path=/tmp", {"Host": host}, 401, "bad_session"),
            ("GET /fb/api/ls with a wrong sign-in", "GET", "/fb/api/ls?path=/tmp", {"Host": host, "X-TM-Session": "0" * 64}, 401, "bad_session"),
            ("GET /open/<code> (0.5.0's sign-in link)", "GET", "/open/0123456789abcdef", {"Host": host}, 404, "not_found"),
        ]
        for name, method, path, headers, status, code in cases:
            st, body, _ = raw(PORT, method, path, headers)
            check(f"AC-09 {name} → {status} {code}", st == status and isinstance(body, dict) and body.get("error") == code, (st, body))
        st, body, _ = raw(PORT, "GET", "/fb/api/ls?path=/tmp", sess)
        check("AC-09 a signed-in request is answered", st == 200 and body.get("status") in ("ready", "loading"), st)
        ts = {"Host": "mac.tail1234.ts.net", "X-Forwarded-Host": "mac.tail1234.ts.net", "X-Forwarded-For": "100.64.0.7",
              "X-Forwarded-Proto": "https", "X-TM-Session": secret}
        st, body, _ = raw(PORT, "GET", "/fb/api/ls?path=/tmp", ts)
        check("AC-09 the same sign-in works through tailscale serve's name", st == 200, st)
        st, body, _ = raw(PORT, "GET", "/fb/api/raw?path=/etc/hosts&t=" + secret, {"Host": host})
        check("AC-09 the sign-in in an address signs nothing in (images are fetched with the header)", st == 401, st)
        st, body, _ = raw(PORT, "GET", "/custom.css", {"Host": host})
        check("AC-09 /custom.css needs the sign-in", st == 401, st)
        st, body, _ = raw(PORT, "GET", "/custom.css", sess)
        check("AC-09 /custom.css is the page's with it", st == 200, st)
        st, body, head = raw(PORT, "GET", "/fb/api/ls?path=/tmp", {"Host": host})
        hardened = [h for h in ("x-content-type-options: nosniff", "referrer-policy: no-referrer", "cache-control: no-store", "permissions-policy: camera=()",
                                "cross-origin-opener-policy: same-origin", "cross-origin-resource-policy: same-origin") if h not in head.lower()]
        check("AC-09 a refusal carries the security headers too", st == 401 and not hardened, hardened)
        st, body, _ = raw(PORT, "GET", "/auth/check", sess)
        check("AC-09 /auth/check says signed in", st == 204, st)
        n = "0123456789abcdef0123456789abcdef"
        import hashlib
        sid = hashlib.sha256(secret.encode()).hexdigest()[:16]
        st, body, _ = raw(PORT, "GET", f"/api/hello?n={n}&id={sid}", {"Host": host})
        check("AC-09 the server proves it holds the page's sign-in", st == 200 and body["proof"] == proof(secret, n))
        st, body, _ = raw(PORT, "GET", f"/api/hello?n={n}&id={'f' * 16}", {"Host": host})
        check("AC-09 an unknown sign-in gets no proof", st == 404 and body["error"] == "unknown_signin", (st, body))
        check("AC-09 the state folder is 0700, the sign-ins private",
              mode(t.app) == 0o700 and mode(os.path.join(t.app, "signins.json")) == 0o600, oct(mode(t.app)))

        # sign-out: one browser, every browser, a new password
        st, other = login("correct horse")
        other = other["secret"]
        mine, mine2, theirs = t.page(secret), t.page(secret), t.page(other)
        my_panel = WS(PORT, path="/fb/api/ws", protocols=["tm", secret])
        my_panel.recv()                                # its first frame, the state
        out = {"Content-Type": "application/json", "Origin": f"http://{host}"}
        st, _, _ = raw(PORT, "POST", "/auth/logout", {**sess, **out}, b"{}")
        st1, _, _ = raw(PORT, "GET", "/fb/api/ls?path=/tmp", sess)
        st2, _, _ = raw(PORT, "GET", "/fb/api/ls?path=/tmp", {"Host": host, "X-TM-Session": other})
        check("AC-09 Sign out ends that browser's sign-in only", st == 204 and st1 == 401 and st2 == 200, (st, st1, st2))
        check("AC-09 Sign out closes every page and Files panel of that sign-in at once (another tab, another device), no other sign-in's",
              closed(mine) and closed(mine2) and closed(my_panel) and not closed(theirs, 1))
        theirs.close()
        st, _, _ = raw(PORT, "GET", "/fb/api/events", {"Host": host, "X-TM-Session": other})
        check("AC-09 an agent's event stream is not served on the page's listener", st == 404, st)
        fourth = login("correct horse")[1]["secret"]
        sess4 = {"Host": host, "X-TM-Session": fourth}
        page4 = t.page(fourth)
        os.chmod(t.app, 0o500)                         # signins.json cannot be written
        try:
            st, body, _ = raw(PORT, "POST", "/auth/logout", {**sess4, **out}, b"{}")
        finally:
            os.chmod(t.app, 0o700)
        st2, _, _ = raw(PORT, "GET", "/fb/api/ls?path=/tmp", sess4)
        check("AC-09 a Sign out signins.json cannot keep still ends the sign-in and its pages, and says what a restart would do",
              st == 503 and body.get("error") == "signout_unsaved" and "terminum signout" in body.get("message", "") and st2 == 401
              and closed(page4), (st, body, st2))
        ws = t.page(other)
        panel = WS(PORT, path="/fb/api/ws", protocols=["tm", other])
        panel.recv()                                   # its first frame, the state
        st, body = unix_request(t.sock, "POST", "/signout", {})
        st2, _, _ = raw(PORT, "GET", "/fb/api/ls?path=/tmp", {"Host": host, "X-TM-Session": other})
        check("AC-09 terminum signout signs every browser out and closes its page", st == 200 and st2 == 401 and closed(ws), (st, st2))
        check("AC-09 terminum signout closes a Files panel's socket at once", panel.status == 101 and closed(panel))
        st, third = login("correct horse")
        st, body = unix_request(t.sock, "POST", "/password", {"password": "battery staple"})
        st2, _, _ = raw(PORT, "GET", "/fb/api/ls?path=/tmp", {"Host": host, "X-TM-Session": third["secret"]})
        st3, _ = login("correct horse")
        st4, _ = login("battery staple")
        check("AC-09 a new password signs everyone out; only it signs in", st == 200 and st2 == 401 and st3 == 401 and st4 == 200,
              (st, st2, st3, st4))

        # brakes: guesses from the tailnet do not keep the owner out once the CLI acts
        tailnet = {"Host": "mac.tail1234.ts.net", "Origin": "https://mac.tail1234.ts.net", "X-Forwarded-For": "100.64.0.9",
                   "X-Forwarded-Host": "mac.tail1234.ts.net"}
        for i in range(5):
            login("guess %d" % i, tailnet)
        st, body = login("battery staple", tailnet)
        check("AC-09 five wrong passwords from one address make it wait, the right one too", st == 429 and body["reason"] == "locked", (st, body))
        st, body = login("battery staple")
        check("AC-09 another address is not held by them", st == 200, (st, body))
        unix_request(t.sock, "POST", "/signout", {})
        st, body = login("battery staple", tailnet)
        check("AC-09 terminum signout starts the brakes over", st == 200, (st, body))
        ws.close()
    finally:
        if not all(results):
            print(t.log())
        t.cleanup()
    summary()


if __name__ == "__main__":
    main()
