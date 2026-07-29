# Docker deployment

Default stack: **nginx UI + backend + PostgreSQL + Apprise** — HTTP on localhost.

The repo’s `bootstrap.toml` is the **Docker default**: multi-organization
catalogue with **API + UI** (`api_config = true`, `source = "api"`,
`ui_config = true`). Desired state is authored in the dashboard (Config → org →
Edit → Apply) into the Postgres ledger. Root `app/releases.yaml` is a stub and
is **not** mounted.

## Quick start

```sh
cp .env.example .env
# Fill blank secrets — see comments in .env.example (openssl rand …)
docker compose up -d
```

Open **http://127.0.0.1:3000**. Sign in with the local admin from `.env`
([Authentication](../operations/authentication.md)). The UI proxies `/api/v1/…`,
`/ready`, `/health`, and `/openapi.json` to the backend — **host `:8080` is not
published**. From the host:

```sh
xrctl --api-url http://127.0.0.1:3000 --api-key "$XRELEASE_API_KEY" status
```

**First desired config:** pick an organization in the switcher → **Config** →
**Edit** → add teams / sources / delivery channels → **Apply**. Sample YAML to
paste or adapt: [`app/releases.example.yaml`](../../app/releases.example.yaml)
or [`deploy/examples/multi-org/`](../../deploy/examples/multi-org/).

## What's mounted

| File | Purpose |
|---|---|
| `bootstrap.toml` | Infra + `[[organizations]]` + authoring variant (**API + UI** in the sample) |
| `.env` | Secrets (DB, API key, session secret, admin password, tokens) |

For **Local**, also mount `app/<id>/releases.yaml` (or a single `app/releases.yaml`
without orgs). See comments in [`docker-compose.yaml`](../../docker-compose.yaml).

## Environment variables

| Variable | Purpose |
|---|---|
| `XRELEASE_DATABASE_URL` | PostgreSQL connection |
| `XRELEASE_SESSION_SECRET` | Required for local UI login (session JWT) |
| `XRELEASE_ADMIN_USER` / `XRELEASE_ADMIN_PASSWORD` | First-boot dashboard admin |
| `XRELEASE_API_KEY` | Server Bearer; pass the same value to `xrctl --api-key` / curl |
| `XRELEASE_WEBHOOK_SECRET` | Inbound forge webhook verification |
| `XRELEASE_APPRISE_ENDPOINT` | Apprise API base (Compose sets `http://apprise:8000`) |
| `XRELEASE_NOVU_API_KEY` | Novu API key when notifier `api_key` / `api_key_env` is unset |
| `XRELEASE_EXPRESS_ACCESS_TOKEN` | Default eXpress Bearer (or per-sink `access_token_env`) |
| `XRELEASE_SMTP_PASSWORD` | SMTP AUTH when notifier `password` is empty |
| `XRELEASE_LOG` | Tracing filter (default `info`) |
| `GITHUB_TOKEN` | GitHub API when needed |
| `VITE_*` | UI container **runtime** settings (`/ui-config.js`). Compose → `ui` service; Helm → `ui.env`. See [docker/README](../../docker/README.md). |

Image tags are in [`docker-compose.yaml`](../../docker-compose.yaml), not `.env`.


Compose may supply weak lab defaults when `.env` is incomplete — replace before
production. Notification destinations: [notifications](../configuration/apprise.md).

## Authoring variants

Default lab = **API + UI** (`bootstrap.toml` only). For **Local** (mount
`app/*.yaml`) or **API**-only (`ui_config=false`), see
[authoring variants](../configuration/overview.md#authoring-variants).

## Optional

| Topic | Doc |
|---|---|
| HTTPS (UI nginx + cert/key) | [TLS](../operations/tls.md) — `compose.tls.yaml` |
| OIDC / SSO | [OIDC](../operations/oidc.md) — set `VITE_*` + `XRELEASE_OIDC_*`, restart UI |
| Compose / images | [docker/README](../../docker/README.md) |
| Without UI | Helm `ui.enabled: false` (or omit the UI service) — backend stays `serve` |
| Apply from CI | [CI/CD integration](../operations/ci-cd.md) |

## Health

| Endpoint | Check |
|---|---|
| UI | `http://127.0.0.1:3000/health` |
| API readiness | `http://127.0.0.1:3000/ready` |
| Metrics | `http://127.0.0.1:3000/metrics` (loopback only — see below) |

## Metrics exposure

`/metrics` needs no authentication. The plain HTTP stack binds the UI to
`127.0.0.1` only, so proxying it there is safe and the dashboard's
"Open /metrics" button works. The edge-facing variants drop it:
`docker/compose.tls.yaml` (published on `0.0.0.0:443`) and the Helm chart
(public Ingress) both return `404`, and expect Prometheus to scrape the
backend directly — [Grafana](../../deploy/grafana/README.md).

## Hardening

Services run with `no-new-privileges` and memory limits; the backend also
drops all capabilities and uses a read-only root filesystem with `tmpfs` for
`/data` and `/tmp`. Details: [docker/README](../../docker/README.md).
