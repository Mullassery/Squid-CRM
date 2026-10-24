# Contributing to Squid-CRM

## Read first

- `docs/api.md` — REST and GraphQL surface.
- `docs/frontend.md` — web client structure and conventions.
- `docs/decision-log.md` — architectural decisions.
- `docs/adr/` — architecture decision records.

## Prerequisites

- Rust (stable; workspace `rust-version = 1.85`).
- PostgreSQL 16 (Docker or local).
- Node.js 20+ and npm 10+ for `apps/web`.

## Local setup

```bash
cp .env.example .env          # change APP_SECRET
docker compose up -d db redis # or use a local PostgreSQL
cargo build --workspace
cargo test --workspace        # unit tests
```

Integration tests need a real database:

```bash
createdb squid_test
TEST_DATABASE_URL=postgres://$USER@127.0.0.1:5432/squid_test \
  cargo test --workspace
```

Live smoke test:

```bash
TEST_DATABASE_URL=postgres://$USER@127.0.0.1:5432/squid_test ./scripts/smoke.sh
```

Run the server:

```bash
DATABASE_URL=postgres://$USER@127.0.0.1:5432/squid \
APP_SECRET=$(openssl rand -hex 32) \
cargo run -p squid-server
```

Run the web client:

```bash
cd apps/web
npm install
npm run dev
```

The dev server listens on <http://localhost:3001> and proxies `/api/*` to
`http://localhost:3000` (`BACKEND_URL` overrides the target).

## Checks required before a PR

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
TEST_DATABASE_URL=... cargo test --workspace
cd apps/web && npm run check
```

## Conventions

- Keep the API contract in `docs/api.md` and the frontend guide in
  `docs/frontend.md` current.
- Any new workspace-scoped query must take `workspace_id` and use the validated
  schema helper.
- Frontend code is strict TypeScript. Prefer explicit types at API boundaries and
  keep server state in the shared `src/lib` clients.
- Do not disable or skip tests. If a test cannot run, report it explicitly.
- Add an ADR for any architectural change.

## Pull requests

State scope, acceptance criteria, risks and rollback in the PR description. Do
not claim performance without reproducible evidence.
