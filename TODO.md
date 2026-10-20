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
- [x] 15 unit + 19 integration tests passing; clippy clean; live smoke test.
- [x] Dockerfile, docker-compose, CI workflow, Makefile, smoke script.

## Phase 2 (contracts)
- [x] Boot upstream via Docker (`twentycrm/twenty:v2.49.0`) and capture SDL,
      OpenAPI and REST/GraphQL examples into `contracts/v2.49.0/`.
- [x] Diff captured contracts against `docs/api-compatibility.md` (divergence
      table §8).
- [ ] Re-capture at `v2.50.0` (no image published at capture time) or build the
      pinned source and capture from it.
- [ ] Map every frontend operation in `twenty-front/src/generated/graphql.ts`.

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
- [x] Person filter grammar (`and`/`or`/`not`, scalar/composite fields) for REST and GraphQL.
- [ ] Remaining pagination (`first`/`last`/`before`/`after`), `fields`, `depth`,
      `viewId` with upstream semantics.
- [ ] Tighten the data API to API keys only (access tokens work today).
- [ ] Refresh-token rotation and access-token revocation.
- [ ] CSRF protection decision for cookie auth.
- [ ] Rate limiting on auth endpoints.
- [x] Person fields: phones, linkedinLink, avatarFile (JSON), companyId, actors.
- [x] Person update (`PATCH /rest/people/{id}`, GraphQL `updatePerson`).
- [x] Person batch mutations (`createPeople`, `updatePeople`, `deletePeople`, `destroyPeople`, `restorePeople`).
- [x] Soft/hard delete + restore (`soft_delete`, `restorePerson`, `destroyPerson`).
- [ ] `mergePeople`, `Person.avatarFile` as `FileObject`, null-clearing updates, full metadata fields.
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
- [ ] `createManyViewFields`/`createManyViewFilters`/`createManyViewSorts`/
      `createManyViewGroups`, view field groups/filter groups writes, roles,
      indexes/relations, remaining
      standard objects, and metadata-driven dynamic schema generation.
- [ ] Custom-object physical tables and dynamic GraphQL/REST schema/relation
      field creation (metadata writes currently persist metadata only).
- [x] `currentUser`/`currentWorkspace` full frontend boot fragment (ADR 0005;
      enterprise surfaces are explicit empty/static defaults).
- [ ] Role/permission engine, billing/SSO data, feature flags, and
      `ApiKey.role` as the upstream `Role` object.
- [x] REST target routes, target updates/batches, and nested target record resolution (`TaskTarget.targetCompany` etc.).
- [x] Scalar ordering parity (`orderBy` + REST `order_by`) for company,
      opportunity, task, note and task/note targets; relation ordering pending.
- [x] REST filter grammar (`eq`/`neq`/`in`/`contains`/`startsWith`/`endsWith`/
      `like`/`ilike`/ranges/`is`, `and`/`or`/`not`) for company, opportunity,
      task, note and task/note targets.
- [ ] Views parity: batch create mutations, view field/filter group writes,
      relation filters and view-based paging semantics.
- [ ] Search, files, export/import.
- [ ] Jobs/cron, webhooks, workflows, integrations.

## Phase 6-8
- [ ] Threat model (`docs/threat-model.md`).
- [ ] Security tests + fuzz/property tests for authz and parsers.
- [ ] Baseline benchmarks on the reference; then Rust comparison (no fabricated numbers).
- [ ] SBOM + `cargo audit` + `cargo deny` in CI.
- [ ] Release docs and known-limitations file.

## Known limitations (current)
- Static schema for the five standard objects; metadata writes persist rows but
  do not generate physical tables or dynamic GraphQL/REST types for custom
  objects (relation field creation is rejected).
- Cursor pagination, `order_by`, and scalar/composite filters implemented for
  whitelisted person columns; relation filters and view-based paging are not.
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
- Data API accepts access tokens in addition to API keys (upstream requires API
  keys).
- No refresh rotation, OAuth, 2FA; API keys implemented but data API still
  accepts access tokens. API-key `roleId` is ignored until metadata roles land.
- Enterprise features excluded (licence).
- No baseline performance numbers captured; no performance claims made.
- `reference/twenty` is git-ignored; recreate with `scripts/clone-reference.sh`.
