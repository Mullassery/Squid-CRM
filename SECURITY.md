# Security Policy

## Reporting a vulnerability

Do not open a public issue for security problems. Report privately to the
maintainers through the repository's security advisory channel and allow time
for a fix before disclosure.

## Implemented controls (Phase 4 slice)

| Area | Control | Evidence |
| --- | --- | --- |
| Passwords | Argon2id, minimum length 8 | `crates/squid-auth/src/lib.rs` tests |
| Tokens | HS256 JWT with `typ`, `sid`, `jti` for login/access; opaque rotating refresh tokens | `login_token_cannot_be_used_as_access_token`, `refresh_tokens_rotate_and_signout_revokes_the_session` |
| Sessions | 32-byte opaque token, SHA-256 at rest, expiry checked; signout deletes the session and revokes refresh/access tokens | `session_tokens_are_random_and_hashed`, `refresh_tokens_rotate_and_signout_revokes_the_session` |
| API keys | random `api_*` token, SHA-256 at rest, expiry + revocation checked on every request | `api_key_lifecycle_authenticates_data_api` |
| Cookies | `squid-session`, HttpOnly, SameSite=Lax, optional Secure | `crates/squid-auth/src/lib.rs` |
| Data API auth | API key or session cookie only; user access tokens rejected | `graphql_login_flow_and_people_query` |
| CSRF | Origin check on unsafe cookie-authenticated requests; bearer/API-key exempt | `cookie_requests_reject_foreign_origins` |
| Auth throttling | 10 attempts / 15 minutes per account (in process) | `blocks_after_the_burst_limit_and_can_be_cleared` |
| Tenant isolation | workspace from principal, validated schema name, membership re-check | integration test |
| SQL injection | no user text interpolated; validated schema identifiers | `rejects_injected_schema_names` |
| CORS | credentialed, exact `FRONTEND_URL` origin, explicit headers | `app()` |
| Request size | 2 MiB limit | `RequestBodyLimitLayer` |
| Secrets | env-only, never logged; `APP_SECRET` min 32 chars | `Config::from_env` |

## Known gaps (must not be claimed as secure yet)

- Auth throttling is per-process only; multi-replica deployments need a shared
  limiter at the edge or in Redis.
- No CSRF token; cookie auth relies on SameSite=Lax plus the Origin check.
  Review for older browsers/proxies before production.
- No app token / OAuth / 2FA support yet. API-key `roleId` is accepted but
  ignored; keys inherit the caller's built-in role until metadata roles land.
- No field/object permission enforcement beyond built-in roles.
- No SSRF guard yet (no outbound integrations in this slice).
- File upload/download URLs are UUID-addressed and do not yet require a
  signature or authentication. Production must put files in object storage with
  short-lived signed URLs before exposing uploads.
- Audit logging is not yet implemented.
- Dependencies are not yet scanned in CI (`cargo audit` planned).

## Secrets

Never commit `.env`, keys, or credentials. Use a secret manager in production.
Rotate `APP_SECRET` with a planned token-invalidation window (rotating it
invalidates all JWTs).
