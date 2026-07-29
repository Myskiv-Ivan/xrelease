# Deployment

**Config in Git, secrets outside Git.** Published images from GHCR.

| | Docker | Kubernetes |
|---|---|---|
| **Command** | `docker compose up -d` | `helm … -f values.yaml -f values.secrets.yaml` |
| **Stack** | UI + PG + Apprise | UI + CloudNativePG + Gateway API |
| **Secrets** | `.env` | `values.secrets.yaml` |
| **Docs** | [`docker/README.md`](../docker/README.md) | [`k8s/README.md`](k8s/README.md) |

```
repo root
├── docker-compose.yaml
└── deploy/
    ├── helm/xrelease/                  ← chart (defaults = deploy target)
    ├── k8s/values.yaml                 ← hostname + image.tag
    ├── k8s/values.secrets.example.yaml ← copy → values.secrets.yaml
    └── k8s/gateway/                    ← platform Gateway
```

```bash
cp .env.example .env && docker compose up -d

TAG=v0.1.4
curl -fsSLO "https://raw.githubusercontent.com/Myskiv-Ivan/xrelease/${TAG}/deploy/k8s/values.yaml"
curl -fsSLO "https://raw.githubusercontent.com/Myskiv-Ivan/xrelease/${TAG}/deploy/k8s/values.secrets.example.yaml"
cp values.secrets.example.yaml values.secrets.yaml
# edit hostname + password
helm upgrade --install xrelease oci://ghcr.io/myskiv-ivan/charts/xrelease \
  --version "${TAG#v}" \
  --namespace xrelease --create-namespace \
  -f values.yaml \
  -f values.secrets.yaml
```

[Docker](../docs/getting-started/docker.md) ·
[Kubernetes](../docs/getting-started/kubernetes.md) ·
[variants](../docs/operations/deployment-variants.md).
