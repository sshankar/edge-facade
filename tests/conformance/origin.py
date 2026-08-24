#!/usr/bin/env python3
"""Echo origin for the conformance suite's T4 Host-parity test.

Stands in for the declared origin (api.example.com) under Viceroy. Returns
the same JSON shape the native mock origin produces, so drivers assert
identically:

    {"host": "...", "path": "...", "query": "..."}

The `host` field is what the adapter delivered — via `override_host` for
static backends (D5.1) — so asserting it equals the URL host exercises the
parity rule end to end.
"""

import json
from http.server import BaseHTTPRequestHandler, HTTPServer


class Origin(BaseHTTPRequestHandler):
    def do_GET(self):
        split = self.path.split("?", 1)
        body = json.dumps(
            {
                "host": self.headers.get("Host", ""),
                "path": split[0],
                "query": split[1] if len(split) > 1 else "",
            }
        ).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *args):
        pass  # keep the origin quiet


if __name__ == "__main__":
    HTTPServer(("127.0.0.1", 18080), Origin).serve_forever()
