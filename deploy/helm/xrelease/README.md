# xrelease Helm chart

Images: `ghcr.io/myskiv-ivan/xrelease` · `ghcr.io/myskiv-ivan/xrelease-ui`  
OCI chart: `oci://ghcr.io/myskiv-ivan/charts/xrelease`

**Defaults = deploy target:** CloudNativePG + Gateway API + NetworkPolicy +
ServiceMonitor + UI×2. Site overlay: [`deploy/k8s/values.yaml`](../../k8s/values.yaml)
(hostname + image tag) + [`values.secrets.yaml`](../../k8s/values.secrets.example.yaml).

```bash
cp deploy/k8s/values.secrets.example.yaml deploy/k8s/values.secrets.yaml
# edit gateway.hostnames in values.yaml; set adminPassword in values.secrets.yaml
helm upgrade --install xrelease ./deploy/helm/xrelease \
  --namespace xrelease --create-namespace \
  -f deploy/k8s/values.yaml \
  -f deploy/k8s/values.secrets.yaml
```

Platform setup: [`docs/getting-started/kubernetes.md`](../../../docs/getting-started/kubernetes.md).

## Constraints

- `replicaCount` must be `1`
- Either `secrets.existingSecret` **or** `secrets.*` — not both
- Local UI needs `secrets.adminPassword` (everything else is generated)
- Empty `image.tag` → `Chart.AppVersion`; images are **linux/amd64** only

## Generated secrets

`apiKey`, `webhookSecret`, `sessionSecret`, `configEncryptionKey` (and builtin
PG password when used) are generated on first install and preserved on upgrade.
Render-only pipelines (ArgoCD / `helm template`) must set `secrets.existingSecret`
— see [`secret.example.yaml`](../../k8s/secret.example.yaml).

```bash
kubectl -n xrelease get secret xrelease-secrets \
  -o jsonpath='{.data.XRELEASE_API_KEY}' | base64 -d
```

## `/metrics`

Unauthenticated on the backend. Not proxied through the UI by default
(`ui.exposeMetrics=false`). Scrape the backend Service or enable
`metrics.serviceMonitor`.

## Values reference

Full key list lives in [`values.yaml`](values.yaml). Optional switches
(Ingress instead of Gateway, external PG, OIDC, TLS):
[deployment variants](../../../docs/operations/deployment-variants.md).
