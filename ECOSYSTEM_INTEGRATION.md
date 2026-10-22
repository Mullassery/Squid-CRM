# Ecosystem Integration

## A. Repository role and boundaries

`Squid-CRM` is the self-hosted CRM and customer-record system of the Mullassery
MarTech/CDP ecosystem. It owns:

- Customer/company/opportunity/task/note records and custom objects.
- Tenant-scoped REST and GraphQL APIs.
- API-key authentication, workspace members and role capabilities.
- Webhook configuration and outbound record events.
- CSV export/import for the core objects.
- Signed local file upload/download.
- A dependency-free Python client (`python/`, distribution `squid-crm`).

It does **not** own tagging, consent adjudication, segmentation/clustering,
reverse ETL orchestration, event streaming, scraping or crawling. Those remain
with PyTagManager, PyTagShield, ClusterAudienceKit, PyReverseETL, MessageBirds,
ScrapeOwl and MudFish respectively.

No sibling repository was modified. No sibling API is assumed beyond the
documented interfaces reproduced here.

## B. Integration matrix

Statuses: **VERIFIED** (executable evidence in this repository), **PARTIAL**
(interface exists, contract/security gap remains), **MISSING**, **BLOCKED**
(counterpart contract unavailable), **NOT APPLICABLE**.

| Counterpart | Direction | Interface used here | Status | Evidence / gap |
| --- | --- | --- | --- | --- |
| PyReverseETL | inbound activation writes | REST `/rest/...`, GraphQL, custom-object REST | PARTIAL | REST/GraphQL CRUD, API keys and `Idempotency-Key` replay are tested (`crates/squid-api/tests/integration.rs`). No bulk upsert or durable outbox; PyReverseETL cannot currently send the header and its exact payload contract is not available locally. |
| ClusterAudienceKit | inbound audience/custom-object sync | Custom-object REST `/rest/{object}` | PARTIAL | Custom-object tables, relation fields and generic CRUD are tested in `metadata_object_and_field_writes`. No batch/upsert endpoint or audience-specific schema contract. |
| PyTagShield | inbound validated events/records | REST/GraphQL, custom objects | PARTIAL | Record APIs and relation fields exist. Consent enforcement belongs to PyTagShield; Squid does not independently prove consent and must not be treated as a consent authority. |
| PyTagManager | inbound findings storage | Custom-object REST | PARTIAL | Generic custom objects can store findings, but no agreed findings schema/version is available in this repository. |
| MessageBirds | outbound record events | Webhooks | PARTIAL | Envelope v1, HMAC signatures, bounded retries, durable delivery log and manual replay are implemented and tested. No shared event-schema registry or ordering guarantee. |
| ScrapeOwl | none | none | NOT APPLICABLE | No direct data contract; scraping output would arrive through another ecosystem service. |
| MudFish | none | none | NOT APPLICABLE | No direct data contract; crawling output would arrive through another ecosystem service. |
| Any counterpart | Python integration | `python/` `squid_crm` client | VERIFIED (client import) | Import/version checked locally. REST/GraphQL contract tests live in the Rust suite. |

## C. Implemented changes

- Standardized outbound webhook envelope:
  - `id`, `type`, `event` (legacy alias), `schemaVersion`, `workspaceId`,
    `occurredAt`, `data`.
- Added `x-squid-event`, `x-squid-event-id`, `x-squid-schema-version`,
  `x-squid-workspace-id`, `x-squid-signature` (`sha256=<hex>`), and a versioned
  user agent.
- Removed the plaintext `x-webhook-secret` header to avoid leaking stored
  secrets over the wire. Consumers must verify the HMAC signature.
- Added workspace-scoped `Idempotency-Key` replay with body-hash conflict
  detection, response replay headers and retry-after-failure semantics.
- Added `x-correlation-id` propagation/echo, bounded webhook retries with
  backoff and `Retry-After`, a durable `core.webhook_deliveries` log, admin
  delivery listing and manual replay.
- Added `examples/webhook_receiver.py` as a runnable HMAC-verifying receiver.
- Added the Python client package under `python/` and published `squid-crm`
  0.1.0 to PyPI.
- Added CSV export/import and role/data/file permission enforcement in earlier
  increments; see `TODO.md`, `docs/decision-log.md` and
  `docs/migration-matrix.md`.

## D. Contract and version assumptions

- REST list envelope: `{data, totalCount, pageInfo}`.
- REST create/update envelopes use `data.create<Object>` /
  `data.update<Object>`; delete returns `data.delete<Object>.id`.
- Data API auth: API key or session cookie; user access tokens are rejected.
- Webhook schema version: `1.0` (the envelope documented above).
- Webhook signature: HMAC-SHA256 over the exact raw JSON body using the
  configured webhook secret.
- File URLs: HMAC-signed, expiring, with `UPLOAD_FILE`/`DOWNLOAD_FILE` checks at
  issuance.
- Mutating requests may send `Idempotency-Key` (1-255 printable ASCII bytes).
  Keys are scoped to a workspace, the request body is hashed, a completed
  response is replayed with `Idempotency-Replayed: true`, and a reused key with
  a different body returns `409 Conflict`. Keys expire; there is no
  cross-workspace sharing.

## E. Test commands and actual results

```bash
cargo fmt --all -- --check
CARGO_INCREMENTAL=0 cargo clippy --workspace --all-targets -- -D warnings
TEST_DATABASE_URL=postgres://$USER@127.0.0.1:5432/squid_test \
  CARGO_INCREMENTAL=0 cargo test --workspace
TEST_DATABASE_URL=postgres://$USER@127.0.0.1:5432/squid_test \
  ./scripts/smoke.sh
```

Last verified result (2026-10-22):

- `cargo fmt --check`: passed.
- clippy with `-D warnings`: passed.
- tests: **69 passing** (13 API unit, 49 integration, 5 auth, 2 DB).
- `scripts/smoke.sh`: passed signup, API-key auth, REST create/update/list,
  metadata/frontend contract, unauthenticated 403 and API-key revocation.
- Webhook envelope/signature behavior is covered by
  `webhook_delivery_posts_record_events` with a live local HTTP receiver.
- Idempotent replay, conflict, tenant scoping and failed-request retry are
  covered by integration tests.
- Python package import check: `squid_crm.__version__ == "0.1.0"`.

Runnable webhook example:

```bash
SQUID_WEBHOOK_SECRET='the-webhook-secret' \
  python3 examples/webhook_receiver.py
# register a webhook at http://127.0.0.1:8099/hook for person.created
# then create a person through /rest/people or GraphQL createPerson
```

## F. Security, privacy and tenant isolation

- All workspace-scoped queries use a validated schema name derived from the
  workspace UUID; cross-workspace access tests exist.
- API keys are hashed at rest; data API rejects user access tokens.
- File URLs are HMAC-signed and expiring; plaintext file URLs are rejected.
- Webhook secrets are no longer transmitted in headers; consumers verify HMAC.
- Role capabilities and `VIEWS`/file flags are enforced; billing/SSO data is
  licence-excluded.
- Squid stores CRM PII by design. It does not decide consent; PyTagShield or
  another consent authority must gate what is sent to Squid or emitted onward.
- Webhook payloads can contain CRM PII. Consumers must apply their own
  destination-specific minimization and retention policy.
- Known gap: no field-level PII masking in webhook payloads and no consent
  metadata on event envelopes.

## G. Remaining gaps and blockers

- **No counterpart contracts verified live.** Sibling repositories and their
  OpenAPI/GraphQL schemas are not present in this workspace. Adapters must not
  be invented from README claims.
- **Idempotency**: header-bearing mutating requests are replay-safe, but
  counterparts must still be taught to send stable keys, and expired-key
  cleanup/retention is not automated.
- **Webhook delivery durability**: bounded in-process retries, a durable
  `core.webhook_deliveries` log, admin delivery listing and manual replay
  exist; there is no automatic background replay/worker, ordering guarantee or
  long-term retention policy.
- **Event schema governance**: envelope v1 exists, but no shared schema registry
  or compatibility tests with MessageBirds.
- **Consent/PII**: Squid does not emit consent decisions or field-level
  minimization metadata.
- **Custom-object GraphQL**: generic REST CRUD exists; metadata-generated
  GraphQL types are not implemented.
- **S3 storage**: local PostgreSQL file bytes only.
- **Jobs/cron/workflows/integrations**: not implemented.

Required counterpart decisions:

- MessageBirds/PyReverseETL should confirm envelope v1 and signature
  verification, or provide their canonical event schema.
- Reverse-ETL/audience consumers should define idempotency keys and retry
  semantics.
- PyTagShield should define whether consent decisions are attached to records,
  events, or both.

## H. Compatibility and next steps

1. Agree idempotency-key generation and retry semantics with reverse-ETL and
   audience consumers; add scheduled expiry cleanup.
2. Add an automatic background replay worker for failed webhook deliveries;
   keep envelope v1 and the existing manual replay stable.
3. Publish a JSON Schema for webhook envelope v1 and contract-test it against
   MessageBirds when its schema is available.
4. Add batch/upsert endpoints for reverse-ETL and audience sync.
5. Add consent/PII metadata only after PyTagShield defines the contract.
6. Consider generated GraphQL for custom objects separately; do not claim it
   until the static schema is replaced or augmented safely.
