# Kubernetes deployment

Helm chart: PostgreSQL + UI + Apprise + Ingress (same idea as Docker Compose).
Secrets stay in an overlay.

Canonical install notes: [`deploy/k8s/README.md`](../../deploy/k8s/README.md) ·
chart: [`deploy/helm/xrelease/`](../../deploy/helm/xrelease/).

## Quick start (HTTP)

```sh
cp deploy/k8s/values.secrets.example.yaml deploy/k8s/values.secrets.yaml
$EDITOR deploy/k8s/values.secrets.yaml
helm upgrade --install xrelease ./deploy/helm/xrelease \
  --namespace xrelease --create-namespace \
  -f deploy/k8s/values.secrets.yaml
```

Open `http://xrelease.local/` (map the host to the Ingress IP). Sign in with
`secrets.adminUser` / `secrets.adminPassword`.
[Authentication](../operations/authentication.md).

Needs an Ingress controller (default class `nginx` — set `ingress.className`
if yours differs).

## Ingress routing

When `ui.enabled=true` (chart default), the **main Ingress** sends all paths to
the UI Service. UI nginx proxies API and probes to the backend:

| Path | Destination |
|---|---|
| `/` (dashboard) | UI `:80` |
| `/api/…`, `/ready`, `/health`, `/openapi.json`, `/metrics` | UI → backend `:8080` |

Leave `ui.env.VITE_API_URL` **empty** (same-origin `/api`).

```sh
curl -sH "Authorization: Bearer $API_KEY" http://xrelease.local/ready
xrctl --api-url http://xrelease.local --api-key "$API_KEY" status
```

Optional **`ingress.api`**: second Ingress for forge webhooks on another host
(direct to the backend Service). See
[`values-tls.example.yaml`](../../deploy/k8s/values-tls.example.yaml).

## HTTPS (certificate files)

1. Create a TLS Secret from your PEMs:

```bash
kubectl -n xrelease create secret tls xrelease-tls \
  --cert=cert.pem --key=key.pem
```

2. Enable Ingress TLS (edit the host to match the certificate):

```bash
helm upgrade --install xrelease ./deploy/helm/xrelease \
  --namespace xrelease --create-namespace \
  -f deploy/k8s/values.secrets.yaml \
  -f deploy/k8s/values-tls.example.yaml
```

Full checklist: [TLS & Ingress](../operations/tls.md).

```bash
xrctl --api-url https://xrelease.example.com --api-key "$API_KEY" status
```

## Authoring

Chart default = **API + UI** (single-document ledger). Samples:
[authoring variants](../configuration/overview.md#authoring-variants) ·
[`deploy/examples/`](../../deploy/examples/).

## Runtime UI config

| Change | Where | Action |
|---|---|---|
| Auth / OIDC client | `ui.env.VITE_*` | `helm upgrade` (+ optional `values-oidc.example.yaml`) |
| Backend JWT | Secret `XRELEASE_OIDC_*` | Update Secret + restart backend |
| Local admin | `sessionSecret` + admin keys | `values.secrets.yaml` only |

```bash
helm upgrade xrelease ./deploy/helm/xrelease \
  -f deploy/k8s/values.secrets.yaml \
  -f deploy/k8s/values-oidc.example.yaml
```

[OIDC](../operations/oidc.md) · [Helm README](../../deploy/helm/xrelease/README.md).

## What's mounted

| Source | Purpose |
|---|---|
| ConfigMap `bootstrap.toml` + `releases.yaml` | Infra + optional seed |
| Secret | DB URL, API key, session/admin, OIDC, tokens |
| TLS Secret (`xrelease-tls`) | Ingress HTTPS (when using the TLS overlay) |
| `ui.env` | Runtime dashboard (`/ui-config.js`) |

## Next

| Topic | Doc |
|---|---|
| TLS & Ingress | [TLS](../operations/tls.md) |
| OIDC | [OIDC](../operations/oidc.md) |
| GitOps overlays | [`deploy/k8s/README.md`](../../deploy/k8s/README.md) |
| Apply from CI | [CI/CD integration](../operations/ci-cd.md) |
