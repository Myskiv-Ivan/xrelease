# Multi-team routing — **Local** authoring

Per-team eXpress bots, Apprise tags, and webhooks. Desired state lives in
`app/releases.yaml` (**Local** / GitOps).

| File | Purpose |
|---|---|
| `bootstrap.toml` | Infra (**Local**) |
| `app/releases.yaml` | Teams, notifiers, sources |
| [`.env.example`](../../../.env.example) | Docker secrets (shared) |

K8s Secret: [`deploy/k8s/secret.multi-team.example.yaml`](../../k8s/secret.multi-team.example.yaml)

## Routing model

| Source `routing_tag` | eXpress | Apprise | Webhook |
|---|---|---|---|
| `platform-team` | `express-platform` | tag-routed channel | — |
| `security-team` | `express-security` | tag-routed channel | `n8n-security` |

Per-team secrets use `access_token_env` in `app/releases.yaml` → Bearer values in
`.env` (Docker) or K8s Secret only.

---

## Docker

Default Compose does **not** mount `app/releases.yaml`. For this **Local**
example, copy files and enable the app volume:

```bash
cp deploy/examples/multi-team/bootstrap.toml bootstrap.toml
mkdir -p app
cp deploy/examples/multi-team/app/releases.yaml app/releases.yaml
cp .env.example .env   # set XRELEASE_EXPRESS_TOKEN_PLATFORM / _SECURITY
```

In [`docker-compose.yaml`](../../../docker-compose.yaml), uncomment:

```yaml
# XRELEASE_APP_CONFIG: /etc/xrelease/app/releases.yaml
# - ./app/releases.yaml:/etc/xrelease/app/releases.yaml:ro
```

```bash
docker compose up -d
# Optional: validate after the stack is up
docker compose exec xrelease xrelease --config /config/bootstrap.toml \
  --app /etc/xrelease/app/releases.yaml validate --strict
```

Register Apprise channels (lab):

```bash
curl -X POST http://127.0.0.1:8000/add/release-channels \
  -d "urls=tgram://BOT/CHAT&tag=platform-team"
```

---

## Kubernetes

```bash
kubectl create namespace xrelease

kubectl create configmap xrelease-config \
  --namespace xrelease \
  --from-file=bootstrap.toml=deploy/examples/multi-team/bootstrap.toml \
  --from-file=releases.yaml=deploy/examples/multi-team/app/releases.yaml

kubectl apply -f deploy/k8s/secret.multi-team.example.yaml

helm upgrade --install xrelease ./deploy/helm/xrelease \
  --namespace xrelease \
  -f deploy/k8s/values-gitops.example.yaml
```

Default chart install is **API + UI**. For this **Local** example use
`values-gitops.example.yaml` (above) or override `config.bootstrapInline` /
`config.appInline` (or `config.existingConfigMap`) plus secrets so
`source=local` matches this example.

---

## Add a third team

1. PR to `app/releases.yaml` — team + notifier with `access_token_env`.
2. Add env var to `.env` or extend the K8s Secret.
3. `validate --strict` → merge → reload / rollout.

Full env · UI · YAML matrix:
[docs/configuration/apprise.md](../../../docs/configuration/apprise.md).

See [`deploy/examples/README.md`](../README.md).
