# Migration Matrix

Status legend: `not-started` · `partial` · `parity` · `excluded (licence)` · `deferred`

Reference pin: `twentyhq/twenty@08a0e5f1ee2e7db7c1e6ef3aeec9bcef731f4e98`
(server `2.50.0`). Golden contracts: `contracts/v2.50.0/`.
Operation coverage: `docs/frontend-operation-map.md`, `docs/rest-route-map.md`.

| Upstream area | Upstream path | Rust crate | Status | Parity evidence | Notes |
| --- | --- | --- | --- | --- | --- |
| Core config / boot | `src/engine/core-modules/twenty-config` | `squid-core` | partial | smoke boot, unit tests | env-driven config only |
| Logger / metrics / telemetry | `core-modules/logger`, `metrics`, `telemetry`, `sentry` | `squid-core` | partial | `tracing` output | metrics/Sentry not implemented |
| Auth: sessions, JWT, API keys | `core-modules/auth`, `api-key`, `app-token`, `jwt` | `squid-auth` | partial | `signup_signin_and_person_crud_over_rest`, `refresh_tokens_rotate_and_signout_revokes_the_session`, `graphql_renew_token_rotates_refresh_tokens`, `cookie_requests_reject_foreign_origins` | sessions/JWT/API keys, data-API key enforcement, refresh rotation/revocation, CSRF origin check and auth burst limiting done; app tokens and distributed rate limiting pending |
| Auth: Google/Microsoft OAuth | `core-modules/auth/strategies/google*`, `microsoft*` | — | not-started | | |
| Auth: 2FA, captcha | `two-factor-authentication`, `captcha` | — | not-started | | |
| Auth: SAML/OIDC/SSO | `sso`, `auth/strategies/saml*`, `oidc*` | — | excluded (licence) | n/a | `@license Enterprise` |
| User | `core-modules/user` | `squid-db`/`squid-api` | partial | boot integration test | email/password user only |
| Workspace & members | `core-modules/workspace`, `workspace-invitation`, `workspace-member` | `squid-db`/`squid-api` | partial | tenant isolation, `workspace_members_query_and_boot_list` | bootstrap, members query, activation simplified; invitations pending |
| Roles & permissions (RBAC) | `metadata-modules/role*`, object/field permissions | `squid-auth` | partial | built-in role tests | built-in roles only; object/field/row permissions pending |
| Row-level permission predicates | `metadata-modules/row-level-permission-predicate*` | — | not-started | | |
| Object metadata (custom objects) | `metadata-modules/object-metadata` | `squid-db`/`squid-api` | partial | metadata integration tests | read/write rows; no physical tables or dynamic schema |
| Field metadata (custom fields) | `metadata-modules/field-metadata` | `squid-db`/`squid-api` | partial | metadata integration tests | read/write rows; relation fields rejected |
| Views / filters / sorts / groups | `metadata-modules/view*` | `squid-db`/`squid-api` | partial | view write/batch integration tests | standard views; relation filters and view paging semantics pending |
| Dynamic GraphQL schema | `api/graphql/*` | `squid-api` | partial | frontend operation map | static schema plus metadata queries; metadata-driven object schema pending |
| REST + OpenAPI | `api/rest/*`, `core-modules/open-api` | `squid-api` | partial | REST route map, example fixtures | 42/535 core routes; OpenAPI not emitted |
| MCP endpoint | `api/mcp` | — | deferred | | |
| Workspace ORM / query builder | `engine/twenty-orm/*` | `squid-db` | partial | repository integration tests | hand-written SQL with validated schema names; no generic query builder |
| People | `src/modules/person` | `squid-db`/`squid-api` | partial | person integration tests + smoke | core fields, batches, merge, null clearing; relation filters/order pending |
| Companies | `src/modules/company` | `squid-db`/`squid-api` | partial | object integration tests | CRUD, filters, ordering; relation filters pending |
| Opportunities | `src/modules/opportunity` | `squid-db`/`squid-api` | partial | object integration tests | CRUD, filters, ordering; relation filters pending |
| Tasks | `src/modules/task` | `squid-db`/`squid-api` | partial | object integration tests | CRUD, targets, filters, ordering |
| Notes | `src/modules/note` | `squid-db`/`squid-api` | partial | object integration tests | CRUD, targets, filters, ordering |
| Task/Note targets | `src/modules/task-target`, `note-target` | `squid-db`/`squid-api` | partial | target integration tests | CRUD/nested resolution; REST batch routes pending |
| Timeline activities | `src/modules/timeline`, `timeline-activity-type` | — | not-started | | |
| Search | `core-modules/search` | — | not-started | | `searchVector` stored, no search query |
| Record export / import | `core-modules/record-export`, `record-import` | — | not-started | | CRUD itself is covered per object |
| Files & upload | `core-modules/file`, `file-storage`, `src/modules/attachment` | `squid-db`/`squid-api` | partial | file/avatar integration test | local PostgreSQL storage, upload handshake and `avatarFile` writes; S3/signed URLs pending |
| Email (Gmail/Graph/IMAP/SMTP) | `core-modules/email`, `messaging` | — | not-started | | |
| Calendar | `core-modules/calendar`, `src/modules/calendar` | — | not-started | | |
| Connected accounts | `metadata-modules/connected-account`, `connection-provider` | — | not-started | | |
| Webhooks | `metadata-modules/webhook`, `workspace-event-emitter` | — | not-started | | metadata REST route missing |
| Background jobs / cron | `src/queue-worker`, `core-modules/message-queue`, `cron` | — | not-started | | Redis/BullMQ parity |
| Workflows | `metadata-modules/workflow*`, `src/modules/workflow` | — | not-started | 36 static data documents MISSING | large area |
| Logic functions | `core-modules/logic-function` | — | not-started | | |
| Route triggers | `core-modules/server-route-trigger` | — | deferred | | |
| Apps / extension platform | `core-modules/application`, `front-component`, `tool*` | — | deferred | | SDK is MIT |
| File storage S3/local | `core-modules/file-storage`, `storage` | `squid-db` | partial | file round-trip test | local PostgreSQL bytes; no S3 backend |
| Secure HTTP client (SSRF) | `core-modules/secure-http-client` | — | not-started | | |
| Secret encryption / rotation | `core-modules/secret-encryption` | `squid-core` | not-started | | secrets from env only |
| Throttler / rate limits | `core-modules/throttler` | `squid-api` | partial | auth limiter unit test | in-process auth burst limiter only |
| Audit / event logs | `core-modules/event-logs` | — | not-started | | |
| Health | `core-modules/health` | `squid-api` | parity | `/healthz` smoke | |
| Billing | `core-modules/billing*` | — | excluded (licence) | n/a | Enterprise |
| Admin panel | `core-modules/admin-panel` | — | excluded (licence) | | largely Enterprise |
| AI providers / agents | `metadata-modules/ai`, `flat-agent`, `skill` | — | deferred | | |
| Dashboards | `src/modules/dashboard` | — | not-started | | |
