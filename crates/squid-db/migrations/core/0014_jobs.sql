-- Durable background jobs. Workers claim due rows with SKIP LOCKED and record
-- attempts/failures. Maintenance jobs are rescheduled by the worker.

CREATE TABLE IF NOT EXISTS core.jobs (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id    UUID REFERENCES core.workspaces(id) ON DELETE CASCADE,
    kind            TEXT NOT NULL,
    payload         JSONB NOT NULL DEFAULT '{}'::jsonb,
    state           TEXT NOT NULL DEFAULT 'pending'
                    CHECK (state IN ('pending', 'running', 'succeeded', 'failed')),
    attempts        INT NOT NULL DEFAULT 0,
    max_attempts    INT NOT NULL DEFAULT 5,
    run_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_error      TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    completed_at    TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS jobs_due_idx
    ON core.jobs (run_at)
    WHERE state = 'pending';

CREATE INDEX IF NOT EXISTS jobs_workspace_idx
    ON core.jobs (workspace_id, created_at DESC);
