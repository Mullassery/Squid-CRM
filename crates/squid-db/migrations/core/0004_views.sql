-- Phase 5 slice: workspace views (read path used by the People table).
-- Mirrors the Squid `core."view"` tables consumed by the
-- frontend `ViewFragment`. Sub-entities are modelled so the fragment validates;
-- only view fields are seeded initially.

CREATE TABLE IF NOT EXISTS core.views (
    id                                        UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id                              UUID NOT NULL REFERENCES core.workspaces(id) ON DELETE CASCADE,
    universal_identifier                      UUID NOT NULL DEFAULT gen_random_uuid(),
    application_id                            UUID NOT NULL DEFAULT '00000000-0000-0000-0000-000000000000',
    is_system_side_effect                     BOOLEAN NOT NULL DEFAULT false,
    name                                      TEXT NOT NULL,
    object_metadata_id                        UUID NOT NULL REFERENCES core.object_metadata(id) ON DELETE CASCADE,
    type                                      TEXT NOT NULL DEFAULT 'TABLE',
    key                                       TEXT,
    icon                                      TEXT NOT NULL DEFAULT 'IconList',
    position                                  DOUBLE PRECISION NOT NULL DEFAULT 0,
    is_compact                                BOOLEAN NOT NULL DEFAULT false,
    is_custom                                 BOOLEAN NOT NULL DEFAULT false,
    open_record_in                            TEXT NOT NULL DEFAULT 'SIDE_PANEL',
    kanban_aggregate_operation                TEXT,
    kanban_aggregate_operation_field_metadata_id UUID,
    main_group_by_field_metadata_id           UUID,
    should_hide_empty_groups                  BOOLEAN NOT NULL DEFAULT false,
    kanban_column_width                       INTEGER,
    group_load_limit                          INTEGER,
    calendar_field_metadata_id                UUID,
    calendar_end_field_metadata_id            UUID,
    calendar_layout                           TEXT,
    visibility                                TEXT NOT NULL DEFAULT 'WORKSPACE',
    created_by_user_workspace_id              UUID,
    is_active                                 BOOLEAN NOT NULL DEFAULT true,
    any_field_filter_value                    TEXT,
    created_at                                TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at                                TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at                                TIMESTAMPTZ
);

CREATE UNIQUE INDEX IF NOT EXISTS views_workspace_object_key_idx
    ON core.views (workspace_id, object_metadata_id, key);

CREATE TABLE IF NOT EXISTS core.view_fields (
    id                   UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id         UUID NOT NULL REFERENCES core.workspaces(id) ON DELETE CASCADE,
    view_id              UUID NOT NULL REFERENCES core.views(id) ON DELETE CASCADE,
    field_metadata_id    UUID NOT NULL REFERENCES core.field_metadata(id) ON DELETE CASCADE,
    view_field_group_id  UUID,
    universal_identifier UUID NOT NULL DEFAULT gen_random_uuid(),
    application_id       UUID NOT NULL DEFAULT '00000000-0000-0000-0000-000000000000',
    is_system_side_effect BOOLEAN NOT NULL DEFAULT false,
    is_visible           BOOLEAN NOT NULL DEFAULT true,
    size                 DOUBLE PRECISION NOT NULL DEFAULT 150,
    position             DOUBLE PRECISION NOT NULL DEFAULT 0,
    aggregate_operation  TEXT,
    is_active            BOOLEAN NOT NULL DEFAULT true,
    created_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at           TIMESTAMPTZ,
    UNIQUE (view_id, field_metadata_id)
);

CREATE TABLE IF NOT EXISTS core.view_field_groups (
    id                   UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id         UUID NOT NULL REFERENCES core.workspaces(id) ON DELETE CASCADE,
    view_id              UUID NOT NULL REFERENCES core.views(id) ON DELETE CASCADE,
    name                 TEXT NOT NULL,
    position             DOUBLE PRECISION NOT NULL DEFAULT 0,
    is_visible           BOOLEAN NOT NULL DEFAULT true,
    is_active            BOOLEAN NOT NULL DEFAULT true,
    created_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at           TIMESTAMPTZ
);

CREATE TABLE IF NOT EXISTS core.view_filters (
    id                              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id                    UUID NOT NULL REFERENCES core.workspaces(id) ON DELETE CASCADE,
    view_id                         UUID NOT NULL REFERENCES core.views(id) ON DELETE CASCADE,
    field_metadata_id               UUID NOT NULL,
    operand                         TEXT NOT NULL,
    value                           JSONB NOT NULL DEFAULT '""'::jsonb,
    view_filter_group_id            UUID,
    position_in_view_filter_group   DOUBLE PRECISION,
    sub_field_name                  TEXT,
    relation_target_field_metadata_id UUID,
    created_at                      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at                      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at                      TIMESTAMPTZ
);

CREATE TABLE IF NOT EXISTS core.view_filter_groups (
    id                            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id                  UUID NOT NULL REFERENCES core.workspaces(id) ON DELETE CASCADE,
    view_id                       UUID NOT NULL REFERENCES core.views(id) ON DELETE CASCADE,
    parent_view_filter_group_id   UUID,
    logical_operator              TEXT NOT NULL DEFAULT 'AND',
    position_in_view_filter_group DOUBLE PRECISION,
    created_at                    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at                    TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at                    TIMESTAMPTZ
);

CREATE TABLE IF NOT EXISTS core.view_sorts (
    id                   UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id         UUID NOT NULL REFERENCES core.workspaces(id) ON DELETE CASCADE,
    view_id              UUID NOT NULL REFERENCES core.views(id) ON DELETE CASCADE,
    field_metadata_id    UUID NOT NULL,
    direction            TEXT NOT NULL DEFAULT 'ASC',
    sub_field_name       TEXT,
    created_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at           TIMESTAMPTZ
);

CREATE TABLE IF NOT EXISTS core.view_groups (
    id                   UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id         UUID NOT NULL REFERENCES core.workspaces(id) ON DELETE CASCADE,
    view_id              UUID NOT NULL REFERENCES core.views(id) ON DELETE CASCADE,
    is_visible           BOOLEAN NOT NULL DEFAULT true,
    field_value          TEXT NOT NULL,
    position             DOUBLE PRECISION NOT NULL DEFAULT 0,
    created_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at           TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS views_workspace_idx
    ON core.views (workspace_id);

CREATE INDEX IF NOT EXISTS view_fields_view_idx
    ON core.view_fields (view_id);
