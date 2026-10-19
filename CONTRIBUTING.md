# Contributing to Squid-CRM

## Read first

- `docs/audit/01-repository-audit.md` — what the reference does.
- `docs/decision-log.md` — decisions, including the licence strategy (D-001).
- `FIREWALL.md` — the reference is a **specification only**; do not copy code.

## Prerequisites

- Rust (stable; workspace `rust-version = 1.85`).
- PostgreSQL 16 (Docker or local).
- Node 24.16 only if you are running the existing frontend.

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

## Checks required before a PR

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
TEST_DATABASE_URL=... cargo test --workspace
```

## Conventions

- No copying from `reference/twenty` (see `FIREWALL.md`).
- Keep the REST/GraphQL contract in `docs/api-compatibility.md` current.
- Any new workspace-scoped query must take `workspace_id` and use the validated
  schema helper.
- Do not disable or skip tests. If a test cannot run, report it explicitly.
- Add an ADR for any architectural change.

## Phase discipline

Work advances phase by phase (`docs/audit/02-phase-plan.md`). State scope,
acceptance criteria, risks and rollback in the PR description. Do not claim
parity or performance without reproducible evidence.
