<div align="center">

# Squid-CRM

**A fast, self-hosted CRM backend in Rust, built to speak Twenty's API.**

Axum · PostgreSQL · GraphQL · REST · multi-tenant · metadata-driven.

[![CI](https://github.com/Mullassery/Squid-CRM/actions/workflows/ci.yml/badge.svg)](https://github.com/Mullassery/Squid-CRM/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-AGPL--3.0-blue.svg)](./LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.85%2B-orange.svg)](https://www.rust-lang.org/)
[![PostgreSQL](https://img.shields.io/badge/PostgreSQL-16-336791.svg)](https://www.postgresql.org/)
[![Status](https://img.shields.io/badge/status-alpha-orange.svg)](#status-and-roadmap)

[Quick start](#quick-start) · [What works](#what-works) · [Compatibility](#compatibility-with-twenty) · [Architecture](#architecture) · [Security](#security) · [Contributing](#contributing)

</div>

---

> **Status: alpha.** Squid-CRM reimplements the Twenty server contract in Rust
> against captured v2.50.0 fixtures. Core CRM data paths, auth, tenancy, roles,
> custom objects, views, search, files and webhooks are implemented; coverage is
> partial and tracked openly per route and per resolver. This is not a drop-in
> replacement for every Twenty feature yet.

## Why another CRM backend?

Twenty is a strong open-source CRM with a large React frontend and an
API-first design. Squid-CRM keeps the API contract and replaces the server with
a Rust core built on Axum, Tokio, SQLx and PostgreSQL:

- **One static binary** for the API server — no Node runtime in production.
- **PostgreSQL-native tenancy** with one schema per workspace.
- **Explicit compatibility.** Golden GraphQL/OpenAPI fixtures and per-area
  status are checked in, so "supported" means tested against a captured
  contract.
- **Boring security defaults.** Argon2id passwords, short-lived JWTs, opaque
  rotating refresh tokens, hashed API keys, Origin checks and tenant isolation.
- **Your data stays yours.** Self-host the backend and point the Twenty
  frontend at it.

## Quick start

Prerequisites: Rust 1.85+, Docker (or PostgreSQL 16).

```bash
git clone https://github.com/Mullassery/Squid-CRM.git
cd Squid-CRM

docker compose up -d db
DATABASE_URL=postgres://postgres:postgres@localhost:5432/squid \
APP_SECRET=$(openssl rand -hex 32) \
cargo run -p squid-server
```

The API listens on `http://localhost:3000`:

```bash
curl http://localhost:3000/healthz
```

Or run the whole stack (database, Redis and server) in containers:

```bash
cp .env.example .env            # set APP_SECRET to 32+ random characters
docker compose up --build
```

Point the upstream Twenty React frontend at the Rust server with
`REACT_APP_SERVER_BASE_URL=http://localhost:3000`. The frontend is not vendored
in this repository.

### Tests

```bash
# Unit tests
cargo test --workspace

# Integration tests need a real PostgreSQL database
docker compose up -d db
docker compose exec db createdb -U postgres squid_test
TEST_DATABASE_URL=postgres://postgres:postgres@127.0.0.1:5432/squid_test \
  cargo test --workspace

# Boot the server and exercise auth + REST + GraphQL
TEST_DATABASE_URL=postgres://postgres:postgres@127.0.0.1:5432/squid_test \
  ./scripts/smoke.sh
```

### Checks

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
./scripts/check-firewall.sh
```

## What works

| Area | Implemented |
| --- | --- |
| **Auth** | Signup/signin/signout, Argon2id, HS256 access + login JWTs, opaque rotating refresh tokens, hashed session cookies, bearer auth, API-key lifecycle |
| **Data API** | `/graphql` and `/rest` accept API keys or the session cookie; user access tokens are rejected, matching upstream |
| **Tenancy** | Schema per workspace, validated schema identifiers, membership re-check on every request |
| **Objects** | People, companies, opportunities, tasks, notes, task/note targets and workspace members with CRUD, batch, soft-delete/restore, merge and null-clearing updates |
| **Queries** | Cursor pagination, `order_by` / `orderBy` grammar, scalar and nested relation filters, relation ordering, `groupBy` with view-based paging |
| **Metadata** | Standard object/field metadata, custom objects and fields, indexes, views, view fields/filters/sorts/groups |
| **Custom objects** | Physical tables and scalar/relation columns created from metadata; generic REST CRUD fallback |
| **Roles** | Metadata-backed built-in and custom roles, member/API-key assignment, permission flags, capability enforcement on REST/GraphQL, admin guards |
| **Search** | PostgreSQL ranked full-text search across person, company, opportunity, task and note |
| **Files** | PostgreSQL-backed storage, HMAC-signed upload/download URLs, `UPLOAD_FILE` / `DOWNLOAD_FILE` checks |
| **Webhooks** | CRUD with admin guards; person REST create/update/delete events delivered with bounded retries |
| **Ops** | `/healthz`, structured tracing, Dockerfile, Compose stack, CI against real PostgreSQL |

REST and GraphQL coverage is **partial**. The per-route and per-resolver status
is checked in:

- [`docs/rest-route-map.md`](./docs/rest-route-map.md)
- [`docs/frontend-operation-map.md`](./docs/frontend-operation-map.md)
- [`docs/migration-matrix.md`](./docs/migration-matrix.md)

## Compatibility with Twenty

Squid-CRM is a derivative work targeting the public API contract of
[Twenty](https://github.com/twentyhq/twenty) **v2.50.0** (pinned source commit
`08a0e5f1`). The repository contains captured GraphQL SDL, OpenAPI documents and
real request/response examples under [`contracts/v2.50.0/`](./contracts/README.md).

What that means in practice:

- Envelopes, error shapes, cursor format, ordering/filter grammar and auth rules
  are matched against fixtures, not guessed.
- Authentication mutations live on `/metadata`; the data API accepts API keys,
  exactly as upstream does.
- Unimplemented Enterprise-licensed areas (SSO/SAML, billing, the admin panel)
  are deliberately excluded rather than approximated.

This project is **not affiliated with or endorsed by Twenty.com, PBC**. Twenty is
AGPL-3.0 with the Twenty Application Exception; see [`NOTICE`](./NOTICE) for the
full attribution and modification notice.

## Architecture

```mermaid
flowchart LR
    FE["Twenty React frontend<br/>(external)"] -->|"HTTPS · cookie / API key"| API
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

| Crate | Responsibility |
| --- | --- |
| `squid-core` | Configuration, domain models, error contract |
| `squid-db` | Pool, migrations, tenant schemas, repositories |
| `squid-auth` | Passwords, JWTs, refresh tokens, sessions, API keys, `AuthContext` |
| `squid-api` | Axum router, REST, GraphQL, CORS and auth middleware |
| `squid-server` | The runnable binary |

More detail: [`docs/architecture/overview.md`](./docs/architecture/overview.md)
and the ADRs in [`docs/adr/`](./docs/adr).

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

Known gaps and the threat model are documented in [`SECURITY.md`](./SECURITY.md).
Report vulnerabilities privately through the repository's security advisory
channel, not a public issue.

## Status and roadmap

Implemented through Phase 5 (core CRM slices, metadata writes, roles, custom
objects, search, files, webhooks). Still open:

- [ ] Metadata-generated GraphQL types/routes for custom objects
- [ ] OpenAPI document emission
- [ ] Remaining REST routes and relation-filter/order parity
- [ ] Background jobs, cron and workflow engine
- [ ] Webhook delivery for every object type
- [ ] OAuth, 2FA, app tokens and shared rate limiting
- [ ] S3-compatible file storage
- [ ] Import/export, email, calendar, timeline and integrations
- [ ] Threat model, fuzz/property tests, benchmarks, SBOM and release docs

The authoritative per-area status is [`docs/migration-matrix.md`](./docs/migration-matrix.md);
the working checklist is [`docs/internal/TODO.md`](docs/internal/TODO.md).

## Contributing

Contributions are welcome, especially:

- Closing parity gaps tracked in the migration matrix and route map
- Integration tests against captured fixtures (never against copied source)
- Security review of auth, tenancy and permission enforcement
- Performance work and benchmarks on realistic datasets

Read [`CONTRIBUTING.md`](./CONTRIBUTING.md) and [`FIREWALL.md`](./FIREWALL.md)
before touching the reference clone. If this direction is useful, **star the
repo** so more people building on Twenty can find it.

## License

Squid-CRM is licensed under **AGPL-3.0**, together with the upstream Twenty
Application Exception. Enterprise-licensed upstream files are excluded. See
[`LICENSE`](./LICENSE) and [`NOTICE`](./NOTICE). A legal review remains a release
gate.
