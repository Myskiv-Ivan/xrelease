# Authentication

How operators and automation access xrelease.

| Method | Who uses it | Credential |
|---|---|---|
| **Local login** (default UI) | People in the dashboard | Username + password → session JWT |
| **API key** | `xrctl`, CI, scripts | `Authorization: Bearer <api-key>` |
| **OIDC / SSO** | People via corporate IdP | IdP access token (Bearer) |

Configure secrets in `.env` (Docker) or a Kubernetes Secret. UI modes use
`VITE_AUTH_MODE` on the **UI container** (runtime `/ui-config.js`): `local`
(default), `oidc`, `hybrid`, or `api_key` — change without rebuilding the image.

## First login (Docker)

After `cp .env.example .env`, fill the blank secrets (see comments in
`.env.example` / `openssl rand …`), then `docker compose up -d`:

1. Open **http://127.0.0.1:3000**
2. Sign in with the values from `.env`:

| Variable | Notes |
|---|---|
| `XRELEASE_ADMIN_USER` | Default `admin` |
| `XRELEASE_ADMIN_PASSWORD` | Set yourself (blank in `.env.example`) |
| `XRELEASE_SESSION_SECRET` | Required — dedicated secret for session JWTs |

Helm lab: set `secrets.sessionSecret` / `secrets.adminPassword` (or use
`secrets.existingSecret` with the same keys as `.env.example`).

On first start, when `app_user` is empty **and** both
`XRELEASE_SESSION_SECRET` and `XRELEASE_ADMIN_PASSWORD` are set, the backend
seeds that admin into PostgreSQL. There is **no** built-in default password:
without those env vars, local login stays disabled (fail closed).

Keep `XRELEASE_API_KEY` for automation only — it is **not** the UI password and
is never reused as the session signing key.

Change admin password and session secret before exposing the UI beyond localhost.

## API key (CLI and automation)

The **server** loads the key from `[api].api_key` / `XRELEASE_API_KEY`. Pass the
same value to xrctl as a flag (not via env on the client):

```bash
xrctl --api-url https://xrelease.example.com --api-key "$XRELEASE_API_KEY" status
```

Or with curl:

```bash
curl -s -H "Authorization: Bearer $XRELEASE_API_KEY" \
  https://xrelease.example.com/api/v1/status
```

## Roles

Three roles (highest wins when several apply):

| Role | Typical access |
|---|---|
| `viewer` | Read status, sources, outbox, config |
| `operator` | Viewer + poll / requeue / notifier tests |
| `admin` | Operator + config apply / rollback / settings |

- **Local admin** is seeded as `admin`.
- **OIDC** users get a global role (and optional per-org grants) from IdP
  groups/claims — see [OIDC / SSO](oidc.md).
- A valid **API key** is always **`admin`** on the backend (full automation
  credential). The UI may mirror that with `VITE_API_KEY_DEFAULT_ROLE`
  (default **`admin`**; override only for demos).

Management route gates: reads → any authenticated principal; mutations such as
poll / requeue / notifier test → `operator`+; config apply / rollback → `admin`
(or org-scoped `admin` on `/organizations/{id}/config/…`).

## Endpoints

| Method | Path | Auth | Purpose |
|---|---|---|---|
| GET | `/api/v1/auth/methods` | none | Which login methods the server offers |
| POST | `/api/v1/auth/login` | none | Local username/password → session JWT |
| POST | `/api/v1/auth/oidc/sync` | OIDC Bearer | Create/update user + assign role; **auto-links** a local user when emails match |
| GET | `/api/v1/auth/me` | Bearer | Current principal |
| POST | `/api/v1/auth/logout` | Bearer | Revoke local session JWTs for this user |
| GET | `/api/v1/auth/users` | admin | Directory of local + OIDC users |
| POST | `/api/v1/auth/users` | admin | Create a local username/password user |
| POST | `/api/v1/auth/users/{id}/oidc` | admin | Link or unlink an IdP `sub` on a local user |

### Linking SSO to an existing local user

1. **Admin UI / API** — on a `local` row, set `oidc_sub` via
   `POST /api/v1/auth/users/{id}/oidc` (blank unlinks). `auth_source` stays
   `local`; password login keeps working.
2. **Auto on SSO** — first `/auth/oidc/sync` with a matching email attaches
   `oidc_sub` to that local row instead of creating a duplicate OIDC-only user.

Management routes (`/api/v1/status`, `/sources`, …) accept:

1. Static API key, or
2. Local session JWT, or
3. Valid OIDC access token

when the corresponding method is configured.

## Production checklist

- [ ] `api.require_auth = true` in `bootstrap.toml` (default; keep it explicit)
- [ ] Strong `XRELEASE_SESSION_SECRET` and `XRELEASE_ADMIN_PASSWORD` (or OIDC-only UI)
- [ ] Strong `XRELEASE_API_KEY` if you use `xrctl` / CI
- [ ] `XRELEASE_WEBHOOK_SECRET` when forge webhooks are public
- [ ] Prefer HTTPS at the edge — see [TLS](tls.md)
- [ ] For SSO, follow [OIDC / SSO](oidc.md)

Environment reference: [HTTP API settings](../configuration/api.md).
