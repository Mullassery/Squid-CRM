# Squid-CRM

A production-grade CRM built as a **Rust backend** (stable Rust, Tokio, Axum, SQLx,
PostgreSQL, OpenAPI) with the **existing React/TypeScript frontend**, derived from
[Twenty](https://github.com/twentyhq/twenty) and preserving its feature parity and
API contract.

> **Status: Phase 1 (audit) complete. No backend code yet.**
> See `docs/audit/` and `docs/decision-log.md` before contributing.

## Where things are

| Path | What |
| --- | --- |
| `reference/twenty` | Pristine, read-only clone of upstream at the pinned commit. Do not modify. |
| `docs/audit/01-repository-audit.md` | Phase 1 audit findings. |
| `docs/audit/02-phase-plan.md` | Eight-phase staged plan with acceptance/risks/rollback. |
| `docs/migration-matrix.md` | Per-area upstream → Rust mapping + status. |
| `docs/decision-log.md` | Decisions and open questions (D-001 is blocking). |
| `LICENSE` / `NOTICE` | Verbatim upstream licence + attribution. Preserve both. |
| `crates/` | Rust workspace (created in Phase 3). |
| `contracts/` | Captured API contracts / golden fixtures (Phase 2). |

## Licence

Squid-CRM is an AGPL-3.0 derivative work (plus the upstream Twenty Application
Exception). Enterprise-licensed upstream files are excluded. See `NOTICE`.

## Getting started

Nothing to build yet. Development begins after the Phase 1 checkpoint
(`docs/decision-log.md` → D-001).

## Environment (audited)

- Rust 1.99, Node 24.16 (frontend pin; `reference/twenty/.nvmrc`)
- PostgreSQL + Redis (via Docker Compose — planned in Phase 7)
- Docker 29.5.2
