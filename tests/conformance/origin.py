#!/usr/bin/env python3
"""Local origin for the conformance suite.

Two protocol spaces on one port (127.0.0.1:18080):

1. Echo/redirect space (T4/T7/T11, r1): any request without the
   `urlencoded=true` query param returns the JSON echo shape
   {"host", "path", "query"}, or a 302 for /t7-redirect.
2. KV namespace space (T8, workerd only): workerd's kvNamespace binding
   translates KV operations into HTTP requests to the bound service with
   path = the URL-encoded key and query `urlencoded=true` (verified against
   src/workerd/api/kv.c++). GET 404 -> missing key; PUT/DELETE 2xx -> ok.

Responses are HTTP/1.1 with `Connection: close`: workerd's kvNamespace
client hangs on HTTP/1.0 close-delimited responses (verified empirically),
so this matters for the KV path specifically.
"""

import json
from http.server import BaseHTTPRequestHandler, HTTPServer
from urllib.parse import unquote, urlparse

KV_DATA = {}


class Origin(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def _send(self, code, body=b"", headers=()):
        self.send_response(code)
        for k, v in headers:
            self.send_header(k, v)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Connection", "close")
        self.end_headers()
        if body:
            self.wfile.write(body)

    def _kv(self):
        """workerd KV namespace protocol (T8)."""
        key = unquote(urlparse(self.path).path.lstrip("/"))
        if self.command == "GET":
            if key in KV_DATA:
                self._send(200, KV_DATA[key])
            else:
                self._send(404)
        elif self.command == "PUT":
            n = int(self.headers.get("Content-Length", 0))
            KV_DATA[key] = self.rfile.read(n)
            self._send(200)
        elif self.command == "DELETE":
            KV_DATA.pop(key, None)
            self._send(200)

    def do_GET(self):
        if "urlencoded=true" in urlparse(self.path).query:
            self._kv()
            return
        split = self.path.split("?", 1)
        # r1 (redirect parity, D5.2): the origin redirects and expects the
        # adapter to pass the 302 through, never follow it.
        if split[0] == "/t7-redirect":
            self._send(302, b"", (("Location", "/t7-target"),))
            return
        if split[0] == "/t7-target":
            self._send(200, b"redirect target")
            return
        body = json.dumps(
            {
                "host": self.headers.get("Host", ""),
                "path": split[0],
                "query": split[1] if len(split) > 1 else "",
            }
        ).encode()
        self._send(200, body, (("Content-Type", "application/json"),))

    def do_PUT(self):
        self._kv()

    def do_DELETE(self):
        self._kv()

    def log_message(self, *args):
        pass  # keep the origin quiet


if __name__ == "__main__":
    HTTPServer(("127.0.0.1", 18080), Origin).serve_forever()
