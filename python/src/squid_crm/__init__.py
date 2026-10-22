"""Python client for the Squid CRM API."""

from __future__ import annotations

import json
import urllib.error
import urllib.parse
import urllib.request
from typing import Any, Dict, Optional

__version__ = "0.1.0"

__all__ = ["SquidClient", "SquidError", "__version__"]


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
    """

    def __init__(
        self,
        base_url: str,
        api_key: Optional[str] = None,
        timeout: float = 30.0,
    ) -> None:
        self.base_url = base_url.rstrip("/")
        self.api_key = api_key
        self.timeout = timeout

    def _request(
        self,
        method: str,
        path: str,
        payload: Optional[Dict[str, Any]] = None,
        params: Optional[Dict[str, Any]] = None,
    ) -> Any:
        url = f"{self.base_url}{path}"
        if params:
            query = urllib.parse.urlencode(
                {key: value for key, value in params.items() if value is not None}
            )
            if query:
                url = f"{url}?{query}"
        data = None
        headers = {"Accept": "application/json"}
        if payload is not None:
            data = json.dumps(payload).encode("utf-8")
            headers["Content-Type"] = "application/json"
        if self.api_key:
            headers["Authorization"] = f"Bearer {self.api_key}"
        request = urllib.request.Request(url, data=data, headers=headers, method=method)
        try:
            with urllib.request.urlopen(request, timeout=self.timeout) as response:
                raw = response.read()
                return json.loads(raw) if raw else None
        except urllib.error.HTTPError as error:
            raw = error.read()
            try:
                body = json.loads(raw)
            except ValueError:
                body = raw.decode("utf-8", errors="replace")
            message = body.get("messages") if isinstance(body, dict) else body
            raise SquidError(str(message), status=error.code, body=body) from error
        except urllib.error.URLError as error:
            raise SquidError(str(error.reason)) from error

    def graphql(
        self,
        query: str,
        variables: Optional[Dict[str, Any]] = None,
    ) -> Any:
        """Execute a GraphQL operation against ``/graphql``."""
        response = self._request(
            "POST",
            "/graphql",
            {"query": query, "variables": variables or {}},
        )
        if isinstance(response, dict) and response.get("errors"):
            raise SquidError(str(response["errors"]), body=response)
        return response.get("data") if isinstance(response, dict) else response

    def list_people(self, limit: int = 20, offset: int = 0) -> Any:
        """List people through the REST data API."""
        return self._request(
            "GET",
            "/rest/people",
            params={"limit": limit, "offset": offset},
        )

    def create_person(
        self,
        first_name: str,
        last_name: str,
        email: str,
        **fields: Any,
    ) -> Any:
        """Create a person through the REST data API."""
        payload: Dict[str, Any] = {
            "name": {"firstName": first_name, "lastName": last_name},
            "emails": {"primaryEmail": email},
        }
        payload.update(fields)
        return self._request("POST", "/rest/people", payload)
