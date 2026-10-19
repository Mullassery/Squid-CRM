# TODO

## Blocking
- [ ] **D-001** — choose licence/derivation strategy (clean-room AGPL vs reference-only + legal review). Owner input required before Phase 2 code.

## Phase 2 (next)
- [ ] Write `docs/api-compatibility.md` (transport, auth, errors, pagination/sort/filter, idempotency, retry).
- [ ] Add contract-capture script that boots reference stack and dumps GraphQL SDL + OpenAPI + example responses at the pinned commit.
- [ ] Generate `contracts/` golden fixtures and commit with commit-hash metadata.
- [ ] Map every operation in `twenty-front/src/generated/graphql.ts` to a backend resolver contract.

## Phase 3
- [ ] ADRs: async-graphql vs Juniper; SQLx vs alternative; dynamic schema approach; tenant context design.
- [ ] `cargo workspace` skeleton compiling with `cargo check`.
- [ ] Architecture diagrams (Mermaid) in `docs/architecture/`.

## Phase 4
- [ ] Person vertical slice (REST + GraphQL) with auth + tenant isolation.
- [ ] First failing test: workspace A cannot read workspace B data.
- [ ] Frontend smoke test against Rust backend.

## Cross-cutting
- [ ] `docs/threat-model.md` (Phase 6).
- [ ] `docs/benchmark-methodology.md` + baseline on reference (Phase 7).
- [ ] SBOM / dependency audit guidance (Phase 8).
- [ ] CI workflows (Phase 7).

## Known limitations (current)
- No Rust code exists yet; no baseline benchmarks captured; Enterprise features excluded pending licence decision.
