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
| PyReverseETL | Warehouse-to-CRM/marketing activation | OPEN | No Squid-CRM connector. Its generic webhook destination posts one record object at a time and cannot add an `Idempotency-Key` header, so it does not match Squid's REST create envelope and cannot use Squid's idempotent write path. CRM adapters are mock-verified only, not live-account verified. Encryption compliance action is reported unresolved. Python connector plugins do not run in the Rust core path. |
| MessageBirds | Event ingestion, CDP, profile/audience pipeline | OPEN | No Squid-CRM ingestion adapter. Squid's webhook envelope is not the MessageBirds/OCDS envelope: it lacks `event_id`, `event_type`, `timestamp`, `source`, `identity`, `schema`, `tenant_id`, `correlation_id`. MessageBirds `POST /events` requires a structurally valid OCDS envelope and then a registered event schema, so Squid webhooks cannot be sent to it unchanged. No batch event endpoint. Edge ingestion gateway is not deployed and has no write-key auth. |
| PyTagShield | Consent-aware event validation, pseudonymization, routing | UNVERIFIED / OPEN | GitHub repository had no README and no workflows at audit time. The server binary was previously found to be a no-op (`fn main() {}`), so a runnable HTTP service contract is not evidenced. No Squid-CRM destination adapter. No executable evidence that consent decisions are attached to outbound CRM records. |
| PyTagManager | GTM/tag diagnosis and structured findings | OPEN | No direct Squid-CRM adapter or agreed findings schema/version. The improved README and `pyproject.toml` description were uncommitted locally at audit time. GitHub Actions showed no recent run for its configured workflows, so CI status was unverified. |
| ClusterAudienceKit | Segmentation, scoring, streaming audiences | OPEN | No direct Squid-CRM adapter. Audience activation is expected to pass through PyReverseETL, but no Squid payload mapping or custom-object schema contract is defined. |
| ScrapeOwl | Rust fetching, parsing, schema-validated extraction | NOT APPLICABLE | No direct CRM contract. Scraped output would have to pass through another ecosystem service before reaching Squid. |
| MudFish | Rust crawling and browser-rendered web intelligence | NOT APPLICABLE | No direct CRM contract. Crawled output would have to pass through another ecosystem service before reaching Squid. |

## 2. Squid-CRM issues

| Issue | Status | Evidence |
| --- | --- | --- |
| CI smoke test fails because `scripts/smoke.sh` uses `mktemp -t squid-smoke` without an `XXXXXX` template. | FIXED IN WORKING TREE | GNU `mktemp` reports `too few X's in template 'squid-smoke'`. |
| REST writes are not idempotent; retrying a create can duplicate records. | FIXED IN WORKING TREE | No `Idempotency-Key` handling or deduplication table at audit time. |
| Webhook delivery is fire-and-forget with a single attempt. | FIXED IN WORKING TREE | `emit_webhooks` spawned one request with a 5-second timeout and ignored the result. |
| Webhook failures are not observable and there is no dead-letter queue. | OPEN | Failures were discarded; retries and delivery logs are being added, but durable DLQ storage remains absent. |
| No shared event-schema contract with MessageBirds. | BLOCKED | Squid envelope differs from MessageBirds/OCDS; a counterpart mapping decision is required. |
| No PyReverseETL Squid adapter or mutually verified payload contract. | BLOCKED | PyReverseETL webhook payload and header support do not match Squid REST writes. |
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
  `Mullassery`, `PyTagShield`, `ScrapeOwl` and `Squid-CRM`.
- `CodeAtlas`, `PyTaskWireCheck` and `SyncFleet` use MIT licenses while the
  other repositories use Apache-2.0; this may be intentional but is a portfolio
  inconsistency.
- `PyRoboReplay` and `PyWeatherEnriched` use `master` while the rest use `main`.
- Topics were missing on `homebrew-formulas`, `Mullassery` and `TinyBridge-UI`.
- `PyTagShield` had no README and no workflows on GitHub.
- Several repositories had low community-health scores because they lack
  `SECURITY.md`, `CONTRIBUTING.md`, issue templates or code of conduct.

## 5. Cross-repository blockers

1. MessageBirds and Squid must agree on either the OCDS envelope or a documented
   mapping into it, including event type, schema registration, identity,
   tenant and correlation semantics.
2. PyReverseETL and Squid must agree on an activation payload plus an
   idempotency header contract. Squid can accept idempotent REST creates, but
   PyReverseETL cannot currently set the header or emit Squid's per-object body.
3. PyTagShield must define whether consent decisions travel on records, events,
   or both before Squid adds consent metadata.
4. ClusterAudienceKit/PyTagManager need agreed custom-object schemas before
   their output can be stored in Squid without ad-hoc fields.

## 6. Verification notes

- This register distinguishes code findings from GitHub-settings findings and
  from failures observed on old commits.
- No sibling repository was modified while producing the audit.
- No test result is reported as passing unless it was executed and observed.
