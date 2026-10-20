-- Standard note object for the Phase 5 module slice.
CREATE TABLE IF NOT EXISTS {{schema}}.note (
    id             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at     TIMESTAMPTZ,
    position       DOUBLE PRECISION NOT NULL DEFAULT 0,
    title          TEXT,
    body_v2        JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_by     JSONB NOT NULL,
    updated_by     JSONB NOT NULL,
    search_vector  TEXT
);

CREATE INDEX IF NOT EXISTS {{schema}}_note_created_at_idx ON {{schema}}.note (created_at);
