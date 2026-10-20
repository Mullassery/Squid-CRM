-- Polymorphic note targets (company/person/opportunity).
CREATE TABLE IF NOT EXISTS {{schema}}.note_target (
    id                      UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    created_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at              TIMESTAMPTZ,
    position                DOUBLE PRECISION NOT NULL DEFAULT 0,
    created_by              JSONB NOT NULL,
    updated_by              JSONB NOT NULL,
    search_vector           TEXT,
    note_id                 UUID NOT NULL REFERENCES {{schema}}.note(id) ON DELETE CASCADE,
    target_company_id       UUID,
    target_opportunity_id   UUID,
    target_person_id        UUID
);

CREATE INDEX IF NOT EXISTS {{schema}}_note_target_note_idx ON {{schema}}.note_target (note_id);
CREATE INDEX IF NOT EXISTS {{schema}}_note_target_company_idx ON {{schema}}.note_target (target_company_id);
CREATE INDEX IF NOT EXISTS {{schema}}_note_target_opportunity_idx ON {{schema}}.note_target (target_opportunity_id);
CREATE INDEX IF NOT EXISTS {{schema}}_note_target_person_idx ON {{schema}}.note_target (target_person_id);
