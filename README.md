<div align="center">

# Squid-CRM

**A fast, self-hosted CRM with a Rust backend and a TypeScript web client.**

Axum · PostgreSQL · GraphQL · REST · multi-tenant · metadata-driven · Next.js

[![CI](https://github.com/Mullassery/Squid-CRM/actions/workflows/ci.yml/badge.svg)](https://github.com/Mullassery/Squid-CRM/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-AGPL--3.0-blue.svg)](./LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.85%2B-orange.svg)](https://www.rust-lang.org/)
[![Next.js](https://img.shields.io/badge/Next.js-16-black.svg)](https://nextjs.org/)
[![PostgreSQL](https://img.shields.io/badge/PostgreSQL-16-336791.svg)](https://www.postgresql.org/)
[![Status](https://img.shields.io/badge/status-alpha-orange.svg)](#status-and-roadmap)

[Quick start](#quick-start) · [Features](#features) · [Architecture](#architecture) · [API](#api) · [Security](#security) · [Contributing](#contributing)

</div>

---

> **Status: alpha.** Squid-CRM is a self-hosted CRM made of a Rust API server and
> a first-party Next.js frontend. Core CRM data paths, auth, tenancy, roles,
> custom objects, views, search, files and webhooks are implemented; coverage is
> tracked openly per route and per resolver.

## Why Squid-CRM

- **One static backend binary** — no Node runtime for the API; PostgreSQL is the
  only required service.
- **First-party web client** — `apps/web` is a typed Next.js app built for this
  API, not a vendored UI.
- **PostgreSQL-native tenancy** — one schema per workspace with a shared `core`
  schema for identity and metadata.
- **Metadata-driven** — objects, fields, views and roles are stored and edited at
  runtime.
- **Boring security defaults** — Argon2id passwords, short-lived JWTs, opaque
  rotating refresh tokens, hashed API keys, Origin checks and tenant isolation.

## Quick start

Prerequisites: Rust 1.85+, Node.js 20+, Docker (or PostgreSQL 16).

```bash
git clone https://github.com/Mullassery/Squid-CRM.git
cd Squid-CRM

docker compose up -d db
DATABASE_URL=postgres://postgres:postgres@localhost:5432/squid \
APP_SECRET=$(openssl rand -hex 32) \
cargo run -p squid-server
```

In another terminal, start the web client:

```bash
cd apps/web
npm install
npm run dev
```

Open <http://localhost:3001>, create a workspace, and start adding records. The
web app serves the UI and proxies `/api/*` to the Rust server at
`BACKEND_URL` (default `http://localhost:3000`).

Or run the whole stack in containers:

```bash
cp .env.example .env            # set APP_SECRET to 32+ random characters
docker compose up --build
```

Then open:

- Web client: <http://localhost:3001>
- API health: <http://localhost:3000/healthz>

### Tests

```bash
# Rust unit tests
cargo test --workspace

# Rust integration tests need a real PostgreSQL database
docker compose up -d db
docker compose exec db createdb -U postgres squid_test
TEST_DATABASE_URL=postgres://postgres:postgres@127.0.0.1:5432/squid_test \
  cargo test --workspace

# Frontend type-check, lint and production build
cd apps/web
npm install
npm run check
```

### Checks

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cd apps/web && npm run check
```

## Features

| Area | Implemented |
| --- | --- |
| **Auth** | Signup/signin/signout, Argon2id, HS256 access + login JWTs, opaque rotating refresh tokens, hashed session cookies, bearer auth, API-key lifecycle |
| **Web client** | Dashboard, global search, standard and custom record CRUD, archive/restore, CSV import/export, files, plus settings for members, roles, API keys, webhooks, data model, views and jobs |
| **Data API** | `/graphql` and `/rest` accept API keys or the `squid-session` cookie |
| **Tenancy** | Schema per workspace, validated schema identifiers, membership re-check on every request |
| **Objects** | People, companies, opportunities, tasks, notes, task/note targets and workspace members with CRUD, batch, soft-delete/restore, merge and null-clearing updates |
| **Queries** | Cursor pagination, `order_by` / `orderBy` grammar, scalar and nested relation filters, relation ordering, `groupBy` with view-based paging |
| **Metadata** | Standard object/field metadata, custom objects and fields, indexes, views, view fields/filters/sorts/groups |
| **Custom objects** | Physical tables and scalar/relation columns created from metadata; generic REST and JSON GraphQL CRUD |
| **Roles** | Metadata-backed built-in and custom roles, member/API-key assignment, permission flags, capability enforcement on REST/GraphQL, admin guards |
| **Search** | PostgreSQL ranked full-text search across person, company, opportunity, task and note |
| **Files** | PostgreSQL-backed storage with optional path-style S3, HMAC-signed upload/download URLs, `UPLOAD_FILE` / `DOWNLOAD_FILE` checks |
| **Webhooks** | CRUD with admin guards, core-object REST/GraphQL event delivery, bounded retries, durable delivery log and replay |
| **Jobs** | Durable `core.jobs` queue with admin enqueue/run, maintenance, webhook replay and `http.request` primitives |
| **Ops** | `/healthz`, structured tracing, Dockerfile, Compose stack, Rust and frontend CI |

The API is documented in [`docs/api.md`](./docs/api.md). The web app is documented
in [`docs/frontend.md`](./docs/frontend.md).

## Web client

`apps/web` is a Next.js App Router application with strict TypeScript and no UI
framework dependency. The main surfaces are:

| Area | Routes |
| --- | --- |
| Dashboard & search | `/dashboard`, `/search` |
| Standard records | `/objects/{people,companies,opportunities,tasks,notes,taskTargets,noteTargets}` |
| Custom records | `/custom/[object]` |
| Files | `/files` |
| Settings | `/settings`, members, roles, API keys, webhooks, data model, views, custom objects, jobs, GraphQL console and profile |

The browser talks to the Next.js server, which proxies `/api/*` to the Rust API
and keeps the HttpOnly `squid-session` cookie same-origin. See
[`docs/frontend.md`](./docs/frontend.md) for architecture and conventions.

## Architecture

```mermaid
flowchart LR
    FE["apps/web · Next.js + TypeScript"] -->|"/api/* proxy · cookie / API key"| API
    subgraph Rust["squid-server · Axum + Tokio"]
        API["/graphql · /rest · /auth<br/>/metadata · /healthz"]
        AUTH["squid-auth"]
        CORE["squid-core"]
        DB["squid-db"]
        API --> AUTH
        API --> CORE
        API --> DB
    end
    DB -->|SQLx| PG[("PostgreSQL 16")]
    subgraph PG
        CORE_SCHEMA[("core schema<br/>users · workspaces · sessions<br/>api keys · metadata · files")]
        WS_A[("workspace_<id>")]
        WS_B[("workspace_<id>")]
    end
    DB --- CORE_SCHEMA
    DB --- WS_A
    DB --- WS_B
    API -. future .-> REDIS[("Redis<br/>jobs · shared limits")]
```

| Path | Responsibility |
| --- | --- |
| `crates/squid-core` | Configuration, domain models, error contract |
| `crates/squid-db` | Pool, migrations, tenant schemas, repositories |
| `crates/squid-auth` | Passwords, JWTs, refresh tokens, sessions, API keys, `AuthContext` |
| `crates/squid-api` | Axum router, REST, GraphQL, CORS and auth middleware |
| `crates/squid-server` | The runnable binary |
| `apps/web` | Next.js web client, same-origin API proxy and typed API clients |
| `python/` | Optional Python client |
| `docs/` | API, architecture, ADRs and operations notes |

More detail: [`docs/architecture/overview.md`](./docs/architecture/overview.md)
and the ADRs in [`docs/adr/`](./docs/adr).

## API

- REST base: `/rest` for data, `/auth` for sessions, `/files` for uploads and
  downloads.
- GraphQL: `/graphql` for data and `/metadata` for metadata, roles, API keys,
  webhooks, views, files and admin jobs.
- Auth: signup/signin sets an HttpOnly `squid-session` cookie. Data APIs also
  accept `Authorization: Bearer api_...` API keys. User access tokens are for
  `/metadata` only.
- Proxy: the web client forwards `/api/*` to `BACKEND_URL`, so browser requests
  and cookies stay same-origin.

See [`docs/api.md`](./docs/api.md) for request/response shapes and the full
operation list.

## Security

| Control | Implementation |
| --- | --- |
| Passwords | Argon2id with a minimum length of 8 |
| Access tokens | HS256 JWT with `typ`, `sid` and `jti`; short TTL |
| Refresh tokens | Opaque, hashed at rest, rotated atomically, replay rejected |
| Sessions | Opaque 32-byte token, SHA-256 at rest, HttpOnly/SameSite=Lax cookie |
| API keys | `api_*` tokens, SHA-256 at rest, expiry and revocation checked per request |
| CSRF | Origin check on unsafe cookie-authenticated requests; bearer/API-key requests exempt |
| Tenancy | Workspace derived from the principal; schema name validated; membership re-checked |
| SQL | No user text interpolated into SQL; validated identifiers |
| Request size | 2 MiB body limit |
| Secrets | Environment only, never logged; `APP_SECRET` must be at least 32 characters |

Known gaps and the threat model are documented in [`SECURITY.md`](./SECURITY.md)
and [`docs/threat-model.md`](./docs/threat-model.md). Report vulnerabilities
privately through the repository's security advisory channel, not a public issue.

## Status and roadmap

Implemented through Phase 5 (core CRM slices, metadata writes, roles, custom
objects, search, files, webhooks and jobs) plus the first-party web client. Still
open:

- [ ] Metadata-generated GraphQL types/routes for custom objects
- [ ] OpenAPI document emission
- [ ] Remaining REST/GraphQL routes and relation-filter/order coverage
- [ ] Workflow engine, cron scheduling and distributed job workers
- [ ] OAuth, 2FA, app tokens and shared rate limiting
- [ ] Live-provider verification for S3 storage
- [ ] Email, calendar, timeline and third-party integrations
- [ ] Threat model review, fuzz/property tests, benchmarks, SBOM and release docs

The working checklist is
[`docs/internal/TODO.md`](./docs/internal/TODO.md).

## Contributing

Contributions are welcome, especially:

- Closing data, metadata and API coverage gaps
- Frontend features and accessibility improvements
- Integration tests against real PostgreSQL
- Security review of auth, tenancy and permission enforcement
- Performance work and benchmarks on realistic datasets

Read [`CONTRIBUTING.md`](./CONTRIBUTING.md) before opening a pull request. If
this direction is useful, **star the repo**.

## License

Squid-CRM is licensed under **AGPL-3.0**. See [`LICENSE`](./LICENSE) and
[`NOTICE`](./NOTICE) for the full attribution and modification notice. A legal
review remains a release gate.
