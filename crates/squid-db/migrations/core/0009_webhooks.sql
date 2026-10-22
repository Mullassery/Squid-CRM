-- Workspace webhooks. Delivery workers are not implemented yet; this table
-- backs the webhook settings CRUD surface.

CREATE TABLE IF NOT EXISTS core.webhooks (
    id             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id   UUID NOT NULL REFERENCES core.workspaces(id) ON DELETE CASCADE,
    target_url     TEXT NOT NULL,
    operations     TEXT[] NOT NULL DEFAULT '{}',
    description    TEXT,
    secret         TEXT NOT NULL,
    application_id UUID NOT NULL DEFAULT '00000000-0000-0000-0000-000000000000',
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at     TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS webhooks_workspace_idx
    ON core.webhooks (workspace_id)
    WHERE deleted_at IS NULL;
