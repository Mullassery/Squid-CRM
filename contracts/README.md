# Captured API contracts

Golden fixtures captured from a **live** Twenty server. These are test oracles
only — never copy them into `crates/` (see `FIREWALL.md`).

## v2.49.0

- Image: `twentycrm/twenty:v2.49.0` (Docker Hub digest
  `sha256:80e1b33b0b4b9dc292e776c64c320be2ae9467677ce639ba9bde1cffca68a3b0`)
- Pinned source under audit: `twentyhq/twenty@08a0e5f1`, `TWENTY_CURRENT_VERSION
  = 2.50.0`. **No 2.50.0 image exists yet** (latest published tag at capture time
  was `v2.49.0`), so these fixtures are one release behind the audited source.
  Re-capture when `v2.50.0` is published.
- Captured by `scripts/capture-contracts.py` on a fresh database with the
  default single-workspace configuration.
- `capture.json` records the tag, timestamp, user, workspace and type counts.

### Contents

| File | What |
| --- | --- |
| `graphql.graphql` / `graphql-introspection.json` | Workspace **data** schema (716 types after activation) |
| `metadata.graphql` / `metadata-introspection.json` | Metadata + auth schema (655 types) |
| `admin.graphql` / `admin-introspection.json` | Admin schema (109 types) |
| `openapi-core.json` | `GET /open-api/core` |
| `openapi-metadata.json` | `GET /open-api/metadata` |
| `client-config.json` | `GET /client-config` |
| `examples/` | Real request/response pairs (REST + GraphQL) |

### How it was captured

```bash
docker compose -f reference/capture/docker-compose.yml up -d
# wait for /healthz, then:
python3 scripts/capture-contracts.py --tag v2.49.0 --out contracts/v2.49.0
```

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
