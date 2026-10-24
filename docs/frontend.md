# Web Client

The Squid-CRM web client lives in `apps/web`. It is a Next.js (App Router)
application written in strict TypeScript and served independently from the Rust
API.

## Quick start

```bash
cd apps/web
npm install
npm run dev
```

Open <http://localhost:3001>. The browser calls the Next.js app at `/api/*`,
which proxies to the Rust server at `BACKEND_URL` (default
`http://localhost:3000`).

```bash
# .env.local
BACKEND_URL=http://localhost:3000
```

## Authentication

- Sign in and sign up call `POST /auth/signin` and `POST /auth/signup` through
  the proxy.
- The server sets an HttpOnly `squid-session` cookie on the web origin.
- `src/proxy.ts` redirects unauthenticated visitors to `/signin` and signed-in
  visitors away from `/signin` and `/signup`.
- `useSession` loads `GET /rest/me` and exposes the user, workspace and role.
- Sign out calls `POST /auth/signout` and clears the cookie.

User access tokens are not stored in the browser. Sessions are cookie-based.

## Routes

| Route | Purpose |
| --- | --- |
| `/dashboard` | Counts, open pipeline and recent records |
| `/search` | Ranked full-text search across standard objects |
| `/objects/[object]` | List, filter, export and import a standard object |
| `/objects/[object]/new` | Create a standard record |
| `/objects/[object]/[id]` | Detail, related records, archive, restore, delete |
| `/objects/[object]/[id]/edit` | Edit a standard record |
| `/custom/[object]` | List custom-object records |
| `/custom/[object]/new` | Create a custom-object record |
| `/custom/[object]/[id]` | Detail, edit, archive and delete a custom record |
| `/files` | Upload files through signed URLs |
| `/settings` | Workspace overview and connection |
| `/settings/members` | Members and role assignment |
| `/settings/roles` | Custom roles, capabilities and permission flags |
| `/settings/api-keys` | API-key creation, token generation and revocation |
| `/settings/webhooks` | Webhook CRUD and delivery replay |
| `/settings/metadata` | Objects, fields and indexes |
| `/settings/views` | Saved views and field visibility |
| `/settings/data` | Custom-object overview |
| `/settings/jobs` | Durable job queue |
| `/settings/console` | GraphQL console for advanced operations |
| `/settings/profile` | Account, session and client configuration |

Standard objects are people, companies, opportunities, tasks, notes, task
targets and note targets. Custom objects appear automatically in the sidebar.

## Data layer

| File | Responsibility |
| --- | --- |
| `src/lib/api.ts` | REST client, error envelope parsing, CSV import/export, signed uploads |
| `src/lib/graphql.ts` | GraphQL client, queries, mutations and fragments |
| `src/lib/types.ts` | API and domain types |
| `src/lib/resources.ts` | Standard-object field definitions, form serialization and payload builders |
| `src/lib/record-values.ts` | Reads flattened form keys from nested API records |
| `src/hooks/useSession.tsx` | Session context and auth actions |
| `src/hooks/useAsyncData.ts` | Loading/error/reload state for API calls |
| `src/hooks/useLookups.tsx` | Cached relation labels for people, companies, opportunities, tasks, notes and members |

REST is used for standard-object CRUD, search uses GraphQL, and settings use
the `/metadata` GraphQL endpoint.

## Components

- `src/components/app-shell.tsx` — sidebar, search bar, workspace navigation and
  auth gate.
- `src/components/ui.tsx` — buttons, fields, badges, modals, alerts, empty states
  and avatars.
- `src/components/record-table.tsx` — generic list table with relation links,
  paging and archive actions.
- `src/components/record-form.tsx` — generic standard-object form.
- `src/components/custom-record-form.tsx` — metadata-driven custom-object form.

## Design

The UI is server-rendered by Next.js and hydrated on the client. Global styles
live in `src/app/globals.css`; there is no CSS framework dependency. The layout
is responsive down to small tablet widths.

## Checks

```bash
cd apps/web
npm run typecheck
npm run lint
npm run build
# or all three
npm run check
```

## Configuration reference

| Variable | Default | Purpose |
| --- | --- | --- |
| `BACKEND_URL` | `http://localhost:3000` | Server-side proxy target |
| `NEXT_PUBLIC_API_BASE_URL` | `/api` | Browser API prefix |
| `PORT` | `3001` | `npm run dev` / `npm start` port |

## Known gaps

- Workspace invitations are not implemented by the API, so the members page can
  only manage existing members.
- File listing is not exposed by the API; the files page shows uploads from the
  current session.
- Created/updated activity feeds, email, calendar, workflows and AI surfaces are
  not implemented by the backend yet.
- Advanced view filter/sort/group mutations and some metadata edges are exposed
  through the GraphQL console rather than dedicated forms.
