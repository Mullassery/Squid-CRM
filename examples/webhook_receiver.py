#!/usr/bin/env python3
"""Minimal Squid-CRM webhook receiver with HMAC verification.

Usage:
  SQUID_WEBHOOK_SECRET=... python3 examples/webhook_receiver.py

Then register a webhook targeting http://127.0.0.1:8099/hook.
The receiver verifies ``x-squid-signature: sha256=<hex>`` over the raw body.
"""

from __future__ import annotations

import hashlib
import hmac
import json
import os
from http.server import BaseHTTPRequestHandler, HTTPServer

SECRET = os.environ.get("SQUID_WEBHOOK_SECRET", "").encode("utf-8")


class Handler(BaseHTTPRequestHandler):
    def do_POST(self) -> None:  # noqa: N802
        if self.path != "/hook":
            self.send_error(404)
            return
        length = int(self.headers.get("content-length", "0"))
        body = self.rfile.read(length)
        signature = self.headers.get("x-squid-signature", "")
        expected = "sha256=" + hmac.new(SECRET, body, hashlib.sha256).hexdigest()
        if not SECRET or not hmac.compare_digest(signature, expected):
            self.send_error(401, "invalid signature")
            return
        event = json.loads(body)
        print(
            json.dumps(
                {
                    "id": event.get("id"),
                    "type": event.get("type"),
                    "schemaVersion": event.get("schemaVersion"),
                    "workspaceId": event.get("workspaceId"),
                }
            )
        )
        self.send_response(200)
        self.end_headers()

    def log_message(self, *args: object) -> None:
        return


if __name__ == "__main__":
    HTTPServer(("127.0.0.1", 8099), Handler).serve_forever()
