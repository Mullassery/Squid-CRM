"""Python client for the Squid CRM API."""

from __future__ import annotations

import json
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid
from typing import Any, Dict, Optional

__version__ = "0.2.0"

__all__ = ["SquidClient", "SquidError", "__version__"]

_RETRYABLE_STATUS = {408, 425, 429, 500, 502, 503, 504}
_SAFE_METHODS = {"GET", "HEAD", "OPTIONS"}


class SquidError(RuntimeError):
    """Raised when the Squid API returns an error response."""

    def __init__(self, message: str, status: Optional[int] = None, body: Any = None):
        super().__init__(message)
        self.status = status
        self.body = body


class SquidClient:
    """Small dependency-free client for the Squid REST/GraphQL API.

    ``api_key`` is the bearer token returned by the API-key lifecycle
    mutations. User access tokens are rejected by the data API, matching the
    upstream contract.

    Mutating requests are retried only when an ``Idempotency-Key`` is supplied,
    so a network retry cannot create duplicate records. Read-only requests are
    always safe to retry. Every request carries an ``X-Correlation-Id`` that can
    be joined with server logs; pass ``correlation_id`` to reuse a caller value.
    """

    def __init__(
        self,
        base_url: str,
        api_key: Optional[str] = None,
        timeout: float = 30.0,
        max_retries: int = 2,
        retry_backoff: float = 0.25,
        correlation_id: Optional[str] = None,
    ) -> None:
        if max_retries < 0:
            raise ValueError("max_retries must be non-negative")
        if retry_backoff < 0:
            raise ValueError("retry_backoff must be non-negative")
        self.base_url = base_url.rstrip("/")
        self.api_key = api_key
        self.timeout = timeout
        self.max_retries = max_retries
        self.retry_backoff = retry_backoff
        self.correlation_id = correlation_id

    def request(
        self,
        method: str,
        path: str,
        payload: Optional[Dict[str, Any]] = None,
        params: Optional[Dict[str, Any]] = None,
        idempotency_key: Optional[str] = None,
        correlation_id: Optional[str] = None,
    ) -> Any:
        """Execute one REST request with retries and tracing headers.

        Supply ``idempotency_key`` for a mutating call that may be retried. The
        same key must be reused by the caller for a later manual retry; the
        helper methods generate one when omitted.
        """
        return self._request(
            method,
            path,
            payload=payload,
            params=params,
            idempotency_key=idempotency_key,
            correlation_id=correlation_id,
        )

    def _request(
        self,
        method: str,
        path: str,
        payload: Optional[Dict[str, Any]] = None,
        params: Optional[Dict[str, Any]] = None,
        idempotency_key: Optional[str] = None,
        correlation_id: Optional[str] = None,
    ) -> Any:
        method = method.upper()
        url = f"{self.base_url}{path}"
        if params:
            query = urllib.parse.urlencode(
                {key: value for key, value in params.items() if value is not None}
            )
            if query:
                url = f"{url}?{query}"
        data = None
        headers = {
            "Accept": "application/json",
            "X-Correlation-Id": correlation_id or self.correlation_id or str(uuid.uuid4()),
        }
        if payload is not None:
            data = json.dumps(payload).encode("utf-8")
            headers["Content-Type"] = "application/json"
        if self.api_key:
            headers["Authorization"] = f"Bearer {self.api_key}"
        if idempotency_key is not None:
            headers["Idempotency-Key"] = idempotency_key

        mutating = method not in _SAFE_METHODS
        retryable = not mutating or idempotency_key is not None
        attempts = 1 + (self.max_retries if retryable else 0)

        for attempt in range(attempts):
            request = urllib.request.Request(
                url, data=data, headers=headers, method=method
            )
            try:
                with urllib.request.urlopen(request, timeout=self.timeout) as response:
                    raw = response.read()
                    return json.loads(raw) if raw else None
            except urllib.error.HTTPError as error:
                raw = error.read()
                if (
                    attempt + 1 < attempts
                    and error.code in _RETRYABLE_STATUS
                    and retryable
                ):
                    time.sleep(self._retry_delay(attempt, error.headers))
                    continue
                raise self._http_error(error.code, raw) from error
            except urllib.error.URLError as error:
                if attempt + 1 < attempts and retryable:
                    time.sleep(self._retry_delay(attempt, None))
                    continue
                raise SquidError(str(error.reason)) from error

        raise SquidError("request failed after retries")

    def _retry_delay(self, attempt: int, headers: Any) -> float:
        retry_after = headers.get("Retry-After") if headers is not None else None
        if retry_after:
            try:
                return min(max(float(retry_after), 0.0), 30.0)
            except ValueError:
                pass
        return min(self.retry_backoff * (2**attempt), 5.0)

    @staticmethod
    def _http_error(status: int, raw: bytes) -> SquidError:
        try:
            body = json.loads(raw)
        except ValueError:
            body = raw.decode("utf-8", errors="replace")
        message = body.get("messages") if isinstance(body, dict) else body
        return SquidError(str(message), status=status, body=body)

    def graphql(
        self,
        query: str,
        variables: Optional[Dict[str, Any]] = None,
        idempotency_key: Optional[str] = None,
        correlation_id: Optional[str] = None,
    ) -> Any:
        """Execute a GraphQL operation against ``/graphql``."""
        response = self._request(
            "POST",
            "/graphql",
            {"query": query, "variables": variables or {}},
            idempotency_key=idempotency_key,
            correlation_id=correlation_id,
        )
        if isinstance(response, dict) and response.get("errors"):
            raise SquidError(str(response["errors"]), body=response)
        return response.get("data") if isinstance(response, dict) else response

    def list_people(
        self,
        limit: int = 20,
        offset: int = 0,
        correlation_id: Optional[str] = None,
    ) -> Any:
        """List people through the REST data API."""
        return self._request(
            "GET",
            "/rest/people",
            params={"limit": limit, "offset": offset},
            correlation_id=correlation_id,
        )

    def create_person(
        self,
        first_name: str,
        last_name: str,
        email: str,
        idempotency_key: Optional[str] = None,
        correlation_id: Optional[str] = None,
        **fields: Any,
    ) -> Any:
        """Create a person through the REST data API.

        An ``Idempotency-Key`` is generated unless one is supplied, so this
        method can be retried safely by the client. Reuse the same key for your
        own retries.
        """
        payload: Dict[str, Any] = {
            "name": {"firstName": first_name, "lastName": last_name},
            "emails": {"primaryEmail": email},
        }
        payload.update(fields)
        return self._request(
            "POST",
            "/rest/people",
            payload,
            idempotency_key=idempotency_key or str(uuid.uuid4()),
            correlation_id=correlation_id,
        )
