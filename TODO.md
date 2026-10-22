# TODO

## Done in the current increment (Phase 4 slice)
- [x] D-001 recorded: reference-only + firewall + legal review.
- [x] D-007 recorded: slice = Person + auth + workspace bootstrap.
- [x] Rust workspace (core/db/auth/api/server) compiles.
- [x] Core + per-workspace migrations with advisory-lock safety.
- [x] Auth: signup/signin/signout, Argon2id, JWT, hashed session cookies, bearer.
- [x] GraphQL login flow + person query/mutations.
- [x] REST people list/get/create/delete with upstream envelope + error contract.
- [x] Tenant isolation enforced and tested against real PostgreSQL.
- [x] 57 tests passing (13 API unit, 37 integration, 5 auth, 2 DB); clippy clean; live smoke test.
- [x] Dockerfile, docker-compose, CI workflow, Makefile, smoke script.

## Phase 2 (contracts)
- [x] Boot upstream via Docker (`twentycrm/twenty:v2.49.0`) and capture SDL,
      OpenAPI and REST/GraphQL examples into `contracts/v2.49.0/`.
- [x] Diff captured contracts against `docs/api-compatibility.md` (divergence
      table §8).
- [x] Build the pinned source (`--target twenty-server`), capture v2.50.0 SDL,
      OpenAPI and examples into `contracts/v2.50.0/`, and diff against v2.49.0.
- [x] Map every frontend operation in `twenty-front/src/generated/graphql.ts`
      and `generated-metadata/graphql.ts` (`docs/frontend-operation-map.md`).
- [x] Map every captured REST/OpenAPI route (`docs/rest-route-map.md`).

## Phase 4 follow-ups (contract alignment — from captured fixtures)
- [x] REST create returns `data.createPerson`; delete returns
      `data.deletePerson.id`.
- [x] Unauthenticated REST is `403 FORBIDDEN_EXCEPTION` (no `code`); 404 has no
      `code`.
- [x] Cursors = base64 `{"id":"<uuid>"}`; support `starting_after` /
      `ending_before`; `hasPreviousPage` only for cursor paging.
- [x] `additionalEmails`/`additionalPhones` are `[]`, never `null`.
- [x] GraphQL `people` returns `PersonConnection` (edges/cursor/pageInfo);
      `person(filter:)`, `createPerson(data:)`.
- [x] `order_by=field[DescNullsLast],...` grammar (REST + GraphQL); `-createdAt`
      rejected.
- [x] Serve auth mutations on `/metadata` and add API-key lifecycle for the data API.
- [x] Person filter grammar (`and`/`or`/`not`, scalar/composite fields,
      `companyId` scalar filter) for REST and GraphQL.
- [x] GraphQL pagination (`first`/`last`/`offset`/`before`/`after`) and
      `pageInfo` for connections.
- [x] REST `fields`/`depth` projection and validation for people, companies,
      opportunities, tasks and notes.
- [x] REST `groupBy` for people/companies/opportunities/tasks/notes and
      task/note targets: raw-array response, `group_by`, `aggregate`,
      `include_records_sample`, `order_by_for_records`, `limit` (0 = all) and
      `viewId` filters (camelCase, ANDed with explicit `filter`).
- [x] Tighten the data API to API keys (or the session cookie); user access
      tokens are rejected on `/graphql` and `/rest`.
- [x] Opaque refresh-token rotation, replay rejection, and access-token
      revocation via the session `sid` on signout.
- [x] CSRF decision: SameSite=Lax HttpOnly cookie + Origin check on unsafe
      cookie-authenticated requests; bearer/API-key requests are exempt.
- [x] In-process burst limiting on signup/signin (10 attempts / 15 minutes per
      account), with a clear extension point for edge/multi-replica limits.
- [x] Person fields: phones, linkedinLink, avatar file, companyId, actors.
- [x] Person update (`PATCH /rest/people/{id}`, GraphQL `updatePerson`).
- [x] Person batch mutations (`createPeople`, `updatePeople`, `deletePeople`, `destroyPeople`, `restorePeople`).
- [x] Soft/hard delete + restore (`soft_delete`, `restorePerson`, `destroyPerson`).
- [x] `mergePeople` with conflict priority, dry run, relation re-pointing and soft delete.
- [x] Null-clearing updates for person, company, opportunity, task, note and
      task/note targets (REST + GraphQL); omitted fields stay unchanged.
- [x] `Person.avatarFile` typed as `[FileObject]`.
- [x] Null-clearing for view settings and view child records.
- [x] Metadata indexes (`createOneIndex`/`deleteOneIndex`,
      `Object.indexMetadataList`), basic `getRoles`/`getRole`/
      `getPermissionFlags` and `mostlyEmptyFieldMetadataIds`.
- [x] Search-field metadata (`Object.searchFieldMetadataList`) and standard
      relation metadata (`Field.relation` for the seeded relationship fields),
      including reciprocal `company.people`/`company.opportunities` and
      `person.pointOfContactForOpportunities` fields.
- [x] Task/note target metadata objects, polymorphic target reverse fields
      (`task.taskTargets`, `note.noteTargets`, company/person/opportunity
      target collections) and target relation metadata.
- [x] Full metadata field coverage: morph metadata (`target` head plus
      `targetCompany`/`targetOpportunity`/`targetPerson`) and workspace-member
      reverse fields (`assignedTasks`, `accountOwnerForCompanies`).
- [x] `client-config` endpoint.

## Phase 5 (module migration)
- [x] Metadata storage (`core.object_metadata` / `core.field_metadata`) and read
      API for the standard person object/fields (frontend boot fragment passes).
- [x] Default person view and `getViews`/`getView` read API matching the
      frontend `ViewFragment`.
- [x] Company object slice: workspace `0003_company.sql`, seeded metadata +
      default view, REST `/rest/companies` CRUD, GraphQL
      `companies`/`company`/`createCompany`/`updateCompany`/`deleteCompany`/
      `destroyCompany`/`restoreCompany`.
- [x] Opportunity object slice: workspace `0004_opportunity.sql`, seeded
      metadata + default view, REST `/rest/opportunities` CRUD, GraphQL
      `opportunities`/`opportunity`/CRUD with stage/name/company filters.
- [x] Task and Note slices: workspace `0005_task.sql` / `0006_note.sql`, seeded
      metadata + default views, REST CRUD and GraphQL CRUD with status/title
      filters; `RichText` body support.
- [x] Task/note targets: `task_target`/`note_target` migrations, GraphQL
      `taskTargets`/`noteTargets` connections with `totalCount`, root queries,
      create/update/delete/destroy/restore single and batch mutations, nested
      target records, and REST `/rest/taskTargets` + `/rest/noteTargets` CRUD.
- [x] Metadata object/field writes (`createOneObject`/`updateOneObject`/
      `deleteOneObject`, `createOneField`/`updateOneField`/`deleteOneField`) with
      system guards; seeded default `name` field for custom objects.
- [x] View writes: `createView`/`updateView`/`deleteView`/`destroyView` plus
      view field/filter/sort/group create/update/delete/destroy and
      `getViewFields`/`getViewFilters`/`getViewSorts`/`getViewGroups` queries.
- [x] View batch/group writes: `createManyViewFields`, `createManyViewGroups`,
      `updateManyViewGroups`, `createManyViewFieldGroups`, view field group CRUD,
      view filter group CRUD and `getViewFieldGroups`/`getViewFilterGroups`.
- [ ] Role/permission assignment engine and metadata-driven dynamic schema
      generation (workspace-member metadata is seeded).
- [ ] Custom-object physical tables and dynamic GraphQL/REST schema/relation
      field creation (metadata writes currently persist metadata only).
- [x] `currentUser`/`currentWorkspace` full frontend boot fragment (ADR 0005;
      enterprise surfaces are explicit empty/static defaults).
- [x] `workspaceMembers`/`workspaceMember` GraphQL queries with filters,
      ordering and pageInfo; boot `workspaceMembers` now lists every member.
- [ ] Role/permission enforcement, billing/SSO data, feature flags, and
      `ApiKey.role` as the upstream `Role` object (built-in role queries exist).
- [x] REST target routes, target updates/batches, and nested target record resolution (`TaskTarget.targetCompany` etc.).
- [x] Scalar ordering parity (`orderBy` + REST `order_by`) for company,
      opportunity, task, note and task/note targets; relation ordering pending.
- [x] REST filter grammar (`eq`/`neq`/`in`/`contains`/`startsWith`/`endsWith`/
      `like`/`ilike`/ranges/`is`, `and`/`or`/`not`) for company, opportunity,
      task, note and task/note targets.
- [ ] Views parity: relation filters and view-based paging semantics.
- [ ] Search, export/import; local file upload exists for attachments/avatars
      but no object-storage backend.
- [ ] Jobs/cron, webhooks, workflows, integrations.

## Phase 6-8
- [ ] Threat model (`docs/threat-model.md`).
- [ ] Security tests + fuzz/property tests for authz and parsers.
- [ ] Baseline benchmarks on the reference; then Rust comparison (no fabricated numbers).
- [ ] SBOM + `cargo audit` + `cargo deny` in CI.
- [ ] Release docs and known-limitations file.

## Known limitations (current)
- Static schema for the eight seeded standard objects (including task/note
  targets and workspaceMember); metadata writes persist rows but do not
  generate physical tables or dynamic GraphQL/REST types for custom objects
  (relation field creation is rejected).
- Cursor pagination, `order_by`, and scalar/composite filters implemented for
  whitelisted person columns including `companyId`; nested relation filters and
  view-based paging are not.
- `WorkspaceMember` queries expose the core profile fields; position/calendar
  and date/time/number-format filters and connection aggregates are absent.
- Company GraphQL filters are limited to `id`/`name`; REST filters cover scalar
  fields. Relation filters and nested relation ordering are not implemented.
- Opportunity relation filters are not implemented; scalar REST filters and
  ordering are applied. GraphQL scalar filter coverage matches the object slice.
- Task/Note target REST filters and ordering cover scalar fields.
- Task/Note GraphQL and REST filters cover scalar fields
  (id/title/status/dueAt/assigneeId); relation filters and nested ordering are not.
- No OpenAPI document emitted yet.
- Workspace activation lifecycle (`PENDING_CREATION` / `activateWorkspace`) and
  `signUpInNewWorkspace` are not implemented; `/auth/signup` activates directly.
- No OAuth, 2FA or app-token flows. API-key `roleId` is ignored until metadata
  roles land; auth throttling is per-process only.
- `groupBy` is evaluated in memory per object slice (up to the repository
  fetch cap for non-person objects); date buckets support the captured
  granularities, week starts on Monday.
- Morph metadata is modelled in metadata only: `taskTarget`/`noteTarget`
  expose a `target` morph head and `targetCompany`/`targetOpportunity`/
  `targetPerson` morph relations, while records still use separate `*Id`
  columns. `Field.relation` is populated for seeded forward relations and
  reciprocal company/person/task/note target relation fields, including
  workspace-member reverse fields. Index metadata is stored but does not create
  physical PostgreSQL indexes.
- File uploads are stored in PostgreSQL (`core.files`) with an unauthenticated
  signed-UUID upload/download URL; S3-compatible storage and stronger upload
  authorization remain open.
- Enterprise features excluded (licence).
- No baseline performance numbers captured; no performance claims made.
- `reference/twenty` is git-ignored; recreate with `scripts/clone-reference.sh`.
