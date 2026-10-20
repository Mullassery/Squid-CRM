-- Phase 5 slice: workspace object/field metadata.
-- Mirrors the subset of upstream `core."objectMetadata"` / `core."fieldMetadata"`
-- consumed by the frontend metadata fragment. Metadata is workspace-scoped.

CREATE TABLE IF NOT EXISTS core.object_metadata (
    id                                UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id                      UUID NOT NULL REFERENCES core.workspaces(id) ON DELETE CASCADE,
    universal_identifier              UUID NOT NULL DEFAULT gen_random_uuid(),
    name_singular                     TEXT NOT NULL,
    name_plural                       TEXT NOT NULL,
    label_singular                    TEXT NOT NULL,
    label_plural                      TEXT NOT NULL,
    description                       TEXT,
    icon                              TEXT,
    color                             TEXT,
    shortcut                          TEXT,
    is_remote                         BOOLEAN NOT NULL DEFAULT false,
    is_active                         BOOLEAN NOT NULL DEFAULT true,
    is_system                         BOOLEAN NOT NULL DEFAULT false,
    is_ui_editable                    BOOLEAN NOT NULL DEFAULT true,
    is_ui_creatable                   BOOLEAN NOT NULL DEFAULT true,
    is_searchable                     BOOLEAN NOT NULL DEFAULT false,
    open_record_in                    TEXT NOT NULL DEFAULT 'USER_CHOICE',
    readability                       TEXT NOT NULL DEFAULT 'OPEN',
    sharing_reach                     TEXT NOT NULL DEFAULT 'ROLE_ACCESS',
    writability                       TEXT NOT NULL DEFAULT 'OPEN',
    application_id                    UUID NOT NULL DEFAULT '00000000-0000-0000-0000-000000000000',
    label_identifier_field_metadata_id UUID,
    image_identifier_field_metadata_id UUID,
    is_label_synced_with_name         BOOLEAN NOT NULL DEFAULT false,
    duplicate_criteria                JSONB,
    created_at                        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at                        TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (workspace_id, name_singular),
    UNIQUE (workspace_id, name_plural)
);

CREATE TABLE IF NOT EXISTS core.field_metadata (
    id                                UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id                      UUID NOT NULL REFERENCES core.workspaces(id) ON DELETE CASCADE,
    object_metadata_id                UUID NOT NULL REFERENCES core.object_metadata(id) ON DELETE CASCADE,
    universal_identifier              UUID NOT NULL DEFAULT gen_random_uuid(),
    type                              TEXT NOT NULL,
    name                              TEXT NOT NULL,
    label                             TEXT NOT NULL,
    description                       TEXT,
    icon                              TEXT,
    is_active                         BOOLEAN NOT NULL DEFAULT true,
    is_system                         BOOLEAN NOT NULL DEFAULT false,
    is_ui_editable                    BOOLEAN NOT NULL DEFAULT true,
    writability                       TEXT NOT NULL DEFAULT 'OPEN',
    is_nullable                       BOOLEAN NOT NULL DEFAULT true,
    is_unique                         BOOLEAN NOT NULL DEFAULT false,
    is_searchable                     BOOLEAN NOT NULL DEFAULT false,
    is_audit_logged                   BOOLEAN NOT NULL DEFAULT true,
    default_value                     JSONB,
    options                           JSONB,
    settings                          JSONB,
    is_label_synced_with_name         BOOLEAN NOT NULL DEFAULT false,
    morph_id                          UUID,
    application_id                    UUID NOT NULL DEFAULT '00000000-0000-0000-0000-000000000000',
    created_at                        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at                        TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (object_metadata_id, name)
);

CREATE INDEX IF NOT EXISTS object_metadata_workspace_idx
    ON core.object_metadata (workspace_id);

CREATE INDEX IF NOT EXISTS field_metadata_workspace_idx
    ON core.field_metadata (workspace_id);

CREATE INDEX IF NOT EXISTS field_metadata_object_idx
    ON core.field_metadata (object_metadata_id);
