"""Optional adapters between Squid webhook envelopes and ecosystem services.

These adapters are intentionally pure functions with no network calls. They
encode the mapping assumptions that still require confirmation by the
counterpart service.
"""

from __future__ import annotations

from typing import Any, Dict, Mapping, Optional

__all__ = ["person_activation_payload", "to_pycustomerhub_event"]


def to_pycustomerhub_event(
    envelope: Mapping[str, Any],
    *,
    identity: Mapping[str, Any],
    schema: str,
    tenant_id: str,
    correlation_id: Optional[str] = None,
    source: str = "squid-crm",
) -> Dict[str, Any]:
    """Map Squid envelope v1 to the documented PyCustomerHub/OCDS field names.

    PyCustomerHub's exact nested schema is not available in this repository, so
    this adapter uses the documented top-level fields only. Treat it as
    unverified until PyCustomerHub confirms or replaces the mapping.
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


def person_activation_payload(
    row: Mapping[str, Any],
    *,
    field_map: Optional[Mapping[str, str]] = None,
) -> Dict[str, Any]:
    """Build a minimal Squid person payload from a warehouse/activation row.

    Only the canonical fields below are forwarded; all other columns are
    intentionally ignored so accidental PII expansion does not occur. Callers
    must still enforce consent and destination policy before writing.
    """
    mapping = {
        "email": "email",
        "first_name": "first_name",
        "last_name": "last_name",
        "job_title": "job_title",
        "company_id": "company_id",
    }
    if field_map:
        mapping.update(field_map)
    email = row.get(mapping["email"])
    if not email:
        raise ValueError("email is required for person activation")
    payload: Dict[str, Any] = {
        "name": {
            "firstName": row.get(mapping["first_name"], "") or "",
            "lastName": row.get(mapping["last_name"], "") or "",
        },
        "emails": {"primaryEmail": str(email)},
    }
    job_title = row.get(mapping["job_title"])
    if job_title:
        payload["jobTitle"] = str(job_title)
    company_id = row.get(mapping["company_id"])
    if company_id:
        payload["companyId"] = str(company_id)
    return payload
