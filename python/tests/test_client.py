import json
import threading
import unittest
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))

from squid_crm import SquidClient, SquidError  # noqa: E402


class _Handler(BaseHTTPRequestHandler):
    def _handle(self):
        length = int(self.headers.get("Content-Length", "0"))
        body = self.rfile.read(length) if length else b""
        self.server.requests.append(
            {"method": self.command, "headers": dict(self.headers), "body": body}
        )
        self.server.callback(self)

    do_GET = _handle
    do_POST = _handle

    def log_message(self, *args):
        pass


def _respond(handler, status, payload=None, headers=None):
    raw = json.dumps(payload).encode() if payload is not None else b""
    handler.send_response(status)
    handler.send_header("Content-Type", "application/json")
    handler.send_header("Content-Length", str(len(raw)))
    for name, value in (headers or {}).items():
        handler.send_header(name, value)
    handler.end_headers()
    handler.wfile.write(raw)


class SquidClientTests(unittest.TestCase):
    def serve(self, callback):
        server = ThreadingHTTPServer(("127.0.0.1", 0), _Handler)
        server.callback = callback
        server.requests = []
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()

        def stop():
            server.shutdown()
            server.server_close()
            thread.join(timeout=2)

        self.addCleanup(stop)
        return server, f"http://127.0.0.1:{server.server_address[1]}"

    def test_create_person_sends_idempotency_and_correlation_headers(self):
        def callback(handler):
            _respond(handler, 201, {"data": {"createPerson": {"id": "p1"}}})

        server, base = self.serve(callback)
        result = SquidClient(base, api_key="key").create_person(
            "Ada", "Lovelace", "ada@example.com"
        )

        self.assertEqual(result["data"]["createPerson"]["id"], "p1")
        headers = server.requests[0]["headers"]
        self.assertEqual(headers["Authorization"], "Bearer key")
        self.assertTrue(headers["Idempotency-Key"])
        self.assertTrue(headers["X-Correlation-Id"])

    def test_create_person_retries_503_with_the_same_key(self):
        def callback(handler):
            if len(handler.server.requests) == 1:
                _respond(handler, 503, {"messages": ["busy"]})
            else:
                _respond(handler, 201, {"data": {"createPerson": {"id": "p1"}}})

        server, base = self.serve(callback)
        result = SquidClient(base, retry_backoff=0).create_person(
            "Ada", "Lovelace", "ada@example.com"
        )

        self.assertEqual(result["data"]["createPerson"]["id"], "p1")
        self.assertEqual(len(server.requests), 2)
        first = server.requests[0]["headers"]["Idempotency-Key"]
        second = server.requests[1]["headers"]["Idempotency-Key"]
        self.assertEqual(first, second)

    def test_read_retries_without_an_idempotency_key(self):
        def callback(handler):
            if len(handler.server.requests) == 1:
                _respond(handler, 500, {"messages": ["boom"]})
            else:
                _respond(handler, 200, {"data": [], "totalCount": 0})

        server, base = self.serve(callback)
        result = SquidClient(base, retry_backoff=0).list_people()

        self.assertEqual(result["totalCount"], 0)
        self.assertEqual(len(server.requests), 2)
        self.assertNotIn("Idempotency-Key", server.requests[0]["headers"])

    def test_post_without_a_key_is_not_retried(self):
        def callback(handler):
            _respond(handler, 503, {"messages": ["busy"]})

        server, base = self.serve(callback)
        with self.assertRaises(SquidError) as raised:
            SquidClient(base, retry_backoff=0).request(
                "POST", "/rest/people", {"name": {}}
            )

        self.assertEqual(raised.exception.status, 503)
        self.assertEqual(len(server.requests), 1)

    def test_http_error_body_is_exposed(self):
        def callback(handler):
            _respond(handler, 403, {"messages": ["Missing authentication token"]})

        _, base = self.serve(callback)
        with self.assertRaises(SquidError) as raised:
            SquidClient(base).list_people()

        self.assertEqual(raised.exception.status, 403)
        self.assertEqual(
            raised.exception.body["messages"], ["Missing authentication token"]
        )

    def test_graphql_errors_raise(self):
        def callback(handler):
            _respond(handler, 200, {"errors": [{"message": "nope"}]})

        _, base = self.serve(callback)
        with self.assertRaises(SquidError):
            SquidClient(base).graphql("{ currentUser { id } }")


if __name__ == "__main__":
    unittest.main()
