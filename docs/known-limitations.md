# Known Limitations — Squid-CRM

Tracks known gaps, intentional exclusions, and caveats for the current release.
The working checklist is `docs/internal/TODO.md`.

## 1. Not implemented (open work)

| Area | Detail | Tracking |
|------|--------|----------|
| Generated GraphQL types for custom objects | Generic JSON GraphQL CRUD (`customObjectRecords`) exists, but no metadata-generated typed GraphQL schema/routes. |
| OpenAPI depth | `/openapi.json` emits an OpenAPI 3.1 skeleton for the supported routes; bodies are generic objects, not full field-level schemas. |
| Workflow engine | No workflow definitions, triggers, actions, or runs. | Not started |
| Background jobs / cron (Redis/BullMQ parity) | Durable `core.jobs` queue exists with `http.request` primitive; no Redis-backed distributed queue or cron scheduler. | Not started |
| OAuth / Google / Microsoft / SAML / OIDC | SSO/SAML/OIDC are `@license Enterprise` and excluded. OAuth strategies not implemented. | Excluded (licence) / not started |
| 2FA / captcha | Not implemented. | Not started |
| App tokens | Not implemented. | Partial |
| Row-level permission predicates | Not implemented. | Not started |
| MCP endpoint | Deferred. | Deferred |
| Route triggers | Deferred. | Deferred |
| Apps / extension platform | Deferred (SDK is MIT). | Deferred |
| AI providers / agents | Deferred. | Deferred |
| Timeline activities | Not implemented. | Not started |
| Email integrations (Gmail/Graph/IMAP/SMTP) | Not implemented. | Not started |
| Calendar integrations | Not implemented. | Not started |
| Connected accounts | Not implemented. | Not started |
| Dashboards | Not implemented. | Not started |
| Audit / event logs | Not implemented. Webhook delivery log exists; no general event-logs module. | Not started |
| Billing | `@license Enterprise` — excluded. | Excluded (licence) |
| Admin panel | Largely `@license Enterprise` — excluded. | Excluded (licence) |
| Workspace invitations | Not implemented. | Partial |
| Workspace activation lifecycle | `PENDING_CREATION` / `activateWorkspace` / `signUpInNewWorkspace` not implemented; signup activates directly. | Known limitation |
| File storage S3 (live provider) | Local PostgreSQL storage with signed URLs; S3-compatible path-style client implemented but not verified against a live provider. | Known limitation |
| Webhook delivery ordering + long-term retention | Durable `core.webhook_deliveries` log + bounded retries + background retry worker exist; ordering guarantees and retention policy open. | Known limitation |
| Distributed rate limiting | Auth throttling is in-process only (10 attempts / 15 min per account). Multi-replica deployments need edge/Redis limiting. | Known limitation |
| REST and GraphQL coverage | Route and operation coverage is partial; see `docs/api.md` for the supported surface. | Partial |
| Relation filters / ordering (remaining objects) | Implemented for person/company/opportunity; pending for some nested relations and targets. | Known limitation |
| Cursor pagination for all objects | Implemented for person; partial for other slices. | Known limitation |

## 2. Intentional exclusions (licence)

| Excluded area | Reason |
|---------------|--------|
| SSO / SAML / OIDC provisioning | `@license Enterprise` upstream files; cannot port without commercial licence (D-006) |
| Billing | `@license Enterprise` upstream files (D-006) |
| Admin panel | Largely `@license Enterprise` upstream files (D-006) |

## 3. Deliberate design decisions

| Divergence | Detail | Decision |
|------------|--------|----------|
| Signup activates workspace directly | No separate `PENDING_CREATION` / `activateWorkspace` step | D-013 |
| Cursor pagination replaces limit/offset | `limit`/`offset` was temporary; cursor pagination is now implemented | D-009 superseded |
| Data API accepts API keys or session cookie | User access tokens rejected on `/graphql` and `/rest` | D-016 |
| Workspace bootstrap is atomic | Signup creates user + workspace in one step (not separate activation) | D-013 |
| `ApiKey.role` returns metadata `Role` object | Legacy enum kept for authorization until permission flags land | D-033 |

## 4. Data model caveats

| Caveat | Detail |
|--------|--------|
| Morph metadata is metadata-only | `taskTarget`/`noteTarget` expose morph heads (`targetCompany`/`targetOpportunity`/`targetPerson`) but records still use separate `*Id` columns. |
| Index metadata is stored but not physical | `createOneIndex`/`deleteOneIndex` store rows in `core.index_metadata`/`core.index_field_metadata` but do not create PostgreSQL indexes. |
| Files stored in PostgreSQL | `core.files` holds file bytes; S3 is optional fallback, not default. |
| `groupBy` evaluated in memory | Per object slice, up to the repository fetch cap for non-person objects. |
| Relation-field DDL for custom objects | MANY_TO_ONE creates a UUID column; morph fields are rejected. |

## 5. Security caveats

| Caveat | Detail | Mitigation status |
|--------|--------|-------------------|
| Auth throttling is per-process | Multi-replica deployments need shared limiter at edge or in Redis | Open (internal/TODO.md Phase 6) |
| No CSRF token | Relies on SameSite=Lax + Origin check; review for older browsers/proxies | Open |
| `http.request` SSRF allowlisting | Blocks localhost/private IPs by default; public domains allowed unless allowlisted | Implemented (D-061) |
| No general audit log | Webhook delivery log exists; no `event-logs` module | Open |
| Webhook delivery ordering | Durable log + retries exist; ordering guarantees open | Open |

## 6. Ecosystem integration caveats

| Caveat | Detail |
|--------|--------|
| No live counterpart contracts | PyCustomerHub, PyReverseETL, PyTagShield, ClusterAudienceKit schemas not confirmed live |
| Webhook envelope v1 | Stable, but no shared JSON Schema or compatibility tests with ecosystem consumers |
| Idempotency key adoption | Counterparts must be taught to send stable idempotency keys |
| Python client 0.3.0 | Published to PyPI |

## 7. Release gates (not yet satisfied)

- [ ] Threat model reviewed (`docs/threat-model.md`)
- [x] `cargo audit` + `cargo deny` in CI (`.github/workflows/ci.yml`)
- [ ] Security test results recorded in `docs/decision-log.md`
- [ ] Legal review of AGPL derivative-work status (outside IP counsel)
- [ ] Branch protection and Dependabot alerts enabled on GitHub
- [ ] Baseline benchmarks captured (no fabricated performance claims)
