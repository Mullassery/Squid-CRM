"""Optional adapters between Squid webhook envelopes and ecosystem services.

These adapters are intentionally pure functions with no network calls. The
PyCustomerHub mapping mirrors the canonical OCDS envelope implemented by
``PyCustomerHub``'s ``POST /v1/squid`` source; the reverse-ETL helper mirrors
Squid's native person create payload.
"""

from __future__ import annotations

import uuid
from typing import Any, Dict, Mapping, Optional, Sequence

__all__ = ["person_activation_payload", "to_pycustomerhub_event"]

_PRIMARY_IDENTITY_KEYS = ("crm_id", "email", "anonymous_id", "user_id", "id")


def _uuid(value: Any, *, field: str) -> str:
    try:
        return str(uuid.UUID(str(value)))
    except (AttributeError, TypeError, ValueError):
        raise ValueError(f"{field} must be a UUID") from None


def _split_schema(schema: str, default_version: str) -> tuple[str, str]:
    """Accept ``name@1.0``, ``name.v1``, or a bare name.

    A bare name keeps the envelope's schemaVersion (or ``1.0``) rather than
    guessing a version from the last dot of a dotted schema name.
    """
    raw = str(schema).strip()
    if not raw:
        raise ValueError("schema is required")
    if "@" in raw:
        name, version = raw.rsplit("@", 1)
    else:
        name, maybe_version = raw.rsplit(".", 1) if "." in raw else (raw, "")
        if maybe_version[:1].isdigit() or (
            maybe_version[:1] in {"v", "V"} and maybe_version[1:].isdigit()
        ):
            version = maybe_version
        else:
            name, version = raw, default_version
    name = name.strip()
    version = (version or default_version).strip()
    if not name or not version:
        raise ValueError("schema name and version are required")
    return name, version


def _identity_refs(
    identity: Mapping[str, Any] | Sequence[Mapping[str, Any]],
    *,
    source: str,
) -> list[Dict[str, Any]]:
    """Normalise a Squid-style identity mapping into OCDS identity claims."""
    if isinstance(identity, Mapping):
        if "namespace" in identity and "value" in identity:
            items = [identity]
        else:
            items = [{"namespace": key, "value": value} for key, value in identity.items()]
    else:
        items = list(identity)

    refs: list[Dict[str, Any]] = []
    for item in items:
        if not isinstance(item, Mapping):
            raise ValueError("identity claims must be mappings")
        namespace = str(item.get("namespace", "")).strip()
        value = item.get("value")
        if not namespace or value is None or not str(value).strip():
            raise ValueError("identity claims require namespace and value")
        refs.append(
            {
                "namespace": namespace,
                "value": str(value).strip(),
                "primary": bool(item.get("primary", False)),
                "source": str(item.get("source", source)),
                "confidence": float(item.get("confidence", 1.0)),
            }
        )

    if not refs:
        raise ValueError("identity is required for destination routing")

    # Exactly one primary claim; prefer the canonical CRM id.
    primaries = [ref for ref in refs if ref["primary"]]
    if not primaries:
        preferred = next(
            (ref for key in _PRIMARY_IDENTITY_KEYS for ref in refs if ref["namespace"] == key),
            refs[0],
        )
        preferred["primary"] = True
    elif len(primaries) > 1:
        for ref in primaries[1:]:
            ref["primary"] = False
    return refs


def to_pycustomerhub_event(
    envelope: Mapping[str, Any],
    *,
    identity: Mapping[str, Any] | Sequence[Mapping[str, Any]],
    schema: str,
    tenant_id: str,
    correlation_id: Optional[str] = None,
    source: str = "squid-crm",
) -> Dict[str, Any]:
    """Map Squid envelope v1 to PyCustomerHub's canonical OCDS envelope.

    The output matches ``PyCustomerHub``'s ``EventEnvelope`` (and therefore
    ``POST /events``), not the older flattened shape. ``identity`` may be a
    single ``{"namespace": ..., "value": ...}`` claim, a mapping of
    namespaces to values, or a sequence of claim mappings.

    ``schema`` accepts ``name@version``, ``name.<numeric-version>``, or a bare
    name; a bare name uses the envelope's ``schemaVersion``. UUID fields are
    validated instead of forwarding strings the Rust API will reject.
    """
    for key in ("id", "type", "occurredAt", "data"):
        if key not in envelope:
            raise ValueError(f"webhook envelope is missing {key}")

    event_id = _uuid(envelope["id"], field="envelope.id")
    correlation = (
        _uuid(correlation_id, field="correlation_id")
        if correlation_id is not None
        else event_id
    )
    schema_name, schema_version = _split_schema(
        schema, str(envelope.get("schemaVersion", "1.0"))
    )
    event_type = str(envelope.get("type") or envelope.get("event") or "").strip()
    if not event_type:
        raise ValueError("webhook envelope has no type/event")
    data = envelope["data"]
    if not isinstance(data, Mapping):
        raise ValueError("webhook envelope data must be an object")

    context: Dict[str, Any] = {}
    supplied_context = envelope.get("context")
    if isinstance(supplied_context, Mapping):
        context.update(supplied_context)
    squid_context: Dict[str, Any] = {
        "schema_version": str(envelope.get("schemaVersion", "1.0")),
    }
    if "workspaceId" in envelope:
        squid_context["workspace_id"] = str(envelope["workspaceId"])
    object_name, _, operation = event_type.partition(".")
    if object_name:
        squid_context["object"] = object_name
    if operation:
        squid_context["operation"] = operation
    context["squid"] = squid_context

    return {
        "event_id": event_id,
        "event_type": event_type,
        "timestamp": str(envelope["occurredAt"]),
        "source": {"type": source, "name": "squid-webhook"},
        "identity": _identity_refs(identity, source=source),
        "context": context,
        "data": dict(data),
        "schema": {"name": schema_name, "version": schema_version},
        "consent": envelope.get("consent"),
        "governance": envelope.get("governance"),
        "correlation_id": correlation,
        "trace_id": envelope.get("traceId"),
        "tenant_id": _uuid(tenant_id, field="tenant_id"),
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
