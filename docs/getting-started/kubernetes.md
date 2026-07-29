# Kubernetes deployment

Chart defaults: **xrelease + CloudNativePG + Gateway API** (+ NetworkPolicy, ServiceMonitor, Apprise, UI×2).

For Traefik Ingress / no Prometheus / custom StorageClass, see
[deployment variants](../operations/deployment-variants.md).

## Install

### Platform (once per cluster)

**Required**

```bash
helm repo add cnpg https://cloudnative-pg.github.io/charts
helm upgrade --install cnpg cnpg/cloudnative-pg \
  --namespace cnpg-system --create-namespace
```

**Front door — pick one**

Gateway API (chart default):

```bash
kubectl apply -f https://github.com/kubernetes-sigs/gateway-api/releases/download/v1.2.1/standard-install.yaml
helm install eg oci://docker.io/envoyproxy/gateway-helm \
  --version v1.2.4 -n envoy-gateway-system --create-namespace
kubectl create namespace xrelease
# edit hostname + gatewayClassName
kubectl apply -f deploy/k8s/gateway/gateway.yaml
```

Or use an existing Ingress controller (e.g. Traefik) and set
`ingress.enabled: true`, `gateway.enabled: false` in the site overlay.

**Optional — Prometheus Operator CRDs** (for `metrics.serviceMonitor`):

```bash
helm repo add prometheus-community https://prometheus-community.github.io/helm-charts
helm upgrade --install prometheus-crds prometheus-community/prometheus-operator-crds \
  --namespace monitoring --create-namespace
```

Without them, set `metrics.serviceMonitor.enabled: false` and
`postgresql.cnpg.monitoring.enablePodMonitor: false`.

### Release

```bash
TAG=v0.2.0
curl -fsSLO "https://raw.githubusercontent.com/Myskiv-Ivan/xrelease/${TAG}/deploy/k8s/values.yaml"
curl -fsSLO "https://raw.githubusercontent.com/Myskiv-Ivan/xrelease/${TAG}/deploy/k8s/values.secrets.example.yaml"
cp values.secrets.example.yaml values.secrets.yaml
# edit gateway.hostnames; set secrets.adminPassword
helm upgrade --install xrelease oci://ghcr.io/myskiv-ivan/charts/xrelease \
  --version "${TAG#v}" \
  --namespace xrelease --create-namespace \
  -f values.yaml \
  -f values.secrets.yaml
```

From a checkout:

```bash
cp deploy/k8s/values.secrets.example.yaml deploy/k8s/values.secrets.yaml
# edit deploy/k8s/values.yaml + values.secrets.yaml
helm upgrade --install xrelease ./deploy/helm/xrelease \
  --namespace xrelease --create-namespace \
  -f deploy/k8s/values.yaml \
  -f deploy/k8s/values.secrets.yaml
```

Password is the one secret you set; API key / webhook / session / encryption
keys are generated:

```bash
kubectl -n xrelease get secret xrelease-secrets \
  -o jsonpath='{.data.XRELEASE_API_KEY}' | base64 -d
```

ArgoCD / `helm template` → `secrets.existingSecret`
([`secret.example.yaml`](../../deploy/k8s/secret.example.yaml)).

Point DNS at the Gateway (or Ingress LB). Sign in with `admin` / your password.
Images are **linux/amd64**.

## What you get

| Layer | Detail |
|---|---|
| App | one poller + UI×2 + Apprise |
| Database | CloudNativePG (`Cluster`, Secret `<name>-app`) |
| Front door | HTTPRoute → UI nginx → `/api` (or Ingress) |
| Hardening | NetworkPolicy, non-root, RO rootfs |
| Observability | ServiceMonitor + CNPG PodMonitor (when CRDs exist) |

`/metrics` is **404** on the public UI — scrape the backend Service.

## TLS

Uncomment the `https` listener in [`gateway.yaml`](../../deploy/k8s/gateway/gateway.yaml),
create the TLS Secret, set `gateway.parentRef.sectionName: https`.
[Gateway API](../operations/gateway.md) · [TLS](../operations/tls.md).

## Backups

Enable `postgresql.cnpg.backup.*` — [CloudNativePG](../operations/cloudnativepg.md).
`helm uninstall` keeps the Cluster (`helm.sh/resource-policy: keep`).

## Next

| Topic | Doc |
|---|---|
| Optional overrides | [Deployment variants](../operations/deployment-variants.md) |
| Apply from CI | [CI/CD](../operations/ci-cd.md) |
