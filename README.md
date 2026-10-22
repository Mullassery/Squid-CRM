# Squid-CRM

A production-grade CRM with a **Rust backend** (stable Rust, Tokio, Axum, SQLx,
PostgreSQL, GraphQL/OpenAPI) and the **existing React/TypeScript frontend**,
derived from [Twenty](https://github.com/twentyhq/twenty) and preserving its API
contract.

> **Status: Phase 4 complete.** Auth, workspace bootstrap, eight standard
> seeded objects (person, company, opportunity, task, note, task/note targets,
> workspaceMember metadata), metadata/view writes and contract-aligned REST +
> GraphQL surfaces are implemented and tested against real PostgreSQL. Dynamic
> schema generation and role enforcement remain Phase 5 work — see
> `docs/api-compatibility.md` and `docs/migration-matrix.md`.

## Quick start

```bash
cp .env.example .env            # set APP_SECRET (>= 32 chars)
docker compose up -d db redis   # or use a local PostgreSQL 16
cargo build --workspace
TEST_DATABASE_URL=postgres://postgres:postgres@127.0.0.1:5432/squid_test \
  cargo test --workspace
./scripts/smoke.sh              # boots the server and exercises the slice
```

Run:

```bash
DATABASE_URL=postgres://postgres:postgres@127.0.0.1:5432/squid \
APP_SECRET=$(openssl rand -hex 32) \
cargo run -p squid-server
```

## Implemented in this increment

- **Auth**: `POST /auth/signup`, `/auth/signin`, `/auth/signout`; Argon2id
  passwords; typed JWT access tokens; hashed opaque session cookies; bearer auth.
- **GraphQL**: `signUp`, `signIn`, `getLoginTokenFromCredentials`,
  `getAuthTokensFromLoginToken`, `signOut`, `currentUser`, `currentWorkspace`,
  `people`, `person`, `createPerson`, `deletePerson`.
- **REST**: `GET/POST /rest/people`, `GET/DELETE /rest/people/{id}`,
  `GET /rest/me`, `GET /healthz` with the upstream envelope and error contract.
- **Tenancy**: schema-per-workspace, validated schema identifiers, membership
  re-check on every request.
- **Tests**: 11 unit + 4 integration (real Postgres), plus a live smoke script.

## Repository layout

| Path | What |
| --- | --- |
| `crates/squid-core` | config, domain models, error contract |
| `crates/squid-db` | pool, migrations, tenant schemas, repositories |
| `crates/squid-auth` | passwords, tokens, sessions, `AuthContext` |
| `crates/squid-api` | Axum router, REST, GraphQL, CORS |
| `crates/squid-server` | binary |
| `docs/audit/` | Phase 1 audit + 8-phase plan |
| `docs/adr/` | architecture decisions |
| `docs/api-compatibility.md` | the compatibility contract |
| `docs/migration-matrix.md` | per-area status |
| `docs/decision-log.md` | decisions (D-001 licence strategy) |
| `reference/twenty` | pristine upstream clone (read-only, git-ignored) |
| `FIREWALL.md` | rules for using the reference |

## Checks

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
TEST_DATABASE_URL=... cargo test --workspace
./scripts/check-firewall.sh
```

## Licence

Squid-CRM is an AGPL-3.0 derivative work (plus the upstream Twenty Application
Exception). Enterprise-licensed upstream files are excluded. See `LICENSE` and
`NOTICE`. A legal review is a release gate (D-001).

## Next steps

See `TODO.md`. The immediate next increment is Phase 2 contract capture
(GraphQL SDL + OpenAPI fixtures) and then the metadata engine needed for real
frontend/GraphQL parity.
