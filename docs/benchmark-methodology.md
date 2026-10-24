# Benchmark Methodology — Squid-CRM

Status: Phase 7 deliverable (benchmarks). No benchmark numbers are published yet;
this document defines the methodology so that any future numbers are reproducible
and not fabricated.

Reference pin: `twentyhq/twenty@08a0e5f1ee2e7db7c1e6ef3aeec9bcef731f4e98`
(server `2.50.0`).

## 1. Principle

> No baseline performance numbers exist yet; no performance claims are made.
> (internal/TODO.md known limitation.)

Benchmarks must be measured on the reference (Twenty Node/NestJS) and on Squid-CRM
(Rust) identically: same dataset, same hardware, same queries, same database
state. Results are reported only where actually measured.

## 2. Benchmark environment

| Component | Specification |
|-----------|---------------|
| Hardware | Single bare-metal or dedicated VM; fixed vCPU count, fixed RAM |
| OS | Linux (CI runner or dedicated host) |
| PostgreSQL | 16, same configuration for both backends |
| Redis | (if used by either backend) same version/config |
| Dataset | Seeded workspace with N persons, companies, opportunities, tasks, notes |
| Concurrency | Fixed number of concurrent clients (e.g. 10, 50, 100) |
| Warm-up | Fixed number of warm-up requests before measurement |

## 3. Metrics

For each endpoint (REST `/rest/...`, GraphQL `/graphql`):

| Metric | Definition |
|--------|------------|
| p50 latency | Median response time |
| p95 latency | 95th percentile response time |
| p99 latency | 99th percentile response time |
| Throughput | Requests per second (RPS) |
| Memory | RSS of the backend process |
| CPU | CPU utilization percentage |
| DB query count | Queries per request (via pg_stat_statements or equivalent) |
| Error rate | % of non-2xx / non-200 responses |

## 4. Endpoints to benchmark

| Area | Endpoints |
|------|-----------|
| Auth | `POST /auth/signup`, `POST /auth/signin`, `POST /auth/refresh` |
| People (REST) | `GET /rest/people`, `GET /rest/people/:id`, `POST /rest/people`, `PATCH /rest/people/:id` |
| People (GraphQL) | `people`, `createPerson`, `updatePerson` |
| Companies | `GET /rest/companies`, `POST /rest/companies` |
| Opportunities | `GET /rest/opportunities`, `POST /rest/opportunities` |
| Tasks | `GET /rest/tasks`, `POST /rest/tasks` |
| Notes | `GET /rest/notes`, `POST /rest/notes` |
| Search | GraphQL `search` across objects |
| Metadata | `getRoles`, `getViews`, `createOneObject` |
| Webhooks | `createWebhook`, `webhookDeliveries` |

## 5. Procedure

1. Deploy the reference Twenty stack (v2.49.0 Docker image) against a fresh PostgreSQL 16.
2. Seed an identical dataset into both reference and Squid-CRM databases.
3. Run warm-up requests against each backend.
4. Run the benchmark suite (fixed concurrency, fixed duration, e.g. 60 seconds).
5. Record raw results (latency histograms, RPS, RSS, CPU, DB query counts).
6. Repeat 3×; report median of runs.
7. Publish raw data; state comparison only where measured.

## 6. Output format

Raw results are committed as structured data (e.g. JSON/CSV) under `docs/benchmarks/`
with:
- Backend version / commit hash
- Dataset size and composition
- Hardware description
- All metrics per endpoint
- Timestamp of measurement

## 7. Comparison rules

- A comparison between reference and Squid-CRM is stated **only** when both
  backends have been benchmarked under identical conditions.
- No relative performance claim (e.g. "X% faster") is made without raw data
  for both sides.
- Squid-CRM-only numbers (e.g. "p95 latency of endpoint Y") may be published
  without comparison.

## 8. CI integration

Benchmark runs are **not** part of the default CI pipeline (they require a
dedicated environment). They are run on-demand and results are recorded in
`docs/decision-log.md` as a checkpoint.
