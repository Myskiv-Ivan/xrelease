# xrelease Helm chart

Images: `ghcr.io/myskiv-ivan/xrelease` and `ghcr.io/myskiv-ivan/xrelease-ui`.

**Defaults = ready operator stack** (same idea as `docker-compose.yaml`):
in-cluster PostgreSQL + UI + Apprise + Ingress + **API + UI** authoring.
Secrets only via overlay — see [`deploy/k8s/README.md`](../../k8s/README.md).

```bash
cp deploy/k8s/values.secrets.example.yaml deploy/k8s/values.secrets.yaml
helm upgrade --install xrelease ./deploy/helm/xrelease \
  --namespace xrelease --create-namespace \
  -f deploy/k8s/values.secrets.yaml
```

## Chart constraints

- **`replicaCount` must be `1`**.
- **`[api].listen = "0.0.0.0:8080"`** in bootstrap.
- Set **either** `secrets.existingSecret` **or** `secrets.*` — not both.
- ConfigMap keys: `bootstrap.toml` + `releases.yaml`.
- Local UI login needs `sessionSecret` + `adminPassword`; `source=api` needs
  `configEncryptionKey`. Empty / `CHANGE_ME*` fail install.
- Empty `image.tag` → `Chart.AppVersion`; default `pullPolicy: Always`.

## Values map (short)

| Key | Default purpose |
|---|---|
| `image` / `ui.image` | GHCR; tag empty → appVersion |
| `postgresql.enabled` | `true` (operators); GitOps turns off |
| `apprise.enabled` | Sidecar |
| `ui.enabled` / `ui.env` | Dashboard; `VITE_*` runtime |
| `ingress` | `xrelease.local` / class `nginx` |
| `secrets.*` | Via `values.secrets.yaml` |

## Optional

| Feature | How |
|---|---|
| TLS | `kubectl create secret tls` + [`values-tls.example.yaml`](../../k8s/values-tls.example.yaml) — [tls.md](../../../docs/operations/tls.md) |
| OIDC | `values-oidc.example.yaml` — [oidc.md](../../../docs/operations/oidc.md) |
| GitOps | `values-gitops.example.yaml` + Secret |
| Grafana | [`deploy/grafana/`](../../grafana/) |
