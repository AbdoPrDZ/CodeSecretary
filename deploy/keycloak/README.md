# Keycloak (local development)

Keycloak is the identity provider for users and devices (decision D14). It only says who someone is; what they may see is decided in Postgres by row-level security.

```sh
docker compose up -d --wait keycloak
python deploy/keycloak/smoke_device_flow.py      # headless check of the agent's device login
```

| What | Value |
|---|---|
| Realm / issuer | `secretary` · `http://localhost:8180/realms/secretary` |
| Discovery | http://localhost:8180/realms/secretary/.well-known/openid-configuration |
| Admin console | http://localhost:8180/admin (`admin` / `admin`) |
| Test user | `dev@example.test` / `dev` (self-registration is also enabled) |
| Device page | http://localhost:8180/realms/secretary/device |

**Inside Compose,** services reach Keycloak at `http://keycloak:8080`. Tokens still carry the `localhost:8180` issuer (`KC_HOSTNAME`, with a dynamic backchannel), so the warehouse checks the same issuer string the agent sees.

## Realm

[`import/secretary-realm.json`](import/secretary-realm.json) is imported on first start only. Once the realm exists, Keycloak skips the import, so changes made in the admin console survive restarts. To re-import after editing the file, see *Reset* below.

- **`secretary-agent`:** a public client for the agent. It supports only the OAuth 2.0 device authorization grant, with no browser redirect and no password grant. With the optional `offline_access` scope, the agent gets an offline refresh token that it keeps in the OS keychain.
  - An audience mapper adds `aud: secretary-warehouse` to access tokens; the warehouse rejects tokens without it.
  - The `sid` claim identifies the device session and maps to `devices.auth_session_id`.
- **`secretary-dashboard`:** a confidential client for the overview web app, using authorization code + PKCE (S256). Redirect URIs are `http://localhost:5173/*` and `http://localhost:8090/*`. The secret `dev-dashboard-secret` is for development only.
- **Tokens:**
  - Access tokens last 5 minutes.
  - Refresh tokens rotate: each one can be used once (`revokeRefreshToken`).
  - Offline sessions expire after 30 days idle.
  - The device code lasts 10 minutes and is polled every 5 s.
- **Not configured yet:**
  - GitHub/Google logins. These need OAuth app credentials; add them under *Identity providers*, then export the realm.
  - Company SSO (Phase 4, OV-44).
  - A service account the overview can use to end device sessions (OV-4).
  - Production settings: HTTPS, a real hostname, secrets from the environment.

## Database

Keycloak has its own `keycloak` database and role on the Compose Postgres server. No CodeSecretary service reads it. A fresh volume gets both from [`deploy/postgres/init/02-keycloak.sh`](../postgres/init/02-keycloak.sh). For a volume that existed before Keycloak was added, create them once by hand:

```sh
docker compose exec postgres psql -U secretary -d secretary \
  -c "CREATE ROLE keycloak LOGIN PASSWORD 'keycloak'" -c "CREATE DATABASE keycloak OWNER keycloak"
```

## Reset

To re-import the realm file, drop and recreate Keycloak's database. This deletes every Keycloak user and session; CodeSecretary data is untouched:

```sh
docker compose stop keycloak
docker compose exec postgres psql -U secretary -d secretary \
  -c "DROP DATABASE keycloak WITH (FORCE)" -c "CREATE DATABASE keycloak OWNER keycloak"
docker compose up -d --wait keycloak
```
