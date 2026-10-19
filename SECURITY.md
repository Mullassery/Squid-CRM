# Security Policy

## Reporting a vulnerability

Do not open a public issue for security problems. Report privately to the
maintainers through the repository's security advisory channel and allow time
for a fix before disclosure.

## Implemented controls (Phase 4 slice)

| Area | Control | Evidence |
| --- | --- | --- |
| Passwords | Argon2id, minimum length 8 | `crates/squid-auth/src/lib.rs` tests |
| Tokens | HS256 JWT with `typ` claim (login/access/refresh) | `login_token_cannot_be_used_as_access_token` |
| Sessions | 32-byte opaque token, SHA-256 at rest, expiry checked | `session_tokens_are_random_and_hashed` |
| Cookies | `twenty-session`, HttpOnly, SameSite=Lax, optional Secure | `crates/squid-api/src/lib.rs` |
| Tenant isolation | workspace from principal, validated schema name, membership re-check | integration test |
| SQL injection | no user text interpolated; validated schema identifiers | `rejects_injected_schema_names` |
| CORS | credentialed, exact `FRONTEND_URL` origin, explicit headers | `app()` |
| Request size | 2 MiB limit | `RequestBodyLimitLayer` |
| Secrets | env-only, never logged; `APP_SECRET` min 32 chars | `Config::from_env` |

## Known gaps (must not be claimed as secure yet)

- No rate limiting / brute-force protection on auth endpoints.
- No CSRF token; cookie auth relies on SameSite=Lax. Cross-site POST protection
  needs review before production.
- No API key / app token / OAuth / 2FA support yet.
- No refresh-token rotation or server-side revocation of access tokens.
- No field/object permission enforcement beyond built-in roles.
- No SSRF guard yet (no outbound integrations in this slice).
- No upload handling yet.
- Audit logging is not yet implemented.
- Dependencies are not yet scanned in CI (`cargo audit` planned).

## Secrets

Never commit `.env`, keys, or credentials. Use a secret manager in production.
Rotate `APP_SECRET` with a planned token-invalidation window (rotating it
invalidates all JWTs).
