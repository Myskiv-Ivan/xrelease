# API overview

`xrelease serve` starts the **backend** (poller + HTTP API + webhooks):

```sh
xrelease serve
```

CLI parameter reference: [`xrelease` / `xrctl`](cli.md).

### Ports (where to call the API)

| How you run | Base URL for browser / `xrctl` / curl |
|---|---|
| Native binary (`serve`) | `http://127.0.0.1:8080` |
| Docker Compose (UI stack) | `http://127.0.0.1:3000` — nginx proxies `/api`, `/ready`, … (backend `:8080` is **not** published) |
| Kubernetes + UI Ingress | `https://your-host` (Ingress → UI Service → proxy → backend `:8080`) |
| Kubernetes API-only Ingress | `https://hooks-host` via `ingress.api` (direct to backend Service) |

Examples below use `:8080` (native). For Compose, substitute `:3000`.

## Endpoints

| Method | Path | Auth | Description |
|---|---|---|---|
| GET | `/health` | none | Liveness (process up; outbox counts best-effort) |
| GET | `/ready` | none | Readiness (PostgreSQL + notifiers; Apprise live, others presence-based) |
| GET | `/metrics` | none | Prometheus counters + latency histograms |
| GET | `/openapi.json` | none | OpenAPI 3 spec |
| GET | `/api/v1/auth/methods` | none | Available login methods |
| POST | `/api/v1/auth/login` | none | Local username/password → session JWT |
| POST | `/api/v1/auth/oidc/sync` | OIDC Bearer | Upsert OIDC user + role |
| GET | `/api/v1/auth/me` | Bearer* | Current principal |
| POST | `/api/v1/auth/logout` | Bearer* | Revoke local session JWTs |
| GET | `/api/v1/auth/users` | admin | List local + OIDC users |
| POST | `/api/v1/auth/users` | admin | Create a local username/password user |
| POST | `/api/v1/auth/users/{id}/oidc` | admin | Link or unlink an OIDC subject on a local user |
| GET | `/api/v1/status` | Bearer* | Dashboard summary |
| GET | `/api/v1/sources` | Bearer* | Config + live runtime state |
| GET | `/api/v1/sources/{id}` | Bearer* | Single source detail |
| GET | `/api/v1/outbox` | Bearer* | Notification outbox queue |
| POST | `/api/v1/outbox/requeue` | Bearer* | Revive dead-letter outbox rows |
| GET | `/api/v1/config` | Bearer* | Effective + desired config (redacted) |
| GET | `/api/v1/config/schema` | Bearer* | Source/sink options, presets, template placeholders, authority |
| GET | `/api/v1/config/revisions` | Bearer* | Ledger history metadata (no bodies) |
| POST | `/api/v1/config/validate` | Bearer* | Dry-run candidate app document |
| POST | `/api/v1/config/apply` | Bearer* + HMAC† | Hot-swap app config (`api_config` + `source=api`; **409** with `[[organizations]]` — use per-org routes) |
| POST | `/api/v1/config/rollback` | Bearer* + HMAC† | Re-apply previous ledger revision (409 with `[[organizations]]`) |
| POST | `/api/v1/reload` | Bearer* | Re-read desired state + hot-swap (`source=local` file, or every `[[organizations]]` authority; also SIGHUP) |
| GET | `/api/v1/organizations` | Bearer* | Organization catalogue + live source counts |
| GET | `/api/v1/organizations/{id}/config` | Bearer* | One org's desired document from its authority (redacted, ETag) |
| GET | `/api/v1/organizations/{id}/config/revisions` | Bearer* | One org's ledger stream (metadata only) |
| POST | `/api/v1/organizations/{id}/config/validate` | Bearer* | Dry-run one org's candidate against the composed runtime |
| POST | `/api/v1/organizations/{id}/config/apply` | Bearer* + HMAC† | Replace one org's desired document (hot-swap + org stream) |
| POST | `/api/v1/organizations/{id}/config/rollback` | Bearer* + HMAC† | Re-apply the org stream's previous revision |
| GET | `/api/v1/teams` | Bearer* | Team routing catalogue |
| GET | `/api/v1/notifiers` | Bearer* | Live notification sinks (kind, name, tags) |
| POST | `/api/v1/notifiers/test` | Bearer* | Send a test notification (one sink or all) |
| POST | `/api/v1/check` | Bearer* | Trigger poll (all sources) |
| POST | `/api/v1/check/{id}` | Bearer* | Trigger poll (one source) |
| POST | `/api/v1/webhooks/github` | HMAC* | GitHub release event |
| POST | `/api/v1/webhooks/gitlab` | Token* | GitLab release event |
| POST | `/api/v1/webhooks/gitea` | Secret* | Gitea / Codeberg release event |
| POST | `/api/v1/webhooks/bitbucket` | HMAC* | Bitbucket tag push |
| POST | `/api/v1/webhooks/docker` | Secret* | Docker Hub push event |
| POST | `/api/v1/webhooks/generic` | Secret* | Custom payload |

\* Required when management auth is configured (API key, local session JWT,
and/or OIDC). Reads need any authenticated principal; poll / requeue / notifier
test need `operator`+; apply / rollback need `admin` (org-scoped on
`/organizations/{id}/…`). See [Authentication](../operations/authentication.md).  
† Apply/rollback return **404** unless `api_config = true`, and **409** when
`source = "local"`. See [authoring variants](../configuration/overview.md#authoring-variants)
(**Local** / **API** / **API + UI**).

`api.rate_limit_per_minute` (default 120; `0` disables) caps management
**mutations**, inbound **webhooks**, and `POST /auth/login`. Read-only
observability is not rate limited. Details:
[HTTP API settings](../configuration/api.md#rate-limiting).

## Example: local login

```sh
TOKEN=$(curl -s -X POST http://127.0.0.1:8080/api/v1/auth/login \
  -H 'Content-Type: application/json' \
  -d '{"username":"admin","password":"dev-admin-change-me"}' \
  | jq -r .access_token)
curl -s -H "Authorization: Bearer $TOKEN" \
  http://127.0.0.1:8080/api/v1/status | jq
```

## Example: list sources (API key)

```sh
curl -s -H "Authorization: Bearer $XRELEASE_API_KEY" \
  http://127.0.0.1:8080/api/v1/sources | jq
```

## Example: trigger check

```sh
curl -s -X POST -H "Authorization: Bearer $XRELEASE_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"dry_run": false}' \
  http://127.0.0.1:8080/api/v1/check | jq
```

## Example: readiness

```sh
curl -s http://127.0.0.1:8080/ready | jq
```

## Driving the API

For humans and CI, [`xrctl`](cli.md#xrctl--remote-management) wraps the management routes
(`status`, `sources`, `outbox`, `organizations`, `show`, `schema`, `history`,
`validate`, `apply`, `rollback`, `reload`). Local ops (`validate`, `sources`,
`health`, `outbox-requeue`, `serve`) use the [`xrelease`](cli.md#xrelease--local-instance)
binary. The optional dashboard covers observability; the `/config` editor needs
the **API + UI** variant — see
[authoring variants](../configuration/overview.md#authoring-variants).

OpenAPI spec: [openapi.md](openapi.md).
