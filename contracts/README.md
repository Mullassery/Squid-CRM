# Captured API contracts

Golden fixtures captured from a **live** Twenty server. These are test oracles
only — never copy them into `crates/` (see `FIREWALL.md`).

## Contents

| File | What |
| --- | --- |
| `graphql.graphql` / `graphql-introspection.json` | Workspace **data** schema (716 types after activation) |
| `metadata.graphql` / `metadata-introspection.json` | Metadata + auth schema (655/656 types) |
| `admin.graphql` / `admin-introspection.json` | Admin schema (109 types) |
| `openapi-core.json` | `GET /open-api/core` |
| `openapi-metadata.json` | `GET /open-api/metadata` |
| `client-config.json` | `GET /client-config` |
| `examples/` | Real request/response pairs (REST + GraphQL) |

## v2.50.0 (authoritative)

- Source: `twentyhq/twenty@08a0e5f1ee2e7db7c1e6ef3aeec9bcef731f4e98`
  (`TWENTY_CURRENT_VERSION = 2.50.0`).
- Image: no Docker Hub tag existed at capture time, so the pinned source was
  built locally: `docker build --target twenty-server -t
  squid-twenty:v2.50.0-pinned -f packages/twenty-docker/twenty/Dockerfile .`
  Image ID `sha256:2f208ac7774ea6e4b46c4974e211d85dddf2468c53b4675c3e623918bb4c146f`.
- Captured 2026-10-20 with `scripts/capture-contracts.py --tag v2.50.0` and
  `scripts/capture-examples.py --tag v2.50.0` on a fresh database after
  `activateWorkspace` (required before the workspace data schema exists).
- `capture.json` records the tag, timestamp, user, workspace and type counts
  (data 716 types, metadata 656, admin 109).
- Diff against v2.49.0: the data and admin schemas are unchanged; metadata adds
  `ApplicationPreferences`, workspace credit fields and `CUSTOM_ROLES`, and
  removes `IS_JSON_FILTER_ENABLED` / `IS_RECORD_SHARE_VISIBILITY_GATING_ENABLED`.
  OpenAPI paths and component schemas are unchanged.

## v2.49.0 (historical)

- Image: `twentycrm/twenty:v2.49.0` (Docker Hub digest
  `sha256:80e1b33b0b4b9dc292e776c64c320be2ae9467677ce639ba9bde1cffca68a3b0`)
- These fixtures are one release behind the audited source. Keep them only for
  diffing; `v2.50.0/` is the golden oracle.
- Captured by `scripts/capture-contracts.py` on a fresh database with the
  default single-workspace configuration.

### How it was captured

```bash
# Use the published image or a locally built pinned-image tag.
TWENTY_IMAGE=squid-twenty:v2.50.0-pinned \
  APP_SECRET=$(openssl rand -hex 32) \
  ENCRYPTION_KEY=$(openssl rand -hex 32) \
  docker compose -p twenty-capture-v250 \
  -f reference/capture/docker-compose.yml up -d
# wait for /healthz, then:
python3 scripts/capture-contracts.py --tag v2.50.0 --out contracts/v2.50.0
python3 scripts/capture-examples.py --tag v2.50.0
```

`capture-contracts.py` signs up a user, creates a workspace, activates it
(`activateWorkspace(data: {})`), then introspects. `capture-examples.py` signs
in with the captured user and issues an Admin-role API key, because the data API
rejects user access tokens.

The script signs up a throwaway user, creates and **activates** a workspace
(required before the data schema exists), then introspects the three schemas.

### Important discovery

Auth mutations live on **`/metadata`**, not `/graphql`. The data API
(`/graphql`, `/rest`) accepts **API keys**; a user access token is rejected for
REST (`NO_AUTHENTICATION_CONTEXT`) and for data resolvers
(`INVALID_AUTH_CONTEXT`) even though it can introspect the schema.

### Reproducing authenticated examples

1. Create an API key (`createApiKey` + `generateApiKeyToken` on `/metadata`).
2. Use `Authorization: Bearer <api-key-token>` against `/rest/people` and
   `/graphql`.
