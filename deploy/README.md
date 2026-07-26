# Deployment

**Config in Git, secrets outside Git.** Use **published images** from GHCR.

| | Docker | Kubernetes |
|---|---|---|
| **Command** | `docker compose up -d` | `helm … -f values.secrets.yaml` |
| **URL** | `http://127.0.0.1:3000` | `http://xrelease.local/` |
| **Secrets** | `.env` | `values.secrets.yaml` / `secret*.yaml` |
| **Docs** | [`docker/README.md`](../docker/README.md) | [`k8s/README.md`](k8s/README.md) |

```
repo root
├── docker-compose.yaml                 ← GHCR images
└── deploy/
    ├── helm/xrelease/values.yaml       ← ready K8s stack
    ├── k8s/values.secrets.example.yaml ← → values.secrets.yaml
    └── k8s/values-gitops.example.yaml  ← production GitOps
```

No install scripts. No `helm --set`. OIDC = your IdP — [oidc.md](../docs/operations/oidc.md).

## Quick start

```bash
# Docker (published images)
cp .env.example .env
docker compose up -d

# Kubernetes (published images)
cp deploy/k8s/values.secrets.example.yaml deploy/k8s/values.secrets.yaml
# edit passwords, then:
helm upgrade --install xrelease ./deploy/helm/xrelease \
  --namespace xrelease --create-namespace \
  -f deploy/k8s/values.secrets.yaml
```

Guides: [Docker](../docs/getting-started/docker.md) ·
[Kubernetes](../docs/getting-started/kubernetes.md) ·
[OIDC](../docs/operations/oidc.md) · [TLS](../docs/operations/tls.md) ·
[CI/CD integration](../docs/operations/ci-cd.md).

## Examples

| Example | Use | K8s Secret |
|---------|-----|------------|
| Repo root | Compose multi-org UI lab | — |
| [infra-app](examples/infra-app/) | config_api / apply | `secret.example.yaml` |
| [multi-team](examples/multi-team/) | per-team routing | `secret.multi-team.example.yaml` |
| [multi-org](examples/multi-org/) | per-org files | `secret.example.yaml` |

[`examples/README.md`](examples/README.md).
