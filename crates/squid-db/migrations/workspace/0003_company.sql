-- Standard company object for the Phase 5 module slice.
CREATE TABLE IF NOT EXISTS {{schema}}.company (
    id               UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at       TIMESTAMPTZ,
    name             TEXT,
    domain_name      JSONB NOT NULL DEFAULT '{"primaryLinkLabel":"","primaryLinkUrl":"","secondaryLinks":[]}'::jsonb,
    address          JSONB NOT NULL DEFAULT '{}'::jsonb,
    linkedin_link    JSONB NOT NULL DEFAULT '{"primaryLinkLabel":"","primaryLinkUrl":"","secondaryLinks":[]}'::jsonb,
    annual_revenue   JSONB NOT NULL DEFAULT '{"amountMicros":null,"currencyCode":""}'::jsonb,
    position         DOUBLE PRECISION NOT NULL DEFAULT 0,
    created_by       JSONB NOT NULL,
    updated_by       JSONB NOT NULL,
    search_vector    TEXT,
    account_owner_id UUID
);

CREATE INDEX IF NOT EXISTS {{schema}}_company_created_at_idx ON {{schema}}.company (created_at);
CREATE INDEX IF NOT EXISTS {{schema}}_company_name_idx ON {{schema}}.company (lower(name));
