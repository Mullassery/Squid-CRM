-- Opaque, rotating refresh tokens. Only SHA-256 hashes are stored. Access
-- tokens carry the session id (`sid`) so deleting the session revokes them.
CREATE TABLE IF NOT EXISTS core.refresh_tokens (
    id                     UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id                UUID NOT NULL REFERENCES core.users(id) ON DELETE CASCADE,
    workspace_id           UUID NOT NULL REFERENCES core.workspaces(id) ON DELETE CASCADE,
    session_id             UUID NOT NULL REFERENCES core.user_sessions(id) ON DELETE CASCADE,
    token_hash             TEXT NOT NULL,
    replaced_by_token_hash TEXT,
    created_at             TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at             TIMESTAMPTZ NOT NULL,
    revoked_at             TIMESTAMPTZ
);

CREATE UNIQUE INDEX IF NOT EXISTS refresh_tokens_hash_idx
    ON core.refresh_tokens (token_hash);

CREATE INDEX IF NOT EXISTS refresh_tokens_session_idx
    ON core.refresh_tokens (session_id);
