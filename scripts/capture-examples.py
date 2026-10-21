#!/usr/bin/env python3
"""Capture REST/GraphQL example responses from a running Twenty server.

Signs in with the capture user from an existing contracts/<tag>/capture.json,
issues an API key (the data API rejects user access tokens), then records the
same request/response pairs as the v2.49.0 fixtures.

Usage:
  python3 scripts/capture-examples.py --tag v2.50.0
"""

import argparse
import json
import os
import secrets
import time
import urllib.error
import urllib.request


def post(url, payload, token=None):
    request = urllib.request.Request(
        url, data=json.dumps(payload).encode(), method="POST"
    )
    request.add_header("Content-Type", "application/json")
    if token:
        request.add_header("Authorization", f"Bearer {token}")
    try:
        with urllib.request.urlopen(request, timeout=60) as response:
            return response.status, json.loads(response.read())
    except urllib.error.HTTPError as error:
        return error.code, json.loads(error.read())


def send(method, url, payload=None, token=None):
    data = json.dumps(payload).encode() if payload is not None else None
    request = urllib.request.Request(url, data=data, method=method)
    request.add_header("Content-Type", "application/json")
    if token:
        request.add_header("Authorization", f"Bearer {token}")
    try:
        with urllib.request.urlopen(request, timeout=60) as response:
            return response.status, json.loads(response.read())
    except urllib.error.HTTPError as error:
        return error.code, json.loads(error.read())


def gql(base, query, variables=None, token=None):
    status, body = post(
        f"{base}/metadata" if token is None else f"{base}/metadata",
        {"query": query, "variables": variables or {}},
        token,
    )
    if body.get("errors"):
        raise RuntimeError(f"metadata GraphQL error: {body['errors']}")
    return body["data"]


def data_gql(base, query, variables=None, token=None):
    status, body = post(
        f"{base}/graphql", {"query": query, "variables": variables or {}}, token
    )
    if body.get("errors"):
        raise RuntimeError(f"data GraphQL error: {body['errors']}")
    return body["data"]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--base-url", default="http://localhost:3000")
    parser.add_argument("--tag", required=True)
    parser.add_argument("--password", default="CorrectHorseBatteryStaple1!")
    parser.add_argument("--out", help="Defaults to contracts/<tag>/examples")
    args = parser.parse_args()

    base = args.base_url.rstrip("/")
    out = args.out or os.path.join("contracts", args.tag, "examples")
    os.makedirs(out, exist_ok=True)

    capture_path = os.path.join("contracts", args.tag, "capture.json")
    capture = json.load(open(capture_path))
    email = capture["testUser"]

    login = gql(
        base,
        """
        mutation($email: String!, $password: String!, $origin: String!) {
          getLoginTokenFromCredentials(email: $email, password: $password, origin: $origin) {
            loginToken { token }
          }
        }
        """,
        {"email": email, "password": args.password, "origin": base},
    )["getLoginTokenFromCredentials"]["loginToken"]["token"]

    session = gql(
        base,
        """
        mutation($loginToken: String!, $origin: String!) {
          getAuthTokensFromLoginToken(loginToken: $loginToken, origin: $origin) {
            tokens { accessOrWorkspaceAgnosticToken { token } }
          }
        }
        """,
        {"loginToken": login, "origin": base},
    )["getAuthTokensFromLoginToken"]["tokens"]["accessOrWorkspaceAgnosticToken"]["token"]

    roles = gql(base, "{ getRoles { id label } }", token=session)["getRoles"]
    role_id = next(
        (role["id"] for role in roles if role["label"] == "Admin"),
        roles[0]["id"],
    )
    expires_at = "2030-01-01T00:00:00.000Z"
    api_key_id = gql(
        base,
        """
        mutation($name: String!, $expiresAt: String!, $roleId: UUID!) {
          createApiKey(input: { name: $name, expiresAt: $expiresAt, roleId: $roleId }) {
            id
          }
        }
        """,
        {"name": f"contract-examples-{secrets.token_hex(3)}", "expiresAt": expires_at, "roleId": role_id},
        token=session,
    )["createApiKey"]["id"]
    api_key = gql(
        base,
        """
        mutation($apiKeyId: UUID!, $expiresAt: String!) {
          generateApiKeyToken(apiKeyId: $apiKeyId, expiresAt: $expiresAt) { token }
        }
        """,
        {"apiKeyId": api_key_id, "expiresAt": expires_at},
        token=session,
    )["generateApiKeyToken"]["token"]

    def save(name, status, body, expected):
        if status != expected:
            raise RuntimeError(f"{name}: expected {expected}, got {status}: {body}")
        with open(os.path.join(out, name), "w") as fh:
            json.dump(body, fh, indent=2)
            fh.write("\n")
        print(f"{name}: {status}")

    ada = {
        "name": {"firstName": "Ada", "lastName": "Lovelace"},
        "emails": {"primaryEmail": "ada@example.com"},
        "jobTitle": "Mathematician",
    }
    status, created = send("POST", f"{base}/rest/people", ada, api_key)
    save("rest-create-person.json", status, created, 201)
    person_id = created["data"]["createPerson"]["id"]

    status, body = send("GET", f"{base}/rest/people/{person_id}", token=api_key)
    save("rest-get-person.json", status, body, 200)

    status, body = send("GET", f"{base}/rest/people?limit=2", token=api_key)
    save("rest-list-people.json", status, body, 200)

    for first, last, title in (
        ("Ivan", "Zhao", "Engineer"),
        ("Dario", "Amodei", "Researcher"),
    ):
        data_gql(
            base,
            """
            mutation($data: PersonCreateInput!) {
              createPerson(data: $data) { id }
            }
            """,
            {
                "data": {
                    "name": {"firstName": first, "lastName": last},
                    "jobTitle": title,
                }
            },
            api_key,
        )
        time.sleep(0.01)

    status, body = post(
        f"{base}/graphql",
        {
            "query": """
            mutation {
              createPerson(data: {
                name: { firstName: "Grace", lastName: "Hopper" }
                jobTitle: "Rear Admiral"
              }) { id name { firstName lastName } jobTitle }
            }
            """
        },
        api_key,
    )
    save("gql-create-person.json", status, body, 200)

    status, body = post(
        f"{base}/graphql",
        {
            "query": """
            {
              people(first: 2) {
                totalCount
                edges {
                  node { id name { firstName lastName } }
                  cursor
                }
                pageInfo { startCursor endCursor hasNextPage hasPreviousPage }
              }
            }
            """
        },
        api_key,
    )
    save("gql-people-connection.json", status, body, 200)

    status, body = send("DELETE", f"{base}/rest/people/{person_id}", token=api_key)
    save("rest-delete-person.json", status, body, 200)

    status, body = send("GET", f"{base}/rest/people")
    save("rest-error-403-no-auth.json", status, body, 403)

    status, body = send(
        "GET", f"{base}/rest/people/{'0' * 8}-0000-4000-8000-{'0' * 12}", token=api_key
    )
    save("rest-error-404.json", status, body, 404)

    status, body = send(
        "POST",
        f"{base}/rest/people",
        {
            "name": {"firstName": "Broken", "lastName": "Email"},
            "emails": {"primaryEmail": "not-an-email"},
        },
        api_key,
    )
    save("rest-error-400.json", status, body, 400)


if __name__ == "__main__":
    main()
