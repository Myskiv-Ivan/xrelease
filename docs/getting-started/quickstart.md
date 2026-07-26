# Quick start

## Default labs (what you get out of the box)

| | **Docker** (`docker-compose.yaml`) | **Kubernetes** (chart defaults) |
|---|---|---|
| **Authoring** | **API + UI** (multi-org ledger) | **API + UI** (single-doc ledger) |
| **Desired state** | Idle until UI / `xrctl` Apply | Idle until UI / `xrctl` Apply |
| **URL** | http://127.0.0.1:3000 | Ingress host (`xrelease.local`) |
| **API from host** | via `:3000` (backend `:8080` not published) | via Ingress → UI nginx |
| **Config** | repo [`bootstrap.toml`](../../bootstrap.toml) | chart `bootstrapInline` (**API + UI**) |

**Local** and **API** (no UI editor): docs + [`deploy/examples/`](../../deploy/examples/) only — not the default labs.

Authoring flags: [Local / API / API + UI](../configuration/overview.md#authoring-variants).

## Docker (recommended)

```sh
git clone https://github.com/Myskiv-Ivan/xrelease.git && cd xrelease
cp .env.example .env
# Fill blank secrets (required — api.require_auth defaults to true), e.g.:
#   openssl rand -hex 24    → XRELEASE_API_KEY, XRELEASE_WEBHOOK_SECRET, XRELEASE_ADMIN_PASSWORD
#   openssl rand -hex 32    → XRELEASE_SESSION_SECRET
#   openssl rand -base64 32 → XRELEASE_CONFIG_ENCRYPTION_KEY
docker compose up -d
```

Dashboard: **http://127.0.0.1:3000**

Sign in with the admin from `.env`:

| | |
|---|---|
| Username | `XRELEASE_ADMIN_USER` → `admin` |
| Password | `XRELEASE_ADMIN_PASSWORD` → value you set in `.env` |

OIDC: [oidc.md](../operations/oidc.md).

Automation (`xrctl`, curl) — use the **UI proxy**, not `:8080`:

```sh
xrctl --api-url http://127.0.0.1:3000 --api-key "$XRELEASE_API_KEY" status
```

| File | Role |
|---|---|
| `bootstrap.toml` | Infra + `[[organizations]]` + **API + UI** |
| `app/releases.example.yaml` | Sample to paste into UI Apply (per org) |
| `.env` | Secrets (never commit) |

The stack boots **idle** (no sources until Apply). After login: org switcher →
**Config** → **Edit** → teams / sources / channels → **Apply**.

More: [Docker](docker.md) · [Authentication](../operations/authentication.md).

## Kubernetes

```sh
cp deploy/k8s/values.secrets.example.yaml deploy/k8s/values.secrets.yaml
# edit passwords in values.secrets.yaml
helm upgrade --install xrelease ./deploy/helm/xrelease \
  --namespace xrelease --create-namespace \
  -f deploy/k8s/values.secrets.yaml
```

Chart default is **API + UI** (same as Docker) + HTTP Ingress — idle until Apply.
Sign in with `secrets.adminUser` / `secrets.adminPassword` from the secrets
overlay, then **Config → Edit → Apply** (or `xrctl apply`).

More: [Kubernetes](kubernetes.md) ·
[`deploy/k8s/README.md`](../../deploy/k8s/README.md).

## Verify

```sh
# Docker / root lab (multi-org, empty orgs OK — expect warnings)
docker compose exec xrelease xrelease --config /config/bootstrap.toml validate

# Or with a published CLI against a filled sample checked out from the repo:
xrctl --api-url http://127.0.0.1:3000 --api-key "$XRELEASE_API_KEY" status
```

Filled single-document samples for GitOps / apply:
[`deploy/examples/`](../../deploy/examples/).
