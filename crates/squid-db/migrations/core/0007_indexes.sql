CREATE TABLE IF NOT EXISTS core.index_metadata (
    id                   UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id         UUID NOT NULL REFERENCES core.workspaces(id) ON DELETE CASCADE,
    object_metadata_id   UUID NOT NULL REFERENCES core.object_metadata(id) ON DELETE CASCADE,
    name                 TEXT NOT NULL,
    is_custom            BOOLEAN NOT NULL DEFAULT TRUE,
    is_unique            BOOLEAN NOT NULL DEFAULT FALSE,
    index_where_clause   TEXT,
    index_type           TEXT NOT NULL DEFAULT 'BTREE',
    created_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at           TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS index_metadata_workspace_idx
    ON core.index_metadata (workspace_id, object_metadata_id);

CREATE TABLE IF NOT EXISTS core.index_field_metadata (
    id                   UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id         UUID NOT NULL REFERENCES core.workspaces(id) ON DELETE CASCADE,
    index_metadata_id    UUID NOT NULL REFERENCES core.index_metadata(id) ON DELETE CASCADE,
    field_metadata_id    UUID NOT NULL REFERENCES core.field_metadata(id) ON DELETE CASCADE,
    position             DOUBLE PRECISION NOT NULL DEFAULT 0,
    sub_field_name       TEXT,
    created_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at           TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS index_field_metadata_index_idx
    ON core.index_field_metadata (index_metadata_id);
