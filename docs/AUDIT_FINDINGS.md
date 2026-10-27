# Squid-CRM — Audit Findings

Audit date: 2026-10-27
Scope: Rust workspace, Next.js client, PostgreSQL schema/migrations, CI,
security documentation
Method: read-only source/docs/CI inspection; no tests run to avoid generated
artifacts and database state

## Summary

Squid has a strong baseline for an alpha CRM: Argon2id passwords, short-lived
JWTs, opaque rotating refresh tokens, hashed sessions/API keys, exact-origin
CORS, Origin checks for cookie requests, schema-per-tenant isolation, body-size
limits, signed file URLs, and a durable HMAC-signed webhook pipeline with
retries and replay. CI runs fmt, clippy `-D warnings`, `cargo audit`,
`cargo deny`, tests, a smoke test, and frontend type-check/lint/build.

The main risks are outbound-request SSRF and auth throttling. `http.request`
filters literal private IPs but not DNS resolution or redirects, and webhook
targets have no SSRF guard. The in-process auth limiter counts successful
logins, can be used to lock a known account, and does not limit distributed
attempts.

| Severity | Count |
|---|---:|
| Critical | 0 |
| High | 0 |
| Medium | 3 |
| Low | 3 |

## Findings

### 1. Medium — `http.request` SSRF bypass via DNS and redirects

**Evidence**

- `crates/squid-api/src/rest.rs:3296-3347`: `http_job_url_allowed` parses the
  URL and checks the literal host. A non-IP hostname passes; private ranges are
  rejected only when the host itself parses as an IP.
- `crates/squid-api/src/rest.rs:3398`: `reqwest::Client::new()` uses the default
  redirect policy.
- `crates/squid-api/src/rest.rs:3381-3389`: the URL check is the only gate.

**Impact**

A workspace admin can enqueue `http.request` to a public hostname resolving to
`127.0.0.1`, `169.254.169.254`, or an RFC1918 address, or to a public URL that
redirects internally. This reaches internal services and cloud metadata despite
the “private blocked by default” control.

**Root cause hypothesis**

The allowlist validates the URL string rather than the resolved connection
address; the job client was not configured to disable/revalidate redirects.

**Recommendation**

Resolve the hostname and reject/revalidate all resolved addresses immediately
before connect; pin the resolved IP; disable redirects or revalidate each hop;
reuse one hardened client; add DNS-rebinding and redirect-chain tests.

### 2. Medium — Webhook targets can reach internal services

**Evidence**

- `crates/squid-api/src/graphql.rs:3436-3460`: `validate_webhook_target` checks
  length, scheme, host presence, and embedded credentials only.
- `crates/squid-api/src/rest.rs:2922-2931`: the webhook client disables
  redirects but posts to the configured target.
- `docs/threat-model.md:93` acknowledges no SSRF guard for outbound
  integrations beyond `http.request`.

**Impact**

A workspace admin can point a webhook at internal ports/services, use delivery
success/failure and response timing as a port scanner, and potentially reach
cloud metadata. The webhook secret is not exposed to the target, but the request
body and delivery metadata are.

**Root cause hypothesis**

Webhook validation predates the `http.request` SSRF control and was not brought
under the same policy.

**Recommendation**

Apply the same connect-time SSRF policy at webhook create and delivery time;
reject loopback/private/link-local targets by default; require an explicit
operator allowlist/opt-in; revalidate DNS at delivery.

### 3. Medium — Auth limiter causes targeted lockout and misses distributed attacks

**Evidence**

- `crates/squid-api/src/rate_limit.rs:20-55`: every `check` records an attempt;
  no success-path clearing.
- `crates/squid-api/src/auth.rs:52,95`: limiter is called before password
  verification.
- Keys are `signup:<email>` / `signin:<email>`; the limiter is in-process.
- `MAX_TRACKED_KEYS=10_000`; when full, new keys receive 429 until expiry.

**Impact**

Ten requests for a known email in 15 minutes — including successful logins —
block that account at the application layer. An attacker can lock out any known
user. Credential stuffing across many emails/IPs is not throttled, and the
10k-key cap can be saturated to deny new auth keys. Multi-replica deployments
each keep a separate window.

**Root cause hypothesis**

The limiter was designed as a coarse per-account burst guard, documented as
complementary to a per-account lockout that was not implemented; success-path
clearing and per-IP buckets were omitted.

**Recommendation**

Clear/decay the bucket on success; add per-IP and per-account buckets; use
progressive delays; add an edge/Redis shared limiter for multi-replica; return
generic responses to reduce enumeration; test successful-login clearing and
many-account attacks.

### 4. Low — Session cookies lack `Secure` by default

**Evidence**

- `crates/squid-core/src/config.rs:55-57`: `COOKIE_SECURE` defaults to false.
- `crates/squid-api/src/lib.rs:687-700`: `Secure` is appended only when true.
- README quick start sets `APP_SECRET` but not `COOKIE_SECURE`.

**Impact**

A production deployment served over HTTP, or one that terminates TLS without
setting the flag, can expose session cookies to network attackers. HttpOnly and
SameSite=Lax still apply.

**Root cause hypothesis**

Local-development default leaked into production guidance; no automatic HTTPS
detection or startup warning.

**Recommendation**

Default to secure in production profiles or refuse `COOKIE_SECURE=false` for
non-loopback binds; document `COOKIE_SECURE=true` in the HTTPS guide.

### 5. Low — CSRF check is conditional on `Origin`

**Evidence**

- `crates/squid-api/src/lib.rs:557-572`: only rejects when `Origin` is present
  and mismatched.
- `docs/threat-model.md:90`: no CSRF token; older browser/proxy review pending.

**Impact**

Requests without `Origin` bypass the explicit check. SameSite=Lax is the main
control; non-browser or legacy clients are not covered by the Origin policy.

**Root cause hypothesis**

The control was intentionally lightweight and token-based CSRF was deferred.

**Recommendation**

Require a valid `Origin`/`Referer` for cookie-authenticated unsafe methods, or
add a double-submit CSRF token; test missing-Origin requests.

### 6. Low — Security documentation contradicts code/CI

**Evidence**

- `docs/threat-model.md:30,76,78,93,96` says SSRF is partial, outbound SSRF
  beyond `http.request` is unguarded, and dependency scanning is not in CI.
- `docs/known-limitations.md:73,89` says SSRF allowlisting and
  `cargo audit`/`cargo deny` are implemented.
- `docs/decision-log.md:62` says SSRF allowlisting remains open.
- `.github/workflows/ci.yml` runs `cargo audit` and `cargo deny`.

**Impact**

Operators cannot reliably determine which protections exist; audit evidence is
inconsistent.

**Root cause hypothesis**

Docs evolved across phases without reconciling D-061 and later controls.

**Recommendation**

Update the threat model/decision log to match code and CI; mark each control
implemented/partial/open with a test reference; add a docs consistency check.

## Positive Observations

- Argon2id; short-lived JWT; opaque rotating refresh; hashed sessions/API keys.
- Exact-origin CORS with credentials.
- Validated schema-per-tenant isolation and per-request membership checks.
- Webhook HMAC over raw body; durable delivery log; retries; replay.
- 2 MiB request body limit; UUID file addressing; signed expiring URLs.
- CI includes audit, deny, tests, smoke, and frontend build/type-check/lint.
- 89 Rust tests, including auth rotation/replay, cross-tenant isolation,
  literal private-IP rejection, idempotency, and webhook signature behavior.

## Verification Notes

- Working tree was clean; no source/config changes were made.
- Tests were not run; CI status was not independently verified.
- S3 live-provider behavior and webhook retry starvation were not load-tested.
- The webhook/`http.request` SSRF findings assume a workspace admin can reach
  those admin-guarded surfaces; the audit did not enumerate every role mapping.
