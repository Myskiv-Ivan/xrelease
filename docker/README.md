# Docker Compose deployment

Default stack: **PostgreSQL + backend + UI (nginx) + Apprise** — published
GHCR images.

| File | Command |
|------|---------|
| [`docker-compose.yaml`](../docker-compose.yaml) | `docker compose up -d` |

**Default `bootstrap.toml`:** multi-org **API + UI**. Desired state → Postgres
ledger via dashboard. OIDC = your IdP — [oidc.md](../docs/operations/oidc.md).
Kubernetes: [`deploy/k8s/README.md`](../deploy/k8s/README.md).

## Quick start

```bash
cp .env.example .env
# edit secrets
docker compose up -d
# → http://127.0.0.1:3000
```

## Layout

| File | Role |
|------|------|
| [`../docker-compose.yaml`](../docker-compose.yaml) | GHCR images (`pull_policy: always`) |
| `Dockerfile` / `Dockerfile.ui` / `Dockerfile.cli` | Image definitions used by releases |
| [`compose.tls.yaml`](compose.tls.yaml) | HTTPS on UI nginx (`cert.pem` / `key.pem`) |
| [`nginx.tls.conf`](nginx.tls.conf) | nginx TLS config mounted by the overlay |
| [`certs/`](certs/README.md) | `cert.pem` + `key.pem` (paths via `.env`) |

## Architecture

```mermaid
flowchart TB
  subgraph compose[docker compose]
    UI[ui nginx :3000]
    XR[xrelease serve — :8080]
    PG[(postgres)]
    AP[apprise]
  end
  CFG[bootstrap.toml] --> XR
  ENV[.env] --> XR
  XR --> PG
  XR --> AP
  UI -->|proxy /api| XR
```

| Service | Host port |
|---------|-----------|
| `postgres` | *(unpublished)* |
| `xrelease` | *(unpublished — use UI :3000)* |
| `ui` | `127.0.0.1:3000` |
| `apprise` | `127.0.0.1:8000` (lab) |

```bash
xrctl --api-url http://127.0.0.1:3000 --api-key "$XRELEASE_API_KEY" status
```

## Config vs secrets

| In Git | In `.env` |
|---|---|
| `bootstrap.toml` | `XRELEASE_*`, `POSTGRES_*`, forge tokens, `VITE_*` |

## HTTPS (UI nginx + cert/key)

TLS terminates on the UI nginx container. Configure in `.env`, then:

```bash
docker compose -f docker-compose.yaml -f docker/compose.tls.yaml up -d
# → https://$XRELEASE_PUBLIC_HOST
```

Guide: [TLS](../docs/operations/tls.md).

## Optional

| Topic | Doc |
|---|---|
| HTTPS (PEM certs) | [tls.md](../docs/operations/tls.md) |
| SSO | [oidc.md](../docs/operations/oidc.md) |
| Apply from CI | [ci-cd.md](../docs/operations/ci-cd.md) |

## Limits

One `serve` poller per database. Do not scale the backend replica count.
