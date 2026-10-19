-- Workspace API keys. The bearer secret is stored as a SHA-256 hash and is
-- only returned once by `generateApiKeyToken`.
CREATE TABLE IF NOT EXISTS core.api_keys (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES core.workspaces(id) ON DELETE CASCADE,
    created_by   UUID REFERENCES core.users(id) ON DELETE SET NULL,
    name         TEXT NOT NULL,
    role         TEXT NOT NULL DEFAULT 'MEMBER'
                 CHECK (role IN ('OWNER', 'ADMIN', 'MEMBER')),
    token_hash   TEXT,
    expires_at   TIMESTAMPTZ NOT NULL,
    revoked_at   TIMESTAMPTZ,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX IF NOT EXISTS api_keys_token_hash_idx
    ON core.api_keys (token_hash)
    WHERE token_hash IS NOT NULL;

CREATE INDEX IF NOT EXISTS api_keys_workspace_idx
    ON core.api_keys (workspace_id);
