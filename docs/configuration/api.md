# HTTP API configuration

Enabled by running `xrelease serve`. Settings live in **`bootstrap.toml`**.

```toml
[api]
listen = "0.0.0.0:8080"   # containers/K8s: not 127.0.0.1
# require_auth = true     # default: refuse serve without auth configured
# require_auth = false    # lab-only on a trusted network
rate_limit_per_minute = 120
# cors_origins — only when the browser calls the API on a *different* origin.
# Compose/Helm UI use same-origin nginx proxy — leave unset.
# Prefer env for secrets — see below

# Pick ONE authoring variant — see overview for full setup
# [config_api]
# api_config = false      # Local (GitOps)
# source = "local"
# ui_config = false
#
# api_config = true       # API (CI / xrctl)
# source = "api"
# ui_config = false
#
# api_config = true       # API + UI (dashboard editor)
# source = "api"
# ui_config = true
# apply_scope = "xrelease:config-apply"   # optional OIDC extra scope on apply
```

Push-apply mutates **desired state only** (body is the app document). Infra
sections in apply payloads are rejected.

## Authoring modes

Short summary — full setup tables:
[Configuration overview](overview.md#authoring-variants).

| Variant | Flags | How you change desired state |
|---|---|---|
| **Local** | `api_config=false`, `source="local"`, `ui_config=false` | Edit files → reload / restart |
| **API** | `api_config=true`, `source="api"`, `ui_config=false` | `xrctl` / CI / curl apply |
| **API + UI** | `api_config=true`, `source="api"`, `ui_config=true` | Dashboard Config + same apply API |

Invalid: `api_config=false` + `source="api"`; `ui_config=true` without
`api_config` + `source="api"`.

### `[config_api].source`

| Value | Boot | `POST …/config/apply` | Typical use |
|---|---|---|---|
| `local` (default) | App file only (ledger ignored) | **409** (even if `api_config=true`) | **Local** / GitOps |
| `api` | Ledger (optional file = first-boot seed) | Allowed when `api_config=true` | **API** / **API + UI** |

Docker Compose lab = **API + UI** (multi-org, idle until first Apply).

**Local** reload: `POST /api/v1/reload` or `SIGHUP` re-reads `--app` /
`XRELEASE_APP_CONFIG` (or every org `app` file) and hot-swaps.

Optional `apply_scope` — when set, OIDC callers must also present that scope on
apply/rollback (API key and local session JWTs are unaffected).

## Secrets (prefer environment)

| Env var | Config field | Purpose |
|---|---|---|
| `XRELEASE_API_LISTEN` | `listen` | Bind address |
| `XRELEASE_WEBHOOK_SECRET` | `webhook_secret` | Inbound forge HMAC / GitLab token / generic webhooks |
| `XRELEASE_API_KEY` | `api_key` | Bearer auth on the **server**; pass the same value to `xrctl --api-key` / curl |
| `XRELEASE_ADMIN_USER` | `api.local_auth.admin_username` | First-boot UI admin username |
| `XRELEASE_ADMIN_PASSWORD` | `api.local_auth.admin_password` | First-boot UI admin password (**required to seed** — no default) |
| `XRELEASE_SESSION_SECRET` | `api.local_auth.session_secret` | HS256 secret for UI session JWTs (**required** for local login; not derived from the API key) |
| `XRELEASE_API_RATE_LIMIT` | `rate_limit_per_minute` | Cap on **mutating** routes only (`0` = disabled) |
| `XRELEASE_CONFIG_APPLY_SECRET` | — | HMAC for apply/rollback (`X-Config-Signature`) |
| `XRELEASE_CONFIG_ENCRYPTION_KEY` | — | **Required** when `source=api`: AES-256-GCM for `app_secret` values (`openssl rand -base64 32`) |
| `XRELEASE_ALLOW_PLAINTEXT_CONFIG_LEDGER` | — | Lab-only: allow API mode without encryption key |
| `XRELEASE_OIDC_ISSUER` | `api.oidc.issuer` | JWT issuer (`iss`) |
| `XRELEASE_OIDC_DISCOVERY_URL` | `api.oidc.discovery_url` | Discovery when ≠ issuer |
| `XRELEASE_OIDC_JWKS_URI` | `api.oidc.jwks_uri` | JWKS override |
| `XRELEASE_OIDC_AUDIENCE` | `api.oidc.audience` | Accepted `aud` (comma-separated) |
| `XRELEASE_OIDC_REQUIRED_SCOPE` | `api.oidc.required_scope` | Required `scope` / `scp` claim |
| `XRELEASE_OIDC_ROLE_CLAIM` | `api.oidc.role_claim` | Claim path for groups/roles (OIDC sync) |
| `XRELEASE_OIDC_ROLE_ADMIN` | `api.oidc.role_admin` | IdP values → `admin` |
| `XRELEASE_OIDC_ROLE_OPERATOR` | `api.oidc.role_operator` | IdP values → `operator` |
| `XRELEASE_OIDC_ROLE_VIEWER` | `api.oidc.role_viewer` | IdP values → `viewer` |
| `XRELEASE_OIDC_DEFAULT_ROLE` | `api.oidc.default_role` | Fallback role when no claim matches |

## Rate limiting

`rate_limit_per_minute` (default `120`; `0` disables) is a **global** cap on:

- All management **mutations** (`POST /api/v1/check*`, `/outbox/requeue`,
  `/notifiers/test`, `/config/validate|apply|rollback`, `/reload`, and the
  matching `/organizations/{id}/config/…` writes)
- All inbound **webhooks** (`POST /api/v1/webhooks/*`)
- `POST /api/v1/auth/login` (brute-force protection)

It does **not** apply to read-only observability (`GET /api/v1/status`,
`/sources`, `/outbox`, `/teams`, `/notifiers`, `/config`, …) so the dashboard
can poll freely.

## Security notes

- Default bind is `127.0.0.1:8080` — safe for a local binary; in Docker/K8s use `0.0.0.0:8080`
- Compose publishes **UI `:3000` only** — call `/api` via that proxy; do not expect host `:8080`
- Default / production: `require_auth = true` plus API key and/or OIDC and/or `XRELEASE_SESSION_SECRET`
- Dashboard login: [Authentication](../operations/authentication.md); keep `XRELEASE_API_KEY` on the server and pass it to `xrctl --api-key`
- Always set `webhook_secret` when exposing webhook ingress
- Use `cors_origins` only for cross-origin browser access to the API; not needed behind the UI nginx proxy

Secrets template: [`.env.example`](../../.env.example).  
Endpoints: [API overview](../api/overview.md).
