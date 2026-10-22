-- Durable webhook delivery log. Rows are created before the first attempt and
-- updated after each attempt so failed deliveries can be inspected/replayed.

CREATE TABLE IF NOT EXISTS core.webhook_deliveries (
    id               UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id     UUID NOT NULL REFERENCES core.workspaces(id) ON DELETE CASCADE,
    webhook_id       UUID NOT NULL REFERENCES core.webhooks(id) ON DELETE CASCADE,
    event            TEXT NOT NULL,
    event_id         UUID NOT NULL,
    state            TEXT NOT NULL DEFAULT 'pending'
                     CHECK (state IN ('pending', 'succeeded', 'failed')),
    attempts         INT NOT NULL DEFAULT 0,
    last_status_code INT,
    last_error       TEXT,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    completed_at     TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS webhook_deliveries_workspace_idx
    ON core.webhook_deliveries (workspace_id, created_at DESC);

CREATE INDEX IF NOT EXISTS webhook_deliveries_failed_idx
    ON core.webhook_deliveries (workspace_id, state)
    WHERE state = 'failed';
