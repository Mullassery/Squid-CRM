# ADR 0001 — Rust stack and crate layout

- Status: Accepted (Phase 3)
- Date: 2026-10-19
- Reference: twentyhq/twenty@08a0e5f1

## Context

The target architecture mandates stable Rust, Tokio, Axum, SQLx, PostgreSQL and
OpenAPI. The audited backend is NestJS + TypeORM + GraphQL Yoga with
metadata-driven, runtime-generated schemas and a custom ORM.

## Decision

- **Async runtime:** Tokio.
- **HTTP:** Axum 0.8 (native tower ecosystem; `async-graphql-axum` integration).
- **Database:** SQLx 0.8 with the tokio-rustls runtime, Postgres, UUID and chrono
  features. Compile-time checked queries are avoided for now because workspace
  schemas are dynamic; dynamic SQL is built only from validated identifiers.
- **GraphQL:** `async-graphql` 7 + `async-graphql-axum`. Chosen over Juniper for
  its mature dynamic-schema support and Axum integration, which a
  metadata-driven schema will require.
- **OpenAPI:** `utoipa` planned for the REST surface (not yet added; Phase 5).
- **Passwords:** Argon2id (`argon2` crate).
- **Tokens:** `jsonwebtoken` (HS256), typed `typ` claim.
- **Observability:** `tracing` + `tracing-subscriber`; OTLP planned.

Crate layout:

| Crate | Responsibility |
| --- | --- |
| `squid-core` | config, domain models, error contract |
| `squid-db` | pool, migrations, tenant schemas, repositories |
| `squid-auth` | password hashing, JWT/opaque sessions, `AuthContext` |
| `squid-api` | Axum router, REST handlers, GraphQL schema |
| `squid-server` | binary wiring + graceful shutdown |

## Consequences

- Additive crates; each is independently testable.
- SQLx compile-time macros are not used, so a schema drift cannot break the
  build; integration tests carry that responsibility instead.
- `async-graphql` requires distinct input/output type names; a mapping layer is
  needed once metadata types are generated.

## Alternatives considered

- **Juniper:** weaker dynamic-schema ergonomics; rejected.
- **Diesel:** compile-time schema fits static schemas, not per-workspace dynamic
  tables; rejected.
- **`sea-orm`:** adds an abstraction over dynamic schemas that we would have to
  bypass; rejected for the slice.
