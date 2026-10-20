-- Polymorphic task targets (company/person/opportunity).
CREATE TABLE IF NOT EXISTS {{schema}}.task_target (
    id                      UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    created_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at              TIMESTAMPTZ,
    position                DOUBLE PRECISION NOT NULL DEFAULT 0,
    created_by              JSONB NOT NULL,
    updated_by              JSONB NOT NULL,
    search_vector           TEXT,
    task_id                 UUID NOT NULL REFERENCES {{schema}}.task(id) ON DELETE CASCADE,
    target_company_id       UUID,
    target_opportunity_id   UUID,
    target_person_id        UUID
);

CREATE INDEX IF NOT EXISTS {{schema}}_task_target_task_idx ON {{schema}}.task_target (task_id);
CREATE INDEX IF NOT EXISTS {{schema}}_task_target_company_idx ON {{schema}}.task_target (target_company_id);
CREATE INDEX IF NOT EXISTS {{schema}}_task_target_opportunity_idx ON {{schema}}.task_target (target_opportunity_id);
CREATE INDEX IF NOT EXISTS {{schema}}_task_target_person_idx ON {{schema}}.task_target (target_person_id);
