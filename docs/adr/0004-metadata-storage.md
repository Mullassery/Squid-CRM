# ADR 0004 — Workspace metadata storage and read API

- Status: Accepted (Phase 5 slice)
- Date: 2026-10-20

## Context

The web client boots by querying `/metadata` for every object and field
(`ObjectMetadataItems`, `objects(paging: { first: 1000 })`). Without that query
the UI cannot render records even though the person data API is complete.

Squid keeps metadata in the shared `core` schema (`core."objectMetadata"`,
`core."fieldMetadata"`) with a `workspaceId` column, while record data lives in
`workspace_<id>` schemas. Standard objects/fields are seeded per workspace.

## Decision

1. Metadata lives in `core.object_metadata` and
   `core.field_metadata`, both scoped by `workspace_id`, not in per-workspace
   schemas.
2. Start with the standard `person` object and its 16 person fields. Seeding is
   idempotent (`ON CONFLICT DO NOTHING`) and runs lazily on first metadata read,
   so existing workspaces created before this migration also get metadata.
3. Expose the read shapes used by the web client:
   `objects(paging, filter)`, `object(id)`, `fields(paging, filter)`,
   `field(id)`, plus `Object.fieldsList` and `Object.fields(paging)`.
4. Use a zero UUID as `applicationId` until an application registry exists. The
   frontend treats it as an opaque identifier.
5. `indexMetadataList` and `searchFieldMetadataList` are empty and `relation` is
   `null` for standard person fields in this slice. The fields are present so
   the web client fragment validates; no index/relation metadata is claimed.
6. Seed one default `TABLE`/`INDEX` view ("All people") per person object with
   five view fields. View filters, sorts, groups and field groups are modelled
   with tables and GraphQL types but return empty lists; view writes are out of
   scope for this slice.

## Consequences

- The web client can load person object/field definitions and render the
  People module against the Rust API.
- Metadata is static for now; it is not yet the source of truth for record
  validation or dynamic GraphQL/REST schemas.
- Adding a standard object (companies, opportunities, …) means adding its seed
  data and record repository; no schema migration is needed for metadata.
- Metadata writes (`createOneObject`, `updateOneField`, …), views, roles and
  `minimalMetadata` remain unimplemented and are listed in `../internal/TODO.md`.

## Verification

- `metadata_objects_match_frontend_fragment` runs the metadata boot query twice
  and asserts idempotency, field types,
  label/image identifier links, and workspace isolation.
