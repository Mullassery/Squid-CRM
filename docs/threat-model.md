# Threat Model — Squid-CRM

Status: Phase 6 deliverable. Authoritative per-area implementation status lives in
`docs/migration-matrix.md` and `TODO.md`; this document records threats, affected
assets, controls, and residual risk.

Reference pin: `twentyhq/twenty@08a0e5f1ee2e7db7c1e6ef3aeec9bcef731f4e98`
(server `2.50.0`).

## 1. Scope

In scope:

- `squid-server` (Axum + Tokio) exposing `/graphql`, `/rest`, `/metadata`,
  `/auth`, `/healthz`, `/files`, `/webhooks` (inbound).
- `squid-core` — config, domain models, error contract.
- `squid-db` — pool, migrations, per-workspace schemas, repositories.
- `squid-auth` — passwords, JWTs, refresh tokens, sessions, API keys,
  `AuthContext`.
- `squid-api` — Axum router, REST, GraphQL, CORS and auth middleware.
- PostgreSQL 16 (`core` schema + per-workspace schemas).
- Background workers: maintenance, webhook retry, job queue (`core.jobs`).
- Python client (`squid_crm`, `python/`) — idempotency, correlation, retry headers.

Out of scope (excluded by licence or deferred):

- Enterprise-gated features: SSO/SAML/OIDC provisioning, billing, admin panel
  (`/* @license Enterprise` files, D-006).
- OAuth, Google/Microsoft strategies, captcha, 2FA, app tokens.
- Workflows, logic functions, route triggers, apps/extension platform, AI
  providers.
- Email, calendar, connected accounts, dashboards, timeline activities.
- S3-compatible storage against a live provider (local PostgreSQL fallback only;
  `http.request` SSRF allowlisting is partial).

Trust boundary: the Rust API server is the trust boundary. Clients (Twenty
frontend, Python client, webhook consumers, job enqueuers) are untrusted
callers authenticated via session cookie, API key, or (for webhook delivery)
HMAC signature.

## 2. Assets

| Asset | Classification | Owner | Notes |
|-------|---------------|-------|-------|
| CRM records (person, company, opportunity, task, note, custom objects) | Confidential PII | Workspace tenant | Multi-tenant; schema-per-workspace isolation |
| Workspace metadata (objects, fields, views, roles, permissions) | Internal | Workspace admin | Drives dynamic schema and RBAC |
| Sessions / refresh tokens | Secret | User | Hashed at rest; opaque; rotated |
| API keys | Secret | Workspace member | Hashed at rest; expiry + revocation per request |
| Webhook secrets | Secret | Workspace admin | Stored; never sent in headers; HMAC verified |
| Files / attachments | Confidential PII | Workspace tenant | PostgreSQL bytes; signed expiring URLs |
| `APP_SECRET` | Secret | Operator | Env-only; min 32 chars; invalidates all JWTs on rotation |
| `core.jobs` payloads | Internal | System | May contain URLs, headers, JSON bodies for `http.request` |
| Audit / webhook delivery log | Internal | System | `core.webhook_deliveries`; retry state, terminal status |

## 3. Threat actors

| Actor | Motivation | Capability |
|-------|-----------|------------|
| External unauthenticated attacker | Data theft, service disruption | Network access to public endpoints |
| Authenticated workspace member (low privilege) | Privilege escalation, cross-tenant data access | Valid session / API key within one workspace |
| Compromised API key holder | Record exfiltration, record mutation | Stolen API key value |
| Malicious webhook consumer | Record injection, SSRF via callbacks | Can receive webhook payloads; may host callback URLs |
| Compromised operator / insider | Full data access, secret exfiltration | Access to host, env vars, database |
| Supply-chain attacker | Code execution, dependency compromise | Malicious crate, compromised registry |

## 4. Threat matrix

| ID | Threat | Asset | Impact | Likelihood | Controls | Residual |
|----|--------|-------|--------|------------|----------|----------|
| T-01 | Credential stuffing / brute force on signup/signin | Sessions, API keys | Account takeover | Medium | Argon2id, 10 attempts/15 min per account (in process), rate limit extension point | Medium (per-process only; multi-replica needs shared limiter) |
| T-02 | Session token theft / replay | Sessions | Account takeover | Medium | 32-byte opaque token, SHA-256 at rest, HttpOnly/SameSite=Lax cookie, signout revokes | Low |
| T-03 | Access token used on data API | CRM records | Unauthorized data access | Low | Data API rejects user access tokens; API key or session cookie only | Low |
| T-04 | Cross-tenant data read | CRM records | Data breach | Low | Workspace derived from principal, validated schema name, membership re-checked per request, integration-tested | Low |
| T-05 | Cross-tenant data write | CRM records | Data tampering | Low | Same tenant context choke point; role capability enforcement on REST/GraphQL | Low |
| T-06 | SQL injection via schema identifier | Database, CRM records | Data breach / tampering | Low | No user text interpolated; validated schema identifiers; integration test | Low |
| T-07 | CSRF on cookie-authenticated unsafe requests | Sessions, CRM records | Unauthorized state change | Low | Origin check on unsafe cookie requests; bearer/API-key exempt | Low (SameSite=Lax + Origin; older browser/proxy review pending) |
| T-08 | Stolen API key | CRM records | Data exfiltration / mutation | Medium | API key hashed at rest, expiry + revocation checked per request | Medium |
| T-09 | Webhook secret leakage in transit | Webhook secrets | Forged webhook delivery | Low | Plaintext `x-webhook-secret` removed; consumers verify HMAC-SHA256 signature | Low |
| T-10 | Webhook replay / duplication | CRM records | Duplicate record creation | Medium | `Idempotency-Key` replay-safe writes with body-hash conflict detection, response replay headers | Low |
| T-11 | SSRF via `http.request` job | Internal network, metadata | Internal service access, data exfiltration | Medium | `http_job_url_allowed` scheme/host/private-IP allowlist; `HTTP_JOB_ALLOW_PRIVATE` opt-out; `HTTP_JOB_ALLOWED_HOSTS` | Medium (live-provider verification pending; localhost/private blocked by default) |
| T-12 | File upload path traversal / oversized upload | Files, storage | File overwrite, DoS | Low | 2 MiB body limit, UUID-addressed files, HMAC-signed expiring URLs, `UPLOAD_FILE`/`DOWNLOAD_FILE` flag checks | Low |
| T-13 | Dependency vulnerability (supply chain) | All | Code execution, data breach | Medium | `cargo audit`, `cargo deny` planned for CI (not yet enabled) | Medium |
| T-14 | Secret exfiltration via logs / env | `APP_SECRET`, webhook secrets | Full compromise | Low | Secrets env-only, never logged; `APP_SECRET` min 32 chars | Low |
| T-15 | Privilege escalation via role mutation | Roles, permissions | Unauthorized admin actions | Low | Admin guard on object/field metadata writes and role/webhook settings mutations; built-in roles immutable | Low |
| T-16 | Missing audit trail | Audit log | Undetectable tampering | Medium | Webhook delivery log (`core.webhook_deliveries`) with retry/terminal state; no general audit log yet | Medium |
| T-17 | Token rotation replay | Refresh tokens | Session hijacking | Low | Opaque refresh tokens, rotated atomically, replay rejected via session `sid`, signout revokes | Low |
| T-18 | Idempotency key reuse with different body | CRM records | 409 Conflict (safe failure) | Low | Body-hash conflict detection; keys expire; workspace-scoped | Low |

## 5. Control gaps and mitigations (open work)

| Gap | Phase | Mitigation / owner |
|-----|-------|-------------------|
| Auth throttling is per-process only | 6 | Add distributed rate limiter (Redis) or edge-level limit before multi-replica production |
| No CSRF token (SameSite=Lax + Origin only) | 6 | Review for older browsers/proxies; consider double-submit cookie |
| No OAuth / 2FA / app tokens | Deferred | Licence-excluded or deferred; document as out of scope |
| No field/object permission enforcement beyond built-in roles | 6 | Permission-flag enforcement for billing/SSO data and feature flags remains open |
| No SSRF guard for outbound integrations beyond `http.request` | 6 | `secure-http-client` parity not implemented; `http.request` allowlisting is the current substrate |
| File storage: PostgreSQL bytes only; S3 live-provider unverified | 6/7 | S3-compatible path-style client implemented; live verification environment-specific |
| No general audit log | 6 | Webhook delivery log exists; general `event-logs` module not started (migration matrix) |
| No dependency scanning in CI | 6 | Add `cargo audit` + `cargo deny` to CI workflow |
| Webhook delivery ordering + long-term retention policy | 6 | Durable log + bounded retries + background worker exist; ordering and retention open |
| No branch protection / vulnerability alerts on GitHub | Repo settings | Enable branch protection and Dependabot alerts on `Mullassery/Squid-CRM` |
| REST route coverage: 493 of 535 core routes missing | 5/6 | Continue route parity per `docs/rest-route-map.md` |
| Custom objects: no generated GraphQL types | 5 | Generic JSON GraphQL CRUD done; typed schema generation deferred |

## 6. Security testing requirements (Phase 6)

Before any production claim, the following must be run and results recorded:

- `cargo audit` clean, or exceptions documented.
- `cargo deny` (licenses, vulnerabilities) clean, or exceptions documented.
- Property/fuzz tests for authz decisions (tenant escape, role escalation, forbidden metadata writes).
- Property/fuzz tests for parsers (REST filter grammar, `order_by`, `fields`/`depth`, GraphQL selection sets).
- Tenant-isolation test: workspace A cannot read workspace B's person (automated, failing test first).
- Auth test: unauthenticated → 401/403; wrong role → 403.
- SSRF test: `http.request` to localhost/private ranges is rejected.
- Idempotency test: reused key with different body → 409; reused key with same body → replayed response.
- Webhook HMAC verification test: tampered body → rejected; valid signature → accepted.

## 7. Release gate

- Threat model reviewed (this document).
- `cargo audit` + `cargo deny` added to CI.
- Security test results recorded in `docs/decision-log.md` checkpoint.
- Legal review of AGPL derivative-work status completed (D-001, outside IP counsel).
