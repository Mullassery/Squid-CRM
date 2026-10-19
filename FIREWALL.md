# Reference Firewall

Per decision **D-001**, `reference/twenty` is a **specification only**.

## Rules

1. Do **not** copy, paste, transliterate, or mechanically translate source code
   from `reference/twenty` into `crates/`.
2. Do **not** vendor upstream files, including generated GraphQL/OpenAPI
   artifacts, unless they are treated as captured test fixtures under
   `contracts/` and clearly labelled with their source commit.
3. Do **not** port any file marked `/* @license Enterprise */`. Those features
   are excluded (`docs/migration-matrix.md`).
4. Derive behaviour from documented interfaces and observable contracts
   (HTTP shapes, status codes, cookie semantics, error names, pagination), not
   from implementation internals.
5. Every Rust module that implements an upstream concept must cite the concept
   (not the code) in a comment or the API compatibility doc.
6. Preserve `LICENSE` and `NOTICE`. Squid-CRM remains AGPL-3.0.

## Review checklist for PRs

- [ ] No code copied from upstream (diff reviewed by a second person).
- [ ] Contract changes reflected in `docs/api-compatibility.md`.
- [ ] No Enterprise-only behaviour implemented.
- [ ] Tests added/updated; nothing skipped.
- [ ] Licence/attribution intact.

## Enforcement

A CI check (`scripts/check-firewall.sh`) fails if Rust sources contain markers
that suggest mechanical copying (e.g. upstream import paths or distinctive
upstream identifiers). This is a heuristic, not a substitute for review.

Outside IP counsel review remains a release gate (D-001).
