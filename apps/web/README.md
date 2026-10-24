# Squid CRM Web

Next.js + TypeScript client for Squid CRM.

## Development

```bash
npm install
npm run dev
```

The app runs on <http://localhost:3001>. Requests to `/api/*` are proxied to the
Rust API at `BACKEND_URL` (default `http://localhost:3000`) by
`src/app/api/[...path]/route.ts`.

```bash
# .env.local
BACKEND_URL=http://localhost:3000
```

## Scripts

| Script | Purpose |
| --- | --- |
| `npm run dev` | Development server on port 3001 |
| `npm run build` | Production build (standalone output) |
| `npm start` | Start the production server |
| `npm run typecheck` | `tsc --noEmit` |
| `npm run lint` | ESLint |
| `npm run check` | Type-check, lint and build |

## Structure

- `src/app` — routes, layouts and the API proxy route handler.
- `src/components` — shell, tables, forms and UI primitives.
- `src/hooks` — session, async data, lookups and custom-object metadata.
- `src/lib` — REST/GraphQL clients, types and object configuration.

See [`../../docs/frontend.md`](../../docs/frontend.md) for the full guide.
