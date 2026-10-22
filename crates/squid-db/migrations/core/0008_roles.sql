-- Workspace roles and role assignments. Built-in roles keep the legacy enum
-- for authorization while custom roles can be assigned to members and API keys.

CREATE TABLE IF NOT EXISTS core.roles (
    id                                 UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id                       UUID NOT NULL REFERENCES core.workspaces(id) ON DELETE CASCADE,
    label                              TEXT NOT NULL,
    description                        TEXT,
    icon                               TEXT,
    is_editable                        BOOLEAN NOT NULL DEFAULT true,
    can_update_all_settings            BOOLEAN NOT NULL DEFAULT false,
    can_access_all_tools               BOOLEAN NOT NULL DEFAULT true,
    can_read_all_object_records        BOOLEAN NOT NULL DEFAULT true,
    can_update_all_object_records      BOOLEAN NOT NULL DEFAULT true,
    can_soft_delete_all_object_records BOOLEAN NOT NULL DEFAULT true,
    can_destroy_all_object_records     BOOLEAN NOT NULL DEFAULT false,
    can_be_assigned_to_users           BOOLEAN NOT NULL DEFAULT true,
    can_be_assigned_to_agents          BOOLEAN NOT NULL DEFAULT false,
    can_be_assigned_to_api_keys        BOOLEAN NOT NULL DEFAULT true,
    application_id                     UUID NOT NULL DEFAULT '00000000-0000-0000-0000-000000000000',
    universal_identifier               TEXT,
    created_at                         TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at                         TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (workspace_id, label)
);

CREATE INDEX IF NOT EXISTS roles_workspace_idx ON core.roles (workspace_id);

ALTER TABLE core.workspace_members
    ADD COLUMN IF NOT EXISTS role_id UUID REFERENCES core.roles(id) ON DELETE SET NULL;

CREATE INDEX IF NOT EXISTS workspace_members_role_idx
    ON core.workspace_members (role_id);

ALTER TABLE core.api_keys
    ADD COLUMN IF NOT EXISTS role_id UUID REFERENCES core.roles(id) ON DELETE SET NULL;

CREATE INDEX IF NOT EXISTS api_keys_role_idx
    ON core.api_keys (role_id);
