# ADR 0002 — Tenant isolation model

- Status: Accepted (Phase 3/4)
- Date: 2026-10-19

## Context

Upstream isolates tenants with a **Postgres schema per workspace** plus a shared
core schema for users, workspaces, members and metadata. Cross-tenant leakage is
the highest-severity failure mode.

## Decision

1. Replicate the schema-per-workspace model.
2. Core schema (literal `core`) holds `users`, `workspaces`,
   `workspace_members`, `user_sessions`.
3. Workspace schemas are named `workspace_<uuid-simple>`.
4. The schema name is derived **only** from `Uuid` and validated against
   `^workspace_[0-9a-f]{32}$` before interpolation.
5. The authenticated principal carries `workspace_id`; every repository method
   requires it explicitly. There is no ambient "current workspace" global.
6. Membership is re-checked on every request (`find_membership_role`); a valid
   token for a workspace the user is no longer a member of is rejected.
7. Migrations take a Postgres advisory lock to make concurrent DDL safe.

## Invariants

- No SQL string is ever built from user-controlled text other than a validated
  schema identifier.
- A request scoped to workspace A can never produce a query against workspace
  B's schema.
- Deleting a workspace cascades members/sessions via FKs.

## Verification

- Unit: `squid_db::tests::rejects_injected_schema_names`.
- Integration: `tenant_isolation_blocks_cross_workspace_reads` (list, read,
  delete all blocked cross-tenant).

## Open work

- Row-level permission predicates (upstream `row-level-permission-predicate`)
  are not implemented; role checks are limited to built-in roles.
- A future `SECURITY DEFINER`/RLS defense-in-depth layer should be evaluated and
  benchmarked, not assumed.
