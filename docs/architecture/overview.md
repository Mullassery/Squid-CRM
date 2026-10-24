# Architecture Overview

## Runtime topology

```mermaid
flowchart LR
  Browser[Next.js web client in apps/web] -->|/api proxy, cookie or API key| API
  subgraph Rust["squid-server (Axum + Tokio)"]
    API[/"/graphql /metadata /rest /auth /files /healthz"/]
    AUTH[squid-auth]
    CORE[squid-core]
    DBLAYER[squid-db]
  end
  API --> AUTH
  API --> CORE
  API --> DBLAYER
  DBLAYER -->|SQLx| PG[(PostgreSQL 16)]
  subgraph PG
    CORE_SCHEMA[(core schema\nusers, workspaces, members,\nsessions, api_keys,\nobject/field/index metadata, files)]
    WS_A[(workspace_<id>\nperson ...)]
    WS_B[(workspace_<id>\nperson ...)]
  end
  DBLAYER --- CORE_SCHEMA
  DBLAYER --- WS_A
  DBLAYER --- WS_B
  API -. future .-> REDIS[(Redis: jobs/cache)]
```

## Request lifecycle

```mermaid
sequenceDiagram
  participant C as Client
  participant A as Axum router
  participant Auth as squid-auth
  participant Repo as squid-db
  participant P as PostgreSQL

  C->>A: POST /rest/people (API key or cookie)
  A->>Auth: resolve_auth_with_policy(headers, data API)
  Auth->>Repo: find_api_key_by_token_hash / find_session_by_hash / find_membership_role
  Repo->>P: SELECT core.api_keys / core.user_sessions / workspace_members
  P-->>Repo: principal + role
  Repo-->>Auth: AuthContext { user, workspace, role }
  Auth-->>A: AuthContext
  A->>Repo: create_person(workspace_id, input)
  Repo->>P: INSERT workspace_<id>.person
  P-->>Repo: row
  Repo-->>A: Person
  A-->>C: 201 { data: { person } }
```

## Data model (Phase 4)

```mermaid
erDiagram
  USERS ||--o{ WORKSPACE_MEMBERS : joins
  WORKSPACES ||--o{ WORKSPACE_MEMBERS : has
  USERS ||--o{ USER_SESSIONS : owns
  WORKSPACES ||--o{ USER_SESSIONS : scopes
  USER_SESSIONS ||--o{ REFRESH_TOKENS : rotates
  USERS {
    uuid id PK
    text email
    text password_hash
    timestamptz created_at
  }
  WORKSPACES {
    uuid id PK
    text display_name
    text subdomain
  }
  WORKSPACE_MEMBERS {
    uuid id PK
    uuid workspace_id FK
    uuid user_id FK
    text role
  }
  USER_SESSIONS {
    uuid id PK
    uuid user_id FK
    uuid workspace_id FK
    text token_hash
    timestamptz expires_at
  }
  REFRESH_TOKENS {
    uuid id PK
    uuid session_id FK
    text token_hash
    text replaced_by_token_hash
    timestamptz expires_at
    timestamptz revoked_at
  }
```

Each workspace additionally owns a schema containing object tables (`person` in
the current slice). Workspace-scoped object/field metadata
lives in `core.object_metadata` / `core.field_metadata` and is seeded lazily:
the standard `person` object plus its 16 fields, and one default table view with
five view fields. See ADR 0004. The web client boot query (`currentUser`) is
served from the same core rows with static defaults for unimplemented
enterprise surfaces (ADR 0005).

## Security boundary summary

- Credentials: Argon2id password hashes; HS256 access/login JWTs with `typ`,
  `sid`, `jti`; opaque, rotating, hashed refresh tokens.
- Sessions: opaque tokens, SHA-256 hashed at rest, HttpOnly/SameSite=Lax cookie;
  deleting a session revokes its refresh tokens and access tokens.
- API keys: `api_*` bearer tokens, SHA-256 hashed at rest, expiry/revocation checks
  (Secure in production). Data APIs accept API keys or the session cookie, never
  user access tokens; `/metadata` accepts access tokens.
- CSRF: credentialed CORS restricted to `FRONTEND_URL` plus an Origin check on
  unsafe cookie-authenticated requests. Auth endpoints are burst-limited.
- Tenancy: `workspace_id` from the principal only; validated schema identifiers.
- Transport: 2 MiB body limit.
- See `SECURITY.md` and `docs/threat-model.md` (Phase 6) for the full model.
