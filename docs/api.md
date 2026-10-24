# API Reference

Squid-CRM exposes a REST data API, a GraphQL data API, and a GraphQL metadata
API. All endpoints are served by `squid-server` (Axum).

## Base URLs

| Prefix | Purpose |
| --- | --- |
| `/rest` | REST CRUD for standard and custom objects |
| `/graphql` | GraphQL data API (records, search, files, jobs) |
| `/metadata` | GraphQL metadata, auth mutations, roles, API keys, webhooks, views |
| `/auth` | Session signup, signin, refresh and signout |
| `/files` | Signed upload and download URLs |
| `/client-config` | Public runtime configuration for the web client |
| `/healthz` | Liveness probe (`ok`) |

The web client proxies `/api/*` to this server. In direct deployments, call the
prefixes above from the browser origin allowed by `FRONTEND_URL`.

## Authentication

### Sessions

`POST /auth/signup` and `POST /auth/signin` return an access/refresh token pair
and set an HttpOnly `squid-session` cookie:

```json
{
  "tokens": {
    "accessOrWorkspaceAgnosticToken": { "token": "...", "expiresAt": "..." },
    "refreshToken": { "token": "...", "expiresAt": "..." }
  },
  "user": { "id": "...", "email": "..." },
  "workspace": { "id": "...", "displayName": "...", "subdomain": "..." }
}
```

- Signup body: `{ "email", "password", "workspaceName" }`.
- Signin body: `{ "email", "password" }`.
- Refresh body: `{ "refreshToken" }`; refresh tokens rotate and replay is
  rejected.
- Signout: `POST /auth/signout`; deletes the session and clears the cookie.

### API keys

Create keys through the metadata GraphQL API:

```graphql
mutation {
  createApiKey(input: {
    name: "ci"
    expiresAt: "2027-01-01T00:00:00Z"
    roleId: "00000000-0000-0000-0000-000000000000"
  }) { id name }
}
```

Then generate the one-time token:

```graphql
mutation { generateApiKeyToken(apiKeyId: "...", expiresAt: "2027-01-01T00:00:00Z") { token } }
```

Send the token as `Authorization: Bearer api_...` on `/rest` and `/graphql`.
User access tokens are accepted on `/metadata` only.

## REST

### Envelopes

List:

```json
{
  "data": { "people": [ ... ] },
  "totalCount": 6,
  "pageInfo": {
    "startCursor": "eyJpZCI6Ii4uLiJ9",
    "endCursor": "eyJpZCI6Ii4uLiJ9",
    "hasNextPage": true,
    "hasPreviousPage": false
  }
}
```

Create/update/delete:

```json
{ "data": { "createPerson": { "id": "..." } } }
{ "data": { "updatePerson": { "id": "..." } } }
{ "data": { "deletePerson": { "id": "..." } } }
```

Get one:

```json
{ "data": { "person": { "id": "..." } } }
```

Errors:

```json
{ "statusCode": 404, "error": "NotFoundException", "messages": ["Record not found"] }
{ "statusCode": 400, "error": "BadRequestException", "messages": ["..."] }
```

Authentication failures include `code` when present:

```json
{
  "statusCode": 403,
  "messages": ["Missing authentication token"],
  "error": "FORBIDDEN_EXCEPTION"
}
```

### Standard resources

| Resource | List key | Create | Update | Delete |
| --- | --- | --- | --- | --- |
| `/rest/people` | `people` | `createPerson` | `updatePerson` | `deletePerson` |
| `/rest/companies` | `companies` | `createCompany` | `updateCompany` | `deleteCompany` |
| `/rest/opportunities` | `opportunities` | `createOpportunity` | `updateOpportunity` | `deleteOpportunity` |
| `/rest/tasks` | `tasks` | `createTask` | `updateTask` | `deleteTask` |
| `/rest/notes` | `notes` | `createNote` | `updateNote` | `deleteNote` |
| `/rest/taskTargets` | `taskTargets` | `createTaskTarget` | `updateTaskTarget` | `deleteTaskTarget` |
| `/rest/noteTargets` | `noteTargets` | `createNoteTarget` | `updateNoteTarget` | `deleteNoteTarget` |

Each resource also supports:

- `GET/PATCH/DELETE /rest/{resource}/{id}`
- `PATCH /rest/{resource}/{id}/restore`
- `GET /rest/{resource}/export` (CSV)
- `POST /rest/{resource}/import` (CSV, `text/csv` body)
- `GET /rest/{resource}/groupBy`

Custom objects use the same REST fallback under `/rest/{objectNamePlural}` and
`/rest/{objectNamePlural}/{id}`, with `/rest/{objectNamePlural}/upsert` for
idempotent batch writes.

### Query parameters

| Parameter | Meaning |
| --- | --- |
| `limit` | Page size (default 20, capped by `MAX_PAGE_SIZE`) |
| `offset` | Offset paging |
| `starting_after` / `ending_before` | Base64 cursor of `{"id":"<uuid>"}` |
| `order_by` | `field[AscNullsFirst]`, `field[DescNullsLast]`, comma-separated |
| `filter` | Filter expression (`field[eq]:value`, `and`, `or`, `not`) |
| `fields` | Comma-separated top-level field projection |
| `depth` | `0` or `1` |

Cursors are base64-encoded JSON: `{"id":"<uuid>"}`.

### Idempotency

`POST /rest/*` and data GraphQL POSTs accept `Idempotency-Key`. The first
successful response is replayed for the same key. Reusing a key with a different
body returns `409 Conflict`. Keys are workspace-scoped.

## GraphQL

POST JSON to `/graphql` or `/metadata`:

```json
{ "query": "query { people(first: 20) { edges { node { id } } } }", "variables": {} }
```

### Data roots

Queries: `people`, `person`, `companies`, `company`, `opportunities`,
`opportunity`, `tasks`, `task`, `notes`, `note`, `taskTargets`, `taskTarget`,
`noteTargets`, `noteTarget`, `workspaceMembers`, `workspaceMember`, `search`,
`customObjectRecords`, `customObjectRecord`, `currentUser`, `currentWorkspace`.

Mutations: single and batch create/update/delete/restore for the standard
objects; `mergePeople`; target create/update/delete/restore; custom-object
record create/update/delete; file upload; jobs.

### Metadata roots

Queries: `objects`, `object`, `fields`, `field`, `getViews`, `getView`,
`getViewFields`, `getViewFilters`, `getViewSorts`, `getViewGroups`,
`getViewFieldGroups`, `getViewFilterGroups`, `getRoles`, `getRole`,
`getApiKeyRoles`, `getPermissionFlags`, `apiKeys`, `apiKey`, `webhooks`,
`webhook`, `webhookDeliveries`, `jobs`, `mostlyEmptyFieldMetadataIds`.

Mutations: object/field/index CRUD; view and view child CRUD; role CRUD and
member/API-key assignment; permission flags; API-key create/generate/revoke;
webhook CRUD and replay; job enqueue/run; file upload; auth mutations.

Auth mutations (`signUp`, `signIn`, `getLoginTokenFromCredentials`,
`getAuthTokensFromLoginToken`, `renewToken`, `signOut`) are also available on
`/graphql` for client convenience.

### Search

```graphql
query Search($q: String!, $limit: Int!) {
  search(searchInput: $q, limit: $limit) {
    edges {
      node {
        recordId
        objectNameSingular
        objectLabelSingular
        label
        imageUrl
        tsRank
        tsRankCD
      }
      cursor
    }
    pageInfo { hasNextPage endCursor }
  }
}
```

## Files

```graphql
mutation {
  createFileUpload(
    filename: "contract.pdf"
    size: 1234
    fileFolder: FilesField
  ) { fileId uploadUrl contentType expiresAt }
}
```

Upload bytes with `PUT` to `uploadUrl`, then call `completeFileUpload`. Download
URLs are HMAC-signed and expire. `UPLOAD_FILE` and `DOWNLOAD_FILE` are enforced
for principals with permission flags.

## Error handling

GraphQL errors use the standard `errors` array with `message`, `path` and
`extensions.code`. REST errors use the envelope shown above. All responses
include `x-correlation-id`; send `x-correlation-id` or `x-request-id` to join
logs.

## Limits

- Request bodies: 2 MiB.
- Page size: `MAX_PAGE_SIZE` (default 100).
- Auth attempts: 10 per 15 minutes per account (in-process).
