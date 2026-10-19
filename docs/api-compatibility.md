# API Compatibility Contract

Derived from `twentyhq/twenty@08a0e5f1` (server `2.50.0`). This document is the
authority for what the Rust backend must reproduce and what the Phase 4 slice
currently implements.

## 1. Transports and prefixes

| Prefix | Upstream purpose | Rust status |
| --- | --- | --- |
| `/graphql` | Workspace data + auth GraphQL | **partial** (auth + person) |
| `/metadata` | Metadata GraphQL | not started |
| `/admin-panel` | Admin GraphQL | excluded (licence) |
| `/rest` | REST CRUD | **partial** (people) |
| `/open-api` | Generated OpenAPI document | not started |
| `/auth` | Sign-up/in, OAuth callbacks | **partial** (local email/password) |
| `/files`, `/file-upload`, `/file` | File storage | not started |
| `/webhooks` | Inbound webhooks | not started |
| `/client-config` | Frontend runtime config | not started |
| `/healthz` | Health | **implemented** |

## 2. Authentication

Upstream supports JWT bearer, cookie sessions (`twenty-session`, HttpOnly), API
keys, app tokens, Google/Microsoft OAuth, SAML/OIDC (Enterprise) and 2FA.

Implemented in the slice:

- `POST /auth/signup` — `{ email, password, workspaceName? }`; atomically creates
  user + workspace + OWNER membership, creates the workspace schema, returns
  tokens and sets `twenty-session`.
- `POST /auth/signin` — `{ email, password }`; returns tokens and sets the cookie.
- `POST /auth/signout` — clears the session and cookie.
- `Authorization: Bearer <access JWT>` — `typ=access` claim required.
- Cookie session — opaque 32-byte token, stored **only as SHA-256 hash**.
- Login tokens — `typ=login`, short-lived, cannot be replayed as access tokens.

GraphQL equivalents: `signUp`, `signIn`, `getLoginTokenFromCredentials`,
`getAuthTokensFromLoginToken`, `signOut`.

**Not yet implemented:** API keys, app tokens, OAuth providers, 2FA, refresh-token
rotation endpoint, workspace switching, SSO (Enterprise, excluded).

## 3. REST envelope (must not drift)

List (`GET /rest/people`):

```json
{
  "data": { "people": [ /* records */ ] },
  "totalCount": 1,
  "pageInfo": {
    "hasNextPage": false,
    "hasPreviousPage": false,
    "startCursor": null,
    "endCursor": null
  }
}
```

Create: `201` with `{ "data": { "person": { ... } } }`.
Get one: `200` with `{ "data": { "person": { ... } } }`.
Delete: soft delete; subsequent read returns `404`.

Errors (all REST failures):

```json
{ "statusCode": 401, "error": "UnauthorizedException",
  "messages": ["Authentication required"], "code": "UNAUTHENTICATED" }
```

Status names follow upstream: `BadRequestException`, `UnauthorizedException`,
`ForbiddenException`, `NotFoundException`, `ConflictException`,
`UnprocessableEntityException`, `TooManyRequestsException`,
`PayloadTooLargeException`, `InternalServerErrorException`.

## 4. Object shape — Person (Phase 4 subset)

Implemented fields:

```json
{
  "id": "uuid",
  "name": { "firstName": "Ada", "lastName": "Lovelace" },
  "emails": { "primaryEmail": "ada@example.com", "additionalEmails": null },
  "jobTitle": "Engineer",
  "createdAt": "2026-01-01T00:00:00Z",
  "updatedAt": "2026-01-01T00:00:00Z"
}
```

Upstream also has `phones`, `linkedinLink`, `avatarFile`, `position`, `createdBy`,
`updatedBy`, `company`, relations and `searchVector`. Those are **not yet**
implemented and are tracked in `migration-matrix.md`.

## 5. Pagination, sorting, filtering

Upstream uses cursor pagination (`starting_after`, `ending_before`), `limit`,
`order_by`, `filter`, `fields`, `depth`, `viewId`.

Slice implements `limit` (clamped to `MAX_PAGE_SIZE`) and `offset`, and returns
`pageInfo` with cursor fields set to `null`. **This is a known gap**: cursor
semantics, `order_by`, `filter` grammar and field selection must be implemented
before parity. The frontend's Apollo cache also expects cursor-based connections.

## 6. Idempotency and retries

Not yet implemented. Upstream retry/idempotency behaviour must be mapped in a
later phase (webhooks and job queues are the main consumers). No claim is made
that current behaviour matches.

## 7. Tenant isolation

Every workspace-scoped query resolves `workspace_id` from the authenticated
principal and derives a validated schema name (`workspace_<32 hex>`). No client
input reaches a schema identifier. Integration test
`tenant_isolation_blocks_cross_workspace_reads` proves workspace B receives
`404` for workspace A's records and an empty list.

## 8. GraphQL specifics

- Input and output types with the same name cannot coexist in `async-graphql`;
  `CreatePersonInput` is exposed as an input object and `Person` as output.
- Field naming is camelCase (matches the frontend fragments).
- Errors carry `extensions.code` where applicable.

## 9. Compatibility verification method

- Rust integration tests (`crates/squid-api/tests/integration.rs`) assert the
  envelopes above against real PostgreSQL.
- Contract fixtures captured from the running upstream are **not yet** in
  `contracts/` (blocked on the reference stack booting in this environment).
  Until captured, compatibility is asserted from source inspection, not from
  live golden files. This limitation is recorded in `TODO.md`.
