-- Core schema: cross-workspace data (users, workspaces, sessions).
-- Mirrors the audited upstream distinction between a shared "core" schema and
-- per-workspace schemas. Kept intentionally small for the Phase 4 slice.

CREATE SCHEMA IF NOT EXISTS core;

CREATE TABLE IF NOT EXISTS core.users (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    email         TEXT NOT NULL,
    password_hash TEXT NOT NULL,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX IF NOT EXISTS users_email_lower_idx
    ON core.users (lower(email));

CREATE TABLE IF NOT EXISTS core.workspaces (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    display_name TEXT NOT NULL,
    subdomain    TEXT NOT NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX IF NOT EXISTS workspaces_subdomain_lower_idx
    ON core.workspaces (lower(subdomain));

CREATE TABLE IF NOT EXISTS core.workspace_members (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES core.workspaces(id) ON DELETE CASCADE,
    user_id      UUID NOT NULL REFERENCES core.users(id) ON DELETE CASCADE,
    role         TEXT NOT NULL DEFAULT 'MEMBER'
                 CHECK (role IN ('OWNER', 'ADMIN', 'MEMBER')),
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (workspace_id, user_id)
);

CREATE INDEX IF NOT EXISTS workspace_members_user_idx
    ON core.workspace_members (user_id);

CREATE TABLE IF NOT EXISTS core.user_sessions (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id      UUID NOT NULL REFERENCES core.users(id) ON DELETE CASCADE,
    workspace_id UUID NOT NULL REFERENCES core.workspaces(id) ON DELETE CASCADE,
    token_hash   TEXT NOT NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at   TIMESTAMPTZ NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS user_sessions_token_hash_idx
    ON core.user_sessions (token_hash);

CREATE INDEX IF NOT EXISTS user_sessions_user_idx
    ON core.user_sessions (user_id);
