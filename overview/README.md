# CodeSecretary Overview (dashboard)

The user-facing part: sign in, see your devices and raw activity, create
projects and the rules that bind activity to them (with a match preview),
manage exclusions and pause windows. Later: sessions, corrections, reports,
teams. It never computes time; that is the engine's job.

| Folder | What |
|---|---|
| `api/` | `secretary-overview`: FastAPI on `secretary-orm`, using the warehouse's schema (D15) |
| `web/` | React 19 + Vite + TanStack Router/Query + Tailwind; built into the API image |
| `Dockerfile` | One image: the API serving the built web app (same origin) |

## Run it (Docker Compose)

```sh
docker compose up -d --build        # postgres, keycloak, warehouse, overview
```

Open <http://localhost:8090> and sign in with the dev user `dev@example.test` / `dev`.

## Develop the web app

```sh
corepack enable                      # once: provides pnpm
pnpm install
pnpm --filter @secretary/overview-web dev      # http://localhost:5173, proxies /api and /auth to :8090
```

For sign-in to return to :5173, run the API with `PUBLIC_URL=http://localhost:5173`
(the realm allows both). `pnpm --filter @secretary/overview-web test` runs the unit
tests, `build` writes `web/dist/`.

## API

Session cookie (`sec_session`, HTTP-only) from `/auth/login` → Keycloak →
`/auth/callback`. Mutations need the `X-CSRF-Token` header (value from
`GET /api/me`). Errors are `{"error": code}`.

| Route | What |
|---|---|
| `GET /auth/login?next=/path`, `GET /auth/callback`, `POST /auth/logout` | Sign-in (authorization code + PKCE), sign-out (returns the Keycloak logout URL) |
| `GET /api/me` | Email, time zone, CSRF token |
| `PUT /api/me/time_zone` | The zone the engine cuts local days in; a change recalculates the last 30 days |
| `GET /api/time?start=YYYY-MM-DD&end=YYYY-MM-DD` | Engine time per project and per day (at most 92 days), unassigned time, recalculating state |
| `GET /api/sessions?day=YYYY-MM-DD` | The engine's sessions of one local day with project, duration and evidence count |
| `GET /api/devices`, `PATCH /api/devices/{id}`, `POST /api/devices/{id}/revoke` | Devices; a revoked device is refused by the warehouse within a minute |
| `GET /api/activity?day=YYYY-MM-DD&tz=Area/City` | Raw events of a local day (at most 5,000), no totals |
| `GET/POST /api/projects`, `PATCH/DELETE /api/projects/{id}` | Projects (name, client, billable, hourly rate, currency, color, archived) with their rules |
| `POST /api/projects/{id}/rules`, `DELETE /api/rules/{id}` | Personal mapping rules (`path`, `remote`, `domain`, `app`, `title`); patterns are normalized; changes queue the engine for the last 30 days |
| `POST /api/rules/preview` | What a rule would match in the last 30 days (event counts, matched values, other projects whose rules match the same events); saves nothing |
| `GET /api/rules/suggestions` | Detected repositories no rule covers (optional one-click projects) |
| `GET/POST /api/exclusions`, `DELETE /api/exclusions/{id}` | Exclusions (`path`, `app`, `domain`) and pause windows (`time_window`); agents pick them up within 5 min |

Configuration is in [api/src/secretary_overview/config.py](api/src/secretary_overview/config.py).
Tests need a Postgres superuser, like the warehouse's:

```sh
pip install -e ./orm -e ./warehouse -e "./overview/api[test]"
DATABASE_URL=postgresql://secretary:secretary@localhost:5432/secretary python -m pytest -q overview/api/tests
```
