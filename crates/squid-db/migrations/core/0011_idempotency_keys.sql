-- Replay-safe REST writes. A key is scoped to one workspace and stores the
-- completed response so retries return the original result instead of
-- creating another record. In-progress rows expire quickly so a crashed
-- process cannot block a key forever.

CREATE TABLE IF NOT EXISTS core.idempotency_keys (
    workspace_id          UUID NOT NULL REFERENCES core.workspaces(id) ON DELETE CASCADE,
    key                   TEXT NOT NULL,
    method                TEXT NOT NULL,
    path                  TEXT NOT NULL,
    request_hash          TEXT NOT NULL,
    state                 TEXT NOT NULL DEFAULT 'in_progress'
                          CHECK (state IN ('in_progress', 'completed')),
    response_status       SMALLINT,
    response_content_type TEXT,
    response_body         BYTEA,
    created_at            TIMESTAMPTZ NOT NULL DEFAULT now(),
    completed_at          TIMESTAMPTZ,
    expires_at            TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (workspace_id, key)
);

CREATE INDEX IF NOT EXISTS idempotency_keys_expiry_idx
    ON core.idempotency_keys (expires_at);
