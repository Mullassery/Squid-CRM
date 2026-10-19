# Decision Log

| ID | Status | Decision | Context | Consequences |
| --- | --- | --- | --- | --- |
| D-000 | Accepted | Treat the reference as read-only; never modify `reference/twenty`; work from a pinned commit. | Upstream moves fast; must preserve provenance. | All parity claims tied to `08a0e5f1ee2e7db7c1e6ef3aeec9bcef731f4e98`. |
| D-001 | **OPEN — needs owner input** | Licence & derivation strategy for the Rust backend. | LICENSE = AGPL-3.0 + Application Exception + 570 Enterprise files + MIT packages. A backend port is a modified version ⇒ AGPL §5/§13 obligations; Enterprise files cannot be ported without a subscription. | Blocks Phase 2 implementation. Options: A) clean-room AGPL backend, Enterprise features excluded (recommended); B) reference-only reimplementation with hard firewall + legal review. |
| D-002 | Proposed | Stack: stable Rust + Tokio + Axum + SQLx + Postgres + `async-graphql` + `utoipa`; alternatives only via ADR. | Brief mandates this unless evidence says otherwise. | Defined in Phase 3. |
| D-003 | Proposed | Keep React/TypeScript frontend unchanged; adapt only the API client config points (`/graphql`, `/rest`, cookie auth) to point at Rust. | Minimises surface; frontend is MIT/AGPL mix but not rewritten. | Frontend stays independently deployable. |
| D-004 | Proposed | Multi-tenancy: replicate schema-per-workspace + core schema; enforce tenant context in one choke point; deny-by-default. | Matches upstream model; strongest isolation. | Requires dynamic SQLx-generated per-schema queries. |
| D-005 | Proposed | Dynamic GraphQL/REST schemas generated from metadata, not hand-written. | Upstream schema is runtime-generated; static schema cannot reach parity. | Larger Phase 4 spike; `async-graphql` dynamic API evaluated. |
| D-006 | Proposed | Enterprise-gated features (SSO/SAML/OIDC provisioning, billing, some admin) are **out of scope** unless a commercial licence is obtained. | Cannot legally port `@license Enterprise` files. | Parity matrix marks these "excluded (licence)". |
