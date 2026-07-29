# Kubernetes

Default deploy is the **chart itself** (CNPG + Gateway). This folder holds only
the site overlay and platform objects.

| File | Role |
|------|------|
| [`values.yaml`](values.yaml) | Hostname + pinned `image.tag` |
| [`values.secrets.example.yaml`](values.secrets.example.yaml) | Copy → `values.secrets.yaml` — dashboard password |
| [`gateway/`](gateway/) | Platform `Gateway` (outside the Helm release) |
| [`secret.example.yaml`](secret.example.yaml) | Optional `secrets.existingSecret` (GitOps / ArgoCD) |
| Chart [`../helm/xrelease/`](../helm/xrelease/) | Templates + full value reference |

```bash
cp deploy/k8s/values.secrets.example.yaml deploy/k8s/values.secrets.yaml
# edit gateway.hostnames in values.yaml; set adminPassword in values.secrets.yaml
helm upgrade --install xrelease ./deploy/helm/xrelease \
  --namespace xrelease --create-namespace \
  -f deploy/k8s/values.yaml \
  -f deploy/k8s/values.secrets.yaml
```

Walkthrough: [Kubernetes](../../docs/getting-started/kubernetes.md).
