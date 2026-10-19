# Architecture Overview

## Runtime topology (Phase 4)

```mermaid
flowchart LR
  Browser[React / TypeScript frontend] -->|HTTPS, cookie + bearer| API
  subgraph Rust["squid-server (Axum + Tokio)"]
    API[/"/graphql /rest /auth /healthz"/]
    AUTH[squid-auth]
    CORE[squid-core]
    DBLAYER[squid-db]
  end
  API --> AUTH
  API --> CORE
  API --> DBLAYER
  DBLAYER -->|SQLx| PG[(PostgreSQL 16)]
  subgraph PG
    CORE_SCHEMA[(core schema\nusers, workspaces,\nmembers, sessions)]
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

  C->>A: POST /rest/people (cookie or Bearer)
  A->>Auth: resolve_auth(headers)
  Auth->>Repo: find_session_by_hash / find_membership_role
  Repo->>P: SELECT core.user_sessions / workspace_members
  P-->>Repo: session + role
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
```

Each workspace additionally owns a schema containing object tables (`person` in
the current slice), mirroring upstream.

## Security boundary summary

- Credentials: Argon2id password hashes; HS256 JWTs with typed claims.
- Sessions: opaque tokens, SHA-256 hashed at rest, HttpOnly/SameSite=Lax cookie
  (Secure in production).
- Tenancy: `workspace_id` from the principal only; validated schema identifiers.
- Transport: credentialed CORS restricted to `FRONTEND_URL`; 2 MiB body limit.
- See `SECURITY.md` and `docs/threat-model.md` (Phase 6) for the full model.
