#!/usr/bin/env python3
"""Activate one warehouse row into Squid-CRM with an idempotency key.

Usage:
  SQUID_BASE_URL=http://127.0.0.1:3000 SQUID_API_KEY=... \
    python3 examples/reverse_etl_person_sync.py <<'JSON'
  {"email": "ada@example.com", "first_name": "Ada", "last_name": "Lovelace"}
  JSON
"""

from __future__ import annotations

import json
import os
import sys
import uuid
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "python" / "src"))

from squid_crm import SquidClient, person_activation_payload  # noqa: E402


def main() -> int:
    base_url = os.environ.get("SQUID_BASE_URL")
    api_key = os.environ.get("SQUID_API_KEY")
    if not base_url or not api_key:
        print("SQUID_BASE_URL and SQUID_API_KEY are required", file=sys.stderr)
        return 2
    row = json.load(sys.stdin)
    payload = person_activation_payload(row)
    key = os.environ.get("SQUID_IDEMPOTENCY_KEY") or str(uuid.uuid4())
    client = SquidClient(base_url, api_key=api_key)
    result = client.create_person_payload(payload, idempotency_key=key)
    print(json.dumps({"idempotencyKey": key, "result": result}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
