# Cross-Repository Blockers — Squid-CRM

Detailed specification of the four cross-repository integration blockers identified
in `docs/ecosystem-issues.md` (§5) and `ECOSYSTEM_INTEGRATION.md` (§G). Each blocker
records the two sides of the contract, the specific fields/semantics that must agree,
the current state of each side, what "resolved" looks like, and the owner of the
next move.

Reference pin: `twentyhq/twenty@08a0e5f1ee2e7db7c1e6ef3aeec9bcef731f4e98`
(server `2.50.0`).

## 1. PyCustomerHub + Squid — shared event-schema contract

**Status:** BLOCKED (`docs/ecosystem-issues.md:32`)

### 1.1 The two sides

| Aspect | Squid-CRM (outbound) | PyCustomerHub (inbound) |
|--------|---------------------|-------------------------|
| Transport | Webhook outbound (HTTP POST to registered `targetUrl`) | `POST /events` |
| Envelope | Squid envelope v1 (`id`, `type`, `event`, `schemaVersion`, `workspaceId`, `occurredAt`, `data`) | OCDS `EventEnvelope` |
| Required fields | `id`, `type`, `schemaVersion`, `workspaceId`, `occurredAt`, `data` | `tenant_id`, `schema{name,version}`, `identity[]`, `data`, `source{type,name}` |
| Identity | embedded inside `data` (CRM record fields) | top-level `identity[]` array |
| Tenancy | `workspaceId` (UUID) | `tenant_id` |
| Correlation | `x-correlation-id` header + envelope `id` | `correlation_id` (not in captured envelope shape) |
| Schema registration | none (envelope is fixed v1) | required — event schema must be registered before `POST /events` accepts it |
| Auth | HMAC-SHA256 signature (`x-squid-signature`) | API key (write-key auth on edge gateway — not deployed) |
| Batch | single event per webhook delivery | no batch event endpoint |

### 1.2 Evidence

- **Squid:** Envelope v1, HMAC signatures, retries, durable `core.webhook_deliveries` log, admin replay are integration-tested (`webhook_delivery_posts_record_events`). The Python adapter `squid_crm.adapters.to_pycustomerhub_event` maps documented top-level OCDS fields but is **unverified** against a live PyCustomerHub stack (`ECOSYSTEM_INTEGRATION.md:36`).
- **PyCustomerHub:** Public `POST /events` observed at repository commit `e79f235`; its envelope is not compatible with Squid envelope v1 without an explicit mapping layer. No live PyCustomerHub stack was available for testing. No batch endpoint. Edge ingestion gateway is not deployed and has no write-key auth (`docs/ecosystem-issues.md:17`).
- **PyTagManager:** Source inspection found PyCustomerHub's `crates/api/src/routes/events.rs` with `EventEnvelope` fields `tenant_id`, `schema{name,version}`, `identity[]`, `data`, `source{type,name}`. PyTagManager event v1 has no `tenant_id`, `schema` ref, or `identity[]` fields — so PyTagManager → PyCustomerHub also needs an explicit mapping (`PyTagManager/ECOSYSTEM_INTEGRATION.md:79`).

### 1.3 What must be agreed

1. **Envelope choice:** Either adopt the OCDS envelope as Squid's outbound webhook envelope (a breaking change to v1), or define a documented field-by-field mapping from Squid envelope v1 → OCDS `EventEnvelope`.
2. **Event type + schema registration:** Squid's `type` (e.g. `person.created`) must map to a registered PyCustomerHub event schema name/version. Who registers schemas and when is undecided.
3. **Identity semantics:** Squid CRM records carry identity inside `data` (email, phone, external IDs). OCDS requires a top-level `identity[]` array. The mapping must decide which CRM fields become `identity` entries and what namespace each uses.
4. **Tenancy:** Squid `workspaceId` → PyCustomerHub `tenant_id`. The relationship (1:1? 1:many? workspace-scoped tenant?) must be documented.
5. **Correlation:** Squid's `x-correlation-id` + envelope `id` → OCDS correlation semantics.
6. **Auth:** PyCustomerHub edge gateway needs write-key auth before Squid webhooks can be sent to it unchanged.
7. **Batch:** PyCustomerHub has no batch event endpoint; Squid delivers one event per webhook. A batching/buffering decision is needed for high-volume workspaces.

### 1.4 Owner of next move

**PyCustomerHub** must confirm its canonical event schema (or accept Squid envelope v1) and deploy the edge ingestion gateway with write-key auth. Squid cannot unilaterally resolve this.

---

## 2. PyReverseETL + Squid — activation payload + idempotency header

**Status:** BLOCKED (`docs/ecosystem-issues.md:33`)

### 2.1 The two sides

| Aspect | Squid-CRM (destination) | PyReverseETL (source) |
|--------|------------------------|----------------------|
| Transport | REST `/rest/{object}` create, GraphQL `create<Object>`, custom-object REST `/rest/{object}` | Generic webhook destination; CRM adapters (Salesforce/HubSpot/Marketo); Python connector plugins |
| Idempotency | Workspace-scoped `Idempotency-Key` header with body-hash conflict detection, response replay, expiry | Cannot currently set the `Idempotency-Key` header; posts one record object at a time via generic webhook |
| Payload shape | Squid REST create envelope: `data.create<Person>`, `data.create<Company>`, etc. | One record object at a time; no Squid per-object body contract |
| Batch | Custom-object `/rest/{object}/upsert` (idempotent, id-present → update, absent → create) | No bulk upsert or durable outbox |
| Auth | API key or session cookie | Bearer / API key / Basic on webhook destination |
| Retry | Bounded retries, `Retry-After`, durable delivery log | Retry with backoff on transient HTTP failures; idempotency ledger for crash-mid-batch recovery |

### 2.2 Evidence

- **Squid:** Idempotent REST creates are integration-tested (`idempotent_same_key_different_body_conflicts`, `idempotency_key_is_workspace_scoped`). Custom-object batch upsert (`/rest/{object}/upsert`) is integration-tested (D-060). Python client `squid_crm` 0.2.1 on PyPI supports idempotency, correlation, and retry headers (12 tests passing).
- **PyReverseETL:** Real Rust sync engine with Python bindings and CLI. PostgreSQL/MySQL/S3 as source/destination. Webhook destination with Bearer/API Key/Basic auth and JSON payload construction. CRM adapters for Salesforce/HubSpot/Marketo are **mock-verified only** (not live-account verified). Python connector plugins do **not** run in the Rust core path — they run through `run_with_connector`, which does not write the Rust lineage graph. Encryption compliance action is reported as unresolved (`README:72-73`).

### 2.3 What must be agreed

1. **Idempotency-Key header:** PyReverseETL must be taught to generate and send a stable `Idempotency-Key` per record, scoped to a Squid workspace, with body-hash semantics. Without this, retries duplicate CRM records.
2. **Payload contract:** PyReverseETL's per-record JSON must map to Squid's REST create envelope (`data.create<Person>` etc.) or to custom-object `/rest/{object}` create. The exact field allowlist (which person fields, which company fields) must be defined.
3. **Batch vs single-record:** Squid supports custom-object batch upsert; PyReverseETL posts one record at a time. A batching decision (buffer N records then upsert, or stream single records) affects throughput and idempotency key stability.
4. **Durable outbox:** PyReverseETL has an idempotency ledger for crash-mid-batch recovery, but no durable outbox for failed Squid deliveries. The retry/backoff contract must match Squid's bounded webhook retry semantics.
5. **Encryption:** PyReverseETL's `Encrypt` compliance action is unresolved — PII masking before egress to Squid is not guaranteed.

### 2.4 Owner of next move

**PyReverseETL** must add `Idempotency-Key` header support and document its per-record payload shape for Squid destinations. Squid cannot unilaterally resolve this.

---

## 3. PyTagShield + Squid — consent decision contract

**Status:** UNVERIFIED / OPEN (`docs/ecosystem-issues.md:18`)

### 3.1 The two sides

| Aspect | Squid-CRM (CRM destination) | PyTagShield (consent authority) |
|--------|----------------------------|---------------------------------|
| Role | System of record for CRM records; stores PII by design | Consent-aware event validation, pseudonymization, routing; fail-closed by default |
| Contract | Record APIs (REST/GraphQL), custom objects; webhook outbound with envelope v1 | `POST /api/v1/event` with `event`, `distinct_id`, `timestamp`, `properties`, optional `consent`; response `status` ∈ {accepted, partially_accepted, backpressure, rejected} |
| Consent metadata | Not emitted on webhook envelopes; no field-level PII minimization | Consent is a gate: destination receives event only when required purpose is granted; pseudonymize before egress |
| PII handling | No field-level PII masking in webhook payloads | Drop/allowlist properties, truncate strings, clear IPs, pseudonymize identifiers with salted hash |
| Audit trail | `core.webhook_deliveries` log with retry/terminal state | Bounded per-destination queues, delivery log |

### 3.2 Evidence

- **Squid:** Webhook envelope v1 has no consent decisions and no field-level PII minimization metadata. `ECOSYSTEM_INTEGRATION.md:145-146` documents this as a known gap. Squid does not decide consent; PyTagShield or another consent authority must gate what is sent to Squid or emitted onward.
- **PyTagShield:** Rust core + Python bindings (110 tests). HTTP server has tested router and handlers, but binary bootstrap is still being wired. At audit time: GitHub repo had no README and no workflows; server binary was a no-op (`fn main() {}`). No Squid-CRM destination adapter. No executable evidence that consent decisions are attached to outbound CRM records (`docs/ecosystem-issues.md:18`).

### 3.3 What must be agreed

1. **Consent attachment point:** Do consent decisions travel on CRM records (as fields), on webhook events (as envelope metadata), or both? This determines whether Squid stores consent metadata on records or emits it on outbound webhooks.
2. **Consent schema:** If consent metadata is stored or emitted, what is its shape? (purpose grants, timestamps, provenance, source, version.)
3. **PII minimization:** Which CRM fields are minimized/pseudonymized before webhook emission? Who decides — PyTagShield policy or Squid configuration?
4. **Consent authority boundary:** Squid must not be treated as a consent authority. PyTagShield must be the decision point; Squid only stores/forwards what PyTagShield approves.
5. **Event routing:** PyTagShield routes events to destinations. Does Squid act as a destination for PyTagShield-processed events, or does Squid emit events that PyTagShield then filters?

### 3.4 Owner of next move

**PyTagShield** must define whether consent decisions attach to records, events, or both, and provide a runnable HTTP service contract (the server binary was a no-op at audit time). Squid cannot add consent metadata until this is defined.

---

## 4. ClusterAudienceKit / PyTagManager + Squid — custom-object schemas

**Status:** OPEN (`docs/ecosystem-issues.md:20`, `19`)

### 4.1 The two sides

| Aspect | ClusterAudienceKit (segmentation output) | PyTagManager (findings output) | Squid-CRM (destination) |
|--------|-----------------------------------------|-------------------------------|------------------------|
| Output | Customer segments, scores, RFM clusters, churn predictions | Tag findings, GTM diagnosis results, consent audits | Custom-object REST `/rest/{object}`, generic JSON GraphQL CRUD |
| Schema | Segment definitions, audience membership, scores | Finding schema/version (not agreed) | Custom-object metadata (object + field definitions) |
| Activation path | Expected to pass through PyReverseETL | Direct custom-object REST (no agreed schema) | Materialized physical tables + scalar/relation columns from metadata |
| Batch | SQL export to 8 warehouse dialects | Single findings records | Custom-object batch upsert (`/rest/{object}/upsert`) |
| Relationship | Segments reference customer identities | Findings reference tags/containers | Relation fields (MANY_TO_ONE) create UUID columns + reverse ONE_TO_MANY |

### 4.2 Evidence

- **Squid:** Custom-object metadata writes create physical tables, scalar/relation columns, and generic REST CRUD are integration-tested (D-040, D-041, D-042). Custom-object batch upsert (D-060) is integration-tested. Generated GraphQL types for custom objects are **not** implemented (known limitation).
- **ClusterAudienceKit:** Rust-powered segmentation engine with Python bindings. No direct Squid-CRM adapter. Audience activation is expected to pass through PyReverseETL, but no Squid payload mapping or custom-object schema contract is defined (`docs/ecosystem-issues.md:20`). External platform activation is deliberately out of scope for ClusterAudienceKit (`README:77`).
- **PyTagManager:** No direct Squid-CRM adapter or agreed findings schema/version (`docs/ecosystem-issues.md:19`). `ECOSYSTEM_INTEGRATION.md:287-289` states ClusterAudienceKit, PyReverseETL and Squid-CRM remain out of scope unless a concrete event/segment/activation contract is agreed.

### 4.3 What must be agreed

1. **Custom-object schema contract:** What object name(s), field names/types, and relation fields will ClusterAudienceKit segments and PyTagManager findings write to Squid? This must be defined before either can write to Squid without ad-hoc fields.
2. **Activation path:** ClusterAudienceKit → PyReverseETL → Squid, or direct ClusterAudienceKit → Squid custom-object REST? The expected path (via PyReverseETL) depends on blocker #2 being resolved first.
3. **Field mapping:** Segment fields (segment_id, segment_name, customer_id, score, RFM class) and finding fields (finding_id, tag_id, container_id, severity, confidence) must map to Squid custom-object field definitions.
4. **Batch/upsert:** Both segmentation and findings are batch-oriented. Squid's custom-object `/rest/{object}/upsert` supports this, but the batch size, frequency, and idempotency key strategy must be agreed.
5. **Generated GraphQL:** If consumers need GraphQL access to custom objects, Squid's generated typed GraphQL schema for custom objects is deferred (known limitation). REST is the interim path.

### 4.4 Owner of next move

**ClusterAudienceKit and PyTagManager** must define their output schemas and agree on a custom-object schema contract with Squid. Squid cannot pre-create object/field metadata without knowing the shape.

---

## 5. Summary: resolution order and dependencies

The four blockers form a dependency chain:

```
PyTagShield (consent contract)  ──┐
                                  ▼
PyCustomerHub (envelope mapping)  ──► Squid webhook outbound
                                  ▼
PyReverseETL (idempotency + payload) ──► Squid REST/custom-object writes
                                  ▼
ClusterAudienceKit / PyTagManager (custom-object schemas) ──► Squid custom-object metadata
```

**Recommended resolution order:**

1. **PyTagShield** defines consent decision contract (records vs events vs both) — this gates whether Squid emits/stores consent metadata on webhooks.
2. **PyCustomerHub** confirms OCDS envelope or documented mapping — this gates Squid webhook delivery to the CDP.
3. **PyReverseETL** adds `Idempotency-Key` header and documents payload shape — this gates warehouse→CRM activation.
4. **ClusterAudienceKit / PyTagManager** define custom-object schemas — this gates audience/finding writes to Squid custom objects.

Until all four are resolved, Squid-CRM's webhook outbound and custom-object ingestion remain partially integrated: the mechanisms exist (envelope v1, HMAC, retries, durable log, custom-object CRUD, batch upsert), but the counterpart contracts are not mutually verified.
