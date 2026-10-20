-- Standard task object for the Phase 5 module slice.
CREATE TABLE IF NOT EXISTS {{schema}}.task (
    id             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at     TIMESTAMPTZ,
    position       DOUBLE PRECISION NOT NULL DEFAULT 0,
    title          TEXT,
    body_v2        JSONB NOT NULL DEFAULT '{}'::jsonb,
    due_at         TIMESTAMPTZ,
    status         TEXT DEFAULT 'TODO',
    created_by     JSONB NOT NULL,
    updated_by     JSONB NOT NULL,
    search_vector  TEXT,
    assignee_id    UUID
);

CREATE INDEX IF NOT EXISTS {{schema}}_task_created_at_idx ON {{schema}}.task (created_at);
