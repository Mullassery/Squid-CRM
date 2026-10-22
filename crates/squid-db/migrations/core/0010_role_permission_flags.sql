-- Permission flags granted to roles.

CREATE TABLE IF NOT EXISTS core.role_permission_flags (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES core.workspaces(id) ON DELETE CASCADE,
    role_id      UUID NOT NULL REFERENCES core.roles(id) ON DELETE CASCADE,
    flag         TEXT NOT NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (role_id, flag)
);

CREATE INDEX IF NOT EXISTS role_permission_flags_role_idx
    ON core.role_permission_flags (role_id);
