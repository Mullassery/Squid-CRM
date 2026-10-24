# ADR 0005 — `currentUser` / `currentWorkspace` boot contract

- Status: Accepted (Phase 5 slice)
- Date: 2026-10-20

## Context

The web client boot query (`currentUser`) joins identity, workspace settings,
roles/permissions, feature flags, SSO, billing and workspace URLs in one
GraphQL document. A single missing field makes the whole query fail. The Rust
slice has real identity/workspace data but no roles, billing, SSO or enterprise
permission model yet.

## Decision

1. Serve the full current-user boot shape from the live user,
   workspace and membership rows: id/email/name, workspace member profile,
   current workspace identity and URLs, member counts and timestamps.
2. Return explicit static defaults for surfaces not implemented yet:
   - `activationStatus = ACTIVE` (no PENDING_CREATION lifecycle yet),
   - one built-in `Role` derived from the membership role (Owner/Admin/Member),
   - empty feature flags, installed applications, available workspaces, SSO,
     billing subscriptions/customer/entitlements, permission flags and
     object/row-level permissions,
   - `permissionFlags` are typed as strings until
     the permission engine exists.
3. Do not claim enterprise capability: nullable billing/SSO data is `null` and
   list surfaces are empty rather than fabricated.
4. Keep the REST auth response (`/auth/*`) on the compact `User`/`Workspace`
   structs; only the GraphQL boot types expand.

## Consequences

- The Squid boot query validates and returns real identity/workspace data.
- Settings that depend on permissions, billing or SSO will render in a
  restricted/empty state, not with fabricated entitlements.
- `ApiKey.role` still serialises the internal role enum (GraphQL name
  `WorkspaceRole`); aligning it with the metadata `Role` object is tracked in
  `../internal/TODO.md`.

## Verification

- `current_user_matches_frontend_boot_fragment` runs the complete fragment and
  asserts identity, workspace, member and role defaults.
