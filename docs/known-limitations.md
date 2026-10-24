# Known Limitations — Squid-CRM

Tracks known gaps, intentional exclusions, and caveats for the current release.
Authoritative per-area implementation status lives in `docs/migration-matrix.md`;
the working checklist is `internal/TODO.md`.

Reference pin: `twentyhq/twenty@08a0e5f1ee2e7db7c1e6ef3aeec9bcef731f4e98`
(server `2.50.0`).

## 1. Not implemented (open work)

| Area | Detail | Tracking |
|------|--------|----------|
| Generated GraphQL types for custom objects | Generic JSON GraphQL CRUD (`customObjectRecords`) exists, but no metadata-generated typed GraphQL schema/routes. | internal/TODO.md Phase 5 follow-up; migration matrix row "Dynamic GraphQL schema" |
| OpenAPI document emission | No OpenAPI document is emitted by the Rust server. | internal/TODO.md Phase 6-8; migration matrix row "REST + OpenAPI" |
| Workflow engine | No workflow definitions, triggers, actions, or runs. | Migration matrix row "Workflows" — not started |
| Background jobs / cron (Redis/BullMQ parity) | Durable `core.jobs` queue exists with `http.request` primitive; no Redis-backed distributed queue or cron scheduler. | Migration matrix row "Background jobs / cron" — not started |
| OAuth / Google / Microsoft / SAML / OIDC | SSO/SAML/OIDC are `@license Enterprise` and excluded. OAuth strategies not implemented. | Migration matrix rows — excluded (licence) / not started |
| 2FA / captcha | Not implemented. | Migration matrix rows — not started |
| App tokens | Not implemented. | Migration matrix row "Auth: sessions, JWT, API keys" — partial |
| Row-level permission predicates | Not implemented. | Migration matrix row — not started |
| MCP endpoint | Deferred. | Migration matrix row — deferred |
| Route triggers | Deferred. | Migration matrix row — deferred |
| Apps / extension platform | Deferred (SDK is MIT). | Migration matrix row — deferred |
| AI providers / agents | Deferred. | Migration matrix row — deferred |
| Timeline activities | Not implemented. | Migration matrix row — not started |
| Email integrations (Gmail/Graph/IMAP/SMTP) | Not implemented. | Migration matrix row — not started |
| Calendar integrations | Not implemented. | Migration matrix row — not started |
| Connected accounts | Not implemented. | Migration matrix row — not started |
| Dashboards | Not implemented. | Migration matrix row — not started |
| Audit / event logs | Not implemented. Webhook delivery log exists; no general event-logs module. | Migration matrix row — not started |
| Billing | `@license Enterprise` — excluded. | Migration matrix row — excluded (licence) |
| Admin panel | Largely `@license Enterprise` — excluded. | Migration matrix row — excluded (licence) |
| Workspace invitations | Not implemented. | Migration matrix row "Workspace & members" — partial |
| Workspace activation lifecycle | `PENDING_CREATION` / `activateWorkspace` / `signUpInNewWorkspace` not implemented; signup activates directly. | internal/TODO.md known limitations |
| File storage S3 (live provider) | Local PostgreSQL storage with signed URLs; S3-compatible path-style client implemented but not verified against a live provider. | internal/TODO.md known limitations; migration matrix row "File storage S3/local" — partial |
| Webhook delivery ordering + long-term retention | Durable `core.webhook_deliveries` log + bounded retries + background retry worker exist; ordering guarantees and retention policy open. | internal/TODO.md known limitations; migration matrix row "Webhooks" — partial |
| Distributed rate limiting | Auth throttling is in-process only (10 attempts / 15 min per account). Multi-replica deployments need edge/Redis limiting. | internal/TODO.md known limitations |
| REST route coverage | 42 of 535 captured core routes implemented. | docs/rest-route-map.md; migration matrix row — partial |
| Relation filters / ordering (remaining objects) | Implemented for person/company/opportunity; pending for some nested relations and targets. | internal/TODO.md known limitations |
| Cursor pagination for all objects | Implemented for person; partial for other slices. | internal/TODO.md known limitations |

## 2. Intentional exclusions (licence)

| Excluded area | Reason |
|---------------|--------|
| SSO / SAML / OIDC provisioning | `@license Enterprise` upstream files; cannot port without commercial licence (D-006) |
| Billing | `@license Enterprise` upstream files (D-006) |
| Admin panel | Largely `@license Enterprise` upstream files (D-006) |

## 3. Semantic divergence from upstream Twenty (deliberate)

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
| No general audit log | Webhook delivery log exists; no `event-logs` module | Open (migration matrix) |
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
