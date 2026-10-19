#!/usr/bin/env python3
"""Capture GraphQL introspection, OpenAPI and client config from a running Twenty server.

Usage:
  python3 scripts/capture-contracts.py --tag v2.49.0 --out contracts/v2.49.0
  python3 scripts/capture-contracts.py --tag v2.49.0 --out out --access-token "$TOKEN"

Creates a throwaway user/workspace unless --access-token is supplied. Stdlib only.
SDL conversion uses graphql-core when available (optional).
"""

import argparse
import json
import os
import secrets
import sys
import time
import urllib.error
import urllib.request

INTROSPECTION_QUERY = """
query IntrospectionQuery {
  __schema {
    queryType { name }
    mutationType { name }
    subscriptionType { name }
    types { ...FullType }
    directives { name description locations args { ...InputValue } }
  }
}
fragment FullType on __Type {
  kind name description
  fields(includeDeprecated: true) {
    name description
    args { ...InputValue }
    type { ...TypeRef }
    isDeprecated deprecationReason
  }
  inputFields { ...InputValue }
  interfaces { ...TypeRef }
  enumValues(includeDeprecated: true) { name description isDeprecated deprecationReason }
  possibleTypes { ...TypeRef }
}
fragment InputValue on __InputValue {
  name description type { ...TypeRef } defaultValue
}
fragment TypeRef on __Type {
  kind name
  ofType { kind name ofType { kind name ofType { kind name ofType { kind name
    ofType { kind name ofType { kind name ofType { kind name } } } } } } }
}
"""

SCHEMAS = (("graphql", "/graphql"), ("metadata", "/metadata"), ("admin", "/admin-panel"))
OPENAPI = (("openapi-core", "/open-api/core"), ("openapi-metadata", "/open-api/metadata"))


def post(url, payload, token=None):
    req = urllib.request.Request(url, data=json.dumps(payload).encode(), method="POST")
    req.add_header("Content-Type", "application/json")
    if token:
        req.add_header("Authorization", f"Bearer {token}")
    with urllib.request.urlopen(req, timeout=60) as resp:
        return json.loads(resp.read())


def gql(url, query, variables=None, token=None):
    result = post(url, {"query": query, "variables": variables or {}}, token)
    if result.get("errors") and not result.get("data"):
        raise RuntimeError(f"GraphQL error at {url}: {result['errors']}")
    return result


def get(url, token=None):
    req = urllib.request.Request(url)
    if token:
        req.add_header("Authorization", f"Bearer {token}")
    with urllib.request.urlopen(req, timeout=60) as resp:
        return resp.status, resp.headers.get("content-type"), resp.read()


def write_sdl(introspection_path, sdl_path):
    try:
        from graphql import build_client_schema, print_schema
    except ImportError:
        return False
    with open(introspection_path) as fh:
        document = json.load(fh)
    schema = build_client_schema(document["data"])
    with open(sdl_path, "w") as fh:
        fh.write(print_schema(schema))
    return True


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--base-url", default="http://localhost:3000")
    ap.add_argument("--tag", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--password", default="CorrectHorseBatteryStaple1!")
    ap.add_argument("--access-token", help="Reuse an existing workspace access token")
    ap.add_argument("--email", help="Label for the capture user when reusing a token")
    ap.add_argument("--workspace-id", help="Label for the workspace when reusing a token")
    args = ap.parse_args()

    base = args.base_url.rstrip("/")
    os.makedirs(args.out, exist_ok=True)

    email = args.email or "(reused token)"
    workspace_id = args.workspace_id or "(reused token)"
    access = args.access_token

    if not access:
        email = f"contract-{int(time.time())}-{secrets.token_hex(4)}@squid.test"
        signup = gql(
            f"{base}/metadata",
            """
            mutation($email: String!, $password: String!) {
              signUp(email: $email, password: $password) {
                tokens { accessOrWorkspaceAgnosticToken { token expiresAt } }
              }
            }
            """,
            {"email": email, "password": args.password},
        )
        wsa = signup["data"]["signUp"]["tokens"]["accessOrWorkspaceAgnosticToken"]["token"]

        new_ws = gql(
            f"{base}/metadata",
            """
            mutation {
              signUpInNewWorkspace(input: { displayName: "Contract Capture" }) {
                loginToken { token expiresAt }
                workspace { id workspaceUrls { subdomainUrl customUrl } }
              }
            }
            """,
            token=wsa,
        )
        workspace_id = new_ws["data"]["signUpInNewWorkspace"]["workspace"]["id"]
        login_token = new_ws["data"]["signUpInNewWorkspace"]["loginToken"]["token"]

        exchanged = gql(
            f"{base}/metadata",
            """
            mutation($loginToken: String!) {
              getAuthTokensFromLoginToken(loginToken: $loginToken, origin: "%s") {
                tokens { accessOrWorkspaceAgnosticToken { token expiresAt } }
              }
            }
            """
            % base,
            {"loginToken": login_token},
        )
        access = exchanged["data"]["getAuthTokensFromLoginToken"]["tokens"][
            "accessOrWorkspaceAgnosticToken"
        ]["token"]

    summary = {
        "tag": args.tag,
        "baseUrl": base,
        "capturedAt": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "testUser": email,
        "workspaceId": workspace_id,
        "schemas": {},
        "openapi": {},
        "endpoints": {},
    }

    for name, path in SCHEMAS:
        result = gql(f"{base}{path}", INTROSPECTION_QUERY, token=access)
        schema = result.get("data", {}).get("__schema")
        if schema is None:
            summary["schemas"][name] = {"status": "no-schema", "errors": result.get("errors")}
            continue
        intro_path = os.path.join(args.out, f"{name}-introspection.json")
        with open(intro_path, "w") as fh:
            json.dump({"data": {"__schema": schema}}, fh, indent=2, sort_keys=True)
        entry = {
            "file": os.path.basename(intro_path),
            "types": len(schema["types"]),
            "queryType": (schema.get("queryType") or {}).get("name"),
            "mutationType": (schema.get("mutationType") or {}).get("name"),
        }
        sdl_path = os.path.join(args.out, f"{name}.graphql")
        if write_sdl(intro_path, sdl_path):
            entry["sdl"] = os.path.basename(sdl_path)
        summary["schemas"][name] = entry

    for name, path in OPENAPI:
        try:
            status, ctype, body = get(f"{base}{path}", token=access)
        except urllib.error.HTTPError as exc:
            summary["openapi"][name] = {"status": exc.code}
            continue
        filename = f"{name}.json"
        with open(os.path.join(args.out, filename), "wb") as fh:
            fh.write(body)
        summary["openapi"][name] = {"file": filename, "status": status, "contentType": ctype}

    status, ctype, body = get(f"{base}/client-config")
    with open(os.path.join(args.out, "client-config.json"), "wb") as fh:
        fh.write(body)
    summary["endpoints"]["/client-config"] = {"status": status, "contentType": ctype}

    with open(os.path.join(args.out, "capture.json"), "w") as fh:
        json.dump(summary, fh, indent=2)

    print(json.dumps(summary, indent=2))


if __name__ == "__main__":
    sys.exit(main())
