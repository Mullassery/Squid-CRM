# Competitive intelligence

## OSS gap analysis — 2026-10-24

**What matters most here:** make custom objects first-class. Squid's Twenty-compatibility promise fails when a user-created object has physical tables and JSON CRUD but no generated GraphQL types, REST routes, or OpenAPI entry. Until metadata drives the API surface, Squid is a fast core-object backend rather than a Twenty replacement.

Scope: Squid-CRM (`squid-server` 0.1.0, AGPL-3.0, alpha). Reimplemented Twenty v2.50.0 data/metadata API in Rust with schema-per-workspace tenancy. Compared against self-hosted OSS CRM/database backends that buyers evaluate for the same job: metadata-driven records, REST/GraphQL APIs, permissions, and self-hosting. Evidence is the repo's `README.md`, `docs/migration-matrix.md`, `docs/api-compatibility.md`, `docs/known-limitations.md`, `Cargo.toml`, plus verified upstream pages.

### Comparable OSS projects
| Project | GitHub | License | Last activity | Why it is the comparison |
| --- | --- | --- | --- | --- |
| Twenty | https://github.com/twentyhq/twenty | AGPL-3.0 plus `@license Enterprise` files (LICENSE) | release `twenty/v2.50.0`, 2026-10-21 | Squid explicitly targets Twenty's v2.50.0 API contract; same CRM domain and metadata model |
| Directus | https://github.com/directus/directus | Monospace Sustainable Core License 1.0 (MSCL-1.0-GPL) | release `v12.5.1`, 2026-10-23 | Database-mirroring backend that auto-generates REST and GraphQL from schema, the closest "bring your own SQL schema, get APIs" alternative |
| Baserow | https://github.com/baserow/baserow | MIT Expat for the Open Source Edition; `premium/` and `enterprise/` separately licensed | release `2.4.1`, 2026-10-13 | Self-hosted Airtable-style metadata database with REST API, views, permissions and automations |
| NocoDB | https://github.com/nocodb/nocodb | Sustainable Use License (master/develop) | release `2026.10.0`, 2026-10-23 | Self-hosted Airtable alternative; competes on metadata tables, views, REST API and automation |

### Capability comparison
| Capability | Ours | Twenty | Directus | Baserow | Gap |
| --- | --- | --- | --- | --- | --- |
| GraphQL and REST over the same records | Partial | Yes | Yes | Partial | Ours has GraphQL + REST for core objects but only 42/535 captured REST routes and a partial resolver set |
| Multi-tenant isolation | Yes | Yes | Partial | Partial | Ours enforces one PostgreSQL schema per workspace with a cross-tenant read/delete test; strongest isolation story |
| Metadata-driven dynamic schema | Partial | Yes | Yes | Yes | Ours stores object/field metadata but does not generate typed GraphQL or REST for custom objects |
| OpenAPI document emission | No | Yes | Yes | Yes | Ours emits no OpenAPI; Baserow publishes `/api/schema.json` |
| Row-level permission rules | No | Partial | Yes | Partial | Ours has role capability flags only; no row predicates |
| Workflow/automation engine | No | Yes | Yes | Yes | Absent in Ours; competitors ship triggers/actions or Flows |
| Durable background jobs and cron | Partial | Yes | Yes | Yes | Ours has a `core.jobs` table and `http.request` primitive, no cron or distributed queue |
| CSV export/import | Yes | Yes | Yes | Yes | Ours is implemented for core objects; subscription export/other formats pending |
| Single static binary deploy | Yes | No | No | No | Rust/Axum binary with no Node runtime; unique among these |
| Fixture-locked Twenty contract parity | Partial | Yes | No | No | Ours tests against captured v2.50.0 fixtures; coverage is partial and documented |

### Gaps that matter (ranked)
1. **No metadata-generated schema for custom objects (absent).** `docs/known-limitations.md` and the migration matrix say custom objects have physical tables and generic JSON CRUD but no generated GraphQL types or REST routes. This breaks the core Twenty promise that a user-created object behaves like a first-class object. Hurts frontend parity and any API consumer that expects typed queries. Effort: M for GraphQL, S for REST fallback hardening.
2. **No OpenAPI emission (absent).** `docs/api-compatibility.md` shows `/open-api/core` and `/open-api/metadata` not implemented; every comparable project publishes a machine-readable spec. Hurts SDK generation, partner integrations and API review. Effort: S.
3. **Automation/jobs are missing or minimal (absent/weaker).** No workflow engine, no cron, no Redis/BullMQ parity; only a durable `core.jobs` queue with an `http.request` primitive. Twenty, Directus, Baserow and NocoDB all sell automation as a headline feature. Hurts teams replacing Twenty rather than just its data API. Effort: L.
4. **REST and relation-filter parity (weaker).** 42/535 captured core routes; relation filters/ordering remain pending for several objects; cursor pagination is complete only for person. `docs/known-limitations.md` calls this out. Hurts migration of existing Twenty clients and bulk integrations. Effort: M, mostly repetitive route work.
5. **Identity breadth (absent, partly licence-blocked).** No OAuth Google/Microsoft, no 2FA, no app tokens, no SSO/SAML/OIDC (the last excluded as Enterprise). Competitors offer OAuth at minimum; Directus and Baserow ship richer auth. Hurts self-hosted teams with corporate IdPs. Effort: M for OAuth/2FA; SSO is blocked without a commercial licence.
6. **Operational hardening (weaker).** Auth throttling is per-process, indexes are metadata-only, no general audit log, and webhook ordering/retention are open. Hurts multi-replica and compliance-sensitive deployments. Effort: M.

### Where we are differentiated
- **Deployment weight:** one static Rust binary with PostgreSQL only; Twenty, Directus, Baserow and NocoDB all require a Node/Python/Java runtime stack.
- **Isolation:** physical schema per workspace, validated identifiers, membership re-check on every request, and a tested cross-tenant denial path.
- **Contract discipline:** Golden GraphQL SDL, OpenAPI captures and per-route/per-resolver status are checked in, so "supported" is falsifiable rather than marketing.
- **Explicit exclusions:** Enterprise-licensed SSO/billing/admin code is excluded instead of approximated, which is honest for AGPL reuse.
- **Security defaults:** Argon2id, rotating hashed refresh tokens, hashed API keys, Origin checks and HMAC-signed file URLs are implemented, not roadmap items.

### Disposition
- **Build:** metadata-generated GraphQL types for custom objects; OpenAPI emission from the same route map; relation filters/order and remaining REST routes; row-level permission predicates; cron/distributed job runner.
- **Integrate/reuse:** S3-compatible storage for files, Redis for shared rate limiting and jobs, an existing OIDC/OAuth library for Google/Microsoft; reuse Twenty's captured fixtures as the compatibility oracle.
- **Defer/reject:** workflow builder UI and app/extension platform (defer until API parity); SSO/SAML and billing (reject while upstream code is Enterprise-licensed); email/calendar integrations (defer, not core to the backend wedge).

### Sources
- https://github.com/twentyhq/twenty (accessed 2026-10-24) — LICENSE, release `twenty/v2.50.0` (2026-10-21)
- https://github.com/directus/directus (accessed 2026-10-24) — `license` MSCL-1.0-GPL, release `v12.5.1` (2026-10-23)
- https://docs.directus.io/reference/introduction.html (accessed 2026-10-24) — REST + GraphQL generated from database schema
- https://github.com/baserow/baserow (accessed 2026-10-24) — LICENSE (MIT OSE), release `2.4.1` (2026-10-13), `/api/schema.json`
- https://github.com/nocodb/nocodb (accessed 2026-10-24) — LICENSE.md (Sustainable Use License), release `2026.10.0` (2026-10-23)
- Repo files inspected: `README.md`, `Cargo.toml`, `docs/migration-matrix.md`, `docs/api-compatibility.md`, `docs/known-limitations.md`, `docs/architecture/overview.md`, `TODO.md`
