# Migration Matrix

Status legend: `not-started` · `in-progress` · `parity` · `excluded (licence)` · `deferred`

Reference pin: `twentyhq/twenty@08a0e5f1ee2e7db7c1e6ef3aeec9bcef731f4e98` (server `2.50.0`).

| Upstream area | Upstream path | Rust crate (planned) | Status | Parity evidence (planned) | Notes |
| --- | --- | --- | --- | --- | --- |
| Core config / boot | `src/engine/core-modules/twenty-config` | `squid-core` | not-started | config tests | |
| Logger / metrics / telemetry | `core-modules/logger`, `metrics`, `telemetry`, `sentry` | `squid-core` | not-started | smoke | |
| Auth: sessions, JWT, api-key | `core-modules/auth`, `api-key`, `app-token`, `jwt`, `session-storage` | `squid-auth` | not-started | auth integration tests | cookie + bearer |
| Auth: Google/Microsoft OAuth | `core-modules/auth/strategies/google*`, `microsoft*` | `squid-auth` | not-started | oauth callback tests | |
| Auth: 2FA, captcha | `two-factor-authentication`, `captcha` | `squid-auth` | not-started | unit + integration | |
| Auth: SAML/OIDC/SSO | `sso`, `auth/strategies/saml*`, `oidc*` | `squid-auth` | excluded (licence) | n/a | `@license Enterprise` |
| User | `core-modules/user` | `squid-core` | not-started | | |
| Workspace & members | `core-modules/workspace`, `workspace-invitation`, `workspace-member` | `squid-tenant` | not-started | tenant isolation tests | schema-per-workspace |
| Roles & permissions (RBAC) | `metadata-modules/role*`, `object-permission`, `field-permission`, `permission-flag` | `squid-metadata` | not-started | authz property tests | |
| Row-level permission predicates | `metadata-modules/row-level-permission-predicate*` | `squid-metadata` | not-started | authz tests | |
| Object metadata (custom objects) | `metadata-modules/object-metadata` | `squid-metadata` | not-started | | |
| Field metadata (custom fields) | `metadata-modules/field-metadata`, `search-field-metadata` | `squid-metadata` | not-started | | |
| Views / filters / sorts / groups | `metadata-modules/view*` | `squid-metadata` | not-started | golden fixtures | exact semantics |
| Dynamic GraphQL schema | `api/graphql/*` | `squid-graphql` | not-started | frontend op coverage | |
| REST + OpenAPI | `api/rest/*`, `core-modules/open-api` | `squid-rest` | not-started | OpenAPI diff | |
| MCP endpoint | `api/mcp` | `squid-rest` | deferred | | |
| Workspace ORM / query builder | `engine/twenty-orm/*` | `squid-orm` | not-started | query golden tests | filters/sorts/pagination |
| People | `src/modules/person` | `squid-rest`/`squid-graphql` | not-started | Phase 4 slice | first slice |
| Companies | `src/modules/company` | — | not-started | | |
| Opportunities | `src/modules/opportunity` | — | not-started | | |
| Tasks | `src/modules/task` | — | not-started | | |
| Notes | `src/modules/note` | — | not-started | | |
| Timeline activities | `src/modules/timeline`, `timeline-activity-type` | — | not-started | | |
| Search | `core-modules/search` | `squid-search` | not-started | | |
| Record CRUD / export / import | `core-modules/record-crud`, `record-export`, `record-position` | `squid-orm` | not-started | | |
| Files & upload | `core-modules/file`, `file-storage`, `src/modules/attachment` | `squid-files` | not-started | upload controls | size/type limits |
| Email (Gmail/Graph/IMAP/SMTP) | `core-modules/email`, `messaging`, `imap-smtp-caldav-connection` | `squid-integrations` | not-started | | |
| Calendar | `core-modules/calendar`, `src/modules/calendar` | `squid-integrations` | not-started | | |
| Connected accounts | `metadata-modules/connected-account`, `connection-provider` | `squid-integrations` | not-started | | |
| Webhooks | `metadata-modules/webhook`, `workspace-event-emitter` | `squid-jobs` | not-started | delivery/retry tests | |
| Background jobs / cron | `src/queue-worker`, `core-modules/message-queue`, `cron` | `squid-jobs` | not-started | job tests | Redis/BullMQ parity |
| Workflows | `metadata-modules/workflow*`, `src/modules/workflow` | `squid-workflows` | not-started | | large area (789 files) |
| Logic functions | `core-modules/logic-function`, `metadata-modules/logic-function*` | `squid-workflows` | not-started | | |
| Route triggers | `core-modules/server-route-trigger` | `squid-workflows` | deferred | | |
| Apps / extension platform | `core-modules/application`, `front-component`, `sdk-client`, `tool*` | `squid-apps` | deferred | | SDK is MIT |
| File storage S3/local | `core-modules/file-storage`, `storage` | `squid-files` | not-started | | |
| Secure HTTP client (SSRF) | `core-modules/secure-http-client` | `squid-core` | not-started | SSRF tests | |
| Secret encryption / rotation | `core-modules/secret-encryption` | `squid-core` | not-started | | |
| Throttler / rate limits | `core-modules/throttler` | `squid-http` | not-started | | |
| Audit / event logs | `core-modules/event-logs` | `squid-core` | not-started | verify scope | |
| Health | `core-modules/health` | `squid-http` | not-started | `/healthz` | |
| Billing | `core-modules/billing*` | — | excluded (licence) | n/a | Enterprise |
| Admin panel | `core-modules/admin-panel` | — | excluded (licence) | | largely Enterprise |
| AI providers / agents | `metadata-modules/ai`, `flat-agent`, `skill` | `squid-apps` | deferred | | |
| Dashboards | `src/modules/dashboard` | — | not-started | | |
