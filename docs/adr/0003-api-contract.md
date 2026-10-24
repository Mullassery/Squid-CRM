# ADR 0003 — API contract strategy

- Status: Accepted
- Date: 2026-10-19

## Context

Squid-CRM has two first-party clients: the Next.js web app in `apps/web` and the
optional Python client in `python/`. Both consume the same HTTP contract. The
server also supports direct REST and GraphQL consumers.

## Decision

1. **The HTTP contract is the boundary.** Document prefixes, envelopes, error
   shapes, cookies and cursor formats in `docs/api.md`; internal module layout is
   not part of the contract.
2. **No silent divergence.** Frontend and Python clients use the documented
   contract. Any change that breaks it is called out in the pull request and
   reflected in `docs/api.md`.
3. **Contract tests.** Integration tests assert the REST envelopes, auth flow,
   cursor format, filter/order grammar and permission checks against real
   PostgreSQL.
4. **First-party web client.** The frontend lives in `apps/web`, proxies `/api`
   to the Rust server and uses the `squid-session` cookie. It is not a vendored
   third-party UI.
5. **Metadata-driven direction.** Object, field, view and role metadata drive the
   API. Typed GraphQL generation for custom objects remains future work; the
   generic JSON CRUD path is the interim contract.

## Consequences

- The web client and direct API consumers share one source of truth.
- Partial coverage is explicit in `docs/internal/TODO.md`.
- Static hand-written resolvers are transitional; new object surfaces should be
  metadata-driven.

## Verification

- `cargo test` integration suite covers REST envelopes and auth.
- `scripts/smoke.sh` exercises the live binary.
- `apps/web` type-checks and builds against the documented API.
