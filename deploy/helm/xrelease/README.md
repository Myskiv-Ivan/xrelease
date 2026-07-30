# xrelease Helm chart

Images: `ghcr.io/myskiv-ivan/xrelease` · `ghcr.io/myskiv-ivan/xrelease-ui`  
OCI: `oci://ghcr.io/myskiv-ivan/charts/xrelease`

**Defaults:** CloudNativePG + Gateway API + NetworkPolicy + ServiceMonitor + UI×1 + Apprise.  
Site overlay: [`deploy/k8s/values.yaml`](../../k8s/values.yaml) + [`values.secrets.yaml`](../../k8s/values.secrets.example.yaml).

```bash
cp deploy/k8s/values.secrets.example.yaml deploy/k8s/values.secrets.yaml
# edit gateway.hostnames; set adminPassword
helm upgrade --install xrelease ./deploy/helm/xrelease \
  --namespace xrelease --create-namespace \
  -f deploy/k8s/values.yaml \
  -f deploy/k8s/values.secrets.yaml
```

Platform prerequisites: [kubernetes.md](../../../docs/getting-started/kubernetes.md).  
Variants (Ingress/Traefik, no Prometheus, …): [deployment-variants.md](../../../docs/operations/deployment-variants.md).

## Constraints

- `replicaCount` must be `1`
- Either `secrets.existingSecret` **or** `secrets.*` — not both
- Local UI needs `secrets.adminPassword` (other secrets are generated)
- Empty `image.tag` → `Chart.AppVersion`; images are **linux/amd64**

## Generated secrets

`apiKey`, `webhookSecret`, `sessionSecret`, `configEncryptionKey` are generated
on first install and kept on upgrade. ArgoCD / `helm template` must set
`secrets.existingSecret` — [`secret.example.yaml`](../../k8s/secret.example.yaml).

```bash
kubectl -n xrelease get secret xrelease-secrets \
  -o jsonpath='{.data.XRELEASE_API_KEY}' | base64 -d
```

## `/metrics`

Unauthenticated on the backend; not proxied through the UI by default.
Scrape the backend Service or enable `metrics.serviceMonitor` (needs Prometheus Operator CRDs).
