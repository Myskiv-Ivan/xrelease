# Infra / app split — **API** authoring

Single-document install with **`api_config = true`**, `source = "api"`,
`ui_config = false` (CI / `xrctl` apply; dashboard has no Config editor).

| File | Layer | How it changes |
|---|---|---|
| `bootstrap.toml` | Infra | Restart / Helm upgrade |
| `app/releases.yaml` | Desired state | `xrctl apply` / `POST /config/apply` (optional first-boot seed) |
| [`.env.example`](../../../.env.example) | Docker secrets | — |

K8s Secret: [`deploy/k8s/secret.example.yaml`](../../k8s/secret.example.yaml)
(optionally set `XRELEASE_CONFIG_APPLY_SECRET`).

> Not the Docker Compose default (that is **API + UI** + multi-org). This example
> replaces root `bootstrap.toml` for a single-doc **API** profile.

## Docker

From **repository root**:

```bash
cp deploy/examples/infra-app/bootstrap.toml bootstrap.toml
mkdir -p app
cp deploy/examples/infra-app/app/releases.yaml app/releases.yaml
cp .env.example .env   # optional: XRELEASE_CONFIG_APPLY_SECRET for HMAC

# Uncomment in docker-compose.yaml so the seed file is available at boot:
#   environment: XRELEASE_APP_CONFIG: /etc/xrelease/app/releases.yaml
#   volumes:     ./app/releases.yaml:/etc/xrelease/app/releases.yaml:ro

docker compose up -d

# If you did not mount the seed file, push it after the stack is up:
xrctl --api-url http://127.0.0.1:3000 --api-key "$XRELEASE_API_KEY" \
  apply app/releases.yaml --if-match none --label seed
```

## Kubernetes

Fits the Helm chart (single `releases.yaml` mount):

```bash
kubectl create namespace xrelease

kubectl create configmap xrelease-config \
  --namespace xrelease \
  --from-file=bootstrap.toml=deploy/examples/infra-app/bootstrap.toml \
  --from-file=releases.yaml=deploy/examples/infra-app/app/releases.yaml

kubectl apply -f deploy/k8s/secret.example.yaml

# values.local.yaml — own ConfigMap + Secret (chart still uses CNPG + Gateway)
#   config:
#     existingConfigMap: xrelease-config
#   secrets:
#     existingSecret: xrelease-secrets

helm upgrade --install xrelease ./deploy/helm/xrelease \
  --namespace xrelease \
  -f deploy/k8s/values.yaml \
  -f values.local.yaml
```

With `source=api`, the mounted file is a **seed** until the first apply; later
changes go through `xrctl` / CI (UI editor stays off because `ui_config=false`).

## GitOps apply

```bash
xrctl --api-url https://xrelease.example.com --api-key "$XRELEASE_API_KEY" \
  apply app/releases.yaml --if-match none --label "$GIT_SHA"
```

Optional HMAC: `XRELEASE_CONFIG_APPLY_SECRET` + header `X-Config-Signature: sha256=…`.

Infra changes still require restart / Helm upgrade.

See [`deploy/examples/README.md`](../README.md).
