# Kubernetes deployment (Helm)

Published GHCR images + secrets overlay. Chart:
[`../helm/xrelease/`](../helm/xrelease/). Docker:
[`../../docker/README.md`](../../docker/README.md).

## Files

| File | Role |
|------|------|
| Chart [`values.yaml`](../helm/xrelease/values.yaml) | Ready stack (PG + UI + Ingress + Apprise, GHCR `Always`) |
| [`values.secrets.example.yaml`](values.secrets.example.yaml) | → `values.secrets.yaml` (gitignored) |
| [`values-tls.example.yaml`](values-tls.example.yaml) | Ingress HTTPS from PEM → TLS Secret |
| [`values-gitops.example.yaml`](values-gitops.example.yaml) | Prod: existing ConfigMap + Secret |
| [`values-oidc.example.yaml`](values-oidc.example.yaml) | Optional OIDC UI overlay |
| [`secret.example.yaml`](secret.example.yaml) | External Secret |
| [`secret.multi-team.example.yaml`](secret.multi-team.example.yaml) | Multi-team tokens |
| [`secret.external.example.yaml`](secret.external.example.yaml) | ExternalSecret (Vault / ESO) |

Config samples: [`../examples/`](../examples/).

## Install

Needs Helm 3 + IngressClass `nginx` (override `ingress.className` if needed).

```bash
cp deploy/k8s/values.secrets.example.yaml deploy/k8s/values.secrets.yaml
# fill passwords (openssl rand -hex 32 / openssl rand -base64 32)
helm upgrade --install xrelease ./deploy/helm/xrelease \
  --namespace xrelease --create-namespace \
  -f deploy/k8s/values.secrets.yaml
```

Open `http://xrelease.local/` (add host → ingress IP). Login =
`secrets.adminUser` / `secrets.adminPassword`. Idle until Config → Apply.

## Secrets

| Mode | Files |
|---|---|
| **Default** | `-f values.secrets.yaml` → chart Secret |
| **Prod** | `kubectl apply -f secret.example.yaml` + `values-gitops.example.yaml` |

Do not mix `existingSecret` and `secrets.*`. Default (`source=api`) needs
`configEncryptionKey`. Same keys as [`.env.example`](../../.env.example).

## OIDC

```bash
helm upgrade xrelease ./deploy/helm/xrelease \
  -f deploy/k8s/values.secrets.yaml \
  -f deploy/k8s/values-oidc.example.yaml
```

Plus backend `XRELEASE_OIDC_*` in the Secret. Keep `VITE_API_URL` empty.

## Production (GitOps)

```bash
kubectl create namespace xrelease
kubectl create configmap xrelease-config \
  --namespace xrelease \
  --from-file=bootstrap.toml=./deploy/examples/infra-app/bootstrap.toml \
  --from-file=releases.yaml=./deploy/examples/infra-app/app/releases.yaml
kubectl apply -f deploy/k8s/secret.example.yaml
helm upgrade --install xrelease ./deploy/helm/xrelease \
  --namespace xrelease \
  -f deploy/k8s/values-gitops.example.yaml
```

## Routing & TLS

| URL | Backend |
|---|---|
| `/` | UI |
| `/api/…`, `/ready`, `/health` | UI nginx → `:8080` |

HTTPS with your certificates: create a `kubernetes.io/tls` Secret, then apply
[`values-tls.example.yaml`](values-tls.example.yaml). Operator guide:
[`../../docs/operations/tls.md`](../../docs/operations/tls.md) ·
[`../../docs/getting-started/kubernetes.md`](../../docs/getting-started/kubernetes.md).

[`../helm/xrelease/README.md`](../helm/xrelease/README.md) ·
[CI/CD apply](../../docs/operations/ci-cd.md).
