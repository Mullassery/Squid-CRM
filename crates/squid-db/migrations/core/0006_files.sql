-- Local file storage backing the metadata upload handshake. Production
-- deployments normally put bytes in object storage; this slice keeps them in
-- PostgreSQL so the contract works without external services.
CREATE TABLE IF NOT EXISTS core.files (
    id                UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id      UUID NOT NULL REFERENCES core.workspaces(id) ON DELETE CASCADE,
    filename          TEXT NOT NULL,
    path              TEXT NOT NULL,
    size              BIGINT NOT NULL DEFAULT 0,
    content_type      TEXT NOT NULL,
    file_folder       TEXT NOT NULL,
    field_metadata_id UUID,
    uploaded          BOOLEAN NOT NULL DEFAULT FALSE,
    content           BYTEA,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS files_workspace_idx ON core.files (workspace_id);
