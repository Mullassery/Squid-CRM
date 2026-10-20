-- Standard opportunity object for the Phase 5 module slice.
CREATE TABLE IF NOT EXISTS {{schema}}.opportunity (
    id                   UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    created_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at           TIMESTAMPTZ,
    name                 TEXT,
    amount               JSONB NOT NULL DEFAULT '{"amountMicros":null,"currencyCode":""}'::jsonb,
    close_date           TIMESTAMPTZ,
    stage                TEXT NOT NULL DEFAULT 'NEW',
    position             DOUBLE PRECISION NOT NULL DEFAULT 0,
    created_by           JSONB NOT NULL,
    updated_by           JSONB NOT NULL,
    search_vector        TEXT,
    company_id           UUID,
    point_of_contact_id  UUID,
    owner_id             UUID
);

CREATE INDEX IF NOT EXISTS {{schema}}_opportunity_created_at_idx ON {{schema}}.opportunity (created_at);
CREATE INDEX IF NOT EXISTS {{schema}}_opportunity_company_id_idx ON {{schema}}.opportunity (company_id);
