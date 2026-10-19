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
- [x] 14 unit + 9 integration tests passing; clippy clean; live smoke test.
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
- [ ] Full `filter` grammar (`and`/`or`/`not`, composite fields).
- [ ] Remaining pagination (`first`/`last`/`before`/`after`), `fields`, `depth`,
      `viewId` with upstream semantics.
- [ ] Tighten the data API to API keys only (access tokens work today).
- [ ] Refresh-token rotation and access-token revocation.
- [ ] CSRF protection decision for cookie auth.
- [ ] Rate limiting on auth endpoints.
- [x] Person fields: phones, linkedinLink, avatarFile (JSON), companyId, actors.
- [ ] Person relations, `avatarFile` as `FileObject`, full metadata fields.
- [ ] Metadata API so the real frontend can load object/field definitions.
- [x] `client-config` endpoint.

## Phase 5 (module migration)
- [ ] Metadata engine (object/field/view/role) and dynamic schema generation.
- [ ] Companies, opportunities, tasks, notes.
- [ ] Views/filters/sorts/groups parity.
- [ ] Search, files, export/import.
- [ ] Jobs/cron, webhooks, workflows, integrations.

## Phase 6-8
- [ ] Threat model (`docs/threat-model.md`).
- [ ] Security tests + fuzz/property tests for authz and parsers.
- [ ] Baseline benchmarks on the reference; then Rust comparison (no fabricated numbers).
- [ ] SBOM + `cargo audit` + `cargo deny` in CI.
- [ ] Release docs and known-limitations file.

## Known limitations (current)
- Static person schema; not metadata-driven.
- Cursor pagination + `order_by` implemented for whitelisted person columns;
  full filter grammar and view-based paging are not.
- No OpenAPI document emitted yet.
- No refresh rotation, OAuth, 2FA; API keys implemented but data API still
  accepts access tokens. API-key `roleId` is ignored until metadata roles land.
- Enterprise features excluded (licence).
- No baseline performance numbers captured; no performance claims made.
- `reference/twenty` is git-ignored; recreate with `scripts/clone-reference.sh`.
