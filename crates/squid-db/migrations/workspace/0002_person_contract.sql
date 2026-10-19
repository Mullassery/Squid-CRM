-- Align the person table with the captured v2.49.0 REST/GraphQL record shape.
-- JSON columns are stored as JSONB and read as text to keep sqlx feature-light.
ALTER TABLE {{schema}}.person
    ADD COLUMN IF NOT EXISTS linkedin_link JSONB NOT NULL
        DEFAULT '{"primaryLinkLabel":"","primaryLinkUrl":"","secondaryLinks":[]}',
    ADD COLUMN IF NOT EXISTS phones JSONB NOT NULL
        DEFAULT '{"primaryPhoneNumber":"","primaryPhoneCountryCode":"","primaryPhoneCallingCode":"","additionalPhones":[]}',
    ADD COLUMN IF NOT EXISTS avatar_url TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS avatar_file JSONB,
    ADD COLUMN IF NOT EXISTS position DOUBLE PRECISION NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS created_by JSONB NOT NULL
        DEFAULT '{"source":"API","workspaceMemberId":null,"name":"API","context":{}}',
    ADD COLUMN IF NOT EXISTS updated_by JSONB NOT NULL
        DEFAULT '{"source":"API","workspaceMemberId":null,"name":"API","context":{}}',
    ADD COLUMN IF NOT EXISTS search_vector TEXT,
    ADD COLUMN IF NOT EXISTS company_id UUID;
