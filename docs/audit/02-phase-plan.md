# Squid-CRM — Staged Migration Plan

Eight phases. Each phase lists **Scope**, **Acceptance criteria**, **Risks**, **Dependencies**, and **Rollback**. A phase advance requires a recorded checkpoint in `docs/decision-log.md` and the relevant checks actually run (no unverified claims).

Reference pin: `twentyhq/twenty@08a0e5f1ee2e7db7c1e6ef3aeec9bcef731f4e98` (`TWENTY_CURRENT_VERSION = 2.50.0`).

---

## Phase 1 — Inventory & licence review ✅ (delivered)

**Scope.** Read-only audit of architecture, licence, features, API surface, data model, frontend coupling, tests, CI, deployment.
**Deliverables.** `01-repository-audit.md`, `02-phase-plan.md`, `decision-log.md`, `../internal/TODO.md`.
**Acceptance.** Every inventory claim cites a file path or command output; licence obligations enumerated; open decisions listed.
**Risks.** Mis-stating a feature as present. Mitigation: evidence column in inventory.
**Dependencies.** Read access to upstream.
**Rollback.** Documentation only; delete `docs/`.

---

## Phase 2 — Feature / API / data-flow mapping

**Scope.** Turn the inventory into machine-usable contracts. Enumerate every GraphQL operation the frontend issues (`twenty-front/src/generated/graphql.ts`), every REST route + OpenAPI component, every metadata mutation, every webhook/job contract, and every data-flow across tenant/role boundaries.
**Deliverables.**
- `docs/migration-matrix.md` (per-module: upstream path → Rust crate → status → parity tests).
- `docs/api-compatibility.md` (transport, auth, error contracts, pagination/sort/filter semantics, idempotency, retry).
- `contracts/` — captured OpenAPI spec + GraphQL SDL for a seeded workspace, used as golden fixtures.
- A repeatable **contract-capture script** that boots the reference stack (Docker Compose) and dumps SDL/OpenAPI/example responses at the pinned commit.
**Acceptance.** Contract fixtures generated from the pinned commit; every frontend GraphQL operation mapped to a backend resolver contract; error/pagination semantics documented with examples.
**Risks.** Capturing a moving target. Mitigation: pin commit, store fixtures in-repo with the commit hash.
**Dependencies.** Phase 1 checkpoint; Docker for reference stack.
**Rollback.** Delete generated fixtures; no runtime impact.

---

## Phase 3 — Rust architecture & ADRs

**Scope.** Define the Rust workspace, service boundaries, request lifecycle, tenant context propagation, dynamic schema strategy, error model, config, observability.
**Stack (evidence-based default).** stable Rust, Tokio, Axum, SQLx (Postgres), `async-graphql` (or Juniper) for dynamic GraphQL, `utoipa` for OpenAPI, `sqlx::migrate` for migrations, `tracing` + OpenTelemetry, `serde`. Alternatives require an ADR with evidence.
**Proposed crates.**
```
squid-crm/
  crates/
    squid-http        # Axum app, middleware, routers (/graphql /rest /metadata /auth ...)
    squid-auth        # sessions, JWT, API keys, OAuth/SAML adapters (AGPL subset)
    squid-tenant      # tenant context, schema routing, RLS enforcement
    squid-metadata    # object/field/view/role metadata engine
    squid-graphql     # dynamic schema builder + resolvers
    squid-rest        # REST handlers + OpenAPI generation
    squid-orm         # SQLx-based workspace repository/query-builder
    squid-jobs        # queue + cron (Redis)
    squid-core        # shared types/errors/config
```
**Deliverables.** `docs/adr/0001..00NN-*.md`; `docs/architecture/*.md` + Mermaid diagrams; crate skeletons that compile with `cargo check`.
**Acceptance.** `cargo build` green on skeletons; ADRs justify each major choice; tenant-isolation model documented with invariants.
**Risks.** Choosing a GraphQL strategy that cannot express metadata-driven schemas. Mitigation: spike dynamic schema generation in Phase 4 before committing.
**Dependencies.** Phase 2 contracts.
**Rollback.** Crates are additive; revert commits.

---

## Phase 4 — Minimal vertical slice

**Scope.** One object (`Person`) end-to-end: `GET /rest/people`, `GET /rest/people/:id`, `POST /rest/people`, plus the corresponding GraphQL `people`/`createPerson` operations, behind the real auth boundary, with tenant isolation, against real PostgreSQL.
**Deliverables.** Working slice; migrations for a minimal core schema + one workspace schema; integration tests against real Postgres (Docker); contract tests diffing responses against Phase 2 fixtures; frontend smoke test with the real Apollo client pointed at the Rust backend.
**Acceptance (all must actually pass, output pasted):**
- `cargo test` (unit + integration via `sqlx::test`/testcontainers).
- Contract fixtures match (status codes, JSON shape, error envelope).
- Tenant-isolation test: workspace A cannot read workspace B's person (automated, failing test first).
- Auth test: unauthenticated → 401; wrong role → 403.
- Frontend loads and lists people through Rust.
**Risks.** Hidden dependencies in metadata bootstrap. Mitigation: seed via the reference stack or captured fixtures.
**Dependencies.** Phase 3 checkpoints.
**Rollback.** Slice lives behind a feature flag; frontend default remains reference backend.

---

## Phase 5 — Incremental module migration

**Scope.** Repeat the Phase 4 pattern in dependency order:
1. Objects: companies, opportunities, tasks, notes.
2. Metadata: object/field CRUD + dynamic schema regeneration.
3. Views: filters, sorts, groups, view CRUD (preserve exact semantics).
4. Relationships & timeline.
5. Auth parity (sessions, API keys, 2FA; SSO/OIDC/SAML only if licensed).
6. Search, record export/import, files/upload.
7. Jobs/cron, workflows, webhooks, integrations, extension/app platform.
Each module: failing parity test first → implement → make it pass → diff review.
**Deliverables.** Updated migration matrix with per-module status; parity test suite.
**Acceptance.** Each migrated module's upstream integration specs are reproduced (or a documented, justified subset) and pass against Rust.
**Risks.** Silent semantic drift (filters/sorts/pagination). Mitigation: golden fixtures + property tests.
**Dependencies.** Phase 4.
**Rollback.** Per-module routing flag toggles back to reference backend.

---

## Phase 6 — Parity & security hardening

**Scope.** RBAC/RLS completeness, rate limiting, SSRF defenses (port `secure-http-client` semantics), upload controls (size/type/AV considerations), CSRF/CORS policy, secret management, encryption-at-rest/key rotation, auditability, safe migrations with rollback.
**Deliverables.** `docs/threat-model.md`; security test suite (property/fuzz tests for authz decisions and parsers); migration rollback tests.
**Acceptance.** Threat model reviewed; fuzz/property tests run with reported results; rollback tested on real Postgres; `cargo audit` clean or exceptions documented.
**Risks.** Claiming security gains without evidence. Mitigation: every security claim tied to a reproducible test.
**Dependencies.** Phase 5 modules that touch the control surface.
**Rollback.** Controls are config-gated; revert middleware.

---

## Phase 7 — Benchmarks & deployment

**Scope.** Representative workloads; measure the reference (Node/NestJS) and Rust backends identically.
**Metrics.** p50/p95/p99 latency, throughput (RPS), memory (RSS), CPU, DB query counts per request, concurrency behaviour.
**Deliverables.** `docs/benchmark-methodology.md`; raw results (no fabricated numbers); `docker-compose.yml` (Postgres, Redis, server, worker, frontend); CI workflows (build, test, migrate+rollback, contract, e2e, `cargo audit`).
**Acceptance.** Methodology reproducible; raw data committed; comparison stated only where measured.
**Risks.** Unfair comparison (warm caches, different DB). Mitigation: same dataset, same hardware, same queries, documented.
**Dependencies.** Phase 6.
**Rollback.** Benchmarks are additive artifacts.

---

## Phase 8 — Documentation & release

**Scope.** README, contributor instructions, API compatibility doc (final), SBOM/dependency-audit guidance, release notes, known limitations.
**Deliverables.** `README.md`, `CONTRIBUTING.md`, `docs/sbom.md`, `SECURITY.md`, `CHANGELOG.md`, `docs/known-limitations.md`.
**Acceptance.** A new contributor can build + test + run locally from the docs alone.
**Risks.** Docs drift. Mitigation: docs referenced by CI where possible.
**Dependencies.** All prior phases.
**Rollback.** N/A.

---

## Working agreements

- Frontend and Rust backend remain **independently testable and deployable**.
- Never disable tests, remove features, silently change APIs, or mark incomplete work complete.
- Every phase starts with failing tests where behaviour is contractual.
- Attribution, `LICENSE`, and notices preserved on every derivative artifact.
- Decision log maintained; TODO and known-limitations kept current.
