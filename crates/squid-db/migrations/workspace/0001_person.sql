-- Per-workspace schema. `{{schema}}` is replaced with a validated workspace
-- schema name (never raw user input).
CREATE TABLE IF NOT EXISTS {{schema}}.person (
    id                UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name_first        TEXT NOT NULL DEFAULT '',
    name_last         TEXT NOT NULL DEFAULT '',
    primary_email     TEXT NOT NULL DEFAULT '',
    additional_emails TEXT[] NOT NULL DEFAULT '{}',
    job_title         TEXT,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at        TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS person_created_at_idx ON {{schema}}.person (created_at);
CREATE INDEX IF NOT EXISTS person_primary_email_idx ON {{schema}}.person (primary_email);
