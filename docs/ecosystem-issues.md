# Ecosystem repository issue register

Audit snapshot for the Mullassery MarTech/CDP repositories. This file records
observed issues only; it does not claim that a repository is otherwise healthy.
Re-check current CI and `git status` before acting because this snapshot ages
quickly.

Status labels: **OPEN**, **FIXED IN WORKING TREE**, **BLOCKED**, **UNVERIFIED**,
**NOT APPLICABLE**.

## 1. Ecosystem repositories

| Repository | Role | Status | Issues found |
| --- | --- | --- | --- |
| Squid-CRM | CRM system of record, REST/GraphQL API, webhooks | OPEN / FIXED IN WORKING TREE | See section 2. |
| PyReverseETL | Warehouse-to-CRM/marketing activation | CODE-SIDE RESOLVED, LIVE PENDING | Native Squid adapter now posts native person/company payloads with a stable `Idempotency-Key` and `X-Correlation-Id`, reusing the key on retries. Mock-tested; no live Squid account was used. Standard-object creates are create-only. Encryption compliance action remains unresolved. |
| PyCustomerHub | Event ingestion, CDP, profile/audience pipeline | CODE-SIDE RESOLVED, LIVE PENDING | Signed `POST /v1/squid` adapter maps envelope v1 to OCDS, registers `squid.*` schemas, maps workspace→tenant, and verifies HMAC. Edge write-key auth is implemented. No live PyCustomerHub stack was used. |
| PyTagShield | Consent-aware event validation, pseudonymization, routing | CODE-SIDE RESOLVED, LIVE PENDING | Runnable Axum server and event-only consent contract (`pytagshield.consent.v1`) are implemented. Squid is not a consent authority and records do not carry consent columns. No live Squid test yet. |
| PyTagManager | GTM/tag diagnosis and structured findings | PARTIAL | Versioned `tagManagerFinding` schema and write plan are defined. Squid metadata acceptance and live write test remain. |
| ClusterAudienceKit | Segmentation, scoring, streaming audiences | PARTIAL | Versioned `audienceSegments` / `audienceSegmentMemberships` schemas and a batched lookup/upsert plan are defined. No HTTP client; Squid acceptance and live write test remain. |
| QuillSift | Rust fetching, parsing, schema-validated extraction | NOT APPLICABLE | No direct CRM contract. Scraped output would have to pass through another ecosystem service before reaching Squid. |
| MudFish | Rust crawling and browser-rendered web intelligence | NOT APPLICABLE | No direct CRM contract. Crawled output would have to pass through another ecosystem service before reaching Squid. |

## 2. Squid-CRM issues

| Issue | Status | Evidence |
| --- | --- | --- |
| CI smoke test fails because `scripts/smoke.sh` uses `mktemp -t squid-smoke` without an `XXXXXX` template. | FIXED IN WORKING TREE | GNU `mktemp` reports `too few X's in template 'squid-smoke'`. |
| REST writes are not idempotent; retrying a create can duplicate records. | FIXED IN WORKING TREE | No `Idempotency-Key` handling or deduplication table at audit time. |
| Webhook delivery is fire-and-forget with a single attempt. | FIXED IN WORKING TREE | `emit_webhooks` now retries with bounded backoff and records attempts in `core.webhook_deliveries`. |
| Webhook failures are not observable and there is no dead-letter queue. | PARTIAL | Attempts, last status/error, terminal state and replayable bodies are stored in `core.webhook_deliveries`; admin listing/manual replay and a background retry worker exist; ordering and retention policy remain open. |
| No shared event-schema contract with PyCustomerHub. | CODE-SIDE RESOLVED | PyCustomerHub now accepts signed Squid envelope v1; live test pending. |
| No PyReverseETL Squid adapter or mutually verified payload contract. | CODE-SIDE RESOLVED | PyReverseETL now sends native Squid payloads with `Idempotency-Key`; live test pending. |
| README and About text describe Twenty API compatibility. | OPEN | Branding removal must preserve AGPL `NOTICE`/`LICENSE` attribution and generated fixture provenance. |
| No branch protection and vulnerability alerts disabled on GitHub. | OPEN | Repository settings, not code. |
| REST route map reports 493 of 535 captured core routes missing. | OPEN | `docs/rest-route-map.md`; this is the upstream compatibility surface, not the ecosystem slice. |
| Custom objects have REST CRUD but no generated GraphQL types. | OPEN | Documented limitation in `docs/api-compatibility.md`. |
| Files are stored in PostgreSQL; S3 storage is absent. | OPEN | Documented limitation. |
| Jobs/cron/workflows/integrations are not implemented. | OPEN | Documented limitation. |
| Working-tree changes from prior work were uncommitted. | OPEN | Envelope, client, examples, tests and docs were modified locally; no commit or push was authorized. |

## 3. CI failures in other repositories

These findings came from the latest workflow runs at audit time. Some may
already be fixed on the default branch. Logs were captured under
`/tmp/ci-logs/` and shallow clones under `/tmp/ci-fix/`.

| Repository | Failure | Likely fix |
| --- | --- | --- |
| `homebrew-tinybridge` | A "Grapify scan" step referenced the nonexistent action `grapify-ai/grapify-action`. The currently checked `bump-formula.yml` did not contain that step, so the failure may be from an older commit. | Re-check the failing run's commit; remove or replace the action only if it is still present. |
| `PyAPICheck` | `cargo audit` reported 4 vulnerabilities. | Upgrade affected dependencies and re-run `cargo audit`. |
| `PyDependencyCheck` | `cargo audit` reported 4 vulnerabilities. | Upgrade affected dependencies and re-run `cargo audit`. |
| `PyBeamGuard` | Test expectation failure in `test_analyze_missing_file_is_clean_error`. | Update the stale assertion to the current clean-error contract. |
| `PyContextFold` | MSRV declared 1.75 but code uses a stable 1.82 item. | Raise `rust-version` to the actual minimum and align CI. |
| `PyDeepMesh` | `mypy` reports a missing positional argument. | Correct the call or annotation. |
| `PyInferenceManager` | Flaky latency test asserts `avg_latency_ms > 0` while a mock returns zero duration. | Make the clock/assertion deterministic. |
| `PyRoboSimulator` | Docker workflow references missing `workload_identity_provider`/`credentials_json`. | Guard the GCP login step when secrets are absent, or remove the workflow path. |
| `PyStreamMCP` | Dependabot pyo3 0.23 security update fails; pyo3 >= 0.24.1 is required. | Upgrade pyo3 and fix API breakage. |
| `SHER-Aurora` | gtk4 API mismatch around `title`, `is_modal`, `child`. | Align with the installed gtk4 version/prelude. |
| `SHER-KERNEL` | Miri reports memory leaks (259). | Investigate or, if intentional, use a narrowly documented `-Zmiri-ignore-leaks` policy. |
| `SHER-Process-Explorer` | egui trait method `ui` incompatible. | Update to the matching egui trait signature. |
| `StatGuardian` | Ruff reports 3 errors. | Apply the specific lint fixes. |
| `PyTerrainMap` | Latest overall run was cancelled on a Dependabot PR; latest `main` run succeeded. | Not a main-branch failure; re-run the dependency PR after updating it. |
| `PyTagShield` | No workflows and no GitHub README. | Add CI and a README only after the server contract is real. |

## 4. GitHub setup issues across all 52 repositories

- No branch protection was enabled on any audited repository.
- Wiki and projects features were enabled by default on repositories that do
  not use them.
- Many repositories have no releases or tags despite being distributed.
- Vulnerability alerts were disabled or unavailable for `homebrew-formulas`,
  `Mullassery`, `PyTagShield`, `QuillSift` and `Squid-CRM`.
- `CodeAtlas`, `PyTaskWireCheck` and `SyncFleet` use MIT licenses while the
  other repositories use Apache-2.0; this may be intentional but is a portfolio
  inconsistency.
- `PyRoboReplay` and `PyWeatherEnriched` use `master` while the rest use `main`.
- Topics were missing on `homebrew-formulas`, `Mullassery` and `TinyBridge-UI`.
- `PyTagShield` had no README and no workflows on GitHub.
- Several repositories had low community-health scores because they lack
  `SECURITY.md`, `CONTRIBUTING.md`, issue templates or code of conduct.

## 5. Cross-repository blockers

1. **PyCustomerHub:** code-side resolved. Signed envelope-v1 ingestion, OCDS
   mapping, `squid.*` schemas, workspace→tenant mapping and edge write-key auth
   are implemented; live verification pending.
2. **PyReverseETL:** code-side resolved. Native Squid adapter sends stable
   `Idempotency-Key` and native person/company payloads; live verification
   pending. Standard-object creates are create-only.
3. **PyTagShield:** code-side resolved. Runnable server and event-only
   `pytagshield.consent.v1` contract; Squid records do not carry consent
   columns. Live verification pending.
4. **ClusterAudienceKit/PyTagManager:** partial. Both counterpart schemas and
   write plans are defined; Squid metadata acceptance and live write tests
   remain.

## 6. Verification notes

- This register distinguishes code findings from GitHub-settings findings and
  from failures observed on old commits.
- No sibling repository was modified while producing the audit.
- No test result is reported as passing unless it was executed and observed.
