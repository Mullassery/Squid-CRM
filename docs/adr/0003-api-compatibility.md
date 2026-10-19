# ADR 0003 — API compatibility strategy

- Status: Accepted (Phase 2/3)
- Date: 2026-10-19

## Context

The React/TypeScript frontend is kept initially and is tightly coupled to the
server's API: Apollo Client posts to `/graphql`, REST helpers use `/rest`, and
auth relies on a `twenty-session` HttpOnly cookie plus bearer tokens. The
server's GraphQL schema is generated at runtime from workspace metadata.

## Decision

1. **Compatibility boundary = the audited HTTP contract**, not internal module
   structure. We reproduce prefixes, envelopes, error names/codes and cookie
   semantics.
2. **No silent divergence.** Every implemented endpoint is documented in
   `docs/api-compatibility.md`; unimplemented behaviour is listed as a gap.
3. **Golden contracts first.** Where possible, capture SDL/OpenAPI/example
   responses from the pinned upstream and store them under `contracts/`. (Not
   yet captured in this environment — tracked as a TODO; current assertions are
   source-derived.)
4. **Frontend adaptation is limited to configuration** initially:
   `REACT_APP_SERVER_BASE_URL` points at the Rust server. Any deeper adapter is
   documented as a compatibility change.
5. **Dynamic schema direction.** Phase 5 will generate GraphQL/REST schemas from
   workspace metadata; the Phase 4 static person schema is a temporary slice,
   not the final design.

## Consequences

- The frontend can be tested against Rust independently of the legacy server.
- Partial parity is explicit; consumers can see which endpoints are safe.
- The static slice will be replaced by a metadata-driven schema, so code should
  not assume hand-written resolvers forever.

## Verification

- `cargo test` integration suite asserts the REST envelopes and auth flow.
- `scripts/smoke.sh` exercises the live binary.
- Contract capture from upstream is the next compatibility milestone.
