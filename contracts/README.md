# Captured API contracts

This directory is for **golden fixtures captured from the pinned upstream**
(`twentyhq/twenty@08a0e5f1`): GraphQL SDL, the generated OpenAPI document, and
example request/response pairs for a seeded workspace.

## Status

**Not yet captured.** Booting the reference stack requires the full upstream
dependency install and a working Docker daemon; neither was available during the
Phase 4 increment. Current compatibility assertions are derived from source
inspection and are documented in `docs/api-compatibility.md`.

## When capturing

Store fixtures with explicit provenance:

```
contracts/
  <commit>/
    graphql-schema.graphql
    openapi.json
    README.md          # exact boot steps + seed data
    examples/
      rest-list-people.json
      gql-get-login-token.json
```

Never treat these fixtures as source to copy into `crates/` (see `FIREWALL.md`);
they are test oracles only.
