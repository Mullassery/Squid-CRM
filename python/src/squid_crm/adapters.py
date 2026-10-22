"""Optional adapters between Squid webhook envelopes and ecosystem services.

These adapters are intentionally pure functions with no network calls. They
encode the mapping assumptions that still require confirmation by the
counterpart service.
"""

from __future__ import annotations

from typing import Any, Dict, Mapping, Optional

__all__ = ["to_messagebirds_event"]


def to_messagebirds_event(
    envelope: Mapping[str, Any],
    *,
    identity: Mapping[str, Any],
    schema: str,
    tenant_id: str,
    correlation_id: Optional[str] = None,
    source: str = "squid-crm",
) -> Dict[str, Any]:
    """Map Squid envelope v1 to the documented MessageBirds/OCDS field names.

    MessageBirds' exact nested schema is not available in this repository, so
    this adapter uses the documented top-level fields only. Treat it as
    unverified until MessageBirds confirms or replaces the mapping.
    """
    if not identity:
        raise ValueError("identity is required for destination routing")
    for key in ("id", "type", "occurredAt", "data"):
        if key not in envelope:
            raise ValueError(f"webhook envelope is missing {key}")
    event_type = envelope["type"]
    return {
        "event_id": str(envelope["id"]),
        "event_type": str(event_type),
        "timestamp": str(envelope["occurredAt"]),
        "source": source,
        "identity": dict(identity),
        "schema": schema,
        "tenant_id": tenant_id,
        "correlation_id": correlation_id or str(envelope["id"]),
        "schema_version": str(envelope.get("schemaVersion", "1.0")),
        "payload": envelope["data"],
    }
