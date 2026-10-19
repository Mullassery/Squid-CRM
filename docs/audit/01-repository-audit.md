# Squid-CRM — Repository Audit (Phase 1)

Read-only audit of the reference repository. No source files were modified.

## 1. Reference identification

| Field | Value |
| --- | --- |
| Upstream | https://github.com/twentyhq/twenty |
| Commit audited | `08a0e5f1ee2e7db7c1e6ef3aeec9bcef731f4e98` |
| Commit date | 2026-10-19 20:58:47 +0200 |
| Commit subject | `Self-host Extra Smart: replace dominated grok-4.6@xhigh with grok-4.7 (#28053)` |
| Server-internal version | `TWENTY_CURRENT_VERSION = '2.50.0'` (`packages/twenty-server/src/engine/core-modules/upgrade/constants/twenty-current-version.constant.ts:10`) |
| Root `package.json` version | `0.2.1` (monorepo tooling only; product version is the constant above) |
| Reference clone | `reference/twenty` (shallow, `--depth 1`), kept pristine/read-only |

> IMPORTANT: this audit describes **this exact commit**. Twenty ships frequently; every claim here must be re-verified against a pinned commit before it is relied upon. Version drift is a first-class risk.

## 2. License & compliance findings (BLOCKING — read first)

License text: `reference/twenty/LICENSE` (763 lines). The project is **not** uniformly licensed:

1. **AGPL-3.0** — the default for the project (server, front, docs, e2e, emails, etc.).
2. **Twenty Application Exception** (AGPL §7 additional permission). Grants that an *Application* that only interacts through the **published interfaces** (REST/GraphQL APIs, webhooks, manifest/config formats, logic functions, front components, SDKs) and *does not incorporate or modify Twenty source* is **not** governed by AGPL. Section 4 ("Limits") is explicit: **if you modify Twenty, AGPL §13 applies to your modified version in full**, and network users must be offered the Corresponding Source.
3. **Enterprise (commercial) license** — files marked `/* @license Enterprise */`. In this commit **570 `.ts`/`.tsx` files** carry that marker (SSO/SAML, some billing, some admin surfaces, etc.). These may only be used in production with a valid Twenty Enterprise subscription.
4. **MIT packages** — as declared in each `package.json`: `twenty-cli`, `twenty-sdk`, `twenty-client-sdk`, `create-twenty-app`, `twenty-shared`, `twenty-ui`, and apps under `packages/twenty-apps`.

### 2.1 Consequence for a Rust backend

A from-scratch Rust reimplementation that reproduces Twenty's server behaviour, schema, and API contracts is almost certainly a **"modified version" / derivative work** of the AGPL-covered server. Under AGPL §5 and §13:

- The whole combined work must be licensed **AGPL-3.0** (plus the Application Exception where it applies).
- If operated as a network service, operators must **offer Corresponding Source** to users.
- Copyright, modification notices, and the LICENSE/NOTICE must be preserved.
- **You may not port `/* @license Enterprise */` files at all** without an Enterprise subscription. That excludes SSO/SAML/OIDC-provisioning, parts of billing, and other enterprise-gated features from scope for an AGPL-only build.

Two viable strategies must be chosen explicitly (see Decision Log `D-001`):

- **Strategy A (recommended for compliance): clean-room AGPL backend.** Reimplement against the *documented public API contracts and observable behaviour*, license Squid-CRM under AGPL-3.0, preserve attribution, exclude Enterprise-only features. Lowest legal risk; strongest obligation to publish source.
- **Strategy B: reference-only reimplementation with a hard firewall.** Keep the TS reference out of the build, do not copy code, and treat API contracts as specs. This *reduces* but does not eliminate AGPL exposure because behaviour/interface parity of a whole server is still a derived work.

> This is an engineering audit, not legal advice. A qualified IP lawyer should review the chosen strategy before public release.

## 3. Architecture of the audited version

Monorepo: **Nx + Yarn 4**, **Node 24.16.0** (`.nvmrc`), 21 packages under `packages/`.

| Layer | Technology | Evidence |
| --- | --- | --- |
| Backend | NestJS 11, TypeORM, PostgreSQL, Redis, GraphQL Yoga, Express, BullMQ | `packages/twenty-server/package.json` (154 deps) |
| Frontend | React 18, Jotai, Linaria (zero-runtime CSS), Vite, Apollo Client | `packages/twenty-front`, `src/modules/apollo` |
| Shared | Isomorphic types/utils (MIT) | `packages/twenty-shared` |
| UI kit | Component library (MIT) | `packages/twenty-ui` |
| SDK/CLI | App platform SDK + CLI (MIT) | `packages/twenty-sdk`, `packages/twenty-cli` |
| E2E | Playwright | `packages/twenty-e2e-testing` |
| Deploy | Docker image `twentycrm/twenty`, server + worker same image | `packages/twenty-docker/docker-compose.yml` |

### 3.1 Code volume (approx.)

| Package | `.ts`/`.tsx` LOC |
| --- | --- |
| twenty-server | ~50,300 |
| twenty-front | ~72,000 |
| twenty-shared | ~62,000 |
| twenty-ui | ~93,900 |
| twenty-sdk | ~50,000 |

Total ≈ **330k LOC** across the five main packages, plus 16 more packages. This is a large, mature codebase — a full parity rewrite is a multi-quarter effort, not a task.

### 3.2 Backend structure

`src/engine` contains:

- **`core-modules/`** — ~70 modules: `auth`, `user`, `workspace`, `workspace-invitation`, `api-key`, `application`, `billing`, `calendar`, `email`, `file`, `file-storage`, `search`, `record-crud`, `record-export`, `record-position`, `record-share`, `workflow`, `throttler`, `captcha`, `secure-http-client`, `secret-encryption`, `two-factor-authentication`, `sso`, `session-storage`, `jwt`, `redis-client`, `open-api`, `graphql`, `logger`, `metrics`, `sentry`, `telemetry`, `usage`, `usage-limit`, `feature-flag`, `i18n`, `health`, `logic-function`, `messaging`, `imap-smtp-caldav-connection`, `admin-panel`, `enterprise`, etc.
- **`metadata-modules/`** — the metadata engine: `object-metadata`, `field-metadata`, `view*`, `page-layout*`, `role`, `object-permission`, `field-permission`, `row-level-permission-predicate`, `route-trigger`, `logic-function`, `webhook`, `workflow`, `search-field-metadata`, `command-menu-item`, navigation/settings menus, `flat-*` projection entities, `minimal-metadata`, `permissions`. This is the heart of Twenty's dynamic model.
- **`api/`** — `graphql` (core, metadata, admin-panel; schema built dynamically per workspace), `rest` (core + metadata; OpenAPI generated dynamically), `mcp`, `common`, `types`.
- **`twenty-orm/`** — a custom ORM layer over TypeORM supporting per-workspace dynamic entities: `workspace-orm.manager`, `query-builder`, `entity-manager`, `field-operations`, `workspace-schema-manager`, `upgrade-aware`, `storage`.
- **`workspace-datasource/`** — `workspace-schema.service.ts` (create/drop **per-workspace Postgres schema**), `getWorkspaceSchemaName`.
- **`workspace-manager/`**, **`workspace-cache`**, **`core-entity-cache`**, **`subscriptions`** (GraphQL subscriptions), **`trash-cleanup`**.

`src/modules/` (workspace business modules): `attachment`, `blocklist`, `calendar`, `call-recording`, `company`, `connected-account`, `contact-creation-manager`, `dashboard`, `emailing`, `match-participant`, `messaging`, `messaging-webhooks`, `note`, `opportunity`, `person`, `task`, `timeline`, `workflow`, `workspace-member`, onboarding/import helpers.

### 3.3 Multi-tenancy & data model

- **Per-workspace Postgres schema**. `createWorkspaceDBSchema(workspaceId)` → `getWorkspaceSchemaName`; DDL gated by `WORKSPACE_SCHEMA_DDL_LOCKED` for hot upgrades (`workspace-schema.service.ts`).
- **Core schema** holds cross-workspace entities: users, workspaces, workspace-members, metadata, roles/permissions, billing, applications, files, connected accounts, etc.
- **80 `.entity.ts` files** total; core entities include all `flat-*` metadata entities, `role`, `object-permission`, `field-permission`, `row-level-permission-predicate(-group)`, `api-key`, `app-token`, `signing-key`, `two-factor-authentication-method`, `feature-flag`, `client-config`, `file`, `connected-account`, `calendar-channel`, `message-channel`, `logic-function`, `page-layout*`, `view*`, and more.
- **Views/filters/sorts/groups** are first-class metadata entities (`view`, `view-field`, `view-filter`, `view-filter-group`, `view-sort`, `view-group`) → filtering/sorting/view semantics are server-driven contracts to preserve.
- **Migrations**: 206 files under `src/database/typeorm/core/migrations` + `legacy-typeorm-migrations-do-not-add/`. Workspace schemas are **dynamic** (no fixed migration folder; generated via instance commands). Upgrade tooling lives in `src/database/commands/upgrade-version-command/` keyed by `TWENTY_CURRENT_VERSION`.

### 3.4 API surface (compatibility boundary)

Routing prefixes (from `packages/twenty-shared/src/types/ApiPath.ts` — with a note that nginx ingress routes these):

| Path | Purpose |
| --- | --- |
| `/graphql` | Main data GraphQL (schema built dynamically per workspace from metadata) |
| `/metadata` | Metadata GraphQL (object/field/view/role/permission management) |
| `/admin-panel` | Admin GraphQL |
| `/rest` | REST CRUD; OpenAPI generated dynamically |
| `/open-api` | OpenAPI document endpoint |
| `/auth` | Auth (sign-in/up, OAuth callbacks, SAML/OIDC, 2FA) |
| `/oauth`, `/.well-known` | OAuth/OIDC discovery |
| `/files`, `/file-upload`, `/file` | File upload/download |
| `/webhooks` | Inbound webhooks |
| `/s` | Route triggers |
| `/mcp` | Model Context Protocol endpoint |
| `/apps` | App platform |
| `/client-config` | Runtime config/feature flags for the frontend |
| `/emailing`, `/cloudflare`, `/public-assets`, `/healthz` | Misc/infra |

**Frontend coupling (critical):** `packages/twenty-front/src/modules/apollo/hooks/useApolloFactory.ts` connects to `${REACT_APP_SERVER_BASE_URL}/graphql`; `RestApiBaseUrl.ts` → `${REACT_APP_SERVER_BASE_URL}/rest`; `ApolloFactory.ts` uses `credentials: 'include'` (cookie session) plus a bearer/context auth link. The frontend's GraphQL document set is generated into `src/generated/graphql.ts` (~1.3k lines of operation definitions).

### 3.5 Auth & security controls observed

- Strategies: JWT (`jwt.auth.strategy`), Google OAuth, Microsoft OAuth, Google/Microsoft APIs (calendar/mail), SAML, OIDC.
- Cookie session via `express-session` + `connect-redis`; bearer tokens for API keys/app tokens.
- 2FA (`two-factor-authentication`, `two-factor-authentication-method` entity), captcha, throttler, `secure-http-client` (SSRF defense helper), `sql-sanitization`, `secret-encryption` (+ fallback key, rotation commands), `domain`/`dns-manager`, `approved-access-domain`, `guard-redirect`, CORS util `apply-credentialed-cors.util`, CSRF-relevant cookie auth.
- Enterprise-only: substantial SSO/admin pieces are `@license Enterprise` — out of scope for an AGPL-only build.

### 3.6 Background jobs, workflows, integrations

- **BullMQ** queues; `src/queue-worker/queue-worker.ts`; cron registration; `DISABLE_CRON_JOBS_REGISTRATION`.
- **Workflow engine**: 789 `.ts` files under workflow paths (triggers, actions, versions, runs).
- **Webhooks**: 208 `.ts` files; outbound webhook metadata entity + inbound `/webhooks`.
- **Integrations**: Google/Microsoft calendar & messaging (Gmail/Microsoft Graph), IMAP/SMTP/CalDAV, connected accounts, `messaging-webhooks`, `calendar`, `calendar-channel`, `message-channel`, `call-recording`, `company-enrichment`, `code-interpreter`, AI provider SDKs (OpenAI, Anthropic, Google, Bedrock, Mistral, xAI, Azure), Zapier package.
- **Storage**: local (`server-local-data`) or S3 (`STORAGE_TYPE`, S3 env vars).

### 3.7 Testing inventory (baseline to preserve/beat)

| Type | Count | Location |
| --- | --- | --- |
| Server unit specs (`*.spec.ts`) | 1,651 | `packages/twenty-server` |
| Server integration specs (`*.integration-spec.ts`) | 871 | `packages/twenty-server` |
| Frontend tests | 1,540 | `packages/twenty-front/src` |
| Playwright e2e | 11 | `packages/twenty-e2e-testing/tests` |

CI: 50+ workflows in `.github/workflows/` (`ci-server.yaml`, `ci-front.yaml`, `ci-e2e-main.yaml`, `ci-blocked-contributors.yaml`, breaking-change checks, cross-version upgrade tests, docker-compose tests, etc.).

## 4. Feature inventory — verification status

The brief lists a target feature set to **verify, not assume**. Status below is from static inspection of this commit.

| Feature | Present? | Evidence / notes |
| --- | --- | --- |
| Authentication | Yes | `core-modules/auth` (JWT, cookie session, OAuth, SAML, OIDC, 2FA) |
| Authorization / RBAC | Yes | `role`, `object-permission`, `field-permission`, `row-level-permission-predicate`, `permissions`, `role-validation` |
| Workspaces / multi-tenancy | Yes | Schema-per-workspace (`workspace-schema.service.ts`), `workspace`/`workspace-member` entities + guards |
| Contacts / People | Yes | `src/modules/person` |
| Companies | Yes | `src/modules/company` |
| Opportunities | Yes | `src/modules/opportunity` |
| Custom objects & fields | Yes | `metadata-modules/object-metadata`, `field-metadata` (+ flat projections) |
| Filtering / sorting / views | Yes | `view`, `view-field`, `view-filter(-group)`, `view-sort`, `view-group` |
| Relationships | Yes | Relation field metadata + `field-operations`, timeline |
| Workflows | Yes | 789 files; `workflow`, `workflow-version`, `route-trigger`, `logic-function` |
| Tasks | Yes | `src/modules/task` |
| Notes | Yes | `src/modules/note`, `@blocknote` editor |
| Email integrations | Yes | `email`, `messaging`, Gmail/Microsoft Graph, IMAP/SMTP (`imap-smtp-caldav-connection`) |
| Calendar integrations | Yes | `calendar`, `calendar-channel`, Google/Microsoft |
| Files | Yes | `file`, `file-storage`, `/files`, `/file-upload` |
| Search | Yes | `core-modules/search`, `search-field-metadata` |
| Audit logs | Partial/verify | `event-logs` module present; `timeline-activity-type`; `timeline` module. Confirm retention/export scope. |
| APIs | Yes | GraphQL (`/graphql`,`/metadata`,`/admin-panel`), REST (`/rest`), MCP (`/mcp`) |
| Webhooks | Yes | 208 files; outbound metadata + `/webhooks` inbound |
| Background jobs | Yes | BullMQ worker, cron registry |
| Extension capabilities | Yes | Applications platform, `logic-function`, front components, SDK/CLI (MIT), `twenty-apps` |
| Billing | Enterprise-gated | `billing*` entities/modules; treat as out of scope for AGPL build |
| SSO/SAML/OIDC | Enterprise-gated | `@license Enterprise` files present; out of scope without subscription |
| Row-level security | Yes | `row-level-permission-predicate(-group)` |
| Dashboards | Yes | `src/modules/dashboard`, `dashboard-sync` |
| Attachments / call recording | Yes | `src/modules/attachment`, `call-recording` |

## 5. Key risks

1. **License (blocking).** AGPL + Enterprise + MIT mix. Backend port ⇒ AGPL obligations; Enterprise files cannot be ported. Choose strategy before any code (D-001).
2. **Scope.** ~330k LOC. Full parity is not a single increment. Parity must be gated module-by-module with a migration matrix.
3. **Dynamic schema.** GraphQL/REST schemas are built at runtime from workspace metadata; a Rust backend must reproduce this, not a static schema. `twenty-orm` behaviour (filters, sorts, relations, permissions) is subtle and very heavily tested upstream (871 integration specs) — the real compatibility bar.
4. **Multi-tenancy correctness.** Schema-per-workspace + RLS predicates + permission flags; tenant escape is the top security risk.
5. **Version drift.** Upstream moves fast; must pin commit and re-audit before parity claims.
6. **No local Postgres** in this environment (`psql` absent); Postgres can run via Docker (Docker 29.5.2 present). Rust 1.99 / Node 26 present, but Node 24.16 is the pinned target for the untouched frontend.
7. **Frontend contract.** Apollo client expectations (error shapes, pagination, optimistic cache normalisation, `RemoteTable` key fields) must be met exactly or the React app breaks.
8. **Benchmarks.** No baseline exists yet; must be measured on the reference before any comparative claim.

## 6. Recommended implementation sequence (summary)

Full detail: `docs/audit/02-phase-plan.md`. Migration matrix: `docs/migration-matrix.md`.

1. **Phase 1 — Inventory & licence review** (this document). Checkpoint: strategy + scope signed off.
2. **Phase 2 — Feature/API/data-flow mapping.** Produce the migration matrix and API compatibility doc from the pinned commit.
3. **Phase 3 — Rust architecture & ADRs.** Crate layout, Axum/Tokio/SQLx, dynamic GraphQL strategy, tenant model, ADRs.
4. **Phase 4 — Minimal vertical slice.** One read path + one write path end-to-end (e.g. `Person` GET/list/create) against real Postgres, with auth + tenant isolation + contract tests.
5. **Phase 5 — Incremental module migration.** Objects, then views/filters/sorts, then metadata, then auth parity, then workflows/webhooks/jobs.
6. **Phase 6 — Parity & security hardening.** RBAC/RLS, rate limits, SSRF/upload controls, secret management, encryption, auditability.
7. **Phase 7 — Benchmarks & deployment.** Baseline vs Rust, Docker Compose, CI.
8. **Phase 8 — Documentation & release.** README, contributor guide, SBOM, threat model, release notes.

No implementation code is written until the Phase 1 checkpoint decisions are recorded in `docs/decision-log.md`.
