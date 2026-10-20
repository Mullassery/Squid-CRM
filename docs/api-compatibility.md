# API Compatibility Contract

Authority for what the Rust backend must reproduce, and what the Phase 4 slice
currently implements.

Sources, in order of precedence:

1. Golden fixtures from a live `twentycrm/twenty:v2.49.0` in `contracts/v2.49.0/`
   (captured 2026-10-19, see `contracts/README.md`).
2. Source inspection of `twentyhq/twenty@08a0e5f1` (`TWENTY_CURRENT_VERSION =
   2.50.0`) for behaviour not exercised by the fixtures.

The image is one release behind the pinned source; re-capture at v2.50.0 when
published. Every claim below is marked `[fixture]` or `[source]`.

## 1. Endpoints

| Prefix | Upstream purpose | Rust status |
| --- | --- | --- |
| `/graphql` | Workspace **data** GraphQL | **partial** (person, auth helpers) |
| `/metadata` | Metadata GraphQL **and auth mutations** | **partial** (auth helpers, API keys) |
| `/admin-panel` | Admin GraphQL | excluded (licence) |
| `/rest` | REST CRUD | **partial** (people) |
| `/open-api/core`, `/open-api/metadata` | OpenAPI JSON | not implemented |
| `/auth` | OAuth/SSO callbacks only | **partial** (local REST auth) |
| `/client-config` | Frontend runtime config | **implemented** (static feature flags) |
| `/files`, `/file-upload`, `/file` | File storage | not implemented |
| `/webhooks` | Inbound webhooks | not implemented |
| `/healthz` | Health | **implemented** |

Note: `GET /open-api` itself is **404**; only `/open-api/core` and
`/open-api/metadata` exist `[fixture]`.

## 2. Authentication — actual upstream model

- GraphQL auth mutations (`signUp`, `signUpInWorkspace`, `signUpInNewWorkspace`,
  `getLoginTokenFromCredentials`, `getAuthTokensFromLoginToken`,
  `activateWorkspace`, API-key management) are served on **`/metadata`**
  `[fixture]`.
- Workspaces start in `PENDING_CREATION`. `activateWorkspace(data: {})` must run
  before the workspace schema (and therefore the data schema) exists `[fixture]`.
- The **data API** (`/graphql`, `/rest`) accepts **API keys** as
  `Authorization: Bearer <token>` `[fixture]`. A user access token is rejected:
  - REST → `400` `{"statusCode":400,"error":"Error","messages":["Authentication context is invalid"],"code":"NO_AUTHENTICATION_CONTEXT"}`
  - GraphQL resolver → `INVALID_AUTH_CONTEXT`
- API key creation `[fixture]`:
  `createApiKey(input: { name, expiresAt, roleId })` then
  `generateApiKeyToken(apiKeyId, expiresAt)`; the latter returns the bearer token.
- Unauthenticated REST `[fixture]`:
  `403` `{"statusCode":403,"messages":["Missing authentication token"],"error":"FORBIDDEN_EXCEPTION"}`
  (no `code` member).
- The frontend uses a `twenty-session` HttpOnly cookie; cookie issuance is
  restricted to an allowed origin `[source]`.

Implemented in the Rust slice (documented divergences in §8):

- `POST /auth/signup`, `/auth/signin`, `/auth/signout`.
- GraphQL `signUp`, `signIn`, `getLoginTokenFromCredentials`,
  `getAuthTokensFromLoginToken`, `signOut`.
- Argon2id passwords; HS256 JWTs with typed `typ` claims; opaque 32-byte session
  tokens stored as SHA-256 hashes.
- Bearer access tokens and the session cookie are accepted for both REST and
  GraphQL (more permissive than upstream).
- `/metadata` is mounted with the same GraphQL schema, so auth mutations and
  API-key management are reachable there. Metadata object/field queries are not
  implemented.
- API keys: `createApiKey(input: { name, expiresAt, roleId })`,
  `generateApiKeyToken(apiKeyId, expiresAt)`, `revokeApiKey(input: { id })`,
  `apiKeys`, `apiKey(input: { id })`. Tokens are `api_<random>`, stored as
  SHA-256 hashes; revocation is immediate. `roleId` is accepted but the key
  inherits the caller's built-in role until the metadata role engine lands.

## 3. REST data contract `[fixture]`

List — `GET /rest/people?limit=2`:

```json
{
  "data": { "people": [ { "id": "...", "name": { "firstName": "Ada" } } ] },
  "totalCount": 6,
  "pageInfo": {
    "startCursor": "eyJpZCI6IjZjNzc0ZDgwLTczMGYtNGUyMy1iNzA3LTM5MDUzOTc3Y2JmZiJ9",
    "endCursor": "eyJpZCI6IjdhOTNkMWU1LTNmNzQtNDk0NS04YTY1LWQ3Zjk5NjA4M2Y3MiJ9",
    "hasNextPage": true,
    "hasPreviousPage": false
  }
}
```

- Cursor is base64 of `{"id":"<uuid>"}`.
- `hasPreviousPage` was `false` for `offset=1`; do not assume parity from
  `offset > 0` without further fixtures.
- `limit`, `offset` and `order_by` are accepted.

Create — `POST /rest/people` → `201`:

```json
{ "data": { "createPerson": { "id": "...", "name": { ... } } } }
```

Get one — `GET /rest/people/{id}` → `{ "data": { "person": { ... } } }`.

Update — `PATCH /rest/people/{id}` with the partial `PersonForUpdate` body
(fields directly, not wrapped in `data`) → `200`:

```json
{ "data": { "updatePerson": { "id": "...", "jobTitle": "Countess" } } }
```

Delete — `DELETE /rest/people/{id}` → `200`:

```json
{ "data": { "deletePerson": { "id": "..." } } }
```

`?soft_delete=true` performs a recoverable delete; the default is a permanent
delete, matching the OpenAPI default (`soft_delete=false`). Restore —
`PATCH /rest/people/{id}/restore` → `{ "data": { "restorePerson": { ... } } }`.

Errors `[fixture]`:

```json
{ "statusCode": 404, "error": "NotFoundException", "messages": ["Record not found"] }
{ "statusCode": 400, "error": "BadRequestException",
  "messages": ["Invalid string value 'not-an-email' for email field \"emails.primaryEmail\""] }
```

There is no `code` member on these two. The `NO_AUTHENTICATION_CONTEXT` body
does include `code`.

## 4. Metadata contract (Phase 5 slice)

`/metadata` now serves the read path used by the frontend boot query
(`ObjectMetadataItems`):

```graphql
objects(paging: CursorPaging, filter: ObjectFilter): ObjectConnection
object(id: UUID!): Object
fields(paging: CursorPaging, filter: FieldFilter): FieldConnection
field(id: UUID!): Field
Object.fieldsList: [Field!]!
Object.fields(paging: CursorPaging, filter: FieldFilter): ObjectFieldsConnection!
```

Implemented: the standard `person` object, its 16 fields, cursor paging for
`objects`, and id/object/active/system filters. `indexMetadataList` and
`searchFieldMetadataList` are empty; `Field.relation` is `null`. Metadata writes,
views, roles, `minimalMetadata`, and non-person standard objects are not
implemented (see `TODO.md`).

## 5. GraphQL data contract `[fixture]`

```graphql
people(first: Int, last: Int, offset: Int, before: String, after: String,
       filter: PersonFilterInput, orderBy: [PersonOrderByInput]): PersonConnection
person(filter: PersonFilterInput!): Person
createPerson(data: PersonCreateInput!, upsert: Boolean, shareWith: [ShareWithInput!]): Person
updatePerson(id: UUID!, data: PersonUpdateInput!): Person
deletePerson(id: UUID!): Person
destroyPerson(id: UUID!): Person
restorePerson(id: UUID!): Person
mergePeople(ids: [UUID!]!, conflictPriorityIndex: Int!, dryRun: Boolean): Person
createPeople(data: [PersonCreateInput!]!, upsert: Boolean, shareWith: [ShareWithInput!]): [Person!]
updatePeople(data: PersonUpdateInput!, filter: PersonFilterInput!): [Person!]
deletePeople(filter: PersonFilterInput!): [Person!]
destroyPeople(filter: PersonFilterInput!): [Person!]
restorePeople(filter: PersonFilterInput!): [Person!]

type PersonConnection {
  totalCount: Int
  edges: [PersonEdge!]!
  pageInfo: PageInfo!
}
type PersonEdge { node: Person! cursor: Cursor! }
type PageInfo {
  startCursor: ConnectionCursor
  endCursor: ConnectionCursor
  hasNextPage: Boolean!
  hasPreviousPage: Boolean!
}
```

Example fixture: `contracts/v2.49.0/examples/gql-people-connection.json`.

## 6. Person shape `[fixture]`

REST record fields observed on create/list:

```json
{
  "id": "uuid",
  "createdAt": "2026-10-19T21:55:27.301Z",
  "updatedAt": "2026-10-19T21:55:27.301Z",
  "deletedAt": null,
  "name": { "firstName": "Ada", "lastName": "Lovelace" },
  "emails": { "primaryEmail": "ada@example.com", "additionalEmails": [] },
  "linkedinLink": { "primaryLinkLabel": "", "primaryLinkUrl": "", "secondaryLinks": [] },
  "jobTitle": "Mathematician",
  "phones": { "primaryPhoneNumber": "", "primaryPhoneCountryCode": "",
              "primaryPhoneCallingCode": "", "additionalPhones": [] },
  "avatarUrl": "",
  "avatarFile": null,
  "position": 0,
  "createdBy": { "source": "API", "workspaceMemberId": null,
                 "name": "contract-capture", "context": {} },
  "updatedBy": { "source": "API", "workspaceMemberId": null,
                 "name": "contract-capture", "context": {} },
  "searchVector": "'ada':1 'ada@example.com':3 ...",
  "companyId": null
}
```

`additionalEmails` and `additionalPhones` are **empty arrays**, not `null`.

## 7. Pagination, sorting, filtering `[fixture]` / `[source]`

- GraphQL connections use `first`/`last`/`before`/`after`/`offset`.
- REST paging parameters are `limit`, `offset`, `starting_after`, `ending_before`.
  `starting_after=<cursor>` is exclusive and flips `hasPreviousPage` to `true`;
  plain `after=` is not the REST spelling.
- REST sorting: `order_by=field` (ascending, `AscNullsFirst` by default), with
  optional direction in brackets and comma-separated fields:
  `?order_by=createdAt[DescNullsLast],name.firstName[AscNullsFirst]`.
  `-createdAt` is **not** accepted (`FIELD_NOT_FOUND`).
- REST cursors are base64 JSON (`{"id": "<uuid>"}` in the fixtures) and must
  round-trip. `hasPreviousPage` stays `false` for pure `offset` paging even when
  `offset > 0`.
- REST filters use the documented mini-language:
  `?filter=createdAt[gte]:"2024-01-01",name.firstName[startsWith]:A`,
  `or(...)`, `not(...)`, quoted values, and `(a,b)` lists for `in`.
  Implemented fields: `id`, `createdAt`, `updatedAt`, `name.firstName`,
  `name.lastName`, `emails.primaryEmail`, `jobTitle`; comparators: `eq`, `neq`,
  `contains`, `startsWith`, `endsWith`, `like`, `ilike`, `gt`, `gte`, `lt`,
  `lte`, `in`, `is` (`NULL`/`NOT_NULL`). Unknown fields/comparators are `400`.
- GraphQL filters accept the same structured operators under
  `PersonFilterInput` (`and`/`or`/`not`, `id`, `name`, `emails`, `jobTitle`,
  dates). Filtered `totalCount` matches the filtered set.

## 8. Tenant isolation

Every workspace-scoped query resolves `workspace_id` from the principal and
derives a validated schema name (`workspace_<32 hex>`). Integration test
`tenant_isolation_blocks_cross_workspace_reads` proves workspace B cannot read,
list or delete workspace A's records.

## 9. Divergences in the current Rust slice

Alignment completed and covered by tests:

| Area | Contract | Rust status |
| --- | --- | --- |
| REST create | `data.createPerson` | aligned |
| REST delete | `data.deletePerson.id` | aligned |
| REST unauth | `403` `FORBIDDEN_EXCEPTION` + `Missing authentication token` | aligned |
| REST 404 | `NotFoundException`, `Record not found`, no `code` | aligned |
| REST 400 | `BadRequestException`, no `code` | aligned |
| REST cursors | base64 `{"id":"<uuid>"}` | aligned (exact fixture cursor asserted) |
| REST paging | `limit`, `offset`, `starting_after`, `ending_before` | aligned |
| REST sorting | `order_by=field[Direction],...` | aligned for whitelisted person columns |
| `additionalEmails` / `additionalPhones` | `[]`, never `null` | aligned |
| Person fields | phones, links, avatar, position, actors, deletedAt, searchVector, companyId | aligned (see actor/searchVector notes) |
| REST/GraphQL update | `PATCH /rest/people/{id}`, `updatePerson` | aligned; `None` fields unchanged (no explicit null clearing yet) |
| Delete semantics | default hard delete, `soft_delete=true` soft, restore endpoint | aligned |
| GraphQL lifecycle | `deletePerson`, `destroyPerson`, `restorePerson` | aligned |
| GraphQL `people` | `PersonConnection` with `edges`/`pageInfo` | aligned |
| GraphQL filters | `and`/`or`/`not`, composite name/emails, dates | aligned for scalar person fields; relation filters absent |
| GraphQL batch mutations | `createPeople`, `updatePeople`, `deletePeople`, `destroyPeople`, `restorePeople` | aligned; `mergePeople` not implemented |
| Metadata read API | `objects`/`object`/`fields`/`field`, `Object.fieldsList` | aligned for standard person; no writes, views, roles, indexes, relations |
| GraphQL `person(filter:)` | `PersonFilterInput!` | aligned for scalar person fields |
| GraphQL `createPerson(data:)` | `PersonCreateInput!`, `upsert`, `shareWith` | aligned; `upsert`/`shareWith` accepted but not applied |
| GraphQL `deletePerson(id:)` | returns `Person` | aligned |
| GraphQL cursor type | `Cursor` scalar | aligned |
| GraphQL `OrderByDirection` | four null-order variants | aligned |

Known remaining divergences (do not claim parity):

| Area | Upstream | Rust today | Action |
| --- | --- | --- | --- |
| GraphQL auth host | `/metadata` | `/metadata`, `/graphql` and `/auth` | tighten later |
| Data-API auth | API keys only | API keys **and** access token/cookie | tighten before release |
| Workspace bootstrap | `PENDING_CREATION` + `activateWorkspace` | atomic signup + immediate schema | document; add lifecycle |
| Workspace prefill | sample people/companies on activation | none (new workspace has 0 people) | implement prefill |
| Filter coverage | all person fields/relations | scalar person fields (`id`, name, email, jobTitle, timestamps); relation filters absent | expand as metadata lands |
| REST sort/filter errors | `FIELD_NOT_FOUND` for unknown fields | `400 BadRequestException` text | align error code |
| GraphQL nullability | `name`/`emails`/`linkedinLink`/`phones`/`avatarUrl` nullable | non-null | align schema |
| GraphQL scalars | `Position`, `TSVector`, `ConnectionCursor` | `Float`, `String`, `Cursor` | add scalar types |
| `createdBy`/`updatedBy` | actor name = workspace member | `name: "API"` | populate from principal |
| `searchVector` | PostgreSQL `to_tsvector` output | simplified lexeme builder | use `to_tsvector` |
| `avatarFile` | `[FileObject]` | JSON value/null | model file objects |
| REST `fields`/`depth`/`viewId` | supported | ignored/absent | add projection params |
| API-key roles | metadata role entity | caller's built-in role; `roleId` ignored | wire role engine |
| Error `code` for auth | `NO_AUTHENTICATION_CONTEXT` on data API | none | align once access tokens are rejected |

## 10. Verification method

- Golden fixtures: `contracts/v2.49.0/` (SDL, OpenAPI, examples).
- Capture script: `scripts/capture-contracts.py` (reproducible).
- Rust integration tests assert the implemented subset against real PostgreSQL.
- Divergences above are asserted as **known**; tests must not claim parity for
  them. No performance claims are made.
